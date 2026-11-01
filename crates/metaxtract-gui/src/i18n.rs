use serde::{Deserialize, Serialize};
use std::{
    collections::BTreeMap,
    sync::{
        OnceLock,
        atomic::{AtomicBool, Ordering},
    },
};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Language {
    English,
    Korean,
}
impl Language {
    pub fn label(self) -> &'static str {
        match self {
            Self::English => "English",
            Self::Korean => "한국어",
        }
    }
}
static KOREAN: AtomicBool = AtomicBool::new(false);
pub fn set(language: Language) {
    KOREAN.store(language == Language::Korean, Ordering::Relaxed);
}
pub fn language() -> Language {
    if KOREAN.load(Ordering::Relaxed) {
        Language::Korean
    } else {
        Language::English
    }
}
pub fn system_language() -> Language {
    if ["LC_ALL", "LC_MESSAGES", "LANG"]
        .iter()
        .filter_map(std::env::var_os)
        .any(|s| s.to_string_lossy().to_ascii_lowercase().starts_with("ko"))
    {
        Language::Korean
    } else {
        Language::English
    }
}
pub fn tr(text: &str) -> &str {
    static TRANSLATIONS: OnceLock<BTreeMap<String, String>> = OnceLock::new();
    let dictionary = TRANSLATIONS.get_or_init(|| {
        serde_json::from_str(include_str!("translations.json")).expect("bundled translations")
    });
    if language() == Language::Korean {
        dictionary.get(text).map(String::as_str).unwrap_or(text)
    } else {
        text
    }
}
pub fn bilingual(text: &str) -> &str {
    if let Some((en, ko)) = text.split_once(" / ") {
        if ko.chars().any(|c| ('가'..='힣').contains(&c)) {
            return if language() == Language::Korean {
                ko
            } else {
                en
            };
        }
    }
    tr(text)
}

#[macro_export]
macro_rules! textf {
    ($en:literal, $ko:literal $(, $arg:expr)* $(,)?) => {
        if $crate::i18n::language() == $crate::i18n::Language::Korean {
            format!($ko $(, $arg)*)
        } else {
            format!($en $(, $arg)*)
        }
    };
}

#[cfg(test)]
mod tests {
    #[test]
    fn bundled_translations_cover_primary_workflows_in_one_language() {
        let dictionary: std::collections::BTreeMap<String, String> =
            serde_json::from_str(include_str!("translations.json")).unwrap();
        for key in [
            "Quick analysis",
            "Save as case",
            "Open",
            "Scan queue",
            "Records",
            "Favorites",
            "Notifications",
            "Choose file",
            "Choose folder",
        ] {
            let value = dictionary.get(key).unwrap();
            assert!(!value.is_empty());
            assert!(!value.contains(" / "));
        }
    }
}
