use gifbrewery_core::{Clip, CropRect, Overlay, Project, RgbaColor, TextAlignment, TextOverlay};
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

    let fps = export_fps(project)?;

    let geometry = RenderGeometry::from_project(project, clip, 1.0);
    let target_max_bytes = project
        .settings
        .gif
        .target_max_bytes
        .filter(|bytes| *bytes > 0)
        .or_else(|| uses_frame_optimized_hdr_encoder(project).then_some(16 * 1024 * 1024));
    let attempts = render_attempts(project);
    let mut final_size = 0;
    let mut final_attempt = None;

    progress(ExportProgress {
        percent: Some(0),
        message: if let Some(target) = target_max_bytes {
            format!(
                "Exporting GIF with {:.1} MB target...",
                target as f64 / 1024.0 / 1024.0
            )
        } else {
            "Exporting maximum-quality GIF...".to_string()
        },
    });

    for (attempt_index, attempt) in attempts.iter().enumerate() {
        let mut attempt_project = project.clone();
        attempt_project.settings.gif.colors = attempt.colors;
        attempt_project.settings.gif.high_quality_quantization = attempt.high_quality_quantization;
        let attempt_clip = attempt_project
            .clips
            .first()
            .ok_or_else(|| "project has no clip".to_string())?;

        crate::diagnostics::log_line(format_args!(
            "GIF export attempt {}: colors={} high_quality_palette={} target={:?}",
            attempt_index + 1,
            attempt.colors,
            attempt.high_quality_quantization,
            target_max_bytes
        ));
        let effective_colors = render_gif_once(
            &attempt_project,
            output_path,
            attempt_clip,
            fps,
            geometry,
            &progress,
        )?;
        let actual_attempt = RenderAttempt {
            colors: effective_colors,
            high_quality_quantization: attempt.high_quality_quantization,
        };
        let size = fs::metadata(output_path)
            .map_err(|err| format!("failed to inspect exported GIF size: {err}"))?
            .len();
        crate::diagnostics::log_line(format_args!(
            "GIF export attempt {} complete: {} bytes at {}x{} colors={} high_quality_palette={}",
            attempt_index + 1,
            size,
            geometry.output_width,
            geometry.output_height,
            actual_attempt.colors,
            actual_attempt.high_quality_quantization
        ));
        final_size = size;
        final_attempt = Some(actual_attempt);

        if target_max_bytes.is_none_or(|target| size <= target) {
            break;
        }
    }

    if let (Some(target), Some(attempt)) = (target_max_bytes, final_attempt) {
        if final_size > target {
            crate::diagnostics::log_line(format_args!(
                "GIF export target not met: final_size={} target={} colors={} high_quality_palette={}",
                final_size,
                target,
                attempt.colors,
                attempt.high_quality_quantization
            ));
        }
    }

    crate::diagnostics::log_line(format_args!(
        "GIF export complete: {} bytes at {}x{} timing={} colors={} high_quality_palette={} target={:?}",
        final_size,
        geometry.output_width,
        geometry.output_height,
        if source_is_gif(project) {
            "source".to_string()
        } else {
            format!("{fps}fps")
        },
        final_attempt.map(|attempt| attempt.colors).unwrap_or(project.settings.gif.colors),
        final_attempt
            .map(|attempt| attempt.high_quality_quantization)
            .unwrap_or(project.settings.gif.high_quality_quantization),
        target_max_bytes
    ));
    progress(ExportProgress {
        percent: Some(100),
        message: "Finalizing GIF loop metadata...".to_string(),
    });
    ensure_gif_loops_forever(output_path)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct RenderAttempt {
    colors: u16,
    high_quality_quantization: bool,
}

fn render_attempts(project: &Project) -> Vec<RenderAttempt> {
    let settings = &project.settings.gif;
    let mut attempts = Vec::new();

    let mut push_attempt = |colors: u16, high_quality_quantization: bool| {
        let attempt = RenderAttempt {
            colors: colors.clamp(2, 256),
            high_quality_quantization,
        };
        if !attempts.contains(&attempt) {
            attempts.push(attempt);
        }
    };

    let requested_colors = settings.colors.clamp(2, 256);
    let target_enabled = settings.target_max_bytes.is_some();
    let start_high_quality = settings.high_quality_quantization && !settings.optimize;
    push_attempt(requested_colors, start_high_quality);

    if target_enabled || settings.optimize {
        push_attempt(requested_colors, false);
    }

    if target_enabled && !settings.optimize {
        for colors in [192, 128] {
            if colors < requested_colors {
                push_attempt(colors, false);
            }
        }
    }

    attempts
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
    let fps = export_fps(project)?;
    let mut frame_clip = clip.clone();
    frame_clip.range.start_seconds = playhead_seconds.max(0.0);
    frame_clip.range.end_seconds = frame_clip.range.start_seconds + (1.0 / f64::from(fps));
    let geometry = RenderGeometry::from_project(project, &frame_clip, 1.0);
    let text_dir = drawtext_temp_dir()?;
    let video_filter = video_filter(project, &frame_clip, Some(fps), geometry, &text_dir)?;

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
    let fps = export_fps(project)?;
    let geometry = RenderGeometry::from_project(project, clip, 1.0);
    let text_dir = drawtext_temp_dir()?;
    let video_filter = video_filter(project, clip, Some(fps), geometry, &text_dir)?;
    fs::create_dir_all(output_dir).map_err(|err| {
        format!(
            "failed to create rendered frame sequence directory {}: {err}",
            output_dir.display()
        )
    })?;

    let output_pattern = output_dir.join("frame-%06d.jpg");
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
            .arg("-q:v")
            .arg("5")
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
                .is_some_and(|name| name.starts_with("frame-") && name.ends_with(".jpg"))
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
) -> Result<u16, String> {
    let duration = clip.range.duration_seconds().max(0.01);
    let source = project
        .source
        .as_ref()
        .ok_or_else(|| "project has no source media".to_string())?;
    let text_dir = drawtext_temp_dir()?;
    let video_filter = video_filter(
        project,
        clip,
        (!source_is_gif(project)).then_some(fps),
        geometry,
        &text_dir,
    )?;

    let frame_optimized_hdr = uses_frame_optimized_hdr_encoder(project);
    crate::diagnostics::log_line(format_args!(
        "GIF encoder path: {}",
        if frame_optimized_hdr {
            "frame-optimized (HDR)"
        } else {
            "adaptive palette"
        }
    ));

    if frame_optimized_hdr {
        let result = render_frame_optimized_hdr_gif(
            project,
            output_path,
            clip,
            fps,
            duration,
            &video_filter,
            progress,
        );
        let _ = fs::remove_dir_all(&text_dir);
        return result;
    }

    let palette_filter = palette_filter(project, &video_filter);
    let mut command = Command::new("ffmpeg");
    command
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
        .arg(palette_filter);
    command.arg("-loop").arg("0").arg(output_path);

    let result = run_ffmpeg_command(&mut command, duration, Some(progress));
    let _ = fs::remove_dir_all(&text_dir);

    result.map(|()| project.settings.gif.colors.clamp(2, 256))
}

fn render_frame_optimized_hdr_gif(
    project: &Project,
    output_path: &Path,
    clip: &Clip,
    fps: u32,
    duration: f64,
    video_filter: &str,
    progress: &dyn Fn(ExportProgress),
) -> Result<u16, String> {
    let source = project
        .source
        .as_ref()
        .ok_or_else(|| "project has no source media".to_string())?;
    let frame_dir = frame_sequence_temp_dir("gifbrewery-hdr-export")?;
    let frame_pattern = frame_dir.join("frame-%06d.png");

    progress(ExportProgress {
        percent: Some(0),
        message: "Rendering color-corrected GIF frames...".to_string(),
    });
    let render_result = run_ffmpeg_command(
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
            .arg(&frame_pattern),
        duration,
        Some(progress),
    );
    if let Err(err) = render_result {
        let _ = fs::remove_dir_all(&frame_dir);
        return Err(err);
    }

    let mut frames = fs::read_dir(&frame_dir)
        .map_err(|err| {
            format!(
                "failed to read HDR export frames in {}: {err}",
                frame_dir.display()
            )
        })?
        .filter_map(|entry| entry.ok().map(|entry| entry.path()))
        .filter(|path| path.extension().is_some_and(|extension| extension == "png"))
        .collect::<Vec<_>>();
    frames.sort();
    if frames.is_empty() {
        let _ = fs::remove_dir_all(&frame_dir);
        return Err("HDR export produced no frames".to_string());
    }

    let image_magick = image_magick_command().ok_or_else(|| {
        "optimized HDR export requires ImageMagick (`magick` or `convert`)".to_string()
    })?;
    let target_max_bytes = project
        .settings
        .gif
        .target_max_bytes
        .filter(|bytes| *bytes > 0)
        .unwrap_or(16 * 1024 * 1024);
    crate::diagnostics::log_line(format_args!(
        "HDR frame optimizer: frames={} fps={} requested_colors={} target={} imagemagick={}",
        frames.len(),
        fps,
        project.settings.gif.colors,
        target_max_bytes,
        image_magick
    ));

    let mut effective_colors = project.settings.gif.colors.clamp(2, 256);
    let mut final_size = 0;
    for colors in hdr_palette_attempts(effective_colors) {
        progress(ExportProgress {
            percent: Some(99),
            message: format!("Optimizing GIF with {colors} colors..."),
        });
        assemble_frame_optimized_gif(image_magick, &frames, fps, colors, output_path)?;
        optimize_gif_with_gifsicle(output_path, colors);
        final_size = fs::metadata(output_path)
            .map_err(|err| format!("failed to inspect optimized HDR GIF: {err}"))?
            .len();
        effective_colors = colors;
        crate::diagnostics::log_line(format_args!(
            "HDR palette attempt complete: colors={} size={} target={}",
            colors, final_size, target_max_bytes
        ));
        if final_size <= target_max_bytes {
            break;
        }
    }

    if final_size > target_max_bytes {
        crate::diagnostics::log_line(format_args!(
            "HDR palette target not met: colors={} size={} target={}",
            effective_colors, final_size, target_max_bytes
        ));
    }
    let _ = fs::remove_dir_all(&frame_dir);
    Ok(effective_colors)
}

fn hdr_palette_attempts(requested_colors: u16) -> Vec<u16> {
    let requested_colors = requested_colors.clamp(2, 256);
    let mut attempts = vec![requested_colors];
    for colors in [192, 128, 96, 64, 48, 32, 24, 16] {
        if colors < requested_colors && !attempts.contains(&colors) {
            attempts.push(colors);
        }
    }
    attempts
}

fn assemble_frame_optimized_gif(
    image_magick: &str,
    frames: &[PathBuf],
    fps: u32,
    colors: u16,
    output_path: &Path,
) -> Result<(), String> {
    let fuzz_percent = if colors <= 85 {
        3
    } else if colors <= 172 {
        2
    } else {
        1
    };
    let mut command = Command::new(image_magick);
    command.arg("-quiet");
    for (index, frame) in frames.iter().enumerate() {
        command
            .arg("-delay")
            .arg(gif_frame_delay_centiseconds(index, fps).to_string())
            .arg(frame);
    }
    command
        .arg("+dither")
        .arg("-colors")
        .arg(colors.to_string())
        .arg("-fuzz")
        .arg(format!("{fuzz_percent}%"))
        .arg("-layers")
        .arg("OptimizeFrame")
        .arg("-layers")
        .arg("OptimizeTransparency")
        .arg("-loop")
        .arg("0")
        .arg("+map")
        .arg("-set")
        .arg("colorspace")
        .arg("sRGB")
        .arg(output_path);
    run_simple_command(&mut command, "ImageMagick GIF assembly")
}

fn image_magick_command() -> Option<&'static str> {
    ["magick", "convert"].into_iter().find(|command| {
        Command::new(command)
            .arg("-version")
            .output()
            .is_ok_and(|output| output.status.success())
    })
}

fn gif_frame_delay_centiseconds(frame_index: usize, fps: u32) -> u32 {
    let fps = u64::from(fps.max(1));
    let frame_index = frame_index as u64;
    let rounded_timestamp = |index: u64| (index * 100 + fps / 2) / fps;
    (rounded_timestamp(frame_index + 1) - rounded_timestamp(frame_index)).max(1) as u32
}

fn optimize_gif_with_gifsicle(output_path: &Path, colors: u16) {
    let available = Command::new("gifsicle")
        .arg("--version")
        .output()
        .is_ok_and(|output| output.status.success());
    if !available {
        crate::diagnostics::log_line(format_args!(
            "gifsicle not found; keeping ImageMagick-optimized GIF"
        ));
        return;
    }

    let optimized_path = output_path.with_extension("gifbrewery-optimized.gif");
    let mut command = Command::new("gifsicle");
    command
        .arg("-O3")
        .arg("--colors")
        .arg(colors.to_string())
        .arg(output_path)
        .arg("-o")
        .arg(&optimized_path);
    match run_simple_command(&mut command, "gifsicle optimization") {
        Ok(()) => {
            if let Err(err) = fs::rename(&optimized_path, output_path) {
                crate::diagnostics::log_line(format_args!(
                    "failed to install gifsicle output: {err}"
                ));
                let _ = fs::remove_file(&optimized_path);
            }
        }
        Err(err) => {
            crate::diagnostics::log_line(format_args!("{err}"));
            let _ = fs::remove_file(&optimized_path);
        }
    }
}

fn run_simple_command(command: &mut Command, label: &str) -> Result<(), String> {
    command.stdin(Stdio::null());
    let output = command
        .output()
        .map_err(|err| format!("failed to run {label}: {err}"))?;
    if output.status.success() {
        return Ok(());
    }

    let stderr = String::from_utf8_lossy(&output.stderr);
    Err(format!(
        "{label} failed with status {}: {}",
        output.status,
        stderr.trim()
    ))
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
    frame_sequence_temp_dir("gifbrewery-drawtext")
}

fn frame_sequence_temp_dir(prefix: &str) -> Result<PathBuf, String> {
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|err| format!("system clock error while preparing temporary files: {err}"))?;
    let path = std::env::temp_dir().join(format!(
        "{prefix}-{}-{}",
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

fn export_fps(project: &Project) -> Result<u32, String> {
    let source = project
        .source
        .as_ref()
        .ok_or_else(|| "project has no source media".to_string())?;

    let source_fps = source.fps.filter(|fps| *fps > 0.0).ok_or_else(|| {
        "source media frame rate is unknown; refusing to render with a guessed fps".to_string()
    })?;

    Ok(source_fps.round().clamp(1.0, 120.0) as u32)
}

fn source_is_gif(project: &Project) -> bool {
    project
        .source
        .as_ref()
        .and_then(|source| Path::new(&source.path).extension())
        .and_then(|extension| extension.to_str())
        .is_some_and(|extension| extension.eq_ignore_ascii_case("gif"))
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
        let aspect = crop_width / crop_height.max(1.0);
        let configured_width = project.settings.gif.output_width.filter(|width| *width > 0);
        let configured_height = project
            .settings
            .gif
            .output_height
            .filter(|height| *height > 0);
        let (output_width, output_height) = match (configured_width, configured_height) {
            (Some(width), Some(height)) => (width.max(1), height.max(1)),
            (Some(width), None) => {
                let width = width.max(1);
                let height = (f64::from(width) / aspect).round().max(1.0) as u32;
                (width, height)
            }
            (None, Some(height)) => {
                let height = height.max(1);
                let width = (f64::from(height) * aspect).round().max(1.0) as u32;
                (width, height)
            }
            (None, None) => (
                (crop_width * auto_scale).round().max(1.0) as u32,
                (crop_height * auto_scale).round().max(1.0) as u32,
            ),
        };

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
    fps: Option<u32>,
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
    let mut filters = Vec::new();
    if let Some(fps) = fps {
        filters.push(format!("fps={fps}"));
    }
    if source_needs_hdr_conversion(project) {
        if let Some(source) = &project.source {
            crate::diagnostics::log_line(format_args!(
                "automatic HDR-to-SDR conversion: color_space={:?} color_transfer={:?} color_primaries={:?} pixel_format={:?}",
                source.color_space,
                source.color_transfer,
                source.color_primaries,
                source.pixel_format
            ));
        }
        filters.extend(hdr_to_sdr_filters());
    }
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
                for (line_index, text_file) in drawtext_files(text_dir, text)? {
                    filters.push(drawtext_filter(
                        text,
                        line_index,
                        clip.range.start_seconds,
                        geometry,
                        &text_file,
                    ));
                }
            }
        }
    }

    if filters.is_empty() {
        filters.push("null".to_string());
    }

    Ok(filters.join(","))
}

fn source_needs_hdr_conversion(project: &Project) -> bool {
    let Some(source) = &project.source else {
        return false;
    };
    source
        .color_transfer
        .as_deref()
        .is_some_and(is_hdr_transfer)
        || source
            .color_primaries
            .as_deref()
            .is_some_and(|primaries| primaries.eq_ignore_ascii_case("bt2020"))
}

fn is_hdr_transfer(transfer: &str) -> bool {
    transfer.eq_ignore_ascii_case("smpte2084")
        || transfer.eq_ignore_ascii_case("arib-std-b67")
        || transfer.eq_ignore_ascii_case("hlg")
}

fn hdr_to_sdr_filters() -> Vec<String> {
    vec![
        "zscale=t=linear:npl=100".to_string(),
        "format=gbrpf32le".to_string(),
        "tonemap=tonemap=hable:desat=0".to_string(),
        "zscale=p=bt709:t=bt709:m=bt709:r=tv".to_string(),
        "format=yuv420p".to_string(),
    ]
}

fn palette_filter(project: &Project, video_filter: &str) -> String {
    let colors = project.settings.gif.colors.clamp(2, 256);
    if project.settings.gif.high_quality_quantization {
        format!(
            "{video_filter},split[gifbrewery_palette_src][gifbrewery_frames];\
             [gifbrewery_palette_src]palettegen=max_colors={colors}:reserve_transparent=0:stats_mode=single[gifbrewery_palette];\
             [gifbrewery_frames][gifbrewery_palette]paletteuse=dither=none:new=1"
        )
    } else {
        format!(
            "{video_filter},split[gifbrewery_palette_src][gifbrewery_frames];\
             [gifbrewery_palette_src]palettegen=max_colors={colors}:reserve_transparent=0:stats_mode=full[gifbrewery_palette];\
             [gifbrewery_frames][gifbrewery_palette]paletteuse=dither=none"
        )
    }
}

fn uses_frame_optimized_hdr_encoder(project: &Project) -> bool {
    project.settings.gif.optimize
        && !project.settings.gif.high_quality_quantization
        && source_needs_hdr_conversion(project)
}

fn drawtext_files(text_dir: &Path, text: &TextOverlay) -> Result<Vec<(usize, PathBuf)>, String> {
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

    let normalized_text = text
        .text
        .replace("\r\n", "\n")
        .replace('\r', "\n")
        .replace('\u{0085}', "\n")
        .replace('\u{2028}', "\n")
        .replace('\u{2029}', "\n");
    let mut files = Vec::new();

    for (line_index, line) in normalized_text.split('\n').enumerate() {
        let clean_line: String = line
            .chars()
            .filter(|ch| *ch == '\t' || !ch.is_control())
            .collect();
        if clean_line.is_empty() {
            continue;
        }

        let path = text_dir.join(format!("{safe_id}-line-{line_index}.txt"));
        fs::write(&path, clean_line).map_err(|err| {
            format!(
                "failed to write drawtext temp file {}: {err}",
                path.display()
            )
        })?;
        files.push((line_index, path));
    }

    Ok(files)
}

fn drawtext_filter(
    text: &TextOverlay,
    line_index: usize,
    clip_start_seconds: f64,
    geometry: RenderGeometry,
    text_file: &Path,
) -> String {
    let escaped_text_file = escape_drawtext(&text_file.display().to_string());
    let font_argument = drawtext_font_argument(&text.font_family, text.font_weight);
    let x = if text.alignment == TextAlignment::Center {
        format!(
            "w*{:.6}+(w*{:.6}-text_w)/2",
            text.bounds.x, text.bounds.width
        )
    } else {
        format!("w*{:.6}", text.bounds.x)
    };
    let y = format!("h*{:.6}+{}*line_h", text.bounds.y, line_index);
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

    let mut filters = Vec::new();
    let stroke_radius = stroke_width.max(0.0) as i32;
    if stroke_radius > 0 {
        let stroke_color = ffmpeg_color(text.stroke_color);
        for (dx, dy) in square_stroke_offsets(stroke_radius) {
            filters.push(drawtext_filter_at_offset(
                &escaped_text_file,
                &font_argument,
                font_size,
                &stroke_color,
                &x,
                &y,
                dx,
                dy,
                &enable,
            ));
        }
    }
    filters.push(drawtext_filter_at_offset(
        &escaped_text_file,
        &font_argument,
        font_size,
        &ffmpeg_color(text.text_color),
        &x,
        &y,
        0,
        0,
        &enable,
    ));
    filters.join(",")
}

fn square_stroke_offsets(radius: i32) -> Vec<(i32, i32)> {
    let mut offsets = Vec::new();
    for dy in -radius..=radius {
        for dx in -radius..=radius {
            if dx == 0 && dy == 0 {
                continue;
            }
            offsets.push((dx, dy));
        }
    }
    offsets
}

fn drawtext_filter_at_offset(
    escaped_text_file: &str,
    font_argument: &str,
    font_size: f64,
    color: &str,
    x: &str,
    y: &str,
    dx: i32,
    dy: i32,
    enable: &str,
) -> String {
    format!(
        "drawtext=textfile='{escaped_text_file}':{font_argument}:fontsize={font_size:.0}:\
         fontcolor={color}:borderw=0:x='{x_offset}':y='{y_offset}':enable='{enable}'",
        x_offset = offset_expression(x, dx),
        y_offset = offset_expression(y, dy),
    )
}

fn offset_expression(expression: &str, offset: i32) -> String {
    match offset.cmp(&0) {
        std::cmp::Ordering::Greater => format!("({expression})+{offset}"),
        std::cmp::Ordering::Less => format!("({expression}){offset}"),
        std::cmp::Ordering::Equal => expression.to_string(),
    }
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
    use super::{
        export_fps, gif_frame_delay_centiseconds, gif_loop_count, hdr_palette_attempts,
        palette_filter, render_attempts, source_needs_hdr_conversion,
        uses_frame_optimized_hdr_encoder, video_filter, RenderAttempt, RenderGeometry,
    };
    use gifbrewery_core::{CropRect, MediaSource, Project, TimelineRange};

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

    #[test]
    fn width_only_resize_preserves_crop_aspect() {
        let mut project = geometry_test_project();
        project.settings.gif.output_width = Some(300);
        project.settings.gif.output_height = None;

        let clip = project.clips.first().expect("default project has a clip");
        let geometry = RenderGeometry::from_project(&project, clip, 1.0);

        assert_eq!(geometry.output_width, 300);
        assert_eq!(geometry.output_height, 150);
    }

    #[test]
    fn height_only_resize_preserves_crop_aspect() {
        let mut project = geometry_test_project();
        project.settings.gif.output_width = None;
        project.settings.gif.output_height = Some(125);

        let clip = project.clips.first().expect("default project has a clip");
        let geometry = RenderGeometry::from_project(&project, clip, 1.0);

        assert_eq!(geometry.output_width, 250);
        assert_eq!(geometry.output_height, 125);
    }

    #[test]
    fn export_fps_requires_source_fps() {
        let mut project = geometry_test_project();
        project
            .source
            .as_mut()
            .expect("test project has source")
            .fps = None;
        project
            .clips
            .first_mut()
            .expect("default project has a clip")
            .frame_strategy = gifbrewery_core::FrameStrategy::Fps(12);

        let err = export_fps(&project).expect_err("missing source fps must not fall back");

        assert!(err.contains("frame rate is unknown"));
    }

    #[test]
    fn hdr_source_gets_automatic_sdr_conversion_before_palette() {
        let mut project = geometry_test_project();
        let source = project.source.as_mut().expect("test project has source");
        source.color_space = Some("bt2020nc".to_string());
        source.color_transfer = Some("smpte2084".to_string());
        source.color_primaries = Some("bt2020".to_string());
        source.pixel_format = Some("yuv420p10le".to_string());
        project.settings.gif.output_width = Some(300);
        project.settings.gif.output_height = Some(150);
        let clip = project.clips.first().expect("default project has a clip");
        let geometry = RenderGeometry::from_project(&project, clip, 1.0);

        let filter = video_filter(
            &project,
            clip,
            Some(24),
            geometry,
            std::path::Path::new("/tmp"),
        )
        .expect("filter");

        assert!(source_needs_hdr_conversion(&project));
        assert!(filter.contains("zscale=t=linear:npl=100"));
        assert!(filter.contains("tonemap=tonemap=hable:desat=0"));
        assert!(filter.contains("zscale=p=bt709:t=bt709:m=bt709:r=tv"));
        assert!(
            filter.find("tonemap=tonemap=hable").expect("tonemap")
                < filter.find(",scale=").expect("scale")
        );
    }

    #[test]
    fn hdr_source_conversion_does_not_depend_on_legacy_toggle() {
        let mut project = geometry_test_project();
        let source = project.source.as_mut().expect("test project has source");
        source.color_space = Some("bt2020nc".to_string());
        source.color_transfer = Some("smpte2084".to_string());
        source.color_primaries = Some("bt2020".to_string());

        project.settings.gif.tone_map_hdr = false;
        assert!(source_needs_hdr_conversion(&project));

        project.settings.gif.tone_map_hdr = true;
        assert!(source_needs_hdr_conversion(&project));
    }

    #[test]
    fn sdr_source_does_not_get_tonemap() {
        let project = geometry_test_project();
        let clip = project.clips.first().expect("default project has a clip");
        let geometry = RenderGeometry::from_project(&project, clip, 1.0);

        let filter = video_filter(
            &project,
            clip,
            Some(24),
            geometry,
            std::path::Path::new("/tmp"),
        )
        .expect("filter");

        assert!(!source_needs_hdr_conversion(&project));
        assert!(!filter.contains("tonemap="));
    }

    #[test]
    fn target_size_adds_smaller_palette_attempts() {
        let mut project = geometry_test_project();
        project.settings.gif.colors = 256;
        project.settings.gif.high_quality_quantization = true;
        project.settings.gif.optimize = false;
        project.settings.gif.target_max_bytes = Some(16 * 1024 * 1024);

        let attempts = render_attempts(&project);

        assert_eq!(
            attempts[..3],
            [
                RenderAttempt {
                    colors: 256,
                    high_quality_quantization: true,
                },
                RenderAttempt {
                    colors: 256,
                    high_quality_quantization: false,
                },
                RenderAttempt {
                    colors: 192,
                    high_quality_quantization: false,
                },
            ]
        );
        assert!(attempts.contains(&RenderAttempt {
            colors: 128,
            high_quality_quantization: false,
        }));
        assert!(!attempts.iter().any(|attempt| attempt.colors < 128));
    }

    #[test]
    fn optimized_hdr_export_uses_frame_optimizer() {
        let mut project = geometry_test_project();
        let source = project.source.as_mut().expect("test project has source");
        source.color_space = Some("bt2020nc".to_string());
        source.color_transfer = Some("smpte2084".to_string());
        source.color_primaries = Some("bt2020".to_string());
        project.settings.gif.optimize = true;
        project.settings.gif.high_quality_quantization = false;
        project.settings.gif.target_max_bytes = Some(16 * 1024 * 1024);

        assert_eq!(
            render_attempts(&project),
            vec![RenderAttempt {
                colors: 256,
                high_quality_quantization: false,
            }]
        );
        assert!(uses_frame_optimized_hdr_encoder(&project));
    }

    #[test]
    fn optimized_sdr_export_uses_adaptive_palette_without_dithering() {
        let mut project = geometry_test_project();
        project.settings.gif.optimize = true;
        project.settings.gif.high_quality_quantization = false;

        let filter = palette_filter(&project, "fps=24,scale=800:335");

        assert!(!uses_frame_optimized_hdr_encoder(&project));
        assert!(filter.contains("palettegen=max_colors=256"));
        assert!(filter.contains("paletteuse=dither=none"));
        assert!(!filter.contains("stats_mode=single"));
    }

    #[test]
    fn optimize_uses_one_size_focused_attempt_without_target() {
        let mut project = geometry_test_project();
        project.settings.gif.colors = 256;
        project.settings.gif.high_quality_quantization = true;
        project.settings.gif.optimize = true;
        project.settings.gif.target_max_bytes = None;

        assert_eq!(
            render_attempts(&project),
            vec![RenderAttempt {
                colors: 256,
                high_quality_quantization: false,
            }]
        );
    }

    #[test]
    fn gif_frame_delays_preserve_24_fps_duration() {
        let delays = (0..24)
            .map(|index| gif_frame_delay_centiseconds(index, 24))
            .collect::<Vec<_>>();

        assert_eq!(delays.iter().sum::<u32>(), 100);
        assert!(delays.iter().all(|delay| matches!(delay, 4 | 5)));
    }

    #[test]
    fn hdr_palette_attempts_start_at_requested_quality() {
        assert_eq!(
            hdr_palette_attempts(256),
            vec![256, 192, 128, 96, 64, 48, 32, 24, 16]
        );
        assert_eq!(hdr_palette_attempts(100), vec![100, 96, 64, 48, 32, 24, 16]);
        assert_eq!(hdr_palette_attempts(48), vec![48, 32, 24, 16]);
    }

    fn geometry_test_project() -> Project {
        let mut project = Project::default();
        project.source = Some(MediaSource {
            path: "test.gif".to_string(),
            duration_seconds: Some(2.0),
            natural_width: Some(800),
            natural_height: Some(400),
            fps: Some(25.0),
            color_space: None,
            color_transfer: None,
            color_primaries: None,
            pixel_format: None,
        });
        let clip = project
            .clips
            .first_mut()
            .expect("default project has a clip");
        clip.range = TimelineRange {
            start_seconds: 0.0,
            end_seconds: 2.0,
        };
        clip.crop = Some(CropRect {
            left: 0.125,
            right: 0.125,
            top: 0.0,
            bottom: 0.25,
        });
        project
    }
}
