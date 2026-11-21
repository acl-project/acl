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

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};

    struct TempDirGuard {
        path: PathBuf,
    }

    impl TempDirGuard {
        fn new(name: &str) -> Self {
            static COUNTER: AtomicUsize = AtomicUsize::new(0);
            let id = COUNTER.fetch_add(1, Ordering::SeqCst);
            let path = std::env::temp_dir().join(format!(
                "acl_test_gpu_{}_{}_{}",
                name,
                std::process::id(),
                id
            ));
            let _ = fs::remove_dir_all(&path);
            fs::create_dir_all(&path).expect("failed to create temp test dir");
            Self { path }
        }

        fn path(&self) -> &Path {
            &self.path
        }
    }

    impl Drop for TempDirGuard {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.path);
        }
    }

    #[test]
    fn test_find_nvidia_egl_vendor_json_fast_path_and_priority() {
        let temp = TempDirGuard::new("fast_path");
        let dir1 = temp.path().join("dir1");
        let dir2 = temp.path().join("dir2");
        let dir3 = temp.path().join("dir3");
        fs::create_dir_all(&dir1).expect("create dir1");
        fs::create_dir_all(&dir2).expect("create dir2");
        fs::create_dir_all(&dir3).expect("create dir3");

        // dir2 has 10_nvidia.json and an earlier lexicographical fallback; fast path selects 10_nvidia.json.
        let dir2_fast = dir2.join("10_nvidia.json");
        fs::write(&dir2_fast, "{}").expect("write dir2 10_nvidia.json");
        fs::write(dir2.join("05_nvidia.json"), "{}").expect("write dir2 05_nvidia.json");

        // dir3 also has 10_nvidia.json, but dir2 comes earlier in search order.
        fs::write(dir3.join("10_nvidia.json"), "{}").expect("write dir3 10_nvidia.json");

        let found = find_nvidia_egl_vendor_json(&[dir1, dir2, dir3]);
        assert_eq!(found, Some(dir2_fast.to_string_lossy().into_owned()));
    }

    #[test]
    fn test_find_nvidia_egl_vendor_json_fallback_lexicographical() {
        let temp = TempDirGuard::new("fallback");
        let dir = temp.path().join("egl_vendor.d");
        fs::create_dir_all(&dir).expect("create vendor dir");

        // Non-matching files and directories named 10_nvidia.json must be ignored.
        fs::write(dir.join("50_mesa.json"), "{}").expect("write 50_mesa.json");
        fs::write(dir.join("10_nvidia.txt"), "{}").expect("write 10_nvidia.txt");
        fs::create_dir_all(dir.join("10_nvidia.json")).expect("create dir named 10_nvidia.json");

        let expected = dir.join("15_NVIDIA.json");
        fs::write(&expected, "{}").expect("write 15_NVIDIA.json");
        fs::write(dir.join("20_nvidia_custom.json"), "{}").expect("write 20_nvidia_custom.json");

        let found = find_nvidia_egl_vendor_json(&[dir]);
        assert_eq!(found, Some(expected.to_string_lossy().into_owned()));
    }

    #[test]
    fn test_find_nvidia_egl_vendor_json_empty_or_missing_dirs() {
        let temp = TempDirGuard::new("missing");
        let missing_dir = temp.path().join("does_not_exist");
        let empty_dir = temp.path().join("empty");
        let mesa_only_dir = temp.path().join("mesa_only");
        fs::create_dir_all(&empty_dir).expect("create empty dir");
        fs::create_dir_all(&mesa_only_dir).expect("create mesa_only dir");
        fs::write(mesa_only_dir.join("50_mesa.json"), "{}").expect("write 50_mesa.json");

        assert_eq!(find_nvidia_egl_vendor_json(&[]), None);
        assert_eq!(
            find_nvidia_egl_vendor_json(&[missing_dir, empty_dir, mesa_only_dir]),
            None
        );
    }

    #[test]
    fn test_apply_vars_nvidia_with_and_without_egl_json() {
        let mut with_json_env = BTreeMap::new();
        let with_json = DiscreteGpuEnv {
            has_nvidia_driver: true,
            nvidia_egl_vendor_json: Some("/custom/glvnd/10_nvidia.json".to_string()),
        };
        with_json.apply_vars(&mut with_json_env);
        assert_eq!(
            with_json_env,
            BTreeMap::from([
                (
                    "__EGL_VENDOR_LIBRARY_FILENAMES".to_string(),
                    "/custom/glvnd/10_nvidia.json".to_string()
                ),
                (
                    "__GLX_VENDOR_LIBRARY_NAME".to_string(),
                    "nvidia".to_string()
                ),
                ("__NV_PRIME_RENDER_OFFLOAD".to_string(), "1".to_string()),
                (
                    "__VK_LAYER_NV_optimus".to_string(),
                    "NVIDIA_only".to_string()
                ),
            ])
        );

        let mut without_json_env = BTreeMap::new();
        let without_json = DiscreteGpuEnv {
            has_nvidia_driver: true,
            nvidia_egl_vendor_json: None,
        };
        without_json.apply_vars(&mut without_json_env);
        assert_eq!(
            without_json_env,
            BTreeMap::from([
                (
                    "__GLX_VENDOR_LIBRARY_NAME".to_string(),
                    "nvidia".to_string()
                ),
                ("__NV_PRIME_RENDER_OFFLOAD".to_string(), "1".to_string()),
                (
                    "__VK_LAYER_NV_optimus".to_string(),
                    "NVIDIA_only".to_string()
                ),
            ])
        );
    }

    #[test]
    fn test_apply_vars_mesa_fallback() {
        let mut env = BTreeMap::new();
        let gpu = DiscreteGpuEnv {
            has_nvidia_driver: false,
            nvidia_egl_vendor_json: None,
        };
        gpu.apply_vars(&mut env);
        assert_eq!(
            env,
            BTreeMap::from([("DRI_PRIME".to_string(), "1".to_string())])
        );
    }
}
