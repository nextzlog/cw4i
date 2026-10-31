// Glyphs of Phosphor Icons (MIT, Copyright (c) 2023 Phosphor Icons); see data/LICENSE.
// The same font, family name and codepoints as the zLog toolbar.

use egui::epaint::text::{FontInsert, FontPriority, InsertFontFamily};
use egui::{Button, Context, FontData, FontFamily, Response, RichText, Role, Ui, WidgetInfo};

pub const FAMILY: &str = "phosphor";

pub const FONT: &[u8] = include_bytes!("../data/Phosphor.ttf");

pub const ARROWS_LEFT_RIGHT: &str = "\u{E0A0}";
pub const CIRCLE: &str = "\u{E18A}";
pub const FOLDER_OPEN: &str = "\u{E256}";
pub const GEAR: &str = "\u{E270}";
pub const PLAY: &str = "\u{E3D0}";
pub const SIDEBAR_SIMPLE: &str = "\u{EC24}";
pub const SQUARE_HALF: &str = "\u{E462}";
pub const SQUARE_SPLIT_VERTICAL: &str = "\u{E874}";
pub const STOP: &str = "\u{E46C}";

pub const ALL: &[&str] = &[
    ARROWS_LEFT_RIGHT,
    CIRCLE,
    FOLDER_OPEN,
    GEAR,
    PLAY,
    SIDEBAR_SIMPLE,
    SQUARE_HALF,
    SQUARE_SPLIT_VERTICAL,
    STOP,
];

pub fn family() -> FontFamily {
    FontFamily::Name(FAMILY.into())
}

pub fn install(ctx: &Context) {
    if ctx.fonts(|f| f.families().contains(&family())) {
        return;
    }
    ctx.add_font(FontInsert::new(
        FAMILY,
        FontData::from_static(FONT),
        vec![InsertFontFamily {
            family: family(),
            priority: FontPriority::Highest,
        }],
    ));
}

pub fn glyph(g: &str) -> RichText {
    RichText::new(g).family(family())
}

pub fn labelled(r: Response, label: &str, selected: bool) -> Response {
    let enabled = r.enabled();
    r.widget_info(|| WidgetInfo::selected(Role::Button, enabled, selected, label));
    r
}

pub fn button(ui: &mut Ui, g: &str, label: &str, selected: bool) -> Response {
    install(ui.ctx());
    let r = ui.add(Button::selectable(
        selected,
        (glyph(g), RichText::new(label)),
    ));
    labelled(r, label, selected)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_icon_is_one_glyph_in_the_private_use_area() {
        assert!(FONT.len() > 100_000);
        for g in ALL {
            let c: Vec<char> = g.chars().collect();
            assert_eq!(c.len(), 1);
            assert!(('\u{E000}'..='\u{F8FF}').contains(&c[0]));
        }
    }

    #[test]
    fn a_button_installs_the_family() {
        let mut h = egui_kittest::Harness::new_ui(|ui| {
            button(ui, PLAY, "Start", false);
        });
        h.run();
        assert!(h.ctx.fonts(|f| f.families().contains(&family())));
        egui_kittest::kittest::Queryable::get_by_label(&h, "Start");
    }
}
