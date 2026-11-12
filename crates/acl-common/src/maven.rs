// SPDX-FileCopyrightText: 2026 The Adwaita Craft Launcher Authors
// SPDX-License-Identifier: GPL-3.0-or-later

use std::cmp::Ordering;

/// Identify the weights of special identifiers officially defined by Maven
fn qualifier_rank(s: &str) -> Option<i32> {
    match s.to_ascii_lowercase().as_str() {
        "alpha" | "a" => Some(-5),
        "beta" | "b" => Some(-4),
        "milestone" | "m" => Some(-3),
        "rc" | "cr" | "pre" => Some(-2),
        "snapshot" => Some(-1),
        "" | "final" | "ga" | "release" => Some(0),
        "sp" => Some(1), // Service Pack is greater than the official version
        _ => None,
    }
}

/// Compare version snippets
fn compare_token(a: &str, b: &str) -> Ordering {
    match (a.parse::<u64>(), b.parse::<u64>()) {
        (Ok(na), Ok(nb)) => na.cmp(&nb),
        (Ok(_), Err(_)) => Ordering::Greater,
        (Err(_), Ok(_)) => Ordering::Less,
        (Err(_), Err(_)) => match (qualifier_rank(a), qualifier_rank(b)) {
            (Some(ra), Some(rb)) => ra.cmp(&rb),
            _ => a.to_ascii_lowercase().cmp(&b.to_ascii_lowercase()),
        },
    }
}

/// Maven version comparison; returns None if it is not a standard Maven version
pub fn compare_maven_rules(candidate: &str, current: &str) -> Option<Ordering> {
    let mut ia = candidate.split(['.', '-', '+', '_']);
    let mut ib = current.split(['.', '-', '+', '_']);

    loop {
        match (ia.next(), ib.next()) {
            (Some(pa), Some(pb)) => {
                let cmp = compare_token(pa, pb);
                if cmp != Ordering::Equal {
                    return Some(cmp);
                }
            }
            (Some(pa), None) => {
                if let Some(rank) = qualifier_rank(pa) {
                    return Some(if rank > 0 {
                        Ordering::Greater
                    } else {
                        Ordering::Less
                    });
                }
                if pa.parse::<u64>().is_ok() {
                    return Some(Ordering::Greater);
                }
                return None; // Unable to determine
            }
            (None, Some(pb)) => {
                if let Some(rank) = qualifier_rank(pb) {
                    return Some(if rank < 0 {
                        Ordering::Greater
                    } else {
                        Ordering::Less
                    });
                }
                if pb.parse::<u64>().is_ok() {
                    return Some(Ordering::Less);
                }
                return None;
            }
            (None, None) => return Some(Ordering::Equal),
        }
    }
}

/// Convert Maven coordinate `group:artifact:version[:classifier]` into relative jar path.
pub fn coordinate_to_path(coordinate: &str) -> Option<String> {
    let part = coordinate.split_once('@');
    let (name, ext) = if let Some((p, s)) = part {
        (p, s)
    } else {
        (coordinate, "jar")
    };
    let mut parts = name.split(':');

    let group = parts.next()?;
    let artifact = parts.next()?;
    let version = parts.next()?;
    let classifier = parts.next();
    let group_path = group.replace('.', "/");

    let file_name = if let Some(classifier) = classifier {
        format!("{artifact}-{version}-{classifier}.{ext}")
    } else {
        format!("{artifact}-{version}.{ext}")
    };

    Some(format!("{group_path}/{artifact}/{version}/{file_name}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_compare_maven_rules_service_pack() {
        assert_eq!(
            compare_maven_rules("1.0-sp", "1.0"),
            Some(Ordering::Greater)
        );
        assert_eq!(
            compare_maven_rules("1.0.1", "1.0-sp"),
            Some(Ordering::Greater)
        );
        assert_eq!(compare_maven_rules("1.0", "1.0-sp"), Some(Ordering::Less));
    }

    #[test]
    fn test_compare_maven_rules_prereleases() {
        assert_eq!(
            compare_maven_rules("1.0", "1.0-alpha"),
            Some(Ordering::Greater)
        );
        assert_eq!(
            compare_maven_rules("1.0-beta", "1.0-alpha"),
            Some(Ordering::Greater)
        );
        assert_eq!(
            compare_maven_rules("1.0-rc", "1.0-beta"),
            Some(Ordering::Greater)
        );
        assert_eq!(
            compare_maven_rules("1.0", "1.0-snapshot"),
            Some(Ordering::Greater)
        );
        assert_eq!(
            compare_maven_rules("1.0-snapshot", "1.0-rc"),
            Some(Ordering::Greater)
        );
    }

    #[test]
    fn test_compare_maven_rules_numeric_segments() {
        assert_eq!(
            compare_maven_rules("0.16.10", "0.16.9"),
            Some(Ordering::Greater)
        );
        assert_eq!(
            compare_maven_rules("14.23.5.2860", "14.23.5.2859"),
            Some(Ordering::Greater)
        );
        assert_eq!(
            compare_maven_rules("21.1.77", "20.4.80"),
            Some(Ordering::Greater)
        );
        assert_eq!(
            compare_maven_rules("1.21.1", "1.21"),
            Some(Ordering::Greater)
        );
        assert_eq!(compare_maven_rules("1.21", "1.21"), Some(Ordering::Equal));
    }

    #[test]
    fn test_maven_artifact_path() {
        assert_eq!(
            coordinate_to_path("com.mojang:authlib:1.5.25"),
            Some("com/mojang/authlib/1.5.25/authlib-1.5.25.jar".to_string())
        );

        assert_eq!(
            coordinate_to_path("org.lwjgl:lwjgl:3.3.3:natives-linux"),
            Some("org/lwjgl/lwjgl/3.3.3/lwjgl-3.3.3-natives-linux.jar".to_string())
        );

        assert_eq!(
            coordinate_to_path("de.oceanlabs.mcp:mcp_config:1.20.1@zip"),
            Some("de/oceanlabs/mcp/mcp_config/1.20.1/mcp_config-1.20.1.zip".to_string())
        );

        assert_eq!(
            coordinate_to_path("de.oceanlabs.mcp:mcp_config:1.20.1:mappings@txt"),
            Some("de/oceanlabs/mcp/mcp_config/1.20.1/mcp_config-1.20.1-mappings.txt".to_string())
        );

        assert_eq!(coordinate_to_path("invalid-name"), None);
    }
}
