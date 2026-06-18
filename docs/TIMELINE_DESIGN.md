# Visual Timeline Design

Date: 2026-06-11

## Purpose

The current prototype has only a basic play/pause strip and a single GTK slider. That is not enough for a GIF Brewery clone. GIF Brewery 3's value is the visual editing surface: seeing the clip, trimming it, placing text/overlay ranges, and scrubbing precisely.

This document defines the target timeline so the next implementation pass does not accidentally settle for a VLC-style player control.

## Target Experience

The timeline should feel like a compact video editor, not a media player.

Required visible parts:

- Thumbnail filmstrip across the loaded media or selected clip.
- Draggable playhead line.
- Draggable trim handles for clip start/end.
- Highlighted selected clip range.
- Overlay timing lanes, starting with text overlays.
- Overlay bars with draggable start/end edges.
- Time ruler with readable tick labels.
- Current time and selected range readout.
- Zoom support for short/long clips.
- Horizontal scroll support for long clips.

The user should be able to answer these questions visually:

- What part of the video will become the GIF?
- Where is the current preview frame?
- When does each text overlay appear and disappear?
- How much of the source is excluded by trim handles?

## Non-Goals For First Pass

Do not build a complete non-linear editor.

The first real pass does not need:

- Multi-track compositing.
- Keyframes.
- Audio waveform.
- Transitions.
- Nested clips.
- Per-frame editing.

It does need to establish the correct visual structure so later features fit naturally.

## GTK Structure

Recommended initial structure:

- `TimelineView`: custom `gtk::DrawingArea` or subclassed `gtk::Widget`.
- `TimelineState`: shared model-adapter state derived from `Project`.
- `TimelineController`: gesture handling and callbacks into the app state.

For the first implementation, a `gtk::DrawingArea` is probably enough. A full subclass can wait until the event model gets complex.

Suggested files:

- `crates/gifbrewery-gtk/src/timeline.rs`
- `crates/gifbrewery-gtk/src/thumbnails.rs`

`ui.rs` should own layout and wire callbacks, but drawing and hit testing should move out of `ui.rs`.

## Data Model

Timeline view state should be independent from the serialized project model.

Suggested view state:

```rust
pub struct TimelineViewState {
    pub media_duration_seconds: f64,
    pub viewport_start_seconds: f64,
    pub viewport_duration_seconds: f64,
    pub playhead_seconds: f64,
    pub clip_start_seconds: f64,
    pub clip_end_seconds: f64,
    pub overlays: Vec<TimelineOverlayRange>,
    pub thumbnails: Vec<TimelineThumbnail>,
}

pub struct TimelineOverlayRange {
    pub id: String,
    pub label: String,
    pub start_seconds: f64,
    pub end_seconds: f64,
    pub selected: bool,
}

pub struct TimelineThumbnail {
    pub timestamp_seconds: f64,
    pub texture: gtk::gdk::Texture,
}
```

The serialized `Project` remains the source of truth. `TimelineViewState` is rebuilt or patched when:

- A source file is opened.
- Media metadata changes.
- Clip range changes.
- Overlay ranges change.
- Zoom/scroll changes.
- The playhead moves.

## Layout

Recommended vertical layout:

```text
------------------------------------------------
| time ruler: 0:00      0:01      0:02         |
------------------------------------------------
| overlay lane: [ caption text range       ]   |
------------------------------------------------
| filmstrip: |thumb|thumb|thumb|thumb|thumb|   |
| selected:      [=====================]       |
| playhead:                 |                  |
------------------------------------------------
```

Minimum dimensions:

- Timeline height: 116-150 px.
- Ruler height: 20 px.
- Overlay lane height: 26-32 px.
- Filmstrip height: 64-84 px.
- Trim handle width: 8-12 px.
- Playhead width: 2 px, with a small grab handle at the top.

The timeline should not resize based on label text. Use fixed lane heights.

## Coordinate Mapping

Use explicit mapping helpers. Do not scatter math through drawing and gestures.

```rust
fn seconds_to_x(seconds: f64, state: &TimelineViewState, width: f64) -> f64;
fn x_to_seconds(x: f64, state: &TimelineViewState, width: f64) -> f64;
```

Mapping:

- `viewport_start_seconds` maps to `x = 0`.
- `viewport_start_seconds + viewport_duration_seconds` maps to `x = width`.
- Clamp interaction results to `0..media_duration_seconds`.

Clip invariants:

- `clip_start_seconds >= 0`
- `clip_end_seconds <= media_duration_seconds`
- `clip_end_seconds > clip_start_seconds`
- Minimum clip length should be at least one frame or `0.01s`, whichever is larger.

Overlay invariants:

- Overlay start/end should clamp to the clip range by default.
- Later, allow overlays to exist outside selected clip only if there is a clear UX reason.

## Hit Testing

Timeline hit targets:

- Playhead.
- Clip start handle.
- Clip end handle.
- Selected clip body.
- Overlay bar body.
- Overlay start edge.
- Overlay end edge.
- Empty filmstrip/ruler area.

Suggested enum:

```rust
enum TimelineHit {
    Playhead,
    ClipStart,
    ClipEnd,
    ClipBody,
    OverlayStart(String),
    OverlayEnd(String),
    OverlayBody(String),
    Empty,
}
```

Interaction behavior:

- Click empty filmstrip: move playhead and seek preview.
- Drag playhead: continuously seek preview, preferably throttled.
- Drag clip start/end: update clip range, update inspector rows, clamp overlays if needed.
- Drag overlay start/end: update overlay timing and inspector rows.
- Drag overlay body: move whole overlay range without changing duration.

## Thumbnail Extraction

The first real filmstrip should use sampled frames, not CSS blocks.

Two viable approaches:

1. GStreamer path:
   - Use `uridecodebin` or `playbin` with `appsink`.
   - Seek to sampled timestamps.
   - Pull frames as RGB/RGBA buffers.
   - Convert to `gdk::MemoryTexture`.

2. ffmpeg helper path:
   - Shell out to `ffmpeg` for prototype thumbnail generation.
   - Write thumbnails to cache directory.
   - Load into `gdk::Texture`.
   - Replace later with in-process extraction if needed.

Recommended path:

- Use ffmpeg helper for the first filmstrip if it gets us visible progress faster.
- Keep it behind a `ThumbnailProvider` trait so GStreamer can replace it without touching timeline drawing.

Suggested trait:

```rust
pub trait ThumbnailProvider {
    fn request_thumbnails(
        &self,
        source_path: &str,
        duration_seconds: f64,
        target_count: usize,
    ) -> anyhow::Result<Vec<TimelineThumbnail>>;
}
```

Thumbnail sampling:

- Target one thumbnail every 80-120 px.
- For a 900 px timeline, request about 10-12 thumbnails.
- Recompute on zoom changes only when the thumbnail density is visibly insufficient.

Caching:

- Cache by source path, file mtime, duration, and thumbnail timestamp.
- Do not permanently cache abandoned temp files in the first pass.

## Drawing Order

Draw in this order:

1. Background.
2. Time ruler and tick labels.
3. Overlay lanes.
4. Filmstrip thumbnails.
5. Dimmed regions outside selected clip.
6. Selected clip outline.
7. Trim handles.
8. Overlay selected outlines.
9. Playhead.
10. Hover/drag affordances.

Use GTK theme colors where possible, with restrained accent usage.

## Integration With Preview

The timeline playhead should drive the preview player.

Expected callbacks:

```rust
pub struct TimelineCallbacks {
    pub on_seek: Box<dyn Fn(f64)>,
    pub on_clip_changed: Box<dyn Fn(f64, f64)>,
    pub on_overlay_changed: Box<dyn Fn(String, f64, f64)>,
}
```

For scrubbing:

- During drag, call `on_seek` at throttled intervals.
- On release, call `on_seek` with exact final position.
- Keep the timeline visually responsive even if preview seeking lags.

## Inspector Synchronization

Inspector rows and timeline handles must stay in sync.

Short-term approach:

- When timeline mutates clip/overlay state, update model and row widgets directly.
- When row widgets mutate state, update model and queue a timeline redraw.

Long-term approach:

- Introduce a small app-state event/update layer:
  - `AppAction::SetClipRange`
  - `AppAction::SetPlayhead`
  - `AppAction::SetOverlayRange`
  - `AppAction::SetOverlayText`

Avoid having every widget mutate `Project` independently.

## Visual Smoke Tests

Current smoke testing only captures screenshots.

Add checks for the real timeline:

- Screenshot contains non-black/non-flat filmstrip area after media load.
- Clip end row equals timeline selected range end.
- Overlay disappear row equals overlay bar end.
- Playhead moves after clicking or dragging timeline.
- Screenshot after GIF/Overlays tab switches still shows timeline intact.

The script should continue writing to:

- `/code/gifbrewery-visual-smoke`

The user explicitly asked for this so they can watch progress.

## First Implementation Milestone

Build a visual timeline without thumbnails first, but with correct geometry:

- Replace `gtk::Scale` with `TimelineView`.
- Draw a fixed-height filmstrip area with placeholder frame cells.
- Draw clip selection and handles.
- Draw playhead.
- Draw one text overlay bar.
- Support click-to-seek.
- Support dragging clip handles.
- Keep `cargo check` clean.
- Refresh screenshots.

This is acceptable only as a stepping stone. It should look structurally like GIF Brewery's timeline, but it does not satisfy the final requirement until thumbnails exist.

## Second Implementation Milestone

Add real thumbnails:

- Implement `ThumbnailProvider`.
- Generate 10-12 sampled thumbnails from loaded media.
- Render thumbnails in the filmstrip.
- Cache thumbnails for the current source during the app session.
- Add visual smoke check for non-placeholder frame content.

## Third Implementation Milestone

Make overlays editable from the timeline:

- Show text overlay bar.
- Drag overlay start/end.
- Drag overlay body.
- Select overlay from timeline.
- Synchronize with overlay inspector rows.

## Key Risk

The biggest design risk is mixing preview playback, model mutation, thumbnail extraction, and timeline drawing into `ui.rs`. Avoid that. The timeline needs its own module and a clean callback boundary, otherwise every later GIF Brewery feature will become harder to add.
