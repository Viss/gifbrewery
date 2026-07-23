use gstreamer as gst;
use std::fmt;
use std::fs::{self, File, OpenOptions};
use std::io::Write;
use std::panic;
use std::path::PathBuf;
use std::process::Command;
use std::sync::{Mutex, OnceLock};

static LOG_FILE: OnceLock<Mutex<File>> = OnceLock::new();
static LOG_PATH: OnceLock<PathBuf> = OnceLock::new();
static GSTREAMER_READY: OnceLock<bool> = OnceLock::new();

pub fn init_debug_log() {
    let path = default_log_path();
    if let Some(parent) = path.parent() {
        let _ = fs::create_dir_all(parent);
    }

    match OpenOptions::new().create(true).append(true).open(&path) {
        Ok(file) => {
            let _ = LOG_PATH.set(path.clone());
            let _ = LOG_FILE.set(Mutex::new(file));
            log_line(format_args!("--- GIF Brewery session start ---"));
            log_line(format_args!("debug log: {}", path.display()));
            log_line(format_args!(
                "argv: {:?}",
                std::env::args().collect::<Vec<_>>()
            ));
            log_line(format_args!(
                "cwd: {}",
                std::env::current_dir()
                    .map(|path| path.display().to_string())
                    .unwrap_or_else(|err| format!("unavailable: {err}"))
            ));
            install_panic_hook();
        }
        Err(err) => {
            eprintln!(
                "failed to create GIF Brewery debug log at {}: {err}",
                path.display()
            );
        }
    }
}

pub fn log_path() -> Option<&'static PathBuf> {
    LOG_PATH.get()
}

pub fn set_gstreamer_ready(ready: bool) {
    let _ = GSTREAMER_READY.set(ready);
}

pub fn gstreamer_ready() -> bool {
    *GSTREAMER_READY.get().unwrap_or(&false)
}

pub fn log_line(args: fmt::Arguments<'_>) {
    eprintln!("{args}");
    if let Some(file) = LOG_FILE.get() {
        if let Ok(mut file) = file.lock() {
            let _ = writeln!(file, "{args}");
        }
    }
}

pub fn runtime_issues(gstreamer_ready: bool) -> Vec<String> {
    let mut issues = Vec::new();

    if !command_available("ffmpeg") {
        issues.push(
            "Missing ffmpeg. Install the ffmpeg package for thumbnailing and GIF export."
                .to_string(),
        );
    }

    if !command_available("magick") && !command_available("convert") {
        issues.push(
            "Missing ImageMagick. Install the imagemagick package for compact HDR GIF export."
                .to_string(),
        );
    }

    if !gstreamer_ready {
        issues.push(
            "GStreamer did not initialize. Install GStreamer runtime libraries and plugins."
                .to_string(),
        );
        return issues;
    }

    for (factory, package_hint) in [
        (
            "playbin",
            "GStreamer playback core, usually from gstreamer1.0-tools/base packages",
        ),
        (
            "gtk4paintablesink",
            "gstreamer1.0-gtk4, needed for video preview inside GTK",
        ),
        ("qtdemux", "gstreamer1.0-plugins-good, needed for MP4 input"),
        (
            "avdec_h264",
            "gstreamer1.0-libav, needed for common H.264 MP4 input",
        ),
    ] {
        if gst::ElementFactory::find(factory).is_none() {
            issues.push(format!(
                "Missing GStreamer element `{factory}`. Install {package_hint}."
            ));
        }
    }

    if !any_factory_available(&["avdec_gif", "gdkpixbufdec"]) {
        issues.push(
            "Missing GStreamer GIF decoding support. Install gstreamer1.0-libav or gstreamer1.0-plugins-good."
                .to_string(),
        );
    }

    issues
}

fn command_available(command: &str) -> bool {
    Command::new(command)
        .arg("-version")
        .output()
        .map(|output| output.status.success())
        .unwrap_or(false)
}

fn any_factory_available(factories: &[&str]) -> bool {
    factories
        .iter()
        .any(|factory| gst::ElementFactory::find(factory).is_some())
}

fn install_panic_hook() {
    panic::set_hook(Box::new(|info| {
        let location = info
            .location()
            .map(|location| format!("{}:{}", location.file(), location.line()))
            .unwrap_or_else(|| "unknown location".to_string());
        let payload = info
            .payload()
            .downcast_ref::<&str>()
            .copied()
            .or_else(|| info.payload().downcast_ref::<String>().map(String::as_str))
            .unwrap_or("unknown panic payload");
        log_line(format_args!("PANIC at {location}: {payload}"));
    }));
}

fn default_log_path() -> PathBuf {
    if let Ok(path) = std::env::var("GIFBREWERY_LOG") {
        return PathBuf::from(path);
    }

    if let Some(path) = exe_adjacent_log_path() {
        return path;
    }

    if let Ok(state_home) = std::env::var("XDG_STATE_HOME") {
        return PathBuf::from(state_home).join("gifbrewery/gifbrewery.log");
    }

    if let Ok(home) = std::env::var("HOME") {
        return PathBuf::from(home).join(".local/state/gifbrewery/gifbrewery.log");
    }

    PathBuf::from("gifbrewery.log")
}

fn exe_adjacent_log_path() -> Option<PathBuf> {
    let exe = std::env::current_exe().ok()?;
    let dir = exe.parent()?;
    let probe_path = dir.join(".gifbrewery-log-write-test");

    match OpenOptions::new()
        .create(true)
        .write(true)
        .truncate(true)
        .open(&probe_path)
    {
        Ok(_) => {
            let _ = fs::remove_file(probe_path);
            Some(dir.join("gifbrewery.log"))
        }
        Err(_) => None,
    }
}
