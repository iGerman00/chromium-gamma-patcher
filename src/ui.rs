//! The window: R2R keygen styled lol

use crate::apply::{self, RestorePlan};
use crate::discovery;
use crate::scan::{self, Row, RowState, ScanMsg};
use crate::store::Store;
use egui::{
    Align2, Color32, Context, FontFamily, FontId, Pos2, Rect, RichText, Sense, Stroke, TextStyle,
    TextureHandle, TextureOptions, Ui, UiBuilder, Vec2,
};
use std::path::PathBuf;
use std::sync::mpsc::Receiver;
use std::sync::Arc;

const FIELD: Color32 = Color32::from_rgb(0xFF, 0xFF, 0xFF);
const GREY: Color32 = Color32::from_rgb(0xD6, 0xD2, 0xCA);
const LIGHT: Color32 = Color32::from_rgb(0xFF, 0xFF, 0xFF);
const SHADOW: Color32 = Color32::from_rgb(0x80, 0x80, 0x80);
const DARK: Color32 = Color32::from_rgb(0x40, 0x40, 0x40);
const INK: Color32 = Color32::from_rgb(0x00, 0x00, 0x00);
const MUTED: Color32 = Color32::from_rgb(0x6E, 0x6E, 0x6E);
const OFF: Color32 = Color32::from_rgb(0x9C, 0x9C, 0x9C);
const NAVY: Color32 = Color32::from_rgb(0x00, 0x00, 0x80);
const WALL: Color32 = Color32::from_rgb(0xFF, 0xFF, 0xFF);

const ART_W: f32 = 668.0;
const ART_H: f32 = 375.0;
pub const WINDOW_W: f32 = ART_W;
pub const WINDOW_H: f32 = ART_H + 294.0;
const PANEL_W: f32 = ART_W - 16.0;
const PANEL_H: f32 = 280.0;
const PANEL_POS: Pos2 = Pos2::new(8.0, ART_H + 6.0);
const PAD: f32 = 8.0;
const FIELD_H: f32 = 22.0;
const ROW_H: f32 = 19.0;
const LIST_H: f32 = 90.0;
const LOG_H: f32 = 60.0;
const PRESETS: [(f32, f32); 2] = [(1.8, 0.2), (2.2, 0.0)];

const ABOUT: &str = "Chromium returns disgusting values for fonts - contrast 0.2 and gamma 1.2.\n\
Less disgusting: 1.8 / 0.2 and 2.2 / 0.0.\n\n\
Both are float32 constants in the executable and can be found by their bytes, so \n\
that's what's done here\n\n\
Patching keeps a pristine copy beside the binary; Restore puts it back after\n\
verifying the hash.\n\n\
Targets under /opt or /usr need root: run this program as sudo -E.\n\n\
The * beside a target pins it; pinned paths are scanned even when they can't\n\
be auto-discovered.";

/// Regular weights, in preference order. egui's own font is Ubuntu-Light,
/// which renders thin and washed out.
const SANS: &[&str] = &[
    "/usr/share/fonts/noto/NotoSans-Regular.ttf",
    "/usr/share/fonts/TTF/DejaVuSans.ttf",
    "/usr/share/fonts/liberation/LiberationSans-Regular.ttf",
    "/usr/share/fonts/truetype/dejavu/DejaVuSans.ttf",
];
const SANS2: &[&str] = &[
    "/usr/share/fonts/TTF/DejaVuSans.ttf",
    "/usr/share/fonts/liberation/LiberationSans-Regular.ttf",
];
const MONO: &[&str] = &[
    "/usr/share/fonts/noto/NotoSansMono-Regular.ttf",
    "/usr/share/fonts/TTF/DejaVuSansMono.ttf",
    "/usr/share/fonts/truetype/dejavu/DejaVuSansMono.ttf",
];
const MONO2: &[&str] = &[
    "/usr/share/fonts/TTF/DejaVuSansMono.ttf",
    "/usr/share/fonts/liberation/LiberationMono-Regular.ttf",
];

fn sans(size: f32) -> FontId {
    FontId::new(size, FontFamily::Proportional)
}

fn mono(size: f32) -> FontId {
    FontId::new(size, FontFamily::Monospace)
}

fn read_first(paths: &[&str]) -> Option<Vec<u8>> {
    paths.iter().find_map(|p| std::fs::read(p).ok())
}

fn install_fonts(ctx: &Context) {
    let mut defs = egui::FontDefinitions::default();
    let mut add = |key: &str, paths: &[&str]| -> Option<String> {
        read_first(paths).map(|bytes| {
            defs.font_data
                .insert(key.to_owned(), Arc::new(egui::FontData::from_owned(bytes)));
            key.to_owned()
        })
    };
    if let Some(primary) = add("ui-sans", SANS) {
        let list = defs.families.entry(FontFamily::Proportional).or_default();
        list.insert(0, primary);
        if let Some(fb) = add("ui-sans-2", SANS2) {
            list.push(fb);
        }
    }
    if let Some(primary) = add("ui-mono", MONO) {
        let list = defs.families.entry(FontFamily::Monospace).or_default();
        list.insert(0, primary);
        if let Some(fb) = add("ui-mono-2", MONO2) {
            list.push(fb);
        }
    }
    ctx.set_fonts(defs);
}

fn install_style(ctx: &Context) {
    let mut style = (*ctx.style()).clone();
    style.text_styles = [
        (TextStyle::Body, sans(13.5)),
        (TextStyle::Button, sans(13.5)),
        (TextStyle::Small, sans(11.5)),
        (TextStyle::Monospace, mono(11.0)),
        (TextStyle::Heading, sans(15.0)),
    ]
    .into();
    style.spacing.item_spacing = Vec2::new(6.0, 6.0);
    style.spacing.button_padding = Vec2::new(6.0, 2.0);
    style.spacing.menu_margin = egui::Margin::ZERO;

    let mut v = egui::Visuals::light();
    v.override_text_color = Some(INK);
    v.window_fill = GREY;
    v.panel_fill = GREY;
    v.extreme_bg_color = FIELD;
    v.window_stroke = Stroke::new(1.0, DARK);
    v.window_shadow = egui::epaint::Shadow::NONE;
    v.popup_shadow = egui::epaint::Shadow::NONE;
    v.window_corner_radius = egui::CornerRadius::ZERO;
    v.menu_corner_radius = egui::CornerRadius::ZERO;
    v.selection.bg_fill = NAVY;
    v.selection.stroke = Stroke::new(1.0, LIGHT);
    for w in [
        &mut v.widgets.inactive,
        &mut v.widgets.hovered,
        &mut v.widgets.active,
        &mut v.widgets.open,
        &mut v.widgets.noninteractive,
    ] {
        w.corner_radius = egui::CornerRadius::ZERO;
        w.expansion = 0.0;
        w.fg_stroke = Stroke::new(1.0, INK);
    }
    v.widgets.noninteractive.bg_fill = GREY;
    v.widgets.inactive.bg_fill = FIELD;
    v.widgets.inactive.weak_bg_fill = FIELD;
    v.widgets.inactive.bg_stroke = Stroke::new(1.0, DARK);
    v.widgets.hovered.bg_fill = FIELD;
    v.widgets.hovered.weak_bg_fill = FIELD;
    v.widgets.hovered.bg_stroke = Stroke::new(1.0, INK);
    v.widgets.active.bg_fill = FIELD;
    v.widgets.active.weak_bg_fill = FIELD;
    v.widgets.active.bg_stroke = Stroke::new(1.0, INK);
    style.animation_time = 0.0;
    style.visuals = v;
    ctx.set_style(style);
}

/// Flat 1px outline plus a light top-left / shadow bottom-right pair.
fn bevel(p: &egui::Painter, rect: Rect, raised: bool, fill: Color32) {
    p.rect_filled(rect, egui::CornerRadius::ZERO, fill);
    p.rect_stroke(
        rect,
        egui::CornerRadius::ZERO,
        Stroke::new(1.0, INK),
        egui::StrokeKind::Inside,
    );
    let (tl, br) = if raised { (LIGHT, SHADOW) } else { (SHADOW, LIGHT) };
    let r = Rect::from_min_max(rect.shrink(1.0).min, rect.shrink(1.0).max - Vec2::splat(1.0));
    p.line_segment([r.left_top(), r.right_top()], Stroke::new(1.0, tl));
    p.line_segment([r.left_top(), r.left_bottom()], Stroke::new(1.0, tl));
    p.line_segment([r.right_top(), r.right_bottom()], Stroke::new(1.0, br));
    p.line_segment([r.left_bottom(), r.right_bottom()], Stroke::new(1.0, br));
}

fn paint_button(p: &egui::Painter, rect: Rect, label: &str, enabled: bool, pressed: bool) {
    bevel(p, rect, !pressed, GREY);
    p.text(
        rect.center() + if pressed { Vec2::splat(1.0) } else { Vec2::ZERO },
        Align2::CENTER_CENTER,
        label,
        sans(13.0),
        if enabled { INK } else { OFF },
    );
}

/// Button in the layout flow.
fn button(ui: &mut Ui, label: &str, size: Vec2, enabled: bool) -> bool {
    let sense = if enabled { Sense::click() } else { Sense::hover() };
    let (rect, resp) = ui.allocate_exact_size(size, sense);
    paint_button(
        ui.painter(),
        rect,
        label,
        enabled,
        enabled && resp.is_pointer_button_down_on(),
    );
    enabled && resp.clicked()
}

/// Sunken white box of an exact height behind a group of widgets.
fn boxed(ui: &mut Ui, width: f32, height: f32, add: impl FnOnce(&mut Ui)) {
    egui::Frame::NONE
        .fill(FIELD)
        .stroke(Stroke::new(1.0, DARK))
        .inner_margin(3.0)
        .outer_margin(0.0)
        .corner_radius(egui::CornerRadius::ZERO)
        .show(ui, |ui| {
            ui.set_min_size(Vec2::new(width, height - 8.0));
            ui.set_max_size(Vec2::new(width, height - 8.0));
            add(ui);
        });
}

fn field(ui: &mut Ui, size: Vec2, widget: impl egui::Widget) -> egui::Response {
    let (rect, _) = ui.allocate_exact_size(size, Sense::hover());
    let response = ui.put(rect, widget);
    // The widget paints its own frame, which changes stroke colour when it
    // takes focus; a border of ours on top keeps it constant.
    ui.painter().rect_stroke(
        rect,
        egui::CornerRadius::ZERO,
        Stroke::new(1.0, INK),
        egui::StrokeKind::Inside,
    );
    response
}

fn state_short(row: &Row) -> &'static str {
    if row.busy {
        return "running - quit it";
    }
    match row.state {
        RowState::Pristine => "ready",
        RowState::PatchedKnown => "patched",
        RowState::PatchedUnknown => "patched (other tool)",
        RowState::Unsigned => "no signature",
        RowState::Unreadable => "unreadable",
    }
}

fn short_path(p: &str) -> String {
    if p.len() <= 52 {
        return p.to_string();
    }
    format!("...{}", &p[p.len() - 49..])
}

// ---------------------------------------------------------------------------

pub struct App {
    store: Store,
    rows: Vec<Row>,
    selected: usize,
    gamma: f32,
    contrast: f32,
    log: Vec<String>,
    /// Last non-empty selection in the log box, for the copy menu.
    log_sel: String,
    log_range: Option<(usize, usize)>,
    scanning: bool,
    progress: (usize, usize),
    rx: Option<Receiver<ScanMsg>>,
    path_input: String,
    show_about: bool,
    art: Option<TextureHandle>,
    screenshot: Option<PathBuf>,
    frames: u64,
    shot_sent: bool,
}

impl App {
    pub fn new(cc: &eframe::CreationContext<'_>) -> Self {
        install_fonts(&cc.egui_ctx);
        install_style(&cc.egui_ctx);
        let store = Store::load();
        let mut app = App {
            store,
            rows: Vec::new(),
            selected: 0,
            gamma: apply::DEFAULT_GAMMA,
            contrast: apply::DEFAULT_CONTRAST,
            log: Vec::new(),
            log_sel: String::new(),
            log_range: None,
            scanning: false,
            progress: (0, 0),
            rx: None,
            path_input: String::new(),
            show_about: false,
            art: None,
            screenshot: std::env::var("CGP_SCREENSHOT").ok().map(PathBuf::from),
            frames: 0,
            shot_sent: false,
        };
        app.art = load_art(&cc.egui_ctx);
        app.rescan(&cc.egui_ctx);
        app
    }

    /// Scroll lines
    fn log_tail(&self) -> String {
        let from = self.log.len().saturating_sub(100);
        self.log[from..].join("\n")
    }

    fn log_line(&mut self, msg: impl Into<String>) {
        self.log.push(msg.into());
        if self.log.len() > 200 {
            self.log.drain(..80);
        }
    }

    fn selected_row(&self) -> Option<&Row> {
        self.rows.get(self.selected)
    }

    fn rescan(&mut self, ctx: &Context) {
        let mut paths: Vec<(PathBuf, bool)> = discovery::candidates()
            .into_iter()
            .map(|p| {
                let bookmarked = self.store.is_bookmarked(&p);
                (p, bookmarked)
            })
            .collect();
        for b in self.store.bookmarks.clone() {
            if !paths.iter().any(|(p, _)| *p == b) {
                paths.push((b, true));
            }
        }
        if paths.is_empty() {
            self.log_line("nothing to scan");
            return;
        }
        let (tx, rx) = std::sync::mpsc::channel();
        scan::spawn_scan(paths.clone(), Arc::new(self.store.clone()), tx);
        self.rx = Some(rx);
        self.scanning = true;
        self.progress = (0, paths.len());
        ctx.request_repaint();
    }

    fn pump(&mut self, ctx: &Context) {
        let Some(rx) = &self.rx else { return };
        let mut rows: Vec<Row> = Vec::new();
        let mut finished = false;
        let mut counts = (0usize, 0usize);

        loop {
            match rx.try_recv() {
                Ok(ScanMsg::Progress { done, total, .. }) => self.progress = (done, total),
                Ok(ScanMsg::Row(row)) => rows.push(row),
                Ok(ScanMsg::Finished { scanned, listed }) => {
                    finished = true;
                    counts = (scanned, listed);
                }
                Err(std::sync::mpsc::TryRecvError::Empty) => break,
                Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                    finished = true;
                    break;
                }
            }
        }

        for row in rows {
            match self.rows.iter().position(|r| r.path == row.path) {
                Some(i) => self.rows[i] = row,
                None => self.rows.push(row),
            }
        }
        if finished {
            self.scanning = false;
            self.rx = None;
            self.rows
                .sort_by(|a, b| (b.can_patch(), &a.label).cmp(&(a.can_patch(), &b.label)));
            if self.selected >= self.rows.len() {
                self.selected = self.rows.len().saturating_sub(1);
            }
            self.log_line(format!(
                "{} scanned, {} usable, {} listed",
                counts.0,
                counts.1,
                self.rows.len()
            ));
        } else {
            ctx.request_repaint_after(std::time::Duration::from_millis(60));
        }
    }

    /// Refresh just the selected target (after patch/restore/pin).
    fn refresh_selected(&mut self) {
        let Some(row) = self.rows.get(self.selected) else { return };
        let path = row.path.clone();
        let bookmarked = self.store.is_bookmarked(&path);
        let fresh = scan::scan_one(&path, bookmarked, &self.store);
        match self.rows.iter().position(|r| r.path == fresh.path) {
            Some(i) => self.rows[i] = fresh,
            None => self.rows.push(fresh),
        }
    }

    fn patch(&mut self) {
        let Some(row) = self.rows.get(self.selected).cloned() else { return };
        if !row.can_patch() {
            self.log_line(format!("{}: nothing to patch", row.label));
            return;
        }
        if let Some(block) = apply::write_blocker_message(&row.path) {
            self.log_line(block);
            return;
        }
        match apply::do_patch(&row.path, self.gamma, self.contrast, true) {
            Ok(outcome) => {
                for line in outcome.detail.lines() {
                    self.log_line(line);
                }
                apply::record_patch(&mut self.store, &row.path, &outcome);
                if !row.running_pids.is_empty() {
                    self.log_line(format!("restart {} to see it", row.label));
                }
            }
            Err(e) => self.log_line(format!("FAIL {e}")),
        }
        self.refresh_selected();
    }

    fn restore(&mut self) {
        let Some(row) = self.rows.get(self.selected).cloned() else { return };
        if let Some(block) = apply::write_blocker_message(&row.path) {
            self.log_line(block);
            return;
        }
        let plan = match apply::restore_plan(&row.path, &self.store) {
            Ok(plan) => plan,
            Err(e) => {
                self.log_line(format!("FAIL {e}"));
                return;
            }
        };
        let result = match &plan {
            RestorePlan::Restore { orig_hash, pairs } => {
                apply::do_restore(&row.path, orig_hash, pairs)
            }
            RestorePlan::RestoreBackup { backup } => apply::do_restore_backup(&row.path, backup),
        };
        match result {
            Ok(detail) => {
                self.log_line(detail);
                self.store.entries.retain(|e| e.path != row.path);
                let _ = self.store.save();
            }
            Err(e) => self.log_line(format!("FAIL {e}")),
        }
        self.refresh_selected();
    }

    fn toggle_pin(&mut self, i: usize) {
        let Some(row) = self.rows.get(i) else { return };
        let path = row.path.clone();
        let label = row.label.clone();
        let (result, verb) = if self.store.is_bookmarked(&path) {
            (self.store.remove_bookmark(&path), "unpinned")
        } else {
            (self.store.add_bookmark(&path), "pinned")
        };
        self.log_line(format!("{verb} {label}"));
        if let Err(e) = result {
            self.log_line(format!("not saved: {e}"));
        }
        let pinned = self.store.is_bookmarked(&path);
        if let Some(r) = self.rows.get_mut(i) {
            r.bookmarked = pinned;
        }
    }

    fn add_path(&mut self, ctx: &Context) {
        let raw = self.path_input.trim().to_string();
        if raw.is_empty() {
            return;
        }
        let path = discovery::expand_tilde(&raw);
        if !path.is_file() {
            self.log_line(format!("not a file: {}", path.display()));
            return;
        }
        if let Err(e) = self.store.add_bookmark(&path) {
            self.log_line(format!("pinned {} - not saved: {e}", short_path(&path.to_string_lossy())));
            return;
        }
        self.path_input.clear();
        self.log_line(format!("pinned {}", short_path(&path.to_string_lossy())));
        self.rescan(ctx);
    }

    // -- drawing -----------------------------------------------------------

    fn backdrop(&self, ctx: &Context) {
        let rect = ctx.viewport_rect();
        let p = ctx.layer_painter(egui::LayerId::background());
        p.rect_filled(rect, egui::CornerRadius::ZERO, WALL);
        let Some(tex) = &self.art else { return };
        // One image pixel per screen pixel, top of the window, no scaling and
        // no crop: the artwork stays exactly as authored.
        let size = tex.size_vec2() / ctx.pixels_per_point();
        let pos = Pos2::new(rect.center().x - size.x / 2.0, rect.top());
        p.image(
            tex.id(),
            Rect::from_min_size(pos, size),
            Rect::from_min_max(Pos2::ZERO, Pos2::new(1.0, 1.0)),
            Color32::WHITE,
        );
    }

    fn dialog(&mut self, ctx: &Context) {
        let mut clicked_row = None;
        let mut toggle = None;

        egui::Area::new(egui::Id::new("panel"))
            .order(egui::Order::Middle)
            .fixed_pos(PANEL_POS)
            .movable(false)
            .constrain(false)
            .show(ctx, |ui| {
                // Chrome first: later paint lands on top.
                let origin = ui.next_widget_position();
                ui.allocate_space(Vec2::new(PANEL_W, PANEL_H));
                let body = Rect::from_min_size(origin, Vec2::new(PANEL_W, PANEL_H));
                {
                    let p = ui.painter();
                    bevel(p, body, true, GREY);
                }

                let left = body.left() + PAD;
                let width = PANEL_W - PAD * 2.0;

                // One column, laid out in order so no row can overlap the next.
                let mut cui = ui.new_child(UiBuilder::new().max_rect(Rect::from_min_size(
                    Pos2::new(left, body.top() + PAD),
                    Vec2::new(width, PANEL_H - PAD * 2.0),
                )));

                // The list is the selector: click a row to pick the target,
                // click its star to pin the path.
                boxed(&mut cui, width - 8.0, LIST_H, |ui| {
                    egui::ScrollArea::vertical()
                        .id_salt("targets")
                        .max_height(LIST_H - 8.0)
                        .auto_shrink([false, false])
                        .show(ui, |ui| {
                            if self.rows.is_empty() {
                                ui.label(
                                    RichText::new(if self.scanning {
                                        "scanning..."
                                    } else {
                                        "no chromium or electron binaries found"
                                    })
                                    .size(12.0)
                                    .color(MUTED),
                                );
                            }
                            for i in 0..self.rows.len() {
                                let (rect, resp) = ui.allocate_exact_size(
                                    Vec2::new(width - 18.0, ROW_H),
                                    Sense::click(),
                                );
                                let star = Rect::from_min_size(
                                    rect.left_top() + Vec2::new(3.0, 3.0),
                                    Vec2::splat(13.0),
                                );
                                let star_resp =
                                    ui.interact(star, ui.id().with(("pin", i)), Sense::click());
                                let row = &self.rows[i];
                                let picked = i == self.selected;
                                let p = ui.painter();
                                if picked {
                                    p.rect_filled(rect, egui::CornerRadius::ZERO, NAVY);
                                } else if resp.hovered() {
                                    p.rect_filled(rect, egui::CornerRadius::ZERO, LIGHT);
                                }
                                let fg = if picked { LIGHT } else { INK };
                                let dim = if picked { LIGHT } else { MUTED };
                                p.text(
                                    Pos2::new(rect.left() + 21.0, rect.center().y),
                                    Align2::LEFT_CENTER,
                                    &row.label,
                                    sans(12.5),
                                    fg,
                                );
                                p.text(
                                    Pos2::new(rect.right() - 4.0, rect.center().y),
                                    Align2::RIGHT_CENTER,
                                    state_short(row),
                                    sans(11.5),
                                    dim,
                                );
                                if row.bookmarked {
                                    p.text(
                                        Pos2::new(star.center().x + 1.0, star.center().y),
                                        Align2::CENTER_CENTER,
                                        "*",
                                        sans(15.0),
                                        fg,
                                    );
                                }
                                if star_resp.clicked() {
                                    toggle = Some(i);
                                } else if resp.clicked() {
                                    clicked_row = Some(i);
                                }
                            }
                        });
                });

                let can_patch = self.selected_row().is_some_and(|r| r.can_patch());
                let can_restore = self.selected_row().is_some_and(|r| r.can_restore());

                let (row_rect, _) =
                    cui.allocate_exact_size(Vec2::new(width, FIELD_H), Sense::hover());
                let mut row = cui.new_child(
                    UiBuilder::new()
                        .max_rect(row_rect)
                        .layout(egui::Layout::left_to_right(egui::Align::Center)),
                );
                row.label("gamma");
                field(
                    &mut row,
                    Vec2::new(52.0, FIELD_H),
                    egui::DragValue::new(&mut self.gamma)
                        .speed(0.01)
                        .range(0.0..=5.0)
                        .fixed_decimals(2),
                );
                row.label("contrast");
                field(
                    &mut row,
                    Vec2::new(52.0, FIELD_H),
                    egui::DragValue::new(&mut self.contrast)
                        .speed(0.01)
                        .range(-1.0..=1.0)
                        .fixed_decimals(2),
                );
                row.add_space(4.0);
                for (gamma, contrast) in PRESETS {
                    if button(
                        &mut row,
                        &format!("{gamma:.1}/{contrast:.1}"),
                        Vec2::new(56.0, FIELD_H),
                        true,
                    ) {
                        self.gamma = gamma;
                        self.contrast = contrast;
                    }
                }

                let (path_rect, _) =
                    cui.allocate_exact_size(Vec2::new(width, FIELD_H), Sense::hover());
                let mut path = cui.new_child(
                    UiBuilder::new()
                        .max_rect(path_rect)
                        .layout(egui::Layout::left_to_right(egui::Align::Center)),
                );
                path.label("pin path");
                field(
                    &mut path,
                    Vec2::new((width - 140.0).max(80.0), FIELD_H),
                    egui::TextEdit::singleline(&mut self.path_input)
                        .hint_text(RichText::new("/path/to/binary").color(MUTED)),
                );
                if button(
                    &mut path,
                    "Add",
                    Vec2::new(60.0, FIELD_H),
                    !self.path_input.trim().is_empty(),
                ) {
                    self.add_path(ctx);
                }
                boxed(&mut cui, width - 8.0, LOG_H, |ui| {
                    egui::ScrollArea::vertical()
                        .id_salt("log")
                        .max_height(LOG_H - 8.0)
                        .auto_shrink([false, false])
                        .stick_to_bottom(true)
                        .show(ui, |ui| {
                            let mut text = self.log_tail();
                            let out = egui::TextEdit::multiline(&mut text)
                                .font(mono(11.0))
                                .desired_width(f32::INFINITY)
                                .desired_rows(3)
                                .frame(false)
                                .show(ui);
                            if let Some(range) = out
                                .cursor_range
                                .filter(|r| r.primary.index != r.secondary.index)
                            {
                                let a = range.primary.index.min(range.secondary.index);
                                let b = range.primary.index.max(range.secondary.index);
                                self.log_sel = text.chars().skip(a).take(b - a).collect();
                                self.log_range = Some((a, b));
                            } else if out.response.clicked() {
                                self.log_sel.clear();
                                self.log_range = None;
                            } else if egui::Popup::is_any_open(ui.ctx()) {
                                if let Some((a, b)) = self.log_range {
                                    let id = out.response.id;
                                    if let Some(mut state) =
                                        egui::widgets::text_edit::TextEditState::load(ui.ctx(), id)
                                    {
                                        state.cursor.set_char_range(Some(
                                            egui::text::CCursorRange::two(
                                                egui::text::CCursor::new(a),
                                                egui::text::CCursor::new(b),
                                            ),
                                        ));
                                        state.store(ui.ctx(), id);
                                    }
                                }
                            }
                            out.response.context_menu(|ui| {
                                // Menu highlight only: the same navy as the target list.
                                ui.style_mut().visuals.override_text_color = None;
                                ui.style_mut().visuals.widgets.hovered.weak_bg_fill = NAVY;
                                ui.style_mut().visuals.widgets.hovered.fg_stroke =
                                    Stroke::new(1.0, LIGHT);
                                if ui
                                    .add_enabled(
                                        !self.log_sel.is_empty(),
                                        egui::Button::new("Copy"),
                                    )
                                    .clicked()
                                {
                                    ui.ctx().copy_text(self.log_sel.clone());
                                    ui.close();
                                }
                            });
                        });
                });
                cui.add_space(10.0);

                // Buttons go last, in the dead space under the log.
                cui.horizontal(|ui| {
                    let row_w = 84.0 + 84.0 + 74.0 + 22.0 + 22.0 + 4.0 * 6.0;
                    ui.add_space(((width - 8.0 - row_w) / 2.0).max(0.0));
                    if button(ui, "Patch", Vec2::new(84.0, FIELD_H), can_patch && !self.scanning) {
                        self.patch();
                    }
                    if button(ui, "Restore", Vec2::new(84.0, FIELD_H), can_restore && !self.scanning)
                    {
                        self.restore();
                    }
                    if button(ui, "Rescan", Vec2::new(74.0, FIELD_H), !self.scanning) {
                        self.rows.clear();
                        self.rescan(ctx);
                    }
                    if button(ui, "?", Vec2::new(22.0, FIELD_H), true) {
                        self.show_about = !self.show_about;
                    }
                    if button(ui, "X", Vec2::new(22.0, FIELD_H), true) {
                        ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                    }
                    if self.scanning {
                        ui.label(
                            RichText::new(format!("{}/{}", self.progress.0, self.progress.1))
                                .size(11.0)
                                .color(MUTED),
                        );
                    }
                });

                if let Some(i) = clicked_row {
                    self.selected = i;
                }
                if let Some(i) = toggle {
                    self.toggle_pin(i);
                }
            });

        if self.show_about {
            self.about(ctx);
        }
    }

    fn about(&mut self, ctx: &Context) {
        let mut open = self.show_about;
        egui::Window::new("About")
            .collapsible(false)
            .resizable(false)
            .open(&mut open)
            .anchor(Align2::CENTER_CENTER, Vec2::ZERO)
            .frame(
                egui::Frame::NONE
                    .fill(WALL)
                    .stroke(Stroke::new(1.0, DARK))
                    .inner_margin(12.0)
                    .shadow(egui::epaint::Shadow::NONE),
            )
            .show(ctx, |ui| {
                ui.set_max_width(400.0);
                ui.label(RichText::new(ABOUT).size(12.5).color(INK));
            });
        self.show_about = open;
    }

    /// Wayland may drop the resize hints, so put the size back.
    fn keep_fixed_size(&self, ctx: &Context) {
        let want = Vec2::new(WINDOW_W, WINDOW_H);
        if (ctx.viewport_rect().size() - want).length() > 2.0 {
            ctx.send_viewport_cmd(egui::ViewportCommand::Resizable(false));
            ctx.send_viewport_cmd(egui::ViewportCommand::InnerSize(want));
        }
    }

    fn screenshot_hook(&mut self, ctx: &Context) {
        let Some(path) = self.screenshot.clone() else { return };
        self.frames += 1;
        if !self.shot_sent {
            if self.frames < 30 {
                ctx.request_repaint();
                return;
            }
            ctx.send_viewport_cmd(egui::ViewportCommand::Screenshot(egui::UserData::default()));
            self.shot_sent = true;
        }
        let image = ctx.input(|i| {
            i.events.iter().find_map(|e| match e {
                egui::Event::Screenshot { image, .. } => Some(image.clone()),
                _ => None,
            })
        });
        match image {
            Some(img) => {
                save_color_image(&path, &img);
                eprintln!("screenshot written to {}", path.display());
                ctx.send_viewport_cmd(egui::ViewportCommand::Close);
            }
            None => ctx.request_repaint(),
        }
    }
}

impl eframe::App for App {
    /// Opaque clear: on Wayland a transparent surface shows the desktop
    /// that can be kinda sucky if you use backdrop blur type effects
    fn clear_color(&self, _visuals: &egui::Visuals) -> [f32; 4] {
        WALL.to_normalized_gamma_f32()
    }

    fn update(&mut self, ctx: &Context, _frame: &mut eframe::Frame) {
        self.keep_fixed_size(ctx);
        self.pump(ctx);
        self.backdrop(ctx);
        self.dialog(ctx);
        self.screenshot_hook(ctx);
    }
}

// ---------------------------------------------------------------------------

fn load_art(ctx: &Context) -> Option<TextureHandle> {
    let bytes: &[u8] = include_bytes!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/assets/xenia_trol.png"
    ));
    let img = image::load_from_memory(bytes).ok()?.to_rgba8();
    let size = [img.width() as usize, img.height() as usize];
    let color = egui::ColorImage::from_rgba_unmultiplied(size, img.as_raw());
    Some(ctx.load_texture("keygen-art", color, TextureOptions::NEAREST))
}

fn save_color_image(path: &std::path::Path, img: &egui::ColorImage) -> Option<()> {
    let [w, h] = img.size;
    let mut buf = Vec::with_capacity(w * h * 4);
    for px in &img.pixels {
        buf.extend_from_slice(&px.to_array());
    }
    image::RgbaImage::from_raw(w as u32, h as u32, buf)?.save(path).ok()?;
    Some(())
}

/// `chromium-gamma-patcher scan`: same scan, plain text.
pub fn headless_scan() -> i32 {
    let store = Store::load();
    let paths = scan_targets(&store);
    let (tx, rx) = std::sync::mpsc::channel();
    scan::spawn_scan(paths, Arc::new(store.clone()), tx);
    let mut rows: Vec<Row> = Vec::new();
    loop {
        match rx.recv() {
            Ok(ScanMsg::Row(row)) => rows.push(row),
            Ok(ScanMsg::Progress { .. }) => {}
            Ok(ScanMsg::Finished { scanned, listed }) => {
                rows.sort_by(|a, b| a.label.cmp(&b.label));
                for row in &rows {
                    println!(
                        "{:<26} {:<22} {}",
                        row.label,
                        state_short(row),
                        row.path.display()
                    );
                    if !row.pairs.is_empty() {
                        let at = row
                            .pairs
                            .iter()
                            .map(|p| format!("0x{:x}/0x{:x}", p.contrast_off, p.gamma_off))
                            .collect::<Vec<_>>()
                            .join(" ");
                        println!("{:26}  offsets {at}", "");
                    }
                    if let Some(block) = apply::write_blocker_message(&row.path) {
                        println!("{:26}  {block}", "");
                    }
                }
                println!("\n{scanned} scanned, {listed} usable");
                return 0;
            }
            Err(_) => return 1,
        }
    }
}

/// Every path worth reading: discovery plus pinned paths, with their pin state.
fn scan_targets(store: &Store) -> Vec<(PathBuf, bool)> {
    let mut paths: Vec<(PathBuf, bool)> = discovery::candidates()
        .into_iter()
        .map(|p| {
            let pinned = store.is_bookmarked(&p);
            (p, pinned)
        })
        .collect();
    for pinned in store.bookmarks.clone() {
        if !paths.iter().any(|(p, _)| *p == pinned) {
            paths.push((pinned, true));
        }
    }
    paths
}
