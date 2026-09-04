use serde::{Deserialize, Serialize};
use thiserror::Error;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Project {
    pub version: u32,
    pub source: Option<MediaSource>,
    pub clips: Vec<Clip>,
    pub overlays: Vec<Overlay>,
    pub settings: ProjectSettings,
}

impl Default for Project {
    fn default() -> Self {
        Self {
            version: 1,
            source: None,
            clips: vec![Clip::default()],
            overlays: Vec::new(),
            settings: ProjectSettings::default(),
        }
    }
}

impl Project {
    pub fn validate(&self) -> Result<(), ProjectError> {
        if self.clips.is_empty() {
            return Err(ProjectError::NoClips);
        }

        for clip in &self.clips {
            clip.range.validate()?;
        }

        for overlay in &self.overlays {
            overlay.range().validate()?;
        }

        Ok(())
    }

    pub fn trim_to_clip_selection(&mut self, playhead_seconds: f64) -> Option<f64> {
        let selection = self.clips.first()?.range;
        let selection_start = selection.start_seconds;
        let fps = self.source.as_ref()?.fps.filter(|fps| *fps > 0.0)?;

        let retained_frames = &self.clips.first()?.retained_source_frames;
        let trimmed_frame_map = if retained_frames.is_empty() {
            None
        } else {
            let start_frame = (selection.start_seconds * fps).round().max(0.0) as usize;
            let end_frame = (selection.end_seconds * fps).round().max(0.0) as usize;
            let start_frame = start_frame.min(retained_frames.len().saturating_sub(1));
            let end_frame = end_frame.clamp(start_frame + 1, retained_frames.len());
            let mut frames = retained_frames[start_frame..end_frame].to_vec();
            let first_source_frame = *frames.first()?;
            for frame in &mut frames {
                *frame -= first_source_frame;
            }
            Some((frames, first_source_frame))
        };
        let duration = trimmed_frame_map
            .as_ref()
            .map(|(frames, _)| frames.len() as f64 / fps)
            .unwrap_or_else(|| selection.duration_seconds())
            .max(1.0 / fps);

        let clip = self.clips.first_mut()?;
        if let Some((frames, first_source_frame)) = trimmed_frame_map {
            clip.source_offset_seconds += first_source_frame as f64 / fps;
            clip.retained_source_frames = frames;
        } else {
            clip.source_offset_seconds += selection_start;
        }
        clip.range = TimelineRange {
            start_seconds: 0.0,
            end_seconds: duration,
        };
        if let Some(source) = self.source.as_mut() {
            source.duration_seconds = Some(duration);
        }

        self.overlays.retain_mut(|overlay| match overlay {
            Overlay::Text(text) => {
                let start = text.range.start_seconds.max(selection.start_seconds);
                let end = text.range.end_seconds.min(selection.end_seconds);
                if end <= start {
                    return false;
                }
                text.range = TimelineRange {
                    start_seconds: start - selection_start,
                    end_seconds: end - selection_start,
                };
                true
            }
        });

        Some((playhead_seconds - selection_start).clamp(0.0, duration))
    }

    pub fn delete_timeline_frame(&mut self, frame_index: usize) -> Option<FrameDeletion> {
        let fps = self.source.as_ref()?.fps.filter(|fps| *fps > 0.0)?;
        let source_duration = self.source.as_ref()?.duration_seconds?;
        let frame_duration = 1.0 / fps;

        let current_frame_count = {
            let clip = self.clips.first()?;
            if clip.retained_source_frames.is_empty() {
                (source_duration * fps).round().max(1.0) as usize
            } else {
                clip.retained_source_frames.len()
            }
        };
        if current_frame_count <= 1 {
            return None;
        }
        let frame_index = frame_index.min(current_frame_count - 1);
        let deleted_at = frame_index as f64 / fps;

        let clip = self.clips.first_mut()?;
        if clip.retained_source_frames.is_empty() {
            clip.retained_source_frames = (0..current_frame_count as u64).collect();
        }
        let source_frame = clip.retained_source_frames.remove(frame_index);
        let new_frame_count = clip.retained_source_frames.len();
        let new_duration = new_frame_count as f64 / fps;

        if deleted_at < clip.range.start_seconds {
            clip.range.start_seconds = (clip.range.start_seconds - frame_duration).max(0.0);
            clip.range.end_seconds = (clip.range.end_seconds - frame_duration).max(frame_duration);
        } else if deleted_at < clip.range.end_seconds {
            clip.range.end_seconds = (clip.range.end_seconds - frame_duration)
                .max(clip.range.start_seconds + frame_duration);
        }
        clip.range.end_seconds = clip.range.end_seconds.min(new_duration);
        clip.range.start_seconds = clip
            .range
            .start_seconds
            .min((clip.range.end_seconds - frame_duration).max(0.0));

        if let Some(source) = self.source.as_mut() {
            source.duration_seconds = Some(new_duration);
        }

        self.overlays.retain_mut(|overlay| {
            let range = match overlay {
                Overlay::Text(text) => &mut text.range,
            };
            if range.end_seconds <= deleted_at {
                return true;
            }
            if range.start_seconds > deleted_at {
                range.start_seconds = (range.start_seconds - frame_duration).max(0.0);
            }
            range.end_seconds = (range.end_seconds - frame_duration).min(new_duration);
            range.end_seconds > range.start_seconds
        });

        Some(FrameDeletion {
            timeline_frame: frame_index,
            source_frame,
            new_frame_count,
            new_duration_seconds: new_duration,
            new_playhead_seconds: frame_index.min(new_frame_count - 1) as f64 / fps,
        })
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct FrameDeletion {
    pub timeline_frame: usize,
    pub source_frame: u64,
    pub new_frame_count: usize,
    pub new_duration_seconds: f64,
    pub new_playhead_seconds: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct MediaSource {
    pub path: String,
    pub duration_seconds: Option<f64>,
    pub natural_width: Option<u32>,
    pub natural_height: Option<u32>,
    #[serde(default)]
    pub fps: Option<f64>,
    #[serde(default)]
    pub color_space: Option<String>,
    #[serde(default)]
    pub color_transfer: Option<String>,
    #[serde(default)]
    pub color_primaries: Option<String>,
    #[serde(default)]
    pub pixel_format: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Clip {
    pub name: String,
    #[serde(default)]
    pub source_offset_seconds: f64,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub retained_source_frames: Vec<u64>,
    pub range: TimelineRange,
    pub frame_strategy: FrameStrategy,
    pub speed: f64,
    pub loop_mode: ClipLoopMode,
    pub crop: Option<CropRect>,
}

impl Default for Clip {
    fn default() -> Self {
        Self {
            name: "Current Clip".to_string(),
            source_offset_seconds: 0.0,
            retained_source_frames: Vec::new(),
            range: TimelineRange {
                start_seconds: 0.0,
                end_seconds: 3.0,
            },
            frame_strategy: FrameStrategy::Fps(0),
            speed: 1.0,
            loop_mode: ClipLoopMode::Forward,
            crop: None,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum FrameStrategy {
    Fps(u32),
    Count(u32),
    DelayMillis(u32),
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub enum ClipLoopMode {
    Forward,
    Reverse,
    Palindrome,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum Overlay {
    Text(TextOverlay),
}

impl Overlay {
    pub fn range(&self) -> TimelineRange {
        match self {
            Overlay::Text(text) => text.range,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct TextOverlay {
    pub id: String,
    pub text: String,
    pub range: TimelineRange,
    pub bounds: Rect,
    pub font_family: String,
    pub font_size: f64,
    pub font_weight: u32,
    #[serde(default = "default_text_alignment")]
    pub alignment: TextAlignment,
    pub text_color: RgbaColor,
    pub stroke_color: RgbaColor,
    pub stroke_width: f64,
    pub shadow_enabled: bool,
    pub background_color: Option<RgbaColor>,
    pub blend_mode: BlendMode,
}

impl TextOverlay {
    pub fn default_caption() -> Self {
        Self {
            id: "caption-1".to_string(),
            text: "Lorem ipsum.".to_string(),
            range: TimelineRange {
                start_seconds: 0.0,
                end_seconds: 3.0,
            },
            bounds: Rect {
                x: 0.1,
                y: 0.72,
                width: 0.8,
                height: 0.18,
            },
            font_family: "Sans".to_string(),
            font_size: 32.0,
            font_weight: 700,
            alignment: TextAlignment::Center,
            text_color: RgbaColor::WHITE,
            stroke_color: RgbaColor::BLACK,
            stroke_width: 1.0,
            shadow_enabled: false,
            background_color: None,
            blend_mode: BlendMode::Normal,
        }
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub enum TextAlignment {
    Left,
    Center,
}

fn default_text_alignment() -> TextAlignment {
    TextAlignment::Center
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq)]
pub struct TimelineRange {
    pub start_seconds: f64,
    pub end_seconds: f64,
}

impl TimelineRange {
    pub fn duration_seconds(self) -> f64 {
        self.end_seconds - self.start_seconds
    }

    pub fn validate(self) -> Result<(), ProjectError> {
        if self.start_seconds < 0.0 {
            return Err(ProjectError::NegativeStartTime);
        }

        if self.end_seconds <= self.start_seconds {
            return Err(ProjectError::InvalidRange);
        }

        Ok(())
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq)]
pub struct Rect {
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq)]
pub struct CropRect {
    pub left: f64,
    pub right: f64,
    pub top: f64,
    pub bottom: f64,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq)]
pub struct RgbaColor {
    pub red: f64,
    pub green: f64,
    pub blue: f64,
    pub alpha: f64,
}

impl RgbaColor {
    pub const BLACK: Self = Self {
        red: 0.0,
        green: 0.0,
        blue: 0.0,
        alpha: 1.0,
    };

    pub const WHITE: Self = Self {
        red: 1.0,
        green: 1.0,
        blue: 1.0,
        alpha: 1.0,
    };
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub enum BlendMode {
    Normal,
    Screen,
    Overlay,
    ColorDodge,
    ColorBurn,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ProjectSettings {
    pub gif: GifExportSettings,
}

impl Default for ProjectSettings {
    fn default() -> Self {
        Self {
            gif: GifExportSettings::default(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct GifExportSettings {
    pub colors: u16,
    pub optimize: bool,
    pub high_quality_quantization: bool,
    pub target_max_bytes: Option<u64>,
    #[serde(default)]
    pub tone_map_hdr: bool,
    #[serde(default)]
    pub output_width: Option<u32>,
    #[serde(default)]
    pub output_height: Option<u32>,
}

impl Default for GifExportSettings {
    fn default() -> Self {
        Self {
            colors: 256,
            optimize: true,
            high_quality_quantization: false,
            target_max_bytes: None,
            tone_map_hdr: false,
            output_width: None,
            output_height: None,
        }
    }
}

#[derive(Debug, Error, Clone, PartialEq)]
pub enum ProjectError {
    #[error("project has no clips")]
    NoClips,
    #[error("timeline start time cannot be negative")]
    NegativeStartTime,
    #[error("timeline end time must be greater than start time")]
    InvalidRange,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn trimming_rebases_source_timeline_playhead_and_overlays() {
        let mut project = Project::default();
        project.source = Some(MediaSource {
            path: "source.webm".to_string(),
            duration_seconds: Some(10.0),
            natural_width: Some(1280),
            natural_height: Some(720),
            fps: Some(30_000.0 / 1001.0),
            color_space: None,
            color_transfer: None,
            color_primaries: None,
            pixel_format: None,
        });
        let clip = project.clips.first_mut().unwrap();
        clip.source_offset_seconds = 2.0;
        clip.range = TimelineRange {
            start_seconds: 3.0,
            end_seconds: 7.0,
        };

        let mut kept = TextOverlay::default_caption();
        kept.id = "kept".to_string();
        kept.range = TimelineRange {
            start_seconds: 4.0,
            end_seconds: 6.0,
        };
        let mut discarded = TextOverlay::default_caption();
        discarded.id = "discarded".to_string();
        discarded.range = TimelineRange {
            start_seconds: 7.5,
            end_seconds: 8.5,
        };
        project.overlays = vec![Overlay::Text(kept), Overlay::Text(discarded)];

        let playhead = project.trim_to_clip_selection(5.0).unwrap();

        assert_eq!(playhead, 2.0);
        assert_eq!(project.source.as_ref().unwrap().duration_seconds, Some(4.0));
        let clip = project.clips.first().unwrap();
        assert_eq!(clip.source_offset_seconds, 5.0);
        assert_eq!(clip.range.start_seconds, 0.0);
        assert_eq!(clip.range.end_seconds, 4.0);
        assert_eq!(project.overlays.len(), 1);
        let Overlay::Text(text) = &project.overlays[0];
        assert_eq!(text.id, "kept");
        assert_eq!(text.range.start_seconds, 1.0);
        assert_eq!(text.range.end_seconds, 3.0);
    }

    #[test]
    fn deleting_frames_builds_one_retained_source_map_and_closes_timing_gaps() {
        let mut project = Project::default();
        project.source = Some(MediaSource {
            path: "source.mp4".to_string(),
            duration_seconds: Some(1.0),
            natural_width: Some(640),
            natural_height: Some(360),
            fps: Some(10.0),
            color_space: None,
            color_transfer: None,
            color_primaries: None,
            pixel_format: None,
        });
        project.clips[0].range = TimelineRange {
            start_seconds: 0.0,
            end_seconds: 1.0,
        };
        let mut overlay = TextOverlay::default_caption();
        overlay.range = TimelineRange {
            start_seconds: 0.5,
            end_seconds: 0.9,
        };
        project.overlays = vec![Overlay::Text(overlay)];

        let first = project.delete_timeline_frame(2).unwrap();
        let second = project.delete_timeline_frame(2).unwrap();

        assert_eq!(first.source_frame, 2);
        assert_eq!(second.source_frame, 3);
        assert_eq!(second.new_frame_count, 8);
        assert_eq!(
            project.clips[0].retained_source_frames,
            vec![0, 1, 4, 5, 6, 7, 8, 9]
        );
        assert!(
            (project.source.as_ref().unwrap().duration_seconds.unwrap() - 0.8).abs() < 0.000_001
        );
        assert!((project.clips[0].range.end_seconds - 0.8).abs() < 0.000_001);
        let Overlay::Text(overlay) = &project.overlays[0];
        assert!((overlay.range.start_seconds - 0.3).abs() < 0.000_001);
        assert!((overlay.range.end_seconds - 0.7).abs() < 0.000_001);
    }

    #[test]
    fn trimming_slices_and_rebases_an_existing_retained_frame_map() {
        let mut project = Project::default();
        project.source = Some(MediaSource {
            path: "source.mp4".to_string(),
            duration_seconds: Some(1.0),
            natural_width: Some(640),
            natural_height: Some(360),
            fps: Some(10.0),
            color_space: None,
            color_transfer: None,
            color_primaries: None,
            pixel_format: None,
        });
        project.clips[0].range = TimelineRange {
            start_seconds: 0.0,
            end_seconds: 1.0,
        };
        project.delete_timeline_frame(2).unwrap();
        project.clips[0].range = TimelineRange {
            start_seconds: 0.1,
            end_seconds: 0.5,
        };

        let playhead = project.trim_to_clip_selection(0.3).unwrap();

        assert!((playhead - 0.2).abs() < 0.000_001);
        assert!((project.clips[0].source_offset_seconds - 0.1).abs() < 0.000_001);
        assert_eq!(project.clips[0].retained_source_frames, vec![0, 2, 3, 4]);
        assert!((project.clips[0].range.end_seconds - 0.4).abs() < 0.000_001);
        assert!(
            (project.source.as_ref().unwrap().duration_seconds.unwrap() - 0.4).abs() < 0.000_001
        );
    }
}
