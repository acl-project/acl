use std::{
    collections::HashSet,
    env, fs,
    path::{Path, PathBuf},
    process::Command,
};

use glob::glob;
use serde::{Deserialize, Serialize};
use tracing::debug;

use crate::error::{CoreError, Result};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct JavaInfo {
    /// Full version string (e.g., "17.0.10", "1.8.0_402", "21.0.2").
    pub version: String,
    /// Parsed major version (e.g., 8 for "1.8.x", 17 for "17.x", 21 for "21.x").
    pub major_version: u32,
    /// Optional vendor or runtime name (e.g., "OpenJDK", "Java(TM)").
    pub vendor: Option<String>,
    /// Whether this is a 64-bit JVM.
    pub is_64bit: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct JavaInstallation {
    /// Path to the `java` executable binary.
    pub path: PathBuf,
    pub info: JavaInfo,
}

impl JavaInfo {
    pub fn parse_from_output(path: &Path, output: &str) -> Result<Self> {
        let mut java_version = None;
        let mut spec_version = None;
        let mut vendor = None;
        let mut bitness = None;

        for line in output.lines() {
            let trimmed = line.trim();
            if let Some((key, value)) = trimmed.split_once('=') {
                let k = key.trim();
                let v = value.trim();

                match k {
                    "java.version" => java_version = Some(v),
                    "java.specification.version" => spec_version = Some(v),
                    "java.vendor" => vendor = Some(v),
                    "sun.arch.data.model" => bitness = Some(v),
                    _ => {}
                }
            }
        }

        let version = java_version.ok_or_else(|| CoreError::JavaProbeFailed {
            path: path.to_path_buf(),
            reason: "Missing java.version property".into(),
        })?;
        let spec = spec_version.ok_or_else(|| CoreError::JavaProbeFailed {
            path: path.to_path_buf(),
            reason: "Missing java.specification.version property".into(),
        })?;

        let major_version = if let Some(stripped) = spec.strip_prefix("1.") {
            stripped.parse::<u32>()
        } else {
            spec.parse::<u32>()
        }
        .map_err(|_| CoreError::JavaProbeFailed {
            path: path.to_path_buf(),
            reason: format!("Failed to parse major version from '{spec}'"),
        })?;

        let is_64bit = bitness == Some("64");

        Ok(Self {
            version: version.to_string(),

            major_version,

            vendor: vendor.map(|s| s.to_string()),

            is_64bit,
        })
    }
}

pub fn probe_java(java_path: &Path) -> Result<JavaInstallation> {
    let output = Command::new(java_path)
        .args(["-XshowSettings:properties", "-version"])
        .output()
        .map_err(|e| CoreError::Io {
            path: java_path.to_path_buf(),
            source: e,
        })?;

    let combined = format!(
        "{}\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );

    let info = JavaInfo::parse_from_output(java_path, &combined)?;

    debug!(
        "Probe java {} {} in {}",
        info.major_version,
        info.vendor.as_ref().map_or("Unknown", |vendor| vendor),
        java_path.display()
    );

    Ok(JavaInstallation {
        path: java_path.to_path_buf(),
        info,
    })
}

fn try_add(path: &Path, seen: &mut HashSet<PathBuf>) -> Option<JavaInstallation> {
    if !path.is_file() {
        return None;
    }

    let path = fs::canonicalize(path).ok()?;

    if !seen.insert(path.clone()) {
        return None;
    }

    probe_java(&path).ok()
}

fn java_from_pattern(seen: &mut HashSet<PathBuf>) -> Vec<JavaInstallation> {
    let mut javas = Vec::new();
    for pattern in [
        "/usr/lib/jvm/*/bin/java",
        "/usr/java/*/bin/java",
        "/opt/java/*/bin/java",
        "/opt/jdk/*/bin/java",
    ] {
        for path in glob(pattern).into_iter().flatten().flatten() {
            if let Some(java) = try_add(&path, &mut *seen) {
                javas.push(java);
            }
        }
    }

    javas
}

fn java_from_home(seen: &mut HashSet<PathBuf>) -> Option<JavaInstallation> {
    let raw = env::var_os("JAVA_HOME")?;
    let home = PathBuf::from(raw);

    let java_path = home.join("bin").join("java");
    try_add(&java_path, seen)
}

pub fn discover_java_installations() -> Vec<JavaInstallation> {
    let mut seen = HashSet::new();
    let mut javas = Vec::new();

    javas.append(&mut java_from_pattern(&mut seen));

    if let Some(java) = java_from_home(&mut seen) {
        javas.push(java);
    }

    javas
}
