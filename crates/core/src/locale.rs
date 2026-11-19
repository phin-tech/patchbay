//! Which of the app's languages to use for the one the system asks for.

/// The language in `available` closest to `system`: the same one (`fr-CA`), the same language elsewhere (`fr`, for
/// `fr-CA`), or English. `system` is as macOS gives it (`fr-CA`, `zh-Hant-TW`); `available` as the locales files name
/// them (`fr`, `zh-TW`).
pub fn best_locale(system: &str, available: &[&str]) -> String {
    let system = system.replace('_', "-");
    let same = |a: &str, b: &str| a.eq_ignore_ascii_case(b);
    if let Some(exact) = available.iter().find(|locale| same(locale, &system)) {
        return exact.to_string();
    }
    let language = system.split('-').next().unwrap_or_default();
    let region = system
        .rsplit('-')
        .next()
        .filter(|region| region.len() == 2 && *region != language);
    if let Some(region) = region
        && let Some(found) = available
            .iter()
            .find(|locale| same(locale, &format!("{language}-{region}")))
    {
        return found.to_string();
    }
    available
        .iter()
        .find(|locale| same(locale, language))
        .map_or_else(|| "en".to_string(), |locale| locale.to_string())
}

#[cfg(test)]
mod tests {
    use super::best_locale;

    #[test]
    fn the_closest_language_is_chosen() {
        let available = ["en", "fr", "zh-TW"];
        assert_eq!(best_locale("fr-CA", &available), "fr");
        assert_eq!(best_locale("fr", &available), "fr");
        assert_eq!(best_locale("zh-Hant-TW", &available), "zh-TW");
        assert_eq!(best_locale("de-DE", &available), "en");
        assert_eq!(best_locale("en_GB", &available), "en");
    }
}
