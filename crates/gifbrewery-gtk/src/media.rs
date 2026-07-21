use gstreamer as gst;
use gstreamer_pbutils as gst_pbutils;
use gtk::gio;
use gtk::prelude::FileExt;
use std::process::Command;

#[derive(Debug, Clone, Default)]
pub struct MediaMetadata {
    pub duration_seconds: Option<f64>,
    pub width: Option<u32>,
    pub height: Option<u32>,
    pub fps: Option<f64>,
}

pub fn discover(file: &gio::File) -> Result<MediaMetadata, glib::Error> {
    let discoverer = gst_pbutils::Discoverer::new(gst::ClockTime::from_seconds(5))?;
    let info = discoverer.discover_uri(file.uri().as_str())?;
    let video = info.video_streams().into_iter().next();

    let fps = video.as_ref().and_then(|stream| {
        let framerate = stream.framerate();
        let denom = framerate.denom();
        if denom > 0 {
            Some(f64::from(framerate.numer()) / f64::from(denom))
        } else {
            None
        }
    });

    let aggregate_duration = info.duration().map(gst::ClockTime::seconds_f64);
    let video_duration = file
        .path()
        .and_then(|path| probe_video_duration_seconds(&path));

    Ok(MediaMetadata {
        duration_seconds: video_duration.or(aggregate_duration),
        width: video.as_ref().map(|stream| stream.width()),
        height: video.as_ref().map(|stream| stream.height()),
        fps,
    })
}

fn probe_video_duration_seconds(path: &std::path::Path) -> Option<f64> {
    let output = Command::new("ffprobe")
        .arg("-v")
        .arg("error")
        .arg("-select_streams")
        .arg("v:0")
        .arg("-show_entries")
        .arg("stream=duration")
        .arg("-of")
        .arg("default=nw=1:nk=1")
        .arg(path)
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    parse_first_positive_float(std::str::from_utf8(&output.stdout).ok()?)
}

fn parse_first_positive_float(output: &str) -> Option<f64> {
    output
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty() && *line != "N/A")
        .find_map(|line| line.parse::<f64>().ok().filter(|value| *value > 0.0))
}

#[cfg(test)]
mod tests {
    use super::parse_first_positive_float;

    #[test]
    fn parses_ffprobe_duration() {
        assert_eq!(parse_first_positive_float("1.583333\n"), Some(1.583333));
    }

    #[test]
    fn ignores_missing_ffprobe_duration() {
        assert_eq!(parse_first_positive_float("N/A\n"), None);
        assert_eq!(parse_first_positive_float("\n"), None);
    }
}
