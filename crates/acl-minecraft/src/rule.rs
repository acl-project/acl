use std::collections::HashMap;

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum RuleAction {
    Allow,
    Disallow,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct OsRule {
    pub name: Option<String>,
    pub arch: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Rule {
    pub action: RuleAction,
    pub os: Option<OsRule>,
    pub features: Option<HashMap<String, bool>>,
}

impl Rule {
    pub fn matches_linux(&self) -> bool {
        // If the rules require enabling a specific feature (e.g., demo mode --demo), it is disabled by default and returns false directly.
        if self.features.is_some() {
            return false;
        }

        if let Some(os) = &self.os {
            if let Some(name) = &os.name
                && name != "linux"
            {
                return false;
            }
            if let Some(arch) = &os.arch
                && arch != std::env::consts::ARCH
            {
                return false;
            }
        }

        true
    }

    pub fn is_allowed_on_linux(rules: &[Rule]) -> bool {
        if rules.is_empty() {
            return true;
        }

        let mut action = RuleAction::Disallow;
        for rule in rules {
            if rule.matches_linux() {
                action = rule.action;
            }
        }

        action == RuleAction::Allow
    }
}
