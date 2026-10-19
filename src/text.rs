//! User-interface text by key, from `lang/<code>.lang` files embedded at build time.
//!
//! Each line of a file is `key = text`, with `{0}`, `{1}` for arguments.
//! The English table is the fallback.

use std::collections::HashMap;
use std::sync::OnceLock;

/// Text tables of every language, fallback first.
pub struct Texts {
    tables: Vec<(&'static str, HashMap<&'static str, &'static str>)>,
}

impl Texts {
    fn new(files: &[(&'static str, &'static str)]) -> Self {
        let tables = files
            .iter()
            .map(|(code, text)| (*code, parse(text)))
            .collect();
        Texts { tables }
    }

    /// Text for `key` in `language`, else in English, else the key itself.
    pub fn get<'a>(&self, language: &str, key: &'a str) -> &'a str
    where
        'static: 'a,
    {
        let find = |code: &str| {
            self.tables
                .iter()
                .find(|(c, _)| *c == code)
                .and_then(|(_, t)| t.get(key).copied())
        };
        find(language)
            .or_else(|| self.tables.first().and_then(|(_, t)| t.get(key).copied()))
            .unwrap_or(key)
    }

    /// [`Texts::get`] with `{0}`, `{1}`, ... replaced by `args`.
    pub fn format(&self, language: &str, key: &str, args: &[&str]) -> String {
        let mut s = self.get(language, key).to_string();
        for (i, a) in args.iter().enumerate() {
            s = s.replace(&format!("{{{i}}}"), a);
        }
        s
    }

    /// Keys of the English table missing in another language.
    pub fn missing(&self) -> Vec<(&'static str, &'static str)> {
        let Some((_, first)) = self.tables.first() else {
            return Vec::new();
        };
        let mut out: Vec<_> = self
            .tables
            .iter()
            .skip(1)
            .flat_map(|(code, t)| {
                first
                    .keys()
                    .filter(|k| !t.contains_key(*k))
                    .map(move |k| (*code, *k))
            })
            .collect();
        out.sort();
        out
    }
}

fn parse(text: &'static str) -> HashMap<&'static str, &'static str> {
    text.lines()
        .map(str::trim)
        .filter(|l| !l.is_empty() && !l.starts_with('#'))
        .filter_map(|l| l.split_once('='))
        .map(|(k, v)| (k.trim(), v.trim()))
        .collect()
}

/// The text tables of CW4ISR.
pub fn texts() -> &'static Texts {
    static TEXTS: OnceLock<Texts> = OnceLock::new();
    TEXTS.get_or_init(|| {
        Texts::new(&[
            ("en", include_str!("../lang/en.lang")),
            ("ja", include_str!("../lang/ja.lang")),
        ])
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_language_is_complete() {
        assert!(texts().missing().is_empty(), "{:?}", texts().missing());
    }

    #[test]
    fn fallback_and_arguments() {
        assert_eq!(texts().get("fr", "start"), texts().get("en", "start"));
        assert_eq!(texts().get("en", "no-such-key"), "no-such-key");
        assert!(texts().format("en", "error", &["x"]).contains('x'));
    }
}
