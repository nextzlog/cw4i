pub mod app;
pub mod audio;
pub mod contact;
pub mod entry;
pub mod event;
pub mod icon;
pub mod panel;
pub mod station;
pub mod text;
pub mod waterfall;

pub use contact::{Contact, Heard};
pub use entry::{CutTable, Pick};
pub use event::Event;
pub use panel::{Cw4i, Place, ToolbarItems};
pub use station::{Station, Stations, Word, WordKind};

pub fn decode(samples: &[f32], rate: u32) -> Stations {
    let mut decoder = cw4i::Decoder::new(rate);
    let mut stations = Stations::default();
    for chunk in samples.chunks((rate / 50).max(1) as usize) {
        for e in decoder.push(chunk) {
            stations.apply(e);
        }
        stations.refresh(&decoder);
    }
    for e in decoder.flush() {
        stations.apply(e);
    }
    stations
}

#[cfg(test)]
mod tests {
    use super::*;

    pub fn morse(text: &str, freq: f64, wpm: f64, rate: u32) -> Vec<f32> {
        let code = |c: char| match c {
            'A' => ".-",
            'B' => "-...",
            'C' => "-.-.",
            'D' => "-..",
            'E' => ".",
            'F' => "..-.",
            'G' => "--.",
            'H' => "....",
            'I' => "..",
            'J' => ".---",
            'K' => "-.-",
            'L' => ".-..",
            'M' => "--",
            'N' => "-.",
            'O' => "---",
            'P' => ".--.",
            'Q' => "--.-",
            'R' => ".-.",
            'S' => "...",
            'T' => "-",
            'U' => "..-",
            'V' => "...-",
            'W' => ".--",
            'X' => "-..-",
            'Y' => "-.--",
            'Z' => "--..",
            '0' => "-----",
            '1' => ".----",
            '2' => "..---",
            '3' => "...--",
            '4' => "....-",
            '5' => ".....",
            '6' => "-....",
            '7' => "--...",
            '8' => "---..",
            '9' => "----.",
            _ => "",
        };
        let unit = 1.2 / wpm;
        let mut keys: Vec<(bool, f64)> = vec![(false, 1.0)];
        for word in text.split(' ') {
            for c in word.chars() {
                for e in code(c).chars() {
                    keys.push((true, if e == '.' { unit } else { 3.0 * unit }));
                    keys.push((false, unit));
                }
                keys.push((false, 2.0 * unit));
            }
            keys.push((false, 4.0 * unit));
        }
        keys.push((false, 1.0));
        let mut out = Vec::new();
        let mut t = 0.0f64;
        let mut seed = 1u64;
        for (on, dur) in keys {
            let n = (dur * rate as f64) as usize;
            for i in 0..n {
                let edge = ((i.min(n - i) as f64) / (0.005 * rate as f64)).min(1.0);
                let amp = if on { 0.5 * edge } else { 0.0 };
                seed ^= seed << 13;
                seed ^= seed >> 7;
                seed ^= seed << 17;
                let noise = ((seed >> 11) as f64 / (1u64 << 53) as f64 - 0.5) * 0.02;
                out.push((amp * (2.0 * std::f64::consts::PI * freq * t).sin() + noise) as f32);
                t += 1.0 / rate as f64;
            }
        }
        out
    }

    #[test]
    fn decodes_a_station_and_offers_its_callsign() {
        let rate = 8000;
        let audio = morse("CQ TEST JA1ZLO JA1ZLO TEST", 700.0, 25.0, rate);
        let stations = decode(&audio, rate);
        let list = stations.sorted();
        let s = list
            .iter()
            .find(|s| s.text().contains("JA1ZLO"))
            .unwrap_or_else(|| panic!("{:?}", list.iter().map(|s| s.text()).collect::<Vec<_>>()));
        assert!((s.freq - 700.0).abs() < 20.0, "freq {}", s.freq);
        assert!(s.confirmed);
        assert_eq!(s.callsign(), Some("JA1ZLO"));
        let picks: Vec<Pick> = s.words.iter().filter_map(Pick::of).collect();
        assert!(
            picks.contains(&Pick::Callsign("JA1ZLO".into())),
            "{picks:?}"
        );
    }

    #[test]
    fn two_stations_are_read_apart() {
        let rate = 8000;
        let a = morse("CQ TEST JA1ZLO JA1ZLO TEST", 600.0, 24.0, rate);
        let b = morse("JH1ABC 5NN 13M TU", 900.0, 28.0, rate);
        let n = a.len().max(b.len());
        let mix: Vec<f32> = (0..n)
            .map(|i| a.get(i).copied().unwrap_or(0.0) + b.get(i).copied().unwrap_or(0.0))
            .collect();
        let stations = decode(&mix, rate);
        let texts: Vec<String> = stations.sorted().iter().map(|s| s.text()).collect();
        assert!(texts.iter().any(|t| t.contains("JA1ZLO")), "{texts:?}");
        assert!(texts.iter().any(|t| t.contains("JH1ABC")), "{texts:?}");
        let b = stations
            .sorted()
            .into_iter()
            .find(|s| s.text().contains("JH1ABC"))
            .unwrap()
            .clone();
        let picks: Vec<Pick> = b.words.iter().filter_map(Pick::of).collect();
        assert!(picks.contains(&Pick::Number("13M".into())), "{picks:?}");
    }
}
