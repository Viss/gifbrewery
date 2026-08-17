use crate::timeline::TimelineThumbnail;
use gdk_pixbuf::Pixbuf;
use std::path::PathBuf;

const THUMB_WIDTH: i32 = 120;
const THUMB_HEIGHT: i32 = 68;

#[derive(Debug, Clone)]
pub struct ThumbnailFile {
    pub timestamp_seconds: f64,
    pub path: PathBuf,
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
