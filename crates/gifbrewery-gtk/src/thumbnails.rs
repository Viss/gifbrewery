use crate::timeline::TimelineThumbnail;
use gdk_pixbuf::Pixbuf;
use std::collections::hash_map::DefaultHasher;
use std::fs;
use std::hash::{Hash, Hasher};
use std::path::{Path, PathBuf};
use std::process::Command;

const THUMB_WIDTH: i32 = 120;
const THUMB_HEIGHT: i32 = 68;

#[derive(Debug, Clone)]
pub struct ThumbnailFile {
    pub timestamp_seconds: f64,
    pub path: PathBuf,
}

pub fn extract_thumbnail_files(
    source_path: &Path,
    duration_seconds: f64,
    target_count: usize,
) -> Vec<ThumbnailFile> {
    if target_count == 0 || duration_seconds <= 0.0 {
        return Vec::new();
    }

    let cache_dir = thumbnail_cache_dir(source_path, duration_seconds);
    if let Err(err) = fs::create_dir_all(&cache_dir) {
        eprintln!("failed to create thumbnail cache: {err}");
        return Vec::new();
    }

    let mut thumbnails = Vec::new();
    let latest_timestamp = (duration_seconds - 0.15).max(0.0);
    for index in 0..target_count {
        let timestamp = latest_timestamp * (index as f64 + 0.5) / target_count as f64;
        let output_path = cache_dir.join(format!("{index:02}.png"));
        if !output_path.exists() && !extract_thumbnail(source_path, timestamp, &output_path) {
            continue;
        }

        thumbnails.push(ThumbnailFile {
            timestamp_seconds: timestamp,
            path: output_path,
        });
    }

    thumbnails
}

pub fn load_thumbnail_pixbufs(files: &[ThumbnailFile]) -> Vec<TimelineThumbnail> {
    let mut thumbnails = Vec::new();
    for file in files {
        match Pixbuf::from_file_at_size(&file.path, THUMB_WIDTH, THUMB_HEIGHT) {
            Ok(pixbuf) => thumbnails.push(TimelineThumbnail {
                timestamp_seconds: file.timestamp_seconds,
                pixbuf,
            }),
            Err(err) => eprintln!("failed to load thumbnail {}: {err}", file.path.display()),
        }
    }
    thumbnails
}

fn extract_thumbnail(source_path: &Path, timestamp: f64, output_path: &Path) -> bool {
    match Command::new("ffmpeg")
        .arg("-hide_banner")
        .arg("-loglevel")
        .arg("error")
        .arg("-y")
        .arg("-ss")
        .arg(format!("{timestamp:.3}"))
        .arg("-i")
        .arg(source_path)
        .arg("-frames:v")
        .arg("1")
        .arg("-vf")
        .arg(format!(
            "scale={THUMB_WIDTH}:{THUMB_HEIGHT}:force_original_aspect_ratio=decrease"
        ))
        .arg("-update")
        .arg("1")
        .arg(output_path)
        .status()
    {
        Ok(status) if status.success() => true,
        Ok(status) => {
            eprintln!("ffmpeg thumbnail extraction failed with status {status}");
            false
        }
        Err(err) => {
            eprintln!("failed to run ffmpeg for thumbnails: {err}");
            false
        }
    }
}

fn thumbnail_cache_dir(source_path: &Path, duration_seconds: f64) -> PathBuf {
    let mut hasher = DefaultHasher::new();
    source_path.hash(&mut hasher);
    duration_seconds.to_bits().hash(&mut hasher);
    if let Ok(metadata) = source_path.metadata() {
        metadata.len().hash(&mut hasher);
        if let Ok(modified) = metadata.modified() {
            modified.hash(&mut hasher);
        }
    }

    std::env::temp_dir()
        .join("gifbrewery-thumbnails")
        .join(format!("{:016x}", hasher.finish()))
}
