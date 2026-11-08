use egui::{Align, Layout, Response, ThemePreference, Ui};

use crate::text::texts;

pub const LANGUAGES: [(&str, &str); 2] = [("en", "English"), ("ja", "日本語")];

#[derive(Clone, Debug, PartialEq)]
pub struct Prefs {
    pub language: String,
    pub theme: ThemePreference,
    pub input: Option<String>,
}

impl Prefs {
    pub fn load(get: impl Fn(&str) -> Option<String>, language: impl FnOnce() -> String) -> Self {
        let device = |k: &str| get(k).filter(|v| !v.is_empty());
        Prefs {
            language: get("app.language")
                .or_else(|| get("language"))
                .unwrap_or_else(language),
            theme: match get("app.theme").or_else(|| get("theme")).as_deref() {
                Some("light") => ThemePreference::Light,
                Some("dark") => ThemePreference::Dark,
                _ => ThemePreference::System,
            },
            input: device("app.input").or_else(|| device("device")),
        }
    }

    pub fn save(&self, mut set: impl FnMut(&str, String)) {
        let theme = match self.theme {
            ThemePreference::Light => "light",
            ThemePreference::Dark => "dark",
            ThemePreference::System => "system",
        };
        set("app.language", self.language.clone());
        set("app.theme", theme.into());
        set("app.input", self.input.clone().unwrap_or_default());
    }
}

const LABEL_WIDTH: f32 = 96.0;

pub fn field(ui: &mut Ui, label: &str, add: impl FnOnce(&mut Ui) -> Response) -> Response {
    ui.horizontal(|ui| {
        let width = LABEL_WIDTH.min(ui.available_width() * 0.35);
        let h = ui.spacing().interact_size.y;
        let id = ui
            .allocate_ui_with_layout(
                egui::vec2(width, h),
                Layout::left_to_right(Align::Center),
                |ui| {
                    ui.set_min_width(width);
                    ui.add(egui::Label::new(label).truncate()).id
                },
            )
            .inner;
        let rest = ui.available_width();
        ui.spacing_mut().text_edit_width = rest;
        ui.spacing_mut().combo_width = rest;
        add(ui).labelled_by(id)
    })
    .inner
}

pub fn sound_cards(names: impl IntoIterator<Item = String>) -> Vec<String> {
    let mut cards: Vec<String> = Vec::new();
    for n in names {
        let l = n.to_lowercase();
        let virtual_device = l.is_empty()
            || l == "default"
            || ["loopback", "monitor", "simulat"]
                .iter()
                .any(|w| l.contains(w));
        if !virtual_device && !cards.contains(&n) {
            cards.push(n);
        }
    }
    cards
}

pub fn settings_window(
    ctx: &egui::Context,
    language: &str,
    open: &mut bool,
    body: impl FnOnce(&mut Ui),
) {
    egui::Window::new(texts().get(language, "settings"))
        .open(open)
        .default_width(360.0)
        .resizable(true)
        .collapsible(false)
        .show(ctx, |ui| {
            egui::ScrollArea::vertical().show(ui, body);
        });
}

pub fn device_field(
    ui: &mut Ui,
    label: &str,
    language: &str,
    device: &mut Option<String>,
    cards: &[String],
) -> Response {
    let default = texts().get(language, "default_device");
    field(ui, label, |ui| {
        egui::ComboBox::from_id_salt(("app-device", label))
            .selected_text(device.clone().unwrap_or(default.into()))
            .width(ui.available_width())
            .truncate()
            .show_ui(ui, |ui| {
                ui.selectable_value(device, None, default);
                for d in cards {
                    ui.selectable_value(device, Some(d.clone()), d);
                }
            })
            .response
    })
}

pub fn theme_field(ui: &mut Ui, language: &str, theme: &mut ThemePreference) -> Response {
    let t = |k| texts().get(language, k);
    let name = |p: ThemePreference| match p {
        ThemePreference::System => t("theme.system"),
        ThemePreference::Light => t("light"),
        ThemePreference::Dark => t("dark"),
    };
    let before = *theme;
    let r = field(ui, t("theme"), |ui| {
        egui::ComboBox::from_id_salt("app-theme")
            .selected_text(name(*theme))
            .width(ui.available_width())
            .show_ui(ui, |ui| {
                for p in [
                    ThemePreference::System,
                    ThemePreference::Light,
                    ThemePreference::Dark,
                ] {
                    ui.selectable_value(theme, p, name(p));
                }
            })
            .response
    });
    if *theme != before {
        ui.ctx().set_theme(*theme);
    }
    r
}

pub fn language_field(ui: &mut Ui, language: &mut String) -> Response {
    let label = texts().get(language.as_str(), "language");
    let shown = LANGUAGES
        .iter()
        .find(|(c, _)| *c == language.as_str())
        .map_or(LANGUAGES[0].1, |(_, n)| *n);
    field(ui, label, |ui| {
        egui::ComboBox::from_id_salt("app-language")
            .selected_text(shown)
            .width(ui.available_width())
            .show_ui(ui, |ui| {
                for (code, name) in LANGUAGES {
                    ui.selectable_value(language, code.to_string(), name);
                }
            })
            .response
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    #[test]
    fn prefs_round_trip_and_follow_the_system_theme_by_default() {
        let fresh = Prefs::load(|_| None, || "ja".into());
        assert_eq!(fresh.theme, ThemePreference::System);
        assert_eq!(
            (fresh.language.as_str(), fresh.input.as_ref()),
            ("ja", None)
        );
        let mut p = fresh.clone();
        p.theme = ThemePreference::Light;
        p.input = Some("USB Audio".into());
        let mut store = HashMap::new();
        p.save(|k, v| {
            store.insert(k.to_string(), v);
        });
        assert_eq!(Prefs::load(|k| store.get(k).cloned(), || "en".into()), p);
        let old = HashMap::from([
            ("language", "ja"),
            ("theme", "dark"),
            ("device", "USB Audio"),
        ]);
        let old = Prefs::load(|k| old.get(k).map(|v| v.to_string()), || "en".into());
        assert_eq!(old.language, "ja");
        assert_eq!(old.theme, ThemePreference::Dark);
        assert_eq!(old.input.as_deref(), Some("USB Audio"));
    }

    #[test]
    fn only_sound_cards_are_listed() {
        let names = [
            "default",
            "USB Audio",
            "Monitor of USB Audio",
            "Loopback",
            "USB Audio",
        ];
        assert_eq!(sound_cards(names.map(String::from)), ["USB Audio"]);
    }
}
