# GIF Brewery GTK Build Notes

Native GTK/libadwaita Rust prototype for a Linux GIF Brewery-style editor.

## Ubuntu/Debian Packages

Install build and runtime dependencies:

```bash
sudo apt-get update
sudo apt-get install -y \
  build-essential \
  cargo \
  rustc \
  pkg-config \
  libgtk-4-dev \
  libadwaita-1-dev \
  libgstreamer1.0-dev \
  libgstreamer-plugins-base1.0-dev \
  gstreamer1.0-gtk4 \
  gstreamer1.0-tools \
  gstreamer1.0-plugins-good \
  gstreamer1.0-plugins-bad \
  gstreamer1.0-plugins-ugly \
  gstreamer1.0-libav \
  ffmpeg
```

Optional for headless visual smoke tests:

```bash
sudo apt-get install -y xvfb x11-apps imagemagick dbus-x11 xdotool
```

## Build

From the source tree root:

```bash
cargo fmt
cargo check
cargo build
```

The debug binary is:

```bash
target/debug/gifbrewery-gtk
```

## Run

```bash
cargo run -p gifbrewery-gtk
```

Or run the built binary directly:

```bash
./target/debug/gifbrewery-gtk
```

## Smoke Tests

Exporter smoke tests:

```bash
./target/debug/gifbrewery-gtk --smoke-export "GIF Brewery 3.app/Contents/Resources/kvo.gif" /tmp/gifbrewery-gif-source-smoke.gif
./target/debug/gifbrewery-gtk --smoke-export-multi-overlay "GIF Brewery 3.app/Contents/Resources/kvo.gif" /tmp/gifbrewery-multi-overlay-smoke.gif
```

Visual smoke test:

```bash
tools/visual-smoke.sh
```

That script writes screenshots and a copied binary to:

```bash
/code/gifbrewery-visual-smoke
```

## Runtime Logs

The app writes `gifbrewery.log` next to the binary being run. If you copy the
binary elsewhere, check that directory for the debug log after crashes.
