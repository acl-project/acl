// SPDX-FileCopyrightText: 2026 The Adwaita Craft Launcher Authors
// SPDX-License-Identifier: GPL-3.0-or-later

use serde::{Deserialize, Serialize};

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
}
