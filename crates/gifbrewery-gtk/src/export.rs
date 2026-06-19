use gifbrewery_core::{Clip, CropRect, FrameStrategy, Overlay, Project, RgbaColor, TextOverlay};
use std::fs;
use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{SystemTime, UNIX_EPOCH};

#[derive(Debug, Clone)]
pub struct ExportProgress {
    pub percent: Option<u8>,
    pub message: String,
}

pub fn export_gif(project: &Project, output_path: &Path) -> Result<(), String> {
    export_gif_with_progress(project, output_path, |_| {})
}

pub fn export_gif_with_progress<F>(
    project: &Project,
    output_path: &Path,
    progress: F,
) -> Result<(), String>
where
    F: Fn(ExportProgress),
{
    project.validate().map_err(|err| err.to_string())?;
    let clip = project
        .clips
        .first()
        .ok_or_else(|| "project has no clip".to_string())?;

    let duration = clip.range.duration_seconds().max(0.01);
    let fps = export_fps(project, duration);

    let explicit_size =
        project.settings.gif.output_width.is_some() || project.settings.gif.output_height.is_some();
    let mut auto_scale = 1.0;

    for attempt in 0..5 {
        let geometry = RenderGeometry::from_project(project, clip, auto_scale);
        progress(ExportProgress {
            percent: Some(0),
            message: format!("Export pass {}...", attempt + 1),
        });
        render_gif_once(project, output_path, clip, fps, geometry, &progress)?;
        let size = fs::metadata(output_path)
            .map_err(|err| format!("failed to inspect exported GIF size: {err}"))?
            .len();
        crate::diagnostics::log_line(format_args!(
            "GIF export attempt {}: {} bytes at {}x{}",
            attempt + 1,
            size,
            geometry.output_width,
            geometry.output_height
        ));

        let Some(target_max_bytes) = project.settings.gif.target_max_bytes else {
            progress(ExportProgress {
                percent: Some(100),
                message: "Finalizing GIF loop metadata...".to_string(),
            });
            return ensure_gif_loops_forever(output_path);
        };
        if size <= target_max_bytes || explicit_size {
            progress(ExportProgress {
                percent: Some(100),
                message: "Finalizing GIF loop metadata...".to_string(),
            });
            return ensure_gif_loops_forever(output_path);
        }

        let shrink = ((target_max_bytes as f64 / size as f64).sqrt() * 0.96).clamp(0.25, 0.92);
        auto_scale *= shrink;
        if geometry.output_width <= 160 || geometry.output_height <= 90 {
            progress(ExportProgress {
                percent: Some(100),
                message: "Finalizing GIF loop metadata...".to_string(),
            });
            return ensure_gif_loops_forever(output_path);
        }
    }

    progress(ExportProgress {
        percent: Some(100),
        message: "Finalizing GIF loop metadata...".to_string(),
    });
    ensure_gif_loops_forever(output_path)
}

pub fn render_frame_png(
    project: &Project,
    playhead_seconds: f64,
    output_path: &Path,
) -> Result<(), String> {
    project.validate().map_err(|err| err.to_string())?;
    let clip = project
        .clips
        .first()
        .ok_or_else(|| "project has no clip".to_string())?;
    let source = project
        .source
        .as_ref()
        .ok_or_else(|| "project has no source media".to_string())?;
    let duration = clip.range.duration_seconds().max(0.01);
    let fps = export_fps(project, duration);
    let mut frame_clip = clip.clone();
    frame_clip.range.start_seconds = playhead_seconds.max(0.0);
    frame_clip.range.end_seconds = frame_clip.range.start_seconds + (1.0 / f64::from(fps));
    let geometry = RenderGeometry::from_project(project, &frame_clip, 1.0);
    let text_dir = drawtext_temp_dir()?;
    let video_filter = video_filter(project, &frame_clip, fps, geometry, &text_dir)?;

    let result = run_ffmpeg_command(
        Command::new("ffmpeg")
            .arg("-hide_banner")
            .arg("-y")
            .arg("-ss")
            .arg(format!("{:.6}", frame_clip.range.start_seconds))
            .arg("-i")
            .arg(&source.path)
            .arg("-frames:v")
            .arg("1")
            .arg("-filter_complex")
            .arg(video_filter)
            .arg("-update")
            .arg("1")
            .arg(output_path),
        0.01,
        None,
    );
    let _ = fs::remove_dir_all(&text_dir);
    result
}

#[derive(Debug, Clone)]
pub struct RenderedFrameSequence {
    pub fps: u32,
    pub duration_seconds: f64,
    pub frames: Vec<PathBuf>,
}

pub fn render_frame_sequence(
    project: &Project,
    output_dir: &Path,
) -> Result<RenderedFrameSequence, String> {
    project.validate().map_err(|err| err.to_string())?;
    let clip = project
        .clips
        .first()
        .ok_or_else(|| "project has no clip".to_string())?;
    let source = project
        .source
        .as_ref()
        .ok_or_else(|| "project has no source media".to_string())?;
    let duration = clip.range.duration_seconds().max(0.01);
    let fps = export_fps(project, duration);
    let geometry = RenderGeometry::from_project(project, clip, 1.0);
    let text_dir = drawtext_temp_dir()?;
    let video_filter = video_filter(project, clip, fps, geometry, &text_dir)?;
    fs::create_dir_all(output_dir).map_err(|err| {
        format!(
            "failed to create rendered frame sequence directory {}: {err}",
            output_dir.display()
        )
    })?;

    let output_pattern = output_dir.join("frame-%06d.png");
    let result = run_ffmpeg_command(
        Command::new("ffmpeg")
            .arg("-hide_banner")
            .arg("-y")
            .arg("-nostats")
            .arg("-progress")
            .arg("pipe:2")
            .arg("-ss")
            .arg(format!("{:.3}", clip.range.start_seconds))
            .arg("-t")
            .arg(format!("{duration:.3}"))
            .arg("-i")
            .arg(&source.path)
            .arg("-filter_complex")
            .arg(video_filter)
            .arg("-start_number")
            .arg("0")
            .arg(output_pattern),
        duration,
        None,
    );
    let _ = fs::remove_dir_all(&text_dir);
    result?;

    let mut frames = fs::read_dir(output_dir)
        .map_err(|err| {
            format!(
                "failed to read rendered frame sequence directory {}: {err}",
                output_dir.display()
            )
        })?
        .filter_map(|entry| entry.ok().map(|entry| entry.path()))
        .filter(|path| {
            path.file_name()
                .and_then(|name| name.to_str())
                .is_some_and(|name| name.starts_with("frame-") && name.ends_with(".png"))
        })
        .collect::<Vec<_>>();
    frames.sort();

    if frames.is_empty() {
        return Err(format!(
            "rendered frame sequence produced no frames in {}",
            output_dir.display()
        ));
    }

    Ok(RenderedFrameSequence {
        fps,
        duration_seconds: duration,
        frames,
    })
}

fn render_gif_once(
    project: &Project,
    output_path: &Path,
    clip: &Clip,
    fps: u32,
    geometry: RenderGeometry,
    progress: &dyn Fn(ExportProgress),
) -> Result<(), String> {
    let duration = clip.range.duration_seconds().max(0.01);
    let source = project
        .source
        .as_ref()
        .ok_or_else(|| "project has no source media".to_string())?;
    let text_dir = drawtext_temp_dir()?;
    let video_filter = video_filter(project, clip, fps, geometry, &text_dir)?;
    let palette_filter = format!(
        "{video_filter},split[gifbrewery_palette_src][gifbrewery_frames];\
         [gifbrewery_palette_src]palettegen=max_colors={}:stats_mode=full[gifbrewery_palette];\
         [gifbrewery_frames][gifbrewery_palette]paletteuse=dither=sierra2_4a",
        project.settings.gif.colors.clamp(256, 256)
    );

    let result = run_ffmpeg_command(
        Command::new("ffmpeg")
            .arg("-hide_banner")
            .arg("-y")
            .arg("-nostats")
            .arg("-progress")
            .arg("pipe:2")
            .arg("-ss")
            .arg(format!("{:.3}", clip.range.start_seconds))
            .arg("-t")
            .arg(format!("{duration:.3}"))
            .arg("-i")
            .arg(&source.path)
            .arg("-filter_complex")
            .arg(palette_filter)
            .arg("-loop")
            .arg("0")
            .arg(output_path),
        duration,
        Some(progress),
    );
    let _ = fs::remove_dir_all(&text_dir);

    result
}

fn run_ffmpeg_command(
    command: &mut Command,
    duration_seconds: f64,
    progress: Option<&dyn Fn(ExportProgress)>,
) -> Result<(), String> {
    command
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::piped());
    let mut child = command
        .spawn()
        .map_err(|err| format!("failed to run ffmpeg: {err}"))?;
    let stderr = child
        .stderr
        .take()
        .ok_or_else(|| "failed to capture ffmpeg progress output".to_string())?;
    let reader = BufReader::new(stderr);
    let mut stderr_tail = Vec::new();
    let mut last_percent = None;

    for line in reader.lines() {
        let Ok(line) = line else {
            continue;
        };
        if line.starts_with("out_time_ms=") {
            if let Some(callback) = progress {
                if let Some(micros) = line
                    .strip_prefix("out_time_ms=")
                    .and_then(|value| value.trim().parse::<f64>().ok())
                {
                    let percent = ((micros / 1_000_000.0) / duration_seconds.max(0.01) * 100.0)
                        .round()
                        .clamp(0.0, 99.0) as u8;
                    if last_percent != Some(percent) {
                        last_percent = Some(percent);
                        callback(ExportProgress {
                            percent: Some(percent),
                            message: format!("Exporting GIF... {percent}%"),
                        });
                    }
                }
            }
        } else if line == "progress=end" {
            if let Some(callback) = progress {
                callback(ExportProgress {
                    percent: Some(100),
                    message: "Exporting GIF... 100%".to_string(),
                });
            }
        } else if !line.trim().is_empty() {
            stderr_tail.push(line);
            if stderr_tail.len() > 12 {
                stderr_tail.remove(0);
            }
        }
    }

    let status = child
        .wait()
        .map_err(|err| format!("failed waiting for ffmpeg: {err}"))?;
    if status.success() {
        Ok(())
    } else if stderr_tail.is_empty() {
        Err(format!("ffmpeg export failed with status {status}"))
    } else {
        Err(format!(
            "ffmpeg export failed with status {status}: {}",
            stderr_tail.join(" | ")
        ))
    }
}

fn drawtext_temp_dir() -> Result<PathBuf, String> {
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|err| format!("system clock error while preparing text overlays: {err}"))?;
    let path = std::env::temp_dir().join(format!(
        "gifbrewery-drawtext-{}-{}",
        std::process::id(),
        now.as_nanos()
    ));
    fs::create_dir_all(&path).map_err(|err| {
        format!(
            "failed to create drawtext temp dir {}: {err}",
            path.display()
        )
    })?;
    Ok(path)
}

fn export_fps(project: &Project, duration: f64) -> u32 {
    if let Some(source_fps) = project
        .source
        .as_ref()
        .and_then(|source| source.fps)
        .filter(|fps| *fps > 0.0)
    {
        return source_fps.round().clamp(1.0, 120.0) as u32;
    }

    let Some(clip) = project.clips.first() else {
        return 30;
    };
    match clip.frame_strategy {
        FrameStrategy::Fps(fps) => fps.clamp(1, 120),
        FrameStrategy::Count(count) => ((f64::from(count) / duration).round() as u32).clamp(1, 120),
        FrameStrategy::DelayMillis(delay) => {
            if delay == 0 {
                30
            } else {
                (1000 / delay).clamp(1, 120)
            }
        }
    }
}

#[derive(Debug, Clone, Copy)]
struct RenderGeometry {
    source_height: u32,
    crop_width: f64,
    crop_height: f64,
    output_width: u32,
    output_height: u32,
}

impl RenderGeometry {
    fn from_project(project: &Project, clip: &Clip, auto_scale: f64) -> Self {
        let source_width = project
            .source
            .as_ref()
            .and_then(|source| source.natural_width)
            .unwrap_or(960)
            .max(1);
        let source_height = project
            .source
            .as_ref()
            .and_then(|source| source.natural_height)
            .unwrap_or(540)
            .max(1);
        let crop = normalized_crop(clip.crop);
        let crop_width = f64::from(source_width) * (1.0 - crop.left - crop.right).max(0.01);
        let crop_height = f64::from(source_height) * (1.0 - crop.top - crop.bottom).max(0.01);
        let output_width = project
            .settings
            .gif
            .output_width
            .filter(|width| *width > 0)
            .unwrap_or_else(|| (crop_width * auto_scale).round() as u32)
            .max(1);
        let output_height = project
            .settings
            .gif
            .output_height
            .filter(|height| *height > 0)
            .unwrap_or_else(|| (crop_height * auto_scale).round() as u32)
            .max(1);

        Self {
            source_height,
            crop_width,
            crop_height,
            output_width,
            output_height,
        }
    }

    fn text_scale(self) -> f64 {
        f64::from(self.output_height) / self.crop_height.max(1.0)
    }
}

fn normalized_crop(crop: Option<CropRect>) -> CropRect {
    let Some(crop) = crop else {
        return CropRect {
            left: 0.0,
            right: 0.0,
            top: 0.0,
            bottom: 0.0,
        };
    };
    let left = crop.left.clamp(0.0, 0.95);
    let right = crop.right.clamp(0.0, 0.95);
    let top = crop.top.clamp(0.0, 0.95);
    let bottom = crop.bottom.clamp(0.0, 0.95);
    let horizontal_total = (left + right).max(1.0);
    let vertical_total = (top + bottom).max(1.0);
    CropRect {
        left: if left + right >= 0.98 {
            left / horizontal_total * 0.98
        } else {
            left
        },
        right: if left + right >= 0.98 {
            right / horizontal_total * 0.98
        } else {
            right
        },
        top: if top + bottom >= 0.98 {
            top / vertical_total * 0.98
        } else {
            top
        },
        bottom: if top + bottom >= 0.98 {
            bottom / vertical_total * 0.98
        } else {
            bottom
        },
    }
}

fn video_filter(
    project: &Project,
    clip: &Clip,
    fps: u32,
    geometry: RenderGeometry,
    text_dir: &Path,
) -> Result<String, String> {
    crate::diagnostics::log_line(format_args!(
        "export geometry: source_height={} crop={:.1}x{:.1} output={}x{} text_scale={:.4}",
        geometry.source_height,
        geometry.crop_width,
        geometry.crop_height,
        geometry.output_width,
        geometry.output_height,
        geometry.text_scale()
    ));
    let mut filters = vec![format!("fps={fps}")];
    if let Some(crop) = clip.crop.map(|crop| normalized_crop(Some(crop))) {
        if crop.left > 0.0 || crop.right > 0.0 || crop.top > 0.0 || crop.bottom > 0.0 {
            filters.push(format!(
                "crop=w='iw*{width:.6}':h='ih*{height:.6}':x='iw*{left:.6}':y='ih*{top:.6}'",
                width = (1.0 - crop.left - crop.right).max(0.01),
                height = (1.0 - crop.top - crop.bottom).max(0.01),
                left = crop.left,
                top = crop.top
            ));
        }
    }
    if f64::from(geometry.output_width) != geometry.crop_width.round()
        || f64::from(geometry.output_height) != geometry.crop_height.round()
    {
        filters.push(format!(
            "scale={}:{}:flags=lanczos",
            geometry.output_width, geometry.output_height
        ));
    }

    for overlay in &project.overlays {
        match overlay {
            Overlay::Text(text) => {
                let text_file = drawtext_file(text_dir, text)?;
                filters.push(drawtext_filter(
                    text,
                    clip.range.start_seconds,
                    geometry,
                    &text_file,
                ))
            }
        }
    }

    Ok(filters.join(","))
}

fn drawtext_file(text_dir: &Path, text: &TextOverlay) -> Result<PathBuf, String> {
    let safe_id: String = text
        .id
        .chars()
        .map(|ch| {
            if ch.is_ascii_alphanumeric() || ch == '-' || ch == '_' {
                ch
            } else {
                '_'
            }
        })
        .collect();
    let path = text_dir.join(format!("{safe_id}.txt"));
    fs::write(&path, text.text.replace('\r', "")).map_err(|err| {
        format!(
            "failed to write drawtext temp file {}: {err}",
            path.display()
        )
    })?;
    Ok(path)
}

fn drawtext_filter(
    text: &TextOverlay,
    clip_start_seconds: f64,
    geometry: RenderGeometry,
    text_file: &Path,
) -> String {
    let escaped_text_file = escape_drawtext(&text_file.display().to_string());
    let font_argument = drawtext_font_argument(&text.font_family, text.font_weight);
    let x = format!("w*{:.6}", text.bounds.x);
    let y = format!("h*{:.6}", text.bounds.y);
    let text_scale = geometry.text_scale();
    let font_size = (text.font_size * text_scale).max(1.0);
    let stroke_width = (text.stroke_width.max(0.0) * text_scale).round();
    let enable_start = text.range.start_seconds - clip_start_seconds;
    let enable_end = text.range.end_seconds - clip_start_seconds;
    let enable = format!("between(t\\,{enable_start:.3}\\,{enable_end:.3})");

    crate::diagnostics::log_line(format_args!(
        "export text overlay: id={} font_family={} font_weight={} configured_font_size={:.2} rendered_font_size={:.2} configured_stroke={:.2} rendered_stroke={:.2} bounds=({:.4},{:.4},{:.4},{:.4}) range={:.3}-{:.3}",
        text.id,
        text.font_family,
        text.font_weight,
        text.font_size,
        font_size,
        text.stroke_width,
        stroke_width,
        text.bounds.x,
        text.bounds.y,
        text.bounds.width,
        text.bounds.height,
        text.range.start_seconds,
        text.range.end_seconds
    ));

    format!(
        "drawtext=textfile='{escaped_text_file}':{font_argument}:fontsize={font_size:.0}:\
         fontcolor={text_color}:bordercolor={stroke_color}:borderw={stroke_width:.0}:\
         x='{x}':y='{y}':enable='{enable}'",
        font_size = font_size,
        text_color = ffmpeg_color(text.text_color),
        stroke_color = ffmpeg_color(text.stroke_color),
        stroke_width = stroke_width,
    )
}

fn drawtext_font_argument(font_family: &str, font_weight: u32) -> String {
    if let Some(font_file) = font_file_for_family(font_family, font_weight) {
        crate::diagnostics::log_line(format_args!(
            "export font resolved: family={} weight={} file={}",
            font_family,
            font_weight,
            font_file.display()
        ));
        format!(
            "fontfile='{}'",
            escape_drawtext(&font_file.display().to_string())
        )
    } else {
        format!("font='{}'", escape_drawtext(font_family))
    }
}

fn font_file_for_family(font_family: &str, font_weight: u32) -> Option<PathBuf> {
    let pattern = if font_weight >= 600 {
        format!("{font_family}:style=Bold")
    } else {
        font_family.to_string()
    };
    let output = Command::new("fc-match")
        .arg("--format=%{file}")
        .arg(pattern)
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let path = String::from_utf8(output.stdout).ok()?;
    let path = path.trim();
    if path.is_empty() {
        None
    } else {
        Some(PathBuf::from(path))
    }
}

fn ffmpeg_color(color: RgbaColor) -> String {
    let to_channel = |value: f64| (value.clamp(0.0, 1.0) * 255.0).round() as u8;
    let alpha = color.alpha.clamp(0.0, 1.0);
    format!(
        "#{:02x}{:02x}{:02x}@{alpha:.3}",
        to_channel(color.red),
        to_channel(color.green),
        to_channel(color.blue)
    )
}

fn escape_drawtext(input: &str) -> String {
    input
        .replace('\\', "\\\\")
        .replace(':', "\\:")
        .replace('\'', "\\'")
        .replace('\n', "\\\\n")
        .replace('\r', "")
        .replace(',', "\\,")
        .replace('[', "\\[")
        .replace(']', "\\]")
}

fn ensure_gif_loops_forever(path: &Path) -> Result<(), String> {
    let bytes = fs::read(path).map_err(|err| {
        format!(
            "failed to verify GIF loop metadata for {}: {err}",
            path.display()
        )
    })?;
    match gif_loop_count(&bytes) {
        Some(0) => Ok(()),
        Some(count) => Err(format!(
            "GIF export did not loop forever: Netscape loop count was {count}"
        )),
        None => {
            Err("GIF export did not include an infinite-loop application extension".to_string())
        }
    }
}

fn gif_loop_count(bytes: &[u8]) -> Option<u16> {
    const NETSCAPE: &[u8] = b"NETSCAPE2.0";
    const ANIMEXTS: &[u8] = b"ANIMEXTS1.0";

    let mut index = 0;
    while index + 19 <= bytes.len() {
        let is_application_extension =
            bytes[index] == 0x21 && bytes[index + 1] == 0xff && bytes[index + 2] == 0x0b;
        if !is_application_extension {
            index += 1;
            continue;
        }

        let identifier = &bytes[index + 3..index + 14];
        let is_loop_extension = identifier == NETSCAPE || identifier == ANIMEXTS;
        if is_loop_extension
            && bytes.get(index + 14) == Some(&0x03)
            && bytes.get(index + 15) == Some(&0x01)
        {
            let low = *bytes.get(index + 16)?;
            let high = *bytes.get(index + 17)?;
            return Some(u16::from_le_bytes([low, high]));
        }

        index += 1;
    }

    None
}

#[cfg(test)]
mod tests {
    use super::gif_loop_count;

    fn gif_with_loop_count(count: u16) -> Vec<u8> {
        let [low, high] = count.to_le_bytes();
        [
            b"GIF89a".as_slice(),
            &[0x21, 0xff, 0x0b],
            b"NETSCAPE2.0".as_slice(),
            &[0x03, 0x01, low, high, 0x00],
        ]
        .concat()
    }

    #[test]
    fn reads_infinite_loop_count() {
        assert_eq!(gif_loop_count(&gif_with_loop_count(0)), Some(0));
    }

    #[test]
    fn reads_finite_loop_count() {
        assert_eq!(gif_loop_count(&gif_with_loop_count(3)), Some(3));
    }

    #[test]
    fn missing_loop_extension_is_none() {
        assert_eq!(gif_loop_count(b"GIF89a"), None);
    }
}
