// SPDX-FileCopyrightText: 2026 The Adwaita Craft Launcher Authors
// SPDX-License-Identifier: GPL-3.0-or-later

use std::cmp::Ordering;

use version_compare::{Cmp, compare_to};

use crate::maven::compare_maven_rules;

pub fn is_newer_version(candidate: &str, current: &str) -> bool {
    // Check whether it is Maven
    if let Some(ord) = compare_maven_rules(candidate, current) {
        match ord {
            Ordering::Greater => return true,
            Ordering::Less => return false,
            Ordering::Equal => {} // If determined equal, continue with a more lenient comparison
        }
    }

    // Check if it's semver
    if let (Ok(a), Ok(b)) = (
        semver::Version::parse(candidate),
        semver::Version::parse(current),
    ) {
        return a > b;
    }

    // Use the mainstream library version-compare as a fallback for non-standard formats
    compare_to(candidate, current, Cmp::Gt).unwrap_or(false)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_is_newer_version_maven_rules_priority() {
        assert!(is_newer_version("1.0.0-sp", "1.0.0"));
        assert!(!is_newer_version("1.0.0", "1.0.0-sp"));
        assert!(is_newer_version("1.0.0", "1.0.0-alpha"));
        assert!(!is_newer_version("1.0.0-alpha", "1.0.0"));
    }

    #[test]
    fn test_is_newer_version_semver_and_fallback() {
        assert!(is_newer_version("1.2.3", "1.2.2"));
        assert!(!is_newer_version("1.2.2", "1.2.3"));
        assert!(is_newer_version("0.16.10", "0.16.9"));
        assert!(is_newer_version("14.23.5.2860", "14.23.5.2859"));
    }
}
