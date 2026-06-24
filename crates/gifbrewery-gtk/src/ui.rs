use crate::media::MediaMetadata;
use crate::thumbnails;
use crate::timeline::{TimelineOverlayRange, TimelineView, TimelineViewState};
use adw::prelude::*;
use gifbrewery_core::{
    CropRect, FrameStrategy, MediaSource, Overlay, Project, Rect, RgbaColor, TextAlignment,
    TextOverlay, TimelineRange,
};
use gtk::{cairo, gdk, gio, pango};
use std::cell::{Cell, RefCell};
use std::collections::hash_map::DefaultHasher;
use std::fs;
use std::hash::{Hash, Hasher};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::rc::Rc;
use std::sync::mpsc;
use std::thread;
use std::time::Instant;

const MAX_INTERACTIVE_PREVIEW_EDGE: u32 = 640;
const MAX_AUTO_PRELOAD_FRAME_PIXELS: u64 = 30_000_000;
const RENDERED_PLAYBACK_RESCALE_DEBOUNCE_MS: u64 = 2_000;

#[derive(Clone)]
pub struct AppHandle {
    window: adw::ApplicationWindow,
    state: Rc<RefCell<AppState>>,
    widgets: AppWidgets,
}

impl AppHandle {
    pub fn present(&self) {
        self.window.present();
    }

    pub fn open_file(&self, file: &gio::File) {
        apply_source_file(&self.state, &self.widgets, file);
    }
}

struct AppState {
    project: Project,
    selected_overlay_id: Option<String>,
    playhead_seconds: f64,
    is_playing: bool,
    thumbnails: Vec<crate::timeline::TimelineThumbnail>,
    syncing_widgets: bool,
    thumbnail_generation: u64,
    preview_render_generation: u64,
    last_preview_render_key: Option<String>,
    preview_render_pending: bool,
    preview_render_rebuild_requested: bool,
    rendered_playback_cache: Option<RenderedPlaybackCache>,
    rendered_playback_generation: u64,
    rendered_playback_preparing: bool,
    rendered_playback_rebuild_requested: bool,
    rendered_playback_preload_deferred: bool,
    rendered_playback_preload_debounce: u64,
    rendered_playback_tick: Option<Instant>,
}

#[derive(Debug, Clone)]
struct RenderedPlaybackCache {
    key: String,
    frames: Vec<PathBuf>,
    fps: f64,
    frame_duration_seconds: f64,
    clip_start_seconds: f64,
    clip_end_seconds: f64,
}

#[derive(Clone)]
struct AppWidgets {
    editor: EditorWidgets,
    inspector: InspectorWidgets,
}

#[derive(Clone)]
struct EditorWidgets {
    preview: gtk::Overlay,
    restart_button: gtk::Button,
    play_button: gtk::Button,
    pause_button: gtk::Button,
    source_title: gtk::Label,
    source_detail: gtk::Label,
    empty_state: gtk::Box,
    open_media_button: gtk::Button,
    rendered_frame: gtk::Picture,
    export_spinner: gtk::Spinner,
    export_status: gtk::Label,
    time_label: gtk::Label,
    timeline_view: TimelineView,
    crop_overlay: Option<CropOverlay>,
    caption_overlay: Option<CaptionOverlay>,
    export_button: gtk::Button,
}

#[derive(Clone)]
struct CaptionOverlay {
    area: gtk::DrawingArea,
    texts: Rc<RefCell<Vec<TextOverlay>>>,
    selected_id: Rc<RefCell<Option<String>>>,
    active_bounds: Rc<RefCell<Vec<(String, PixelBounds)>>>,
    source_height: Rc<Cell<f64>>,
    exact_preview_aspect: Rc<Cell<f64>>,
    exact_preview_visible: Rc<Cell<bool>>,
}

#[derive(Clone)]
struct CropOverlay {
    area: gtk::DrawingArea,
    crop: Rc<RefCell<Option<CropRect>>>,
    visible: Rc<Cell<bool>>,
}

#[derive(Debug, Clone, Copy)]
struct PixelBounds {
    x: f64,
    y: f64,
    width: f64,
    height: f64,
}

#[derive(Debug, Clone, Copy)]
struct CaptionDragStart {
    model_bounds: Rect,
    pixel_bounds: Option<PixelBounds>,
}

impl PixelBounds {
    fn contains(self, x: f64, y: f64) -> bool {
        x >= self.x && x <= self.x + self.width && y >= self.y && y <= self.y + self.height
    }
}

impl CaptionOverlay {
    fn new(initial_texts: Vec<TextOverlay>, selected_id: Option<String>) -> Self {
        let area = gtk::DrawingArea::builder()
            .halign(gtk::Align::Fill)
            .valign(gtk::Align::Fill)
            .hexpand(true)
            .vexpand(true)
            .build();
        area.set_focusable(true);
        area.set_cursor_from_name(Some("move"));
        area.set_visible(false);

        let texts = Rc::new(RefCell::new(initial_texts));
        let selected_id = Rc::new(RefCell::new(selected_id));
        let active_bounds = Rc::new(RefCell::new(Vec::new()));
        let source_height = Rc::new(Cell::new(540.0));
        let exact_preview_aspect = Rc::new(Cell::new(16.0 / 9.0));
        let exact_preview_visible = Rc::new(Cell::new(false));

        area.set_draw_func({
            let texts = Rc::clone(&texts);
            let selected_id = Rc::clone(&selected_id);
            let active_bounds = Rc::clone(&active_bounds);
            let source_height = Rc::clone(&source_height);
            let exact_preview_aspect = Rc::clone(&exact_preview_aspect);
            let exact_preview_visible = Rc::clone(&exact_preview_visible);
            move |_, cr, width, height| {
                let selected_id = selected_id.borrow().clone();
                let mut selected_bounds = None;
                let mut bounds_by_id = Vec::new();
                for text in texts.borrow().iter() {
                    let bounds = if exact_preview_visible.get() {
                        let rect = contained_rect(
                            f64::from(width),
                            f64::from(height),
                            exact_preview_aspect.get(),
                        );
                        draw_caption_overlay_in_rect(cr, rect, source_height.get(), text)
                    } else {
                        draw_caption_overlay(
                            cr,
                            f64::from(width),
                            f64::from(height),
                            source_height.get(),
                            text,
                        )
                    };
                    bounds_by_id.push((text.id.clone(), bounds));
                    if selected_id.as_deref() == Some(text.id.as_str()) {
                        selected_bounds = Some(bounds);
                    }
                }
                if let Some(bounds) = selected_bounds {
                    draw_selected_caption_bounds(cr, bounds);
                }
                *active_bounds.borrow_mut() = bounds_by_id;
            }
        });

        Self {
            area,
            texts,
            selected_id,
            active_bounds,
            source_height,
            exact_preview_aspect,
            exact_preview_visible,
        }
    }

    fn widget(&self) -> gtk::DrawingArea {
        self.area.clone()
    }

    fn set_texts_for_playhead(
        &self,
        texts: Vec<TextOverlay>,
        selected_id: Option<String>,
        playhead_seconds: f64,
    ) {
        let visible_count = texts
            .iter()
            .filter(|text| overlay_visible_at_playhead(text, playhead_seconds))
            .count();
        let in_range = visible_count > 0;
        if self.area.is_visible() != in_range {
            crate::diagnostics::log_line(format_args!(
                "caption timing visibility: playhead={playhead_seconds:.3} visible={in_range} active_count={visible_count}"
            ));
        }
        *self.texts.borrow_mut() = texts
            .into_iter()
            .filter(|text| overlay_visible_at_playhead(text, playhead_seconds))
            .collect();
        *self.selected_id.borrow_mut() = selected_id;
        self.area.set_visible(in_range);
        self.area.queue_draw();
    }

    fn hit_test(&self, x: f64, y: f64) -> Option<(String, PixelBounds)> {
        self.active_bounds
            .borrow()
            .iter()
            .rev()
            .find(|(_, bounds)| bounds.contains(x, y))
            .cloned()
    }

    fn set_source_height(&self, source_height: Option<u32>) {
        self.source_height
            .set(source_height.map(f64::from).unwrap_or(540.0).max(1.0));
        self.area.queue_draw();
    }

    fn set_exact_preview_aspect(&self, aspect: f64) {
        self.exact_preview_aspect.set(aspect.max(0.01));
        self.area.queue_draw();
    }

    fn set_exact_preview_visible(&self, visible: bool) {
        self.exact_preview_visible.set(visible);
        self.area.queue_draw();
    }
}

impl CropOverlay {
    fn new(initial_crop: Option<CropRect>, visible: bool) -> Self {
        let area = gtk::DrawingArea::builder()
            .halign(gtk::Align::Fill)
            .valign(gtk::Align::Fill)
            .hexpand(true)
            .vexpand(true)
            .build();
        area.set_can_target(false);
        area.set_visible(visible);

        let crop = Rc::new(RefCell::new(initial_crop));
        let visible = Rc::new(Cell::new(visible));
        area.set_draw_func({
            let crop = Rc::clone(&crop);
            let visible = Rc::clone(&visible);
            move |_, cr, width, height| {
                if !visible.get() {
                    return;
                }
                draw_crop_overlay(cr, f64::from(width), f64::from(height), *crop.borrow());
            }
        });

        Self {
            area,
            crop,
            visible,
        }
    }

    fn widget(&self) -> gtk::DrawingArea {
        self.area.clone()
    }

    fn set_crop(&self, crop: Option<CropRect>, visible: bool) {
        *self.crop.borrow_mut() = crop;
        self.visible.set(visible);
        self.area.set_visible(visible);
        self.area.queue_draw();
    }
}

#[derive(Clone)]
struct InspectorWidgets {
    clip_start: adw::SpinRow,
    clip_end: adw::SpinRow,
    clip_speed: adw::SpinRow,
    clip_fps: adw::SpinRow,
    target_size_mb: adw::SpinRow,
    optimize_gif: adw::SwitchRow,
    high_quality_quantization: adw::SwitchRow,
    output_width: adw::SpinRow,
    output_height: adw::SpinRow,
    crop_left: adw::SpinRow,
    crop_right: adw::SpinRow,
    crop_top: adw::SpinRow,
    crop_bottom: adw::SpinRow,
    overlay_text: Option<gtk::TextView>,
    overlay_font_row: Option<adw::ActionRow>,
    overlay_font: Option<gtk::Button>,
    overlay_font_refresh: Option<gtk::Button>,
    overlay_text_color: Option<gtk::ColorDialogButton>,
    overlay_stroke_color: Option<gtk::ColorDialogButton>,
    overlay_list: Option<gtk::ListBox>,
    overlay_add: Option<gtk::Button>,
    overlay_delete: Option<gtk::Button>,
    overlay_start: Option<adw::SpinRow>,
    overlay_end: Option<adw::SpinRow>,
    overlay_mark_start: Option<gtk::Button>,
    overlay_mark_end: Option<gtk::Button>,
    overlay_font_size: Option<adw::SpinRow>,
    overlay_bold: Option<gtk::ToggleButton>,
    overlay_alignment: Option<gtk::ToggleButton>,
    overlay_stroke_width: Option<adw::SpinRow>,
    overlay_shadow: Option<adw::SwitchRow>,
}

pub fn build_main_window(app: &adw::Application) -> AppHandle {
    cleanup_stale_preview_cache_dirs();

    let state = Rc::new(RefCell::new(AppState {
        project: Project::default(),
        selected_overlay_id: None,
        playhead_seconds: 0.0,
        is_playing: false,
        thumbnails: Vec::new(),
        syncing_widgets: false,
        thumbnail_generation: 0,
        preview_render_generation: 0,
        last_preview_render_key: None,
        preview_render_pending: false,
        preview_render_rebuild_requested: false,
        rendered_playback_cache: None,
        rendered_playback_generation: 0,
        rendered_playback_preparing: false,
        rendered_playback_rebuild_requested: false,
        rendered_playback_preload_deferred: false,
        rendered_playback_preload_debounce: 0,
        rendered_playback_tick: None,
    }));

    let project = state.borrow().project.clone();
    let window = adw::ApplicationWindow::builder()
        .application(app)
        .title("GIF Brewery")
        .default_width(1280)
        .default_height(820)
        .build();

    let toolbar = adw::ToolbarView::new();
    let header = adw::HeaderBar::new();

    let open_button = gtk::Button::builder()
        .label("Open Media")
        .tooltip_text("Open a video or GIF to edit")
        .build();

    let export_button = gtk::Button::builder()
        .label("Create GIF")
        .tooltip_text("Create GIF from the current edit")
        .build();
    export_button.add_css_class("suggested-action");
    export_button.set_sensitive(false);
    let export_spinner = gtk::Spinner::builder()
        .tooltip_text("Export in progress")
        .visible(false)
        .build();
    let export_status = gtk::Label::builder().label("").visible(false).build();
    export_status.add_css_class("dim-label");

    header.pack_start(&open_button);
    header.pack_end(&export_spinner);
    header.pack_end(&export_button);
    toolbar.add_top_bar(&header);

    let root = gtk::Paned::builder()
        .orientation(gtk::Orientation::Horizontal)
        .wide_handle(false)
        .build();

    let (editor, editor_widgets) =
        build_editor(&project, &export_button, &export_spinner, &export_status);
    let (inspector, inspector_widgets) = build_inspector(&project);
    let widgets = AppWidgets {
        editor: editor_widgets,
        inspector: inspector_widgets,
    };
    install_widget_bindings(&state, &widgets, &window);
    install_preview_overlay_drag(&state, &widgets);
    install_playback_poll(&state, &widgets);
    install_keyboard_shortcuts(&window, &state, &widgets);
    root.set_start_child(Some(&editor));
    root.set_end_child(Some(&inspector));
    root.set_resize_start_child(true);
    root.set_shrink_start_child(false);
    root.set_resize_end_child(false);
    root.set_shrink_end_child(false);
    root.set_position(900);

    toolbar.set_content(Some(&root));
    window.set_content(Some(&toolbar));

    install_actions(app, &window);

    open_button.connect_clicked({
        let state = Rc::clone(&state);
        let widgets = widgets.clone();
        let window = window.clone();
        move |_| {
            open_media_dialog(&state, &widgets, &window);
        }
    });

    widgets.editor.open_media_button.connect_clicked({
        let state = Rc::clone(&state);
        let widgets = widgets.clone();
        let window = window.clone();
        move |_| {
            open_media_dialog(&state, &widgets, &window);
        }
    });

    export_button.connect_clicked({
        let state = Rc::clone(&state);
        let widgets = widgets.clone();
        let window = window.clone();
        move |_| {
            export_current_gif(&state, &widgets, &window);
        }
    });

    window.present();
    show_runtime_issues_if_needed(&window);

    AppHandle {
        window,
        state,
        widgets,
    }
}

fn build_editor(
    project: &Project,
    export_button: &gtk::Button,
    export_spinner: &gtk::Spinner,
    export_status: &gtk::Label,
) -> (gtk::Box, EditorWidgets) {
    let editor = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .spacing(0)
        .build();
    editor.add_css_class("view");

    let preview = gtk::Overlay::new();
    preview.set_focusable(true);
    preview.set_hexpand(true);
    preview.set_vexpand(true);

    let video_area = gtk::DrawingArea::builder()
        .content_width(800)
        .content_height(450)
        .hexpand(true)
        .vexpand(true)
        .build();
    video_area.add_css_class("video-canvas");
    video_area.set_draw_func(|_, cr, width, height| {
        cr.set_source_rgb(0.06, 0.065, 0.07);
        let _ = cr.paint();

        cr.set_source_rgb(0.18, 0.19, 0.2);
        cr.rectangle(0.5, 0.5, f64::from(width - 1), f64::from(height - 1));
        let _ = cr.stroke();
    });
    preview.set_child(Some(&video_area));

    let rendered_frame = gtk::Picture::builder()
        .halign(gtk::Align::Fill)
        .valign(gtk::Align::Fill)
        .hexpand(true)
        .vexpand(true)
        .can_target(false)
        .visible(false)
        .build();
    rendered_frame.set_content_fit(gtk::ContentFit::Contain);
    preview.add_overlay(&rendered_frame);

    let source_badge = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .spacing(2)
        .halign(gtk::Align::Start)
        .valign(gtk::Align::Start)
        .margin_top(16)
        .margin_start(16)
        .build();
    source_badge.add_css_class("source-status");

    let source_title = gtk::Label::builder()
        .label("Open media to begin")
        .halign(gtk::Align::Start)
        .xalign(0.0)
        .build();
    source_title.add_css_class("source-title");

    let source_detail = gtk::Label::builder()
        .label("Waiting for source")
        .halign(gtk::Align::Start)
        .xalign(0.0)
        .ellipsize(gtk::pango::EllipsizeMode::Middle)
        .max_width_chars(80)
        .build();
    source_detail.add_css_class("dim-label");

    source_badge.append(&source_title);
    source_badge.append(export_status);
    preview.add_overlay(&source_badge);

    let empty_state = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .spacing(12)
        .halign(gtk::Align::Center)
        .valign(gtk::Align::Center)
        .build();
    empty_state.add_css_class("media-empty-state");
    let open_media_button = gtk::Button::builder()
        .label("Open Media...")
        .icon_name("document-open-symbolic")
        .tooltip_text("Open a video or GIF")
        .build();
    open_media_button.add_css_class("suggested-action");
    open_media_button.add_css_class("pill");
    empty_state.append(&open_media_button);
    let text_overlays = text_overlays_from_project(project);
    let selected_overlay_id = text_overlays.first().map(|text| text.id.clone());
    let crop_overlay = CropOverlay::new(None, project.source.is_some());
    preview.add_overlay(&crop_overlay.widget());
    let caption = CaptionOverlay::new(text_overlays, selected_overlay_id);
    preview.add_overlay(&caption.widget());
    preview.add_overlay(&empty_state);
    empty_state.set_visible(project.source.is_none());
    let crop_overlay = Some(crop_overlay);
    let caption_overlay = Some(caption);

    let (timeline, timeline_controls, time_label, timeline_view) = build_timeline(project);
    editor.append(&preview);
    editor.append(&timeline);

    (
        editor,
        EditorWidgets {
            source_title,
            source_detail,
            empty_state,
            open_media_button,
            rendered_frame,
            export_spinner: export_spinner.clone(),
            export_status: export_status.clone(),
            preview,
            restart_button: timeline_controls.restart,
            play_button: timeline_controls.play,
            pause_button: timeline_controls.pause,
            time_label,
            timeline_view,
            crop_overlay,
            caption_overlay,
            export_button: export_button.clone(),
        },
    )
}

struct TimelineControls {
    restart: gtk::Button,
    play: gtk::Button,
    pause: gtk::Button,
}

fn build_timeline(project: &Project) -> (gtk::Box, TimelineControls, gtk::Label, TimelineView) {
    let timeline = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .spacing(8)
        .margin_top(8)
        .margin_bottom(8)
        .margin_start(12)
        .margin_end(12)
        .build();
    timeline.add_css_class("timeline");

    let controls = gtk::Box::builder()
        .orientation(gtk::Orientation::Horizontal)
        .spacing(6)
        .build();

    let restart_button = gtk::Button::builder()
        .icon_name("media-skip-backward-symbolic")
        .tooltip_text("Restart preview")
        .focusable(false)
        .build();
    restart_button.add_css_class("flat");
    controls.append(&restart_button);

    let play_button = gtk::Button::builder()
        .icon_name("media-playback-start-symbolic")
        .tooltip_text("Play preview")
        .focusable(false)
        .build();
    play_button.add_css_class("flat");
    controls.append(&play_button);

    let pause_button = gtk::Button::builder()
        .icon_name("media-playback-pause-symbolic")
        .tooltip_text("Pause preview")
        .focusable(false)
        .build();
    pause_button.add_css_class("flat");
    controls.append(&pause_button);

    let clip = project.clips.first().expect("default project has a clip");
    let time_label = gtk::Label::new(Some(&format!(
        "{:.2}s - {:.2}s",
        clip.range.start_seconds, clip.range.end_seconds
    )));
    time_label.set_hexpand(true);
    time_label.set_halign(gtk::Align::End);
    controls.append(&time_label);

    let selected_overlay_id = project.overlays.first().map(|overlay| match overlay {
        Overlay::Text(text) => text.id.as_str(),
    });
    let timeline_view = TimelineView::new(timeline_state_from_project(
        project,
        selected_overlay_id,
        0.0,
        Vec::new(),
    ));

    timeline.append(&controls);
    timeline.append(&timeline_view.widget());
    (
        timeline,
        TimelineControls {
            restart: restart_button,
            play: play_button,
            pause: pause_button,
        },
        time_label,
        timeline_view,
    )
}

fn build_inspector(project: &Project) -> (gtk::Box, InspectorWidgets) {
    let stack = adw::ViewStack::new();
    stack.set_vexpand(true);

    let (clip_page, clip_widgets) = build_clip_page(project);
    stack.add_titled_with_icon(&clip_page, Some("clip"), "Clip", "video-x-generic-symbolic");
    let (gif_page, gif_widgets) = build_gif_page(project);
    stack.add_titled_with_icon(&gif_page, Some("gif"), "GIF", "image-x-generic-symbolic");
    let (overlays_page, overlay_widgets) = build_overlays_page(project);
    stack.add_titled_with_icon(
        &overlays_page,
        Some("overlays"),
        "Overlays",
        "insert-text-symbolic",
    );

    let switcher = adw::ViewSwitcher::builder()
        .stack(&stack)
        .policy(adw::ViewSwitcherPolicy::Wide)
        .build();

    let box_ = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .spacing(0)
        .width_request(340)
        .build();
    box_.append(&switcher);
    box_.append(&stack);
    (
        box_,
        InspectorWidgets {
            clip_start: clip_widgets.start,
            clip_end: clip_widgets.end,
            clip_speed: clip_widgets.speed,
            clip_fps: clip_widgets.fps,
            target_size_mb: gif_widgets.target_size_mb,
            optimize_gif: gif_widgets.optimize,
            high_quality_quantization: gif_widgets.high_quality_quantization,
            output_width: gif_widgets.output_width,
            output_height: gif_widgets.output_height,
            crop_left: gif_widgets.crop_left,
            crop_right: gif_widgets.crop_right,
            crop_top: gif_widgets.crop_top,
            crop_bottom: gif_widgets.crop_bottom,
            overlay_text: overlay_widgets.text,
            overlay_font_row: overlay_widgets.font_row,
            overlay_font: overlay_widgets.font,
            overlay_font_refresh: overlay_widgets.font_refresh,
            overlay_text_color: overlay_widgets.text_color,
            overlay_stroke_color: overlay_widgets.stroke_color,
            overlay_list: overlay_widgets.list,
            overlay_add: overlay_widgets.add,
            overlay_delete: overlay_widgets.delete,
            overlay_start: overlay_widgets.start,
            overlay_end: overlay_widgets.end,
            overlay_mark_start: overlay_widgets.mark_start,
            overlay_mark_end: overlay_widgets.mark_end,
            overlay_font_size: overlay_widgets.font_size,
            overlay_bold: overlay_widgets.bold,
            overlay_alignment: overlay_widgets.alignment,
            overlay_stroke_width: overlay_widgets.stroke_width,
            overlay_shadow: overlay_widgets.shadow,
        },
    )
}

struct ClipInspectorWidgets {
    start: adw::SpinRow,
    end: adw::SpinRow,
    speed: adw::SpinRow,
    fps: adw::SpinRow,
}

fn build_clip_page(project: &Project) -> (gtk::ScrolledWindow, ClipInspectorWidgets) {
    let page = adw::PreferencesPage::new();
    let group = adw::PreferencesGroup::builder().title("Clip").build();
    let clip = project.clips.first().expect("default project has a clip");
    let max_frame = max_media_frame_index(project) as f64;

    let start = spin_row(
        "Start frame",
        frame_index_for_seconds(project, clip.range.start_seconds) as f64,
        0.0,
        max_frame,
        1.0,
    );
    group.add(&start);
    let end = spin_row(
        "End frame",
        frame_index_for_seconds(project, clip.range.end_seconds) as f64,
        1.0,
        max_frame.max(1.0),
        1.0,
    );
    group.add(&end);
    let speed = spin_row("Speed", clip.speed, 0.05, 8.0, 0.05);
    group.add(&speed);
    let fps = spin_row(
        "Source frame rate",
        project
            .source
            .as_ref()
            .and_then(|source| source.fps)
            .unwrap_or(0.0),
        0.0,
        240.0,
        1.0,
    );
    fps.set_sensitive(false);
    group.add(&fps);
    page.add(&group);

    (
        scrolled_page(page),
        ClipInspectorWidgets {
            start,
            end,
            speed,
            fps,
        },
    )
}

struct GifInspectorWidgets {
    target_size_mb: adw::SpinRow,
    optimize: adw::SwitchRow,
    high_quality_quantization: adw::SwitchRow,
    output_width: adw::SpinRow,
    output_height: adw::SpinRow,
    crop_left: adw::SpinRow,
    crop_right: adw::SpinRow,
    crop_top: adw::SpinRow,
    crop_bottom: adw::SpinRow,
}

fn build_gif_page(project: &Project) -> (gtk::ScrolledWindow, GifInspectorWidgets) {
    let page = adw::PreferencesPage::new();
    let group = adw::PreferencesGroup::builder().title("Export").build();
    let settings = &project.settings.gif;
    let clip_crop = project
        .clips
        .first()
        .and_then(|clip| clip.crop)
        .unwrap_or(CropRect {
            left: 0.0,
            right: 0.0,
            top: 0.0,
            bottom: 0.0,
        });

    let target_size_mb = spin_row(
        "Mastodon target size",
        settings
            .target_max_bytes
            .map(bytes_to_megabytes)
            .unwrap_or(16.0),
        1.0,
        99.0,
        1.0,
    );
    let optimize = switch_row("Optimize GIF", settings.optimize);
    let high_quality_quantization = switch_row(
        "Maximum quality palettes",
        settings.high_quality_quantization,
    );
    group.add(&high_quality_quantization);
    page.add(&group);

    let size_group = adw::PreferencesGroup::builder().title("Resize").build();
    let output_width = spin_row(
        "Width",
        effective_output_dimensions(project)
            .map(|(width, _)| f64::from(width))
            .unwrap_or_else(|| settings.output_width.map(f64::from).unwrap_or(0.0)),
        0.0,
        4096.0,
        2.0,
    );
    size_group.add(&output_width);
    let output_height = spin_row(
        "Height",
        effective_output_dimensions(project)
            .map(|(_, height)| f64::from(height))
            .unwrap_or_else(|| settings.output_height.map(f64::from).unwrap_or(0.0)),
        0.0,
        4096.0,
        2.0,
    );
    size_group.add(&output_height);
    page.add(&size_group);

    let crop_group = adw::PreferencesGroup::builder().title("Crop").build();
    let crop_left = spin_row("Left %", clip_crop.left * 100.0, 0.0, 95.0, 1.0);
    crop_group.add(&crop_left);
    let crop_right = spin_row("Right %", clip_crop.right * 100.0, 0.0, 95.0, 1.0);
    crop_group.add(&crop_right);
    let crop_top = spin_row("Top %", clip_crop.top * 100.0, 0.0, 95.0, 1.0);
    crop_group.add(&crop_top);
    let crop_bottom = spin_row("Bottom %", clip_crop.bottom * 100.0, 0.0, 95.0, 1.0);
    crop_group.add(&crop_bottom);
    page.add(&crop_group);

    (
        scrolled_page(page),
        GifInspectorWidgets {
            target_size_mb,
            optimize,
            high_quality_quantization,
            output_width,
            output_height,
            crop_left,
            crop_right,
            crop_top,
            crop_bottom,
        },
    )
}

struct OverlayInspectorWidgets {
    text: Option<gtk::TextView>,
    font_row: Option<adw::ActionRow>,
    font: Option<gtk::Button>,
    font_refresh: Option<gtk::Button>,
    text_color: Option<gtk::ColorDialogButton>,
    stroke_color: Option<gtk::ColorDialogButton>,
    list: Option<gtk::ListBox>,
    add: Option<gtk::Button>,
    delete: Option<gtk::Button>,
    start: Option<adw::SpinRow>,
    end: Option<adw::SpinRow>,
    mark_start: Option<gtk::Button>,
    mark_end: Option<gtk::Button>,
    font_size: Option<adw::SpinRow>,
    bold: Option<gtk::ToggleButton>,
    alignment: Option<gtk::ToggleButton>,
    stroke_width: Option<adw::SpinRow>,
    shadow: Option<adw::SwitchRow>,
}

fn build_overlays_page(project: &Project) -> (gtk::ScrolledWindow, OverlayInspectorWidgets) {
    let page = adw::PreferencesPage::new();
    let overlays_group = adw::PreferencesGroup::builder().title("Overlays").build();
    let options_group = adw::PreferencesGroup::builder()
        .title("Selected Overlay Options")
        .build();

    let placeholder = project
        .overlays
        .first()
        .map(|overlay| match overlay {
            Overlay::Text(text) => text.clone(),
        })
        .unwrap_or_else(TextOverlay::default_caption);
    let has_overlay = !project.overlays.is_empty();
    let has_source = project.source.is_some();

    let overlay_list = gtk::ListBox::new();
    overlay_list.set_selection_mode(gtk::SelectionMode::Single);
    overlay_list.add_css_class("boxed-list");
    populate_overlay_list(&overlay_list, &overlay_labels(project), 0);
    overlays_group.add(&overlay_list);

    let add_button = gtk::Button::builder()
        .label("Add")
        .tooltip_text("Add a text overlay")
        .sensitive(has_source)
        .build();
    overlays_group.add(&action_row_with_suffix("Text overlay", &add_button));

    let delete_button = gtk::Button::builder()
        .label("Delete")
        .tooltip_text("Delete the selected text overlay")
        .sensitive(has_overlay)
        .build();
    overlays_group.add(&action_row_with_suffix("Selected overlay", &delete_button));

    let font_size_row = spin_row("Font size", placeholder.font_size, 6.0, 240.0, 1.0);
    font_size_row.set_sensitive(has_overlay);
    options_group.add(&font_size_row);

    let stroke_width_row = spin_row("Stroke width", placeholder.stroke_width, 0.0, 20.0, 0.5);
    stroke_width_row.set_sensitive(has_overlay);
    options_group.add(&stroke_width_row);

    let text_color_button = color_button("Text color", placeholder.text_color);
    text_color_button.set_tooltip_text(Some("Text color"));
    text_color_button.set_sensitive(has_overlay);
    let stroke_color_button = color_button("Stroke color", placeholder.stroke_color);
    stroke_color_button.set_tooltip_text(Some("Stroke color"));
    stroke_color_button.set_sensitive(has_overlay);
    let color_box = gtk::Box::builder()
        .orientation(gtk::Orientation::Horizontal)
        .spacing(8)
        .build();
    color_box.append(&labeled_compact_widget("Text", &text_color_button));
    color_box.append(&labeled_compact_widget("Stroke", &stroke_color_button));
    options_group.add(&action_row_with_suffix("Colors", &color_box));

    let bold_button = gtk::ToggleButton::builder()
        .label("B")
        .tooltip_text("Toggle bold text")
        .active(placeholder.font_weight >= 600)
        .sensitive(has_overlay)
        .build();
    bold_button.add_css_class("flat");
    let alignment_button = gtk::ToggleButton::builder()
        .label("Center")
        .tooltip_text("Toggle centered text")
        .active(placeholder.alignment == TextAlignment::Center)
        .sensitive(has_overlay)
        .build();
    alignment_button.add_css_class("flat");
    let style_box = gtk::Box::builder()
        .orientation(gtk::Orientation::Horizontal)
        .spacing(8)
        .build();
    style_box.append(&bold_button);
    style_box.append(&alignment_button);
    options_group.add(&action_row_with_suffix("Style", &style_box));

    let text_box = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .spacing(4)
        .margin_top(6)
        .margin_bottom(6)
        .margin_start(12)
        .margin_end(12)
        .build();
    let text_label = gtk::Label::builder()
        .label("Text")
        .xalign(0.0)
        .halign(gtk::Align::Start)
        .build();
    text_label.add_css_class("caption");
    let text_view = gtk::TextView::builder()
        .wrap_mode(gtk::WrapMode::WordChar)
        .top_margin(6)
        .bottom_margin(6)
        .left_margin(8)
        .right_margin(8)
        .height_request(56)
        .sensitive(has_overlay)
        .build();
    text_view.buffer().set_text(&placeholder.text);
    let text_scroller = gtk::ScrolledWindow::builder()
        .min_content_height(56)
        .max_content_height(86)
        .child(&text_view)
        .build();
    text_scroller.add_css_class("card");
    text_box.append(&text_label);
    text_box.append(&text_scroller);
    let text_preferences_row = adw::PreferencesRow::builder()
        .child(&text_box)
        .selectable(false)
        .build();
    options_group.add(&text_preferences_row);

    let font_row = adw::ActionRow::builder()
        .title("Font family")
        .subtitle(&placeholder.font_family)
        .build();
    let font_button = gtk::Button::builder()
        .label("Choose")
        .tooltip_text("Choose an installed font family")
        .sensitive(has_overlay)
        .build();
    font_row.add_suffix(&font_button);
    let font_refresh_button = gtk::Button::builder()
        .label("Refresh")
        .tooltip_text("Refresh the system font list")
        .sensitive(has_overlay)
        .build();
    font_row.add_suffix(&font_refresh_button);
    font_row.set_activatable_widget(Some(&font_button));
    font_row.set_sensitive(has_overlay);
    options_group.add(&font_row);

    let font_count = available_font_family_count();
    crate::diagnostics::log_line(format_args!(
        "font picker initialized: families={font_count} selected={}",
        placeholder.font_family
    ));
    let mark_appears_button = gtk::Button::builder()
        .label("Set start")
        .tooltip_text("Set appear time to the current playhead")
        .sensitive(has_overlay)
        .build();
    let mark_disappears_button = gtk::Button::builder()
        .label("Set end")
        .tooltip_text("Set disappear time to the current playhead")
        .sensitive(has_overlay)
        .build();
    let timing_box = gtk::Box::builder()
        .orientation(gtk::Orientation::Horizontal)
        .spacing(8)
        .build();
    timing_box.append(&mark_appears_button);
    timing_box.append(&mark_disappears_button);
    options_group.add(&action_row_with_suffix("Timing", &timing_box));

    page.add(&overlays_group);
    page.add(&options_group);
    (
        scrolled_page(page),
        OverlayInspectorWidgets {
            text: Some(text_view),
            font_row: Some(font_row),
            font: Some(font_button),
            font_refresh: Some(font_refresh_button),
            text_color: Some(text_color_button),
            stroke_color: Some(stroke_color_button),
            list: Some(overlay_list),
            add: Some(add_button),
            delete: Some(delete_button),
            start: None,
            end: None,
            mark_start: Some(mark_appears_button),
            mark_end: Some(mark_disappears_button),
            font_size: Some(font_size_row),
            bold: Some(bold_button),
            alignment: Some(alignment_button),
            stroke_width: Some(stroke_width_row),
            shadow: None,
        },
    )
}

fn spin_row(title: &str, value: f64, min: f64, max: f64, step: f64) -> adw::SpinRow {
    let adjustment = gtk::Adjustment::new(value, min, max, step, step * 10.0, 0.0);
    adw::SpinRow::builder()
        .title(title)
        .adjustment(&adjustment)
        .digits(if step < 1.0 { 2 } else { 0 })
        .build()
}

fn switch_row(title: &str, active: bool) -> adw::SwitchRow {
    adw::SwitchRow::builder()
        .title(title)
        .active(active)
        .build()
}

fn action_row_with_suffix(title: &str, suffix: &impl IsA<gtk::Widget>) -> adw::ActionRow {
    let row = adw::ActionRow::builder().title(title).build();
    row.add_suffix(suffix);
    row.set_activatable_widget(Some(suffix));
    row
}

fn labeled_compact_widget(label: &str, widget: &impl IsA<gtk::Widget>) -> gtk::Box {
    let box_ = gtk::Box::builder()
        .orientation(gtk::Orientation::Horizontal)
        .spacing(4)
        .build();
    let label = gtk::Label::builder()
        .label(label)
        .xalign(0.0)
        .css_classes(["caption"])
        .build();
    box_.append(&label);
    box_.append(widget);
    box_
}

fn populate_overlay_list(list: &gtk::ListBox, labels: &[String], selected_index: usize) {
    while let Some(child) = list.first_child() {
        list.remove(&child);
    }

    for label in labels {
        let row = gtk::ListBoxRow::new();
        let label = gtk::Label::builder()
            .label(label)
            .xalign(0.0)
            .halign(gtk::Align::Fill)
            .margin_top(8)
            .margin_bottom(8)
            .margin_start(12)
            .margin_end(12)
            .ellipsize(gtk::pango::EllipsizeMode::End)
            .build();
        row.set_child(Some(&label));
        list.append(&row);
    }

    if let Some(row) = list.row_at_index(selected_index as i32) {
        list.select_row(Some(&row));
    }
}

fn color_button(title: &str, color: RgbaColor) -> gtk::ColorDialogButton {
    let dialog = gtk::ColorDialog::builder().title(title).modal(true).build();
    let button = gtk::ColorDialogButton::new(Some(dialog));
    button.set_rgba(&rgba_to_gdk(color));
    button
}

fn text_buffer_string(buffer: &gtk::TextBuffer) -> String {
    buffer
        .text(&buffer.start_iter(), &buffer.end_iter(), true)
        .to_string()
}

fn available_font_family_count() -> usize {
    font_family_names().len()
}

fn font_family_names() -> Vec<String> {
    let mut families = pangocairo::FontMap::default()
        .list_families()
        .into_iter()
        .map(|family| family.name().to_string())
        .collect::<Vec<_>>();
    families.sort_by_key(|family| family.to_lowercase());
    families.dedup();
    families
}

fn refresh_system_fonts() {
    let font_map = pangocairo::FontMap::default();
    font_map.changed();
    let family_count = font_map.list_families().len();
    crate::diagnostics::log_line(format_args!(
        "font picker refreshed: families={family_count}"
    ));
}

fn open_font_family_dialog(
    state: &Rc<RefCell<AppState>>,
    widgets: &AppWidgets,
    window: &adw::ApplicationWindow,
) {
    let families = Rc::new(font_family_names());
    let family_count = families.len();
    crate::diagnostics::log_line(format_args!(
        "font family list opened: families={family_count}"
    ));

    let picker = gtk::Window::builder()
        .title("Select text font")
        .modal(true)
        .transient_for(window)
        .default_width(420)
        .default_height(560)
        .build();

    let root = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .spacing(8)
        .margin_top(12)
        .margin_bottom(12)
        .margin_start(12)
        .margin_end(12)
        .build();
    let search = gtk::SearchEntry::builder()
        .placeholder_text("Search installed fonts")
        .build();
    let list = gtk::ListBox::new();
    list.add_css_class("boxed-list");
    list.set_selection_mode(gtk::SelectionMode::Single);
    populate_font_family_list(&list, &families, "");

    let scroller = gtk::ScrolledWindow::builder()
        .hexpand(true)
        .vexpand(true)
        .child(&list)
        .build();
    root.append(&search);
    root.append(&scroller);
    picker.set_child(Some(&root));

    let state = Rc::clone(state);
    let widgets = widgets.clone();
    list.connect_row_activated({
        let picker = picker.clone();
        move |_, row| {
            let Some(family) = row.tooltip_text().map(|family| family.to_string()) else {
                crate::diagnostics::log_line(format_args!(
                    "font family list row activated without family"
                ));
                return;
            };
            crate::diagnostics::log_line(format_args!(
                "font family list selected: family={family}"
            ));
            update_overlay_font_family(&state, &widgets, &family);
            picker.close();
        }
    });

    search.connect_search_changed({
        let list = list.clone();
        let families = Rc::clone(&families);
        move |search| {
            let query = search.text().to_string();
            populate_font_family_list(&list, &families, &query);
        }
    });

    picker.present();
}

fn populate_font_family_list(list: &gtk::ListBox, families: &[String], query: &str) {
    while let Some(child) = list.first_child() {
        list.remove(&child);
    }

    let query = query.trim().to_lowercase();
    for family in families
        .iter()
        .filter(|family| query.is_empty() || family.to_lowercase().contains(query.as_str()))
    {
        let row = gtk::ListBoxRow::new();
        row.set_tooltip_text(Some(family));
        let label = gtk::Label::builder()
            .label(family)
            .xalign(0.0)
            .halign(gtk::Align::Fill)
            .margin_top(8)
            .margin_bottom(8)
            .margin_start(12)
            .margin_end(12)
            .ellipsize(pango::EllipsizeMode::End)
            .build();
        row.set_child(Some(&label));
        list.append(&row);
    }
}

fn media_file_filters() -> gio::ListStore {
    let filters = gio::ListStore::new::<gtk::FileFilter>();

    let media = gtk::FileFilter::new();
    media.set_name(Some("Video and GIF files"));
    media.add_mime_type("video/*");
    media.add_mime_type("image/gif");
    media.add_pattern("*.gif");
    media.add_pattern("*.GIF");
    filters.append(&media);

    let all_files = gtk::FileFilter::new();
    all_files.set_name(Some("All files"));
    all_files.add_pattern("*");
    filters.append(&all_files);

    filters
}

fn font_description_for_text_overlay(text: &TextOverlay) -> pango::FontDescription {
    let mut description = pango::FontDescription::from_string(&text.font_family);
    description.set_absolute_size((text.font_size * f64::from(pango::SCALE)).round());
    description.set_weight(pango::Weight::__Unknown(text.font_weight as i32));
    description
}

fn rgba_to_gdk(color: RgbaColor) -> gdk::RGBA {
    gdk::RGBA::new(
        color.red as f32,
        color.green as f32,
        color.blue as f32,
        color.alpha as f32,
    )
}

fn rgba_from_gdk(color: gdk::RGBA) -> RgbaColor {
    RgbaColor {
        red: f64::from(color.red()),
        green: f64::from(color.green()),
        blue: f64::from(color.blue()),
        alpha: f64::from(color.alpha()),
    }
}

fn bytes_to_megabytes(bytes: u64) -> f64 {
    bytes as f64 / 1024.0 / 1024.0
}

fn clip_fps_value(strategy: &FrameStrategy) -> f64 {
    match strategy {
        FrameStrategy::Fps(fps) => f64::from(*fps),
        FrameStrategy::Count(count) => f64::from(*count),
        FrameStrategy::DelayMillis(delay) if *delay > 0 => f64::from(1000 / delay),
        FrameStrategy::DelayMillis(_) => 12.0,
    }
}

fn project_frame_fps(project: &Project) -> f64 {
    source_frame_fps(project)
        .or_else(|| {
            project
                .clips
                .first()
                .map(|clip| clip_fps_value(&clip.frame_strategy))
        })
        .unwrap_or(12.0)
        .clamp(1.0, 240.0)
}

fn frame_duration_seconds(project: &Project) -> f64 {
    1.0 / project_frame_fps(project)
}

fn frame_index_for_seconds(project: &Project, seconds: f64) -> i64 {
    let fps = project_frame_fps(project);
    let max_frame = max_media_frame_index(project);
    ((seconds.max(0.0) * fps).round() as i64).clamp(0, max_frame)
}

fn seconds_for_frame_index(project: &Project, frame: i64) -> f64 {
    let fps = project_frame_fps(project);
    let duration = project_duration_seconds(project)
        .or_else(|| project.clips.first().map(|clip| clip.range.end_seconds))
        .unwrap_or(3.0)
        .max(0.01);
    let frame = frame.clamp(0, max_media_frame_index(project));
    (frame as f64 / fps).clamp(0.0, duration)
}

fn snap_seconds_to_project_frame(project: &Project, seconds: f64) -> f64 {
    seconds_for_frame_index(project, frame_index_for_seconds(project, seconds))
}

fn max_media_frame_index(project: &Project) -> i64 {
    let fps = project_frame_fps(project);
    let duration = project_duration_seconds(project)
        .or_else(|| project.clips.first().map(|clip| clip.range.end_seconds))
        .unwrap_or(3.0)
        .max(0.01);
    (duration * fps).floor().max(1.0) as i64
}

fn cropped_source_dimensions(project: &Project) -> Option<(f64, f64)> {
    let source = project.source.as_ref()?;
    let source_width = f64::from(source.natural_width?.max(1));
    let source_height = f64::from(source.natural_height?.max(1));
    let crop = project.clips.first().and_then(|clip| clip.crop);
    let crop = crop.unwrap_or(CropRect {
        left: 0.0,
        right: 0.0,
        top: 0.0,
        bottom: 0.0,
    });
    let width_fraction =
        (1.0 - crop.left.clamp(0.0, 0.95) - crop.right.clamp(0.0, 0.95)).clamp(0.02, 1.0);
    let height_fraction =
        (1.0 - crop.top.clamp(0.0, 0.95) - crop.bottom.clamp(0.0, 0.95)).clamp(0.02, 1.0);
    Some((
        (source_width * width_fraction).max(1.0),
        (source_height * height_fraction).max(1.0),
    ))
}

fn output_aspect_ratio(project: &Project) -> f64 {
    cropped_source_dimensions(project)
        .map(|(width, height)| width / height.max(1.0))
        .unwrap_or(16.0 / 9.0)
        .max(0.01)
}

fn paired_output_height_for_width(project: &Project, width: u32) -> u32 {
    (f64::from(width.max(1)) / output_aspect_ratio(project))
        .round()
        .max(1.0) as u32
}

fn paired_output_width_for_height(project: &Project, height: u32) -> u32 {
    (f64::from(height.max(1)) * output_aspect_ratio(project))
        .round()
        .max(1.0) as u32
}

fn effective_output_dimensions(project: &Project) -> Option<(u32, u32)> {
    let crop_dimensions = cropped_source_dimensions(project).map(|(width, height)| {
        (
            width.round().max(1.0) as u32,
            height.round().max(1.0) as u32,
        )
    });
    match (
        project.settings.gif.output_width,
        project.settings.gif.output_height,
    ) {
        (Some(width), Some(height)) => Some((width.max(1), height.max(1))),
        (Some(width), None) => Some((width.max(1), paired_output_height_for_width(project, width))),
        (None, Some(height)) => Some((
            paired_output_width_for_height(project, height),
            height.max(1),
        )),
        (None, None) => crop_dimensions,
    }
}

fn reflow_output_height_from_width(project: &mut Project) {
    if let Some(width) = project.settings.gif.output_width {
        project.settings.gif.output_height = Some(paired_output_height_for_width(project, width));
    }
}

fn scrolled_page(page: adw::PreferencesPage) -> gtk::ScrolledWindow {
    gtk::ScrolledWindow::builder()
        .child(&page)
        .hscrollbar_policy(gtk::PolicyType::Never)
        .vscrollbar_policy(gtk::PolicyType::Automatic)
        .build()
}

fn install_actions(app: &adw::Application, window: &adw::ApplicationWindow) {
    let quit_action = gio::SimpleAction::new("quit", None);
    let weak_app = app.downgrade();
    quit_action.connect_activate(move |_, _| {
        if let Some(app) = weak_app.upgrade() {
            app.quit();
        }
    });
    app.add_action(&quit_action);
    app.set_accels_for_action("app.quit", &["<primary>q"]);

    let css = gtk::CssProvider::new();
    css.load_from_string(
        r#"
        .video-canvas {
          background: #111418;
        }

        .timeline {
          background: color-mix(in srgb, @window_bg_color 92%, @accent_bg_color);
        }

        .caption-overlay {
          color: white;
          font-weight: 700;
          font-size: 32px;
          padding: 8px 14px;
          border: 1px solid rgba(255,255,255,0.45);
          background: rgba(0,0,0,0.18);
        }

        .caption-shadow {
          text-shadow: 0 1px 2px black;
        }

        .media-empty-state {
          background: rgba(0,0,0,0.32);
          padding: 18px;
        }

        .source-status {
          background: transparent;
          border: none;
          padding: 4px 6px;
        }

        .source-status label {
          color: white;
          text-shadow: 0 1px 2px rgba(0,0,0,0.95);
        }

        .source-title {
          font-weight: 700;
        }
        "#,
    );

    if let Some(display) = gdk::Display::default() {
        gtk::style_context_add_provider_for_display(
            &display,
            &css,
            gtk::STYLE_PROVIDER_PRIORITY_APPLICATION,
        );
    }

    let about = gio::SimpleAction::new("about", None);
    let weak_window = window.downgrade();
    about.connect_activate(move |_, _| {
        if let Some(window) = weak_window.upgrade() {
            let dialog = adw::AboutDialog::builder()
                .application_name("GIF Brewery")
                .application_icon("image-x-generic-symbolic")
                .developer_name("GIF Brewery Linux contributors")
                .version("0.1.0")
                .comments("A native GNOME GIF editor inspired by GIF Brewery 3.")
                .build();
            dialog.present(Some(&window));
        }
    });
    app.add_action(&about);
}

fn show_runtime_issues_if_needed(window: &adw::ApplicationWindow) {
    let issues = crate::diagnostics::runtime_issues(crate::diagnostics::gstreamer_ready());
    if issues.is_empty() {
        if let Some(path) = crate::diagnostics::log_path() {
            crate::diagnostics::log_line(format_args!("runtime dependency check passed"));
            crate::diagnostics::log_line(format_args!("debug log path: {}", path.display()));
        }
        return;
    }

    let log_note = crate::diagnostics::log_path()
        .map(|path| format!("\n\nDebug log: {}", path.display()))
        .unwrap_or_default();
    let body = format!(
        "{}{}",
        issues
            .iter()
            .map(|issue| format!("- {issue}"))
            .collect::<Vec<_>>()
            .join("\n"),
        log_note
    );

    crate::diagnostics::log_line(format_args!("runtime dependency issues:\n{body}"));

    let dialog = adw::AlertDialog::builder()
        .heading("GIF Brewery is missing runtime support")
        .body(&body)
        .build();
    dialog.add_response("close", "Close");
    dialog.set_default_response(Some("close"));
    dialog.set_close_response("close");
    dialog.present(Some(window));
}

fn open_media_dialog(
    state: &Rc<RefCell<AppState>>,
    widgets: &AppWidgets,
    window: &adw::ApplicationWindow,
) {
    crate::diagnostics::log_line(format_args!("open media dialog requested"));
    let state = Rc::clone(state);
    let widgets = widgets.clone();
    let dialog = gtk::FileDialog::builder()
        .title("Open Media")
        .filters(&media_file_filters())
        .modal(true)
        .build();

    dialog.open(
        Some(window),
        gio::Cancellable::NONE,
        move |result| match result {
            Ok(file) => apply_source_file(&state, &widgets, &file),
            Err(err) => {
                crate::diagnostics::log_line(format_args!(
                    "open media dialog closed without file: {err}"
                ));
            }
        },
    );
}

fn install_widget_bindings(
    state: &Rc<RefCell<AppState>>,
    widgets: &AppWidgets,
    window: &adw::ApplicationWindow,
) {
    widgets.editor.restart_button.connect_clicked({
        let state = Rc::clone(state);
        let widgets = widgets.clone();
        move |_| restart_preview_at_clip_start(&state, &widgets)
    });

    widgets.editor.play_button.connect_clicked({
        let state = Rc::clone(state);
        let widgets = widgets.clone();
        move |_| start_preview_playback(&state, &widgets)
    });

    widgets.editor.pause_button.connect_clicked({
        let state = Rc::clone(state);
        let widgets = widgets.clone();
        move |_| pause_preview_playback(&state, &widgets)
    });

    widgets
        .inspector
        .clip_start
        .connect_notify_local(Some("value"), {
            let state = Rc::clone(state);
            let widgets = widgets.clone();
            move |row, _| {
                if state.borrow().syncing_widgets {
                    return;
                }
                update_clip_start_frame(&state, &widgets, row.value());
            }
        });

    widgets
        .inspector
        .clip_end
        .connect_notify_local(Some("value"), {
            let state = Rc::clone(state);
            let widgets = widgets.clone();
            move |row, _| {
                if state.borrow().syncing_widgets {
                    return;
                }
                update_clip_end_frame(&state, &widgets, row.value());
            }
        });

    widgets
        .inspector
        .target_size_mb
        .connect_notify_local(Some("value"), {
            let state = Rc::clone(state);
            let widgets = widgets.clone();
            move |row, _| {
                if state.borrow().syncing_widgets {
                    return;
                }
                update_target_size_mb(&state, &widgets, row.value());
            }
        });

    widgets
        .inspector
        .clip_speed
        .connect_notify_local(Some("value"), {
            let state = Rc::clone(state);
            let widgets = widgets.clone();
            move |row, _| {
                if state.borrow().syncing_widgets {
                    return;
                }
                update_clip_speed(&state, &widgets, row.value());
            }
        });

    widgets
        .inspector
        .clip_fps
        .connect_notify_local(Some("value"), {
            let state = Rc::clone(state);
            let widgets = widgets.clone();
            move |row, _| {
                if state.borrow().syncing_widgets {
                    return;
                }
                update_clip_fps(&state, &widgets, row.value());
            }
        });

    widgets
        .inspector
        .optimize_gif
        .connect_notify_local(Some("active"), {
            let state = Rc::clone(state);
            let widgets = widgets.clone();
            move |row, _| {
                if state.borrow().syncing_widgets {
                    return;
                }
                update_optimize_gif(&state, &widgets, row.is_active());
            }
        });

    widgets
        .inspector
        .high_quality_quantization
        .connect_notify_local(Some("active"), {
            let state = Rc::clone(state);
            let widgets = widgets.clone();
            move |row, _| {
                if state.borrow().syncing_widgets {
                    return;
                }
                update_high_quality_quantization(&state, &widgets, row.is_active());
            }
        });

    widgets
        .inspector
        .output_width
        .connect_notify_local(Some("value"), {
            let state = Rc::clone(state);
            let widgets = widgets.clone();
            move |row, _| {
                if state.borrow().syncing_widgets {
                    return;
                }
                update_output_width(&state, &widgets, row.value());
            }
        });

    widgets
        .inspector
        .output_height
        .connect_notify_local(Some("value"), {
            let state = Rc::clone(state);
            let widgets = widgets.clone();
            move |row, _| {
                if state.borrow().syncing_widgets {
                    return;
                }
                update_output_height(&state, &widgets, row.value());
            }
        });

    widgets
        .inspector
        .crop_left
        .connect_notify_local(Some("value"), {
            let state = Rc::clone(state);
            let widgets = widgets.clone();
            move |row, _| {
                if state.borrow().syncing_widgets {
                    return;
                }
                update_clip_crop_margin(&state, &widgets, CropEdge::Left, row.value());
            }
        });

    widgets
        .inspector
        .crop_right
        .connect_notify_local(Some("value"), {
            let state = Rc::clone(state);
            let widgets = widgets.clone();
            move |row, _| {
                if state.borrow().syncing_widgets {
                    return;
                }
                update_clip_crop_margin(&state, &widgets, CropEdge::Right, row.value());
            }
        });

    widgets
        .inspector
        .crop_top
        .connect_notify_local(Some("value"), {
            let state = Rc::clone(state);
            let widgets = widgets.clone();
            move |row, _| {
                if state.borrow().syncing_widgets {
                    return;
                }
                update_clip_crop_margin(&state, &widgets, CropEdge::Top, row.value());
            }
        });

    widgets
        .inspector
        .crop_bottom
        .connect_notify_local(Some("value"), {
            let state = Rc::clone(state);
            let widgets = widgets.clone();
            move |row, _| {
                if state.borrow().syncing_widgets {
                    return;
                }
                update_clip_crop_margin(&state, &widgets, CropEdge::Bottom, row.value());
            }
        });

    if let Some(row) = &widgets.inspector.overlay_start {
        row.connect_notify_local(Some("value"), {
            let state = Rc::clone(state);
            let widgets = widgets.clone();
            move |row, _| {
                if state.borrow().syncing_widgets {
                    return;
                }
                update_overlay_start(&state, &widgets, row.value());
            }
        });
    }

    if let Some(row) = &widgets.inspector.overlay_end {
        row.connect_notify_local(Some("value"), {
            let state = Rc::clone(state);
            let widgets = widgets.clone();
            move |row, _| {
                if state.borrow().syncing_widgets {
                    return;
                }
                update_overlay_end(&state, &widgets, row.value());
            }
        });
    }

    if let Some(list) = &widgets.inspector.overlay_list {
        list.connect_row_selected({
            let state = Rc::clone(state);
            let widgets = widgets.clone();
            move |_, row| {
                if state.borrow().syncing_widgets {
                    return;
                }
                if let Some(row) = row {
                    select_overlay_by_index(&state, &widgets, row.index().max(0) as usize);
                }
            }
        });
    }

    if let Some(button) = &widgets.inspector.overlay_add {
        button.connect_clicked({
            let state = Rc::clone(state);
            let widgets = widgets.clone();
            move |_| {
                add_text_overlay_at_playhead(&state, &widgets);
            }
        });
    }

    if let Some(button) = &widgets.inspector.overlay_delete {
        button.connect_clicked({
            let state = Rc::clone(state);
            let widgets = widgets.clone();
            move |_| {
                delete_selected_overlay(&state, &widgets);
            }
        });
    }

    if let Some(button) = &widgets.inspector.overlay_mark_start {
        button.connect_clicked({
            let state = Rc::clone(state);
            let widgets = widgets.clone();
            move |_| {
                mark_overlay_start_at_playhead(&state, &widgets);
            }
        });
    }

    if let Some(button) = &widgets.inspector.overlay_mark_end {
        button.connect_clicked({
            let state = Rc::clone(state);
            let widgets = widgets.clone();
            move |_| {
                mark_overlay_end_at_playhead(&state, &widgets);
            }
        });
    }

    if let Some(row) = &widgets.inspector.overlay_text {
        row.buffer().connect_changed({
            let state = Rc::clone(state);
            let widgets = widgets.clone();
            move |buffer| {
                if state.borrow().syncing_widgets {
                    return;
                }
                update_overlay_text(&state, &widgets, text_buffer_string(buffer).as_str());
            }
        });
    }

    if let Some(button) = &widgets.inspector.overlay_font {
        button.connect_clicked({
            let state = Rc::clone(state);
            let widgets = widgets.clone();
            let window = window.clone();
            move |_| {
                open_font_family_dialog(&state, &widgets, &window);
            }
        });
    }

    if let Some(button) = &widgets.inspector.overlay_font_refresh {
        button.connect_clicked(move |_| refresh_system_fonts());
    }

    if let Some(button) = &widgets.inspector.overlay_text_color {
        button.connect_notify_local(Some("rgba"), {
            let state = Rc::clone(state);
            let widgets = widgets.clone();
            move |button, _| {
                if state.borrow().syncing_widgets {
                    return;
                }
                update_overlay_text_color(&state, &widgets, rgba_from_gdk(button.rgba()));
            }
        });
    }

    if let Some(button) = &widgets.inspector.overlay_stroke_color {
        button.connect_notify_local(Some("rgba"), {
            let state = Rc::clone(state);
            let widgets = widgets.clone();
            move |button, _| {
                if state.borrow().syncing_widgets {
                    return;
                }
                update_overlay_stroke_color(&state, &widgets, rgba_from_gdk(button.rgba()));
            }
        });
    }

    if let Some(row) = &widgets.inspector.overlay_font_size {
        row.connect_notify_local(Some("value"), {
            let state = Rc::clone(state);
            let widgets = widgets.clone();
            move |row, _| {
                if state.borrow().syncing_widgets {
                    return;
                }
                update_overlay_font_size(&state, &widgets, row.value());
            }
        });
    }

    if let Some(button) = &widgets.inspector.overlay_bold {
        button.connect_toggled({
            let state = Rc::clone(state);
            let widgets = widgets.clone();
            move |button| {
                if state.borrow().syncing_widgets {
                    return;
                }
                update_overlay_bold(&state, &widgets, button.is_active());
            }
        });
    }

    if let Some(button) = &widgets.inspector.overlay_alignment {
        button.connect_toggled({
            let state = Rc::clone(state);
            let widgets = widgets.clone();
            move |button| {
                if state.borrow().syncing_widgets {
                    return;
                }
                let alignment = if button.is_active() {
                    TextAlignment::Center
                } else {
                    TextAlignment::Left
                };
                update_overlay_alignment(&state, &widgets, alignment);
            }
        });
    }

    if let Some(row) = &widgets.inspector.overlay_stroke_width {
        row.connect_notify_local(Some("value"), {
            let state = Rc::clone(state);
            let widgets = widgets.clone();
            move |row, _| {
                if state.borrow().syncing_widgets {
                    return;
                }
                update_overlay_stroke_width(&state, &widgets, row.value());
            }
        });
    }

    if let Some(row) = &widgets.inspector.overlay_shadow {
        row.connect_notify_local(Some("active"), {
            let state = Rc::clone(state);
            let widgets = widgets.clone();
            move |row, _| {
                if state.borrow().syncing_widgets {
                    return;
                }
                update_overlay_shadow(&state, &widgets, row.is_active());
            }
        });
    }

    widgets.editor.timeline_view.connect_seek({
        let state = Rc::clone(state);
        let widgets = widgets.clone();
        move |seconds| {
            update_playhead(&state, &widgets, seconds);
        }
    });

    widgets.editor.timeline_view.connect_clip_changed({
        let state = Rc::clone(state);
        let widgets = widgets.clone();
        move |range| {
            update_clip_range(&state, &widgets, range);
        }
    });

    widgets.editor.timeline_view.connect_overlay_changed({
        let state = Rc::clone(state);
        let widgets = widgets.clone();
        move |id, range| {
            update_overlay_range(&state, &widgets, &id, range);
        }
    });
}

fn install_keyboard_shortcuts(
    window: &adw::ApplicationWindow,
    state: &Rc<RefCell<AppState>>,
    widgets: &AppWidgets,
) {
    let controller = gtk::EventControllerKey::new();
    controller.set_propagation_phase(gtk::PropagationPhase::Capture);
    controller.connect_key_pressed({
        let state = Rc::clone(state);
        let widgets = widgets.clone();
        move |_, key, _, _| {
            if overlay_text_editor_has_focus(&widgets) {
                if key == gdk::Key::space || key == gdk::Key::Right || key == gdk::Key::Left {
                    crate::diagnostics::log_line(format_args!(
                        "keyboard shortcut ignored for overlay text focus: key={key:?}"
                    ));
                }
                return glib::Propagation::Proceed;
            }

            if key == gdk::Key::space {
                crate::diagnostics::log_line(format_args!("spacebar playback toggle"));
                toggle_preview_playback(&state, &widgets);
                glib::Propagation::Stop
            } else if key == gdk::Key::Right {
                step_playhead_by_frames(&state, &widgets, 1);
                glib::Propagation::Stop
            } else if key == gdk::Key::Left {
                step_playhead_by_frames(&state, &widgets, -1);
                glib::Propagation::Stop
            } else {
                glib::Propagation::Proceed
            }
        }
    });
    window.add_controller(controller);
}

fn overlay_text_editor_has_focus(widgets: &AppWidgets) -> bool {
    widgets
        .inspector
        .overlay_text
        .as_ref()
        .map(|text| text.has_focus())
        .unwrap_or(false)
}

fn install_playback_poll(state: &Rc<RefCell<AppState>>, widgets: &AppWidgets) {
    let state = Rc::clone(state);
    let widgets = widgets.clone();
    glib::timeout_add_local(std::time::Duration::from_millis(16), move || {
        sync_playback_position(&state, &widgets);
        glib::ControlFlow::Continue
    });
}

fn install_preview_overlay_drag(state: &Rc<RefCell<AppState>>, widgets: &AppWidgets) {
    let Some(caption) = &widgets.editor.caption_overlay else {
        return;
    };

    let preview_click = gtk::GestureClick::new();
    preview_click.connect_pressed({
        let preview = widgets.editor.preview.clone();
        move |_, _, _, _| {
            let _ = preview.grab_focus();
        }
    });
    widgets.editor.preview.add_controller(preview_click);

    let click = gtk::GestureClick::new();
    click.connect_pressed({
        let state = Rc::clone(state);
        let caption = caption.clone();
        let widgets = widgets.clone();
        move |_, _, x, y| {
            let _ = caption.area.grab_focus();
            let Some((id, _)) = caption.hit_test(x, y) else {
                return;
            };
            {
                let mut state = state.borrow_mut();
                state.selected_overlay_id = Some(id.clone());
            }
            crate::diagnostics::log_line(format_args!("preview overlay clicked: id={id}"));
            update_timeline_widgets(&state, &widgets);
        }
    });
    caption.area.add_controller(click);

    let drag = gtk::GestureDrag::new();
    let drag_start = Rc::new(RefCell::new(None::<CaptionDragStart>));
    let drag_active = Rc::new(Cell::new(false));

    drag.connect_drag_begin({
        let state = Rc::clone(state);
        let caption = caption.clone();
        let widgets = widgets.clone();
        let drag_start = Rc::clone(&drag_start);
        let drag_active = Rc::clone(&drag_active);
        move |_, x, y| {
            let _ = caption.area.grab_focus();
            let hit = caption.hit_test(x, y);
            let active = hit.is_some();
            crate::diagnostics::log_line(format_args!(
                "caption drag begin: pointer=({x:.1},{y:.1}) hit={hit:?} active={active}"
            ));
            drag_active.set(active);
            if !active {
                *drag_start.borrow_mut() = None;
                return;
            }

            let (id, pixel_bounds) = hit.expect("active hit exists");
            let model_bounds = {
                let mut state = state.borrow_mut();
                state.selected_overlay_id = Some(id.clone());
                crate::diagnostics::log_line(format_args!("preview overlay selected: id={id}"));
                selected_text_overlay(&state.project, Some(id.as_str())).map(|text| text.bounds)
            };
            update_timeline_widgets(&state, &widgets);
            let start = model_bounds.map(|model_bounds| CaptionDragStart {
                model_bounds,
                pixel_bounds: Some(pixel_bounds),
            });
            crate::diagnostics::log_line(format_args!("caption drag start: {start:?}"));
            *drag_start.borrow_mut() = start;
        }
    });

    drag.connect_drag_update({
        let state = Rc::clone(state);
        let widgets = widgets.clone();
        let drag_start = Rc::clone(&drag_start);
        let drag_active = Rc::clone(&drag_active);
        move |_, offset_x, offset_y| {
            if !drag_active.get() {
                return;
            }
            let Some(start) = *drag_start.borrow() else {
                return;
            };
            crate::diagnostics::log_line(format_args!(
                "caption drag update: offset=({offset_x:.1},{offset_y:.1}) start={start:?}"
            ));
            update_overlay_position_from_drag(&state, &widgets, start, offset_x, offset_y);
        }
    });

    drag.connect_drag_end({
        let drag_start = Rc::clone(&drag_start);
        let drag_active = Rc::clone(&drag_active);
        move |_, _, _| {
            crate::diagnostics::log_line(format_args!("caption drag end"));
            drag_active.set(false);
            *drag_start.borrow_mut() = None;
        }
    });

    caption.area.add_controller(drag);
}

fn apply_source_file(state: &Rc<RefCell<AppState>>, widgets: &AppWidgets, file: &gio::File) {
    let path = file.path();
    let uri = file.uri();
    crate::diagnostics::log_line(format_args!("loading media: {uri}"));
    let metadata = match crate::media::discover(file) {
        Ok(metadata) => {
            crate::diagnostics::log_line(format_args!("media metadata: {metadata:?}"));
            Some(metadata)
        }
        Err(err) => {
            crate::diagnostics::log_line(format_args!("media discovery failed for {uri}: {err}"));
            None
        }
    };
    let thumbnail_source = path.as_ref().zip(
        metadata
            .as_ref()
            .and_then(|metadata| metadata.duration_seconds),
    );
    let display_path = path
        .as_ref()
        .map(|path| path.display().to_string())
        .unwrap_or_else(|| uri.to_string());
    let title = path
        .as_ref()
        .and_then(|path| path.file_name())
        .and_then(|name| name.to_str())
        .map(ToOwned::to_owned)
        .unwrap_or_else(|| uri.to_string());

    {
        let mut state = state.borrow_mut();
        let duration_seconds = metadata
            .as_ref()
            .and_then(|metadata| metadata.duration_seconds)
            .filter(|duration| *duration > 0.0);

        state.project.source = Some(MediaSource {
            path: display_path.clone(),
            duration_seconds,
            natural_width: metadata.as_ref().and_then(|metadata| metadata.width),
            natural_height: metadata.as_ref().and_then(|metadata| metadata.height),
            fps: metadata
                .as_ref()
                .and_then(|metadata| metadata.fps)
                .filter(|fps| *fps > 0.0),
        });
        state.playhead_seconds = 0.0;
        state.thumbnails = Vec::new();
        state.thumbnail_generation = state.thumbnail_generation.wrapping_add(1);
        invalidate_render_outputs(&mut state);

        if let Some(duration_seconds) = duration_seconds {
            let clip_end = duration_seconds.max(0.01);
            if let Some(clip) = state.project.clips.first_mut() {
                clip.range.start_seconds = 0.0;
                clip.range.end_seconds = clip_end;
            }

            for overlay in &mut state.project.overlays {
                match overlay {
                    Overlay::Text(text) => {
                        text.range = normalized_range_for_clip(
                            TimelineRange {
                                start_seconds: 0.0,
                                end_seconds: clip_end,
                            },
                            TimelineRange {
                                start_seconds: 0.0,
                                end_seconds: clip_end,
                            },
                        );
                    }
                }
            }
        }
    }

    widgets
        .editor
        .source_title
        .set_label("Loading preview frames...");
    widgets.editor.source_detail.set_label(&title);
    widgets
        .editor
        .source_detail
        .set_tooltip_text(Some(&source_detail(
            &compact_path(&display_path),
            metadata.as_ref(),
        )));
    widgets.editor.empty_state.set_visible(false);
    if let Some(caption) = &widgets.editor.caption_overlay {
        caption.area.set_visible(true);
    }
    update_timeline_widgets(state, widgets);
    if let Some((path, duration)) = thumbnail_source {
        start_thumbnail_worker(state, widgets, path.to_path_buf(), duration);
    }
    if should_auto_preload_rendered_playback(&state.borrow().project) {
        start_rendered_playback_preload(state, widgets, "source loaded");
    } else {
        crate::diagnostics::log_line(format_args!(
            "rendered sequence preload skipped on source load: estimated cache too large"
        ));
    }
    widgets.editor.export_button.set_sensitive(true);
}

fn start_thumbnail_worker(
    state: &Rc<RefCell<AppState>>,
    widgets: &AppWidgets,
    path: PathBuf,
    duration: f64,
) {
    let generation = state.borrow().thumbnail_generation;
    let (sender, receiver) = mpsc::channel();
    thread::spawn(move || {
        let started = Instant::now();
        let thumbnails = thumbnails::extract_thumbnail_files(&path, duration, 12);
        let elapsed = started.elapsed().as_secs_f64();
        let _ = sender.send((generation, thumbnails, elapsed));
    });

    let receiver = Rc::new(RefCell::new(receiver));
    let state = Rc::clone(state);
    let widgets = widgets.clone();
    glib::timeout_add_local(std::time::Duration::from_millis(80), move || {
        let Ok((generation, thumbnails, elapsed)) = receiver.borrow_mut().try_recv() else {
            return glib::ControlFlow::Continue;
        };
        if state.borrow().thumbnail_generation == generation {
            let thumbnails = thumbnails::load_thumbnail_pixbufs(&thumbnails);
            crate::diagnostics::log_line(format_args!(
                "timeline thumbnails ready: count={} elapsed={elapsed:.3}s",
                thumbnails.len()
            ));
            state.borrow_mut().thumbnails = thumbnails;
            update_timeline_widgets(&state, &widgets);
        } else {
            crate::diagnostics::log_line(format_args!(
                "discarded stale timeline thumbnails: generation={generation}"
            ));
        }
        glib::ControlFlow::Break
    });
}

fn start_rendered_playback_preload(
    state: &Rc<RefCell<AppState>>,
    widgets: &AppWidgets,
    reason: &str,
) {
    let (project, cache_key, generation, output_dir) = {
        let mut state = state.borrow_mut();
        if state.project.source.is_none() {
            return;
        }
        if state.rendered_playback_preparing {
            state.rendered_playback_rebuild_requested = true;
            crate::diagnostics::log_line(format_args!(
                "rendered sequence preload already running; queued rebuild reason={reason}"
            ));
            return;
        }
        let project = playback_preload_project(&state.project);
        let cache_key = rendered_playback_cache_key(&project);
        if state
            .rendered_playback_cache
            .as_ref()
            .is_some_and(|cache| cache.key == cache_key && !cache.frames.is_empty())
        {
            state.rendered_playback_rebuild_requested = false;
            return;
        }
        state.rendered_playback_preparing = true;
        state.rendered_playback_rebuild_requested = false;
        let generation = state.rendered_playback_generation;
        (
            project,
            cache_key.clone(),
            generation,
            rendered_playback_sequence_dir(&cache_key),
        )
    };

    widgets
        .editor
        .source_title
        .set_label("Loading preview frames...");
    widgets
        .editor
        .export_status
        .set_label("Preparing frame cache");
    widgets.editor.export_status.set_visible(true);
    crate::diagnostics::log_line(format_args!(
        "rendered sequence preload started: reason={reason} key={cache_key}"
    ));

    let (sender, receiver) = mpsc::channel::<(
        u64,
        String,
        PathBuf,
        Result<crate::export::RenderedFrameSequence, String>,
    )>();
    thread::spawn({
        let cache_key = cache_key.clone();
        let output_dir = output_dir.clone();
        move || {
            let started = Instant::now();
            let _ = std::fs::remove_dir_all(&output_dir);
            let result = crate::export::render_frame_sequence(&project, &output_dir);
            crate::diagnostics::log_line(format_args!(
                "rendered sequence preload worker finished in {:.3}s",
                started.elapsed().as_secs_f64()
            ));
            let _ = sender.send((generation, cache_key, output_dir, result));
        }
    });

    let receiver = Rc::new(RefCell::new(receiver));
    let state = Rc::clone(state);
    let widgets = widgets.clone();
    glib::timeout_add_local(std::time::Duration::from_millis(80), move || {
        let Ok((generation, cache_key, output_dir, result)) = receiver.borrow_mut().try_recv()
        else {
            return glib::ControlFlow::Continue;
        };

        let current_key =
            rendered_playback_cache_key(&playback_preload_project(&state.borrow().project));
        let is_current =
            state.borrow().rendered_playback_generation == generation && current_key == cache_key;
        if !is_current {
            let should_restart = state.borrow().rendered_playback_rebuild_requested;
            {
                let mut state = state.borrow_mut();
                state.rendered_playback_preparing = false;
                state.rendered_playback_rebuild_requested = false;
            }
            let _ = std::fs::remove_dir_all(output_dir);
            crate::diagnostics::log_line(format_args!(
                "rendered sequence preload discarded stale cache"
            ));
            if should_restart {
                defer_rendered_playback_preload(&state, &widgets, "coalesced project changes");
            }
            return glib::ControlFlow::Break;
        }

        match result {
            Ok(sequence) => {
                let clip_range = state
                    .borrow()
                    .project
                    .clips
                    .first()
                    .map(|clip| clip.range)
                    .unwrap_or(TimelineRange {
                        start_seconds: 0.0,
                        end_seconds: sequence.duration_seconds,
                    });
                {
                    let mut state = state.borrow_mut();
                    state.rendered_playback_cache = Some(RenderedPlaybackCache {
                        key: cache_key.clone(),
                        frames: sequence.frames,
                        fps: f64::from(sequence.fps),
                        frame_duration_seconds: 1.0 / f64::from(sequence.fps.max(1)),
                        clip_start_seconds: clip_range.start_seconds,
                        clip_end_seconds: clip_range.end_seconds,
                    });
                    state.rendered_playback_preparing = false;
                    state.rendered_playback_rebuild_requested = false;
                }
                widgets.editor.source_title.set_label("Preview ready");
                widgets.editor.export_status.set_visible(false);
                crate::diagnostics::log_line(format_args!(
                    "rendered sequence preload ready: dir={} fps={} frames={}",
                    output_dir.display(),
                    sequence.fps,
                    state
                        .borrow()
                        .rendered_playback_cache
                        .as_ref()
                        .map(|cache| cache.frames.len())
                        .unwrap_or(0)
                ));
                update_timeline_widgets(&state, &widgets);
            }
            Err(err) => {
                {
                    let mut state = state.borrow_mut();
                    state.rendered_playback_preparing = false;
                    state.rendered_playback_rebuild_requested = false;
                    state.rendered_playback_cache = None;
                }
                widgets.editor.source_title.set_label("Preview ready");
                widgets.editor.export_status.set_visible(false);
                crate::diagnostics::log_line(format_args!(
                    "rendered sequence preload failed: {err}"
                ));
            }
        }

        glib::ControlFlow::Break
    });
}

fn export_current_gif(
    state: &Rc<RefCell<AppState>>,
    widgets: &AppWidgets,
    window: &adw::ApplicationWindow,
) {
    let project = state.borrow().project.clone();
    if project.source.is_none() {
        return;
    }

    let dialog = gtk::FileDialog::builder()
        .title("Save GIF")
        .initial_name("gifbrewery-export.gif")
        .modal(true)
        .build();

    let widgets = widgets.clone();
    let dialog_parent = window.clone();
    let callback_window = window.clone();
    dialog.save(
        Some(&dialog_parent),
        gio::Cancellable::NONE,
        move |result| {
            let Ok(file) = result else {
                return;
            };
            let Some(output_path) = file.path() else {
                eprintln!("GIF export failed: selected destination is not a local path");
                return;
            };

            widgets.editor.export_button.set_sensitive(false);
            widgets.editor.export_button.set_label("Exporting...");
            widgets.editor.export_spinner.set_visible(true);
            widgets.editor.export_spinner.start();
            widgets.editor.export_status.set_label("Exporting GIF...");
            widgets.editor.export_status.set_visible(true);

            start_export_worker(
                project.clone(),
                output_path,
                widgets.clone(),
                callback_window.clone(),
            );
        },
    );
}

fn start_export_worker(
    project: Project,
    output_path: PathBuf,
    widgets: AppWidgets,
    window: adw::ApplicationWindow,
) {
    enum ExportWorkerMessage {
        Progress(crate::export::ExportProgress),
        Finished(PathBuf, Result<(), String>),
    }

    let (sender, receiver) = mpsc::channel::<ExportWorkerMessage>();
    let worker_output_path = output_path.clone();
    thread::spawn(move || {
        crate::diagnostics::log_line(format_args!(
            "GIF export worker started: {}",
            worker_output_path.display()
        ));
        let progress_sender = sender.clone();
        let result = crate::export::export_gif_with_progress(
            &project,
            Path::new(&worker_output_path),
            move |progress| {
                let _ = progress_sender.send(ExportWorkerMessage::Progress(progress));
            },
        );
        let _ = sender.send(ExportWorkerMessage::Finished(worker_output_path, result));
    });

    let receiver = Rc::new(RefCell::new(receiver));
    glib::timeout_add_local(std::time::Duration::from_millis(150), move || loop {
        let message = receiver.borrow_mut().try_recv();
        match message {
            Ok(ExportWorkerMessage::Progress(progress)) => {
                let label = if let Some(percent) = progress.percent {
                    format!("{} ({percent}%)", progress.message)
                } else {
                    progress.message
                };
                widgets.editor.export_status.set_label(&label);
            }
            Ok(ExportWorkerMessage::Finished(path, result)) => {
                widgets.editor.export_button.set_label("Create GIF");
                widgets.editor.export_button.set_sensitive(true);
                widgets.editor.export_spinner.stop();
                widgets.editor.export_spinner.set_visible(false);
                widgets.editor.export_status.set_visible(false);

                match result {
                    Ok(()) => {
                        crate::diagnostics::log_line(format_args!(
                            "exported GIF to {}",
                            path.display()
                        ));
                        show_export_preview(&window, &path);
                    }
                    Err(err) => {
                        crate::diagnostics::log_line(format_args!("GIF export failed: {err}"));
                        show_export_error(&window, &err);
                    }
                }

                return glib::ControlFlow::Break;
            }
            Err(mpsc::TryRecvError::Empty) => return glib::ControlFlow::Continue,
            Err(mpsc::TryRecvError::Disconnected) => return glib::ControlFlow::Break,
        }
    });
}

fn show_export_preview(window: &adw::ApplicationWindow, output_path: &Path) {
    let file = gio::File::for_path(output_path);
    let preview = gtk::Window::builder()
        .title("GIF Exported")
        .default_width(760)
        .default_height(560)
        .modal(false)
        .build();
    preview.set_transient_for(Some(window));

    let content = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .spacing(12)
        .margin_top(12)
        .margin_bottom(12)
        .margin_start(12)
        .margin_end(12)
        .build();
    let picture = gtk::Picture::builder()
        .content_fit(gtk::ContentFit::ScaleDown)
        .vexpand(true)
        .hexpand(true)
        .build();
    content.append(&picture);

    let footer = gtk::Box::builder()
        .orientation(gtk::Orientation::Horizontal)
        .spacing(8)
        .build();
    let path_label = gtk::Label::builder()
        .label(output_path.display().to_string())
        .ellipsize(pango::EllipsizeMode::Middle)
        .xalign(0.0)
        .hexpand(true)
        .build();
    let open_button = gtk::Button::builder()
        .label("Open")
        .icon_name("document-open-symbolic")
        .build();
    open_button.connect_clicked({
        let file = file.clone();
        move |_| {
            if let Err(err) =
                gio::AppInfo::launch_default_for_uri(&file.uri(), None::<&gio::AppLaunchContext>)
            {
                crate::diagnostics::log_line(format_args!(
                    "failed to open exported GIF preview URI: {err}"
                ));
            }
        }
    });
    footer.append(&path_label);
    footer.append(&open_button);
    content.append(&footer);

    preview.set_child(Some(&content));
    preview.present();
    start_export_preview_animation(output_path, picture);
}

fn start_export_preview_animation(output_path: &Path, picture: gtk::Picture) {
    let output_path = output_path.to_path_buf();
    let preview_dir =
        std::env::temp_dir().join(format!("gifbrewery-export-preview-{}", std::process::id()));
    let (sender, receiver) = mpsc::channel::<Result<Vec<PathBuf>, String>>();
    thread::spawn(move || {
        let _ = std::fs::remove_dir_all(&preview_dir);
        if let Err(err) = std::fs::create_dir_all(&preview_dir) {
            let _ = sender.send(Err(format!(
                "failed to create export preview frame directory {}: {err}",
                preview_dir.display()
            )));
            return;
        }
        let pattern = preview_dir.join("preview-%06d.png");
        let output = Command::new("ffmpeg")
            .arg("-hide_banner")
            .arg("-y")
            .arg("-i")
            .arg(&output_path)
            .arg("-start_number")
            .arg("0")
            .arg(pattern)
            .output();
        let output = match output {
            Ok(output) => output,
            Err(err) => {
                let _ = sender.send(Err(format!(
                    "failed to run ffmpeg for export preview: {err}"
                )));
                return;
            }
        };
        if !output.status.success() {
            let _ = sender.send(Err(format!(
                "ffmpeg export preview extraction failed: {}",
                String::from_utf8_lossy(&output.stderr)
            )));
            return;
        }
        let mut frames = match std::fs::read_dir(&preview_dir) {
            Ok(entries) => entries
                .filter_map(|entry| entry.ok().map(|entry| entry.path()))
                .filter(|path| {
                    path.file_name()
                        .and_then(|name| name.to_str())
                        .is_some_and(|name| name.starts_with("preview-") && name.ends_with(".png"))
                })
                .collect::<Vec<_>>(),
            Err(err) => {
                let _ = sender.send(Err(format!(
                    "failed to read export preview frames {}: {err}",
                    preview_dir.display()
                )));
                return;
            }
        };
        frames.sort();
        if frames.is_empty() {
            let _ = sender.send(Err(
                "export preview extraction produced no frames".to_string()
            ));
        } else {
            let _ = sender.send(Ok(frames));
        }
    });

    let receiver = Rc::new(RefCell::new(receiver));
    glib::timeout_add_local(std::time::Duration::from_millis(80), move || {
        let Ok(result) = receiver.borrow_mut().try_recv() else {
            return glib::ControlFlow::Continue;
        };
        match result {
            Ok(frames) => animate_picture_frames(picture.clone(), frames, 33),
            Err(err) => crate::diagnostics::log_line(format_args!("{err}")),
        }
        glib::ControlFlow::Break
    });
}

fn animate_picture_frames(picture: gtk::Picture, frames: Vec<PathBuf>, delay_ms: u64) {
    let frames = Rc::new(frames);
    let index = Rc::new(Cell::new(0usize));
    picture.set_content_fit(gtk::ContentFit::ScaleDown);
    if let Ok(texture) = gdk::Texture::from_file(&gio::File::for_path(&frames[0])) {
        picture.set_size_request(texture.width(), texture.height());
    }
    picture.set_file(Some(&gio::File::for_path(&frames[0])));
    glib::timeout_add_local(std::time::Duration::from_millis(delay_ms), move || {
        let next = (index.get() + 1) % frames.len();
        index.set(next);
        picture.set_file(Some(&gio::File::for_path(&frames[next])));
        glib::ControlFlow::Continue
    });
}

fn show_export_error(window: &adw::ApplicationWindow, error: &str) {
    let dialog = adw::AlertDialog::builder()
        .heading("GIF Export Failed")
        .body(error)
        .build();
    dialog.add_response("close", "Close");
    dialog.set_default_response(Some("close"));
    dialog.present(Some(window));
}

fn restart_preview_at_clip_start(state: &Rc<RefCell<AppState>>, widgets: &AppWidgets) {
    let start_seconds = {
        let mut state = state.borrow_mut();
        state.is_playing = false;
        let start_seconds = state
            .project
            .clips
            .first()
            .map(|clip| clip.range.start_seconds)
            .unwrap_or(0.0);
        state.playhead_seconds = start_seconds;
        start_seconds
    };

    crate::diagnostics::log_line(format_args!(
        "preview restart at clip start: {start_seconds:.3}s"
    ));
    update_timeline_widgets(state, widgets);
}

fn start_preview_playback(state: &Rc<RefCell<AppState>>, widgets: &AppWidgets) {
    start_rendered_preview_playback(state, widgets);
}

fn start_rendered_preview_playback(state: &Rc<RefCell<AppState>>, widgets: &AppWidgets) {
    let (cache_ready, start_seconds, preparing) = {
        let mut state = state.borrow_mut();
        if state.project.source.is_none() {
            return;
        }
        let clip_range = state
            .project
            .clips
            .first()
            .map(|clip| clip.range)
            .unwrap_or(TimelineRange {
                start_seconds: 0.0,
                end_seconds: 0.01,
            });
        if state.playhead_seconds < clip_range.start_seconds
            || state.playhead_seconds >= clip_range.end_seconds
        {
            state.playhead_seconds = clip_range.start_seconds;
        }
        let cache_key = rendered_playback_cache_key(&playback_preload_project(&state.project));
        let cache_ready = state
            .rendered_playback_cache
            .as_ref()
            .is_some_and(|cache| cache.key == cache_key && !cache.frames.is_empty());
        (
            cache_ready,
            state.playhead_seconds,
            state.rendered_playback_preparing,
        )
    };

    if cache_ready {
        {
            let mut state = state.borrow_mut();
            state.is_playing = true;
            state.rendered_playback_tick = Some(Instant::now());
        }
        crate::diagnostics::log_line(format_args!(
            "rendered sequence playback start from cached frames: {start_seconds:.3}s"
        ));
        update_timeline_widgets(state, widgets);
        return;
    }

    widgets.editor.source_title.set_label(if preparing {
        "Preview frames still loading..."
    } else {
        "Preview frames are not ready"
    });
    widgets.editor.export_status.set_visible(preparing);
    widgets.editor.export_status.set_label("Please wait");
    crate::diagnostics::log_line(format_args!(
        "playback requested before frame cache was ready: playhead={start_seconds:.3}s preparing={preparing}"
    ));
}

fn sync_rendered_preview_playback(state: &Rc<RefCell<AppState>>, widgets: &AppWidgets) {
    let (frame_path, reached_clip_end) = {
        let mut state = state.borrow_mut();
        let Some(cache) = state.rendered_playback_cache.clone() else {
            state.is_playing = false;
            state.rendered_playback_tick = None;
            return;
        };
        if cache.key != rendered_playback_cache_key(&playback_preload_project(&state.project)) {
            state.is_playing = false;
            state.rendered_playback_tick = None;
            state.rendered_playback_cache = None;
            crate::diagnostics::log_line(format_args!(
                "rendered sequence playback stopped because project changed"
            ));
            return;
        }
        if cache.frames.is_empty() {
            state.is_playing = false;
            state.rendered_playback_tick = None;
            return;
        }

        let now = Instant::now();
        let elapsed = state
            .rendered_playback_tick
            .replace(now)
            .map(|previous| now.saturating_duration_since(previous).as_secs_f64())
            .unwrap_or(cache.frame_duration_seconds);
        let clip_range = state
            .project
            .clips
            .first()
            .map(|clip| clip.range)
            .unwrap_or(TimelineRange {
                start_seconds: cache.clip_start_seconds,
                end_seconds: cache.clip_end_seconds,
            });
        let next = state.playhead_seconds + elapsed;
        let reached_clip_end = next >= clip_range.end_seconds;
        state.playhead_seconds = if reached_clip_end {
            state.is_playing = false;
            state.rendered_playback_tick = None;
            clip_range.start_seconds
        } else {
            next
        };
        let frame_index = ((state.playhead_seconds - cache.clip_start_seconds).max(0.0) * cache.fps)
            .floor() as usize;
        let frame_index = frame_index.min(cache.frames.len().saturating_sub(1));
        (cache.frames[frame_index].clone(), reached_clip_end)
    };

    widgets
        .editor
        .rendered_frame
        .set_file(Some(&gio::File::for_path(frame_path)));
    widgets.editor.rendered_frame.set_visible(true);
    if let Some(crop_overlay) = &widgets.editor.crop_overlay {
        crop_overlay.set_crop(None, false);
    }
    if let Some(caption) = &widgets.editor.caption_overlay {
        caption.set_exact_preview_visible(true);
    }
    if reached_clip_end {
        crate::diagnostics::log_line(format_args!("rendered sequence playback reached clip end"));
    }
    update_timeline_widgets(state, widgets);
}

fn pause_preview_playback(state: &Rc<RefCell<AppState>>, widgets: &AppWidgets) {
    {
        let mut state = state.borrow_mut();
        state.is_playing = false;
        state.rendered_playback_tick = None;
    }
    crate::diagnostics::log_line(format_args!("preview playback pause requested"));
    update_timeline_widgets(state, widgets);
}

fn toggle_preview_playback(state: &Rc<RefCell<AppState>>, widgets: &AppWidgets) {
    if state.borrow().is_playing {
        pause_preview_playback(state, widgets);
    } else {
        start_preview_playback(state, widgets);
    }
}

fn sync_playback_position(state: &Rc<RefCell<AppState>>, widgets: &AppWidgets) {
    if !state.borrow().is_playing {
        return;
    }

    sync_rendered_preview_playback(state, widgets);
}

fn update_playhead(state: &Rc<RefCell<AppState>>, widgets: &AppWidgets, seconds: f64) {
    {
        let mut state = state.borrow_mut();
        state.is_playing = false;
        let duration = project_duration_seconds(&state.project).unwrap_or_else(|| {
            state
                .project
                .clips
                .first()
                .map(|clip| clip.range.end_seconds)
                .unwrap_or(3.0)
        });
        state.playhead_seconds =
            snap_seconds_to_project_frame(&state.project, seconds).clamp(0.0, duration.max(0.01));
    }
    update_timeline_widgets(state, widgets);
}

fn step_playhead_by_frames(state: &Rc<RefCell<AppState>>, widgets: &AppWidgets, frames: i32) {
    let (next_seconds, frame_duration, fps, next_frame) = {
        let state = state.borrow();
        let fps = source_frame_fps(&state.project).unwrap_or_else(|| {
            state
                .project
                .clips
                .first()
                .map(|clip| clip_fps_value(&clip.frame_strategy))
                .unwrap_or(12.0)
        });
        let fps = fps.clamp(1.0, 240.0);
        let frame_duration = 1.0 / fps;
        let duration = project_duration_seconds(&state.project).unwrap_or_else(|| {
            state
                .project
                .clips
                .first()
                .map(|clip| clip.range.end_seconds)
                .unwrap_or(3.0)
        });
        let current_frame = (state.playhead_seconds * fps).round() as i64;
        let max_frame = (duration.max(0.01) * fps).floor().max(0.0) as i64;
        let next_frame = (current_frame + i64::from(frames)).clamp(0, max_frame);
        let next_seconds = (next_frame as f64 / fps).clamp(0.0, duration.max(0.01));
        (next_seconds, frame_duration, fps, next_frame)
    };
    crate::diagnostics::log_line(format_args!(
        "frame step: frames={frames} fps={fps:.6} frame_duration={frame_duration:.6}s next_frame={next_frame} next={next_seconds:.6}s"
    ));
    update_playhead(state, widgets, next_seconds);
}

fn update_clip_start_frame(state: &Rc<RefCell<AppState>>, widgets: &AppWidgets, start_frame: f64) {
    {
        let mut state = state.borrow_mut();
        state.is_playing = false;
        let start_seconds = seconds_for_frame_index(&state.project, start_frame.round() as i64);
        let media_end = project_duration_seconds(&state.project).unwrap_or(3600.0);
        let frame_gap = frame_duration_seconds(&state.project);
        if let Some(clip) = state.project.clips.first_mut() {
            let max_start = (clip.range.end_seconds - frame_gap).max(0.0);
            clip.range.start_seconds = start_seconds.clamp(0.0, max_start.min(media_end));
            let clip_start = clip.range.start_seconds;
            state.playhead_seconds = clip_start;
            clamp_overlays_to_clip(&mut state.project);
            invalidate_exact_preview_output(&mut state);
        }
    }
    update_timeline_widgets(state, widgets);
    defer_rendered_playback_preload(state, widgets, "clip start updated");
}

fn update_clip_end_frame(state: &Rc<RefCell<AppState>>, widgets: &AppWidgets, end_frame: f64) {
    {
        let mut state = state.borrow_mut();
        state.is_playing = false;
        let end_seconds = seconds_for_frame_index(&state.project, end_frame.round() as i64);
        let media_end = project_duration_seconds(&state.project).unwrap_or(3600.0);
        let frame_gap = frame_duration_seconds(&state.project);
        if let Some(clip) = state.project.clips.first_mut() {
            let min_end = clip.range.start_seconds + frame_gap;
            clip.range.end_seconds = end_seconds.clamp(min_end, media_end.max(min_end));
            let clip_range = clip.range;
            state.playhead_seconds = clip_range.end_seconds;
            clamp_overlays_to_clip(&mut state.project);
            invalidate_exact_preview_output(&mut state);
        }
    }
    update_timeline_widgets(state, widgets);
    defer_rendered_playback_preload(state, widgets, "clip end updated");
}

fn update_target_size_mb(state: &Rc<RefCell<AppState>>, widgets: &AppWidgets, _megabytes: f64) {
    {
        let mut state = state.borrow_mut();
        state.project.settings.gif.target_max_bytes = None;
    }
    update_timeline_widgets(state, widgets);
}

fn update_clip_speed(state: &Rc<RefCell<AppState>>, widgets: &AppWidgets, speed: f64) {
    {
        let mut state = state.borrow_mut();
        if let Some(clip) = state.project.clips.first_mut() {
            clip.speed = speed.clamp(0.05, 8.0);
            invalidate_render_outputs(&mut state);
        }
    }
    update_timeline_widgets(state, widgets);
}

fn update_clip_fps(state: &Rc<RefCell<AppState>>, widgets: &AppWidgets, fps: f64) {
    {
        let mut state = state.borrow_mut();
        if let Some(clip) = state.project.clips.first_mut() {
            clip.frame_strategy = FrameStrategy::Fps(fps.round().clamp(1.0, 60.0) as u32);
            invalidate_render_outputs(&mut state);
        }
    }
    update_timeline_widgets(state, widgets);
}

fn update_optimize_gif(state: &Rc<RefCell<AppState>>, widgets: &AppWidgets, enabled: bool) {
    {
        let mut state = state.borrow_mut();
        state.project.settings.gif.optimize = enabled;
    }
    update_timeline_widgets(state, widgets);
}

fn update_high_quality_quantization(
    state: &Rc<RefCell<AppState>>,
    widgets: &AppWidgets,
    enabled: bool,
) {
    {
        let mut state = state.borrow_mut();
        state.project.settings.gif.high_quality_quantization = enabled;
    }
    update_timeline_widgets(state, widgets);
}

fn update_output_width(state: &Rc<RefCell<AppState>>, widgets: &AppWidgets, width: f64) {
    {
        let mut state = state.borrow_mut();
        let width = width.round().max(0.0) as u32;
        if width >= 2 {
            let height = paired_output_height_for_width(&state.project, width);
            state.project.settings.gif.output_width = Some(width);
            state.project.settings.gif.output_height = Some(height);
        } else {
            state.project.settings.gif.output_width = None;
            state.project.settings.gif.output_height = None;
        }
        invalidate_exact_preview_output(&mut state);
    }
    defer_rendered_playback_preload(state, widgets, "resize width updated");
    update_timeline_widgets(state, widgets);
}

fn update_output_height(state: &Rc<RefCell<AppState>>, widgets: &AppWidgets, height: f64) {
    {
        let mut state = state.borrow_mut();
        let height = height.round().max(0.0) as u32;
        if height >= 2 {
            let width = paired_output_width_for_height(&state.project, height);
            state.project.settings.gif.output_width = Some(width);
            state.project.settings.gif.output_height = Some(height);
        } else {
            state.project.settings.gif.output_width = None;
            state.project.settings.gif.output_height = None;
        }
        invalidate_exact_preview_output(&mut state);
    }
    defer_rendered_playback_preload(state, widgets, "resize height updated");
    update_timeline_widgets(state, widgets);
}

#[derive(Debug, Clone, Copy)]
enum CropEdge {
    Left,
    Right,
    Top,
    Bottom,
}

fn update_clip_crop_margin(
    state: &Rc<RefCell<AppState>>,
    widgets: &AppWidgets,
    edge: CropEdge,
    percent: f64,
) {
    {
        let mut state = state.borrow_mut();
        let Some(clip) = state.project.clips.first_mut() else {
            return;
        };
        let mut crop = clip.crop.unwrap_or(CropRect {
            left: 0.0,
            right: 0.0,
            top: 0.0,
            bottom: 0.0,
        });
        let value = (percent / 100.0).clamp(0.0, 0.95);
        match edge {
            CropEdge::Left => crop.left = value,
            CropEdge::Right => crop.right = value,
            CropEdge::Top => crop.top = value,
            CropEdge::Bottom => crop.bottom = value,
        }
        normalize_crop_rect(&mut crop);
        clip.crop =
            if crop.left == 0.0 && crop.right == 0.0 && crop.top == 0.0 && crop.bottom == 0.0 {
                None
            } else {
                Some(crop)
            };
        reflow_output_height_from_width(&mut state.project);
        invalidate_exact_preview_output(&mut state);
    }
    defer_rendered_playback_preload(state, widgets, "crop updated");
    update_timeline_widgets(state, widgets);
}

fn normalize_crop_rect(crop: &mut CropRect) {
    if crop.left + crop.right >= 0.98 {
        let total = (crop.left + crop.right).max(1.0);
        crop.left = crop.left / total * 0.98;
        crop.right = crop.right / total * 0.98;
    }
    if crop.top + crop.bottom >= 0.98 {
        let total = (crop.top + crop.bottom).max(1.0);
        crop.top = crop.top / total * 0.98;
        crop.bottom = crop.bottom / total * 0.98;
    }
}

fn update_clip_range(state: &Rc<RefCell<AppState>>, widgets: &AppWidgets, range: TimelineRange) {
    {
        let mut state = state.borrow_mut();
        state.is_playing = false;
        let media_end = project_duration_seconds(&state.project).unwrap_or(3600.0);
        let frame_gap = frame_duration_seconds(&state.project);
        let start_seconds = snap_seconds_to_project_frame(&state.project, range.start_seconds)
            .clamp(0.0, media_end);
        let min_end = start_seconds + frame_gap;
        let end_seconds = snap_seconds_to_project_frame(&state.project, range.end_seconds)
            .clamp(min_end, media_end.max(min_end));
        if let Some(clip) = state.project.clips.first_mut() {
            let old_start_seconds = clip.range.start_seconds;
            clip.range = TimelineRange {
                start_seconds,
                end_seconds,
            };
            state.playhead_seconds = state.playhead_seconds.clamp(start_seconds, end_seconds);
            if (old_start_seconds - start_seconds).abs() > 0.001 {
                state.playhead_seconds = start_seconds;
            }
            clamp_overlays_to_clip(&mut state.project);
            invalidate_exact_preview_output(&mut state);
        }
    }
    update_timeline_widgets(state, widgets);
    defer_rendered_playback_preload(state, widgets, "clip range updated");
}

fn selected_text_overlay<'a>(
    project: &'a Project,
    selected_id: Option<&str>,
) -> Option<&'a TextOverlay> {
    if let Some(selected_id) = selected_id {
        if let Some(text) = project.overlays.iter().find_map(|overlay| match overlay {
            Overlay::Text(text) if text.id == selected_id => Some(text),
            _ => None,
        }) {
            return Some(text);
        }
    }

    project.overlays.iter().find_map(|overlay| match overlay {
        Overlay::Text(text) => Some(text),
    })
}

fn selected_text_overlay_mut<'a>(
    project: &'a mut Project,
    selected_id: Option<&str>,
) -> Option<&'a mut TextOverlay> {
    if let Some(selected_id) = selected_id {
        if let Some(index) = project.overlays.iter().position(|overlay| match overlay {
            Overlay::Text(text) => text.id == selected_id,
        }) {
            return match &mut project.overlays[index] {
                Overlay::Text(text) => Some(text),
            };
        }
    }

    project
        .overlays
        .iter_mut()
        .find_map(|overlay| match overlay {
            Overlay::Text(text) => Some(text),
        })
}

fn text_overlays_from_project(project: &Project) -> Vec<TextOverlay> {
    project
        .overlays
        .iter()
        .map(|overlay| match overlay {
            Overlay::Text(text) => text.clone(),
        })
        .collect()
}

fn overlay_visible_at_playhead(text: &TextOverlay, playhead_seconds: f64) -> bool {
    playhead_seconds + 0.0005 >= text.range.start_seconds
        && playhead_seconds <= text.range.end_seconds + 0.0005
}

fn next_text_overlay_defaults(project: &Project, selected_id: Option<&str>) -> TextOverlay {
    if let Some(selected_id) = selected_id {
        if let Some(text) = project.overlays.iter().find_map(|overlay| match overlay {
            Overlay::Text(text) if text.id == selected_id => Some(text.clone()),
            _ => None,
        }) {
            return text;
        }
    }

    if let Some(text) = project
        .overlays
        .iter()
        .rev()
        .find_map(|overlay| match overlay {
            Overlay::Text(text) => Some(text.clone()),
        })
    {
        return text;
    }

    let mut text = TextOverlay::default_caption();
    if let Some(font_family) = load_last_font_family() {
        text.font_family = font_family;
    }
    text
}

fn initial_overlay_font_size(project: &Project, requested_size: f64) -> f64 {
    let reference_height = preview_text_reference_height(project)
        .or_else(|| {
            project
                .source
                .as_ref()
                .and_then(|source| source.natural_height)
        })
        .unwrap_or(540);
    let fit_size = (f64::from(reference_height) * 0.13).clamp(12.0, 42.0);
    requested_size.clamp(6.0, fit_size)
}

fn last_font_family_path() -> Option<PathBuf> {
    if let Ok(config_home) = std::env::var("XDG_CONFIG_HOME") {
        let path = PathBuf::from(config_home);
        if !path.as_os_str().is_empty() {
            return Some(path.join("gifbrewery-gtk").join("last-font-family"));
        }
    }

    std::env::var("HOME").ok().and_then(|home| {
        let path = PathBuf::from(home);
        (!path.as_os_str().is_empty()).then(|| {
            path.join(".config")
                .join("gifbrewery-gtk")
                .join("last-font-family")
        })
    })
}

fn load_last_font_family() -> Option<String> {
    let path = last_font_family_path()?;
    let family = fs::read_to_string(path).ok()?;
    let family = family.trim();
    (!family.is_empty()).then(|| family.to_string())
}

fn save_last_font_family(family: &str) {
    let Some(path) = last_font_family_path() else {
        return;
    };
    if let Some(parent) = path.parent() {
        if let Err(err) = fs::create_dir_all(parent) {
            crate::diagnostics::log_line(format_args!(
                "failed to create font preference directory {}: {err}",
                parent.display()
            ));
            return;
        }
    }
    if let Err(err) = fs::write(&path, family.trim()) {
        crate::diagnostics::log_line(format_args!(
            "failed to save last font family {}: {err}",
            path.display()
        ));
    }
}

fn overlay_labels(project: &Project) -> Vec<String> {
    project
        .overlays
        .iter()
        .enumerate()
        .map(|(index, overlay)| match overlay {
            Overlay::Text(text) => {
                let label = text.text.trim();
                if label.is_empty() {
                    format!("Text {}", index + 1)
                } else {
                    format!("Text {}: {}", index + 1, label)
                }
            }
        })
        .collect()
}

fn selected_overlay_index(project: &Project, selected_id: Option<&str>) -> usize {
    selected_id
        .and_then(|selected_id| {
            project.overlays.iter().position(|overlay| match overlay {
                Overlay::Text(text) => text.id == selected_id,
            })
        })
        .unwrap_or(0)
}

fn select_overlay_by_index(
    state: &Rc<RefCell<AppState>>,
    widgets: &AppWidgets,
    selected_index: usize,
) {
    {
        let mut state = state.borrow_mut();
        state.selected_overlay_id =
            state
                .project
                .overlays
                .get(selected_index)
                .map(|overlay| match overlay {
                    Overlay::Text(text) => text.id.clone(),
                });
        crate::diagnostics::log_line(format_args!(
            "selected overlay changed: index={selected_index} id={:?}",
            state.selected_overlay_id
        ));
    }
    update_timeline_widgets(state, widgets);
}

fn add_text_overlay_at_playhead(state: &Rc<RefCell<AppState>>, widgets: &AppWidgets) {
    {
        let mut state = state.borrow_mut();
        let clip_range = state
            .project
            .clips
            .first()
            .map(|clip| clip.range)
            .unwrap_or(TimelineRange {
                start_seconds: 0.0,
                end_seconds: 3.0,
            });
        let id = format!("caption-{}", state.project.overlays.len() + 1);
        let start_seconds = state.playhead_seconds.clamp(
            clip_range.start_seconds,
            (clip_range.end_seconds - 0.01).max(0.0),
        );
        let mut text =
            next_text_overlay_defaults(&state.project, state.selected_overlay_id.as_deref());
        text.id = id.clone();
        text.text = format!("Text {}", state.project.overlays.len() + 1);
        text.font_size = initial_overlay_font_size(&state.project, text.font_size);
        text.shadow_enabled = false;
        let overlay_offset = (state.project.overlays.len() % 4) as f64;
        text.bounds.width = 0.92;
        text.bounds.x = (0.04 + overlay_offset * 0.025).min(0.96 - text.bounds.width);
        text.bounds.y = (0.72 - overlay_offset * 0.08).max(0.12);
        text.range = TimelineRange {
            start_seconds,
            end_seconds: (start_seconds + 1.0).min(clip_range.end_seconds),
        };
        if text.range.end_seconds <= text.range.start_seconds {
            text.range.end_seconds = text.range.start_seconds + 0.01;
        }
        state.project.overlays.push(Overlay::Text(text));
        state.selected_overlay_id = Some(id.clone());
        invalidate_overlay_output(&mut state);
        crate::diagnostics::log_line(format_args!(
            "added text overlay at playhead: id={id} start={start_seconds:.3}"
        ));
    }
    update_timeline_widgets(state, widgets);
}

fn delete_selected_overlay(state: &Rc<RefCell<AppState>>, widgets: &AppWidgets) {
    {
        let mut state = state.borrow_mut();
        if state.project.overlays.is_empty() {
            return;
        }
        let selected_id = state.selected_overlay_id.clone();
        let index = selected_overlay_index(&state.project, selected_id.as_deref());
        let removed = state.project.overlays.remove(index);
        state.selected_overlay_id = state.project.overlays.first().map(|overlay| match overlay {
            Overlay::Text(text) => text.id.clone(),
        });
        invalidate_overlay_output(&mut state);
        crate::diagnostics::log_line(format_args!(
            "deleted text overlay: removed={removed:?} selected={:?}",
            state.selected_overlay_id
        ));
    }
    update_timeline_widgets(state, widgets);
}

fn update_overlay_font_family(state: &Rc<RefCell<AppState>>, widgets: &AppWidgets, family: &str) {
    {
        let mut state = state.borrow_mut();
        let selected_id = state.selected_overlay_id.clone();
        if let Some(overlay) = selected_text_overlay_mut(&mut state.project, selected_id.as_deref())
        {
            let family = family.trim();
            if !family.is_empty() {
                overlay.font_family = family.to_string();
                save_last_font_family(family);
                crate::diagnostics::log_line(format_args!(
                    "updated text overlay font family: id={} family={family}",
                    overlay.id
                ));
                invalidate_overlay_output(&mut state);
            }
        }
    }
    update_timeline_widgets(state, widgets);
}

fn update_overlay_text_color(
    state: &Rc<RefCell<AppState>>,
    widgets: &AppWidgets,
    color: RgbaColor,
) {
    {
        let mut state = state.borrow_mut();
        let selected_id = state.selected_overlay_id.clone();
        if let Some(overlay) = selected_text_overlay_mut(&mut state.project, selected_id.as_deref())
        {
            overlay.text_color = color;
            invalidate_overlay_output(&mut state);
        }
    }
    update_timeline_widgets(state, widgets);
}

fn update_overlay_stroke_color(
    state: &Rc<RefCell<AppState>>,
    widgets: &AppWidgets,
    color: RgbaColor,
) {
    {
        let mut state = state.borrow_mut();
        let selected_id = state.selected_overlay_id.clone();
        if let Some(overlay) = selected_text_overlay_mut(&mut state.project, selected_id.as_deref())
        {
            overlay.stroke_color = color;
            invalidate_overlay_output(&mut state);
        }
    }
    update_timeline_widgets(state, widgets);
}

fn update_overlay_start(state: &Rc<RefCell<AppState>>, widgets: &AppWidgets, start_seconds: f64) {
    {
        let mut state = state.borrow_mut();
        let Some(clip_range) = state.project.clips.first().map(|clip| clip.range) else {
            return;
        };
        let selected_id = state.selected_overlay_id.clone();
        if let Some(text) = selected_text_overlay_mut(&mut state.project, selected_id.as_deref()) {
            let max_start = (text.range.end_seconds - 0.01).max(clip_range.start_seconds);
            text.range.start_seconds = start_seconds.clamp(clip_range.start_seconds, max_start);
            invalidate_overlay_output(&mut state);
        }
    }
    update_timeline_widgets(state, widgets);
}

fn mark_overlay_start_at_playhead(state: &Rc<RefCell<AppState>>, widgets: &AppWidgets) {
    {
        let mut state = state.borrow_mut();
        let Some(clip_range) = state.project.clips.first().map(|clip| clip.range) else {
            return;
        };
        let playhead_seconds = state.playhead_seconds;
        let selected_id = state.selected_overlay_id.clone();
        if let Some(text) = selected_text_overlay_mut(&mut state.project, selected_id.as_deref()) {
            let max_start = (clip_range.end_seconds - 0.01).max(clip_range.start_seconds);
            let start_seconds = playhead_seconds.clamp(clip_range.start_seconds, max_start);
            text.range.start_seconds = start_seconds;
            if text.range.end_seconds <= start_seconds {
                text.range.end_seconds = (start_seconds + 0.01).min(clip_range.end_seconds);
            }
            let range = text.range;
            invalidate_overlay_output(&mut state);
            crate::diagnostics::log_line(format_args!(
                "overlay mark appears at playhead: playhead={playhead_seconds:.3} range={:.3}-{:.3}",
                range.start_seconds, range.end_seconds
            ));
        }
    }
    update_timeline_widgets(state, widgets);
}

fn mark_overlay_end_at_playhead(state: &Rc<RefCell<AppState>>, widgets: &AppWidgets) {
    {
        let mut state = state.borrow_mut();
        let Some(clip_range) = state.project.clips.first().map(|clip| clip.range) else {
            return;
        };
        let playhead_seconds = state.playhead_seconds;
        let selected_id = state.selected_overlay_id.clone();
        if let Some(text) = selected_text_overlay_mut(&mut state.project, selected_id.as_deref()) {
            let min_end = text.range.start_seconds + 0.01;
            text.range.end_seconds = playhead_seconds.clamp(min_end, clip_range.end_seconds);
            let range = text.range;
            invalidate_overlay_output(&mut state);
            crate::diagnostics::log_line(format_args!(
                "overlay mark disappears at playhead: playhead={playhead_seconds:.3} range={:.3}-{:.3}",
                range.start_seconds, range.end_seconds
            ));
        }
    }
    update_timeline_widgets(state, widgets);
}

fn update_overlay_range(
    state: &Rc<RefCell<AppState>>,
    widgets: &AppWidgets,
    id: &str,
    range: TimelineRange,
) {
    {
        let mut state = state.borrow_mut();
        state.selected_overlay_id = Some(id.to_string());
        let Some(clip_range) = state.project.clips.first().map(|clip| clip.range) else {
            return;
        };
        let mut changed = false;
        for overlay in &mut state.project.overlays {
            match overlay {
                Overlay::Text(text) if text.id == id => {
                    let start_seconds = range
                        .start_seconds
                        .clamp(clip_range.start_seconds, clip_range.end_seconds - 0.01);
                    let end_seconds = range
                        .end_seconds
                        .clamp(start_seconds + 0.01, clip_range.end_seconds);
                    text.range = TimelineRange {
                        start_seconds,
                        end_seconds,
                    };
                    changed = true;
                }
                _ => {}
            }
        }
        if changed {
            invalidate_overlay_output(&mut state);
        }
    }
    update_timeline_widgets(state, widgets);
}

fn update_overlay_text(state: &Rc<RefCell<AppState>>, widgets: &AppWidgets, text: &str) {
    {
        let mut state = state.borrow_mut();
        let selected_id = state.selected_overlay_id.clone();
        if let Some(overlay) = selected_text_overlay_mut(&mut state.project, selected_id.as_deref())
        {
            overlay.text = text.to_string();
            invalidate_overlay_output(&mut state);
        }
    }
    update_timeline_widgets(state, widgets);
}

fn update_overlay_font_size(state: &Rc<RefCell<AppState>>, widgets: &AppWidgets, font_size: f64) {
    {
        let mut state = state.borrow_mut();
        let selected_id = state.selected_overlay_id.clone();
        if let Some(overlay) = selected_text_overlay_mut(&mut state.project, selected_id.as_deref())
        {
            overlay.font_size = font_size.clamp(6.0, 240.0);
            invalidate_overlay_output(&mut state);
        }
    }
    update_timeline_widgets(state, widgets);
}

fn update_overlay_bold(state: &Rc<RefCell<AppState>>, widgets: &AppWidgets, bold: bool) {
    {
        let mut state = state.borrow_mut();
        let selected_id = state.selected_overlay_id.clone();
        if let Some(overlay) = selected_text_overlay_mut(&mut state.project, selected_id.as_deref())
        {
            overlay.font_weight = if bold { 700 } else { 400 };
            invalidate_overlay_output(&mut state);
        }
    }
    update_timeline_widgets(state, widgets);
}

fn update_overlay_alignment(
    state: &Rc<RefCell<AppState>>,
    widgets: &AppWidgets,
    alignment: TextAlignment,
) {
    {
        let mut state = state.borrow_mut();
        let selected_id = state.selected_overlay_id.clone();
        if let Some(overlay) = selected_text_overlay_mut(&mut state.project, selected_id.as_deref())
        {
            overlay.alignment = alignment;
            invalidate_overlay_output(&mut state);
        }
    }
    update_timeline_widgets(state, widgets);
}

fn update_overlay_stroke_width(
    state: &Rc<RefCell<AppState>>,
    widgets: &AppWidgets,
    stroke_width: f64,
) {
    {
        let mut state = state.borrow_mut();
        let selected_id = state.selected_overlay_id.clone();
        if let Some(overlay) = selected_text_overlay_mut(&mut state.project, selected_id.as_deref())
        {
            overlay.stroke_width = stroke_width.clamp(0.0, 20.0);
            invalidate_overlay_output(&mut state);
        }
    }
    update_timeline_widgets(state, widgets);
}

fn update_overlay_position_from_drag(
    state: &Rc<RefCell<AppState>>,
    widgets: &AppWidgets,
    start: CaptionDragStart,
    offset_x: f64,
    offset_y: f64,
) {
    let preview_width = f64::from(widgets.editor.preview.width()).max(1.0);
    let preview_height = f64::from(widgets.editor.preview.height()).max(1.0);
    let content_rect = {
        let state = state.borrow();
        if uses_rendered_output_preview(&state.project) {
            contained_rect(
                preview_width,
                preview_height,
                rendered_preview_aspect(&state.project),
            )
        } else {
            PixelBounds {
                x: 0.0,
                y: 0.0,
                width: preview_width,
                height: preview_height,
            }
        }
    };
    let content_width = content_rect.width.max(1.0);
    let content_height = content_rect.height.max(1.0);
    let (min_x, max_x, min_y, max_y) = if let Some(bounds) = start.pixel_bounds {
        let ink_left = (bounds.x - content_rect.x) / content_width;
        let ink_right = (bounds.x + bounds.width - content_rect.x) / content_width;
        let ink_top = (bounds.y - content_rect.y) / content_height;
        let ink_bottom = (bounds.y + bounds.height - content_rect.y) / content_height;
        (
            start.model_bounds.x - ink_left,
            1.0 - (ink_right - start.model_bounds.x),
            start.model_bounds.y - ink_top,
            1.0 - (ink_bottom - start.model_bounds.y),
        )
    } else {
        (
            0.0,
            (1.0 - start.model_bounds.width).max(0.0),
            0.0,
            (1.0 - start.model_bounds.height).max(0.0),
        )
    };

    {
        let mut state = state.borrow_mut();
        let selected_id = state.selected_overlay_id.clone();
        if let Some(overlay) = selected_text_overlay_mut(&mut state.project, selected_id.as_deref())
        {
            overlay.bounds.x =
                (start.model_bounds.x + offset_x / content_width).clamp(min_x, max_x.max(min_x));
            overlay.bounds.y =
                (start.model_bounds.y + offset_y / content_height).clamp(min_y, max_y.max(min_y));
            let bounds = overlay.bounds;
            invalidate_overlay_output(&mut state);
            crate::diagnostics::log_line(format_args!(
                "caption drag computed: preview=({preview_width:.1}x{preview_height:.1}) content={content_rect:?} start={start:?} clamp=({min_x:.4},{max_x:.4},{min_y:.4},{max_y:.4}) new_bounds={:?}",
                bounds
            ));
        }
    }
    update_timeline_widgets(state, widgets);
}

fn update_overlay_shadow(state: &Rc<RefCell<AppState>>, widgets: &AppWidgets, enabled: bool) {
    {
        let mut state = state.borrow_mut();
        let selected_id = state.selected_overlay_id.clone();
        if let Some(overlay) = selected_text_overlay_mut(&mut state.project, selected_id.as_deref())
        {
            overlay.shadow_enabled = enabled;
            invalidate_overlay_output(&mut state);
        }
    }
    update_timeline_widgets(state, widgets);
}

fn update_overlay_end(state: &Rc<RefCell<AppState>>, widgets: &AppWidgets, end_seconds: f64) {
    {
        let mut state = state.borrow_mut();
        let Some(clip_range) = state.project.clips.first().map(|clip| clip.range) else {
            return;
        };
        let selected_id = state.selected_overlay_id.clone();
        if let Some(text) = selected_text_overlay_mut(&mut state.project, selected_id.as_deref()) {
            let min_end = text.range.start_seconds + 0.01;
            text.range.end_seconds = end_seconds.clamp(min_end, clip_range.end_seconds);
            invalidate_overlay_output(&mut state);
        }
    }
    update_timeline_widgets(state, widgets);
}

fn project_duration_seconds(project: &Project) -> Option<f64> {
    project.source.as_ref()?.duration_seconds
}

fn source_frame_fps(project: &Project) -> Option<f64> {
    project
        .source
        .as_ref()
        .and_then(|source| source.fps)
        .filter(|fps| *fps > 0.0)
}

fn clamp_overlays_to_clip(project: &mut Project) {
    let Some(clip_range) = project.clips.first().map(|clip| clip.range) else {
        return;
    };

    for overlay in &mut project.overlays {
        match overlay {
            Overlay::Text(text) => {
                text.range = normalized_range_for_clip(text.range, clip_range);
            }
        }
    }
}

fn normalized_range_for_clip(range: TimelineRange, clip_range: TimelineRange) -> TimelineRange {
    let clip_start = clip_range.start_seconds.min(clip_range.end_seconds);
    let clip_end = clip_range.end_seconds.max(clip_start + 0.01);
    let max_start = (clip_end - 0.01).max(clip_start);
    let start_seconds = range.start_seconds.clamp(clip_start, max_start);
    let end_seconds = range
        .end_seconds
        .clamp(start_seconds + 0.01, clip_end.max(start_seconds + 0.01));

    TimelineRange {
        start_seconds,
        end_seconds,
    }
}

fn update_timeline_widgets(state: &Rc<RefCell<AppState>>, widgets: &AppWidgets) {
    let (
        clip_range,
        overlay_range,
        text_overlay,
        text_overlays,
        selected_overlay_id,
        selected_overlay_index,
        overlay_labels,
        playhead_seconds,
        timeline_state,
        clip_start_frame,
        clip_end_frame,
        max_frame,
    ) = {
        let state = state.borrow();
        let clip = state
            .project
            .clips
            .first()
            .expect("default project has a clip");
        let selected_overlay_id = state.selected_overlay_id.clone();
        let text_overlay =
            selected_text_overlay(&state.project, selected_overlay_id.as_deref()).cloned();
        let overlay_range = text_overlay.as_ref().map(|text| text.range);
        let selected_overlay_index =
            selected_overlay_index(&state.project, selected_overlay_id.as_deref());
        let clip_start_frame = frame_index_for_seconds(&state.project, clip.range.start_seconds);
        let clip_end_frame = frame_index_for_seconds(&state.project, clip.range.end_seconds);
        let max_frame = max_media_frame_index(&state.project);
        (
            clip.range,
            overlay_range,
            text_overlay,
            text_overlays_from_project(&state.project),
            selected_overlay_id.clone(),
            selected_overlay_index,
            overlay_labels(&state.project),
            state.playhead_seconds,
            timeline_state_from_project(
                &state.project,
                selected_overlay_id.as_deref(),
                state.playhead_seconds,
                state.thumbnails.clone(),
            ),
            clip_start_frame,
            clip_end_frame,
            max_frame,
        )
    };

    state.borrow_mut().syncing_widgets = true;

    widgets.editor.time_label.set_label(&format!(
        "Frames {}-{} ({:.2}s - {:.2}s)",
        clip_start_frame, clip_end_frame, clip_range.start_seconds, clip_range.end_seconds
    ));
    widgets.editor.timeline_view.set_state(timeline_state);
    widgets.inspector.target_size_mb.set_value(
        state
            .borrow()
            .project
            .settings
            .gif
            .target_max_bytes
            .map(bytes_to_megabytes)
            .unwrap_or(16.0),
    );
    widgets
        .inspector
        .clip_start
        .adjustment()
        .set_upper(max_frame as f64);
    widgets
        .inspector
        .clip_end
        .adjustment()
        .set_upper((max_frame as f64).max(1.0));
    widgets
        .inspector
        .clip_start
        .set_value(clip_start_frame as f64);
    widgets.inspector.clip_end.set_value(clip_end_frame as f64);
    widgets.inspector.clip_speed.set_value(
        state
            .borrow()
            .project
            .clips
            .first()
            .map(|clip| clip.speed)
            .unwrap_or(1.0),
    );
    widgets.inspector.clip_fps.set_value(
        state
            .borrow()
            .project
            .source
            .as_ref()
            .and_then(|source| source.fps)
            .unwrap_or(0.0),
    );
    widgets
        .inspector
        .optimize_gif
        .set_active(state.borrow().project.settings.gif.optimize);
    widgets.inspector.high_quality_quantization.set_active(
        state
            .borrow()
            .project
            .settings
            .gif
            .high_quality_quantization,
    );
    let output_dimensions = effective_output_dimensions(&state.borrow().project);
    widgets.inspector.output_width.set_value(
        output_dimensions
            .map(|(width, _)| f64::from(width))
            .unwrap_or(0.0),
    );
    widgets.inspector.output_height.set_value(
        output_dimensions
            .map(|(_, height)| f64::from(height))
            .unwrap_or(0.0),
    );
    let clip_crop = state
        .borrow()
        .project
        .clips
        .first()
        .and_then(|clip| clip.crop)
        .unwrap_or(CropRect {
            left: 0.0,
            right: 0.0,
            top: 0.0,
            bottom: 0.0,
        });
    widgets
        .inspector
        .crop_left
        .set_value(clip_crop.left * 100.0);
    widgets
        .inspector
        .crop_right
        .set_value(clip_crop.right * 100.0);
    widgets.inspector.crop_top.set_value(clip_crop.top * 100.0);
    widgets
        .inspector
        .crop_bottom
        .set_value(clip_crop.bottom * 100.0);
    if let Some(crop_overlay) = &widgets.editor.crop_overlay {
        let (crop, has_source, rendered_output_preview) = {
            let state = state.borrow();
            (
                state.project.clips.first().and_then(|clip| clip.crop),
                state.project.source.is_some(),
                uses_rendered_output_preview(&state.project),
            )
        };
        if rendered_output_preview {
            crop_overlay.set_crop(None, false);
        } else {
            crop_overlay.set_crop(crop, has_source);
        }
    }

    if let Some(list) = &widgets.inspector.overlay_list {
        populate_overlay_list(list, &overlay_labels, selected_overlay_index);
    }
    if let Some(button) = &widgets.inspector.overlay_add {
        button.set_sensitive(state.borrow().project.source.is_some());
    }
    if let Some(button) = &widgets.inspector.overlay_delete {
        button.set_sensitive(text_overlay.is_some());
    }

    let has_text_overlay = text_overlay.is_some();
    if let Some(row) = &widgets.inspector.overlay_text {
        row.set_sensitive(has_text_overlay);
    }
    if let Some(row) = &widgets.inspector.overlay_font_row {
        row.set_sensitive(has_text_overlay);
    }
    if let Some(button) = &widgets.inspector.overlay_font {
        button.set_sensitive(has_text_overlay);
    }
    if let Some(button) = &widgets.inspector.overlay_font_refresh {
        button.set_sensitive(has_text_overlay);
    }
    if let Some(button) = &widgets.inspector.overlay_text_color {
        button.set_sensitive(has_text_overlay);
    }
    if let Some(button) = &widgets.inspector.overlay_stroke_color {
        button.set_sensitive(has_text_overlay);
    }
    if let Some(row) = &widgets.inspector.overlay_start {
        row.set_sensitive(has_text_overlay);
        row.set_value(overlay_range.map_or(0.0, |range| range.start_seconds));
    }
    if let Some(button) = &widgets.inspector.overlay_mark_start {
        button.set_sensitive(has_text_overlay);
    }
    if let Some(row) = &widgets.inspector.overlay_end {
        row.set_sensitive(has_text_overlay);
        row.set_value(overlay_range.map_or(0.01, |range| range.end_seconds));
    }
    if let Some(button) = &widgets.inspector.overlay_mark_end {
        button.set_sensitive(has_text_overlay);
    }
    if let Some(row) = &widgets.inspector.overlay_font_size {
        row.set_sensitive(has_text_overlay);
    }
    if let Some(row) = &widgets.inspector.overlay_bold {
        row.set_sensitive(has_text_overlay);
    }
    if let Some(button) = &widgets.inspector.overlay_alignment {
        button.set_sensitive(has_text_overlay);
    }
    if let Some(row) = &widgets.inspector.overlay_stroke_width {
        row.set_sensitive(has_text_overlay);
    }
    if let Some(row) = &widgets.inspector.overlay_shadow {
        row.set_sensitive(has_text_overlay);
    }

    if let Some(text) = text_overlay {
        if let Some(row) = &widgets.inspector.overlay_text {
            let buffer = row.buffer();
            if !row.has_focus() && text_buffer_string(&buffer) != text.text {
                buffer.set_text(&text.text);
            }
        }
        if let Some(row) = &widgets.inspector.overlay_font {
            row.set_label("Choose");
        }
        if let Some(row) = &widgets.inspector.overlay_font_row {
            row.set_subtitle(&text.font_family);
        }
        if let Some(button) = &widgets.inspector.overlay_text_color {
            button.set_rgba(&rgba_to_gdk(text.text_color));
        }
        if let Some(button) = &widgets.inspector.overlay_stroke_color {
            button.set_rgba(&rgba_to_gdk(text.stroke_color));
        }
        if let Some(row) = &widgets.inspector.overlay_font_size {
            row.set_value(text.font_size);
        }
        if let Some(row) = &widgets.inspector.overlay_bold {
            row.set_active(text.font_weight >= 600);
        }
        if let Some(button) = &widgets.inspector.overlay_alignment {
            button.set_active(text.alignment == TextAlignment::Center);
        }
        if let Some(row) = &widgets.inspector.overlay_stroke_width {
            row.set_value(text.stroke_width);
        }
        if let Some(row) = &widgets.inspector.overlay_shadow {
            row.set_active(text.shadow_enabled);
        }
    } else {
        if let Some(row) = &widgets.inspector.overlay_text {
            let buffer = row.buffer();
            if !row.has_focus() && !text_buffer_string(&buffer).is_empty() {
                buffer.set_text("");
            }
        }
        if let Some(row) = &widgets.inspector.overlay_font {
            row.set_label("Choose");
        }
        if let Some(row) = &widgets.inspector.overlay_font_row {
            row.set_subtitle("");
        }
        if let Some(row) = &widgets.inspector.overlay_font_size {
            row.set_value(32.0);
        }
        if let Some(row) = &widgets.inspector.overlay_bold {
            row.set_active(false);
        }
        if let Some(button) = &widgets.inspector.overlay_alignment {
            button.set_active(false);
        }
        if let Some(row) = &widgets.inspector.overlay_stroke_width {
            row.set_value(1.0);
        }
        if let Some(row) = &widgets.inspector.overlay_shadow {
            row.set_active(false);
        }
    }

    if let Some(caption) = &widgets.editor.caption_overlay {
        let reference_height = preview_text_reference_height(&state.borrow().project);
        caption.set_source_height(reference_height);
        caption.set_exact_preview_aspect(rendered_preview_aspect(&state.borrow().project));
        caption.set_texts_for_playhead(text_overlays, selected_overlay_id, playhead_seconds);
    }

    state.borrow_mut().syncing_widgets = false;
    refresh_exact_preview_frame(state, widgets);
}

fn refresh_exact_preview_frame(state: &Rc<RefCell<AppState>>, widgets: &AppWidgets) {
    let (project, playhead_seconds, generation, should_render) = {
        let mut state = state.borrow_mut();
        let uses_rendered_preview = uses_rendered_output_preview(&state.project);
        if state.is_playing && uses_rendered_preview {
            if let Some(crop_overlay) = &widgets.editor.crop_overlay {
                crop_overlay.set_crop(None, false);
            }
            if let Some(caption) = &widgets.editor.caption_overlay {
                caption.set_exact_preview_visible(true);
            }
            return;
        }
        if state.project.source.is_none() || (state.is_playing && !uses_rendered_preview) {
            state.preview_render_generation = state.preview_render_generation.wrapping_add(1);
            state.last_preview_render_key = None;
            state.preview_render_pending = false;
            widgets.editor.rendered_frame.set_visible(false);
            if let Some(caption) = &widgets.editor.caption_overlay {
                caption.set_exact_preview_visible(false);
                if state.is_playing {
                    caption.area.set_visible(false);
                }
            }
            return;
        }

        let key = preview_render_key(&state.project, state.playhead_seconds);
        if state.last_preview_render_key.as_deref() == Some(key.as_str()) {
            return;
        }
        if state.preview_render_pending {
            state.preview_render_rebuild_requested = true;
            return;
        }
        state.last_preview_render_key = Some(key);
        state.preview_render_generation = state.preview_render_generation.wrapping_add(1);
        state.preview_render_pending = true;
        state.preview_render_rebuild_requested = false;
        if let Some(crop_overlay) = &widgets.editor.crop_overlay {
            crop_overlay.set_crop(None, false);
        }
        (
            base_preview_project(&state.project),
            state.playhead_seconds,
            state.preview_render_generation,
            true,
        )
    };

    if !should_render {
        return;
    }

    let key = preview_render_key(&project, playhead_seconds);
    let output_path = rendered_preview_cache_path(&key);
    if output_path.exists() {
        let mut state = state.borrow_mut();
        state.preview_render_pending = false;
        state.preview_render_rebuild_requested = false;
        widgets
            .editor
            .rendered_frame
            .set_file(Some(&gio::File::for_path(&output_path)));
        widgets.editor.rendered_frame.set_visible(true);
        if let Some(crop_overlay) = &widgets.editor.crop_overlay {
            crop_overlay.set_crop(None, false);
        }
        if let Some(caption) = &widgets.editor.caption_overlay {
            caption.set_exact_preview_visible(true);
        }
        return;
    }

    let (sender, receiver) = mpsc::channel::<(u64, PathBuf, Result<(), String>)>();
    thread::spawn({
        let output_path = output_path.clone();
        move || {
            let result = crate::export::render_frame_png(&project, playhead_seconds, &output_path);
            let _ = sender.send((generation, output_path, result));
        }
    });

    let receiver = Rc::new(RefCell::new(receiver));
    let state = Rc::clone(state);
    let widgets = widgets.clone();
    glib::timeout_add_local(std::time::Duration::from_millis(80), move || {
        let Ok((generation, path, result)) = receiver.borrow_mut().try_recv() else {
            return glib::ControlFlow::Continue;
        };

        if state.borrow().preview_render_generation != generation {
            let should_restart = state.borrow().preview_render_rebuild_requested;
            {
                let mut state = state.borrow_mut();
                state.preview_render_pending = false;
                state.preview_render_rebuild_requested = false;
            }
            let _ = std::fs::remove_file(path);
            if should_restart {
                refresh_exact_preview_frame(&state, &widgets);
            }
            return glib::ControlFlow::Break;
        }

        {
            let mut state = state.borrow_mut();
            state.preview_render_pending = false;
            state.preview_render_rebuild_requested = false;
        }
        match result {
            Ok(()) => {
                widgets
                    .editor
                    .rendered_frame
                    .set_file(Some(&gio::File::for_path(&path)));
                widgets.editor.rendered_frame.set_visible(true);
                if let Some(crop_overlay) = &widgets.editor.crop_overlay {
                    crop_overlay.set_crop(None, false);
                }
                if let Some(caption) = &widgets.editor.caption_overlay {
                    caption.set_exact_preview_visible(true);
                }
            }
            Err(err) => {
                crate::diagnostics::log_line(format_args!(
                    "exact preview render failed at {playhead_seconds:.3}s: {err}"
                ));
                widgets.editor.rendered_frame.set_visible(false);
                if let Some(crop_overlay) = &widgets.editor.crop_overlay {
                    let state = state.borrow();
                    let crop = state.project.clips.first().and_then(|clip| clip.crop);
                    crop_overlay.set_crop(crop, state.project.source.is_some());
                }
                if let Some(caption) = &widgets.editor.caption_overlay {
                    caption.set_exact_preview_visible(false);
                }
            }
        }

        glib::ControlFlow::Break
    });
}

fn rendered_preview_cache_path(key: &str) -> PathBuf {
    let mut hasher = DefaultHasher::new();
    key.hash(&mut hasher);
    let dir = std::env::temp_dir().join(format!(
        "gifbrewery-rendered-preview-{}",
        std::process::id()
    ));
    let _ = std::fs::create_dir_all(&dir);
    dir.join(format!("{:016x}.png", hasher.finish()))
}

fn invalidate_render_outputs(state: &mut AppState) {
    invalidate_exact_preview_output(state);
    state.rendered_playback_cache = None;
    state.rendered_playback_generation = state.rendered_playback_generation.wrapping_add(1);
    state.rendered_playback_preload_deferred = false;
    state.rendered_playback_rebuild_requested = true;
    if !state.rendered_playback_preparing {
        state.rendered_playback_rebuild_requested = false;
    }
    state.rendered_playback_tick = None;
}

fn defer_rendered_playback_preload(
    state: &Rc<RefCell<AppState>>,
    widgets: &AppWidgets,
    reason: &'static str,
) {
    let token = {
        let mut state = state.borrow_mut();
        state.rendered_playback_cache = None;
        state.rendered_playback_generation = state.rendered_playback_generation.wrapping_add(1);
        state.rendered_playback_preload_deferred = true;
        state.rendered_playback_rebuild_requested = false;
        state.rendered_playback_tick = None;
        state.rendered_playback_preload_debounce =
            state.rendered_playback_preload_debounce.wrapping_add(1);
        state.rendered_playback_preload_debounce
    };

    let state = Rc::clone(state);
    let widgets = widgets.clone();
    glib::timeout_add_local(
        std::time::Duration::from_millis(RENDERED_PLAYBACK_RESCALE_DEBOUNCE_MS),
        move || {
            let should_start = {
                let mut state = state.borrow_mut();
                if state.rendered_playback_preload_debounce != token {
                    return glib::ControlFlow::Break;
                }
                state.rendered_playback_preload_deferred = false;
                state.project.source.is_some()
            };
            if should_start {
                start_rendered_playback_preload(&state, &widgets, reason);
            }
            glib::ControlFlow::Break
        },
    );
}

fn invalidate_exact_preview_output(state: &mut AppState) {
    state.preview_render_generation = state.preview_render_generation.wrapping_add(1);
    state.last_preview_render_key = None;
    if state.preview_render_pending {
        state.preview_render_rebuild_requested = true;
    } else {
        state.preview_render_rebuild_requested = false;
    }
}

fn invalidate_overlay_output(_state: &mut AppState) {
    // Captions are live GTK overlays. Touching, moving, adding, or styling them
    // must not invalidate ffmpeg-rendered media frames.
}

fn rendered_playback_sequence_dir(key: &str) -> PathBuf {
    let mut hasher = DefaultHasher::new();
    key.hash(&mut hasher);
    std::env::temp_dir()
        .join(format!(
            "gifbrewery-rendered-playback-{}",
            std::process::id()
        ))
        .join(format!("{:016x}", hasher.finish()))
}

fn cleanup_stale_preview_cache_dirs() {
    let Ok(entries) = fs::read_dir(std::env::temp_dir()) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        let Some(name) = path.file_name().and_then(|name| name.to_str()) else {
            continue;
        };
        if name.starts_with("gifbrewery-rendered-playback-")
            || name.starts_with("gifbrewery-rendered-preview-")
            || name.starts_with("gifbrewery-export-preview-")
        {
            let _ = fs::remove_dir_all(path);
        }
    }
}

fn playback_preload_project(project: &Project) -> Project {
    base_preview_project(project)
}

fn should_auto_preload_rendered_playback(project: &Project) -> bool {
    let project = playback_preload_project(project);
    let Some((width, height)) = effective_output_dimensions(&project) else {
        return false;
    };
    let Some(clip) = project.clips.first() else {
        return false;
    };
    let duration = clip.range.duration_seconds().max(0.01);
    let fps = source_frame_fps(&project)
        .unwrap_or_else(|| clip_fps_value(&clip.frame_strategy))
        .clamp(1.0, 120.0);
    let estimated_frame_pixels =
        u64::from(width) * u64::from(height) * (duration * fps).ceil().max(1.0) as u64;
    if estimated_frame_pixels > MAX_AUTO_PRELOAD_FRAME_PIXELS {
        crate::diagnostics::log_line(format_args!(
            "rendered sequence preload auto-skip estimate: output={}x{} duration={duration:.3}s fps={fps:.1} frame_pixels={estimated_frame_pixels}",
            width, height
        ));
        return false;
    }
    true
}

fn base_preview_project(project: &Project) -> Project {
    let mut project = project.clone();
    project.overlays.clear();
    apply_interactive_preview_size_cap(&mut project);
    project
}

fn apply_interactive_preview_size_cap(project: &mut Project) {
    let Some((width, height)) = effective_output_dimensions(project) else {
        return;
    };
    let max_edge = width.max(height);
    if max_edge <= MAX_INTERACTIVE_PREVIEW_EDGE {
        project.settings.gif.output_width = Some(width.max(1));
        project.settings.gif.output_height = Some(height.max(1));
        return;
    }

    let scale = f64::from(MAX_INTERACTIVE_PREVIEW_EDGE) / f64::from(max_edge);
    let preview_width = (f64::from(width) * scale).round().max(1.0) as u32;
    let preview_height = (f64::from(height) * scale).round().max(1.0) as u32;
    crate::diagnostics::log_line(format_args!(
        "interactive preview size capped: source_output={}x{} preview={}x{}",
        width, height, preview_width, preview_height
    ));
    project.settings.gif.output_width = Some(preview_width);
    project.settings.gif.output_height = Some(preview_height);
}

fn rendered_playback_cache_key(project: &Project) -> String {
    let mut key = String::new();
    if let Some(source) = &project.source {
        key.push_str(&source.path);
        key.push(';');
        key.push_str(&format!(
            "{:?}:{:?}:{:?}:{:?};",
            source.duration_seconds, source.natural_width, source.natural_height, source.fps
        ));
    }
    if let Some(clip) = project.clips.first() {
        key.push_str(&format!(
            "{:.6}:{:.6}:{:.6}:{:?};",
            clip.range.start_seconds, clip.range.end_seconds, clip.speed, clip.crop
        ));
        key.push_str(&format!("{:?};", clip.frame_strategy));
    }
    key.push_str(&format!(
        "{:?}:{:?}:{:?}:{:?}:{:?};",
        project.settings.gif.output_width,
        project.settings.gif.output_height,
        project.settings.gif.colors,
        project.settings.gif.high_quality_quantization,
        project.settings.gif.optimize
    ));
    key
}

fn preview_render_key(project: &Project, playhead_seconds: f64) -> String {
    let mut key = format!("{playhead_seconds:.6};");
    if let Some(source) = &project.source {
        key.push_str(&source.path);
        key.push(';');
        key.push_str(&format!(
            "{:?}:{:?}:{:?};",
            source.natural_width, source.natural_height, source.fps
        ));
    }
    if let Some(clip) = project.clips.first() {
        key.push_str(&format!(
            "{:.6}:{:.6}:{:?};",
            clip.range.start_seconds, clip.range.end_seconds, clip.crop
        ));
    }
    key.push_str(&format!(
        "{:?}:{:?};",
        project.settings.gif.output_width, project.settings.gif.output_height
    ));
    key
}

fn uses_rendered_output_preview(project: &Project) -> bool {
    project.source.is_some()
}

fn preview_text_reference_height(project: &Project) -> Option<u32> {
    let source_height = project
        .source
        .as_ref()
        .and_then(|source| source.natural_height)?;
    let crop = project.clips.first().and_then(|clip| clip.crop);
    let Some(crop) = crop else {
        return Some(source_height);
    };
    let top = crop.top.clamp(0.0, 0.95);
    let bottom = crop.bottom.clamp(0.0, 0.95);
    let total = top + bottom;
    let cropped_fraction = if total >= 0.98 { 0.02 } else { 1.0 - total };
    Some(
        (f64::from(source_height) * cropped_fraction)
            .round()
            .max(1.0) as u32,
    )
}

fn rendered_preview_aspect(project: &Project) -> f64 {
    if let (Some(width), Some(height)) = (
        project.settings.gif.output_width,
        project.settings.gif.output_height,
    ) {
        if height > 0 {
            return f64::from(width.max(1)) / f64::from(height);
        }
    }

    let Some(source) = &project.source else {
        return 16.0 / 9.0;
    };
    let source_width = f64::from(source.natural_width.unwrap_or(16).max(1));
    let source_height = f64::from(source.natural_height.unwrap_or(9).max(1));
    let crop = project.clips.first().and_then(|clip| clip.crop);
    let Some(crop) = crop else {
        return source_width / source_height.max(1.0);
    };
    let width_fraction =
        (1.0 - crop.left.clamp(0.0, 0.95) - crop.right.clamp(0.0, 0.95)).clamp(0.02, 1.0);
    let height_fraction =
        (1.0 - crop.top.clamp(0.0, 0.95) - crop.bottom.clamp(0.0, 0.95)).clamp(0.02, 1.0);
    (source_width * width_fraction) / (source_height * height_fraction).max(1.0)
}

fn contained_rect(width: f64, height: f64, aspect: f64) -> PixelBounds {
    let aspect = aspect.max(0.01);
    let widget_aspect = width / height.max(1.0);
    if widget_aspect > aspect {
        let content_width = height * aspect;
        PixelBounds {
            x: (width - content_width) / 2.0,
            y: 0.0,
            width: content_width,
            height,
        }
    } else {
        let content_height = width / aspect;
        PixelBounds {
            x: 0.0,
            y: (height - content_height) / 2.0,
            width,
            height: content_height,
        }
    }
}

fn timeline_state_from_project(
    project: &Project,
    selected_overlay_id: Option<&str>,
    playhead_seconds: f64,
    thumbnails: Vec<crate::timeline::TimelineThumbnail>,
) -> TimelineViewState {
    let clip = project.clips.first().expect("default project has a clip");
    let media_duration_seconds = project
        .source
        .as_ref()
        .and_then(|source| source.duration_seconds)
        .unwrap_or(clip.range.end_seconds)
        .max(clip.range.end_seconds)
        .max(0.01);
    let overlays = text_overlays_from_project(project)
        .into_iter()
        .map(|text| TimelineOverlayRange {
            id: text.id,
            label: text.text,
            range: text.range,
        })
        .collect();

    TimelineViewState {
        media_duration_seconds,
        frame_fps: Some(project_frame_fps(project)),
        playhead_seconds: playhead_seconds.clamp(0.0, media_duration_seconds),
        clip_range: clip.range,
        overlays,
        selected_overlay_id: selected_overlay_id.map(ToOwned::to_owned),
        thumbnails,
    }
}

fn draw_caption_overlay(
    cr: &cairo::Context,
    preview_width: f64,
    preview_height: f64,
    source_height: f64,
    text: &TextOverlay,
) -> PixelBounds {
    let (scaled_text, x, y, layout, bounds) =
        caption_layout(cr, preview_width, preview_height, source_height, text);
    let stroke_width = scaled_text.stroke_width.max(0.0);

    let _ = cr.save();
    if stroke_width > 0.0 {
        set_cairo_source_rgba(cr, scaled_text.stroke_color);
        cr.set_line_join(cairo::LineJoin::Miter);
        cr.set_line_cap(cairo::LineCap::Square);
        cr.set_line_width(stroke_width * 2.0);
        for (dx, dy) in square_stroke_offsets(stroke_width) {
            cr.new_path();
            cr.move_to(x + dx, y + dy);
            pangocairo::functions::layout_path(cr, &layout);
            let _ = cr.fill();
        }
    }

    set_cairo_source_rgba(cr, scaled_text.text_color);
    cr.new_path();
    cr.move_to(x, y);
    pangocairo::functions::layout_path(cr, &layout);
    let _ = cr.fill();
    let _ = cr.restore();

    bounds
}

fn square_stroke_offsets(stroke_width: f64) -> Vec<(f64, f64)> {
    let radius = stroke_width.round().max(0.0) as i32;
    if radius <= 0 {
        return Vec::new();
    }

    let mut offsets = Vec::new();
    for dy in -radius..=radius {
        for dx in -radius..=radius {
            if dx == 0 && dy == 0 {
                continue;
            }
            offsets.push((f64::from(dx), f64::from(dy)));
        }
    }
    offsets
}

fn draw_caption_overlay_in_rect(
    cr: &cairo::Context,
    rect: PixelBounds,
    source_height: f64,
    text: &TextOverlay,
) -> PixelBounds {
    let _ = cr.save();
    cr.rectangle(rect.x, rect.y, rect.width, rect.height);
    cr.clip();
    cr.translate(rect.x, rect.y);
    let mut bounds = draw_caption_overlay(cr, rect.width, rect.height, source_height, text);
    let _ = cr.restore();
    bounds.x += rect.x;
    bounds.y += rect.y;
    bounds
}

fn caption_layout(
    cr: &cairo::Context,
    preview_width: f64,
    preview_height: f64,
    source_height: f64,
    text: &TextOverlay,
) -> (TextOverlay, f64, f64, pango::Layout, PixelBounds) {
    let preview_scale = preview_height / source_height.max(1.0);
    let mut scaled_text = text.clone();
    scaled_text.font_size = (text.font_size * preview_scale).max(1.0);
    scaled_text.stroke_width = (text.stroke_width * preview_scale).max(0.0);
    let layout = pangocairo::functions::create_layout(cr);
    layout.set_text(&scaled_text.text);
    layout.set_font_description(Some(&font_description_for_text_overlay(&scaled_text)));
    layout.set_width((text.bounds.width * preview_width * f64::from(pango::SCALE)).round() as i32);
    layout.set_wrap(pango::WrapMode::WordChar);
    if text.alignment == TextAlignment::Center {
        layout.set_alignment(pango::Alignment::Center);
    } else {
        layout.set_alignment(pango::Alignment::Left);
    }

    let x = text.bounds.x * preview_width;
    let y = text.bounds.y * preview_height;
    let (ink_rect, _) = layout.pixel_extents();
    let stroke_width = scaled_text.stroke_width.max(0.0);
    let stroke_padding = stroke_width.ceil();
    let bounds = PixelBounds {
        x: x + f64::from(ink_rect.x()) - stroke_padding,
        y: y + f64::from(ink_rect.y()) - stroke_padding,
        width: f64::from(ink_rect.width()) + stroke_padding * 2.0,
        height: f64::from(ink_rect.height()) + stroke_padding * 2.0,
    };

    (scaled_text, x, y, layout, bounds)
}

fn draw_selected_caption_bounds(cr: &cairo::Context, bounds: PixelBounds) {
    let _ = cr.save();
    cr.set_source_rgba(0.12, 0.58, 1.0, 0.95);
    cr.set_line_width(2.0);
    cr.rectangle(
        bounds.x - 5.0,
        bounds.y - 5.0,
        bounds.width + 10.0,
        bounds.height + 10.0,
    );
    let _ = cr.stroke();

    cr.set_source_rgba(1.0, 1.0, 1.0, 0.88);
    cr.set_line_width(1.0);
    cr.rectangle(
        bounds.x - 2.5,
        bounds.y - 2.5,
        bounds.width + 5.0,
        bounds.height + 5.0,
    );
    let _ = cr.stroke();
    let _ = cr.restore();
}

fn draw_crop_overlay(cr: &cairo::Context, width: f64, height: f64, crop: Option<CropRect>) {
    let crop = crop.unwrap_or(CropRect {
        left: 0.0,
        right: 0.0,
        top: 0.0,
        bottom: 0.0,
    });
    let left = crop.left.clamp(0.0, 0.95) * width;
    let right = width - crop.right.clamp(0.0, 0.95) * width;
    let top = crop.top.clamp(0.0, 0.95) * height;
    let bottom = height - crop.bottom.clamp(0.0, 0.95) * height;
    let crop_width = (right - left).max(1.0);
    let crop_height = (bottom - top).max(1.0);
    let has_crop = crop.left > 0.0 || crop.right > 0.0 || crop.top > 0.0 || crop.bottom > 0.0;

    let _ = cr.save();
    if has_crop {
        cr.set_source_rgba(0.0, 0.0, 0.0, 0.46);
        cr.rectangle(0.0, 0.0, width, top);
        cr.rectangle(0.0, bottom, width, (height - bottom).max(0.0));
        cr.rectangle(0.0, top, left, crop_height);
        cr.rectangle(right, top, (width - right).max(0.0), crop_height);
        let _ = cr.fill();
    }

    cr.set_source_rgba(1.0, 1.0, 1.0, if has_crop { 0.92 } else { 0.34 });
    cr.set_line_width(if has_crop { 2.0 } else { 1.0 });
    cr.rectangle(left + 0.5, top + 0.5, crop_width - 1.0, crop_height - 1.0);
    let _ = cr.stroke();

    cr.set_source_rgba(0.15, 0.62, 1.0, if has_crop { 0.9 } else { 0.45 });
    cr.set_line_width(1.0);
    let third_w = crop_width / 3.0;
    let third_h = crop_height / 3.0;
    for offset in [third_w, third_w * 2.0] {
        cr.move_to(left + offset, top);
        cr.line_to(left + offset, bottom);
    }
    for offset in [third_h, third_h * 2.0] {
        cr.move_to(left, top + offset);
        cr.line_to(right, top + offset);
    }
    let _ = cr.stroke();
    let _ = cr.restore();
}

fn set_cairo_source_rgba(cr: &cairo::Context, color: RgbaColor) {
    cr.set_source_rgba(
        color.red.clamp(0.0, 1.0),
        color.green.clamp(0.0, 1.0),
        color.blue.clamp(0.0, 1.0),
        color.alpha.clamp(0.0, 1.0),
    );
}

fn source_detail(path: &str, metadata: Option<&MediaMetadata>) -> String {
    let mut parts = vec![format!("Source: {path}")];

    if let Some(metadata) = metadata {
        if let Some(duration) = metadata.duration_seconds {
            parts.push(format_duration(duration));
        }

        if let (Some(width), Some(height)) = (metadata.width, metadata.height) {
            parts.push(format!("{width} x {height}"));
        }

        if let Some(fps) = metadata.fps.filter(|fps| *fps > 0.0) {
            parts.push(format!("{fps:.2} fps"));
        }
    }

    parts.join(" | ")
}

fn format_duration(seconds: f64) -> String {
    if seconds >= 60.0 {
        let minutes = (seconds / 60.0).floor();
        let seconds = seconds - minutes * 60.0;
        format!("{minutes:.0}:{seconds:05.2}")
    } else {
        format!("{seconds:.2}s")
    }
}

fn compact_path(path: &str) -> String {
    let path = Path::new(path);
    let Some(file_name) = path.file_name().and_then(|name| name.to_str()) else {
        return path.display().to_string();
    };

    let Some(parent) = path.parent() else {
        return file_name.to_string();
    };

    format!("{}/{}", parent.display(), file_name)
}
