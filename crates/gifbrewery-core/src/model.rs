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
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct MediaSource {
    pub path: String,
    pub duration_seconds: Option<f64>,
    pub natural_width: Option<u32>,
    pub natural_height: Option<u32>,
    #[serde(default)]
    pub fps: Option<f64>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Clip {
    pub name: String,
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
    pub output_width: Option<u32>,
    #[serde(default)]
    pub output_height: Option<u32>,
}

impl Default for GifExportSettings {
    fn default() -> Self {
        Self {
            colors: 256,
            optimize: false,
            high_quality_quantization: true,
            target_max_bytes: None,
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
