// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Nyabi (nyabi-gh)

//! Interface text in the user's language, from the Fluent files in `i18n/`.
//! English is the source; Korean and Japanese carry 2.2.13's translations,
//! taken from its Qt catalogues by `tools/ts_to_ftl.py`.

use std::collections::HashMap;
use std::sync::OnceLock;

use fluent_bundle::concurrent::FluentBundle;
use fluent_bundle::{FluentArgs, FluentResource};
use unic_langid::LanguageIdentifier;

use crate::settings::Language;

const LANGUAGES: [(&str, &str); 3] = [
    ("en", include_str!("../i18n/en.ftl")),
    ("ko", include_str!("../i18n/ko.ftl")),
    ("ja", include_str!("../i18n/ja.ftl")),
];

struct Text {
    /// The chosen language's code, as "ko".
    language: &'static str,
    /// The chosen language, then English for what it lacks.
    bundles: Vec<FluentBundle<FluentResource>>,
    /// Messages without arguments, formatted once.
    plain: HashMap<String, &'static str>,
}

/// The language chosen in the settings, set before any text is shown.
static CHOSEN: OnceLock<Language> = OnceLock::new();

/// Uses `language` from the settings unless `UGURUGU_LANGUAGE` names one.
/// Text already shown keeps its language, so this comes first.
pub fn choose(language: Language) {
    if CHOSEN.set(language).is_err() {
        tracing::warn!("the interface language was already chosen");
    }
}

fn text() -> &'static Text {
    static TEXT: OnceLock<Text> = OnceLock::new();
    TEXT.get_or_init(|| {
        let chosen = CHOSEN
            .get()
            .filter(|language| **language != Language::System)
            .map(|language| language.code().to_owned());
        let preferred = std::env::var("UGURUGU_LANGUAGE")
            .ok()
            .or(chosen)
            .map(|language| vec![language])
            .unwrap_or_else(ugu_win::locale::preferred_ui_languages);
        let chosen = language_for(&preferred);
        tracing::info!(language = chosen, "interface language");
        load(chosen)
    })
}

/// As 2.2.13's Qt translator picks a catalogue: the first preferred
/// language that has one, else the English source, which has none.
fn language_for(preferred: &[String]) -> &'static str {
    preferred
        .iter()
        .find_map(|name| {
            let primary = name.split(['-', '_']).next()?.to_ascii_lowercase();
            LANGUAGES
                .iter()
                .find(|(code, _)| *code != "en" && *code == primary)
                .map(|(code, _)| *code)
        })
        .unwrap_or("en")
}

fn bundle(code: &str) -> (FluentBundle<FluentResource>, Vec<String>) {
    let source = LANGUAGES
        .iter()
        .find(|(each, _)| *each == code)
        .map(|(_, source)| *source)
        .expect("a known language");
    let resource = FluentResource::try_new(source.to_owned())
        .unwrap_or_else(|(_, errors)| panic!("{code}.ftl does not parse: {errors:?}"));
    // The files are generated one message per line.
    let ids = source
        .lines()
        .filter_map(|line| line.split_once(" = "))
        .filter(|(id, _)| {
            id.chars()
                .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
        })
        .map(|(id, _)| id.to_owned())
        .collect();
    let language: LanguageIdentifier = code.parse().expect("a valid language tag");
    let mut bundle = FluentBundle::new_concurrent(vec![language]);
    // egui's fonts have no glyphs for the bidi isolation marks.
    bundle.set_use_isolating(false);
    bundle
        .add_resource(resource)
        .unwrap_or_else(|errors| panic!("{code}.ftl repeats messages: {errors:?}"));
    (bundle, ids)
}

fn load(code: &'static str) -> Text {
    let mut bundles = Vec::new();
    let mut ids = Vec::new();
    let order: &[&str] = if code == "en" { &["en"] } else { &[code, "en"] };
    for each in order {
        let (bundle, names) = bundle(each);
        bundles.push(bundle);
        ids.extend(names);
    }
    let mut text = Text {
        language: code,
        bundles,
        plain: HashMap::new(),
    };
    // Messages with variables are formatted when used, with their values.
    for id in ids {
        if text.plain.contains_key(&id) {
            continue;
        }
        let plain = text.bundles.iter().find_map(|bundle| {
            let pattern = bundle.get_message(&id)?.value()?;
            let mut errors = Vec::new();
            let value = bundle
                .format_pattern(pattern, None, &mut errors)
                .into_owned();
            Some(errors.is_empty().then_some(value))
        });
        if let Some(Some(value)) = plain {
            text.plain.insert(id, String::leak(value));
        }
    }
    text
}

fn format(
    bundles: &[FluentBundle<FluentResource>],
    id: &str,
    args: Option<&FluentArgs>,
) -> Option<String> {
    bundles.iter().find_map(|bundle| {
        let pattern = bundle.get_message(id)?.value()?;
        let mut errors = Vec::new();
        let value = bundle
            .format_pattern(pattern, args, &mut errors)
            .into_owned();
        if !errors.is_empty() {
            tracing::warn!(id, ?errors, "interface text did not format");
        }
        Some(value)
    })
}

/// The text of message `id`.
/// The interface language's code, as "ko".
pub fn language() -> &'static str {
    text().language
}

pub fn tr(id: &'static str) -> &'static str {
    text().plain.get(id).copied().unwrap_or_else(|| {
        tracing::warn!(id, "no interface text");
        id
    })
}

/// The text of message `id` with `args` put in.
/// Arguments for `tr_with`.
pub fn args<const N: usize>(pairs: [(&'static str, String); N]) -> FluentArgs<'static> {
    let mut args = FluentArgs::new();
    for (name, value) in pairs {
        args.set(name, value);
    }
    args
}

pub fn tr_with(id: &'static str, args: &FluentArgs) -> String {
    format(&text().bundles, id, Some(args)).unwrap_or_else(|| {
        tracing::warn!(id, "no interface text");
        id.to_owned()
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ids(code: &str) -> Vec<String> {
        let mut ids = bundle(code).1;
        ids.sort();
        ids
    }

    #[test]
    fn every_language_has_every_message_and_no_other() {
        let english = ids("en");
        assert!(!english.is_empty());
        for code in ["ko", "ja"] {
            assert_eq!(ids(code), english, "{code}.ftl");
        }
    }

    #[test]
    fn the_first_preferred_language_with_a_catalogue_wins() {
        let names = |list: &[&str]| {
            list.iter()
                .map(|name| (*name).to_owned())
                .collect::<Vec<_>>()
        };
        assert_eq!(language_for(&names(&["ko-KR", "en-US"])), "ko");
        assert_eq!(language_for(&names(&["fr-FR", "ja-JP"])), "ja");
        assert_eq!(language_for(&names(&["en-US", "ko-KR"])), "ko");
        assert_eq!(language_for(&names(&["de-DE"])), "en");
        assert_eq!(language_for(&[]), "en");
    }

    #[test]
    fn missing_messages_fall_back_to_english() {
        let text = load("ko");
        let english = load("en");
        for (id, value) in &english.plain {
            assert!(!value.contains('{'), "{id}");
            assert!(text.plain.contains_key(id), "{id}");
            assert!(!value.is_empty(), "{id}");
        }
    }
}
