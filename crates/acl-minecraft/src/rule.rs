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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_rule_matches_linux_features() {
        let mut features = HashMap::new();
        features.insert("is_demo_user".to_string(), true);

        let rule = Rule {
            action: RuleAction::Allow,
            os: None,
            features: Some(features),
        };
        assert!(!rule.matches_linux());
    }

    #[test]
    fn test_rule_matches_linux_os_name() {
        let linux_rule = Rule {
            action: RuleAction::Allow,
            os: Some(OsRule {
                name: Some("linux".to_string()),
                arch: None,
            }),
            features: None,
        };
        assert!(linux_rule.matches_linux());

        let windows_rule = Rule {
            action: RuleAction::Allow,
            os: Some(OsRule {
                name: Some("windows".to_string()),
                arch: None,
            }),
            features: None,
        };
        assert!(!windows_rule.matches_linux());

        let osx_rule = Rule {
            action: RuleAction::Allow,
            os: Some(OsRule {
                name: Some("osx".to_string()),
                arch: None,
            }),
            features: None,
        };
        assert!(!osx_rule.matches_linux());

        let any_os_rule = Rule {
            action: RuleAction::Allow,
            os: None,
            features: None,
        };
        assert!(any_os_rule.matches_linux());
    }

    #[test]
    fn test_rule_matches_linux_os_arch() {
        let matching_arch_rule = Rule {
            action: RuleAction::Allow,
            os: Some(OsRule {
                name: Some("linux".to_string()),
                arch: Some(std::env::consts::ARCH.to_string()),
            }),
            features: None,
        };
        assert!(matching_arch_rule.matches_linux());

        let mismatching_arch_rule = Rule {
            action: RuleAction::Allow,
            os: Some(OsRule {
                name: Some("linux".to_string()),
                arch: Some("unknown_foreign_arch".to_string()),
            }),
            features: None,
        };
        assert!(!mismatching_arch_rule.matches_linux());
    }

    #[test]
    fn test_is_allowed_on_linux_empty_rules() {
        assert!(Rule::is_allowed_on_linux(&[]));
    }

    #[test]
    fn test_is_allowed_on_linux_single_rule() {
        let allow_rule = Rule {
            action: RuleAction::Allow,
            os: Some(OsRule {
                name: Some("linux".to_string()),
                arch: None,
            }),
            features: None,
        };
        assert!(Rule::is_allowed_on_linux(&[allow_rule]));

        let disallow_rule = Rule {
            action: RuleAction::Disallow,
            os: Some(OsRule {
                name: Some("linux".to_string()),
                arch: None,
            }),
            features: None,
        };
        assert!(!Rule::is_allowed_on_linux(&[disallow_rule]));
    }

    #[test]
    fn test_is_allowed_on_linux_rule_chaining() {
        let base_disallow = Rule {
            action: RuleAction::Disallow,
            os: None,
            features: None,
        };
        let allow_linux = Rule {
            action: RuleAction::Allow,
            os: Some(OsRule {
                name: Some("linux".to_string()),
                arch: None,
            }),
            features: None,
        };
        assert!(Rule::is_allowed_on_linux(&[base_disallow, allow_linux]));

        let base_allow = Rule {
            action: RuleAction::Allow,
            os: None,
            features: None,
        };
        let disallow_windows = Rule {
            action: RuleAction::Disallow,
            os: Some(OsRule {
                name: Some("windows".to_string()),
                arch: None,
            }),
            features: None,
        };
        assert!(Rule::is_allowed_on_linux(&[
            base_allow.clone(),
            disallow_windows
        ]));

        let disallow_linux = Rule {
            action: RuleAction::Disallow,
            os: Some(OsRule {
                name: Some("linux".to_string()),
                arch: None,
            }),
            features: None,
        };
        assert!(!Rule::is_allowed_on_linux(&[base_allow, disallow_linux]));
    }
}
