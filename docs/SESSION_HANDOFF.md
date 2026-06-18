# GIF Brewery Linux Clone - Session Handoff

Date: 2026-06-13

## Goal

Build a native GNOME/Wayland Linux clone of GIF Brewery 3 for Ubuntu 26.04-style desktops. The user explicitly does not want an Electron/Tauri/webapp wrapper, Flatpak, or Snap. Target delivery should be a `.deb` and/or single native executable. The GUI is required and is the product.

## Source Reference

The original app bundle was copied into this workspace:

- `/codex/gifbrewery/GIF Brewery 3.app`
- Original watched/shared copy: `/code/GIF Brewery 3.app`

Reverse-engineering notes are in:

- `docs/PORTING_NOTES.md`

Important observed GIF Brewery 3 capabilities:

- Video/GIF/image import
- Video preview and timeline/scrubber
- Clip start/end
- FPS, frame count, frame delay controls
- Speed changes
- Loop/reverse/palindrome modes
- Crop, resize, canvas resize
- Text/image/sticker overlays
- Overlay timing, font, colors, stroke, shadow, background, alignment, blend modes
- Filters
- Saved frames/PNG export
- Media joiner
- Screen/window/video recording
- Gfycat/upload/import integrations

Open-source references in workspace:

- `reference/gifcurry`

Product direction notes from the user:

- This should feel like LosslessCut plus Instagiffer: open a video, trim to a snippet, crop if needed, add styled text, and export a high-quality loop.
- Do not optimize around turning colors down. Default GIF export should keep 256 colors and high-quality quantization.
- Social posting constraints matter more than palette reduction. Current Mastodon documentation says animated GIF/GIFV uploads are up to 16 MB and under 1 megapixel, and soundless MP4/WebM files can loop like animated GIFs. Videos are documented up to 99 MB and 120 fps.

## Architecture Chosen

Rust workspace with a separate model/core crate and native GTK/libadwaita frontend:

- `crates/gifbrewery-core`: typed project/media/timeline/overlay/export model.
- `crates/gifbrewery-gtk`: GTK 4/libadwaita native GNOME UI.

This keeps the project aligned with the user's native GNOME/Wayland requirement and avoids web UI wrappers.

## Current Files Of Interest

- `Cargo.toml`: workspace root.
- `crates/gifbrewery-core/src/model.rs`: serializable project model.
- `crates/gifbrewery-gtk/src/main.rs`: libadwaita app startup, file-open handling, GStreamer init.
- `crates/gifbrewery-gtk/src/ui.rs`: current editor shell, inspector tabs, metadata UI wiring, preview-player UI integration.
- `crates/gifbrewery-gtk/src/media.rs`: media discovery and new `VideoPreview` wrapper.
- `tools/visual-smoke.sh`: headless Xvfb screenshot script writing to `/code/gifbrewery-visual-smoke`.
- `debian/`: early package skeleton.

## Installed System Dependencies

Development and visual inspection packages installed during this work:

- Rust/Cargo/rustfmt
- `pkg-config`
- `libgtk-4-dev`
- `libadwaita-1-dev`
- `libgstreamer1.0-dev`
- `libgstreamer-plugins-base1.0-dev`
- `gstreamer1.0-gtk4`
- `xvfb`
- `x11-apps`
- `imagemagick`
- `dbus-x11`
- `xdotool`
- `file`
- `gstreamer1.0-tools`
- `gstreamer1.0-plugins-good`
- `gstreamer1.0-plugins-bad`
- `gstreamer1.0-plugins-ugly`
- `gstreamer1.0-libav`
- `ffmpeg`

The important runtime finding: metadata discovery did not populate duration/dimensions until the GStreamer runtime plugin sets were installed. After installing them, the sample MP4 was detected as:

- `loading.mp4`
- Duration: about `0.79s`
- Dimensions: `2560 x 1440`
- FPS: `10.00`

## Verified Commands

Clean as of this handoff:

```bash
cargo fmt
cargo check
```

Last known `cargo check` status:

```text
Finished `dev` profile [unoptimized + debuginfo] target(s) in 0.14s
```

Visual smoke command:

```bash
tools/visual-smoke.sh
```

This writes:

- `/code/gifbrewery-visual-smoke/clip.png`
- `/code/gifbrewery-visual-smoke/gif.png`
- `/code/gifbrewery-visual-smoke/overlays.png`

Known headless warning:

```text
libEGL warning: DRI3 error: Could not get DRI3 device
```

This is expected under Xvfb/software rendering and has not blocked screenshot capture.

## Current UI State

The native GTK app currently has:

- Header bar with open button and `Create GIF` button.
- Main preview/editor area.
- Timeline strip with restart/play/pause buttons and a visual editor timeline.
- Inspector tabs:
  - Clip
  - GIF
  - Overlays
- Source badge over the preview.
- Default centered text overlay.
- File-open via header button and `HANDLES_OPEN` command-line open.

Metadata wiring works:

- Opening a media file updates source title/detail.
- Source detail shows path, duration, dimensions, and FPS when available.
- Clip range is clamped to the discovered media duration.
- Timeline label/range is updated.
- Timeline click/drag seeks the preview player.
- Timeline draws a time ruler, playhead, text overlay lane, selected clip range, trim handles, and decoded frame thumbnails.
- Clip inspector `Start` and `End` rows update the project range and timeline.
- Overlay inspector `Appears` and `Disappears` rows update the first text overlay timing and clamp to the current clip.
- Overlay inspector `Text`, `Font`, `Text color`, `Stroke color`, `Font size`, `Stroke width`, and `Shadow` rows update the first text overlay model.
- Overlay inspector `Text`, `Font`, `Text color`, `Font size`, and `Shadow` update the visible preview label.
- GIF tab has a Mastodon target size row defaulting to 16 MB. The format selector was removed; this port exports GIF.
- Default text overlay disappear time is clamped to the discovered media duration.
- `Create GIF` button becomes enabled after opening media.
- `Create GIF` runs the first-pass GIF exporter and writes `/code/gifbrewery-export.gif`.
- The exporter accepts video or GIF input, trims to the current clip, applies the current FPS strategy, uses `palettegen`/`paletteuse`, and burns the first text overlay with font, fill color, stroke color, and stroke width.
- GIF export always writes an infinite loop; there is no UI or model setting to disable looping.

The last visually verified screenshot after the visual timeline and thumbnail changes showed:

- Source badge: `0.79s | 2560 x 1440 | 10.00 fps`
- Timeline: `0.00s - 0.79s`
- Video frame visible in the preview via `gtk4paintablesink`, not a black placeholder.
- Real decoded frame thumbnails in the filmstrip, generated with `ffmpeg`.
- Text overlay bar in the timeline.
- Clip End: `0.79`
- Overlay Disappears: `0.79`

## Most Recent Change

The newest code changes:

- Updated export defaults:
  - GIF colors default to `256`.
  - High-quality quantization defaults to enabled.
  - Mastodon target max size defaults to `16 MB`.
- Removed the old output format selector. GIF is the only exposed export format.
- Reworked the GIF tab around Mastodon target size instead of palette-count-first controls.
- Added native GTK font and color controls for text overlays:
  - `Font`
  - `Text color`
  - `Stroke color`
- Added first-pass ffmpeg GIF export from the app:
  - Source can be an MP4/video file or an animated GIF.
  - Text overlay is burned into the exported GIF.
  - Text fill and stroke colors are included in export.
  - Output path is currently hardcoded to `/code/gifbrewery-export.gif`.
- Added `gifbrewery-gtk --smoke-export SOURCE OUTPUT` for export smoke tests using the same exporter.

Export smoke artifacts written for review:

- `/code/gifbrewery-visual-smoke/export-video-with-text.gif`
- `/code/gifbrewery-visual-smoke/export-video-with-text-first-frame.png`
- `/code/gifbrewery-visual-smoke/export-gif-source-with-text.gif`
- `/code/gifbrewery-visual-smoke/export-gif-source-with-text-first-frame.png`
- `/code/gifbrewery-visual-smoke/export-manifest.txt`
- Added `crates/gifbrewery-gtk/src/timeline.rs`:
  - Custom GTK drawing timeline.
  - Ruler, playhead, overlay lane, selected clip range, trim handles, and filmstrip.
  - Click/drag seek support.
  - Drag support for clip start/end and text overlay start/end.
- Added `crates/gifbrewery-gtk/src/thumbnails.rs`:
  - Uses `ffmpeg` to sample source frames.
  - Stores thumbnails under `/tmp/gifbrewery-thumbnails`.
  - Loads thumbnails as `gdk-pixbuf` pixbufs and paints them into the timeline.
- Replaced the old plain GTK slider with the visual timeline.
- Bound additional text overlay controls:
  - `Text`
  - `Font`
  - `Text color`
  - `Stroke color`
  - `Font size`
  - `Stroke width`
  - `Shadow`
- Validated the GTK4/GStreamer preview player:
  - New `VideoPreview` in `crates/gifbrewery-gtk/src/media.rs`
  - Uses GStreamer `playbin`.
  - Uses `gtk4paintablesink`.
  - Reads the sink's `paintable` property.
  - Shows it inside a `gtk::Picture`.
  - `apply_source_file` calls `player.open_file(file)` after media load.
- Added GStreamer bus logging for preview errors, warnings, EOS, and playbin state changes.
- Wires preview controls:
  - restart: pause and seek to 0
  - play: set pipeline to `Playing`
  - pause: set pipeline to `Paused`
- The visual timeline now seeks preview playback.
- Basic timing rows are now model-backed:
  - Clip `Start`/`End`
  - Text overlay `Appears`/`Disappears`
  - Text overlay text/font size/stroke width/shadow
- Added a `syncing_widgets` guard to avoid recursive GTK notify callbacks while programmatically updating spin rows.

This has been rebuilt and visually smoke-tested:

```bash
cargo build
tools/visual-smoke.sh
```

The latest inspected screenshot is:

- `/code/gifbrewery-visual-smoke/clip.png`

The expected headless EGL/DRI3 warning still appears, but it does not block capture. No GStreamer preview errors were printed during the last smoke run.

## Important Caveats

The current UI is still a prototype shell, not a full usable editor. It has a first-pass GIF exporter, but the output path is still hardcoded.

The app currently does not persist projects.

Several inspector controls are now model-backed, but some are still display-only:

- Clip speed/FPS rows.
- Optimize/high-quality rows still need model binding.

The text overlay shown in the preview is a GTK label overlay, not yet the exact final ffmpeg render.
Text stroke width and stroke color are exported, but not visually rendered in the GTK preview label.

The preview pipeline is compile-clean and visually confirmed with the sample MP4.

## Recommended Next Steps

1. Continue binding inspector controls to the model:
   - Clip speed/FPS.
   - Optimize/high-quality.
   - Text overlay background/blend when those controls are added.

2. Continue the export path:
   - Replace the hardcoded `/code/gifbrewery-export.gif` with a save dialog.
   - Run export off the GTK UI thread and surface errors in-app instead of only `stderr`.
   - Add crop to the ffmpeg filter chain.
   - Add direct overlay positioning/resizing in the preview.
   - Add size-budget iteration for the Mastodon target size instead of exposing color reduction as the primary workflow.
   - Keep export backend separate from GTK UI.

3. Improve visual smoke coverage:
   - Current script captures tab screenshots.
   - Add a check that the preview area is not entirely black once video playback is expected.
   - Add one capture after pressing play.
   - Add a simple interaction test for scrubber/SpinRow timing changes.
   - Add a visual check that the filmstrip contains decoded thumbnails rather than placeholders.

4. Continue the visual timeline pass described in `docs/TIMELINE_DESIGN.md`:
   - Add overlay body dragging.
   - Add clip body dragging if useful.
   - Add hover affordances and cursor changes.
   - Add zoom/scroll for longer clips.

5. Update Debian packaging once runtime dependencies settle:
   - GTK 4
   - libadwaita
   - GStreamer base/good/bad/ugly/libav/gtk4 plugin packages
   - ffmpeg

## Useful Commands

Run app directly with sample media:

```bash
cargo run --bin gifbrewery-gtk -- "/code/GIF Brewery 3.app/Contents/Resources/loading.mp4"
```

Headless visual smoke:

```bash
tools/visual-smoke.sh
```

Export smoke:

```bash
./target/debug/gifbrewery-gtk --smoke-export "/code/GIF Brewery 3.app/Contents/Resources/loading-smaller.mp4" /code/gifbrewery-visual-smoke/export-video-with-text.gif
./target/debug/gifbrewery-gtk --smoke-export "/code/GIF Brewery 3.app/Contents/Resources/kvo.gif" /code/gifbrewery-visual-smoke/export-gif-source-with-text.gif
```

Override smoke source:

```bash
SMOKE_SOURCE=/path/to/video.mp4 tools/visual-smoke.sh
```

Build and check:

```bash
cargo fmt
cargo check
cargo build
```

Inspect GTK4 sink availability:

```bash
gst-inspect-1.0 gtk4paintablesink
```

Inspect MP4/H.264 plugin availability:

```bash
gst-inspect-1.0 qtdemux
gst-inspect-1.0 avdec_h264
```

## Notes For Resuming

The workspace is not a git repository, so use file-level review and avoid relying on `git status`.

The user wants to watch progress in `/code`; keep writing screenshots there, not just under `target/`.

The visual smoke script only rebuilds if `target/debug/gifbrewery-gtk` is missing. After code edits, run `cargo build` before `tools/visual-smoke.sh`; otherwise screenshots may show the old binary.

The current stopping point is intentionally conservative: code compiles, visual smoke passes, durable notes are written, and the next implementation step is clear.

## 2026-06-13 Late Session Addendum

User corrections handled:

- Removed the GIF `Format` dropdown entirely. The app only exports GIF.
- Removed `ExportFormat` and `ProjectSettings.output_format` from the core model.
- Removed GIF loop configurability from the model/UI. The user wants every GIF to loop.
- `crates/gifbrewery-gtk/src/export.rs` now always passes `-loop 0` to ffmpeg, which ImageMagick reports as `Iterations: 0` (infinite loop).
- Removed `GifExportSettings.loop_count` and `loop_delay_ms`.
- Removed the GIF tab `Loop count` row.
- Added direct drag support for the text overlay in the main preview:
  - `EditorWidgets` now stores the preview `gtk::Overlay`.
  - `install_preview_overlay_drag` attaches `gtk::GestureDrag` to the caption label.
  - Dragging updates `TextOverlay.bounds.x/y` in normalized preview coordinates.
  - `apply_text_overlay_position` positions the GTK label from the same bounds that ffmpeg export reads.
  - The label cursor is set to `move`.

Files changed in this late session:

- `crates/gifbrewery-core/src/model.rs`
- `crates/gifbrewery-core/src/lib.rs`
- `crates/gifbrewery-gtk/src/export.rs`
- `crates/gifbrewery-gtk/src/main.rs`
- `crates/gifbrewery-gtk/src/ui.rs`
- `docs/SESSION_HANDOFF.md`
- `docs/PORTING_NOTES.md`

Verification completed:

```bash
cargo fmt
cargo check
cargo build
```

Last `cargo check` was clean with no warnings after switching preview size reads from deprecated `allocated_width/allocated_height` to `width()/height()`.

Export smoke tests completed:

```bash
./target/debug/gifbrewery-gtk --smoke-export "/codex/gifbrewery/GIF Brewery 3.app/Contents/Resources/loading-smaller.mp4" /code/gifbrewery-visual-smoke/export-video-with-text.gif
./target/debug/gifbrewery-gtk --smoke-export "/codex/gifbrewery/GIF Brewery 3.app/Contents/Resources/kvo.gif" /code/gifbrewery-visual-smoke/export-gif-source-with-text.gif
```

Results:

- MP4 input exported successfully to animated GIF with burned text/stroke.
- GIF input exported successfully to animated GIF with burned text/stroke.
- `identify -verbose` reported `Iterations: 0` for both regenerated exports, confirming infinite looping.
- Review artifacts live in `/code/gifbrewery-visual-smoke/`.

Known problem encountered:

- Attempted to drive the GTK app with `xdotool` under Xvfb to click `Create GIF`, but `xdotool` repeatedly failed with `Can't open display: (null)` even when `DISPLAY=:100`/`:101` was passed via `env`.
- Some failures were from reusing occupied Xvfb display numbers; a fresh Xvfb display started, but `xdotool` still did not attach reliably in this container.
- Workaround: added `gifbrewery-gtk --smoke-export SOURCE OUTPUT`, which uses the same app exporter without needing mouse automation.

Important caveat:

- Text overlay dragging compiles cleanly but was not visually smoke-tested with mouse automation because of the `xdotool`/Xvfb issue above.
- Next session should manually launch the app, drag the text in the preview, export, and confirm the exported text position follows the drag.

## 2026-06-13 Resume Addendum

Implemented after resuming:

- Replaced the hardcoded app export path with a native GTK save dialog:
  - `Create GIF` now opens a `Save GIF` dialog.
  - Default filename is `gifbrewery-export.gif`.
  - The selected local destination is passed to the existing GIF exporter.
- Bound additional inspector controls into the project model:
  - Clip `Speed`
  - Clip `Frames per second`
  - GIF `Optimize GIF`
  - GIF `High-quality quantization`
- Kept the existing CLI smoke export path unchanged.

Verification completed:

```bash
cargo fmt
cargo check
cargo build
./target/debug/gifbrewery-gtk --smoke-export "GIF Brewery 3.app/Contents/Resources/loading-smaller.mp4" /tmp/gifbrewery-export-smoke.gif
tools/visual-smoke.sh
```

The visual smoke run completed with the known Xvfb software-rendering warning and wrote fresh screenshots to `/code/gifbrewery-visual-smoke`.

Remaining caveats:

- GIF export still runs synchronously on the GTK UI thread after a save destination is chosen.
- Export errors are still reported to `stderr`, not in an in-app dialog.
- `Optimize GIF` and `High-quality quantization` are now model-backed, but the exporter still needs a fuller quality/optimization strategy beyond the current palettegen/paletteuse command.

## 2026-06-13 Black Screenshot Follow-Up

The user reported that `/code/gifbrewery-visual-smoke/clip.png`, `gif.png`, and `overlays.png` were just black squares. This was confirmed with ImageMagick:

```text
Colors: 1
mean: 0 (0)
Histogram: all pixels #000000
```

Findings:

- The old smoke script captured the X root window on a fixed display `:99`.
- It did not verify that Xvfb actually started on that display.
- It did not verify that the GIF Brewery window existed before taking screenshots.
- The default smoke source pointed at `/code/GIF Brewery 3.app/...`, which may not exist in this workspace; the repo-local source under `/codex/gifbrewery/GIF Brewery 3.app/...` should be preferred.
- A first smoke-script patch switched to Xvfb `-displayfd`, copied the current binary to `/code/gifbrewery-visual-smoke/gifbrewery-gtk`, waited for a visible `GIF Brewery` window, captured that window id instead of the root, and wrote app logs to `/code/gifbrewery-visual-smoke/app.log`.
- That made the harness more correct, but screenshots were still all-black under Xvfb.

Likely cause:

- GTK 4 rendering under Xvfb was using a GL/GSK path that is not captured correctly by `xwd`, even though the app window exists.

Final smoke-script fix applied and verified:

- `tools/visual-smoke.sh` now launches the app with:

```bash
GSK_RENDERER=cairo
LIBGL_ALWAYS_SOFTWARE=1
GDK_BACKEND=x11
```
- The script now uses Xvfb `-displayfd` instead of a fixed `:99`.
- It waits for the `GIF Brewery` window before capture.
- It captures the app window id instead of the root window.
- It writes app logs to `/code/gifbrewery-visual-smoke/app.log`.
- It copies the current debug binary to `/code/gifbrewery-visual-smoke/gifbrewery-gtk` via a temporary file and rename, avoiding `Text file busy` failures.
- Each capture retries if ImageMagick reports mean brightness `0`, so the script fails instead of accepting a black screenshot.

Verified after the fix:

```bash
tools/visual-smoke.sh
identify -verbose /code/gifbrewery-visual-smoke/clip.png /code/gifbrewery-visual-smoke/gif.png /code/gifbrewery-visual-smoke/overlays.png
```

Result:

- `clip.png`, `gif.png`, and `overlays.png` are now `1280 x 820` RGB PNGs.
- ImageMagick reported nonzero means around `0.85` and nonzero standard deviations, confirming they are no longer black squares.

Binary note:

- `/code/gifbrewery-visual-smoke/gifbrewery-gtk` is copied by the smoke script for convenience.
- It is not a monolithic/static binary. `file` reports a dynamically linked ELF with debug info.
- It should run on this machine from that directory, but it depends on system GTK/libadwaita/GStreamer libraries, GStreamer plugins, and `ffmpeg`; it is not portable to a bare machine by copying just that one file.

## 2026-06-13 User Binary Test Follow-Up

The user ran `/code/gifbrewery-visual-smoke/gifbrewery-gtk` directly and reported:

- Text dragging was blinky/jumpy.
- Text could not be dragged right past about halfway across the preview.
- There was no obvious way to load/import a GIF or other file.
- If missing libraries are responsible, the app should validate runtime support and show an in-app warning.
- The app should emit a debug log so outside-container behavior can be diagnosed.

Fixes implemented:

- Made the header import control explicit:
  - Button now says `Open Media...`.
  - File dialog title is `Open Media`.
  - File dialog includes `Video and GIF files` and `All files` filters.
- Fixed the text drag clamp:
  - The old code clamped `bounds.x` to `1.0 - overlay.bounds.width`.
  - Default text bounds width is `0.8`, so dragging could only move the overlay box to `x <= 0.2`.
  - Drag now clamps against the actual caption label allocation divided by preview width/height.
- Reduced drag flicker:
  - Drag updates now mutate the overlay position and directly reapply only caption position.
  - They no longer refresh the full timeline/inspector/label markup on every mouse motion.
- Changed ffmpeg text export positioning:
  - `TextOverlay.bounds.x/y` now map to drawtext top-left position (`x=w*bounds.x`, `y=h*bounds.y`) instead of centering text inside an 80%-wide box.
- Added runtime diagnostics:
  - New `crates/gifbrewery-gtk/src/diagnostics.rs`.
  - App creates a debug log at `$GIFBREWERY_LOG`, or `$XDG_STATE_HOME/gifbrewery/gifbrewery.log`, or `~/.local/state/gifbrewery/gifbrewery.log`.
  - Logs argv, cwd, GStreamer initialization, runtime dependency check result, file load URI, discovered metadata, and preview URI.
  - Important GStreamer preview bus messages now go through the debug log too.
- Added startup runtime validation:
  - Checks for `ffmpeg`.
  - Checks GStreamer initialization.
  - Checks key factories: `playbin`, `gtk4paintablesink`, `qtdemux`, `avdec_h264`.
  - Checks GIF decode support via either `avdec_gif` or `gdkpixbufdec`.
  - If issues exist, shows an `adw::AlertDialog` listing install hints and the debug log path.

Verification completed:

```bash
cargo fmt
cargo check
cargo build
./target/debug/gifbrewery-gtk --smoke-export "GIF Brewery 3.app/Contents/Resources/kvo.gif" /tmp/gifbrewery-gif-source-smoke.gif
tools/visual-smoke.sh
```

Results:

- `cargo check` clean.
- GIF-source smoke export succeeded.
- Visual smoke regenerated `/code/gifbrewery-visual-smoke/gifbrewery-gtk`.
- `clip.png`, `gif.png`, and `overlays.png` remain non-black RGB screenshots.
- Latest log showed `runtime dependency check passed` in this container.

Remaining caveat:

- The drag behavior was fixed by correcting the clamp/update path and compiles cleanly, but it still needs the user to retry direct pointer dragging outside the container and share `~/.local/state/gifbrewery/gifbrewery.log` if it remains jumpy.

## 2026-06-13 Caption Drag/Stroke Follow-Up

The user retried the direct binary and reported that caption dragging was still badly broken:

- Ghost copies appeared while dragging.
- The caption appeared to jump between positions.
- Drag motion followed the mouse only at a fraction of the cursor scale.
- The caption tended to jump down toward the bottom of the frame.
- Stroke did not appear in the preview.
- The user asked for debug logging to capture the specific interaction data being fought.

Implementation changes:

- Replaced the old GTK `Label` caption overlay with a custom transparent `gtk::DrawingArea` overlay:
  - New local `CaptionOverlay` type in `crates/gifbrewery-gtk/src/ui.rs`.
  - The drawing area fills the preview overlay and remains stable during drag.
  - Text is drawn with Cairo/Pango using `pangocairo`.
  - Stroke is now rendered in preview by drawing the Pango layout path, stroking it, then filling it.
  - Shadow is drawn in preview as a small translucent black offset before stroke/fill.
- Added `pangocairo = "0.20"` to `crates/gifbrewery-gtk/Cargo.toml`; `Cargo.lock` updated.
- Removed the old moving-label margin positioning path:
  - The old approach moved a GTK widget under the pointer during drag, likely causing allocation churn and jumpy coordinates.
  - The custom drawing layer records actual caption pixel bounds for hit testing and clamp math.
- Dragging now:
  - Starts only when the pointer is inside the recorded caption pixel bounds.
  - Uses the full preview drawing area as the gesture coordinate space.
  - Updates `TextOverlay.bounds.x/y`.
  - Repaints only the caption overlay during drag, not the full inspector/timeline.
- Debug logging now captures caption drag telemetry:
  - `caption drag begin`: pointer position, hit bounds, active state.
  - `caption drag start bounds`: starting normalized model bounds.
  - `caption drag update`: raw gesture offsets and starting bounds.
  - `caption drag computed`: preview size, caption pixel bounds, clamp max, resulting normalized bounds.
  - `caption drag end`.

Verification completed:

```bash
cargo fmt
cargo check
cargo build
./target/debug/gifbrewery-gtk --smoke-export "GIF Brewery 3.app/Contents/Resources/loading-smaller.mp4" /tmp/gifbrewery-export-smoke.gif
tools/visual-smoke.sh
```

Results:

- `cargo check` clean.
- MP4 smoke export succeeded.
- Visual smoke regenerated `/code/gifbrewery-visual-smoke/gifbrewery-gtk`.
- `clip.png`, `gif.png`, and `overlays.png` remain non-black RGB screenshots.

Remaining caveat:

- The new drag path is structurally different and logs detailed telemetry, but direct human pointer dragging still needs to be retried outside the container. If it is still wrong, the next report should include `~/.local/state/gifbrewery/gifbrewery.log`, specifically the `caption drag ...` lines.

## 2026-06-13 Import/Font/Timeline Follow-Up

The user reported:

- The app may have crashed.
- The font picker "chokes real hard" when typing/searching fonts.
- The bottom video scrubber/timeline does not appear interactive.
- The user still could not find a way to open movies/GIFs.
- The user asked to review Instagiffer and LosslessCut source, and corrected that local source copies may already exist under `/code` or `/codex`.

Local source search:

- Searched `/code` and `/codex` for LosslessCut/Instagiffer directories.
- No local LosslessCut or Instagiffer clone was found.
- Existing local reference clone remains `reference/gifcurry`.
- A fresh shallow LosslessCut clone was created during this turn at `/tmp/lossless-cut`.
- Attempted `https://github.com/instagiffer/instagiffer.git`, but GitHub requested credentials; no public clone was obtained from that URL.

LosslessCut source observations from `/tmp/lossless-cut`:

- File import is explicit and multi-entry:
  - `src/main/menu.ts` has File > Open with `CmdOrCtrl+O` sending `openFilesDialog`.
  - `src/main/index.ts` handles CLI files, second-instance files, macOS `open-file`, and delayed open until renderer readiness.
  - `src/renderer/src/App.tsx` has `openFilesDialog` and visible no-file state wiring.
  - `src/renderer/src/TopMenu.tsx` and `App.tsx` include drag/drop file handling.
- Timeline is a primary interaction surface:
  - `src/renderer/src/Timeline.tsx` computes mouse-to-time from the timeline element bounds.
  - It seeks on mouse down and continues seeking on window mousemove until mouseup.

Fixes implemented locally:

- Removed GTK's system `FontDialogButton` from the overlay inspector.
  - Replaced it with a simple `Font family` `EntryRow`.
  - This avoids expensive system font picker/search behavior while keeping font-family editing possible.
- Made import visible in the main editor area:
  - Header still has `Open Media...`.
  - Preview now shows a centered `Open Media...` button before a source is loaded.
  - The centered button and header button use the same `open_media_dialog` helper.
  - The centered button hides after a media source is loaded.
- Added more debug logging:
  - `open media dialog requested`
  - dialog cancellation/error
  - existing media-load metadata logging remains
  - timeline click/drag begin/drag update logs in `crates/gifbrewery-gtk/src/timeline.rs`
- Timeline now sets a pointer cursor and logs:
  - click pointer position, width, hit target
  - seek seconds computed from click
  - drag begin state
  - drag update delta and computed seconds delta

Verification completed:

```bash
cargo fmt
cargo check
cargo build
./target/debug/gifbrewery-gtk --smoke-export "GIF Brewery 3.app/Contents/Resources/kvo.gif" /tmp/gifbrewery-gif-source-smoke.gif
tools/visual-smoke.sh
```

Results:

- `cargo check` clean.
- GIF-source export smoke succeeded.
- Visual smoke regenerated `/code/gifbrewery-visual-smoke/gifbrewery-gtk`.
- `clip.png`, `gif.png`, and `overlays.png` remain non-black RGB screenshots.

Remaining next steps:

- Retry direct app use with `/code/gifbrewery-visual-smoke/gifbrewery-gtk`.
- If timeline still appears noninteractive, inspect `~/.local/state/gifbrewery/gifbrewery.log` for `timeline click` / `timeline drag` entries to determine whether events reach the widget.
- If caption drag still misbehaves, inspect `caption drag ...` entries in the same log.
- Add real drag/drop file import to the GTK preview/window, matching LosslessCut's body-level drop behavior.
- Consider adding a native app menu or keyboard shortcut action for Open (`Ctrl+O`), matching LosslessCut's File > Open affordance.

## 2026-06-13 Timeline Crash Follow-Up

The user reported another crash while clicking around, specifically when clicking the left edge of the timeline, and asked where crash logs go.

Crash log status:

- Normal debug logs go to `~/.local/state/gifbrewery/gifbrewery.log` unless `GIFBREWERY_LOG` is set.
- Before this fix, Rust panics were only guaranteed to go to stderr, so a double-clicked/copied binary could crash without a useful persistent panic line.
- Added a panic hook in `crates/gifbrewery-gtk/src/diagnostics.rs`; future Rust panics write `PANIC at file:line: payload` to the same debug log.

Likely crash cause fixed:

- `crates/gifbrewery-gtk/src/timeline.rs` called app callbacks while holding a mutable borrow of the timeline view state.
- Those callbacks call back into `TimelineView::set_state`, which tries to mutably borrow the same state again.
- This can panic with a `RefCell already borrowed`-style error, especially when clicking/dragging timeline edges.
- Fixed click and drag handlers to compute pending callback data while the timeline state is borrowed, release the borrow, then call callbacks.

Open button visibility fix:

- The custom full-preview caption drawing layer was above the empty-state `Open Media...` button and could intercept it.
- Caption overlay is now hidden until a media source is loaded.
- `apply_source_file` hides the empty-state open button and shows the caption overlay.

Verification completed:

```bash
cargo fmt
cargo check
cargo build
./target/debug/gifbrewery-gtk --smoke-export "GIF Brewery 3.app/Contents/Resources/loading-smaller.mp4" /tmp/gifbrewery-export-smoke.gif
tools/visual-smoke.sh
```

Results:

- `cargo check` clean.
- MP4 smoke export succeeded.
- Visual smoke regenerated `/code/gifbrewery-visual-smoke/gifbrewery-gtk`.
- `clip.png`, `gif.png`, and `overlays.png` remain non-black RGB screenshots.

Next crash triage:

- Ask the user to run the refreshed `/code/gifbrewery-visual-smoke/gifbrewery-gtk`.
- If it crashes again, collect `~/.local/state/gifbrewery/gifbrewery.log` and look for `PANIC`, `timeline click`, `timeline drag`, and `caption drag` lines near the end.

## 2026-06-13 Adjacent Crash Log Follow-Up

The user asked for crash/debug logs to be written beside the binary instead of under `~/.local/state`, so they do not need to copy logs manually.

Implemented:

- `crates/gifbrewery-gtk/src/diagnostics.rs` now chooses log path in this order:
  1. `$GIFBREWERY_LOG`, if set.
  2. `gifbrewery.log` next to the current executable, if that directory is writable.
  3. `$XDG_STATE_HOME/gifbrewery/gifbrewery.log`.
  4. `~/.local/state/gifbrewery/gifbrewery.log`.
  5. `gifbrewery.log` in the current working directory.
- The executable-adjacent path is tested with a temporary `.gifbrewery-log-write-test` file before use.
- `tools/visual-smoke.sh` now launches the copied smoke binary at `/code/gifbrewery-visual-smoke/gifbrewery-gtk`, not `target/debug/gifbrewery-gtk`, so smoke behavior matches how the user runs it.

Verified:

```bash
cargo fmt
cargo check
cargo build
tools/visual-smoke.sh
```

Result:

- `/code/gifbrewery-visual-smoke/gifbrewery.log` is created beside `/code/gifbrewery-visual-smoke/gifbrewery-gtk`.
- The log reports:

```text
debug log: /code/gifbrewery-visual-smoke/gifbrewery.log
argv: ["/code/gifbrewery-visual-smoke/gifbrewery-gtk", ...]
runtime dependency check passed
```

Updated crash triage:

- If the user runs `/code/gifbrewery-visual-smoke/gifbrewery-gtk`, ask them for `/code/gifbrewery-visual-smoke/gifbrewery.log`.
- Only fall back to `~/.local/state/gifbrewery/gifbrewery.log` if the executable directory was not writable.

## 2026-06-13 Playback/Scrubber Follow-Up

The user loaded a GIF and reported:

- The white vertical read head/playhead on the scrubber does not move with playback.
- The GIF appears to play, but the scrubber/playhead is disconnected.
- Space sometimes pauses playback and sometimes just activates the last clicked control.
- Dragging the left edge of the selected range and pressing play starts from the beginning instead of the new clip start.

Reference review:

- GIF Brewery 3 Mach-O strings confirm separate but connected player/scrubber control concepts:
  - `HRPlayerViewController`
  - `HRScrubberView`
  - `handlePlayButton:`
  - `handleScrubbing:`
  - `updateScrubber:`
  - `currentTime`
  - `setStartTime:`
  - `setEndTime:`
  - `seekToTime:`
- This matches the missing contract in the GTK port: playback position must be observed and pushed back into timeline state; the scrubber cannot be command-only.
- LosslessCut's `Timeline.tsx` also maps mouse position to timeline time and calls `seekAbs` on mouse down/move, keeping the timeline and playback state coupled.

Fixes implemented:

- `VideoPreview` now exposes `position_seconds()` via GStreamer `query_position`.
- Playback command logging added:
  - `preview play`
  - `preview pause`
  - `preview seek: ...`
- Timeline play/pause/restart buttons are now stored in `EditorWidgets` and route through app-state helpers instead of directly calling GStreamer.
- Added playback app-state flag: `AppState::is_playing`.
- Added a 100 ms playback poll:
  - While playing, reads GStreamer position.
  - Updates `AppState::playhead_seconds`.
  - Calls `update_timeline_widgets`, moving the white playhead during playback.
  - When playback reaches clip end, pauses and seeks back to clip start.
- Restart now seeks to clip start, not source time `0`.
- Play now starts from the current playhead if it is inside the clip; otherwise it seeks to clip start first.
- Timeline seek/scrub pauses playback and seeks preview to the requested time.
- Moving clip start through inspector or timeline:
  - Pauses playback.
  - Moves playhead to the new clip start.
  - Seeks preview to that new start.
- Moving clip end:
  - Pauses playback.
  - Clamps playhead to the new range.
- Spacebar:
  - Added window-level `EventControllerKey`.
  - Space now logs `spacebar playback toggle` and calls the playback toggle helper.
  - Playback buttons are `focusable(false)`, reducing GTK's default "press last focused button" behavior.

Verification completed:

```bash
cargo fmt
cargo check
cargo build
./target/debug/gifbrewery-gtk --smoke-export "GIF Brewery 3.app/Contents/Resources/kvo.gif" /tmp/gifbrewery-gif-source-smoke.gif
tools/visual-smoke.sh
```

Results:

- `cargo check` clean.
- GIF-source smoke export succeeded.
- Visual smoke regenerated `/code/gifbrewery-visual-smoke/gifbrewery-gtk`.
- `/code/gifbrewery-visual-smoke/gifbrewery.log` is written beside the binary.
- `clip.png`, `gif.png`, and `overlays.png` remain non-black RGB screenshots.

Remaining validation:

- User should retry playback with a loaded GIF and inspect `/code/gifbrewery-visual-smoke/gifbrewery.log` for `preview playback start`, `preview seek`, `spacebar playback toggle`, and any `timeline ...` entries if behavior still feels disconnected.

## 2026-06-13 Final Handoff Before Context Limit

Current user-reported bugs after the playback/scrubber patch:

- Spacebar still does not appear to work reliably as play/pause.
- Text overlay `Appears` / `Disappears` times do not affect the live preview.
- In GIF Brewery 3, text overlays actually appear and disappear in the preview at their configured times; they are not decorative editor-only widgets.
- The GTK port must therefore gate caption preview rendering by current playhead time, using the overlay's `TimelineRange`.

Important current implementation state:

- Logs for the copied binary should now be adjacent to it:
  - Binary: `/code/gifbrewery-visual-smoke/gifbrewery-gtk`
  - Log: `/code/gifbrewery-visual-smoke/gifbrewery.log`
- `diagnostics.rs` installs a panic hook and writes `PANIC ...` lines to the debug log.
- `tools/visual-smoke.sh` now runs the copied binary from `/code/gifbrewery-visual-smoke`, not `target/debug`, so smoke behavior matches what the user runs.
- The app is not a git checkout; use filesystem inspection and this handoff file.
- Root `AGENTS.md` exists and instructs future sessions to read this file.

Recent code changes that are already in place:

- `VideoPreview::position_seconds()` queries GStreamer position.
- `AppState` has `is_playing`.
- Playback buttons route through helper functions in `ui.rs`:
  - `restart_preview_at_clip_start`
  - `start_preview_playback`
  - `pause_preview_playback`
  - `toggle_preview_playback`
  - `sync_playback_position`
- A 100 ms `glib::timeout_add_local` poll calls `sync_playback_position`.
- Timeline click/drag callbacks were changed to release internal state borrows before invoking app callbacks, fixing a likely `RefCell already borrowed` panic.
- Caption preview was converted from moving GTK `Label` to custom Cairo/Pango `CaptionOverlay`.
- Stroke preview rendering exists via Cairo path stroke/fill.
- Font picker was removed and replaced with plain `Font family` text row.
- A centered `Open Media...` button was added and the caption layer is hidden until media loads so it should not cover the open button.

Likely next fixes:

1. Fix spacebar reliably:
   - Current code uses `gtk::EventControllerKey` on the window.
   - It may not receive key events depending on focused child/controller phase.
   - Try an application/window action with accelerator instead:
     - create `gio::SimpleAction` such as `win.toggle-playback` or `app.toggle-playback`
     - set accelerator to `"space"` or `"<Space>"`
     - route it to `toggle_preview_playback`.
   - Alternatively set key controller propagation phase to capture if available in GTK 4 Rust API.
   - Add log line at the actual action entry point and verify it appears in `/code/gifbrewery-visual-smoke/gifbrewery.log`.

2. Make overlay timing affect live preview:
   - `CaptionOverlay` currently draws whenever visible; it does not check playhead time.
   - Add current playhead to the caption overlay state, or pass a boolean visible flag derived from:
     `text.range.start_seconds <= playhead_seconds <= text.range.end_seconds`
   - `update_timeline_widgets` already has both `text_overlay` and `state.playhead_seconds` available in the borrowed state block; include the playhead in the tuple and update the caption overlay accordingly.
   - During playback, `sync_playback_position` updates playhead and calls `update_timeline_widgets`, so once caption drawing is gated by playhead, overlay appears/disappears during playback.
   - During manual timeline seek, `update_playhead` calls `update_timeline_widgets`, so overlay should also appear/disappear while scrubbing.

3. Consider clip-scoped playback semantics:
   - GStreamer itself will keep playing source media unless explicitly paused at clip end.
   - The 100 ms poll currently pauses and seeks to clip start once position reaches clip end.
   - If users expect loop preview, add a preview-loop toggle later; GIF export remains always-looping.

4. Improve logging for the two current bugs:
   - Spacebar/action logs should include focused widget if easy to get.
   - Caption overlay drawing logs should be throttled; do not log every frame, but log transitions:
     - hidden because playhead before start
     - shown in range
     - hidden after end

Reference notes already discovered:

- GIF Brewery 3 Mach-O strings include:
  - `HRPlayerViewController`
  - `HRScrubberView`
  - `handlePlayButton:`
  - `handleScrubbing:`
  - `updateScrubber:`
  - `currentTime`
  - `setStartTime:`
  - `setEndTime:`
  - `seekToTime:`
- This supports the architecture that preview, scrubber, and overlay visibility must all be tied to current playback time.
- GIF Brewery 3 localization includes import/drag-drop messaging:
  - "Import using the + above or drag and drop any image, GIF, or video files to get started!"
- LosslessCut clone used for reference is at `/tmp/lossless-cut` in this session; no local `/code` or `/codex` LosslessCut/Instagiffer clone was found when searched.
- LosslessCut's timeline maps mouse position to time and calls `seekAbs` on mouse down/move; import is exposed through File > Open, open dialogs, CLI/open-with handling, and drag/drop.

Last verified commands:

```bash
cargo fmt
cargo check
cargo build
./target/debug/gifbrewery-gtk --smoke-export "GIF Brewery 3.app/Contents/Resources/kvo.gif" /tmp/gifbrewery-gif-source-smoke.gif
tools/visual-smoke.sh
```

Last verification result:

- `cargo check` clean.
- GIF-source smoke export succeeded.
- Visual smoke regenerated `/code/gifbrewery-visual-smoke/gifbrewery-gtk`.
- `/code/gifbrewery-visual-smoke/gifbrewery.log` is created beside the binary.
- Screenshots remained non-black RGB.

## 2026-06-14 Update

User clarified the expected GIF Brewery timing workflow:

- Move the playhead to the exact frame/time where text should appear.
- Press a button to mark that text overlay's appear/start time.
- Move the playhead to where that text should disappear.
- Press another button to mark the disappear/end time.
- Repeat that workflow for multiple independent text overlays, so captions can appear/disappear at different times.

Implemented in this update:

- `CaptionOverlay` now gates live preview visibility by `TextOverlay.range` and current `AppState.playhead_seconds`.
- `update_timeline_widgets` passes the playhead into the caption preview update path.
- During playback and manual scrub/seek, the visible caption now appears only while:
  `range.start_seconds <= playhead_seconds <= range.end_seconds`.
- Caption timing transitions are logged as:
  `caption timing visibility: playhead=... range=... visible=...`
- Spacebar key handling now sets the window key controller to GTK capture phase:
  `controller.set_propagation_phase(gtk::PropagationPhase::Capture);`
  This should make the spacebar toggle reach playback before focused buttons/rows consume it.
- The Overlays inspector now has two action rows:
  - `Appears at playhead` with a `Set` button.
  - `Disappears at playhead` with a `Set` button.
- Those buttons update the first text overlay's timing from `AppState.playhead_seconds`, refresh the inspector/timeline/preview, and log:
  - `overlay mark appears at playhead: ...`
  - `overlay mark disappears at playhead: ...`

Important remaining gap:

- The UI still only exposes and edits the first text overlay.
- The core model supports a vector of overlays, but there is no overlay list, add-text-overlay button, selected-overlay state, or per-overlay caption drawing stack yet.
- To match GIF Brewery 3 properly, the next session should add:
  - text overlay list/add/duplicate/delete controls
  - selected overlay state
  - one preview drawing layer capable of rendering all text overlays whose ranges include the current playhead, or separate layers per overlay
  - timing mark buttons acting on the selected overlay
  - timeline lane selection/editing for multiple overlay bars

Black screenshot/debug-log status:

- `tools/visual-smoke.sh` now refreshes artifacts under `/code/gifbrewery-visual-smoke`.
- The copied runnable binary is:
  `/code/gifbrewery-visual-smoke/gifbrewery-gtk`
- The adjacent debug log is:
  `/code/gifbrewery-visual-smoke/gifbrewery.log`
- The latest screenshot stats are nonblack:
  - `clip.png mean=28648.7 min=0 max=65535`
  - `gif.png mean=55840.1 min=0 max=65535`
  - `overlays.png mean=55770.3 min=0 max=65535`
- If screenshots regress to black squares, inspect:
  - whether the smoke script is launching the copied binary from `/code/gifbrewery-visual-smoke`
  - `/code/gifbrewery-visual-smoke/gifbrewery.log`
  - GStreamer `gtk4paintablesink` availability
  - whether Xvfb/software rendering emitted only the known `libEGL warning: DRI3 error` or a real GStreamer/GTK error

Verification completed after this update:

```bash
cargo fmt
cargo check
cargo build
./target/debug/gifbrewery-gtk --smoke-export "GIF Brewery 3.app/Contents/Resources/kvo.gif" /tmp/gifbrewery-gif-source-smoke.gif
tools/visual-smoke.sh
identify -format '%f %[mean] %[min] %[max]\n' /code/gifbrewery-visual-smoke/clip.png /code/gifbrewery-visual-smoke/gif.png /code/gifbrewery-visual-smoke/overlays.png
```

Results:

- `cargo check` clean.
- `cargo build` clean.
- GIF-source smoke export succeeded and wrote `/tmp/gifbrewery-gif-source-smoke.gif`.
- Visual smoke regenerated `/code/gifbrewery-visual-smoke/gifbrewery-gtk`.
- `/code/gifbrewery-visual-smoke/gifbrewery.log` exists beside the copied binary.
- Screenshot pixel stats confirm the smoke screenshots are not black.

## 2026-06-14 Multi-Overlay Follow-Up

Implemented after the user clarified the multi-caption workflow:

- `AppState` now tracks `selected_overlay_id`.
- The Overlays inspector now includes:
  - a clickable overlay list in the overlay section
  - an `Add` button for text overlays
  - a `Delete` button for the selected overlay
- All overlay style/timing/text edits now target the selected text overlay instead of always editing the first overlay.
- `Appears at playhead` and `Disappears at playhead` now mark the selected overlay.
- Dragging the caption preview updates the selected overlay's bounds.
- `CaptionOverlay` now holds/draws all text overlays active at the current playhead, not just the selected overlay.
- Preview hit bounds are tracked for the selected overlay so dragging still targets the selected caption.
- The timeline bar now displays/edits the selected overlay's range.
- The exporter already iterates all `project.overlays`, so multiple text overlays should be included in GIF export via multiple `drawtext` filters.

Current limitation:

- This now follows the user's correction that GIF Brewery handles multi-overlay selection inside the overlay section, not primarily through timeline multi-lane editing.
- Timeline is still a single selected-overlay bar, which is acceptable for now if overlay list selection remains the primary workflow.
- The next session should focus manual testing on the overlay-section list workflow before doing any timeline overlay-lane expansion.
- Visual smoke does not exercise add/select/delete because it only opens tabs and screenshots. Manual testing should specifically:
  - add a second overlay
  - select it in the overlay list
  - set separate appear/disappear times from the playhead
  - scrub/play through both ranges
  - confirm both captions appear/disappear independently
  - export and inspect that both captions are burned in at their configured times

Verification after the multi-overlay follow-up:

```bash
cargo fmt
cargo check
cargo build
./target/debug/gifbrewery-gtk --smoke-export "GIF Brewery 3.app/Contents/Resources/kvo.gif" /tmp/gifbrewery-gif-source-smoke.gif
tools/visual-smoke.sh
identify -format '%f %[mean] %[min] %[max]\n' /code/gifbrewery-visual-smoke/clip.png /code/gifbrewery-visual-smoke/gif.png /code/gifbrewery-visual-smoke/overlays.png
```

Results:

- `cargo check` clean.
- `cargo build` clean.
- GIF-source smoke export succeeded and wrote `/tmp/gifbrewery-gif-source-smoke.gif`.
- Visual smoke regenerated `/code/gifbrewery-visual-smoke/gifbrewery-gtk` at about `75M`.
- `/code/gifbrewery-visual-smoke/gifbrewery.log` exists beside the copied binary.
- Screenshot stats were nonblack:
  - `clip.png mean=55908.5 min=0 max=65535`
  - `gif.png mean=55840.1 min=0 max=65535`
  - `overlays.png mean=55677.1 min=0 max=65535`

## 2026-06-14 Overlay List Correction

User clarified that GIF Brewery selects multiple overlays inside the Overlays section: click an overlay there, and the controls map to that selected overlay. Timeline multi-lane overlay editing is not the important interaction.

Implemented:

- Replaced the `Selected overlay` dropdown with a clickable `gtk::ListBox` in the Overlays inspector.
- The list is repopulated whenever overlays are added/deleted or label text changes.
- Selecting a list row updates `selected_overlay_id`, and all controls below the list map to that overlay.
- `Add` and `Delete` remain in the Overlays section.

Verification after this correction:

```bash
cargo fmt
cargo check
cargo build
./target/debug/gifbrewery-gtk --smoke-export "GIF Brewery 3.app/Contents/Resources/kvo.gif" /tmp/gifbrewery-gif-source-smoke.gif
tools/visual-smoke.sh
identify -format '%f %[mean] %[min] %[max]\n' /code/gifbrewery-visual-smoke/clip.png /code/gifbrewery-visual-smoke/gif.png /code/gifbrewery-visual-smoke/overlays.png
```

Results:

- `cargo check` clean.
- `cargo build` clean.
- GIF-source smoke export succeeded.
- Visual smoke regenerated `/code/gifbrewery-visual-smoke/gifbrewery-gtk`.
- `/code/gifbrewery-visual-smoke/gifbrewery.log` exists beside the copied binary.
- Screenshot stats were nonblack:
  - `clip.png mean=55908.5 min=0 max=65535`
  - `gif.png mean=55840.1 min=0 max=65535`
  - `overlays.png mean=55745 min=0 max=65535`

## 2026-06-14 Multi-Overlay Smoke Coverage

Added deterministic smoke coverage for multiple timed text overlays:

- New CLI command:
  ```bash
  ./target/debug/gifbrewery-gtk --smoke-export-multi-overlay SOURCE OUTPUT
  ```
- This builds a project with two text overlays:
  - `FIRST`, visible from `0.00s` to `0.45s`
  - `SECOND`, visible from `0.55s` to `1.00s`
- It exports through the same `export::export_gif` path as the GUI.
- `tools/visual-smoke.sh` now runs the multi-overlay export after screenshots and writes:
  - `/code/gifbrewery-visual-smoke/export-multi-overlay.gif`
  - `/code/gifbrewery-visual-smoke/export-multi-overlay.log`
  - `/code/gifbrewery-visual-smoke/export-multi-overlay-first-frame.png`
  - `/code/gifbrewery-visual-smoke/export-multi-overlay-late-frame.png`

Verification:

```bash
cargo fmt
cargo check
cargo build
./target/debug/gifbrewery-gtk --smoke-export-multi-overlay "GIF Brewery 3.app/Contents/Resources/kvo.gif" /tmp/gifbrewery-multi-overlay-smoke.gif
tools/visual-smoke.sh
identify -format '%f %[mean] %[min] %[max]\n' /code/gifbrewery-visual-smoke/clip.png /code/gifbrewery-visual-smoke/gif.png /code/gifbrewery-visual-smoke/overlays.png /code/gifbrewery-visual-smoke/export-multi-overlay-first-frame.png /code/gifbrewery-visual-smoke/export-multi-overlay-late-frame.png
```

Results:

- `cargo check` clean.
- `cargo build` clean.
- Direct multi-overlay smoke export succeeded and wrote `/tmp/gifbrewery-multi-overlay-smoke.gif`.
- Visual smoke completed and wrote the multi-overlay artifacts under `/code/gifbrewery-visual-smoke`.
- Pixel stats were nonblack:
  - `clip.png mean=55908.5 min=0 max=65535`
  - `gif.png mean=55840.1 min=0 max=65535`
  - `overlays.png mean=55745 min=0 max=65535`
  - `export-multi-overlay-first-frame.png mean=65405.6 min=0 max=65535`
  - `export-multi-overlay-late-frame.png mean=65394 min=0 max=65535`

## 2026-06-14 GIF Loop Enforcement

User clarified that GIF Brewery should always create looping GIFs. Non-looping GIF output should not be possible from this app.

Implemented:

- `export::export_gif` still passes ffmpeg `-loop 0`.
- Added a binary-level post-export verifier:
  - reads the GIF application extension
  - accepts only Netscape/ANIMEXTS loop count `0`
  - returns an export error if the loop extension is missing or finite
- Added unit tests for:
  - infinite loop count `0`
  - finite loop count
  - missing loop extension

Verification:

```bash
cargo fmt
cargo test -p gifbrewery-gtk export::tests
cargo check
cargo build
./target/debug/gifbrewery-gtk --smoke-export "GIF Brewery 3.app/Contents/Resources/kvo.gif" /tmp/gifbrewery-gif-source-smoke.gif
./target/debug/gifbrewery-gtk --smoke-export-multi-overlay "GIF Brewery 3.app/Contents/Resources/kvo.gif" /tmp/gifbrewery-multi-overlay-smoke.gif
tools/visual-smoke.sh
identify -verbose /tmp/gifbrewery-gif-source-smoke.gif /tmp/gifbrewery-multi-overlay-smoke.gif /code/gifbrewery-visual-smoke/export-multi-overlay.gif | rg -n "Image:|Iterations|Scene"
```

Results:

- Export parser unit tests passed.
- `cargo check` clean.
- `cargo build` clean.
- Direct single-overlay and multi-overlay smoke exports succeeded.
- Full visual smoke succeeded.
- Checked GIFs report `Iterations: 0`, meaning infinite looping:
  - `/tmp/gifbrewery-gif-source-smoke.gif`
  - `/tmp/gifbrewery-multi-overlay-smoke.gif`
  - `/code/gifbrewery-visual-smoke/export-multi-overlay.gif`
- Screenshot/frame stats remained nonblack:
  - `clip.png mean=55908.5 min=0 max=65535`
  - `gif.png mean=55840.1 min=0 max=65535`
  - `overlays.png mean=55745 min=0 max=65535`
  - `export-multi-overlay-first-frame.png mean=65405.6 min=0 max=65535`
  - `export-multi-overlay-late-frame.png mean=65394 min=0 max=65535`

## 2026-06-14 GIF Open Crash Follow-Up

User reported that running the binary, opening a GIF, and loading it crashed.

Important process note:

- `tools/visual-smoke.sh` was already known-good with:
  `Xvfb -displayfd 3 -screen 0 "${SCREEN_SIZE}" -nolisten tcp`
- During this follow-up, running the GUI/Xvfb smoke inside the default sandbox caused Xvfb listener failures:
  `Failed to find a socket to listen on`
- That was a sandbox mistake, not evidence that the old Xvfb command was wrong.
- GUI/Xvfb smoke must be run with escalation/outside the sandbox.

Implemented defensive crash hardening:

- Added `normalized_range_for_clip` in `ui.rs`.
- `apply_source_file` now normalizes overlay ranges when media loads instead of blindly setting overlay end to `clip_end`.
- `clamp_overlays_to_clip` now uses the same helper.
- This avoids invalid `f64::clamp(min, max)` calls when a GIF reports a very short or unusual duration and prevents invalid overlay ranges after media load.

Verification:

```bash
cargo fmt
cargo check
cargo build
./target/debug/gifbrewery-gtk --smoke-export "GIF Brewery 3.app/Contents/Resources/kvo.gif" /tmp/gifbrewery-gif-source-smoke.gif
./target/debug/gifbrewery-gtk --smoke-export-multi-overlay "GIF Brewery 3.app/Contents/Resources/kvo.gif" /tmp/gifbrewery-multi-overlay-smoke.gif
OUT_DIR=/tmp/gifbrewery-gif-open-smoke SMOKE_SOURCE="/codex/gifbrewery/GIF Brewery 3.app/Contents/Resources/kvo.gif" tools/visual-smoke.sh
tools/visual-smoke.sh
```

The two `tools/visual-smoke.sh` commands were run escalated/outside the sandbox.

Results:

- `cargo check` clean.
- `cargo build` clean.
- GIF export smoke passed.
- Multi-overlay GIF export smoke passed.
- Escalated GIF-open visual smoke passed with the bundled `kvo.gif`.
- `/tmp/gifbrewery-gif-open-smoke/gifbrewery.log` showed:
  - GStreamer initialized
  - runtime dependency check passed
  - `kvo.gif` metadata loaded as duration `1.0`, dimensions `300 x 225`, FPS `20.0`
  - preview URI opened
  - no panic
- Standard `/code` visual smoke passed after rerunning escalated.
- Refreshed binary:
  `/code/gifbrewery-visual-smoke/gifbrewery-gtk`
- Screenshot stats:
  - `clip.png mean=55908.5 min=0 max=65535`
  - `gif.png mean=55840.1 min=0 max=65535`
  - `overlays.png mean=55745 min=0 max=65535`
  - `export-multi-overlay-first-frame.png mean=65405.6 min=0 max=65535`
  - `export-multi-overlay-late-frame.png mean=65394 min=0 max=65535`

If the user still sees a crash with a different GIF, ask them to send or point to:

- the exact GIF file if possible
- the adjacent log from the binary directory, usually:
  `/code/gifbrewery-visual-smoke/gifbrewery.log`

## 2026-06-14 Timeline Overlay Comforts

User requested:

- The small text-overlay bar above the scrubber should show all text overlays, not only the selected/current overlay.
- Overlapping text overlays should not make the lane taller; overlapping segments should split the existing lane vertically.
- The overlay that starts first should render on top; later overlapping overlays render on the bottom half.
- Overlay timeline segments should be draggable left/right to adjust their timing.
- Left/right arrow keys should step the playhead one frame backward/forward for precise mark-in/mark-out placement.

Implemented:

- `TimelineViewState` now carries `overlays: Vec<TimelineOverlayRange>` and `selected_overlay_id`.
- The overlay lane now draws all text overlays.
- Overlap handling:
  - non-overlapping bars use the top half of the lane
  - if a bar overlaps an earlier bar, it renders in the bottom half
  - lane height remains unchanged
- Overlay bars use a repeating color palette so multiple text layers are distinguishable.
- The selected overlay gets a white outline.
- Timeline hit-testing now works across all overlay bars.
- Dragging the body of an overlay bar shifts that whole overlay range left/right while preserving duration.
- Dragging overlay start/end handles still resizes the range.
- Dragging any overlay segment selects it, so the Overlays inspector maps to the moved overlay.
- Left/right arrow keys now step the playhead by one frame using the current clip FPS strategy.
- Frame-step actions log:
  `frame step: frames=... frame_duration=... next=...`

Verification:

```bash
cargo fmt
cargo check
cargo test -p gifbrewery-gtk export::tests
cargo build
./target/debug/gifbrewery-gtk --smoke-export "GIF Brewery 3.app/Contents/Resources/kvo.gif" /tmp/gifbrewery-gif-source-smoke.gif
./target/debug/gifbrewery-gtk --smoke-export-multi-overlay "GIF Brewery 3.app/Contents/Resources/kvo.gif" /tmp/gifbrewery-multi-overlay-smoke.gif
tools/visual-smoke.sh
```

The `tools/visual-smoke.sh` command was run escalated/outside the sandbox, which is required for Xvfb.

Results:

- `cargo check` clean.
- Export parser unit tests passed.
- `cargo build` clean.
- GIF export smoke passed.
- Multi-overlay GIF export smoke passed.
- Standard visual smoke passed and refreshed:
  `/code/gifbrewery-visual-smoke/gifbrewery-gtk`
- Screenshot/frame stats:
  - `clip.png mean=55824.3 min=0 max=65535`
  - `gif.png mean=55755.9 min=0 max=65535`
  - `overlays.png mean=55660.8 min=0 max=65535`
  - `export-multi-overlay-first-frame.png mean=65405.6 min=0 max=65535`
  - `export-multi-overlay-late-frame.png mean=65394 min=0 max=65535`

Manual validation still recommended:

- Add two overlays that partially overlap.
- Confirm both bars appear in the small lane.
- Confirm the earlier-starting overlay appears on top in the overlap and the later-starting overlay appears on bottom.
- Drag a whole overlay bar left/right and confirm its Appears/Disappears values both move while duration stays fixed.
- Use left/right arrow keys to step one frame and set appear/disappear marks from the playhead.

## 2026-06-14 Overlay Lane Follow-up

User provided `/code/gifbrewery-visual-smoke/Screenshot From 2026-06-14 14-37-46.png`
showing the overlay lane was still wrong: the later overlay was rendered half-height
for its entire duration instead of only for the time span that overlapped another
overlay. The screenshot also showed the unwanted initial `Lorem ipsum.` overlay,
which made newly added overlays hard to select when their preview text was stacked.

Implemented:

- Removed the default initial text overlay from the project and timeline defaults.
  A new project now starts with no overlays selected and no `Lorem ipsum.` text in
  the preview.
- Kept the Overlays inspector usable in the empty state:
  - Add remains available.
  - Delete and edit controls disable when no text overlay is selected.
  - Deleting the last overlay is allowed and returns the inspector to the empty
    state.
- New overlays still use the normal text defaults, but get a small position offset
  based on overlay count so multiple newly added overlays are not directly stacked
  on the exact same preview pixels.
- Added a visible preview selection indicator: the selected text overlay now draws
  a blue outer outline and white inner outline around its rendered text bounds.
- Reworked the small overlay lane above the filmstrip:
  - non-overlapping spans render at full lane height
  - only the overlapping time span is split into top/bottom halves
  - the earlier-starting overlay occupies the top half during overlap
  - the later-starting overlay occupies the bottom half during overlap
  - hit-testing uses the same per-segment geometry, so dragging/selecting follows
    what is actually drawn
- Cleaned the warning-producing temporary variables in `build_overlays_page`.

Verification:

```bash
cargo fmt
cargo check
cargo test -p gifbrewery-gtk export::tests
cargo build
./target/debug/gifbrewery-gtk --smoke-export "GIF Brewery 3.app/Contents/Resources/kvo.gif" /tmp/gifbrewery-gif-source-smoke.gif
./target/debug/gifbrewery-gtk --smoke-export-multi-overlay "GIF Brewery 3.app/Contents/Resources/kvo.gif" /tmp/gifbrewery-multi-overlay-smoke.gif
tools/visual-smoke.sh
identify /code/gifbrewery-visual-smoke/*.png
```

Results:

- `cargo check` clean.
- Export parser unit tests passed: 3 passed.
- `cargo build` clean.
- GIF export smoke passed.
- Multi-overlay GIF export smoke passed.
- Xvfb visual smoke passed and refreshed:
  - `/code/gifbrewery-visual-smoke/gifbrewery-gtk`
  - `/code/gifbrewery-visual-smoke/clip.png`
  - `/code/gifbrewery-visual-smoke/gif.png`
  - `/code/gifbrewery-visual-smoke/overlays.png`
  - `/code/gifbrewery-visual-smoke/export-multi-overlay-first-frame.png`
  - `/code/gifbrewery-visual-smoke/export-multi-overlay-late-frame.png`
- Fresh PNG dimensions were valid/nonzero, including:
  - `clip.png PNG 1280x820`
  - `gif.png PNG 1280x820`
  - `overlays.png PNG 1280x820`

Remaining manual check:

- In the GUI, add two overlays whose ranges partially overlap. Confirm the
  non-overlapped parts of each segment are full height, and only the shared time
  span is split top/bottom.

## 2026-06-14 Continuity Notes For Next Session

Read this section first after `AGENTS.md`.

Current budget/runway situation:

- The user wants an API burn report at the end of each turn.
- Use:

```bash
/codex/todo-worker status
```

- The useful fields are:
  - `rate_limit: used_5h=... available_5h=... used_week=...`
  - `rate_limit_action`
  - `burn_rate_5h`
- As of this handoff, the dashboard showed:
  - `used_5h=98.0%`
  - `available_5h=2.0%`
  - `used_week=45.0%`
  - `rate_limit_action: pause (hard stop threshold reached)`
- Do not start a large implementation while this is still near the hard stop.
  Prefer one narrow fix, direct command verification, then stop.

Workspace continuity:

- This workspace is not a git repository. Do not expect `git status` to help.
- `AGENTS.md` exists in `/codex/gifbrewery` and instructs agents to read this
  handoff before editing.
- Source root:
  `/codex/gifbrewery`
- User-visible/shared artifacts should go under:
  `/code`
- Primary visual smoke artifact directory:
  `/code/gifbrewery-visual-smoke`
- Refreshed binary from the smoke script:
  `/code/gifbrewery-visual-smoke/gifbrewery-gtk`
- Runtime/debug log location depends on where the binary is run from. The app
  intentionally writes the log next to the executable. For the smoke binary, check:
  `/code/gifbrewery-visual-smoke/gifbrewery.log`
- If the user runs another copied binary, ask them for the adjacent
  `gifbrewery.log` beside that binary.

Reference/example locations:

- Original app bundle in this workspace:
  `/codex/gifbrewery/GIF Brewery 3.app`
- Original shared copy:
  `/code/GIF Brewery 3.app`
- Bundled GIF Brewery sample used by smoke tests:
  `/codex/gifbrewery/GIF Brewery 3.app/Contents/Resources/kvo.gif`
- Porting/reverse-engineering notes:
  `docs/PORTING_NOTES.md`
- Open-source reference already present:
  `reference/gifcurry`
- User previously said LosslessCut and Instagiffer had already been cloned under
  `/code` and/or `/codex`; check those roots before trying to clone or browse.

Commands that were useful today:

```bash
cargo fmt
cargo check
cargo test -p gifbrewery-gtk export::tests
cargo build
./target/debug/gifbrewery-gtk --smoke-export "GIF Brewery 3.app/Contents/Resources/kvo.gif" /tmp/gifbrewery-gif-source-smoke.gif
./target/debug/gifbrewery-gtk --smoke-export-multi-overlay "GIF Brewery 3.app/Contents/Resources/kvo.gif" /tmp/gifbrewery-multi-overlay-smoke.gif
tools/visual-smoke.sh
identify /code/gifbrewery-visual-smoke/*.png
```

Important obstacle lessons:

- Run `tools/visual-smoke.sh` escalated/outside the sandbox. Xvfb worked when
  run that way; trying to run GUI/Xvfb in the normal sandbox previously caused
  socket/listener style failures and wasted time.
- The visual smoke screenshots can be nonblack even when Xvfb prints the known
  software-rendering/DRI warning. Verify with `identify` or ImageMagick stats
  instead of assuming the warning means failure.
- The app logs GStreamer/runtime issues and UI actions. If the user reports a
  crash, first inspect the adjacent `gifbrewery.log`.
- The export smoke commands do not prove the GTK timeline interaction works, but
  they quickly catch exporter regressions and GIF loop issues.
- The visual smoke does prove the app can render under headless GTK and refreshes
  the binary/artifacts in `/code/gifbrewery-visual-smoke`.

Current implementation hotspots:

- Core model:
  `crates/gifbrewery-core/src/model.rs`
- Main GTK UI and inspector wiring:
  `crates/gifbrewery-gtk/src/ui.rs`
- Timeline drawing, hit testing, dragging, frame stepping:
  `crates/gifbrewery-gtk/src/timeline.rs`
- Media/GStreamer preview:
  `crates/gifbrewery-gtk/src/media.rs`
- GIF export:
  `crates/gifbrewery-gtk/src/export.rs`
- Visual smoke:
  `tools/visual-smoke.sh`

Details from today's overlay work that are easy to forget:

- `Project::default()` now starts with `overlays: Vec::new()`.
- `TimelineViewState::default()` also starts with no overlays and
  `selected_overlay_id: None`.
- `TextOverlay::default_caption()` still says `Lorem ipsum.` because it is still
  used as a convenient factory for newly created text overlays before the UI
  immediately overwrites `id` and visible `text`. Do not mistake that function
  alone for a live default project overlay.
- New overlay creation lives in `add_text_overlay_at_playhead` and now offsets
  bounds slightly by overlay count to avoid stacking text directly on identical
  pixels.
- Empty overlay inspector behavior is handled in `build_overlays_page` and
  `update_timeline_widgets`; edit controls are disabled when no overlay is
  selected, but Add stays active.
- Deleting the last overlay is intentionally allowed now.
- Preview selected-overlay indication is drawn in `draw_selected_caption_bounds`
  after `draw_caption_overlay` computes the selected text's rendered pixel bounds.
- Timeline overlap drawing is now per-segment:
  - `overlay_bar_segments`
  - `overlay_lane_at`
  - `overlay_segment_geometry`
  - `overlay_handle_geometry`
- The lane should not assign a whole overlay to half-height just because it
  overlaps somewhere. Only the exact overlapping span should split vertically.
- Hit testing uses the same segment geometry; keep it in sync with drawing if the
  lane layout changes again.

Known manual validation still worth doing when budget recovers:

- Launch `/code/gifbrewery-visual-smoke/gifbrewery-gtk`.
- Open a GIF/video.
- Confirm a fresh project starts with no `Lorem ipsum.` text in the preview or
  overlay list.
- Add two text overlays whose timing ranges partially overlap.
- Confirm each non-overlapped part is full lane height and only the shared time
  span splits top/bottom.
- Confirm the selected overlay outline in the preview makes it obvious which text
  layer is active.
- Drag overlay lane segments left/right and confirm the Overlays inspector's
  Appears/Disappears values move with the segment.

## 2026-06-14 Font Family Picker Bug

User-reported unresolved bug:

- The current `Font family` control is broken for real use.
- When typing/searching in it, the text appears to move right-to-left or otherwise behaves incorrectly.
- It is effectively impossible for the user to find or select a different font.
- This is blocking because the user plans to install 1980s/1990s IBM PC BIOS-style fonts to reproduce Monkey Island-style on-screen text, and the app must make newly installed fonts discoverable/selectable.

Important next-session guidance:

- Treat this as a real product requirement, not a cosmetic issue.
- The current control is an `adw::EntryRow` for raw font-family text, which is likely the wrong UX and may also be part of the broken typing behavior.
- Investigate replacing it with a native GTK font chooser/control or a custom searchable font list populated from Fontconfig/Pango.
- The user needs to select installed system fonts reliably after installing new font files.
- Add debug logging around font enumeration/selection if this area is touched, because previous user reports included choking/crashing while interacting with font selection.

## 2026-06-15 Font Family Picker Fix

Implemented a first-pass fix for the broken `Font family` selector.

Problem being addressed:

- The old font control was an `adw::EntryRow` that required typing raw font
  family names.
- User reported the field behaved unusably while typing/searching, with text
  movement appearing wrong and no reliable way to find/select installed fonts.
- User plans to install 1980s/1990s IBM PC BIOS-style fonts for Monkey
  Island-style overlay captions, so installed system fonts need to be selectable
  through the UI.

Implementation:

- Replaced the raw `Font family` entry with `gtk::FontDialogButton`.
- Configured the button as family-only selection:
  - `FontLevel::Family`
  - `use_font=true`
  - `use_size=false`
- Kept font size controlled by the existing separate `Font size` row.
- Added a `Refresh` button beside the font control. It calls
  `pangocairo::FontMap::default().changed()` and logs the resulting detected
  family count. This is meant to help after installing new font files without
  restarting the entire debugging flow.
- Font selection now listens to `notify::font-desc`, extracts the selected Pango
  family name, logs the selected description, and updates the selected text
  overlay's `font_family`.
- Programmatic widget sync updates the `FontDialogButton` font description
  instead of writing text into an entry.
- Empty overlay state still disables font controls until a text overlay exists.

Files touched:

- `crates/gifbrewery-gtk/src/ui.rs`

Verification:

```bash
cargo fmt
cargo check
cargo test -p gifbrewery-gtk export::tests
cargo build
tools/visual-smoke.sh
identify /code/gifbrewery-visual-smoke/clip.png /code/gifbrewery-visual-smoke/gif.png /code/gifbrewery-visual-smoke/overlays.png
rg -n "font picker|runtime dependency|panic|error" /code/gifbrewery-visual-smoke/gifbrewery.log /code/gifbrewery-visual-smoke/app.log
```

Results:

- `cargo check` clean.
- Export parser unit tests passed: 3 passed.
- `cargo build` clean.
- Xvfb visual smoke passed and refreshed `/code/gifbrewery-visual-smoke`.
- Fresh screenshot dimensions:
  - `clip.png PNG 1280x820`
  - `gif.png PNG 1280x820`
  - `overlays.png PNG 1280x820`
- Smoke log showed:
  - `font picker initialized: families=21 selected=Sans`
  - `runtime dependency check passed`
- No panic/error lines appeared in the searched smoke logs.

Manual validation still needed:

- Install the user's IBM PC BIOS-style fonts.
- Launch the refreshed `/code/gifbrewery-visual-smoke/gifbrewery-gtk` binary.
- Add/select a text overlay.
- Open `Font family`.
- Confirm the newly installed fonts appear in the font dialog and can be selected.
- Confirm selected font updates the preview and exported GIF text.
- If fonts do not appear immediately, press `Refresh` and check adjacent
  `gifbrewery.log` for `font picker refreshed: families=...`.

## 2026-06-15 Text Entry Reversal And Font Row Follow-up

User provided a fresh screenshot:

- `/code/gifbrewery-visual-smoke/Screenshot From 2026-06-15 16-52-17.png`

Observed from screenshot:

- User appeared to type `hello`, but the text field/preview showed `olleh`.
- This was caused by the text row being rewritten from model state after every
  `changed` signal. GTK moved the insertion point back to the start after each
  programmatic `set_text`, so the next typed character inserted before the
  previous ones.
- The `Font family` row was also unusable visually. The embedded
  `gtk::FontDialogButton` rendered a wide font-preview area full of missing-glyph
  boxes and squeezed the `Font family` title into vertical wrapping.

Implemented:

- `update_timeline_widgets` no longer calls `set_text` on the overlay text
  `EntryRow` while the row has keyboard focus. This preserves the user's cursor
  and should stop normal typing from reversing.
- Removed the embedded `gtk::FontDialogButton` from the sidebar.
- Replaced it with a normal `adw::ActionRow`:
  - title: `Font family`
  - subtitle: current selected overlay font family
  - suffix button: `Choose`
- The `Choose` button opens `gtk::FontDialog::choose_family` parented to the main
  window, then writes the selected Pango family name into the selected overlay.
- Kept the separate `Refresh` button for nudging Pango's font map after installing
  new fonts.

Verification:

```bash
cargo fmt
cargo check
cargo test -p gifbrewery-gtk export::tests
cargo build
tools/visual-smoke.sh
identify /code/gifbrewery-visual-smoke/clip.png /code/gifbrewery-visual-smoke/gif.png /code/gifbrewery-visual-smoke/overlays.png
```

Results:

- `cargo check` clean.
- Export parser unit tests passed: 3 passed.
- `cargo build` clean.
- Xvfb visual smoke passed and refreshed `/code/gifbrewery-visual-smoke`.
- Fresh `overlays.png` showed the font row as a readable `Font family` row with
  a compact `Choose` button and no garbled inline glyph preview.

Manual validation still needed:

- Run `/code/gifbrewery-visual-smoke/gifbrewery-gtk`.
- Add a text overlay.
- Type `hello` into the Text row and confirm it remains `hello`, not `olleh`.
- Click `Choose`, select a font family, and confirm the row subtitle, preview,
  and export all use the selected family.

## 2026-06-15 Font Selection Crash Follow-up

User reported the app crashed while selecting a particular font.

Log finding:

- The user-run log in `/code/gifbrewery-visual-smoke/gifbrewery.log` ended after:
  `font family dialog opened: families=243`
- There was no successful selection callback after that point, which strongly
  suggests the native GTK font dialog crashed while browsing/rendering a specific
  font family or preview.

Implemented:

- Removed the native `gtk::FontDialog::choose_family` path.
- Replaced it with an app-owned modal font picker:
  - `gtk::Window`
  - `gtk::SearchEntry`
  - `gtk::ListBox`
  - rows are plain labels rendered in the normal UI font
- The custom list stores/selects only the family name string. It does not preview
  each font, which avoids crashing just because a selected/installed font has
  unusual glyph coverage or rendering behavior.
- Search filters the in-memory Pango family-name list.
- Row activation logs:
  `font family list selected: family=...`
- Opening logs:
  `font family list opened: families=...`

Verification:

```bash
cargo fmt
cargo check
cargo test -p gifbrewery-gtk export::tests
cargo build
tools/visual-smoke.sh
identify /code/gifbrewery-visual-smoke/clip.png /code/gifbrewery-visual-smoke/gif.png /code/gifbrewery-visual-smoke/overlays.png
```

Results:

- `cargo check` clean.
- Export parser unit tests passed: 3 passed.
- `cargo build` clean.
- Xvfb visual smoke passed and refreshed `/code/gifbrewery-visual-smoke`.
- Fresh PNG dimensions were valid/nonzero.

Manual validation still needed:

- Run refreshed `/code/gifbrewery-visual-smoke/gifbrewery-gtk`.
- Add/select a text overlay.
- Click `Choose`.
- Search for and select the previously crashing font.
- Confirm the app no longer crashes and the selected family appears in the row
  subtitle.

## 2026-06-16 Frame-Accurate Arrow Key Stepping

User reported:

- Imported MP4 arrow-key stepping does not show every frame.
- Left/right arrow keys appear to move only across keyframes/P-frames.
- Requirement: arrow keys must step through every visible source frame so the
  playhead can land on the exact frame for overlay appear/disappear marks.

Root causes found:

- Preview seeks used:
  `gst::SeekFlags::FLUSH | gst::SeekFlags::KEY_UNIT`
- `KEY_UNIT` explicitly asks GStreamer to seek to keyframes/key units, which is
  the wrong behavior for frame-by-frame overlay placement.
- Arrow-key stepping used the clip/export FPS strategy rather than the imported
  source media FPS. If the source is 10 fps but export settings are 12 fps, arrow
  stepping lands on export-frame timestamps instead of source-frame timestamps.

Implemented:

- Added `fps: Option<f64>` to `MediaSource` with `#[serde(default)]` for backward
  compatibility.
- Store discovered media FPS in `Project.source` during media import.
- Arrow-key stepping now prefers source FPS from imported media metadata.
- Arrow-key stepping now snaps through integer source frame indices:
  `next_seconds = next_frame / source_fps`
  instead of repeatedly adding a floating-point delta.
- Frame-step logging now includes:
  `frames`, `fps`, `frame_duration`, `next_frame`, and `next`.
- Preview seeks no longer use `KEY_UNIT`.
- Preview seek path now tries:
  1. `FLUSH | ACCURATE`
  2. fallback to plain `FLUSH`
  3. logs both errors if both fail
- Removed the unnecessary immediate seek during `VideoPreview::open_file`; it
  could fail before the GStreamer pipeline had prerolled and did not help frame
  stepping.

Files touched:

- `crates/gifbrewery-core/src/model.rs`
- `crates/gifbrewery-gtk/src/main.rs`
- `crates/gifbrewery-gtk/src/media.rs`
- `crates/gifbrewery-gtk/src/ui.rs`

Verification:

```bash
cargo fmt
cargo check
cargo test -p gifbrewery-gtk export::tests
cargo build
tools/visual-smoke.sh
identify /code/gifbrewery-visual-smoke/clip.png /code/gifbrewery-visual-smoke/gif.png /code/gifbrewery-visual-smoke/overlays.png
```

Results:

- `cargo check` clean.
- Export parser unit tests passed: 3 passed.
- `cargo build` clean.
- Xvfb visual smoke passed and refreshed `/code/gifbrewery-visual-smoke`.
- Fresh PNG dimensions were valid/nonzero.
- `app.log` showed media metadata with FPS and no immediate startup seek after
  `opening preview URI`.

Manual validation still needed:

- Run refreshed `/code/gifbrewery-visual-smoke/gifbrewery-gtk`.
- Open an MP4 with known FPS.
- Press left/right arrows and inspect adjacent `gifbrewery.log`.
- Confirm `frame step` lines show the source FPS and `next_frame` increments or
  decrements by exactly 1.
- Visually confirm the preview changes on every source frame, not only keyframes.

## 2026-06-16 Text Editing Keyboard Shortcut Guard

User reported:

- Spacebar playback control is useful, but when the overlay text field has focus,
  Space must insert a space into the text.
- Left/right arrow keys must also behave like normal text editing keys while the
  text field has focus.

Implemented:

- Added `text_editing_has_focus(window)` helper.
- Global keyboard shortcuts now check focus before handling Space/Left/Right.
- If focus is in or under an editable text widget, the shortcut handler returns
  `Propagation::Proceed` so GTK text editing receives the key event.
- Covered focus in:
  - `adw::EntryRow`
  - `gtk::Entry`
  - `gtk::SearchEntry`
  - `gtk::Text`
  - descendants of those widgets
- Logs ignored shortcuts while text editing:
  `keyboard shortcut ignored for text editing focus: key=...`

Verification:

```bash
cargo fmt
cargo check
cargo test -p gifbrewery-gtk export::tests
cargo build
tools/visual-smoke.sh
identify /code/gifbrewery-visual-smoke/clip.png /code/gifbrewery-visual-smoke/gif.png /code/gifbrewery-visual-smoke/overlays.png
```

Results:

- `cargo check` clean.
- Export parser unit tests passed: 3 passed.
- `cargo build` clean.
- Xvfb visual smoke passed and refreshed `/code/gifbrewery-visual-smoke`.

Manual validation still needed:

- Run refreshed `/code/gifbrewery-visual-smoke/gifbrewery-gtk`.
- Add/select a text overlay.
- Focus the Text field.
- Confirm Space inserts spaces into overlay text instead of toggling playback.
- Confirm Left/Right move the text cursor instead of frame-stepping the preview.
- Click away from the text field and confirm Space/Left/Right resume playback and
  frame-step shortcuts.

## 2026-06-16 High-Quality GIF Export And Overlay UX Pass

User dropped these comparison files in `/code/gifbrewery-visual-smoke`:

- `video_pug.mp4`
- `test_pug.gif`

Observed with `ffprobe`:

- `video_pug.mp4`: 960x540, about 45.788s, 30 fps, 1372 frames.
- `test_pug.gif`: 960x540, 4.5s, 54 frames, about 12 fps, 11.6 MB.

Root cause for low-framerate export:

- Export used `clip.frame_strategy`, whose default was `FrameStrategy::Fps(12)`.
- Smoke export projects did not populate source FPS, so the exporter could not
  preserve the imported MP4 cadence.

Implemented:

- GIF export now prefers discovered source FPS when available.
- Smoke export now initializes GStreamer before metadata discovery and stores
  source duration, dimensions, and FPS in the smoke `Project`.
- GIF export default `optimize` is now false.
- Export palette size is forced to the GIF ceiling of 256 colors. GIF cannot
  preserve more than 256 colors, but the app should not voluntarily reduce it.
- Text export escaping now supports multiline text by translating overlay
  newlines to ffmpeg `drawtext` line breaks and stripping carriage returns.
- Overlay text input changed from single-line `EntryRow` to multiline
  `gtk::TextView` inside a scrolled row.
- Global Space/Left/Right shortcuts now only yield to the overlay text editor,
  not to every entry/spin/search control. Clicking font-size buttons or the
  preview should return keyboard control to playback/frame stepping.
- Preview overlay rendering now records overlay bounds and supports hit testing.
  Drag begin on a text overlay selects that overlay before moving it, so the
  inspector maps to the clicked overlay.
- Overlay inspector layout was reorganized:
  - Top group: `Overlays` list plus Add/Delete.
  - Second group: `Selected Overlay Options`.
  - This keeps the overlay list visible above the font/config controls and makes
    the selected overlay area visually distinct from configuration options.

Verification:

```bash
cargo fmt
cargo check
cargo test -p gifbrewery-gtk export::tests
cargo build
tools/visual-smoke.sh
identify /code/gifbrewery-visual-smoke/clip.png /code/gifbrewery-visual-smoke/gif.png /code/gifbrewery-visual-smoke/overlays.png
./target/debug/gifbrewery-gtk --smoke-export /code/gifbrewery-visual-smoke/video_pug.mp4 /tmp/gifbrewery-pug-source-fps.gif
ffprobe /tmp/gifbrewery-pug-source-fps.gif
```

Results:

- `cargo check` clean.
- Export parser unit tests passed: 3 passed.
- `cargo build` clean.
- Xvfb visual smoke passed and refreshed `/code/gifbrewery-visual-smoke`.
- Fresh PNG dimensions were valid/nonzero.
- Direct pug smoke export produced a 3.0s, 90-frame GIF at source cadence
  (about 30 fps). Size was about 18.6 MB for 3 seconds at 960x540 with 256-color
  palette, so preserving all frames/colors will hit social file-size limits fast.
- Latest overlay screenshot:
  `/code/gifbrewery-visual-smoke/overlays.png`

Manual validation still needed:

- Open `/code/gifbrewery-visual-smoke/video_pug.mp4` in the refreshed binary.
- Confirm exported GIF preserves source frame cadence unless the user explicitly
  changes export settings later.
- Confirm Enter creates line breaks in overlay text and export preserves them.
- Confirm Space/Left/Right edit text only while the multiline text editor has
  focus, then return to playback/frame-step after clicking the preview or other
  non-text controls.
- Confirm clicking or dragging a preview overlay selects it and visibly updates
  the selected overlay/inspector state.
- If a pure click on preview text does not select, add a `GestureClick` handler
  using the same `CaptionOverlay::hit_test` path currently used by drag begin.

API dashboard note:

- Check runway with `/codex/todo-worker status`.
- At this handoff the dashboard reported `used_5h=100.0% available_5h=0.0%`,
  `used_week=63.0%`, `rate_limit_action: pause`.

## 2026-06-16 Export Fidelity, Async Export, Crop/Resize Controls

User reported:

- Text in exported GIFs looked visibly different from the preview: exported text
  was smaller while stroke width kept the same numeric value.
- Exporting MP4s could make Ubuntu show the force-quit dialog, indicating the UI
  thread was blocked by synchronous ffmpeg work.
- After export there was no in-app confirmation or preview.
- The pug MP4 can produce huge GIFs, so users need crop/resize controls and the
  target-size workflow should not drop frames/colors first.
- Overlay text should support carriage returns/newlines.

Implemented:

- Preview text now uses Pango absolute pixel size instead of point size, so the
  same font-size number is much closer to ffmpeg `drawtext` pixels.
- Export geometry now computes source size, crop size, output size, and text
  scale in one place.
- Text font size and stroke width are scaled with output height during export,
  so resized GIFs keep text/stroke proportions instead of preserving a stale
  absolute stroke number.
- Export filter chain now supports:
  - source FPS preservation
  - crop margins
  - lanczos resize
  - text overlays after crop/resize
  - 256-color palette generation
  - infinite GIF loop
- GIF tab now has:
  - Output Size: Width, Height (`0` means use cropped source size)
  - Crop: Left %, Right %, Top %, Bottom %
- Crop margins are normalized so users cannot crop to an impossible empty frame.
- Export now runs on a background thread instead of the GTK callback/UI thread.
- Export button changes to `Exporting...`, disables during export, and restores
  when the worker result arrives.
- Successful export opens a small `GIF Exported` preview window with the rendered
  GIF and an Open button.
- Export errors are surfaced in an in-app alert dialog.
- If no explicit output width/height is set and the GIF exceeds the target byte
  budget, the exporter retries by reducing dimensions. It preserves frames and
  palette size first; it does not drop frame rate or reduce colors as the first
  size-control mechanism.
- Multiline overlay text export now uses ffmpeg `drawtext=textfile=...` temp
  files instead of fragile inline `text='...'` escaping. This fixed newlines
  rendering as the letter `n`.
- Added smoke command:
  `gifbrewery-gtk --smoke-export-layout SOURCE OUTPUT`
  which exports a 2-second cropped/resized 640x360 GIF with multiline stroked
  text.

Smoke artifacts:

- `/code/gifbrewery-visual-smoke/export-layout-pug.gif`
- `/code/gifbrewery-visual-smoke/export-layout-pug-first-frame.png`

Validation:

```bash
cargo fmt
cargo check
cargo test -p gifbrewery-gtk export::tests
cargo build
tools/visual-smoke.sh
./target/debug/gifbrewery-gtk --smoke-export-layout /code/gifbrewery-visual-smoke/video_pug.mp4 /code/gifbrewery-visual-smoke/export-layout-pug.gif
ffprobe -v error -select_streams v:0 -show_entries stream=width,height,nb_frames,r_frame_rate,avg_frame_rate,duration -of default=noprint_wrappers=1 /code/gifbrewery-visual-smoke/export-layout-pug.gif
ffmpeg -hide_banner -y -i /code/gifbrewery-visual-smoke/export-layout-pug.gif -frames:v 1 -update 1 /code/gifbrewery-visual-smoke/export-layout-pug-first-frame.png
```

Results:

- `cargo check` clean.
- Export parser unit tests passed: 3 passed.
- `cargo build` clean.
- Standard Xvfb visual smoke passed and refreshed `/code/gifbrewery-visual-smoke`.
- Layout smoke with `video_pug.mp4` produced:
  - 640x360
  - 2.000s
  - 60 frames
  - r_frame_rate 30/1
  - about 5.5 MB
- Visual inspection of
  `/code/gifbrewery-visual-smoke/export-layout-pug-first-frame.png` confirmed
  multiline text rendered as two lines with stroke.

Manual validation still needed:

- Run `/code/gifbrewery-visual-smoke/gifbrewery-gtk` outside the container.
- Export a user-tuned overlay and confirm preview text size/stroke now matches
  the final GIF closely.
- Confirm Ubuntu no longer shows a force-quit dialog during MP4 export.
- Confirm the export preview window appears after normal Save GIF export.
- Try leaving width/height at `0` on a long clip and verify the automatic
  target-size retry chooses smaller dimensions instead of reducing FPS/colors.

## 2026-06-16 Follow-Up: Preview Fidelity, Focus, Export Feedback, Crop Cue

User reported after the previous pass:

- Preview text and exported text were still visibly different.
- Arrow-key playhead control could be lost after interacting with overlays.
- The generated GIF preview window appeared, but no GIF loaded inside it.
- Export still lacked a clear visual progress cue.
- Overlays could be created before media load and interfere with the media
  opener.
- The open-media control was not clear enough.
- The 12 fps selector is not a workflow the user wants.
- Numeric crop controls need a visual crop cue; Instagiffer source was requested
  for comparison, but no Instagiffer checkout was found under `/code` or
  `/codex` during this session.

Implemented:

- Preview overlay text now scales from source-media pixels into preview pixels.
  Export already scales source pixels into output pixels, so preview/export now
  share the same coordinate system for font size and stroke width.
- Export now resolves the selected font family through `fc-match` and passes
  ffmpeg `drawtext` a `fontfile=...` when possible. This should better match the
  Pango/fontconfig face used in the GTK preview.
- Preview/caption widgets are focusable and grab focus when clicked or dragged.
  This prevents stale focus in the overlay text editor from continuing to steal
  Space/Left/Right after preview interaction.
- Added a preview click handler that selects the clicked text overlay, not only
  drag-begin selection.
- Export progress is now visible in the header:
  - `Create GIF` changes to `Exporting...`
  - a spinner starts
  - status label shows `Exporting GIF...`
- Export completion preview now uses `gtk::Video` with autoplay and loop instead
  of `gtk::Picture`, which was unreliable for animated GIF playback.
- Header open control is now a clear text button: `Open Media`.
- The empty-state media opener is layered above caption overlays.
- Overlay Add is disabled until a media source is loaded.
- Clip FPS row is now a disabled `Source frame rate` display instead of an
  editable frame-rate workflow.
- Added a crop guide overlay over the preview:
  - shaded discarded areas when crop margins are nonzero
  - visible crop rectangle
  - rule-of-thirds guide lines

Verification:

```bash
cargo fmt
cargo check
cargo test -p gifbrewery-gtk export::tests
cargo build
tools/visual-smoke.sh
./target/debug/gifbrewery-gtk --smoke-export-layout /code/gifbrewery-visual-smoke/video_pug.mp4 /code/gifbrewery-visual-smoke/export-layout-pug.gif
ffprobe -v error -select_streams v:0 -show_entries stream=width,height,nb_frames,r_frame_rate,avg_frame_rate,duration -of default=noprint_wrappers=1 /code/gifbrewery-visual-smoke/export-layout-pug.gif
ffmpeg -hide_banner -y -i /code/gifbrewery-visual-smoke/export-layout-pug.gif -frames:v 1 -update 1 /code/gifbrewery-visual-smoke/export-layout-pug-first-frame.png
```

Results:

- `cargo check` clean.
- Export parser tests passed: 3 passed.
- `cargo build` clean.
- Xvfb visual smoke passed and refreshed `/code/gifbrewery-visual-smoke`.
- `clip.png`, `gif.png`, and `overlays.png` are valid nonblack PNGs.
- The latest `clip.png` shows the crop guide and disabled source-frame-rate row.
- Layout smoke remains 640x360, 2.0s, 60 frames, source-rate cadence.
- Visual inspection of
  `/code/gifbrewery-visual-smoke/export-layout-pug-first-frame.png` confirmed
  multiline stroked text still renders correctly.

Manual validation still needed:

- Confirm a real user-tuned overlay now matches between preview and export. This
  is much closer by construction, but exact font rasterization may still differ
  slightly between Pango/Cairo and ffmpeg/freetype.
- Confirm the export preview window animates the GIF outside Xvfb.
- Confirm Space/Left/Right return to timeline control after clicking or dragging
  a preview overlay.
- Later: make the crop rectangle draggable/resizable directly in the preview.

## 2026-06-16 End Of Day Handoff: Exact Preview Required

API dashboard:

- Use `/codex/todo-worker status` to check API runway.
- At this handoff it reported about `used_5h=92.0% available_5h=8.0%`.
- Do not start a broad rewrite until the window refreshes.

Current user reports:

- Cropping is visually confusing. The preview can appear zoomed to the cropped
  output while the old crop guide/box still represents the uncropped source.
  That makes the crop controls feel delayed and inaccurate.
- The preview font, playback font, and exported font can still differ. The user
  explicitly wants the preview to be exactly the output, not a fake overlay.
- The current architecture mixes a live GStreamer video player with GTK/Pango
  overlays and then exports through ffmpeg `drawtext`. That is the root of the
  fidelity problem.
- The user believes Instagiffer’s more reliable model was extracting frames to
  still images, applying operations to those images, previewing those rendered
  frames, then reconstructing the GIF. The next session should seriously follow
  that lead.
- The MP4 test file has music, but GIF output does not need audio. Do not let
  audio playback drive preview/export fidelity decisions.

Immediate code changes made right before this handoff:

- Fixed crop-related text scaling in `crates/gifbrewery-gtk/src/export.rs`:
  text scale is now `output_height / crop_height`, not
  `output_height / source_height`. Cropping alone should no longer shrink text.
- Added export diagnostics:
  - `export geometry: source_height=... crop=... output=... text_scale=...`
  - `export text overlay: id=... configured_font_size=... rendered_font_size=...`
    plus stroke, bounds, and timing.
- Added ffmpeg progress parsing with `-progress pipe:2`.
- Added exact single-frame render helper:
  `export::render_frame_png(project, playhead_seconds, output_path)`.
- Added smoke CLI:
  `gifbrewery-gtk --smoke-render-frame SOURCE OUTPUT`.
- Hid the crop overlay when an exact rendered preview frame is visible, so the
  UI no longer shows a cropped output image with an uncropped source crop box.
- During playback, the approximate GTK/Pango caption overlay is hidden instead
  of pretending to be exact.
- Rebuilt and copied the binary to:
  `/code/gifbrewery-visual-smoke/gifbrewery-gtk`.

Validation from this pass:

```bash
cargo fmt
cargo check
cargo test -p gifbrewery-gtk export::tests
cargo build
```

Additional smoke artifacts from the crop text-scale fix:

- `/code/gifbrewery-visual-smoke/render-frame-preview-fixed.png`
- `/code/gifbrewery-visual-smoke/export-layout-fixed.gif`
- `/code/gifbrewery-visual-smoke/export-layout-fixed-first-frame.png`

Important known limitation:

- The exact preview path currently renders a static PNG for the paused/current
  frame. Playback is still the GStreamer video player. Since the fake overlay is
  hidden during playback, playback will not show exact rendered captions. This
  is intentional for now; showing fake captions was misleading.

Recommended next architecture:

1. Build an Instagiffer-style frame workspace:
   - create a per-session temp directory
   - extract the selected clip to image frames at source FPS
   - preserve exact frame index mapping for arrow-key stepping and overlay timing
2. Make all editing preview come from rendered frames:
   - crop source frame
   - resize/output transform
   - apply text overlays using the same renderer as export
   - display the rendered PNG frame in the preview
3. Export GIF from those same rendered frames:
   - do not reimplement a separate preview path and export path
   - if preview frame N looks right, exported frame N should be the same pixels
4. Keep GStreamer only as an optional source playback/helper path, not as the
   authoritative preview for caption/crop/export results.
5. Add frame-cache invalidation:
   - source/clip range/FPS changes invalidate extracted frames
   - crop/output size/text/style changes invalidate rendered frames
   - playhead movement should render or load only the current frame quickly

Required smoke tests to add next:

- Create a smoke mode that programmatically builds a project with:
  - loaded MP4 source, preferably `/code/gifbrewery-visual-smoke/video_pug.mp4`
  - at least two text overlays
  - different overlay timing ranges
  - non-default font size, stroke width, color, and crop
- Render the exact preview frame to PNG at a timestamp where text is visible.
- Export the GIF from the same project.
- Extract the corresponding frame from the exported GIF.
- Compare preview PNG and extracted GIF frame:
  - dimensions must match
  - use ImageMagick `compare` or a similar pixel metric
  - write artifacts under `/code/gifbrewery-visual-smoke`
  - fail the smoke if the images differ beyond a small GIF palette/dither
    tolerance
- Also screenshot the actual app preview after creating overlays, so the user
  can inspect:
  `/code/gifbrewery-visual-smoke/overlay-preview-*.png`

User-requested feature to implement:

- Add bold font support for text overlays when the selected font family supports
  a bold face.
- Model already has `TextOverlay.font_weight`; UI currently lacks a clear bold
  control.
- Add a bold toggle in the overlay inspector.
- Preview/export must use the same font face selection:
  - for Pango/static measuring, set bold weight
  - for ffmpeg/ImageMagick/frame-renderer path, resolve the bold font file via
    fontconfig or equivalent
- Log the chosen font file/style during export so font mismatches are diagnosable.

Pragmatic warning for the next session:

- Do not spend more time trying to make GTK/Pango overlays visually match
  ffmpeg `drawtext` during live video playback. The user has rejected that
  approach, and the technical evidence agrees with them. The next meaningful
  step is a shared rendered-frame pipeline.

## 2026-06-18 Source-Work Resume: Smoke Compare And Bold

Operational directory:

- The user wants active source work in:
  `/code/gifbrewery-visual-smoke/source-work`
- The parent smoke directory remains the review/artifact directory:
  `/code/gifbrewery-visual-smoke`
- The mounted source-work directory can be edited, but Cargo build output in its
  local `target/` may hit read-only lock errors. Use:

```bash
env CARGO_TARGET_DIR=/tmp/gifbrewery-source-work-target cargo check
env CARGO_TARGET_DIR=/tmp/gifbrewery-source-work-target cargo build
```

Implemented on resume:

- Added smoke command:
  `gifbrewery-gtk --smoke-compare-preview SOURCE OUT_DIR`
- The smoke command builds a cropped project with two timed text overlays,
  renders an exact preview PNG at `0.750s`, exports the GIF, extracts the
  corresponding GIF frame, and compares the preview/export frame with
  ImageMagick `compare -metric RMSE`.
- Added artifacts under:
  `/code/gifbrewery-visual-smoke/preview-compare-smoke-bold`
- Latest comparison result:
  `rmse=0.036357`
- Added a Bold switch to the overlay inspector.
- Bold updates `TextOverlay.font_weight` to `700`; non-bold sets it to `400`.
- Export now passes font weight into font resolution. For weight `>= 600`, it
  asks fontconfig for `family:style=Bold`.
- Export logs the resolved font file, e.g.:
  `/usr/share/fonts/truetype/dejavu/DejaVuSans-Bold.ttf`
- Rebuilt and copied the updated binary to:
  `/code/gifbrewery-visual-smoke/gifbrewery-gtk`

Validation:

```bash
cargo fmt
env CARGO_TARGET_DIR=/tmp/gifbrewery-source-work-target cargo check
env CARGO_TARGET_DIR=/tmp/gifbrewery-source-work-target cargo test -p gifbrewery-gtk export::tests
env CARGO_TARGET_DIR=/tmp/gifbrewery-source-work-target cargo build
/tmp/gifbrewery-source-work-target/debug/gifbrewery-gtk --smoke-compare-preview /code/gifbrewery-visual-smoke/video_pug.mp4 /tmp/gifbrewery-preview-compare-smoke-bold
```

Next high-value task:

- Replace the remaining live-video-authoritative preview path with a shared
  rendered-frame pipeline. The new comparison smoke is the guardrail: preview
  frames and exported frames should stay visually equivalent.

## 2026-06-18 Crop UX Stabilization

User reported after testing:

- Cropping still felt weird because paused editing showed a cropped/zoomed
  output preview, but pressing Play switched back to the uncropped raw video.
- Text overlay selection boxes could be smaller than the text after cropping.
- The user asked to pick one UX: either crop is a zoomed output preview and all
  operations stay zoomed, or crop is only a box over the full source and the
  preview never zooms.

Decision for current code:

- Use the zoomed/output-preview UX for cropped or overlay projects, because it
  is the only current path that can be made exact against export.
- Raw GStreamer playback is now suppressed for projects with crop/output resize
  or overlays. Pressing Play keeps the exact rendered frame instead of zooming
  back out to the raw source. This is a temporary compromise until rendered-frame
  playback is implemented.
- Caption hit/selection bounds now use the cropped output reference height, not
  the original source height. This should make the selection box track the text
  size better after crop.

Files touched:

- `crates/gifbrewery-gtk/src/ui.rs`

Validation:

```bash
cargo fmt
env CARGO_TARGET_DIR=/tmp/gifbrewery-source-work-target cargo check
env CARGO_TARGET_DIR=/tmp/gifbrewery-source-work-target cargo test -p gifbrewery-gtk export::tests
env CARGO_TARGET_DIR=/tmp/gifbrewery-source-work-target cargo build
/tmp/gifbrewery-source-work-target/debug/gifbrewery-gtk --smoke-compare-preview /code/gifbrewery-visual-smoke/video_pug.mp4 /tmp/gifbrewery-preview-compare-crop-ux
```

Artifacts:

- `/code/gifbrewery-visual-smoke/preview-compare-crop-ux`
- `rmse=0.036357`

Still needed:

- Implement real rendered-frame playback/stepping so Play advances through
  rendered output frames instead of freezing on the current exact frame.

## 2026-06-18 Rendered-Frame Playback First Pass

User reiterated that Instagiffer worked because it treated video as still frames
in a temp directory rather than trusting live video playback plus overlays.

Implemented first pass:

- For projects that use crop, output resize, or overlays, Play now uses the
  rendered-output preview path instead of raw GStreamer video playback.
- The playback timer advances the playhead by source frame duration and requests
  exact rendered PNG frames.
- Rendered preview PNGs are cached under `/tmp` by a hash of the project,
  playhead, crop/output settings, and overlays. Repeated playback can reuse
  cached still frames.
- Raw GStreamer playback remains only for simple source preview cases where no
  crop/output resize/overlays are active.

Files touched:

- `crates/gifbrewery-gtk/src/ui.rs`

Validation:

```bash
cargo fmt
env CARGO_TARGET_DIR=/tmp/gifbrewery-source-work-target cargo check
env CARGO_TARGET_DIR=/tmp/gifbrewery-source-work-target cargo test -p gifbrewery-gtk export::tests
env CARGO_TARGET_DIR=/tmp/gifbrewery-source-work-target cargo build
/tmp/gifbrewery-source-work-target/debug/gifbrewery-gtk --smoke-compare-preview /code/gifbrewery-visual-smoke/video_pug.mp4 /tmp/gifbrewery-preview-compare-frame-playback
```

Artifacts:

- `/code/gifbrewery-visual-smoke/preview-compare-frame-playback`
- `rmse=0.036357`

Important caveat:

- This is still on-demand per-frame rendering, not a complete Instagiffer-style
  pre-extracted source-frame workspace. It fixes the UX inconsistency first, but
  it may be slow until frame extraction/render caching is broadened.

## 2026-06-18 Cropped Playback Freeze Follow-Up

User clarified the current broken cropped-playback behavior:

- After cropping, pressing Space moves the playhead, but the preview does not
  animate.
- Pressing Space again stops playback and then the preview jumps to the frame
  where playback stopped.
- Screenshots showed the cropped exact frame centered while strips of the raw
  full video were visible around it.

Root causes found:

- Rendered-frame playback advanced the playhead every timer tick even when an
  exact preview frame was still rendering. Each new playhead invalidated the
  previous render generation, so frames were often discarded until playback
  stopped.
- The exact preview `gtk::Picture` uses contain-style sizing, but caption
  selection bounds were computed against the full overlay widget. After crop,
  this made the selection rectangle smaller/offset relative to the visible text.
- The contained exact preview did not have an opaque backdrop above the raw
  video layer, so the old raw full-source preview could show through in the
  side gutters.

Implemented:

- Added `preview_render_pending` throttling. For rendered-output playback,
  `sync_playback_position` now waits for the current rendered frame to complete
  before advancing another frame.
- Added a black `rendered_backdrop` overlay behind the exact rendered PNG and
  above the raw video layer.
- Added cached preview frame paths under `/tmp/gifbrewery-rendered-preview-$PID`
  keyed by project/playhead/crop/output/overlay state.
- Caption hit/selection bounds now use the contained rendered-frame rectangle
  and rendered preview aspect ratio, so the blue active-overlay box lines up
  with the visible cropped output preview.
- Rebuilt and replaced the mounted test binary:
  `/code/gifbrewery-visual-smoke/gifbrewery-gtk`

Files touched:

- `crates/gifbrewery-gtk/src/ui.rs`

Validation:

```bash
cargo fmt
env CARGO_TARGET_DIR=/tmp/gifbrewery-source-work-target cargo check
env CARGO_TARGET_DIR=/tmp/gifbrewery-source-work-target cargo test -p gifbrewery-gtk export::tests
env CARGO_TARGET_DIR=/tmp/gifbrewery-source-work-target cargo build
/tmp/gifbrewery-source-work-target/debug/gifbrewery-gtk --smoke-compare-preview /code/gifbrewery-visual-smoke/video_pug.mp4 /code/gifbrewery-visual-smoke/preview-compare-render-throttle
```

Artifacts:

- `/code/gifbrewery-visual-smoke/preview-compare-render-throttle`
- `overlay-preview-compare.txt`
- `overlay-preview-exact.png`
- `overlay-preview-export-frame.png`
- `overlay-preview-diff.png`
- `overlay-preview-export.gif`
- Latest compare result remains `rmse=0.036357`

Important caveat:

- This is still an on-demand rendered PNG playback loop. It is more honest than
  raw video playback and fixes the “playhead moves but preview does not update”
  bug caused by render invalidation, but the correct long-term architecture is
  still an Instagiffer-style frame workspace: extract frames, render/cache
  output frames, preview those same frames, then export from those same pixels.

API/runway note:

- `/codex/todo-worker status` is available for the dashboard. At this point in
  the session it reported `used_5h=78.0% available_5h=22.0%`.

## 2026-06-18 Cropped Playback Regression Fix

User reported the previous rendered-frame playback fix was worse in practice:

- Crop controls flashed the white crop grid/bounding box in the wrong place.
- Cropped playback ran at roughly one third of the video frame rate.
- The smoke tests did not cover crop plus playback, so the regression was handed
  to the user instead of caught locally.

Root causes:

- `update_timeline_widgets` restored the crop overlay on every crop/control
  update, then `refresh_exact_preview_frame` hid it again. In rendered-output
  mode this created visible crop-grid flashes.
- Cropped/rendered playback was still rendering one PNG per frame on demand.
  That meant one ffmpeg process per frame and could not keep up with real
  playback.
- Existing smoke coverage checked one paused exact frame versus one exported
  GIF frame. It did not validate frame sequence generation or playback path
  readiness.

Implemented:

- Added `export::render_frame_sequence(project, output_dir)`, which renders the
  full cropped/output/overlay preview sequence with one ffmpeg invocation using
  the same filter chain as export.
- Rendered-output playback now prepares a cached PNG sequence when Play is
  pressed, then plays cached frames by elapsed wall-clock time.
- Rendered playback cache keys include source, clip range/crop, frame strategy,
  output size/settings, and overlay state. If the project changes, playback
  stops instead of showing stale cached frames.
- The crop overlay is now disabled in rendered-output preview mode, so the crop
  grid should not flash during crop-button edits.
- The exact paused-frame renderer now stays out of the way while cached rendered
  playback is running.
- Playback poll interval changed to 16ms so cached playback can service higher
  source frame rates better than the old 33ms tick.
- Added smoke command:
  `gifbrewery-gtk --smoke-crop-playback SOURCE OUT_DIR`

Files touched:

- `crates/gifbrewery-gtk/src/export.rs`
- `crates/gifbrewery-gtk/src/main.rs`
- `crates/gifbrewery-gtk/src/ui.rs`

Validation:

```bash
cargo fmt
env CARGO_TARGET_DIR=/tmp/gifbrewery-source-work-target cargo check
env CARGO_TARGET_DIR=/tmp/gifbrewery-source-work-target cargo test -p gifbrewery-gtk export::tests
env CARGO_TARGET_DIR=/tmp/gifbrewery-source-work-target cargo build
/tmp/gifbrewery-source-work-target/debug/gifbrewery-gtk --smoke-crop-playback /code/gifbrewery-visual-smoke/video_pug.mp4 /code/gifbrewery-visual-smoke/crop-playback-smoke
/tmp/gifbrewery-source-work-target/debug/gifbrewery-gtk --smoke-compare-preview /code/gifbrewery-visual-smoke/video_pug.mp4 /code/gifbrewery-visual-smoke/preview-compare-render-throttle
```

Artifacts:

- `/code/gifbrewery-visual-smoke/crop-playback-smoke`
- `/code/gifbrewery-visual-smoke/preview-compare-render-throttle`

Latest crop playback smoke metrics:

```text
fps=30
duration_seconds=2.000000
expected_frames=60
actual_frames=60
render_seconds=0.219426
generated_fps=273.441
frame_budget_ms=33.333
```

Latest preview/export compare:

```text
rmse=0.036357
```

Rebuilt user-test binary:

- `/code/gifbrewery-visual-smoke/gifbrewery-gtk`

## 2026-06-18 Instagiffer-Style Preview Preload

User reported:

- Spacebar still felt wrong because the app was preparing/loading preview frames
  at playback time, sometimes repeatedly across stop/start.
- The tool should behave like Instagiffer/GIF Brewery 3: do heavy frame work
  when the source is loaded or when the edit model changes, not when the user
  presses Space.
- The gray media-info card in the preview was not useful. Use that area for
  lightweight status messages instead.

Reference checked:

- `/code/gifbrewery-visual-smoke/instagiffer/instagiffer.py`
- `/code/gifbrewery-visual-smoke/instagiffer/igf_animgif.py`
- `/code/gifbrewery-visual-smoke/instagiffer/igf_ui.py`

Relevant Instagiffer behavior:

- CLI `MakeGif` explicitly does `ExtractFrames()`, then crop/resize, then GIF
  generation.
- `ExtractFrames()` uses ffmpeg to write numbered PNG files under the working
  frame directory.
- GUI load sets user-facing loading status and immediately builds enough
  processed frame state for preview interaction.

Implemented:

- Added a rendered playback preload worker:
  - source load/model updates start `render_frame_sequence` in the background
  - the generated PNG frame sequence becomes `rendered_playback_cache`
  - stale workers are discarded by `rendered_playback_generation`
  - if the model changed during a preload, a fresh preload starts from the latest
    project state
- Changed Space/playback behavior:
  - Space no longer launches any render/ffmpeg work
  - if the frame cache is ready, playback starts immediately from cached PNGs
  - if the frame cache is still loading, the preview status says so and returns
  - no more per-Space `render_frame_sequence` jobs
- Cache invalidation now increments playback generation, clears stale cache, and
  lets `update_timeline_widgets` start the next background preload.
- Removed the media-info card behavior:
  - source overlay now uses `.source-status`, transparent/no border
  - top-left preview area shows status like `Loading preview frames...`,
    `Preview ready`, or export/preload status
  - media details are no longer shown as visible preview chrome
- Export status now appears in the preview status overlay, not packed into the
  headerbar top-right.

Validation:

```bash
cargo fmt
env CARGO_TARGET_DIR=/tmp/gifbrewery-source-work-target cargo check
env CARGO_TARGET_DIR=/tmp/gifbrewery-source-work-target cargo test -p gifbrewery-gtk export::tests
env CARGO_TARGET_DIR=/tmp/gifbrewery-source-work-target cargo build
/tmp/gifbrewery-source-work-target/debug/gifbrewery-gtk --smoke-crop-playback /code/gifbrewery-visual-smoke/video_pug.mp4 /tmp/gifbrewery-preload-crop-playback
/tmp/gifbrewery-source-work-target/debug/gifbrewery-gtk --smoke-compare-preview /code/gifbrewery-visual-smoke/video_pug.mp4 /tmp/gifbrewery-preload-preview-compare
APP=/tmp/gifbrewery-source-work-target/debug/gifbrewery-gtk SMOKE_SOURCE=/code/gifbrewery-visual-smoke/video_pug.mp4 OUT_DIR=/tmp/gifbrewery-preload-visual-smoke tools/visual-smoke.sh
env CARGO_TARGET_DIR=/tmp/gifbrewery-source-work-target cargo build --release
```

Latest metrics:

```text
crop playback expected_frames=60 actual_frames=60 render_seconds=0.253641 generated_fps=236.554
preview/export rmse=0.036357
visual smoke means: clip=0.556724 gif=0.55549 overlays=0.555033
```

GUI smoke log showed the preload doing the heavy work at media load:

```text
rendered sequence preload started: reason=timeline/model updated
timeline thumbnails ready: count=12 elapsed=0.000s
rendered sequence preload worker finished in 6.152s
rendered sequence preload ready: ... fps=30 frames=1372
```

Artifacts copied to:

- `/code/gifbrewery-visual-smoke/gifbrewery-preload-crop-playback`
- `/code/gifbrewery-visual-smoke/gifbrewery-preload-preview-compare`
- `/code/gifbrewery-visual-smoke/gifbrewery-preload-visual-smoke`

Rebuilt user-test binary:

- `/code/gifbrewery-visual-smoke/gifbrewery-gtk`
- Release build, dynamically linked, not stripped, 1.6 MB.

## 2026-06-18 Preview Worker Coalescing Fix

User reported a serious regression:

- Dragging the right trim handle caused repeated preview generation.
- The app spawned many ffmpeg processes and pushed CPU to 100%.
- Since frames had already been rendered, trim changes should not repeatedly
  regenerate the same still-image cache.

Root cause:

- `invalidate_render_outputs` set `rendered_playback_preparing = false` even
  while the old worker thread/ffmpeg process was still running.
- The next timeline update therefore believed no preload worker existed and
  started another one. High-frequency trim dragging could produce many ffmpeg
  processes.
- The exact single-frame preview path had the same class of bug:
  invalidation cleared `preview_render_pending` while its worker was still
  running, allowing overlapping `render_frame_png` jobs.

Implemented:

- Added coalescing state for playback preload:
  - `rendered_playback_rebuild_requested`
  - if a preload is running, later invalidations queue one rebuild instead of
    starting another ffmpeg process
  - stale output is discarded when the running worker finishes
  - at most one replacement worker starts after the current worker exits
- Added coalescing state for exact single-frame preview:
  - `preview_render_rebuild_requested`
  - exact preview renders are also one-at-a-time
  - high-frequency updates collapse into one final rerender
- Changed playback cache semantics:
  - playback preload now renders a full source-duration project via
    `playback_preload_project`
  - trim start/end changes no longer invalidate the playback frame cache
  - playback uses the current clip range only to decide what cached frame range
    to play/stop at
- Trim changes now call `invalidate_exact_preview_output` only. They update the
  still preview but do not ask ffmpeg to rebuild the full playback cache.
- Crop/resize/fps/speed/text/style changes still invalidate playback cache
  because those change actual frame pixels.

Validation:

```bash
cargo fmt
env CARGO_TARGET_DIR=/tmp/gifbrewery-source-work-target cargo check
env CARGO_TARGET_DIR=/tmp/gifbrewery-source-work-target cargo test -p gifbrewery-gtk export::tests
env CARGO_TARGET_DIR=/tmp/gifbrewery-source-work-target cargo build
APP=/tmp/gifbrewery-source-work-target/debug/gifbrewery-gtk SMOKE_SOURCE=/code/gifbrewery-visual-smoke/video_pug.mp4 OUT_DIR=/tmp/gifbrewery-worker-coalesce-visual-smoke tools/visual-smoke.sh
env CARGO_TARGET_DIR=/tmp/gifbrewery-source-work-target cargo build --release
pgrep -a ffmpeg || true
```

GUI smoke log showed one preload on media load:

```text
rendered sequence preload started: reason=timeline/model updated ...
timeline thumbnails ready: count=12 elapsed=0.000s
rendered sequence preload worker finished in 6.226s
rendered sequence preload ready: ... fps=30 frames=1372
```

Screenshot means remained non-black:

```text
clip.png mean=0.556724
gif.png mean=0.55549
overlays.png mean=0.555033
```

No ffmpeg processes were left after validation.

Artifact copied to:

- `/code/gifbrewery-visual-smoke/gifbrewery-worker-coalesce-visual-smoke`

Rebuilt user-test binary:

- `/code/gifbrewery-visual-smoke/gifbrewery-gtk`
- Release build, dynamically linked, not stripped, 1.6 MB.

Remaining validation gap:

- The current smoke did not perform a real xdotool drag of the right trim
  handle. The code path for worker fan-out is fixed and load-time behavior was
  verified, but a dedicated trim-drag stress smoke should be added next.

## 2026-06-18 Caption Cache Invalidation Fix

User reported the previous fix was still wrong:

- Touching, adding, moving, or editing captions triggered full preview
  regeneration.
- This violates the requested Instagiffer model: generate media frames once at
  load, then handle captions interactively without regenerating source frames.

Implemented:

- Caption/overlay operations now call `invalidate_overlay_output`, which does
  not invalidate ffmpeg-rendered media frame caches.
- Added `base_preview_project`, which strips overlays before exact preview frame
  rendering.
- `playback_preload_project` now uses that base project, so the playback cache
  is media/base-frame data only.
- Removed overlays from `rendered_playback_cache_key` and `preview_render_key`.
  Caption text/style/timing/position changes no longer change cache keys.
- Captions remain live GTK overlays on top of cached media frames.

Validation:

```bash
cargo fmt
env CARGO_TARGET_DIR=/tmp/gifbrewery-source-work-target cargo check
env CARGO_TARGET_DIR=/tmp/gifbrewery-source-work-target cargo test -p gifbrewery-gtk export::tests
env CARGO_TARGET_DIR=/tmp/gifbrewery-source-work-target cargo build --release
```

Rebuilt user-test binary:

- `/code/gifbrewery-visual-smoke/gifbrewery-gtk`
- Release build, dynamically linked, not stripped, 1.6 MB.

Remaining validation gap:

- Because the API window hit the soft stop threshold, no GUI drag/add-caption
  smoke was run for this final change. Next session should add/run a smoke that
  adds and drags captions while checking the log for zero `rendered sequence
  preload started` events after initial media load.

## 2026-06-18 Caption Text Visibility Fix

User reported:

- After removing captions from ffmpeg-rendered preview caches, adding an overlay
  showed the selection box but not the text.

Root cause:

- In exact-preview mode, `CaptionOverlay` used `caption_pixel_bounds`, which
  only calculated bounds and did not draw text. Previously the text had been
  visible because ffmpeg burned it into the preview PNG; after making captions
  live GTK overlays, that branch had to draw the text itself.

Implemented:

- Added `draw_caption_overlay_in_rect`, which clips/translates to the contained
  exact-preview rectangle and calls the normal caption drawing path.
- Removed the now-unused `caption_pixel_bounds` helper.

Validation:

```bash
cargo fmt
env CARGO_TARGET_DIR=/tmp/gifbrewery-source-work-target cargo check
env CARGO_TARGET_DIR=/tmp/gifbrewery-source-work-target cargo build --release
```

Binary note:

- Could not overwrite `/code/gifbrewery-visual-smoke/gifbrewery-gtk` because it
  was open (`Text file busy`).
- Staged fixed binary at:
  `/code/gifbrewery-visual-smoke/gifbrewery-gtk.captionfix`

## 2026-06-18 Performance Pass And Binary Size Check

User reported:

- On a fresh session, Space sometimes did not start playback.
- The first Space press could feel delayed, probably because preview frames were
  being prepared synchronously or in too large a batch.
- The binary in the smoke directory was about 80 MB, raising concern about what
  was bundled into it.

Implemented:

- Timeline thumbnail extraction now happens off the GTK thread. The worker only
  sends `PathBuf`/timestamp data back; pixbufs are loaded on the GTK thread
  because `gdk_pixbuf::Pixbuf` is not `Send`.
- Rendered preview playback now prepares an 8 second exact-frame window from the
  current playhead instead of rendering the full clip before playback can start.
  Playback state flips immediately so Space/Pause feels responsive while the
  short cache is prepared.
- If playback reaches the end of the prepared 8 second window before the real
  clip end, it advances to the next window and prepares that segment.
- Removed the unused black `rendered_backdrop` drawing overlay from the preview
  stack. It was always hidden by recent exact-preview code and was a risk for
  black-screen regressions.
- Removed one dead thumbnail helper exposed by `cargo check`.

Binary size finding:

- `/code/gifbrewery-visual-smoke/gifbrewery-gtk` is the old debug/dev build:
  `with debug_info, not stripped`, dynamically linked, 78 MB.
- Optimized release build:
  `/tmp/gifbrewery-source-work-target/release/gifbrewery-gtk`, dynamically
  linked, not stripped, 1.6 MB.
- Stripped release copy:
  `/tmp/gifbrewery-gtk-release-stripped`, 1.2 MB.
- Conclusion: the 80 MB size is normal for the current debug artifact and is
  mostly debug metadata/unoptimized dev build output. GTK/GStreamer are dynamic
  runtime dependencies, not bundled into that binary.
- Could not overwrite `/code/gifbrewery-visual-smoke/gifbrewery-gtk` because the
  mounted file was still busy from outside the container. Staged the release
  binary instead as:
  `/code/gifbrewery-visual-smoke/gifbrewery-gtk.release`.
- Follow-up: the user closed the running app and the staged release binary was
  copied over `/code/gifbrewery-visual-smoke/gifbrewery-gtk`. The active shared
  smoke binary is now the 1.6 MB release build.
- A sandboxed smoke run of that copied binary could not create
  `/code/gifbrewery-visual-smoke/gifbrewery.log`, so diagnostics fell back to
  the XDG state path and hit this container's read-only home. The diagnostics
  code still correctly tries the executable directory first; this is a sandbox
  write-permission artifact, not expected when the user runs it from the mounted
  host directory.

Validation:

```bash
cargo fmt
env CARGO_TARGET_DIR=/tmp/gifbrewery-source-work-target cargo check
env CARGO_TARGET_DIR=/tmp/gifbrewery-source-work-target cargo test -p gifbrewery-gtk export::tests
env CARGO_TARGET_DIR=/tmp/gifbrewery-source-work-target cargo build
env CARGO_TARGET_DIR=/tmp/gifbrewery-source-work-target cargo build --release
/tmp/gifbrewery-source-work-target/debug/gifbrewery-gtk --smoke-crop-playback /code/gifbrewery-visual-smoke/video_pug.mp4 /tmp/gifbrewery-crop-playback-perf-pass
/tmp/gifbrewery-source-work-target/debug/gifbrewery-gtk --smoke-compare-preview /code/gifbrewery-visual-smoke/video_pug.mp4 /tmp/gifbrewery-preview-compare-perf-pass
/tmp/gifbrewery-source-work-target/debug/gifbrewery-gtk --smoke-overlay-after-crop /code/gifbrewery-visual-smoke/video_pug.mp4 /tmp/gifbrewery-overlay-after-crop-perf-pass
/tmp/gifbrewery-source-work-target/release/gifbrewery-gtk --smoke-crop-playback /code/gifbrewery-visual-smoke/video_pug.mp4 /tmp/gifbrewery-release-crop-playback-perf-pass
```

Latest debug crop playback smoke:

```text
fps=30
duration_seconds=2.000000
expected_frames=60
actual_frames=60
render_seconds=0.537630
generated_fps=111.601
frame_budget_ms=33.333
```

Latest preview/export compare:

```text
rmse=0.036357
```

Artifacts copied for review:

- `/code/gifbrewery-visual-smoke/gifbrewery-crop-playback-perf-pass`
- `/code/gifbrewery-visual-smoke/gifbrewery-preview-compare-perf-pass`
- `/code/gifbrewery-visual-smoke/gifbrewery-overlay-after-crop-perf-pass`

## 2026-06-18 Frame-Authoritative Preview Rewrite

User pushed back on the hybrid video-player/rendered-frame approach after a new
screenshot showed the preview going black and obscuring both media and text.
The user explicitly requested following Instagiffer/GIF Brewery 3 instead of
playing source video as video. This is correct for a GIF editor.

Reference findings:

- Instagiffer's command workflow is explicitly:
  `ExtractFrames() -> CropAndResize() -> Generate()`.
- Instagiffer keeps persistent working directories:
  - `original`
  - `resized`
  - `processed`
  - `preview.gif`
- Instagiffer extracts frames with ffmpeg into `original/image%04d.png`.
- Instagiffer crops/resizes each extracted frame into `resized`.
- Instagiffer applies captions/effects per frame during `ImageProcessing`;
  captions are frame-index gated with `frameStart` and `frameEnd`.
- Instagiffer then generates the final GIF from processed frame files.
- GIF Brewery 3 binary strings also support a frame/image-authoritative model:
  `readFrames`, `readFramesFromClip`, `framesForCurrentClip`,
  `applyOverlaysToImage:atTime:`, `cropImage:toRect:`,
  `moveOneFrameForward`, `moveOneFrameBackward`, and
  `setSuppressesPlayerRendering:`.

Implemented:

- Removed the visible GStreamer `VideoPreview` from the editor preview stack.
- Deleted the unused GStreamer preview wrapper from `media.rs`; GStreamer remains
  only for metadata discovery.
- The preview area now uses a neutral drawing canvas plus the rendered PNG
  `gtk::Picture`.
- `uses_rendered_output_preview(project)` now returns true for every loaded
  source. In other words, all loaded media preview is frame/render output.
- Play always uses `render_frame_sequence` and cached PNG frame playback.
- Clip start/end/range changes no longer seek a video player; they invalidate
  render output and refresh the frame-backed preview.
- The black rendered backdrop is kept as a widget field but never shown. This
  removes the failure mode where a stale/empty rendered layer blacked out the
  visible media/text.
- `tools/visual-smoke.sh` now accepts `APP=...` so smoke tests can run against
  the actual binary built in `/tmp/gifbrewery-source-work-target`.

Files touched:

- `crates/gifbrewery-gtk/src/ui.rs`
- `crates/gifbrewery-gtk/src/media.rs`
- `tools/visual-smoke.sh`
- `docs/SESSION_HANDOFF.md`

Validation:

```bash
cargo fmt
env CARGO_TARGET_DIR=/tmp/gifbrewery-source-work-target cargo check
env CARGO_TARGET_DIR=/tmp/gifbrewery-source-work-target cargo test -p gifbrewery-gtk export::tests
env CARGO_TARGET_DIR=/tmp/gifbrewery-source-work-target cargo build
/tmp/gifbrewery-source-work-target/debug/gifbrewery-gtk --smoke-overlay-after-crop /code/gifbrewery-visual-smoke/video_pug.mp4 /tmp/gifbrewery-overlay-after-crop-smoke
/tmp/gifbrewery-source-work-target/debug/gifbrewery-gtk --smoke-crop-playback /code/gifbrewery-visual-smoke/video_pug.mp4 /tmp/gifbrewery-crop-playback-smoke
/tmp/gifbrewery-source-work-target/debug/gifbrewery-gtk --smoke-compare-preview /code/gifbrewery-visual-smoke/video_pug.mp4 /tmp/gifbrewery-preview-compare-render-throttle
APP=/tmp/gifbrewery-source-work-target/debug/gifbrewery-gtk SMOKE_SOURCE=/code/gifbrewery-visual-smoke/video_pug.mp4 tools/visual-smoke.sh
```

Latest metrics:

```text
overlay-after-crop rmse=0.139834
crop playback actual_frames=60 expected_frames=60 generated_fps=245.570
preview/export rmse=0.036357
visual smoke clip mean=0.551736 size=1280x820
visual smoke gif mean=0.550502 size=1280x820
visual smoke overlays mean=0.550044 size=1280x820
```

Visual artifacts:

- `/code/gifbrewery-visual-smoke/clip.png`
- `/code/gifbrewery-visual-smoke/gif.png`
- `/code/gifbrewery-visual-smoke/overlays.png`
- `/code/gifbrewery-visual-smoke/overlay-after-crop-smoke`
- `/code/gifbrewery-visual-smoke/crop-playback-smoke`
- `/code/gifbrewery-visual-smoke/preview-compare-render-throttle`

Rebuilt user-test binary:

- `/code/gifbrewery-visual-smoke/gifbrewery-gtk`

Important next step:

- Continue toward a full Instagiffer-style workspace rather than reintroducing
  video playback. The current code renders full preview sequences on Play and
  single exact frames for paused edits. The next better version should maintain
  extracted source frames and rendered output frames as explicit project caches
  and rebuild them in the background after edits.

## 2026-06-18 Overlay-After-Crop Regression Smoke

On resume, added a dedicated regression smoke for the last user-reported issue:
crop first, then add an overlay, and ensure the rendered frame visibly changes.

Implemented:

- Added CLI command:
  `gifbrewery-gtk --smoke-overlay-after-crop SOURCE OUT_DIR`
- The command:
  - builds the same cropped project used by preview/export compare
  - renders one frame at `0.750s` with overlays removed
  - renders the same frame with overlays present
  - compares the two PNGs with ImageMagick RMSE
  - fails if the difference is too small, which would indicate overlay text did
    not appear in the rendered preview frame
- Writes review artifacts under:
  `/code/gifbrewery-visual-smoke/overlay-after-crop-smoke`

Files touched:

- `crates/gifbrewery-gtk/src/main.rs`

Validation:

```bash
cargo fmt
env CARGO_TARGET_DIR=/tmp/gifbrewery-source-work-target cargo check
env CARGO_TARGET_DIR=/tmp/gifbrewery-source-work-target cargo test -p gifbrewery-gtk export::tests
env CARGO_TARGET_DIR=/tmp/gifbrewery-source-work-target cargo build
/tmp/gifbrewery-source-work-target/debug/gifbrewery-gtk --smoke-overlay-after-crop /code/gifbrewery-visual-smoke/video_pug.mp4 /tmp/gifbrewery-overlay-after-crop-smoke
/tmp/gifbrewery-source-work-target/debug/gifbrewery-gtk --smoke-crop-playback /code/gifbrewery-visual-smoke/video_pug.mp4 /tmp/gifbrewery-crop-playback-smoke
/tmp/gifbrewery-source-work-target/debug/gifbrewery-gtk --smoke-compare-preview /code/gifbrewery-visual-smoke/video_pug.mp4 /tmp/gifbrewery-preview-compare-render-throttle
```

Latest metrics:

```text
overlay-after-crop rmse=0.139834
crop playback actual_frames=60 expected_frames=60 generated_fps=286.553
preview/export rmse=0.036357
```

Artifacts copied to:

- `/code/gifbrewery-visual-smoke/overlay-after-crop-smoke`
- `/code/gifbrewery-visual-smoke/crop-playback-smoke`
- `/code/gifbrewery-visual-smoke/preview-compare-render-throttle`

Rebuilt user-test binary:

- `/code/gifbrewery-visual-smoke/gifbrewery-gtk`

Remaining caveat:

- Playback still prepares the rendered frame sequence on Play, not continuously
  in the background as edits happen. The next step toward full Instagiffer-style
  behavior is background invalidation/rebuild of source and rendered frame
  caches after crop/text/output edits, with progress/status surfaced in the UI.

## 2026-06-18 Overlay Visibility And Export Preview Fix

User reported:

- After cropping, adding a new overlay showed the selected text box, but the
  text itself did not become visible until the box was moved.
- The exported GIF looked fine in the desktop `sushi` preview, but the app's
  post-export preview window showed severe line/interlacing artifacts.

Findings:

- The overlay box was the GTK selection overlay, but the visible text in
  rendered-output mode comes from the exact rendered PNG underneath. Overlay and
  crop edits were not consistently invalidating stale exact-preview and
  rendered-playback caches, so a newly added overlay could leave the old PNG on
  screen until a later drag caused another render.
- The post-export preview used `gtk::Video` on the GIF file. The user's
  screenshot showed that widget/GStreamer path rendering the GIF incorrectly,
  while external desktop preview rendered the file correctly.

Implemented:

- Added `invalidate_render_outputs(&mut AppState)` and call it from crop,
  output-size, overlay add/delete, overlay text/style/timing, and overlay drag
  updates.
- Invalidation bumps the preview generation, clears the last exact-preview key,
  clears pending status, and drops rendered playback caches so old frames cannot
  be reused after a model edit.
- Replaced the export preview window's `gtk::Video` with a frame-sequence
  preview:
  - extract exported GIF frames to PNGs with ffmpeg
  - animate those PNGs in a `gtk::Picture`
  - keep the existing path/footer controls

Files touched:

- `crates/gifbrewery-gtk/src/ui.rs`

Validation:

```bash
cargo fmt
env CARGO_TARGET_DIR=/tmp/gifbrewery-source-work-target cargo check
env CARGO_TARGET_DIR=/tmp/gifbrewery-source-work-target cargo test -p gifbrewery-gtk export::tests
env CARGO_TARGET_DIR=/tmp/gifbrewery-source-work-target cargo build
/tmp/gifbrewery-source-work-target/debug/gifbrewery-gtk --smoke-crop-playback /code/gifbrewery-visual-smoke/video_pug.mp4 /code/gifbrewery-visual-smoke/crop-playback-smoke
/tmp/gifbrewery-source-work-target/debug/gifbrewery-gtk --smoke-compare-preview /code/gifbrewery-visual-smoke/video_pug.mp4 /code/gifbrewery-visual-smoke/preview-compare-render-throttle
```

Latest crop playback smoke:

```text
fps=30
duration_seconds=2.000000
expected_frames=60
actual_frames=60
render_seconds=0.232714
generated_fps=257.827
frame_budget_ms=33.333
```

Latest preview/export compare:

```text
rmse=0.036357
```

Rebuilt user-test binary:

- `/code/gifbrewery-visual-smoke/gifbrewery-gtk`
