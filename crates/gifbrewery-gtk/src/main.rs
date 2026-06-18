mod diagnostics;
mod export;
mod media;
mod thumbnails;
mod timeline;
mod ui;

use adw::prelude::*;
use gifbrewery_core::{
    CropRect, MediaSource, Overlay, Project, Rect, RgbaColor, TextOverlay, TimelineRange,
};
use gtk::gio;
use std::cell::RefCell;
use std::fs;
use std::path::Path;
use std::process::Command;
use std::rc::Rc;
use std::time::Instant;

const APP_ID: &str = "dev.gifbrewery.GifBrewery";

fn main() -> glib::ExitCode {
    diagnostics::init_debug_log();

    if let Some(exit_code) = maybe_run_smoke_export() {
        return exit_code;
    }

    let app = adw::Application::builder()
        .application_id(APP_ID)
        .flags(gio::ApplicationFlags::HANDLES_OPEN)
        .build();

    app.connect_startup(|_| {
        adw::init().expect("failed to initialize libadwaita");
        match gstreamer::init() {
            Ok(()) => {
                diagnostics::set_gstreamer_ready(true);
                diagnostics::log_line(format_args!("GStreamer initialized"));
            }
            Err(err) => {
                diagnostics::set_gstreamer_ready(false);
                diagnostics::log_line(format_args!("failed to initialize GStreamer: {err}"));
            }
        }
    });

    let app_handle = Rc::new(RefCell::new(None::<ui::AppHandle>));

    app.connect_activate({
        let app_handle = Rc::clone(&app_handle);
        move |app| {
            let mut handle = app_handle.borrow_mut();
            if let Some(handle) = handle.as_ref() {
                handle.present();
            } else {
                *handle = Some(ui::build_main_window(app));
            }
        }
    });

    app.connect_open({
        let app_handle = Rc::clone(&app_handle);
        move |app, files, _hint| {
            let mut handle = app_handle.borrow_mut();
            if handle.is_none() {
                *handle = Some(ui::build_main_window(app));
            }

            if let (Some(handle), Some(file)) = (handle.as_ref(), files.first()) {
                handle.open_file(file);
                handle.present();
            }
        }
    });

    app.run()
}

fn maybe_run_smoke_export() -> Option<glib::ExitCode> {
    let mut args = std::env::args().skip(1);
    let command = args.next()?;
    if command != "--smoke-export"
        && command != "--smoke-export-multi-overlay"
        && command != "--smoke-export-layout"
        && command != "--smoke-render-frame"
        && command != "--smoke-compare-preview"
        && command != "--smoke-crop-playback"
        && command != "--smoke-overlay-after-crop"
    {
        return None;
    }

    let Some(source_path) = args.next() else {
        eprintln!("usage: gifbrewery-gtk {command} SOURCE OUTPUT");
        return Some(glib::ExitCode::FAILURE);
    };
    let Some(output_path) = args.next() else {
        eprintln!("usage: gifbrewery-gtk {command} SOURCE OUTPUT");
        return Some(glib::ExitCode::FAILURE);
    };

    if let Err(err) = gstreamer::init() {
        eprintln!("failed to initialize GStreamer for smoke export: {err}");
        return Some(glib::ExitCode::FAILURE);
    }
    diagnostics::set_gstreamer_ready(true);

    let project = match command.as_str() {
        "--smoke-export-multi-overlay" => multi_overlay_smoke_project(source_path),
        "--smoke-compare-preview" | "--smoke-crop-playback" | "--smoke-overlay-after-crop" => {
            compare_preview_smoke_project(source_path)
        }
        "--smoke-export-layout" | "--smoke-render-frame" => layout_smoke_project(source_path),
        _ => smoke_project(source_path),
    };

    if command == "--smoke-crop-playback" {
        return match run_crop_playback_smoke(&project, Path::new(&output_path)) {
            Ok(()) => {
                eprintln!("crop playback smoke artifacts written to {output_path}");
                Some(glib::ExitCode::SUCCESS)
            }
            Err(err) => {
                eprintln!("crop playback smoke failed: {err}");
                Some(glib::ExitCode::FAILURE)
            }
        };
    }

    if command == "--smoke-overlay-after-crop" {
        return match run_overlay_after_crop_smoke(&project, Path::new(&output_path)) {
            Ok(()) => {
                eprintln!("overlay-after-crop smoke artifacts written to {output_path}");
                Some(glib::ExitCode::SUCCESS)
            }
            Err(err) => {
                eprintln!("overlay-after-crop smoke failed: {err}");
                Some(glib::ExitCode::FAILURE)
            }
        };
    }

    if command == "--smoke-compare-preview" {
        return match run_preview_compare_smoke(&project, Path::new(&output_path)) {
            Ok(()) => {
                eprintln!("preview/export comparison artifacts written to {output_path}");
                Some(glib::ExitCode::SUCCESS)
            }
            Err(err) => {
                eprintln!("preview/export comparison failed: {err}");
                Some(glib::ExitCode::FAILURE)
            }
        };
    }

    if command == "--smoke-render-frame" {
        return match export::render_frame_png(&project, 0.0, Path::new(&output_path)) {
            Ok(()) => {
                eprintln!("rendered frame to {output_path}");
                Some(glib::ExitCode::SUCCESS)
            }
            Err(err) => {
                eprintln!("frame render failed: {err}");
                Some(glib::ExitCode::FAILURE)
            }
        };
    }

    match export::export_gif(&project, Path::new(&output_path)) {
        Ok(()) => {
            eprintln!("exported GIF to {output_path}");
            Some(glib::ExitCode::SUCCESS)
        }
        Err(err) => {
            eprintln!("GIF export failed: {err}");
            Some(glib::ExitCode::FAILURE)
        }
    }
}

fn smoke_project(source_path: String) -> Project {
    let mut project = Project::default();
    let file = gio::File::for_path(&source_path);
    let metadata = media::discover(&file).ok();
    project.source = Some(MediaSource {
        path: source_path,
        duration_seconds: metadata
            .as_ref()
            .and_then(|metadata| metadata.duration_seconds),
        natural_width: metadata.as_ref().and_then(|metadata| metadata.width),
        natural_height: metadata.as_ref().and_then(|metadata| metadata.height),
        fps: metadata.as_ref().and_then(|metadata| metadata.fps),
    });
    project
}

fn multi_overlay_smoke_project(source_path: String) -> Project {
    let mut project = smoke_project(source_path);
    if let Some(clip) = project.clips.first_mut() {
        clip.range = TimelineRange {
            start_seconds: 0.0,
            end_seconds: 1.0,
        };
    }

    let mut first = TextOverlay::default_caption();
    first.id = "caption-smoke-1".to_string();
    first.text = "FIRST".to_string();
    first.range = TimelineRange {
        start_seconds: 0.0,
        end_seconds: 0.45,
    };
    first.bounds = Rect {
        x: 0.08,
        y: 0.12,
        width: 0.84,
        height: 0.2,
    };
    first.text_color = RgbaColor::WHITE;
    first.stroke_color = RgbaColor::BLACK;
    first.stroke_width = 2.0;

    let mut second = TextOverlay::default_caption();
    second.id = "caption-smoke-2".to_string();
    second.text = "SECOND".to_string();
    second.range = TimelineRange {
        start_seconds: 0.55,
        end_seconds: 1.0,
    };
    second.bounds = Rect {
        x: 0.08,
        y: 0.68,
        width: 0.84,
        height: 0.2,
    };
    second.text_color = RgbaColor {
        red: 1.0,
        green: 0.93,
        blue: 0.2,
        alpha: 1.0,
    };
    second.stroke_color = RgbaColor::BLACK;
    second.stroke_width = 2.0;

    project.overlays = vec![Overlay::Text(first), Overlay::Text(second)];
    project
}

fn layout_smoke_project(source_path: String) -> Project {
    let mut project = smoke_project(source_path);
    if let Some(clip) = project.clips.first_mut() {
        clip.range = TimelineRange {
            start_seconds: 0.0,
            end_seconds: 2.0,
        };
        clip.crop = Some(CropRect {
            left: 0.05,
            right: 0.05,
            top: 0.08,
            bottom: 0.08,
        });
    }
    project.settings.gif.output_width = Some(640);
    project.settings.gif.output_height = Some(360);

    let mut text = TextOverlay::default_caption();
    text.id = "caption-layout-smoke".to_string();
    text.text = "Layout smoke\nstroke + resize".to_string();
    text.range = TimelineRange {
        start_seconds: 0.0,
        end_seconds: 2.0,
    };
    text.bounds = Rect {
        x: 0.08,
        y: 0.64,
        width: 0.84,
        height: 0.24,
    };
    text.font_size = 42.0;
    text.stroke_width = 4.0;
    text.text_color = RgbaColor::WHITE;
    text.stroke_color = RgbaColor::BLACK;
    project.overlays = vec![Overlay::Text(text)];
    project
}

fn compare_preview_smoke_project(source_path: String) -> Project {
    let mut project = smoke_project(source_path);
    if let Some(clip) = project.clips.first_mut() {
        clip.range = TimelineRange {
            start_seconds: 0.0,
            end_seconds: 2.0,
        };
        clip.crop = Some(CropRect {
            left: 0.12,
            right: 0.10,
            top: 0.10,
            bottom: 0.12,
        });
    }
    project.settings.gif.output_width = Some(640);
    project.settings.gif.output_height = Some(360);

    let mut first = TextOverlay::default_caption();
    first.id = "caption-compare-1".to_string();
    first.text = "Preview = Export".to_string();
    first.range = TimelineRange {
        start_seconds: 0.0,
        end_seconds: 1.25,
    };
    first.bounds = Rect {
        x: 0.08,
        y: 0.16,
        width: 0.84,
        height: 0.18,
    };
    first.font_size = 44.0;
    first.stroke_width = 4.0;
    first.text_color = RgbaColor::WHITE;
    first.stroke_color = RgbaColor::BLACK;

    let mut second = TextOverlay::default_caption();
    second.id = "caption-compare-2".to_string();
    second.text = "Second overlay".to_string();
    second.range = TimelineRange {
        start_seconds: 0.5,
        end_seconds: 2.0,
    };
    second.bounds = Rect {
        x: 0.14,
        y: 0.68,
        width: 0.78,
        height: 0.18,
    };
    second.font_size = 36.0;
    second.stroke_width = 3.0;
    second.text_color = RgbaColor {
        red: 1.0,
        green: 0.92,
        blue: 0.18,
        alpha: 1.0,
    };
    second.stroke_color = RgbaColor::BLACK;

    project.overlays = vec![Overlay::Text(first), Overlay::Text(second)];
    project
}

fn run_preview_compare_smoke(project: &Project, out_dir: &Path) -> Result<(), String> {
    let compare_seconds = 0.75;
    fs::create_dir_all(out_dir).map_err(|err| {
        format!(
            "failed to create preview comparison directory {}: {err}",
            out_dir.display()
        )
    })?;

    let preview_png = out_dir.join("overlay-preview-exact.png");
    let export_gif = out_dir.join("overlay-preview-export.gif");
    let export_frame = out_dir.join("overlay-preview-export-frame.png");
    let diff_png = out_dir.join("overlay-preview-diff.png");
    let manifest = out_dir.join("overlay-preview-compare.txt");

    export::render_frame_png(project, compare_seconds, &preview_png)?;
    export::export_gif(project, &export_gif)?;
    extract_gif_frame(&export_gif, compare_seconds, &export_frame)?;
    let rmse = compare_images(&preview_png, &export_frame, &diff_png)?;

    fs::write(
        &manifest,
        format!(
            "compare_seconds={compare_seconds:.3}\npreview={}\nexport={}\nexport_frame={}\ndiff={}\nrmse={rmse:.6}\n",
            preview_png.display(),
            export_gif.display(),
            export_frame.display(),
            diff_png.display()
        ),
    )
    .map_err(|err| format!("failed to write {}: {err}", manifest.display()))?;

    if rmse > 0.12 {
        return Err(format!(
            "preview/export RMSE {rmse:.6} exceeded tolerance 0.120000; see {}",
            diff_png.display()
        ));
    }

    Ok(())
}

fn run_crop_playback_smoke(project: &Project, out_dir: &Path) -> Result<(), String> {
    fs::create_dir_all(out_dir).map_err(|err| {
        format!(
            "failed to create crop playback smoke directory {}: {err}",
            out_dir.display()
        )
    })?;
    let frames_dir = out_dir.join("frames");
    let _ = fs::remove_dir_all(&frames_dir);

    let started = Instant::now();
    let sequence = export::render_frame_sequence(project, &frames_dir)?;
    let render_seconds = started.elapsed().as_secs_f64();
    let expected_frames = (sequence.duration_seconds * f64::from(sequence.fps))
        .ceil()
        .max(1.0) as usize;
    let frame_count = sequence.frames.len();
    let generated_fps = frame_count as f64 / render_seconds.max(0.001);
    let frame_budget_ms = 1000.0 / f64::from(sequence.fps.max(1));
    let first_frame = out_dir.join("crop-playback-first-frame.png");
    let middle_frame = out_dir.join("crop-playback-middle-frame.png");
    let last_frame = out_dir.join("crop-playback-last-frame.png");

    fs::copy(&sequence.frames[0], &first_frame).map_err(|err| {
        format!(
            "failed to copy first playback frame to {}: {err}",
            first_frame.display()
        )
    })?;
    fs::copy(&sequence.frames[frame_count / 2], &middle_frame).map_err(|err| {
        format!(
            "failed to copy middle playback frame to {}: {err}",
            middle_frame.display()
        )
    })?;
    fs::copy(&sequence.frames[frame_count - 1], &last_frame).map_err(|err| {
        format!(
            "failed to copy last playback frame to {}: {err}",
            last_frame.display()
        )
    })?;

    let manifest = out_dir.join("crop-playback-smoke.txt");
    fs::write(
        &manifest,
        format!(
            "fps={}\nduration_seconds={:.6}\nexpected_frames={expected_frames}\nactual_frames={frame_count}\nrender_seconds={render_seconds:.6}\ngenerated_fps={generated_fps:.3}\nframe_budget_ms={frame_budget_ms:.3}\nframes_dir={}\nfirst_frame={}\nmiddle_frame={}\nlast_frame={}\n",
            sequence.fps,
            sequence.duration_seconds,
            frames_dir.display(),
            first_frame.display(),
            middle_frame.display(),
            last_frame.display()
        ),
    )
    .map_err(|err| format!("failed to write {}: {err}", manifest.display()))?;

    if frame_count + 1 < expected_frames {
        return Err(format!(
            "rendered playback sequence produced too few frames: actual={frame_count} expected={expected_frames}"
        ));
    }

    Ok(())
}

fn run_overlay_after_crop_smoke(project: &Project, out_dir: &Path) -> Result<(), String> {
    let compare_seconds = 0.75;
    fs::create_dir_all(out_dir).map_err(|err| {
        format!(
            "failed to create overlay-after-crop smoke directory {}: {err}",
            out_dir.display()
        )
    })?;

    let mut cropped_without_overlay = project.clone();
    cropped_without_overlay.overlays.clear();
    let before_png = out_dir.join("before-overlay.png");
    let after_png = out_dir.join("after-overlay.png");
    let diff_png = out_dir.join("overlay-diff.png");
    let manifest = out_dir.join("overlay-after-crop-smoke.txt");

    export::render_frame_png(&cropped_without_overlay, compare_seconds, &before_png)?;
    export::render_frame_png(project, compare_seconds, &after_png)?;
    let rmse = compare_images(&before_png, &after_png, &diff_png)?;

    fs::write(
        &manifest,
        format!(
            "compare_seconds={compare_seconds:.3}\nbefore={}\nafter={}\ndiff={}\nrmse={rmse:.6}\n",
            before_png.display(),
            after_png.display(),
            diff_png.display()
        ),
    )
    .map_err(|err| format!("failed to write {}: {err}", manifest.display()))?;

    if rmse < 0.006 {
        return Err(format!(
            "overlay-after-crop rendered frame barely changed: rmse={rmse:.6}; see {}",
            diff_png.display()
        ));
    }

    Ok(())
}

fn extract_gif_frame(gif_path: &Path, seconds: f64, output_path: &Path) -> Result<(), String> {
    let output = Command::new("ffmpeg")
        .arg("-hide_banner")
        .arg("-y")
        .arg("-ss")
        .arg(format!("{seconds:.3}"))
        .arg("-i")
        .arg(gif_path)
        .arg("-frames:v")
        .arg("1")
        .arg("-update")
        .arg("1")
        .arg(output_path)
        .output()
        .map_err(|err| format!("failed to run ffmpeg for frame extract: {err}"))?;
    if output.status.success() {
        Ok(())
    } else {
        Err(format!(
            "ffmpeg frame extract failed: {}",
            String::from_utf8_lossy(&output.stderr)
        ))
    }
}

fn compare_images(preview_png: &Path, export_frame: &Path, diff_png: &Path) -> Result<f64, String> {
    let output = Command::new("compare")
        .arg("-metric")
        .arg("RMSE")
        .arg(preview_png)
        .arg(export_frame)
        .arg(diff_png)
        .output()
        .map_err(|err| format!("failed to run ImageMagick compare: {err}"))?;
    let stderr = String::from_utf8_lossy(&output.stderr);
    let normalized = stderr
        .split('(')
        .nth(1)
        .and_then(|tail| tail.split(')').next())
        .and_then(|value| value.trim().parse::<f64>().ok())
        .ok_or_else(|| format!("failed to parse ImageMagick RMSE output: {stderr}"))?;
    Ok(normalized)
}
