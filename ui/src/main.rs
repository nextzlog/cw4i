#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use std::path::PathBuf;
use std::sync::Arc;

use cw4i_ui::app::{self, Prefs};
use cw4i_ui::text::texts;
use cw4i_ui::Cw4i;
use egui::RichText;

fn main() -> eframe::Result {
    let mut files: Vec<PathBuf> = std::env::args_os().skip(1).map(PathBuf::from).collect();
    let play = if files.first().is_some_and(|f| f.as_os_str() == "--play") {
        files.drain(..).nth(1)
    } else {
        None
    };
    if !files.is_empty() {
        for f in &files {
            print_file(f);
        }
        return Ok(());
    }
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([820.0, 600.0])
            .with_min_inner_size([480.0, 360.0])
            .with_drag_and_drop(true)
            .with_title("CW4ISR"),
        ..Default::default()
    };
    eframe::run_native(
        "CW4ISR",
        options,
        Box::new(|cc| {
            install_japanese_font(&cc.egui_ctx);
            Ok(Box::new(App::new(cc, play)))
        }),
    )
}

fn print_file(path: &std::path::Path) {
    match cw4i_ui::audio::read_wav(path) {
        Ok((x, rate)) => {
            println!("{}", path.display());
            for s in cw4i_ui::decode(&x, rate).sorted() {
                if s.confirmed {
                    println!("{:6.0} Hz {:3.0} WPM  {}", s.freq, s.wpm, s.text().trim());
                }
            }
        }
        Err(e) => eprintln!("{}: {e}", path.display()),
    }
}

struct App {
    decoder: Cw4i,
    prefs: Prefs,
    devices: Vec<String>,
    settings_open: bool,
    notice: Option<(String, std::time::Instant)>,
}

impl App {
    fn new(cc: &eframe::CreationContext<'_>, play: Option<PathBuf>) -> Self {
        let get = |k: &str| cc.storage.and_then(|s| s.get_string(k));
        let prefs = Prefs::load(get, system_language);
        cc.egui_ctx.set_theme(prefs.theme);
        let mut decoder = Cw4i::new(&prefs.language);
        decoder.toolbar_items.device = false;
        for (k, _) in decoder.settings() {
            if let Some(v) = get(&format!("cw4i.{k}")).or_else(|| get(k)) {
                decoder.load_setting(k, &v);
            }
        }
        decoder.device = prefs.input.clone();
        if let Some(f) = play {
            decoder.open_file(&f);
        }
        App {
            decoder,
            prefs,
            devices: Vec::new(),
            settings_open: false,
            notice: None,
        }
    }

    fn copy(&mut self, ctx: &egui::Context, text: &str) {
        ctx.copy_text(text.into());
        self.notice = Some((
            texts().format(&self.prefs.language, "copied", &[text]),
            std::time::Instant::now(),
        ));
    }

    fn open_wav(&mut self) {
        if let Some(f) = rfd::FileDialog::new()
            .add_filter("WAV", &["wav", "WAV"])
            .pick_file()
        {
            self.decoder.open_file(&f);
        }
    }

    fn settings_body(&mut self, ui: &mut egui::Ui) {
        if self.devices.is_empty() {
            self.devices = app::sound_cards(cw4i_ui::audio::devices());
        }
        let lang = self.prefs.language.clone();
        let input = texts().get(&lang, "input");
        app::device_field(ui, input, &lang, &mut self.prefs.input, &self.devices);
        app::language_field(ui, &mut self.prefs.language);
        app::theme_field(ui, &lang, &mut self.prefs.theme);
        self.decoder.settings_ui(ui);
    }
}

impl eframe::App for App {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();
        let notice = self
            .notice
            .as_ref()
            .filter(|(_, at)| at.elapsed().as_secs_f32() < 3.0)
            .map(|(n, _)| n.clone());
        if notice.is_some() {
            ctx.request_repaint_after(std::time::Duration::from_millis(200));
        }
        egui::Panel::top("toolbar")
            .frame(
                egui::Frame::side_top_panel(&ctx.global_style())
                    .inner_margin(egui::Margin::symmetric(8, 6)),
            )
            .show(ui, |ui| {
                self.decoder.toolbar(ui, |ui| {
                    if let Some(n) = &notice {
                        let n = RichText::new(n).color(ui.visuals().selection.bg_fill);
                        ui.add(egui::Label::new(n).truncate());
                    }
                });
            });
        if self.settings_open {
            let mut open = true;
            let lang = self.prefs.language.clone();
            app::settings_window(&ctx, &lang, &mut open, |ui| self.settings_body(ui));
            self.settings_open = open;
        }
        if self.decoder.device != self.prefs.input {
            self.decoder.device = self.prefs.input.clone();
            if self.decoder.running() {
                self.decoder.stop();
                self.decoder.start();
            }
        }
        self.decoder.language = self.prefs.language.clone();
        for e in self.decoder.take_events() {
            match e {
                cw4i_ui::Event::OpenWav => self.open_wav(),
                cw4i_ui::Event::Settings => {
                    self.settings_open = !self.settings_open;
                    ctx.request_repaint();
                }
                _ => {}
            }
        }
        egui::CentralPanel::default()
            .frame(egui::Frame::central_panel(&ctx.global_style()).inner_margin(8))
            .show(ui, |ui| {
                self.decoder.body(ui);
            });
        let picks = self.decoder.take_picks();
        if let Some(p) = picks.last() {
            self.copy(&ctx, p.text());
        }
    }

    fn save(&mut self, storage: &mut dyn eframe::Storage) {
        for (k, v) in self.decoder.settings() {
            if k != "device" {
                storage.set_string(&format!("cw4i.{k}"), v);
            }
        }
        self.prefs.save(|k, v| storage.set_string(k, v));
    }
}

fn system_language() -> String {
    let lang = std::env::var("LANG").unwrap_or_default();
    if lang.starts_with("ja") || cfg!(target_os = "windows") && is_japanese_windows() {
        "ja".into()
    } else {
        "en".into()
    }
}

// Windows has no LANG; Japanese Windows ships the Yu Gothic font.
fn is_japanese_windows() -> bool {
    std::path::Path::new("C:\\Windows\\Fonts\\YuGothR.ttc").exists()
        && std::env::var("USERPROFILE").is_ok()
}

// egui has no Japanese glyphs; borrow a system font as a fallback for every family.
fn install_japanese_font(ctx: &egui::Context) {
    const CANDIDATES: &[&str] = &[
        "/System/Library/Fonts/ヒラギノ角ゴシック W3.ttc",
        "/System/Library/Fonts/Hiragino Sans GB.ttc",
        "C:\\Windows\\Fonts\\YuGothR.ttc",
        "C:\\Windows\\Fonts\\meiryo.ttc",
        "C:\\Windows\\Fonts\\msgothic.ttc",
        "/usr/share/fonts/opentype/ipafont-gothic/ipag.ttf",
        "/usr/share/fonts/opentype/noto/NotoSansCJK-Regular.ttc",
        "/usr/share/fonts/noto-cjk/NotoSansCJK-Regular.ttc",
    ];
    let Some(bytes) = CANDIDATES.iter().find_map(|p| std::fs::read(p).ok()) else {
        return;
    };
    let mut fonts = egui::FontDefinitions::default();
    fonts.font_data.insert(
        "japanese".into(),
        Arc::new(egui::FontData::from_owned(bytes)),
    );
    for family in [egui::FontFamily::Proportional, egui::FontFamily::Monospace] {
        fonts
            .families
            .entry(family)
            .or_default()
            .push("japanese".into());
    }
    ctx.set_fonts(fonts);
}
