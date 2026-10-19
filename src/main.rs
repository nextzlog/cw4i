//! The CW4ISR application: the decoder window, where a click on a callsign or a number
//! copies it to the clipboard.
//!
//! Given WAV files as arguments, it decodes them without a window and prints the
//! stations heard; with `--play FILE` it opens the window and plays the file.

#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use std::path::PathBuf;
use std::sync::Arc;

use cw4i::text::texts;
use cw4i::Cw4i;
use egui::RichText;

const DEVICE: &str = "device";
const LANGUAGE: &str = "language";
const THEME: &str = "theme";
const PLACE: &str = "waterfall";

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

/// Decode a file and print one line per station: frequency, speed and text.
fn print_file(path: &std::path::Path) {
    match cw4i::audio::read_wav(path) {
        Ok((x, rate)) => {
            println!("{}", path.display());
            for s in cw4i::decode(&x, rate).sorted() {
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
    dark: bool,
    notice: Option<(String, std::time::Instant)>,
}

impl App {
    fn new(cc: &eframe::CreationContext<'_>, play: Option<PathBuf>) -> Self {
        let get = |k| cc.storage.and_then(|s| s.get_string(k));
        let language = get(LANGUAGE).unwrap_or_else(system_language);
        let dark = get(THEME).map(|t| t != "light").unwrap_or(true);
        let mut decoder = Cw4i::new(&language);
        decoder.device = get(DEVICE).filter(|d| !d.is_empty());
        if let Some(p) = get(PLACE).as_deref().and_then(cw4i::Place::from_name) {
            decoder.place = p;
        }
        if let Some(f) = play {
            decoder.open_file(&f);
        }
        App {
            decoder,
            dark,
            notice: None,
        }
    }

    fn copy(&mut self, ctx: &egui::Context, text: &str) {
        ctx.copy_text(text.into());
        self.notice = Some((
            texts().format(&self.decoder.language, "copied", &[text]),
            std::time::Instant::now(),
        ));
    }
}

impl eframe::App for App {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();
        ctx.set_theme(if self.dark {
            egui::Theme::Dark
        } else {
            egui::Theme::Light
        });
        let language = self.decoder.language.clone();
        let mut dark = self.dark;
        let mut lang = language.clone();
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
                    .inner_margin(egui::Margin::symmetric(10, 6)),
            )
            .show(ui, |ui| {
                self.decoder.toolbar(ui, |ui| {
                    let theme = if dark { "\u{2600}" } else { "\u{263E}" };
                    let tip = texts().get(&language, if dark { "light" } else { "dark" });
                    if ui.button(theme).on_hover_text(tip).clicked() {
                        dark = !dark;
                    }
                    egui::ComboBox::from_id_salt("language")
                        .width(80.0)
                        .selected_text(if lang == "ja" { "日本語" } else { "English" })
                        .show_ui(ui, |ui| {
                            ui.selectable_value(&mut lang, "ja".into(), "日本語");
                            ui.selectable_value(&mut lang, "en".into(), "English");
                        });
                    if let Some(n) = &notice {
                        ui.label(RichText::new(n).color(ui.visuals().selection.bg_fill));
                    }
                });
            });
        self.dark = dark;
        self.decoder.language = lang;
        egui::CentralPanel::default()
            .frame(egui::Frame::central_panel(&ctx.global_style()).inner_margin(10))
            .show(ui, |ui| {
                self.decoder.body(ui);
            });
        let picks = self.decoder.take_picks();
        if let Some(p) = picks.last() {
            self.copy(&ctx, p.text());
        }
    }

    fn save(&mut self, storage: &mut dyn eframe::Storage) {
        storage.set_string(DEVICE, self.decoder.device.clone().unwrap_or_default());
        storage.set_string(LANGUAGE, self.decoder.language.clone());
        storage.set_string(PLACE, self.decoder.place.name().into());
        storage.set_string(THEME, if self.dark { "dark" } else { "light" }.into());
    }
}

/// "ja" when the system speaks Japanese, else "en".
fn system_language() -> String {
    let lang = std::env::var("LANG").unwrap_or_default();
    if lang.starts_with("ja") || cfg!(target_os = "windows") && is_japanese_windows() {
        "ja".into()
    } else {
        "en".into()
    }
}

/// Windows has no LANG; Japanese Windows ships the Yu Gothic font.
fn is_japanese_windows() -> bool {
    std::path::Path::new("C:\\Windows\\Fonts\\YuGothR.ttc").exists()
        && std::env::var("USERPROFILE").is_ok()
}

/// egui has no Japanese glyphs; borrow a system font as a fallback for every family.
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
