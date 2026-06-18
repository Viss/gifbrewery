# Janky gifbrewery linux clone in rust! 

I used gpt5.5 to do this build for me. It took me something like 6 days, because 5.5 on medium think only lets me run for ~30-90 minutes a day with the subscription I have. 

I waterboarded it, i hit it in the face with a brick, i made it redo its own work a bunch of times. It seems like the 'best' way to use these things is to give them something to take a copy of, or to use as a reference, than to have them invent stuff from scratch. Even with instagiffer, gif brewery 3 (an osx mach-o binary that it had to reverse), gifcurry and losslesscut as reference data, it still struggled. I have zero doubts that any actual rust dev who looks at this code will get super upset. 

Treat this thing like a super janky, mythbusters-esque PoC thing. I built it purely for myself, so that I had *SOMETHING* i could use to spin off gif quickly, and I felt guilty constantly badgering the actual devs I have access to into trying to port instagiffer from python2 to python3 just to satisfy my insatiable lust for churning out homegrown, cage free, non-gmo, hand spun gifs when i saw an opportunity. 

My old workflow was something like: find a snippet of something fun, hand it to lossless cut to chop it down to gif-size, then import it into gif brewery 3 or instagiffer to snip off frames, add text, then export. I've tried to basically put all that functionality into one tool with this effort. If you wish to contribute, feel free, but I totally wont feel bad if you're like "ew, clanker shitpile. radioactive, stay away". I did it this way out of necessity, not because i thought the clanker would do a good job. 

## Current status

The repository currently contains:

- A Rust workspace.
- `gifbrewery-core`, a typed model for projects, clips, overlays, and GIF export settings.
- `gifbrewery-gtk`, a native GTK 4/libadwaita application shell.

## basic installation 

First install all the various gizmos and doodads:

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
