use gstreamer as gst;
use gstreamer_pbutils as gst_pbutils;
use gtk::gio;
use gtk::prelude::FileExt;

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

    Ok(MediaMetadata {
        duration_seconds: info.duration().map(gst::ClockTime::seconds_f64),
        width: video.as_ref().map(|stream| stream.width()),
        height: video.as_ref().map(|stream| stream.height()),
        fps,
    })
}
