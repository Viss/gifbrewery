use gdk_pixbuf::Pixbuf;
use gifbrewery_core::TimelineRange;
use gtk::cairo;
use gtk::gdk::prelude::GdkCairoContextExt;
use gtk::prelude::*;
use std::cell::{Cell, RefCell};
use std::rc::Rc;

const RULER_HEIGHT: f64 = 22.0;
const OVERLAY_HEIGHT: f64 = 30.0;
const FILMSTRIP_TOP: f64 = RULER_HEIGHT + OVERLAY_HEIGHT + 8.0;
const FILMSTRIP_HEIGHT: f64 = 72.0;
const HANDLE_WIDTH: f64 = 10.0;
const MIN_DURATION: f64 = 0.01;

#[derive(Clone)]
pub struct TimelineView {
    area: gtk::DrawingArea,
    state: Rc<RefCell<TimelineViewState>>,
    callbacks: Rc<RefCell<TimelineCallbacks>>,
}

#[derive(Debug, Clone)]
pub struct TimelineViewState {
    pub media_duration_seconds: f64,
    pub frame_fps: Option<f64>,
    pub playhead_seconds: f64,
    pub clip_range: TimelineRange,
    pub overlays: Vec<TimelineOverlayRange>,
    pub selected_overlay_id: Option<String>,
    pub thumbnails: Vec<TimelineThumbnail>,
}

#[derive(Debug, Clone)]
pub struct TimelineOverlayRange {
    pub id: String,
    pub label: String,
    pub range: TimelineRange,
}

#[derive(Debug, Clone)]
pub struct TimelineThumbnail {
    pub timestamp_seconds: f64,
    pub pixbuf: Pixbuf,
}

#[derive(Debug, Clone, Copy)]
struct OverlayBarSegment {
    start_seconds: f64,
    end_seconds: f64,
    lane: Option<usize>,
}

#[derive(Default)]
struct TimelineCallbacks {
    on_seek: Option<Box<dyn Fn(f64)>>,
    on_clip_changed: Option<Box<dyn Fn(TimelineRange)>>,
    on_overlay_changed: Option<Box<dyn Fn(String, TimelineRange)>>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum TimelineHit {
    Playhead,
    ClipStart,
    ClipEnd,
    OverlayStart,
    OverlayEnd,
    OverlayBody,
    Empty,
}

#[derive(Debug, Clone, Copy)]
struct TimelineDragOrigin {
    playhead_seconds: f64,
    clip_range: TimelineRange,
    overlay_range: Option<TimelineRange>,
    overlay_index: Option<usize>,
}

impl Default for TimelineDragOrigin {
    fn default() -> Self {
        Self {
            playhead_seconds: 0.0,
            clip_range: TimelineRange {
                start_seconds: 0.0,
                end_seconds: 3.0,
            },
            overlay_range: None,
            overlay_index: None,
        }
    }
}

impl Default for TimelineViewState {
    fn default() -> Self {
        Self {
            media_duration_seconds: 3.0,
            frame_fps: Some(12.0),
            playhead_seconds: 0.0,
            clip_range: TimelineRange {
                start_seconds: 0.0,
                end_seconds: 3.0,
            },
            overlays: Vec::new(),
            selected_overlay_id: None,
            thumbnails: Vec::new(),
        }
    }
}

impl TimelineView {
    pub fn new(initial_state: TimelineViewState) -> Self {
        let area = gtk::DrawingArea::builder()
            .content_width(720)
            .content_height(138)
            .hexpand(true)
            .build();
        area.add_css_class("visual-timeline");
        area.set_cursor_from_name(Some("pointer"));

        let state = Rc::new(RefCell::new(initial_state));
        let callbacks = Rc::new(RefCell::new(TimelineCallbacks::default()));
        let drag_hit = Rc::new(Cell::new(TimelineHit::Empty));
        let drag_origin = Rc::new(Cell::new(TimelineDragOrigin::default()));

        area.set_draw_func({
            let state = Rc::clone(&state);
            move |_, cr, width, height| {
                draw_timeline(cr, width, height, &state.borrow());
            }
        });

        let click = gtk::GestureClick::new();
        click.set_button(0);
        click.connect_pressed({
            let area = area.clone();
            let state = Rc::clone(&state);
            let callbacks = Rc::clone(&callbacks);
            move |_, _, x, y| {
                let width = f64::from(area.width());
                let seek_seconds = {
                    let mut state = state.borrow_mut();
                    let hit = hit_test(x, y, &state, width);
                    crate::diagnostics::log_line(format_args!(
                        "timeline click: pointer=({x:.1},{y:.1}) width={width:.1} hit={hit:?}"
                    ));
                    match hit {
                        TimelineHit::Empty | TimelineHit::Playhead => {
                            let seconds =
                                snap_seconds_to_frame(x_to_seconds(x, &state, width), &state);
                            crate::diagnostics::log_line(format_args!(
                                "timeline seek from click: seconds={seconds:.3}"
                            ));
                            state.playhead_seconds = seconds;
                            Some(seconds)
                        }
                        _ => None,
                    }
                };

                if let Some(seconds) = seek_seconds {
                    area.queue_draw();
                    if let Some(callback) = &callbacks.borrow().on_seek {
                        callback(seconds);
                    }
                }
            }
        });
        area.add_controller(click);

        let drag = gtk::GestureDrag::new();
        drag.connect_drag_begin({
            let area = area.clone();
            let state = Rc::clone(&state);
            let drag_hit = Rc::clone(&drag_hit);
            let drag_origin = Rc::clone(&drag_origin);
            move |_, x, y| {
                let width = f64::from(area.width());
                let state = state.borrow();
                let hit = hit_test(x, y, &state, width);
                crate::diagnostics::log_line(format_args!(
                    "timeline drag begin: pointer=({x:.1},{y:.1}) width={width:.1} hit={hit:?} playhead={:.3} clip={:?} overlay={:?}",
                    state.playhead_seconds,
                    state.clip_range,
                    hit_overlay(&state, x, y, width).map(|(_, overlay)| (&overlay.id, overlay.range))
                ));
                drag_hit.set(hit);
                let overlay_hit = hit_overlay(&state, x, y, width);
                drag_origin.set(TimelineDragOrigin {
                    playhead_seconds: state.playhead_seconds,
                    clip_range: state.clip_range,
                    overlay_range: overlay_hit.as_ref().map(|(_, overlay)| overlay.range),
                    overlay_index: overlay_hit.map(|(index, _)| index),
                });
            }
        });
        drag.connect_drag_update({
            let area = area.clone();
            let state = Rc::clone(&state);
            let callbacks = Rc::clone(&callbacks);
            let drag_hit = Rc::clone(&drag_hit);
            let drag_origin = Rc::clone(&drag_origin);
            move |_, offset_x, _| {
                let width = f64::from(area.width());
                let hit = drag_hit.get();
                if hit == TimelineHit::Empty {
                    return;
                }

                let origin = drag_origin.get();
                let seconds_delta = pixels_to_seconds(offset_x, &state.borrow(), width);
                let mut seek = None;
                let mut clip_changed = None;
                let mut overlay_changed = None;

                {
                    let mut state = state.borrow_mut();
                    crate::diagnostics::log_line(format_args!(
                        "timeline drag update: hit={hit:?} offset_x={offset_x:.1} seconds_delta={seconds_delta:.3}"
                    ));

                    match hit {
                        TimelineHit::Playhead => {
                            state.playhead_seconds = snap_seconds_to_frame(
                                origin.playhead_seconds + seconds_delta,
                                &state,
                            );
                            seek = Some(state.playhead_seconds);
                        }
                        TimelineHit::ClipStart => {
                            let gap = min_frame_gap_seconds(&state);
                            let max_start = (origin.clip_range.end_seconds - gap).max(0.0);
                            state.clip_range.start_seconds =
                                snap_seconds_to_frame_in_range(
                                    origin.clip_range.start_seconds + seconds_delta,
                                    0.0,
                                    max_start,
                                    &state,
                                );
                            state.playhead_seconds = state.clip_range.start_seconds;
                            seek = Some(state.playhead_seconds);
                            clamp_overlay_to_clip(&mut state);
                            clip_changed = Some(state.clip_range);
                        }
                        TimelineHit::ClipEnd => {
                            let gap = min_frame_gap_seconds(&state);
                            let min_end = origin.clip_range.start_seconds + gap;
                            state.clip_range.end_seconds =
                                snap_seconds_to_frame_in_range(
                                    origin.clip_range.end_seconds + seconds_delta,
                                    min_end,
                                    state.media_duration_seconds,
                                    &state,
                                );
                            state.playhead_seconds = state.clip_range.end_seconds;
                            seek = Some(state.playhead_seconds);
                            clamp_overlay_to_clip(&mut state);
                            clip_changed = Some(state.clip_range);
                        }
                        TimelineHit::OverlayStart => {
                            let clip_range = state.clip_range;
                            let gap = min_frame_gap_seconds(&state);
                            let frame_fps = state.frame_fps;
                            if let (Some(origin_range), Some(index)) =
                                (origin.overlay_range, origin.overlay_index)
                            {
                                if let Some(overlay) = state.overlays.get_mut(index)
                                {
                                    overlay.range.start_seconds =
                                        snap_seconds_to_frame_in_range_with_fps(
                                            origin_range.start_seconds + seconds_delta,
                                            clip_range.start_seconds,
                                            overlay.range.end_seconds - gap,
                                            frame_fps,
                                        );
                                    overlay_changed = Some((overlay.id.clone(), overlay.range));
                                }
                            }
                        }
                        TimelineHit::OverlayEnd => {
                            let clip_range = state.clip_range;
                            let gap = min_frame_gap_seconds(&state);
                            let frame_fps = state.frame_fps;
                            if let (Some(origin_range), Some(index)) =
                                (origin.overlay_range, origin.overlay_index)
                            {
                                if let Some(overlay) = state.overlays.get_mut(index)
                                {
                                    overlay.range.end_seconds =
                                        snap_seconds_to_frame_in_range_with_fps(
                                            origin_range.end_seconds + seconds_delta,
                                            overlay.range.start_seconds + gap,
                                            clip_range.end_seconds,
                                            frame_fps,
                                        );
                                    overlay_changed = Some((overlay.id.clone(), overlay.range));
                                }
                            }
                        }
                        TimelineHit::OverlayBody => {
                            let clip_range = state.clip_range;
                            let frame_fps = state.frame_fps;
                            if let (Some(origin_range), Some(index)) =
                                (origin.overlay_range, origin.overlay_index)
                            {
                                if let Some(overlay) = state.overlays.get_mut(index)
                                {
                                    let duration = origin_range.duration_seconds();
                                    let start_seconds = snap_seconds_to_frame_in_range_with_fps(
                                        origin_range.start_seconds + seconds_delta,
                                        clip_range.start_seconds,
                                        clip_range.end_seconds - duration,
                                        frame_fps,
                                    );
                                    overlay.range = TimelineRange {
                                        start_seconds,
                                        end_seconds: start_seconds + duration,
                                    };
                                    overlay_changed = Some((overlay.id.clone(), overlay.range));
                                }
                            }
                        }
                        TimelineHit::Empty => {}
                    }
                }

                area.queue_draw();
                if let Some(seconds) = seek {
                    if let Some(callback) = &callbacks.borrow().on_seek {
                        callback(seconds);
                    }
                }
                if let Some(range) = clip_changed {
                    if let Some(callback) = &callbacks.borrow().on_clip_changed {
                        callback(range);
                    }
                }
                if let Some((id, range)) = overlay_changed {
                    if let Some(callback) = &callbacks.borrow().on_overlay_changed {
                        callback(id, range);
                    }
                }
            }
        });
        area.add_controller(drag);

        let motion = gtk::EventControllerMotion::new();
        motion.connect_motion({
            let area = area.clone();
            let state = Rc::clone(&state);
            move |_, x, y| {
                let width = f64::from(area.width());
                let hit = hit_test(x, y, &state.borrow(), width);
                match hit {
                    TimelineHit::ClipStart
                    | TimelineHit::ClipEnd
                    | TimelineHit::OverlayStart
                    | TimelineHit::OverlayEnd => area.set_cursor_from_name(Some("ew-resize")),
                    TimelineHit::Playhead | TimelineHit::OverlayBody => {
                        area.set_cursor_from_name(Some("pointer"));
                    }
                    TimelineHit::Empty => area.set_cursor_from_name(None),
                }
            }
        });
        motion.connect_leave({
            let area = area.clone();
            move |_| {
                area.set_cursor_from_name(None);
            }
        });
        area.add_controller(motion);

        Self {
            area,
            state,
            callbacks,
        }
    }

    pub fn widget(&self) -> gtk::DrawingArea {
        self.area.clone()
    }

    pub fn set_state(&self, next_state: TimelineViewState) {
        *self.state.borrow_mut() = next_state;
        self.area.queue_draw();
    }

    pub fn connect_seek<F: Fn(f64) + 'static>(&self, callback: F) {
        self.callbacks.borrow_mut().on_seek = Some(Box::new(callback));
    }

    pub fn connect_clip_changed<F: Fn(TimelineRange) + 'static>(&self, callback: F) {
        self.callbacks.borrow_mut().on_clip_changed = Some(Box::new(callback));
    }

    pub fn connect_overlay_changed<F: Fn(String, TimelineRange) + 'static>(&self, callback: F) {
        self.callbacks.borrow_mut().on_overlay_changed = Some(Box::new(callback));
    }
}

fn draw_timeline(cr: &cairo::Context, width: i32, height: i32, state: &TimelineViewState) {
    let width = f64::from(width.max(1));
    let height = f64::from(height.max(1));

    cr.set_source_rgb(0.13, 0.135, 0.145);
    cr.rectangle(0.0, 0.0, width, height);
    let _ = cr.fill();

    draw_ruler(cr, width, state);
    draw_overlay_lane(cr, width, state);
    draw_filmstrip(cr, width, state);
    draw_clip_selection(cr, width, state);
    draw_playhead(cr, width, height, state);
}

fn draw_ruler(cr: &cairo::Context, width: f64, state: &TimelineViewState) {
    cr.set_source_rgb(0.72, 0.74, 0.76);
    cr.select_font_face("Sans", cairo::FontSlant::Normal, cairo::FontWeight::Normal);
    cr.set_font_size(10.0);

    let duration = state.media_duration_seconds.max(MIN_DURATION);
    let tick_count = if duration <= 2.0 { 5 } else { 7 };
    for tick in 0..tick_count {
        let t = duration * f64::from(tick) / f64::from(tick_count - 1);
        let x = seconds_to_x(t, state, width);
        cr.set_source_rgb(0.36, 0.38, 0.4);
        cr.move_to(x + 0.5, 3.0);
        cr.line_to(x + 0.5, RULER_HEIGHT - 4.0);
        let _ = cr.stroke();

        cr.set_source_rgb(0.76, 0.78, 0.8);
        cr.move_to((x + 4.0).min(width - 32.0).max(2.0), 15.0);
        let _ = cr.show_text(&format_time(t));
    }
}

fn draw_overlay_lane(cr: &cairo::Context, width: f64, state: &TimelineViewState) {
    let y = RULER_HEIGHT + 3.0;
    cr.set_source_rgb(0.18, 0.19, 0.205);
    rounded_rect(cr, 0.5, y, width - 1.0, OVERLAY_HEIGHT - 4.0, 5.0);
    let _ = cr.fill();

    for (index, overlay) in state.overlays.iter().enumerate() {
        let (red, green, blue) = overlay_color(index);
        let segments = overlay_bar_segments(index, overlay, &state.overlays);
        for segment in &segments {
            let (bar_y, bar_height) = overlay_segment_geometry(y, segment.lane);
            let x1 = seconds_to_x(segment.start_seconds, state, width);
            let x2 = seconds_to_x(segment.end_seconds, state, width);
            let bar_width = (x2 - x1).max(2.0);
            cr.set_source_rgb(red, green, blue);
            rounded_rect(cr, x1, bar_y, bar_width, bar_height, 3.0);
            let _ = cr.fill();

            if state.selected_overlay_id.as_deref() == Some(overlay.id.as_str()) {
                cr.set_source_rgb(1.0, 1.0, 1.0);
                cr.set_line_width(1.5);
                rounded_rect(
                    cr,
                    x1 + 0.75,
                    bar_y + 0.75,
                    (bar_width - 1.5).max(1.0),
                    (bar_height - 1.5).max(1.0),
                    3.0,
                );
                let _ = cr.stroke();
            }
        }

        let x1 = seconds_to_x(overlay.range.start_seconds, state, width);
        let x2 = seconds_to_x(overlay.range.end_seconds, state, width);
        let bar_width = (x2 - x1).max(12.0);
        if bar_width > 42.0 {
            let label_lane = segments.first().and_then(|segment| segment.lane);
            let (bar_y, bar_height) = overlay_segment_geometry(y, label_lane);
            cr.set_source_rgb(0.96, 0.98, 1.0);
            cr.select_font_face("Sans", cairo::FontSlant::Normal, cairo::FontWeight::Bold);
            cr.set_font_size(9.0);
            cr.move_to(x1 + 6.0, bar_y + bar_height - 3.0);
            let _ = cr.show_text(&overlay.label);
        }

        let (start_y, start_height) =
            overlay_handle_geometry(index, overlay, &state.overlays, true);
        let (end_y, end_height) = overlay_handle_geometry(index, overlay, &state.overlays, false);
        draw_edge_grip(cr, x1, start_y + 2.0, start_height - 4.0);
        draw_edge_grip(cr, x2, end_y + 2.0, end_height - 4.0);
    }
}

fn draw_filmstrip(cr: &cairo::Context, width: f64, state: &TimelineViewState) {
    cr.set_source_rgb(0.075, 0.08, 0.09);
    rounded_rect(cr, 0.5, FILMSTRIP_TOP, width - 1.0, FILMSTRIP_HEIGHT, 5.0);
    let _ = cr.fill();

    if !state.thumbnails.is_empty() {
        draw_thumbnails(cr, width, &state.thumbnails);
        return;
    }

    let cell_width = 78.0;
    let gap = 2.0;
    let cells = (width / cell_width).ceil() as usize + 1;
    for index in 0..cells {
        let x = f64::from(index as u32) * cell_width;
        cr.set_source_rgb(0.16, 0.17, 0.185);
        cr.rectangle(
            x + gap,
            FILMSTRIP_TOP + gap,
            cell_width - gap * 2.0,
            FILMSTRIP_HEIGHT - gap * 2.0,
        );
        let _ = cr.fill();

        cr.set_source_rgb(0.23, 0.24, 0.255);
        cr.move_to(x + 10.0, FILMSTRIP_TOP + 14.0);
        cr.line_to(
            (x + cell_width - 10.0).min(width),
            FILMSTRIP_TOP + FILMSTRIP_HEIGHT - 12.0,
        );
        let _ = cr.stroke();

        cr.set_source_rgb(0.11, 0.115, 0.125);
        cr.rectangle(x + gap, FILMSTRIP_TOP + gap, cell_width - gap * 2.0, 7.0);
        cr.rectangle(
            x + gap,
            FILMSTRIP_TOP + FILMSTRIP_HEIGHT - 9.0,
            cell_width - gap * 2.0,
            7.0,
        );
        let _ = cr.fill();
    }
}

fn draw_thumbnails(cr: &cairo::Context, width: f64, thumbnails: &[TimelineThumbnail]) {
    let cell_width = width / thumbnails.len().max(1) as f64;
    let gap = 2.0;

    for (index, thumbnail) in thumbnails.iter().enumerate() {
        let _timestamp_seconds = thumbnail.timestamp_seconds;
        let x = index as f64 * cell_width;
        cr.set_source_rgb(0.02, 0.022, 0.026);
        cr.rectangle(
            x + gap,
            FILMSTRIP_TOP + gap,
            cell_width - gap * 2.0,
            FILMSTRIP_HEIGHT - gap * 2.0,
        );
        let _ = cr.fill();

        let pixbuf_width = f64::from(thumbnail.pixbuf.width().max(1));
        let pixbuf_height = f64::from(thumbnail.pixbuf.height().max(1));
        let target_width = (cell_width - gap * 2.0).max(1.0);
        let target_height = (FILMSTRIP_HEIGHT - gap * 2.0).max(1.0);
        let scale = (target_width / pixbuf_width)
            .min(target_height / pixbuf_height)
            .max(0.01);
        let draw_width = pixbuf_width * scale;
        let draw_height = pixbuf_height * scale;
        let draw_x = x + (cell_width - draw_width) / 2.0;
        let draw_y = FILMSTRIP_TOP + (FILMSTRIP_HEIGHT - draw_height) / 2.0;

        let _ = cr.save();
        cr.rectangle(
            x + gap,
            FILMSTRIP_TOP + gap,
            cell_width - gap * 2.0,
            FILMSTRIP_HEIGHT - gap * 2.0,
        );
        cr.clip();
        cr.translate(draw_x, draw_y);
        cr.scale(scale, scale);
        cr.set_source_pixbuf(&thumbnail.pixbuf, 0.0, 0.0);
        let _ = cr.paint();
        let _ = cr.restore();

        cr.set_source_rgba(0.0, 0.0, 0.0, 0.34);
        cr.rectangle(x + gap, FILMSTRIP_TOP + gap, cell_width - gap * 2.0, 7.0);
        cr.rectangle(
            x + gap,
            FILMSTRIP_TOP + FILMSTRIP_HEIGHT - 9.0,
            cell_width - gap * 2.0,
            7.0,
        );
        let _ = cr.fill();
    }
}

fn draw_clip_selection(cr: &cairo::Context, width: f64, state: &TimelineViewState) {
    let start_x = seconds_to_x(state.clip_range.start_seconds, state, width);
    let end_x = seconds_to_x(state.clip_range.end_seconds, state, width);

    cr.set_source_rgba(0.0, 0.0, 0.0, 0.42);
    cr.rectangle(0.0, FILMSTRIP_TOP, start_x.max(0.0), FILMSTRIP_HEIGHT);
    cr.rectangle(
        end_x,
        FILMSTRIP_TOP,
        (width - end_x).max(0.0),
        FILMSTRIP_HEIGHT,
    );
    let _ = cr.fill();

    cr.set_source_rgb(0.95, 0.62, 0.18);
    cr.set_line_width(2.0);
    rounded_rect(
        cr,
        start_x + 1.0,
        FILMSTRIP_TOP + 1.0,
        (end_x - start_x - 2.0).max(1.0),
        FILMSTRIP_HEIGHT - 2.0,
        4.0,
    );
    let _ = cr.stroke();

    draw_trim_handle(cr, start_x, true);
    draw_trim_handle(cr, end_x, false);
}

fn draw_trim_handle(cr: &cairo::Context, x: f64, _left: bool) {
    let handle_x = trim_handle_left_x(x);
    cr.set_source_rgb(0.95, 0.62, 0.18);
    rounded_rect(
        cr,
        handle_x,
        FILMSTRIP_TOP,
        HANDLE_WIDTH,
        FILMSTRIP_HEIGHT,
        3.0,
    );
    let _ = cr.fill();

    cr.set_source_rgb(0.18, 0.12, 0.05);
    let mark_x = handle_x + HANDLE_WIDTH / 2.0;
    cr.move_to(mark_x, FILMSTRIP_TOP + 18.0);
    cr.line_to(mark_x, FILMSTRIP_TOP + FILMSTRIP_HEIGHT - 18.0);
    let _ = cr.stroke();
}

fn trim_handle_left_x(frame_boundary_x: f64) -> f64 {
    frame_boundary_x - HANDLE_WIDTH / 2.0
}

fn draw_playhead(cr: &cairo::Context, width: f64, height: f64, state: &TimelineViewState) {
    let x = seconds_to_x(state.playhead_seconds, state, width).clamp(1.0, width - 1.0);
    cr.set_source_rgb(0.98, 0.98, 0.98);
    cr.set_line_width(2.0);
    cr.move_to(x, RULER_HEIGHT - 1.0);
    cr.line_to(x, height - 4.0);
    let _ = cr.stroke();

    cr.move_to(x, RULER_HEIGHT - 1.0);
    cr.line_to(x - 7.0, RULER_HEIGHT + 8.0);
    cr.line_to(x + 7.0, RULER_HEIGHT + 8.0);
    cr.close_path();
    let _ = cr.fill();
}

fn hit_test(x: f64, y: f64, state: &TimelineViewState, width: f64) -> TimelineHit {
    let playhead_x = seconds_to_x(state.playhead_seconds, state, width);
    if (x - playhead_x).abs() <= 7.0 {
        return TimelineHit::Playhead;
    }

    let overlay_y = RULER_HEIGHT + 3.0;
    if (overlay_y..=overlay_y + OVERLAY_HEIGHT).contains(&y) {
        if let Some((index, overlay)) = hit_overlay(state, x, y, width) {
            let start_x = seconds_to_x(overlay.range.start_seconds, state, width);
            let end_x = seconds_to_x(overlay.range.end_seconds, state, width);
            if (x - start_x).abs() <= HANDLE_WIDTH {
                return TimelineHit::OverlayStart;
            }
            if (x - end_x).abs() <= HANDLE_WIDTH {
                return TimelineHit::OverlayEnd;
            }
            if overlay_body_contains(index, overlay, x, y, state, width) {
                return TimelineHit::OverlayBody;
            }
        }
    }

    if (FILMSTRIP_TOP..=FILMSTRIP_TOP + FILMSTRIP_HEIGHT).contains(&y) {
        let start_x = seconds_to_x(state.clip_range.start_seconds, state, width);
        let end_x = seconds_to_x(state.clip_range.end_seconds, state, width);
        if (x - start_x).abs() <= HANDLE_WIDTH {
            return TimelineHit::ClipStart;
        }
        if (x - end_x).abs() <= HANDLE_WIDTH {
            return TimelineHit::ClipEnd;
        }
    }

    TimelineHit::Empty
}

fn clamp_overlay_to_clip(state: &mut TimelineViewState) {
    for overlay in &mut state.overlays {
        overlay.range.start_seconds = clamp_seconds(
            overlay.range.start_seconds,
            state.clip_range.start_seconds,
            state.clip_range.end_seconds - MIN_DURATION,
        );
        overlay.range.end_seconds = clamp_seconds(
            overlay.range.end_seconds,
            overlay.range.start_seconds + MIN_DURATION,
            state.clip_range.end_seconds,
        );
    }
}

fn hit_overlay<'a>(
    state: &'a TimelineViewState,
    x: f64,
    y: f64,
    width: f64,
) -> Option<(usize, &'a TimelineOverlayRange)> {
    state
        .overlays
        .iter()
        .enumerate()
        .rev()
        .find(|(index, overlay)| overlay_body_contains(*index, overlay, x, y, state, width))
}

fn overlay_body_contains(
    index: usize,
    overlay: &TimelineOverlayRange,
    x: f64,
    y: f64,
    state: &TimelineViewState,
    width: f64,
) -> bool {
    let top = RULER_HEIGHT + 3.0;
    overlay_bar_segments(index, overlay, &state.overlays)
        .iter()
        .any(|segment| {
            let (bar_y, bar_height) = overlay_segment_geometry(top, segment.lane);
            let start_x = seconds_to_x(segment.start_seconds, state, width);
            let end_x = seconds_to_x(segment.end_seconds, state, width);
            x >= start_x && x <= end_x.max(start_x + 2.0) && y >= bar_y && y <= bar_y + bar_height
        })
}

fn overlay_bar_segments(
    index: usize,
    overlay: &TimelineOverlayRange,
    overlays: &[TimelineOverlayRange],
) -> Vec<OverlayBarSegment> {
    let mut cuts = vec![overlay.range.start_seconds, overlay.range.end_seconds];
    for (other_index, other) in overlays.iter().enumerate() {
        if other_index == index || !ranges_overlap(overlay.range, other.range) {
            continue;
        }
        cuts.push(
            other
                .range
                .start_seconds
                .clamp(overlay.range.start_seconds, overlay.range.end_seconds),
        );
        cuts.push(
            other
                .range
                .end_seconds
                .clamp(overlay.range.start_seconds, overlay.range.end_seconds),
        );
    }
    cuts.sort_by(f64::total_cmp);
    cuts.dedup_by(|left, right| (*left - *right).abs() < f64::EPSILON);

    cuts.windows(2)
        .filter_map(|window| {
            let start_seconds = window[0];
            let end_seconds = window[1];
            if end_seconds - start_seconds <= f64::EPSILON {
                return None;
            }
            let midpoint = start_seconds + (end_seconds - start_seconds) / 2.0;
            Some(OverlayBarSegment {
                start_seconds,
                end_seconds,
                lane: overlay_lane_at(index, midpoint, overlays),
            })
        })
        .collect()
}

fn overlay_lane_at(index: usize, seconds: f64, overlays: &[TimelineOverlayRange]) -> Option<usize> {
    let mut active: Vec<(usize, f64)> = overlays
        .iter()
        .enumerate()
        .filter(|(_, overlay)| {
            overlay.range.start_seconds <= seconds && seconds < overlay.range.end_seconds
        })
        .map(|(index, overlay)| (index, overlay.range.start_seconds))
        .collect();

    if active.len() <= 1 {
        return None;
    }

    active.sort_by(|left, right| {
        left.1
            .total_cmp(&right.1)
            .then_with(|| left.0.cmp(&right.0))
    });
    active
        .iter()
        .position(|(active_index, _)| *active_index == index)
        .map(|position| position.min(1))
}

fn overlay_segment_geometry(top: f64, lane: Option<usize>) -> (f64, f64) {
    let full_y = top + 4.0;
    let full_height = OVERLAY_HEIGHT - 12.0;
    match lane {
        Some(lane) => {
            let row_height = full_height / 2.0;
            (full_y + lane as f64 * row_height, row_height.max(8.0))
        }
        None => (full_y, full_height.max(8.0)),
    }
}

fn overlay_handle_geometry(
    index: usize,
    overlay: &TimelineOverlayRange,
    overlays: &[TimelineOverlayRange],
    start: bool,
) -> (f64, f64) {
    let boundary = if start {
        overlay.range.start_seconds
    } else {
        overlay.range.end_seconds
    };
    let sample = if start {
        (boundary + MIN_DURATION / 2.0).min(overlay.range.end_seconds)
    } else {
        (boundary - MIN_DURATION / 2.0).max(overlay.range.start_seconds)
    };
    overlay_segment_geometry(RULER_HEIGHT + 3.0, overlay_lane_at(index, sample, overlays))
}

fn ranges_overlap(left: TimelineRange, right: TimelineRange) -> bool {
    left.start_seconds < right.end_seconds && right.start_seconds < left.end_seconds
}

fn overlay_color(index: usize) -> (f64, f64, f64) {
    const COLORS: &[(f64, f64, f64)] = &[
        (0.22, 0.50, 0.83),
        (0.78, 0.36, 0.28),
        (0.28, 0.62, 0.42),
        (0.66, 0.48, 0.82),
        (0.88, 0.62, 0.22),
    ];
    COLORS[index % COLORS.len()]
}

fn seconds_to_x(seconds: f64, state: &TimelineViewState, width: f64) -> f64 {
    let duration = state.media_duration_seconds.max(MIN_DURATION);
    (seconds / duration).clamp(0.0, 1.0) * width
}

fn x_to_seconds(x: f64, state: &TimelineViewState, width: f64) -> f64 {
    let duration = state.media_duration_seconds.max(MIN_DURATION);
    (x / width.max(1.0)).clamp(0.0, 1.0) * duration
}

fn pixels_to_seconds(pixels: f64, state: &TimelineViewState, width: f64) -> f64 {
    let duration = state.media_duration_seconds.max(MIN_DURATION);
    pixels / width.max(1.0) * duration
}

fn clamp_seconds(value: f64, min: f64, max: f64) -> f64 {
    value.clamp(min, max.max(min))
}

fn snap_seconds_to_frame(seconds: f64, state: &TimelineViewState) -> f64 {
    snap_seconds_to_frame_in_range(seconds, 0.0, state.media_duration_seconds, state)
}

fn snap_seconds_to_frame_in_range(
    seconds: f64,
    min: f64,
    max: f64,
    state: &TimelineViewState,
) -> f64 {
    snap_seconds_to_frame_in_range_with_fps(seconds, min, max, state.frame_fps)
}

fn snap_seconds_to_frame_in_range_with_fps(
    seconds: f64,
    min: f64,
    max: f64,
    frame_fps: Option<f64>,
) -> f64 {
    let clamped = clamp_seconds(seconds, min, max);
    let Some(fps) = frame_fps.filter(|fps| *fps > 0.0) else {
        return clamped;
    };
    let snapped = (clamped * fps).round() / fps;
    clamp_seconds(snapped, min, max)
}

fn min_frame_gap_seconds(state: &TimelineViewState) -> f64 {
    state
        .frame_fps
        .filter(|fps| *fps > 0.0)
        .map(|fps| 1.0 / fps)
        .unwrap_or(MIN_DURATION)
        .max(MIN_DURATION)
}

fn format_time(seconds: f64) -> String {
    if seconds >= 60.0 {
        let minutes = (seconds / 60.0).floor();
        let seconds = seconds - minutes * 60.0;
        format!("{minutes:.0}:{seconds:02.0}")
    } else {
        format!("{seconds:.1}s")
    }
}

fn draw_edge_grip(cr: &cairo::Context, x: f64, y: f64, height: f64) {
    cr.set_source_rgba(1.0, 1.0, 1.0, 0.72);
    cr.set_line_width(1.0);
    cr.move_to(x, y);
    cr.line_to(x, y + height);
    let _ = cr.stroke();
}

fn rounded_rect(cr: &cairo::Context, x: f64, y: f64, width: f64, height: f64, radius: f64) {
    let radius = radius.min(width / 2.0).min(height / 2.0);
    cr.new_sub_path();
    cr.arc(
        x + width - radius,
        y + radius,
        radius,
        -std::f64::consts::FRAC_PI_2,
        0.0,
    );
    cr.arc(
        x + width - radius,
        y + height - radius,
        radius,
        0.0,
        std::f64::consts::FRAC_PI_2,
    );
    cr.arc(
        x + radius,
        y + height - radius,
        radius,
        std::f64::consts::FRAC_PI_2,
        std::f64::consts::PI,
    );
    cr.arc(
        x + radius,
        y + radius,
        radius,
        std::f64::consts::PI,
        std::f64::consts::PI * 1.5,
    );
    cr.close_path();
}

#[cfg(test)]
mod tests {
    use super::{snap_seconds_to_frame_in_range_with_fps, trim_handle_left_x, HANDLE_WIDTH};

    fn assert_close(left: f64, right: f64) {
        assert!(
            (left - right).abs() < 0.000_001,
            "expected {left} to equal {right}"
        );
    }

    #[test]
    fn trim_handle_is_centered_on_frame_boundary() {
        let frame_x = 240.0;
        let handle_left = trim_handle_left_x(frame_x);

        assert_close(handle_left + HANDLE_WIDTH / 2.0, frame_x);
    }

    #[test]
    fn frame_snap_respects_source_frame_rate() {
        let snapped = snap_seconds_to_frame_in_range_with_fps(0.044, 0.0, 1.0, Some(24.0));

        assert_close(snapped, 1.0 / 24.0);
    }
}
