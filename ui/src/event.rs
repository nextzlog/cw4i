use crate::contact::Contact;
use crate::entry::Pick;
use crate::station::WordKind;

#[derive(Clone, Debug, PartialEq)]
pub enum Event {
    Pick {
        text: String,
        kind: WordKind,
    },
    Contact(Contact),
    Appeared {
        station: u32,
        freq: f64,
    },
    Lost {
        station: u32,
        callsign: Option<String>,
    },
    Start,
    Stop,
    OpenWav,
    Settings,
}

impl Event {
    pub fn pick(pick: &Pick) -> Event {
        let kind = match pick {
            Pick::Callsign(_) => WordKind::Callsign,
            Pick::Number(_) => WordKind::Exchange,
        };
        Event::Pick {
            text: pick.text().into(),
            kind,
        }
    }
}
