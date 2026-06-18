# Porting Notes

## Product Target

The target is a native GNOME/Wayland GIF editor that replicates GIF Brewery 3's
editing workflow as closely as is practical on Linux. The GUI is the product:
preview, timeline, captions, colors, overlay timing, crop/resize, filters, and
export controls must all be exposed visually.

## Source Applications

### GIF Brewery 3

Bundle inspected: `GIF Brewery 3.app`

Observed facts:

- Version: `3.9.5`, build `40054`.
- Binary: 64-bit x86_64 Mach-O.
- macOS SDK: 10.14.
- Main frameworks: AppKit, AVFoundation, AVKit, CoreMedia, CoreVideo, ImageIO,
  Quartz, QuartzCore, CoreGraphics, CoreText, CoreData, WebKit, CoreMediaIO.
- Credited libraries: GIFLIB, exoquant, kdtree, AFNetworking, AFOAuth2Manager,
  RXPromise, XCDYouTubeKit, KVOController, libextobjc.

Inferred implementation shape:

- AVFoundation extracts frames and creates MP4 output.
- CoreImage/Quartz/CoreText render filters and overlays.
- GIFLIB/ImageIO/exoquant/kdtree support GIF creation, quantization, and palette application.
- AppKit nibs/storyboards define the main editor, frames panel, overlays panel,
  GIF properties, MP4 creation, recording, media joiner, and Gfycat upload screens.

### Gifcurry

Reference clone: `reference/gifcurry`

Useful behavior references:

- Text overlays have text, font family/style/stretch/weight/size, origin,
  x/y translation, rotation, start/end time, outline size/color, and fill color.
- Export settings include input/output path, start/end seconds, width, FPS,
  color count, dithering, crop amounts, and output as GIF or video.
- Validation rules are straightforward and useful for UI constraints:
  FPS range, color count range, crop bounds, overlay timing, and text style values.

Implementation note: Gifcurry is reference-only. Do not copy UI code into the new
application.

### LosslessCut

Useful reference only for timeline/segment workflow, keyframe awareness,
thumbnail generation, waveform generation, and FFmpeg command strategy.

LosslessCut is not a suitable base for this project because the desired target is
native GTK/libadwaita, not Electron.

## Feature Map

### Main Editor

- Open video, GIF, image, and project files.
- Preview media with play/pause, frame step, scrubber, and set start/end actions.
- Show selected range duration and output dimensions.
- Save project state.

### Clip Properties

- Start time and end time.
- Frame count, frame delay, or FPS mode.
- Speed.
- Forward, reverse, and palindrome/looping clip modes.
- Crop and resize.

### GIF Properties

- Color count.
- Palette algorithm.
- High-quality quantization toggle.
- Optimize GIF toggle.
- Always-on GIF looping.
- Create from current clip or saved frames.

### Overlays

- Text captions.
- Image/sticker overlays.
- Start and end times per overlay.
- Drag/resize/transform handles in preview.
- Text color, stroke color, stroke width, shadow, background, alignment, font.
- Blend modes observed in UI: Normal, Screen, Overlay, Color Dodge, Color Burn.

### Frames

- Preview generated frames.
- Save current clip frames.
- Export frames as PNGs.
- Set all image frame delays.

### Filters

- Crop.
- Resize and canvas resize.
- Color controls.
- Sepia, monochrome, comic effect, dot screen.
- Hue, gamma, sharpness, blur/radius/intensity controls.

### Capture

- Screen area recording.
- Window recording.
- Camera/video recording.
- Cursor and mouse click recording options.
- Default audio input option.

Linux implementation should use PipeWire and xdg-desktop-portal for Wayland capture.

### Media Joiner

- Add images, GIFs, and videos.
- Reorder/remove media.
- Set image delays.
- Stitch to GIF or MP4.

### Upload/Import Integrations

Gfycat, YouTube, Twitch Clips, Streamable, Twitter, Instagram, and Facebook appear
in the original app. These should be deferred until the local editor/exporter is
credible.

## Milestones

1. Native app shell: GTK 4/libadwaita window, preview area, timeline, inspector.
2. Open video and show playback with GStreamer.
3. Trim range and scrubber.
4. Text overlay creation, inspector editing, and preview placement.
5. GIF export from current range with overlay rendered.
6. Crop/resize/color/FPS/GIF settings.
7. Saved frames and PNG export.
8. Filters.
9. Media joiner.
10. Wayland screen/window recording.
11. Optional web/import/upload integrations.
