use std::collections::HashMap;
use std::sync::Arc;

use egui::accesskit::Role;
use egui::ThemePreference;
use egui_kittest::kittest::{NodeT, Queryable};
use egui_kittest::{Harness, HarnessBuilder};
use egui_system_font_provider::SystemFontProvider;

use cw4i_ui::app::{self, Prefs};
use cw4i_ui::{Cw4i, Event};

const START: &str = "Start";
const OPEN_WAV: &str = "Open…";
const SETTINGS: &str = "Settings";
const PLACES: [&str; 4] = [
    "Waterfall at the left",
    "Waterfall at the top",
    "Waterfall at the right",
    "Reverse the waterfall flow",
];

fn builder<S>(width: f32, height: f32) -> HarnessBuilder<S> {
    Harness::builder()
        .with_size([width, height])
        .with_context_setup(|ctx| ctx.add_font_provider(Arc::new(SystemFontProvider::new())))
}

fn panel(width: f32) -> Harness<'static, Cw4i> {
    let mut panel = Cw4i::new("en");
    panel.toolbar_items.device = false;
    let mut h = builder(width, 400.0).build_ui_state(
        |ui, p: &mut Cw4i| {
            p.toolbar(ui, |_| {});
            p.body(ui);
        },
        panel,
    );
    h.run_steps(3);
    h
}

fn before(a: egui::Rect, b: egui::Rect) -> bool {
    let same_row = (a.center().y - b.center().y).abs() < 2.0;
    (same_row && a.max.x <= b.min.x) || a.max.y <= b.min.y + 1.0
}

#[test]
fn the_icon_toolbar_keeps_its_order_and_fits_every_width() {
    for w in [280.0, 360.0, 600.0, 820.0] {
        let mut h = panel(w);
        let mut labels = vec![START, OPEN_WAV, "Clear"];
        labels.extend(PLACES);
        labels.push(cw4i_ui::icon::CIRCLE);
        let rects: Vec<egui::Rect> = labels.iter().map(|l| h.get_by_label(l).rect()).collect();
        for (r, l) in rects.windows(2).zip(labels.windows(2)) {
            assert!(before(r[0], r[1]), "{l:?} at {w}");
        }
        let settings = h.get_by_label(SETTINGS).rect();
        assert!(
            settings.max.x > w - 12.0 && settings.min.y < 20.0,
            "{settings:?} at {w}"
        );
        assert!(rects[0].min.x < 12.0 && rects[0].max.x <= settings.min.x);
        for c in h.query_all_by(|n| {
            matches!(
                n.role(),
                Role::Button | Role::TextInput | Role::CheckBox | Role::ComboBox
            )
        }) {
            let r = c.rect();
            assert!(
                r.min.x >= -0.5 && r.max.x <= w + 0.5,
                "{:?} {r:?} at {w}",
                c.accesskit_node().label()
            );
        }
        for gone in ["Light", "Dark", "Show candidates", "My call"] {
            assert!(h.query_by_label(gone).is_none(), "{gone} at {w}");
        }
        assert!(h.query_by_role(Role::ComboBox).is_none(), "no device combo");
        let hint = h
            .get_by_label("Press Start, open a WAV file, or drop one here.")
            .rect();
        let bottom = rects.iter().fold(settings.max.y, |b, r| b.max(r.max.y));
        assert!(
            hint.min.y >= bottom,
            "the body starts below the toolbar at {w}"
        );
        h.get_by_label(OPEN_WAV).click();
        h.get_by_label(SETTINGS).click();
        h.run_steps(2);
        let events = h.state_mut().take_events();
        assert!(events.contains(&Event::OpenWav) && events.contains(&Event::Settings));
    }
}

#[test]
fn a_host_can_hide_the_settings_button_and_draw_the_settings_itself() {
    let mut p = Cw4i::new("en");
    p.toolbar_items.settings = false;
    p.host_call = true;
    let mut h = builder(600.0, 300.0).build_ui_state(
        |ui, p: &mut Cw4i| {
            p.toolbar(ui, |_| {});
            p.settings_ui(ui);
        },
        p,
    );
    h.run_steps(2);
    assert!(h.query_by_label(SETTINGS).is_none());
    assert!(h.query_by_label("My call").is_none());
    h.get_by_label("Show candidates").click();
    h.run_steps(2);
    assert!(h.state().show_candidates);
}

#[test]
fn the_theme_is_a_setting_and_persists() {
    let mut h = builder(600.0, 300.0).build_ui_state(
        |ui, p: &mut Prefs| {
            let lang = p.language.clone();
            app::theme_field(ui, &lang, &mut p.theme);
        },
        Prefs::load(|_| None, || "en".into()),
    );
    h.run_steps(2);
    assert_eq!(h.state().theme, ThemePreference::System);
    h.get_by_label("Theme").click();
    h.run_steps(2);
    h.get_by_label("Light").click();
    h.run_steps(3);
    assert_eq!(h.state().theme, ThemePreference::Light);
    let mut store = HashMap::new();
    let mut prefs = h.state().clone();
    prefs.language = "ja".into();
    prefs.save(|k, v| {
        store.insert(k.to_string(), v);
    });
    assert_eq!(store["app.theme"], "light");
    assert_eq!(
        Prefs::load(|k| store.get(k).cloned(), || "en".into()),
        prefs
    );
}

#[test]
fn the_settings_fields_grow_with_the_dialog_width() {
    for w in [280.0, 600.0] {
        let cards = vec!["USB Audio CODEC".to_string()];
        let mut h = builder(w, 300.0).build_ui_state(
            |ui, p: &mut Prefs| {
                app::device_field(ui, "Input", "en", &mut p.input, &cards);
                app::language_field(ui, &mut p.language);
                app::theme_field(ui, "en", &mut p.theme);
            },
            Prefs::load(|_| None, || "en".into()),
        );
        h.run_steps(2);
        let combos = h.get_all_by_role(Role::ComboBox).collect::<Vec<_>>();
        assert_eq!(combos.len(), 3);
        for c in combos {
            let r = c.rect();
            assert!(r.max.x > w - 24.0 && r.width() > w * 0.55, "{r:?} at {w}");
        }
    }
}

#[test]
fn a_host_input_panel_asks_the_host_to_start_stop_and_open() {
    for w in [280.0, 360.0, 600.0] {
        let mut p = Cw4i::new("en");
        p.host_input = true;
        let mut h = builder(w, 400.0).build_ui_state(
            |ui, p: &mut Cw4i| {
                p.toolbar(ui, |_| {});
                p.body(ui);
            },
            p,
        );
        h.run_steps(3);
        let start = h.get_by_label(START).rect();
        let open = h.get_by_label(OPEN_WAV).rect();
        assert!(before(start, open) && start.min.x < 12.0, "at {w}");
        for c in h.query_all_by(|n| matches!(n.role(), Role::Button | Role::ComboBox)) {
            let r = c.rect();
            assert!(r.min.x >= -0.5 && r.max.x <= w + 0.5, "{r:?} at {w}");
        }
        h.get_by_label(START).click();
        h.run_steps(2);
        assert_eq!(h.state_mut().take_events(), vec![Event::Start]);
        assert!(!h.state().running());
        h.state_mut().host_running = true;
        h.run_steps(2);
        h.get_by_label("Stop").click();
        h.get_by_label(OPEN_WAV).click();
        h.run_steps(2);
        assert_eq!(
            h.state_mut().take_events(),
            vec![Event::Stop, Event::OpenWav]
        );
    }
}
