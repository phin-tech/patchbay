//! The app's language: the closest the app has to the system's, or English, its own text. The kit's widgets follow the
//! same choice, since `rust-i18n` keeps one language for the whole app. Translations are in `locales/app.toml`.

use patchbay_core::best_locale;
use rust_i18n::available_locales;

/// Chooses the language, before anything shows text.
pub fn choose() {
    let system = sys_locale::get_locale().unwrap_or_else(|| "en".to_string());
    let available = available_locales!();
    let available: Vec<&str> = available.iter().map(|locale| locale.as_ref()).collect();
    rust_i18n::set_locale(&best_locale(&system, &available));
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;
    use std::fs;
    use std::path::Path;

    fn translations() -> toml::Table {
        include_str!("../locales/app.toml")
            .parse()
            .expect("locales/app.toml is TOML")
    }

    #[test]
    fn every_translation_is_of_text_the_app_shows() {
        let src = Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
        let source: String = fs::read_dir(src)
            .unwrap()
            .filter_map(|entry| fs::read_to_string(entry.ok()?.path()).ok())
            .collect();
        for key in translations().keys().filter(|key| *key != "_version") {
            assert!(
                source.contains(&format!("t!({key:?}")),
                "{key:?} is translated but never shown"
            );
        }
    }

    #[test]
    fn every_translation_has_every_language() {
        let mut languages: Option<(String, BTreeSet<String>)> = None;
        for (key, value) in translations().iter().filter(|(key, _)| *key != "_version") {
            let these: BTreeSet<String> = value
                .as_table()
                .expect("a table of languages")
                .keys()
                .cloned()
                .collect();
            match &languages {
                None => languages = Some((key.clone(), these)),
                Some((first, all)) => assert_eq!(
                    &these, all,
                    "{key:?} and {first:?} have different languages"
                ),
            }
        }
    }
}
