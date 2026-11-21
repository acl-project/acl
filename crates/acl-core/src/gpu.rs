// SPDX-FileCopyrightText: 2026 The Adwaita Craft Launcher Authors
// SPDX-License-Identifier: GPL-3.0-or-later

use std::{
    collections::BTreeMap,
    env, fs,
    path::{Path, PathBuf},
};

// Every sane driver uses Mesa and "DRI_PRIME=1".
// NVIDIA, however, requires a scavenger hunt across distro-specific directories.
const DEFAULT_EGL_VENDOR_DIRS: &[&str] = &[
    "/etc/glvnd/egl_vendor.d",
    "/usr/share/glvnd/egl_vendor.d",
    "/usr/lib/x86_64-linux-gnu/GL/glvnd/egl_vendor.d",
    "/usr/lib/aarch64-linux-gnu/GL/glvnd/egl_vendor.d",
    "/usr/lib/x86_64-linux-gnu/glvnd/egl_vendor.d",
];

fn egl_vendor_search_dirs() -> Vec<PathBuf> {
    if let Some(raw) = env::var_os("__EGL_VENDOR_LIBRARY_DIRS") {
        let dirs: Vec<PathBuf> = env::split_paths(&raw)
            .filter(|p| !p.as_os_str().is_empty())
            .collect();
        if !dirs.is_empty() {
            return dirs;
        }
    }

    DEFAULT_EGL_VENDOR_DIRS.iter().map(PathBuf::from).collect()
}

fn find_nvidia_egl_vendor_json(dirs: &[PathBuf]) -> Option<String> {
    for dir in dirs {
        // Fast path: standard filename used by NVIDIA drivers.
        let standard = dir.join("10_nvidia.json");
        if standard.is_file() {
            return Some(standard.to_string_lossy().into_owned());
        }

        // Fallback: scan for any `*nvidia*.json` in lexicographical order (libglvnd spec).
        let Ok(entries) = fs::read_dir(dir) else {
            continue;
        };

        let found = entries
            .flatten()
            .map(|entry| entry.path())
            .filter(|path| {
                path.is_file()
                    && path.extension().and_then(|e| e.to_str()) == Some("json")
                    && path
                        .file_name()
                        .and_then(|n| n.to_str())
                        .is_some_and(|name| name.to_ascii_lowercase().contains("nvidia"))
            })
            .min();

        if let Some(found) = found {
            return Some(found.to_string_lossy().into_owned());
        }
    }

    None
}

/// Detected discrete GPU driver environment on Linux.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DiscreteGpuEnv {
    /// Whether the official NVIDIA driver (`nvidia` / `nvidia-open`) is present.
    pub has_nvidia_driver: bool,
    /// Path to NVIDIA's GLVND EGL vendor JSON, if installed.
    pub nvidia_egl_vendor_json: Option<String>,
}

impl DiscreteGpuEnv {
    /// Detects the host GPU driver environment.
    pub fn detect() -> Self {
        let nvidia_egl_vendor_json = find_nvidia_egl_vendor_json(&egl_vendor_search_dirs());
        let has_nvidia_driver =
            Path::new("/proc/driver/nvidia/version").exists() || nvidia_egl_vendor_json.is_some();
        Self {
            has_nvidia_driver,
            nvidia_egl_vendor_json,
        }
    }

    /// Inserts discrete GPU offload environment variables into `env`.
    pub fn apply_vars(&self, env: &mut BTreeMap<String, String>) {
        if self.has_nvidia_driver {
            // NVIDIA proprietary / nvidia-open PRIME render offload.
            env.insert("__NV_PRIME_RENDER_OFFLOAD".to_string(), "1".to_string());
            env.insert(
                "__GLX_VENDOR_LIBRARY_NAME".to_string(),
                "nvidia".to_string(),
            );
            env.insert(
                "__VK_LAYER_NV_optimus".to_string(),
                "NVIDIA_only".to_string(),
            );
            if let Some(json_path) = &self.nvidia_egl_vendor_json {
                env.insert(
                    "__EGL_VENDOR_LIBRARY_FILENAMES".to_string(),
                    json_path.clone(),
                );
            }
        } else {
            // Mesa PRIME offload for AMD and Intel discrete GPUs (and nouveau).
            env.insert("DRI_PRIME".to_string(), "1".to_string());
        }
    }
}
