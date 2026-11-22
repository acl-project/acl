// SPDX-FileCopyrightText: 2026 The Adwaita Craft Launcher Authors
// SPDX-License-Identifier: GPL-3.0-or-later

use serde::{Deserialize, Serialize};
use unic_langid::LanguageIdentifier;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Language {
    English,
    SimplifiedChinese,
    #[default]
    #[serde(other)]
    System,
}

impl Language {
    pub const ALL: &[Self] = &[Self::System, Self::English, Self::SimplifiedChinese];
    pub fn tag(self) -> Option<&'static str> {
        match self {
            Self::System => None,
            Self::English => Some("en-US"),
            Self::SimplifiedChinese => Some("zh-CN"),
        }
    }

    pub fn to_index(self) -> u32 {
        match self {
            Self::System => 0,
            Self::English => 1,
            Self::SimplifiedChinese => 2,
        }
    }

    pub fn from_index(index: u32) -> Self {
        match index {
            1 => Self::English,
            2 => Self::SimplifiedChinese,
            _ => Self::System,
        }
    }
    pub fn to_mojang_code(self) -> Option<String> {
        match self {
            Language::English => None,
            Language::SimplifiedChinese => Some("zh_CN".to_string()),
            Language::System => {
                let raw = sys_locale::get_locale()?;
                locale_to_mojang(&raw)
            }
        }
    }
}

/// Converts a single system locale string into Mojang format with an uppercase region code.
pub fn locale_to_mojang(raw: &str) -> Option<String> {
    // Handle POSIX suffixes that Linux may return, such as 'zh_CN.UTF-8' or 'sr_RS@latin'
    let cleaned = raw.split(['.', '@']).next()?.trim();
    let id: LanguageIdentifier = cleaned.parse().ok()?;

    let lang = id.language.as_str();
    let script = id.script.as_ref().map(|s| s.as_str());
    let region = id.region.as_ref().map(|r| r.as_str());

    match (lang, script, region) {
        ("en", _, None | Some("US")) => None,
        ("lzh", _, _) => Some("lzh".to_string()),
        ("zh", _, Some("HK" | "MO")) => Some("zh_HK".to_string()),
        ("zh", _, Some("TW")) | ("zh", Some("Hant"), _) => Some("zh_TW".to_string()),
        ("zh", _, Some("CN" | "SG")) | ("zh", Some("Hans"), _) => Some("zh_CN".to_string()),
        ("nb", _, _) => Some("no_NO".to_string()),
        ("iw", _, _) => Some("he_IL".to_string()),
        ("in", _, _) => Some("id_ID".to_string()),
        // Other languages: unic-langid guarantees lowercase lang and uppercase region; join as xx_YY
        (l, _, Some(r)) => Some(format!("{l}_{r}")),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde::de::{IntoDeserializer, value::Error};

    #[test]
    fn test_language_default() {
        assert_eq!(Language::default(), Language::System);
    }

    #[test]
    fn test_language_tag() {
        assert_eq!(Language::System.tag(), None);
        assert_eq!(Language::English.tag(), Some("en-US"));
        assert_eq!(Language::SimplifiedChinese.tag(), Some("zh-CN"));
    }

    #[test]
    fn test_language_index_mapping() {
        assert_eq!(Language::System.to_index(), 0);
        assert_eq!(Language::English.to_index(), 1);
        assert_eq!(Language::SimplifiedChinese.to_index(), 2);

        assert_eq!(Language::from_index(0), Language::System);
        assert_eq!(Language::from_index(1), Language::English);
        assert_eq!(Language::from_index(2), Language::SimplifiedChinese);
        assert_eq!(Language::from_index(3), Language::System);
        assert_eq!(Language::from_index(999), Language::System);
    }

    #[test]
    fn test_language_deserialization() {
        let de = |s: &'static str| -> Result<Language, Error> {
            Language::deserialize(s.into_deserializer())
        };

        assert_eq!(de("system").expect("deserialize system"), Language::System);
        assert_eq!(
            de("english").expect("deserialize english"),
            Language::English
        );
        assert_eq!(
            de("simplified_chinese").expect("deserialize simplified_chinese"),
            Language::SimplifiedChinese
        );

        // Unknown variants fallback to System via #[serde(other)]
        assert_eq!(
            de("unknown").expect("deserialize unknown"),
            Language::System
        );
        assert_eq!(de("").expect("deserialize empty string"), Language::System);
    }

    #[test]
    fn test_language_all_constant() {
        assert_eq!(
            Language::ALL,
            &[
                Language::System,
                Language::English,
                Language::SimplifiedChinese
            ]
        );
        assert_eq!(Language::ALL.len(), 3);
        for (idx, &lang) in Language::ALL.iter().enumerate() {
            assert_eq!(lang.to_index(), idx as u32);
            assert_eq!(Language::from_index(idx as u32), lang);
        }
    }
}
