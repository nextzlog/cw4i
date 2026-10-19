//! The CW4ISR window contents: source controls, a waterfall with station markers,
//! and one lane per station with its decoded text and the words it recognised.
//!
//! [`Cw4i`] draws into any egui `Ui`, so the standalone application and hosts such as
//! zLog show the same decoder; picked words are handed out by [`Cw4i::take_picks`].

use std::path::Path;

use egui::text::{LayoutJob, TextFormat};
use egui::{Color32, FontId, Rect, RichText, Sense, Stroke, Ui};

use crate::audio::{self, Device, Source, WavFile};
use crate::entry::Pick;
use crate::station::{Station, Stations, WordKind};
use crate::text::texts;
use crate::waterfall::{Waterfall, HI_HZ, LO_HZ};

/// Seconds a station stays listed after it went off the air.
const KEEP_SEC: u64 = 120;

/// Characters of decoded text shown in a lane.
const LANE_CHARS: usize = 120;

/// Where the waterfall is drawn.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Place {
    /// A tall panel at the left, frequency downwards.
    Left,
    /// A tall panel at the right, frequency downwards.
    Right,
    /// A wide strip above the lanes, frequency to the right.
    Top,
}

impl Place {
    /// Name used in settings.
    pub fn name(self) -> &'static str {
        match self {
            Place::Left => "left",
            Place::Right => "right",
            Place::Top => "top",
        }
    }

    pub fn from_name(name: &str) -> Option<Place> {
        [Place::Left, Place::Right, Place::Top]
            .into_iter()
            .find(|p| p.name() == name)
    }
}

/// The decoder and its window contents.
pub struct Cw4i {
    /// Language code of the user interface ("en" or "ja").
    pub language: String,
    /// Selected input device, `None` for the system default.
    pub device: Option<String>,
    /// Also list tracks not yet shown to be stations.
    pub show_candidates: bool,
    /// Where the waterfall is drawn.
    pub place: Place,
    devices: Vec<String>,
    source: Option<Source>,
    decoder: Option<rumo::Decoder>,
    stations: Stations,
    waterfall: Option<Waterfall>,
    texture: Option<(egui::TextureHandle, u64, bool, bool)>,
    selected: Option<u32>,
    picks: Vec<Pick>,
    error: Option<String>,
    samples: u64,
}

impl Default for Cw4i {
    fn default() -> Self {
        Cw4i::new("en")
    }
}

impl Cw4i {
    pub fn new(language: &str) -> Self {
        Cw4i {
            language: language.into(),
            device: None,
            show_candidates: false,
            place: Place::Top,
            devices: Vec::new(),
            source: None,
            decoder: None,
            stations: Stations::default(),
            waterfall: None,
            texture: None,
            selected: None,
            picks: Vec::new(),
            error: None,
            samples: 0,
        }
    }

    fn t(&self, key: &'static str) -> &'static str {
        texts().get(&self.language, key)
    }

    fn begin(&mut self, source: Result<Source, String>) {
        match source {
            Ok(s) => {
                let rate = s.sample_rate();
                self.decoder = Some(rumo::Decoder::new(rate));
                self.waterfall = Some(Waterfall::new(rate));
                self.stations.clear();
                self.samples = 0;
                self.source = Some(s);
                self.error = None;
            }
            Err(e) => self.error = Some(e),
        }
    }

    /// Start capturing the selected input device.
    pub fn start(&mut self) {
        let d = Device::open(self.device.as_deref()).map(Source::Device);
        self.begin(d);
    }

    /// Decode a WAV file at real time, as if it came from the receiver.
    pub fn open_file(&mut self, path: &Path) {
        let f = WavFile::open(path).map(Source::File);
        self.begin(f);
    }

    /// Stop the input; decoded text stays on screen.
    pub fn stop(&mut self) {
        self.flush();
        self.source = None;
    }

    pub fn running(&self) -> bool {
        self.source.is_some()
    }

    /// Forget every station heard.
    pub fn clear(&mut self) {
        if let Some(rate) = self.source.as_ref().map(Source::sample_rate) {
            self.decoder = Some(rumo::Decoder::new(rate));
        }
        if let Some(w) = self.waterfall.as_mut() {
            w.clear();
        }
        self.stations.clear();
        self.selected = None;
    }

    fn flush(&mut self) {
        if let Some(d) = self.decoder.as_mut() {
            for e in d.flush() {
                self.stations.apply(e);
            }
        }
    }

    /// Decode the samples that arrived since the last frame.
    pub fn process(&mut self) {
        let Some(src) = self.source.as_mut() else {
            return;
        };
        let x = src.take();
        let finished = src.finished();
        let rate = src.sample_rate() as u64;
        if let (Some(d), Some(w)) = (self.decoder.as_mut(), self.waterfall.as_mut()) {
            w.push(&x);
            for e in d.push(&x) {
                self.stations.apply(e);
            }
            self.stations.refresh(d);
        }
        self.samples += x.len() as u64;
        self.stations
            .forget_before(self.samples.saturating_sub(KEEP_SEC * rate));
        if finished {
            self.stop();
        }
    }

    /// Words picked since the last call, for the host's entry row.
    pub fn take_picks(&mut self) -> Vec<Pick> {
        std::mem::take(&mut self.picks)
    }

    /// Stations heard, sorted by frequency.
    pub fn stations(&self) -> Vec<&Station> {
        self.stations.sorted()
    }

    /// Draw the toolbar, the waterfall and the lanes.
    pub fn ui(&mut self, ui: &mut Ui) {
        self.toolbar(ui, |_| {});
        ui.add_space(6.0);
        self.body(ui);
    }

    /// One row of controls; `extra` adds the host's own items at the right end.
    pub fn toolbar(&mut self, ui: &mut Ui, extra: impl FnOnce(&mut Ui)) {
        if self.devices.is_empty() {
            self.devices = audio::devices();
        }
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 6.0;
            let (label, running) = if self.running() {
                (format!("\u{25A0}  {}", self.t("stop")), true)
            } else {
                (format!("\u{25B6}  {}", self.t("start")), false)
            };
            let accent = ui.visuals().selection.bg_fill;
            let button = egui::Button::new(RichText::new(label).strong())
                .min_size(egui::vec2(84.0, 24.0))
                .fill(if running {
                    Color32::from_rgb(190, 60, 60)
                } else {
                    accent
                });
            if ui.add(button).clicked() {
                if running {
                    self.stop();
                } else {
                    self.start();
                }
            }
            let shown = self
                .device
                .clone()
                .unwrap_or_else(|| self.t("default_device").into());
            let default = self.t("default_device");
            egui::ComboBox::from_id_salt("cw4i_device")
                .selected_text(shown)
                .width(200.0)
                .show_ui(ui, |ui| {
                    ui.selectable_value(&mut self.device, None, default);
                    for d in &self.devices {
                        ui.selectable_value(&mut self.device, Some(d.clone()), d);
                    }
                })
                .response
                .on_hover_text(self.t("device"));
            if ui
                .button("\u{27F2}")
                .on_hover_text(self.t("reset"))
                .clicked()
            {
                self.clear();
            }
            let label = self.t("show_candidates");
            ui.toggle_value(&mut self.show_candidates, label);
            let (left, top, right) = (
                self.t("place_left"),
                self.t("place_top"),
                self.t("place_right"),
            );
            ui.selectable_value(&mut self.place, Place::Left, "\u{25E7}")
                .on_hover_text(left);
            ui.selectable_value(&mut self.place, Place::Top, "\u{2B12}")
                .on_hover_text(top);
            ui.selectable_value(&mut self.place, Place::Right, "\u{25E8}")
                .on_hover_text(right);
            ui.separator();
            let (dot, state) = match &self.source {
                Some(s) => (Color32::from_rgb(220, 70, 70), s.name().to_string()),
                None => (ui.visuals().weak_text_color(), self.t("not_running").into()),
            };
            ui.label(RichText::new("\u{25CF}").color(dot));
            ui.add(egui::Label::new(RichText::new(state).weak()).truncate());
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), extra);
        });
    }

    /// The waterfall and the lanes.
    pub fn body(&mut self, ui: &mut Ui) {
        self.process();
        if self.running() {
            ui.ctx()
                .request_repaint_after(std::time::Duration::from_millis(40));
        }
        let dropped: Vec<std::path::PathBuf> = ui.ctx().input(|i| {
            i.raw
                .dropped_files
                .iter()
                .map(|f| f.path().to_path_buf())
                .collect()
        });
        if let Some(p) = dropped.first() {
            self.open_file(p.as_path());
        }
        if let Some(e) = &self.error {
            ui.colored_label(
                Color32::from_rgb(220, 60, 60),
                texts().format(&self.language, "error", &[e]),
            );
        }
        // a gap between the waterfall and the lanes, on the side facing the lanes
        const GAP: i8 = 10;
        let gap = |m: egui::Margin| egui::Frame::new().inner_margin(m);
        match self.place {
            Place::Top => {
                egui::Panel::top("cw4i_waterfall_top")
                    .resizable(true)
                    .default_size(160.0)
                    .size_range(70.0..=500.0)
                    .frame(gap(egui::Margin {
                        bottom: GAP,
                        ..Default::default()
                    }))
                    .show(ui, |ui| self.waterfall_ui(ui, false));
            }
            Place::Left => {
                egui::Panel::left("cw4i_waterfall_left")
                    .resizable(true)
                    .default_size(230.0)
                    .size_range(130.0..=600.0)
                    .frame(gap(egui::Margin {
                        right: GAP,
                        ..Default::default()
                    }))
                    .show(ui, |ui| self.waterfall_ui(ui, true));
            }
            Place::Right => {
                egui::Panel::right("cw4i_waterfall_right")
                    .resizable(true)
                    .default_size(230.0)
                    .size_range(130.0..=600.0)
                    .frame(gap(egui::Margin {
                        left: GAP,
                        ..Default::default()
                    }))
                    .show(ui, |ui| self.waterfall_ui(ui, true));
            }
        }
        egui::CentralPanel::no_frame().show(ui, |ui| self.lanes(ui));
    }

    fn waterfall_ui(&mut self, ui: &mut Ui, vertical: bool) {
        let dark = ui.visuals().dark_mode;
        let size = ui.available_size();
        let (rect, response) = ui.allocate_exact_size(size, Sense::click());
        let painter = ui.painter_at(rect);
        painter.rect_filled(rect, 6.0, ui.visuals().extreme_bg_color);
        if let Some(w) = &self.waterfall {
            let stale = match &self.texture {
                Some((_, v, d, o)) => *v != w.version || *d != dark || *o != vertical,
                None => true,
            };
            if stale {
                let image = if vertical {
                    w.image_vertical(dark)
                } else {
                    w.image(dark)
                };
                match self.texture.as_mut() {
                    Some((t, v, d, o)) => {
                        t.set(image, egui::TextureOptions::LINEAR);
                        (*v, *d, *o) = (w.version, dark, vertical);
                    }
                    None => {
                        let t = ui.ctx().load_texture(
                            "cw4i_waterfall",
                            image,
                            egui::TextureOptions::LINEAR,
                        );
                        self.texture = Some((t, w.version, dark, vertical));
                    }
                }
            }
            if let Some((t, _, _, _)) = &self.texture {
                let uv = Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0));
                painter.image(t.id(), rect, uv, Color32::WHITE);
            }
        }
        // position of a frequency along the frequency axis
        let at = |f: f64| {
            let r = ((f - LO_HZ) / (HI_HZ - LO_HZ)) as f32;
            if vertical {
                rect.top() + r * rect.height()
            } else {
                rect.left() + r * rect.width()
            }
        };
        let tag = |painter: &egui::Painter,
                   pos: egui::Pos2,
                   align: egui::Align2,
                   text: String,
                   font: FontId,
                   colour: Color32| {
            let g = painter.layout_no_wrap(text, font, colour);
            let r = align.anchor_size(pos, g.size());
            painter.rect_filled(
                r.expand(2.0),
                3.0,
                ui.visuals().window_fill.gamma_multiply(0.85),
            );
            painter.galley(r.min, g, Color32::WHITE);
        };
        let faint = ui.visuals().weak_text_color();
        let text = ui.visuals().text_color();
        let mut f = (LO_HZ / 100.0).ceil() * 100.0;
        while f <= HI_HZ {
            let p = at(f);
            let (a, b, label, align) = if vertical {
                (
                    egui::pos2(rect.right() - 6.0, p),
                    egui::pos2(rect.right(), p),
                    egui::pos2(rect.right() - 8.0, p),
                    egui::Align2::RIGHT_CENTER,
                )
            } else {
                (
                    egui::pos2(p, rect.bottom() - 6.0),
                    egui::pos2(p, rect.bottom()),
                    egui::pos2(p, rect.bottom() - 8.0),
                    egui::Align2::CENTER_BOTTOM,
                )
            };
            painter.line_segment([a, b], Stroke::new(1.0, faint));
            if (f as i64) % 200 == 0 {
                tag(
                    &painter,
                    label,
                    align,
                    format!("{f:.0}"),
                    FontId::proportional(10.0),
                    text,
                );
            }
            f += 100.0;
        }
        let accent = ui.visuals().selection.bg_fill;
        let strong = ui.visuals().strong_text_color();
        let mut placed: Vec<Rect> = Vec::new();
        for s in self.stations.sorted() {
            if !s.confirmed && !self.show_candidates {
                continue;
            }
            let p = at(s.freq);
            let colour = if !s.alive || !s.confirmed {
                faint
            } else {
                accent
            };
            let (tri, label, align, line) = if vertical {
                let x = rect.left();
                (
                    vec![
                        egui::pos2(x, p - 5.0),
                        egui::pos2(x, p + 5.0),
                        egui::pos2(x + 7.0, p),
                    ],
                    egui::pos2(x + 10.0, p),
                    egui::Align2::LEFT_CENTER,
                    [egui::pos2(x + 8.0, p), egui::pos2(rect.right(), p)],
                )
            } else {
                let y = rect.top();
                (
                    vec![
                        egui::pos2(p - 5.0, y),
                        egui::pos2(p + 5.0, y),
                        egui::pos2(p, y + 7.0),
                    ],
                    egui::pos2(p, y + 9.0),
                    egui::Align2::CENTER_TOP,
                    [egui::pos2(p, y + 24.0), egui::pos2(p, rect.bottom())],
                )
            };
            painter.add(egui::Shape::convex_polygon(tri, colour, Stroke::NONE));
            let name = s
                .callsign()
                .map(str::to_string)
                .unwrap_or_else(|| format!("{:.0}", s.freq));
            let g = painter.layout_no_wrap(name, FontId::monospace(11.0), strong);
            // move a label that would cover an earlier one to the next free row
            let mut r = align.anchor_size(label, g.size()).expand(2.0);
            for _ in 0..6 {
                if !placed.iter().any(|q: &Rect| q.intersects(r)) {
                    break;
                }
                r = if vertical {
                    r.translate(egui::vec2(r.width() + 3.0, 0.0))
                } else {
                    r.translate(egui::vec2(0.0, r.height() + 2.0))
                };
            }
            placed.push(r);
            painter.rect_filled(r, 3.0, ui.visuals().window_fill.gamma_multiply(0.85));
            painter.galley(r.min + egui::vec2(2.0, 2.0), g, Color32::WHITE);
            if Some(s.id) == self.selected {
                painter.line_segment(line, Stroke::new(1.0, accent));
            }
        }
        if response.clicked() {
            if let Some(q) = response.interact_pointer_pos() {
                let q = if vertical { q.y } else { q.x };
                self.selected = self
                    .stations
                    .sorted()
                    .into_iter()
                    .filter(|s| s.confirmed || self.show_candidates)
                    .min_by(|a, b| (at(a.freq) - q).abs().total_cmp(&(at(b.freq) - q).abs()))
                    .filter(|s| (at(s.freq) - q).abs() < 12.0)
                    .map(|s| s.id);
            }
        }
    }

    fn lanes(&mut self, ui: &mut Ui) {
        let shown: Vec<Station> = self
            .stations
            .sorted()
            .into_iter()
            .filter(|s| s.confirmed || self.show_candidates)
            .cloned()
            .collect();
        if shown.is_empty() {
            ui.add_space(40.0);
            ui.vertical_centered(|ui| {
                ui.label(RichText::new(self.t("no_signals")).weak().size(15.0));
                ui.label(RichText::new(self.t("drop_hint")).weak().small());
            });
            return;
        }
        egui::ScrollArea::vertical()
            .auto_shrink([false, false])
            .show(ui, |ui| {
                for s in &shown {
                    self.lane(ui, s);
                }
            });
    }

    fn lane(&mut self, ui: &mut Ui, s: &Station) {
        let visuals = ui.visuals().clone();
        let selected = Some(s.id) == self.selected;
        let accent = visuals.selection.bg_fill;
        let amber = Color32::from_rgb(214, 140, 30);
        let frame = egui::Frame::new()
            .fill(if selected {
                accent.gamma_multiply(0.18)
            } else {
                visuals.faint_bg_color
            })
            .stroke(Stroke::new(
                1.0,
                if selected {
                    accent
                } else {
                    visuals.widgets.noninteractive.bg_stroke.color
                },
            ))
            .corner_radius(6.0)
            .inner_margin(egui::Margin::symmetric(10, 6));
        let response = frame
            .show(ui, |ui| {
                ui.set_width(ui.available_width());
                ui.horizontal_top(|ui| {
                    // left column: callsign or frequency, then frequency, speed and SNR
                    ui.allocate_ui_with_layout(
                        egui::vec2(120.0, 40.0),
                        egui::Layout::top_down(egui::Align::Min),
                        |ui| {
                            ui.set_width(120.0);
                            let head = match s.callsign() {
                                Some(c) => RichText::new(c).monospace().size(16.0).strong(),
                                None => RichText::new(format!("{:.0} Hz", s.freq))
                                    .monospace()
                                    .size(16.0),
                            };
                            ui.label(if s.alive { head } else { head.weak() });
                            let meta = format!(
                                "{:.0} Hz \u{00B7} {:.0} WPM \u{00B7} {:+.0} dB",
                                s.freq, s.wpm, s.snr
                            );
                            ui.label(RichText::new(meta).small().weak());
                        },
                    );
                    ui.vertical(|ui| {
                        ui.label(lane_text(s, &visuals));
                        let picks: Vec<(Pick, &crate::station::Word)> = s
                            .words
                            .iter()
                            .filter_map(|w| Pick::of(w).map(|p| (p, w)))
                            .collect();
                        if !picks.is_empty() {
                            ui.horizontal_wrapped(|ui| {
                                ui.spacing_mut().item_spacing.x = 4.0;
                                for (pick, w) in picks {
                                    let mut text = RichText::new(pick.text()).monospace();
                                    if w.kind == WordKind::Callsign {
                                        text = text.strong();
                                    }
                                    let colour = if w.sure { accent } else { amber };
                                    let b = egui::Button::new(text)
                                        .fill(colour.gamma_multiply(0.22))
                                        .stroke(Stroke::new(1.0, colour))
                                        .corner_radius(10.0);
                                    let tip = texts().format(
                                        &self.language,
                                        if w.sure {
                                            "pick_tip"
                                        } else {
                                            "pick_tip_doubtful"
                                        },
                                        &[&format!("{:.0}", 100.0 * w.p)],
                                    );
                                    if ui.add(b).on_hover_text(tip).clicked() {
                                        self.picks.push(pick);
                                        self.selected = Some(s.id);
                                    }
                                }
                            });
                        }
                    });
                });
            })
            .response;
        let response = response.interact(Sense::click());
        if response.clicked() {
            self.selected = Some(s.id);
        }
        response.context_menu(|ui| {
            if ui.button(self.t("copy")).clicked() {
                ui.ctx().copy_text(s.text());
                ui.close();
            }
        });
        ui.add_space(4.0);
    }
}

/// Committed text (doubtful characters in amber, failed words underlined) and the
/// grey uncommitted guess.
fn lane_text(s: &Station, visuals: &egui::Visuals) -> LayoutJob {
    let font = FontId::monospace(15.0);
    let normal = visuals.strong_text_color();
    let amber = Color32::from_rgb(214, 140, 30);
    let mut job = LayoutJob::default();
    let skip = s.letters.len().saturating_sub(LANE_CHARS);
    for l in &s.letters[skip..] {
        let mut f = TextFormat::simple(font.clone(), if l.doubtful { amber } else { normal });
        if l.suspect {
            f.underline = Stroke::new(1.0, amber);
        }
        job.append(&l.ch.to_string(), 0.0, f);
    }
    if !s.pending.is_empty() {
        let f = TextFormat::simple(font, visuals.weak_text_color());
        job.append(&s.pending, 0.0, f);
    }
    job.wrap.max_rows = 2;
    job.wrap.break_anywhere = true;
    job
}
