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
