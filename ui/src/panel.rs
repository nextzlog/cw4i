use std::path::Path;

use egui::text::{LayoutJob, TextFormat};
use egui::{Color32, FontId, Rect, RichText, Sense, Stroke, Ui};

use crate::audio::{self, Device, HostInput, Source, WavFile};
use crate::contact::Contact;
use crate::entry::{CutTable, Pick};
use crate::event::Event;
use crate::icon;
use crate::station::{Station, Stations, WordKind};
use crate::text::texts;
use crate::waterfall::{Waterfall, HI_HZ, LO_HZ};

const KEEP_SEC: u64 = 120;

const EVENTS_KEPT: usize = 256;

type Handler = Box<dyn FnMut(&Event)>;

const LANE_CHARS: usize = 120;

const LANE_ROWS: usize = 2;

const NARROW: f32 = 360.0;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Place {
    Left,
    Right,
    Top,
}

impl Place {
    pub fn newest_last(self, reverse: bool) -> bool {
        matches!(self, Place::Left) != reverse
    }

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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ToolbarItems {
    pub start: bool,
    pub wav: bool,
    pub device: bool,
    pub clear: bool,
    pub waterfall: bool,
    pub status: bool,
    pub settings: bool,
}

impl ToolbarItems {
    pub const ALL: ToolbarItems = ToolbarItems {
        start: true,
        wav: true,
        device: true,
        clear: true,
        waterfall: true,
        status: true,
        settings: true,
    };

    pub const NONE: ToolbarItems = ToolbarItems {
        start: false,
        wav: false,
        device: false,
        clear: false,
        waterfall: false,
        status: false,
        settings: false,
    };
}

impl Default for ToolbarItems {
    fn default() -> Self {
        ToolbarItems::ALL
    }
}

pub struct Cw4i {
    pub language: String,
    pub toolbar_items: ToolbarItems,
    pub device: Option<String>,
    pub show_candidates: bool,
    pub place: Place,
    pub host_input: bool,
    pub host_running: bool,
    pub host_call: bool,
    pub my_call: String,
    pub reverse_flow: bool,
    pub empty_hint: Option<String>,
    devices: Vec<String>,
    source: Option<Source>,
    decoder: Option<cw4i::Decoder>,
    stations: Stations,
    waterfall: Option<Waterfall>,
    texture: Option<(egui::TextureHandle, u64, bool, (bool, bool))>,
    selected: Option<u32>,
    picks: Vec<Pick>,
    events: Vec<Event>,
    handlers: Vec<Handler>,
    present: std::collections::BTreeMap<u32, Option<String>>,
    cut: CutTable,
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
            toolbar_items: ToolbarItems::ALL,
            device: None,
            show_candidates: false,
            place: Place::Top,
            host_input: false,
            host_running: false,
            host_call: false,
            my_call: String::new(),
            reverse_flow: false,
            empty_hint: None,
            devices: Vec::new(),
            source: None,
            decoder: None,
            stations: Stations::default(),
            waterfall: None,
            texture: None,
            selected: None,
            picks: Vec::new(),
            events: Vec::new(),
            handlers: Vec::new(),
            present: Default::default(),
            cut: CutTable::default(),
            error: None,
            samples: 0,
        }
    }

    pub fn settings(&self) -> Vec<(&'static str, String)> {
        vec![
            ("device", self.device.clone().unwrap_or_default()),
            ("waterfall", self.place.name().into()),
            ("candidates", self.show_candidates.to_string()),
            ("my_call", self.my_call.clone()),
            ("reverse_flow", self.reverse_flow.to_string()),
        ]
    }

    pub fn load_setting(&mut self, key: &str, value: &str) {
        match key {
            "device" => self.device = Some(value.to_string()).filter(|d| !d.is_empty()),
            "waterfall" => self.place = Place::from_name(value).unwrap_or(self.place),
            "candidates" => self.show_candidates = value == "true",
            "reverse_flow" => self.reverse_flow = value == "true",
            "my_call" => self.my_call = value.trim().to_uppercase(),
            _ => {}
        }
    }

    pub fn set_cut_table(&mut self, table: CutTable) {
        self.cut = table;
    }

    pub fn cut_table(&self) -> &CutTable {
        &self.cut
    }

    fn is_mine(&self, call: &str) -> bool {
        !self.my_call.is_empty() && call.eq_ignore_ascii_case(&self.my_call)
    }

    fn t(&self, key: &'static str) -> &'static str {
        texts().get(&self.language, key)
    }

    fn begin(&mut self, source: Result<Source, String>) {
        match source {
            Ok(s) => {
                let rate = s.sample_rate();
                self.decoder = Some(cw4i::Decoder::new(rate));
                self.waterfall = Some(Waterfall::new(rate));
                self.stations.clear();
                self.samples = 0;
                self.source = Some(s);
                self.error = None;
            }
            Err(e) => self.error = Some(e),
        }
    }

    pub fn start(&mut self) {
        let d = Device::open(self.device.as_deref()).map(Source::Device);
        self.begin(d);
    }

    pub fn open_file(&mut self, path: &Path) {
        let f = WavFile::open(path).map(Source::File);
        self.begin(f);
    }

    pub fn push_samples(&mut self, name: &str, rate: u32, samples: &[f32]) {
        let same = matches!(&self.source, Some(Source::Host(h)) if h.name == name && h.sample_rate == rate);
        if !same {
            self.begin(Ok(Source::Host(HostInput {
                name: name.into(),
                sample_rate: rate,
                buffer: Vec::new(),
            })));
        }
        if let Some(Source::Host(h)) = self.source.as_mut() {
            h.buffer.extend_from_slice(samples);
        }
    }

    pub fn stop(&mut self) {
        self.flush();
        self.source = None;
    }

    pub fn running(&self) -> bool {
        self.source.is_some()
    }

    pub fn clear(&mut self) {
        if let Some(rate) = self.source.as_ref().map(Source::sample_rate) {
            self.decoder = Some(cw4i::Decoder::new(rate));
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
        self.follow_stations();
        if finished {
            self.stop();
        }
    }

    pub fn take_picks(&mut self) -> Vec<Pick> {
        std::mem::take(&mut self.picks)
    }

    pub fn on_event(&mut self, handler: impl FnMut(&Event) + 'static) {
        self.handlers.push(Box::new(handler));
    }

    pub fn take_events(&mut self) -> Vec<Event> {
        std::mem::take(&mut self.events)
    }

    fn emit(&mut self, event: Event) {
        for h in &mut self.handlers {
            h(&event);
        }
        if self.events.len() >= EVENTS_KEPT {
            self.events.remove(0);
        }
        self.events.push(event);
    }

    fn pick(&mut self, pick: Pick) {
        self.emit(Event::pick(&pick));
        self.picks.push(pick);
    }

    pub fn contact(&self, station: u32) -> Option<Contact> {
        self.stations
            .sorted()
            .into_iter()
            .find(|s| s.id == station)
            .map(|s| Contact::with_cut(s, &self.my_call, &self.cut))
    }

    pub fn pick_station(&mut self, station: u32) {
        if let Some(c) = self.contact(station) {
            self.selected = Some(station);
            self.emit(Event::Contact(c));
        }
    }

    fn follow_stations(&mut self) {
        let now: Vec<(u32, f64, Option<String>)> = self
            .stations
            .sorted()
            .into_iter()
            .filter(|s| s.confirmed && s.alive)
            .map(|s| (s.id, s.freq, s.callsign().map(str::to_string)))
            .collect();
        for (id, freq, call) in &now {
            if !self.present.contains_key(id) {
                self.emit(Event::Appeared {
                    station: *id,
                    freq: *freq,
                });
            }
            self.present.insert(*id, call.clone());
        }
        let lost: Vec<(u32, Option<String>)> = self
            .present
            .iter()
            .filter(|(id, _)| !now.iter().any(|(n, _, _)| n == *id))
            .map(|(id, c)| (*id, c.clone()))
            .collect();
        for (station, callsign) in lost {
            self.present.remove(&station);
            self.emit(Event::Lost { station, callsign });
        }
    }

    pub fn stations(&self) -> Vec<&Station> {
        self.stations.sorted()
    }

    pub fn ui(&mut self, ui: &mut Ui) {
        self.toolbar(ui, |_| {});
        ui.add_space(6.0);
        self.body(ui);
    }

    pub fn toolbar(&mut self, ui: &mut Ui, extra: impl FnOnce(&mut Ui)) {
        let row = egui::vec2(ui.available_width(), ui.spacing().interact_size.y);
        let right = egui::Layout::right_to_left(egui::Align::Min);
        ui.allocate_ui_with_layout(row, right, |ui| {
            ui.spacing_mut().item_spacing.x = 6.0;
            if self.toolbar_items.settings
                && icon::button(ui, icon::GEAR, self.t("settings"), false).clicked()
            {
                self.emit(Event::Settings);
            }
            extra(ui);
            let wrap = egui::Layout::left_to_right(egui::Align::Center).with_main_wrap(true);
            let row = egui::vec2(ui.available_width(), ui.spacing().interact_size.y);
            ui.allocate_ui_with_layout(row, wrap, |ui| {
                ui.set_row_height(row.y);
                self.tools(ui)
            });
        });
    }

    pub fn tools(&mut self, ui: &mut Ui) {
        let items = if self.host_input {
            ToolbarItems {
                device: false,
                ..self.toolbar_items
            }
        } else {
            self.toolbar_items
        };
        if items.device && self.devices.is_empty() {
            self.devices = audio::devices();
        }
        if items.start {
            let running = if self.host_input {
                self.host_running
            } else {
                self.running()
            };
            let (g, label) = if running {
                (icon::STOP, self.t("stop"))
            } else {
                (icon::PLAY, self.t("start"))
            };
            if icon::button(ui, g, label, running).clicked() {
                match (self.host_input, running) {
                    (true, true) => self.emit(Event::Stop),
                    (true, false) => self.emit(Event::Start),
                    (false, true) => self.stop(),
                    (false, false) => self.start(),
                }
            }
        }
        if items.wav {
            if !self.host_input && matches!(self.source, Some(Source::File(_))) {
                if icon::button(ui, icon::STOP, self.t("stop_wav"), false).clicked() {
                    self.stop();
                }
            } else if icon::button(ui, icon::FOLDER_OPEN, self.t("open_wav"), false).clicked() {
                self.emit(Event::OpenWav);
            }
        }
        if items.device {
            let shown = self
                .device
                .clone()
                .unwrap_or_else(|| self.t("default_device").into());
            let default = self.t("default_device");
            egui::ComboBox::from_id_salt("cw4i_device")
                .selected_text(shown)
                .width(140.0)
                .show_ui(ui, |ui| {
                    ui.selectable_value(&mut self.device, None, default);
                    for d in &self.devices {
                        ui.selectable_value(&mut self.device, Some(d.clone()), d);
                    }
                })
                .response
                .on_hover_text(self.t("device"));
        }
        if items.clear && ui.button(self.t("reset")).clicked() {
            self.clear();
        }
        if items.waterfall {
            let (left, top, right) = (
                self.t("place_left"),
                self.t("place_top"),
                self.t("place_right"),
            );
            icon::install(ui.ctx());
            for (place, g, tip) in [
                (Place::Left, icon::SIDEBAR_SIMPLE, left),
                (Place::Top, icon::SQUARE_SPLIT_VERTICAL, top),
                (Place::Right, icon::SQUARE_HALF, right),
            ] {
                let r = ui.selectable_value(&mut self.place, place, icon::glyph(g));
                icon::labelled(r, tip, self.place == place).on_hover_text(tip);
            }
            let flow = self.t("reverse_flow");
            let r = ui.toggle_value(&mut self.reverse_flow, icon::glyph(icon::ARROWS_LEFT_RIGHT));
            icon::labelled(r, flow, self.reverse_flow).on_hover_text(flow);
        }
        if items.status {
            ui.separator();
            let (dot, state) = match &self.source {
                Some(s) => (Color32::from_rgb(220, 70, 70), s.name().to_string()),
                None => (ui.visuals().weak_text_color(), self.t("not_running").into()),
            };
            icon::install(ui.ctx());
            ui.label(icon::glyph(icon::CIRCLE).color(dot));
            ui.add(egui::Label::new(RichText::new(state).weak()).truncate());
        }
    }

    pub fn settings_ui(&mut self, ui: &mut Ui) {
        if !self.host_call {
            let label = self.t("my_call");
            crate::app::field(ui, label, |ui| {
                let edit = egui::TextEdit::singleline(&mut self.my_call)
                    .char_limit(12)
                    .font(egui::TextStyle::Monospace);
                ui.add(edit)
            });
            self.my_call = self.my_call.trim().to_uppercase();
        }
        let label = self.t("show_candidates");
        ui.checkbox(&mut self.show_candidates, label);
    }

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
        let shape = (vertical, self.place.newest_last(self.reverse_flow));
        let dark = ui.visuals().dark_mode;
        let size = ui.available_size();
        let (rect, response) = ui.allocate_exact_size(size, Sense::click());
        let painter = ui.painter_at(rect);
        painter.rect_filled(rect, 6.0, ui.visuals().extreme_bg_color);
        if let Some(w) = &self.waterfall {
            let stale = match &self.texture {
                Some((_, v, d, o)) => *v != w.version || *d != dark || *o != shape,
                None => true,
            };
            if stale {
                let image = if vertical {
                    w.image_vertical(dark, shape.1)
                } else {
                    w.image(dark, shape.1)
                };
                match self.texture.as_mut() {
                    Some((t, v, d, o)) => {
                        t.set(image, egui::TextureOptions::LINEAR);
                        (*v, *d, *o) = (w.version, dark, shape);
                    }
                    None => {
                        let t = ui.ctx().load_texture(
                            "cw4i_waterfall",
                            image,
                            egui::TextureOptions::LINEAR,
                        );
                        self.texture = Some((t, w.version, dark, shape));
                    }
                }
            }
            if let Some((t, _, _, _)) = &self.texture {
                let uv = Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0));
                painter.image(t.id(), rect, uv, Color32::WHITE);
            }
        }
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
            let ink = if self.is_mine(&name) {
                mine(ui.visuals())
            } else {
                strong
            };
            let g = painter.layout_no_wrap(name, FontId::monospace(11.0), ink);
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
            let hint = match &self.source {
                Some(Source::File(_)) => self.t("decoding"),
                Some(_) => self.t("waiting"),
                None => self.empty_hint.as_deref().unwrap_or(self.t("idle")),
            };
            ui.weak(hint);
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
        let mine_call = s.callsign().is_some_and(|c| self.is_mine(c));
        let accent = visuals.selection.bg_fill;
        let amber = amber(&visuals);
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
                } else if mine_call {
                    mine(&visuals)
                } else {
                    visuals.widgets.noninteractive.bg_stroke.color
                },
            ))
            .corner_radius(4.0)
            .inner_margin(egui::Margin::symmetric(8, 4));
        let tip = self.t("contact_tip");
        let mut contact = false;
        let response = frame
            .show(ui, |ui| {
                ui.set_width(ui.available_width());
                let mut header = |ui: &mut Ui| {
                    let head = match s.callsign() {
                        Some(c) => {
                            let t = RichText::new(c).monospace().size(15.0).strong();
                            if mine_call {
                                t.color(mine(&visuals))
                            } else {
                                t
                            }
                        }
                        None => RichText::new(format!("{:.0} Hz", s.freq))
                            .monospace()
                            .size(15.0),
                    };
                    ui.horizontal(|ui| {
                        let head = if s.alive { head } else { head.weak() };
                        let clicked = ui
                            .add(egui::Label::new(head).sense(Sense::click()))
                            .on_hover_text(tip)
                            .clicked();
                        let button = egui::Button::new(RichText::new("\u{00BB}").monospace())
                            .small()
                            .corner_radius(8.0);
                        if clicked || ui.add(button).on_hover_text(tip).clicked() {
                            contact = true;
                        }
                    });
                    let meta = format!(
                        "{:.0} Hz \u{00B7} {:.0} WPM \u{00B7} {:+.0} dB",
                        s.freq, s.wpm, s.snr
                    );
                    ui.label(RichText::new(meta).small().weak());
                };
                let mut body = |ui: &mut Ui| {
                    let font = FontId::monospace(15.0);
                    let glyph = ui.fonts_mut(|f| f.glyph_width(&font, 'M')).max(1.0);
                    let fit = lane_fit(ui.available_width(), glyph);
                    ui.add(egui::Label::new(lane_text(s, &visuals, fit)).wrap());
                    self.chips(ui, s, amber);
                };
                if stacked(ui.available_width()) {
                    ui.horizontal_wrapped(header);
                    body(ui);
                } else {
                    ui.horizontal_top(|ui| {
                        ui.allocate_ui_with_layout(
                            egui::vec2(120.0, 36.0),
                            egui::Layout::top_down(egui::Align::Min),
                            |ui| {
                                ui.set_width(120.0);
                                header(ui);
                            },
                        );
                        ui.vertical(body);
                    });
                }
            })
            .response;
        if contact {
            self.pick_station(s.id);
        }
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
        ui.add_space(3.0);
    }

    fn chips(&mut self, ui: &mut Ui, s: &Station, amber: Color32) {
        let picks: Vec<(Pick, &crate::station::Word)> = s
            .words
            .iter()
            .filter_map(|w| self.cut.pick(w).map(|p| (p, w)))
            .collect();
        if picks.is_empty() {
            return;
        }
        let accent = ui.visuals().selection.bg_fill;
        let mine_colour = mine(ui.visuals());
        ui.horizontal_wrapped(|ui| {
            ui.spacing_mut().item_spacing.x = 4.0;
            for (pick, w) in picks {
                let mine = matches!(&pick, Pick::Callsign(c) if self.is_mine(c));
                let mut text = RichText::new(pick.text()).monospace();
                if w.kind == WordKind::Callsign {
                    text = text.strong();
                }
                let colour = if mine {
                    mine_colour
                } else if w.sure {
                    accent
                } else {
                    amber
                };
                let b = egui::Button::new(text)
                    .fill(colour.gamma_multiply(0.18))
                    .stroke(Stroke::new(1.0, colour))
                    .corner_radius(8.0);
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
                    self.pick(pick);
                    self.selected = Some(s.id);
                }
            }
        });
    }
}

fn stacked(width: f32) -> bool {
    width < NARROW
}

fn lane_fit(width: f32, glyph: f32) -> usize {
    let per_row = ((width / glyph).floor() as usize).max(1);
    (per_row * LANE_ROWS).min(LANE_CHARS)
}

fn amber(visuals: &egui::Visuals) -> Color32 {
    if visuals.dark_mode {
        Color32::from_rgb(230, 160, 50)
    } else {
        Color32::from_rgb(170, 95, 0)
    }
}

fn mine(visuals: &egui::Visuals) -> Color32 {
    if visuals.dark_mode {
        Color32::from_rgb(110, 210, 120)
    } else {
        Color32::from_rgb(20, 130, 40)
    }
}

fn lane_text(s: &Station, visuals: &egui::Visuals, fit: usize) -> LayoutJob {
    let font = FontId::monospace(15.0);
    let normal = visuals.strong_text_color();
    let amber = amber(visuals);
    let mut job = LayoutJob::default();
    let pending = s.pending.chars().count().min(fit);
    let skip = s.letters.len().saturating_sub(fit - pending);
    for l in &s.letters[skip..] {
        let mut f = TextFormat::simple(font.clone(), if l.doubtful { amber } else { normal });
        if l.suspect {
            f.underline = Stroke::new(1.0, amber);
        }
        job.append(&l.ch.to_string(), 0.0, f);
    }
    if pending > 0 {
        let tail: String = s
            .pending
            .chars()
            .skip(s.pending.chars().count() - pending)
            .collect();
        let f = TextFormat::simple(font, visuals.weak_text_color());
        job.append(&tail, 0.0, f);
    }
    job.wrap.max_rows = LANE_ROWS;
    job.wrap.break_anywhere = true;
    job
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_cut_table_can_be_set() {
        let mut p = Cw4i::new("en");
        assert_eq!(p.cut_table(), &CutTable::default());
        p.set_cut_table(CutTable::new([('O', '0')]));
        assert_eq!(p.cut_table().pairs(), [('O', '0')]);
    }

    #[test]
    fn a_clicked_station_reaches_the_handlers_as_a_contact() {
        let rate = 8000;
        let audio = crate::tests::morse("TU JA1ZLO 5NN 1317M", 700.0, 25.0, rate);
        let mut p = Cw4i::new("en");
        p.my_call = "JA1YWX".into();
        let seen = std::rc::Rc::new(std::cell::RefCell::new(Vec::new()));
        let log = seen.clone();
        p.on_event(move |e| log.borrow_mut().push(e.clone()));
        p.push_samples("test", rate, &audio);
        p.process();
        p.push_samples("test", rate, &[0.0; 16000]);
        p.process();
        let id = p
            .stations()
            .iter()
            .find(|s| s.text().contains("JA1ZLO"))
            .map(|s| s.id)
            .expect("decoded");
        p.pick_station(id);
        let events = p.take_events();
        assert!(events
            .iter()
            .any(|e| matches!(e, Event::Appeared { station, .. } if *station == id)));
        let Some(Event::Contact(c)) = seen.borrow().last().cloned() else {
            panic!("{:?}", seen.borrow());
        };
        assert_eq!(c.callsign.as_deref(), Some("JA1ZLO"));
        assert_eq!(c.report.as_deref(), Some("599"));
        assert_eq!(c.exchange, ["1317M"]);
        assert!((c.freq - 700.0).abs() < 20.0 && c.wpm > 15.0 && c.p > 0.0);
        assert_eq!(events.len(), seen.borrow().len());
        p.pick(Pick::Number("13M".into()));
        assert_eq!(p.take_picks(), [Pick::Number("13M".into())]);
        assert_eq!(
            p.take_events(),
            [Event::Pick {
                text: "13M".into(),
                kind: WordKind::Exchange
            }]
        );
    }

    fn station(text: &str, pending: &str) -> Station {
        let mut st = Stations::default();
        st.apply(cw4i::Event::TrackConfirmed {
            track: 1,
            sample: 0,
            freq: 700.0,
        });
        let mut s = st.sorted()[0].clone();
        s.letters = text
            .chars()
            .map(|ch| crate::station::Letter {
                ch,
                doubtful: false,
                suspect: false,
            })
            .collect();
        s.pending = pending.into();
        s
    }

    fn shown(job: &LayoutJob) -> String {
        job.text.clone()
    }

    #[test]
    fn the_newest_text_is_never_cut_off() {
        let s = station("CQ TEST JA1ZLO JA1ZLO TEST", "K");
        let job = lane_text(&s, &egui::Visuals::dark(), 10);
        assert_eq!(shown(&job), "1ZLO TESTK");
        assert_eq!(
            shown(&lane_text(&s, &egui::Visuals::light(), 100)),
            "CQ TEST JA1ZLO JA1ZLO TESTK"
        );
    }

    #[test]
    fn lanes_fit_two_rows_and_stack_when_narrow() {
        assert_eq!(lane_fit(100.0, 10.0), 20);
        assert_eq!(lane_fit(1.0, 10.0), 2);
        assert_eq!(lane_fit(10000.0, 9.0), LANE_CHARS);
        assert!(stacked(260.0));
        assert!(!stacked(600.0));
    }

    #[test]
    fn settings_round_trip_and_my_call_matches_any_case() {
        let mut a = Cw4i::new("en");
        a.place = Place::Right;
        a.show_candidates = true;
        a.load_setting("my_call", " ja1zlo ");
        a.device = Some("Mic".into());
        let mut b = Cw4i::new("en");
        for (k, v) in a.settings() {
            b.load_setting(k, &v);
        }
        assert_eq!(b.place, Place::Right);
        assert!(b.show_candidates);
        assert_eq!(b.device.as_deref(), Some("Mic"));
        assert!(b.is_mine("JA1ZLO") && b.is_mine("ja1zlo"));
        assert!(!b.is_mine("JA1ZLP"));
        assert!(!Cw4i::new("en").is_mine(""));
        assert!(!b.reverse_flow);
        a.reverse_flow = true;
        for (k, v) in a.settings() {
            b.load_setting(k, &v);
        }
        assert!(b.reverse_flow);
        b.load_setting("waterfall", "bogus");
        assert_eq!(b.place, Place::Right);
    }

    #[test]
    fn the_newest_data_faces_the_lanes_unless_reversed() {
        assert!(Place::Left.newest_last(false));
        assert!(!Place::Right.newest_last(false));
        assert!(!Place::Top.newest_last(false));
        assert!(!Place::Left.newest_last(true));
        assert!(Place::Right.newest_last(true));
        assert!(Place::Top.newest_last(true));
    }

    #[test]
    fn my_call_is_readable_in_both_themes() {
        let d = mine(&egui::Visuals::dark());
        let l = mine(&egui::Visuals::light());
        assert!(d.g() > 150 && l.g() < 150);
        assert_ne!(
            amber(&egui::Visuals::dark()),
            amber(&egui::Visuals::light())
        );
    }
}
