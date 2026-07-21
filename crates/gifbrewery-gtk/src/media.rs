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
    let ffprobe = file.path().and_then(|path| probe_video_metadata(&path));
    let discoverer = gst_pbutils::Discoverer::new(gst::ClockTime::from_seconds(5))?;
    let info = match discoverer.discover_uri(file.uri().as_str()) {
        Ok(info) => info,
        Err(err) => {
            if let Some(metadata) = ffprobe {
                return Ok(metadata);
            }
            return Err(err);
        }
    };
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

    Ok(MediaMetadata {
        duration_seconds: ffprobe
            .as_ref()
            .and_then(|metadata| metadata.duration_seconds)
            .or(aggregate_duration),
        width: ffprobe
            .as_ref()
            .and_then(|metadata| metadata.width)
            .or_else(|| video.as_ref().map(|stream| stream.width())),
        height: ffprobe
            .as_ref()
            .and_then(|metadata| metadata.height)
            .or_else(|| video.as_ref().map(|stream| stream.height())),
        fps: ffprobe.as_ref().and_then(|metadata| metadata.fps).or(fps),
    })
}

fn probe_video_metadata(path: &std::path::Path) -> Option<MediaMetadata> {
    let output = Command::new("ffprobe")
        .arg("-v")
        .arg("error")
        .arg("-select_streams")
        .arg("v:0")
        .arg("-show_entries")
        .arg("stream=width,height,r_frame_rate,avg_frame_rate,duration")
        .arg("-of")
        .arg("default=nw=1")
        .arg(path)
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    parse_ffprobe_video_metadata(std::str::from_utf8(&output.stdout).ok()?)
}

fn parse_positive_u32(value: &str) -> Option<u32> {
    value.trim().parse::<u32>().ok().filter(|value| *value > 0)
}

fn parse_positive_float(value: &str) -> Option<f64> {
    value
        .trim()
        .parse::<f64>()
        .ok()
        .filter(|value| value.is_finite() && *value > 0.0)
}

fn parse_frame_rate(value: &str) -> Option<f64> {
    let value = value.trim();
    if value.is_empty() || value == "N/A" || value == "0/0" {
        return None;
    }
    if let Some((numer, denom)) = value.split_once('/') {
        let numer = numer.parse::<f64>().ok()?;
        let denom = denom.parse::<f64>().ok()?;
        if denom > 0.0 && numer > 0.0 {
            return Some(numer / denom);
        }
        return None;
    }
    parse_positive_float(value)
}

fn parse_ffprobe_video_metadata(output: &str) -> Option<MediaMetadata> {
    let mut metadata = MediaMetadata::default();
    let mut r_frame_rate = None;
    let mut avg_frame_rate = None;

    for line in output.lines().map(str::trim) {
        let Some((key, value)) = line.split_once('=') else {
            continue;
        };
        match key {
            "width" => metadata.width = parse_positive_u32(value),
            "height" => metadata.height = parse_positive_u32(value),
            "duration" => metadata.duration_seconds = parse_positive_float(value),
            "r_frame_rate" => r_frame_rate = parse_frame_rate(value),
            "avg_frame_rate" => avg_frame_rate = parse_frame_rate(value),
            _ => {}
        }
    }

    metadata.fps = avg_frame_rate.or(r_frame_rate);
    (metadata.duration_seconds.is_some()
        || metadata.width.is_some()
        || metadata.height.is_some()
        || metadata.fps.is_some())
    .then_some(metadata)
}

#[cfg(test)]
mod tests {
    use super::{parse_ffprobe_video_metadata, parse_frame_rate};

    #[test]
    fn parses_frame_rates() {
        assert_eq!(parse_frame_rate("24/1"), Some(24.0));
        assert_eq!(parse_frame_rate("24000/1001"), Some(24000.0 / 1001.0));
        assert_eq!(parse_frame_rate("0/0"), None);
        assert_eq!(parse_frame_rate("N/A"), None);
    }

    #[test]
    fn parses_ffprobe_video_metadata() {
        let metadata = parse_ffprobe_video_metadata(
            "width=3840\nheight=2160\nr_frame_rate=24/1\navg_frame_rate=24/1\nduration=2.083333\n",
        )
        .expect("metadata");

        assert_eq!(metadata.width, Some(3840));
        assert_eq!(metadata.height, Some(2160));
        assert_eq!(metadata.fps, Some(24.0));
        assert_eq!(metadata.duration_seconds, Some(2.083333));
    }
}
