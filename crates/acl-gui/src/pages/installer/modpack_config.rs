// SPDX-FileCopyrightText: 2026 The Adwaita Craft Launcher Authors
// SPDX-License-Identifier: GPL-3.0-or-later

use std::{path::PathBuf, sync::Arc};

use crate::install::ModpackInstallSource;
use crate::t;
use acl_core::{InstanceConfig, LauncherConfig};
use acl_minecraft::{
    AssetItemData, AssetVersion, ModpackLoader, ModpackManifest, ProjectType, ReleaseType,
    match_best_version,
};
use gtk::prelude::*;
use libadwaita::prelude::*;
use relm4::ComponentParts;
use relm4::{SharedState, prelude::*};
use tracing::error;

#[derive(Debug, Clone)]
pub enum ModpackSource {
    Online(AssetItemData),
    Local {
        path: PathBuf,
        manifest: ModpackManifest,
    },
}

#[derive(Debug)]
pub struct ModpackInit {
    pub config: Arc<SharedState<LauncherConfig>>,
    pub client: reqwest::Client,
    pub cache_dir: std::path::PathBuf,
}

#[derive(Debug)]
pub enum ModpackInput {
    Open(Box<ModpackSource>),
    NameChanged(String),
    SetManualVersion(Box<AssetVersion>),
    OpenVersionSelect,
    ConfirmInstall,
    BackClicked,
    Refresh,
}

#[derive(Debug)]
pub enum ModpackOutput {
    Back,
    OpenVersionSelect {
        title: String,
        versions: Vec<AssetVersion>,
    },
    Install {
        name: String,
        source: ModpackInstallSource,
    },

    ShowToast(String),
}

#[derive(Debug)]
pub enum ModpackCmdOutput {
    VersionsLoaded {
        request_id: u64,
        result: Result<Vec<AssetVersion>, String>,
    },
}

pub struct ModpackPage {
    config: Arc<SharedState<LauncherConfig>>,
    client: reqwest::Client,
    cache_dir: PathBuf,

    source: Option<ModpackSource>,
    default_name: String,
    instance_name: String,
    name_entry: adw::EntryRow,

    all_versions: Vec<AssetVersion>,
    /// It is None when initialized.
    selected_version: Option<AssetVersion>,
    is_loading: bool,
    request_id: u64,
}

impl ModpackPage {
    fn is_online(&self) -> bool {
        matches!(self.source, Some(ModpackSource::Online(_)))
    }
    fn is_local(&self) -> bool {
        matches!(self.source, Some(ModpackSource::Local { .. }))
    }

    fn title(&self) -> &str {
        match &self.source {
            Some(ModpackSource::Online(item)) => item.title.as_str(),
            Some(ModpackSource::Local { manifest, .. }) => manifest.name.as_str(),
            None => "",
        }
    }

    fn effective_name(&self) -> String {
        let trimmed = self.instance_name.trim();
        if !trimmed.is_empty() {
            trimmed.to_string()
        } else {
            self.default_name.clone()
        }
    }

    fn can_install(&self) -> bool {
        if InstanceConfig::validate_instance_id(&self.effective_name()).is_err() {
            return false;
        }

        match &self.source {
            Some(ModpackSource::Local { .. }) => true,
            Some(ModpackSource::Online(_)) => {
                !self.is_loading
                    && self
                        .selected_version
                        .as_ref()
                        .and_then(|v| v.primary_file.as_ref())
                        .is_some()
            }
            None => false,
        }
    }

    fn version_title(&self) -> String {
        if self.is_loading {
            return t!("installer-asset-loading-versions", "Loading versions...");
        }

        match &self.selected_version {
            Some(ver) => ver.name.clone(),
            None => t!(
                "installer-asset-no-compatible-version",
                "No compatible version found"
            ),
        }
    }

    fn version_subtitle(&self) -> String {
        if self.is_loading {
            return t!(
                "installer-asset-fetching-versions",
                "Fetching available versions..."
            );
        }

        if let Some(ver) = &self.selected_version {
            let release = match ver.release_type {
                ReleaseType::Release => t!("installer-asset-release", "Release"),
                ReleaseType::Beta => t!("installer-asset-beta", "Beta"),
                ReleaseType::Alpha => t!("installer-asset-alpha", "Alpha"),
            };

            return format!("{} · {release}", ver.version_number);
        }

        t!(
            "installer-asset-choose-manually",
            "Click to choose a version manually"
        )
    }

    fn game_version_text(&self) -> String {
        match &self.source {
            Some(ModpackSource::Local { manifest, .. }) => manifest.game_version.clone(),
            Some(ModpackSource::Online(_)) => {
                if self.is_loading {
                    return "...".to_string();
                }
                self.selected_version
                    .as_ref()
                    .filter(|v| !v.game_versions.is_empty())
                    .map(|v| v.game_versions.join(", "))
                    .unwrap_or_else(|| "-".to_string())
            }
            None => String::new(),
        }
    }

    fn loader_text(&self) -> String {
        match &self.source {
            Some(ModpackSource::Local { manifest, .. }) => match &manifest.loader {
                None => "Vanilla".to_string(),
                Some(ModpackLoader::Fabric(ver)) => format!("Fabric {ver}"),
                Some(ModpackLoader::Quilt(ver)) => format!("Quilt {ver}"),
                Some(ModpackLoader::Forge(ver)) => format!("Forge {ver}"),
                Some(ModpackLoader::NeoForge(ver)) => format!("NeoForge {ver}"),
            },
            Some(ModpackSource::Online(_)) => {
                if self.is_loading {
                    return "...".to_string();
                }
                self.selected_version
                    .as_ref()
                    .filter(|v| !v.loaders.is_empty())
                    .map(|v| v.loaders.join(", "))
                    .unwrap_or_else(|| "-".to_string())
            }
            None => String::new(),
        }
    }

    fn pack_version_text(&self) -> String {
        match &self.source {
            Some(ModpackSource::Local { manifest, .. }) => manifest.version.clone(),
            _ => String::new(),
        }
    }

    fn files_count_text(&self) -> String {
        match &self.source {
            Some(ModpackSource::Local { manifest, .. }) => manifest.files.len().to_string(),
            _ => String::new(),
        }
    }

    fn spawn_fetch_versions(&mut self, sender: &ComponentSender<Self>) {
        let Some(ModpackSource::Online(item)) = &self.source else {
            return;
        };

        let project_id = item.project_id.clone();
        let host = item.host;
        self.request_id = self.request_id.wrapping_add(1);
        let request_id = self.request_id;
        self.is_loading = true;

        let client = self.client.clone();
        let cache_dir = self.cache_dir.clone();
        let download_source = self.config.read().download_source;

        sender.oneshot_command(async move {
            let result = super::fetch_asset_versions(
                &client,
                host,
                &project_id,
                &cache_dir,
                download_source,
                false,
            )
            .await;

            ModpackCmdOutput::VersionsLoaded { request_id, result }
        });
    }
}

#[relm4::component(pub)]
impl Component for ModpackPage {
    type Init = ModpackInit;
    type Input = ModpackInput;
    type Output = ModpackOutput;
    type CommandOutput = ModpackCmdOutput;

    view! {
        #[root]
        adw::ToolbarView {
            add_top_bar = &adw::HeaderBar {
                #[wrap(Some)]
                set_title_widget = &adw::WindowTitle {
                    #[watch]
                    set_title: model.title(),
                },

                pack_start = &gtk::Button {
                    connect_clicked => ModpackInput::BackClicked,
                    adw::ButtonContent {
                        set_icon_name: "go-previous-symbolic",
                        #[watch]
                        set_label: &t!("instances-create-button-back", "Back"),
                    },
                },

                pack_end = &gtk::Button {
                    add_css_class: "suggested-action",
                    #[watch]
                    set_sensitive: model.can_install(),
                    connect_clicked => ModpackInput::ConfirmInstall,
                    #[watch]
                    set_label: &t!("instances-create-button-create", "Start Install"),
                },
            },

            #[wrap(Some)]
            set_content = &adw::PreferencesPage {
                adw::PreferencesGroup {
                    #[watch]
                    set_title: &t!("instances-create-settings-title", "Instance Settings"),

                    #[local_ref]
                    name_entry -> adw::EntryRow {
                        add_css_class: "monospace",
                        #[watch]
                        set_title: &t!("instances-create-settings-instance-name", "Instance Name"),
                        connect_changed[sender] => move |entry| {
                            sender.input(ModpackInput::NameChanged(entry.text().to_string()));
                        },
                    },
                },

                // Version selector is only shown for online modpacks
                adw::PreferencesGroup {
                    #[watch]
                    set_visible: model.is_online(),
                    #[watch]
                    set_title: &t!("installer-asset-version-title", "Version"),

                    adw::ActionRow {
                        #[watch]
                        set_title: &model.version_title(),
                        #[watch]
                        set_subtitle: &model.version_subtitle(),
                        #[watch]
                        set_activatable: !model.is_loading && !model.all_versions.is_empty(),
                        connect_activated => ModpackInput::OpenVersionSelect,
                        add_suffix = &gtk::Image::from_icon_name("go-next-symbolic"),
                    }

                },

                // Read-only modpack details
                adw::PreferencesGroup {
                    #[watch]
                    set_title: &t!("installer-modpack-info-title", "Modpack Information"),

                    adw::ActionRow {
                        #[watch]
                        set_title: &t!("instances-create-settings-game-version", "Game Version"),
                        #[watch]
                        set_subtitle: &model.game_version_text(),
                    },

                    adw::ActionRow {
                        #[watch]
                        set_title: &t!("instances-create-settings-loader-title", "Mod Loader"),
                        #[watch]
                        set_subtitle: &model.loader_text(),
                    },

                    adw::ActionRow {
                        #[watch]
                        set_visible: model.is_local(),
                        #[watch]
                        set_title: &t!("installer-modpack-version-label", "Pack Version"),
                        #[watch]
                        set_subtitle: &model.pack_version_text(),
                    },

                    adw::ActionRow {
                        #[watch]
                        set_visible: model.is_local(),
                        #[watch]
                        set_title: &t!("installer-modpack-files-label", "Modpack Files"),
                        #[watch]
                        set_subtitle: &model.files_count_text(),
                    },

                }

            },
        }
    }

    fn init(
        init: Self::Init,
        root: Self::Root,
        sender: ComponentSender<Self>,
    ) -> ComponentParts<Self> {
        init.config
            .subscribe(sender.input_sender(), |_| ModpackInput::Refresh);

        let model = Self {
            config: init.config,
            client: init.client,
            cache_dir: init.cache_dir,
            source: None,
            default_name: String::new(),
            instance_name: String::new(),
            name_entry: adw::EntryRow::new(),
            all_versions: Vec::new(),
            selected_version: None,
            is_loading: false,
            request_id: 0,
        };

        let name_entry = &model.name_entry;
        let widgets = view_output!();

        ComponentParts { model, widgets }
    }

    fn update(&mut self, msg: Self::Input, sender: ComponentSender<Self>, _root: &Self::Root) {
        match msg {
            ModpackInput::Open(source) => {
                // Invalidate any in-flight version request from a previous online modpack.
                self.request_id = self.request_id.wrapping_add(1);
                self.is_loading = false;
                self.all_versions.clear();
                self.selected_version = None;

                let raw_name = match source.as_ref() {
                    ModpackSource::Online(item) => item.title.as_str(),
                    ModpackSource::Local { manifest, .. } => manifest.name.as_str(),
                };
                let sanitized = InstanceConfig::sanitize_instance_id(raw_name);

                self.default_name = sanitized.clone();
                self.instance_name = sanitized.clone();
                self.name_entry.set_text(&sanitized);

                let is_online = matches!(source.as_ref(), ModpackSource::Online(_));
                self.source = Some(*source);

                if is_online {
                    self.spawn_fetch_versions(&sender);
                }
            }
            ModpackInput::NameChanged(name) => {
                self.instance_name = name;
            }
            ModpackInput::SetManualVersion(ver) => {
                self.selected_version = Some(*ver);
            }
            ModpackInput::OpenVersionSelect => {
                if !self.is_online() || self.all_versions.is_empty() {
                    return;
                }

                let _ = sender.output(ModpackOutput::OpenVersionSelect {
                    title: self.title().to_string(),
                    versions: self.all_versions.clone(),
                });
            }
            ModpackInput::ConfirmInstall => {
                if !self.can_install() {
                    return;
                }

                let install_source = match &self.source {
                    Some(ModpackSource::Local { path, .. }) => {
                        ModpackInstallSource::Local(path.clone())
                    }
                    Some(ModpackSource::Online(_)) => {
                        let Some(file) = self
                            .selected_version
                            .as_ref()
                            .and_then(|v| v.primary_file.clone())
                        else {
                            return;
                        };
                        ModpackInstallSource::Online(file)
                    }
                    None => return,
                };

                let _ = sender.output(ModpackOutput::Install {
                    name: self.effective_name(),
                    source: install_source,
                });
            }
            ModpackInput::BackClicked => {
                let _ = sender.output(ModpackOutput::Back);
            }
            ModpackInput::Refresh => {}
        }
    }

    fn update_cmd(
        &mut self,
        msg: Self::CommandOutput,
        sender: ComponentSender<Self>,
        _root: &Self::Root,
    ) {
        match msg {
            ModpackCmdOutput::VersionsLoaded { request_id, result } => {
                if request_id != self.request_id {
                    return;
                }

                self.is_loading = false;

                match result {
                    Ok(versions) => {
                        self.selected_version =
                            match_best_version(&versions, ProjectType::Modpack, None, None);
                        self.all_versions = versions;
                    }
                    Err(err) => {
                        error!("Failed to fetch modpack versions: {err}");
                        let _ = sender.output(ModpackOutput::ShowToast(err));
                    }
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use acl_minecraft::{AssetFile, ModpackFile, ModpackFileSource, ProjectHost};

    fn sample_online_item() -> AssetItemData {
        AssetItemData {
            host: ProjectHost::Modrinth,
            project_type: ProjectType::Modpack,
            project_id: "pack-1".to_string(),
            title: "Online Test Pack".to_string(),
            author: "Author".to_string(),
            description: "Desc".to_string(),
            icon_url: None,
            downloads: 100,
            display_categories: Vec::new(),
            latest_version: Some("1.0.0".to_string()),
        }
    }

    fn sample_asset_version(release_type: ReleaseType, with_file: bool) -> AssetVersion {
        AssetVersion {
            host: ProjectHost::Modrinth,
            id: "ver-1".to_string(),
            project_id: "pack-1".to_string(),
            name: "Pack 1.0.0".to_string(),
            version_number: "1.0.0".to_string(),
            release_type,
            game_versions: vec!["1.20.1".to_string(), "1.20.2".to_string()],
            loaders: vec!["fabric".to_string(), "quilt".to_string()],
            date_published: "2026-01-01T00:00:00Z".to_string(),
            downloads: 50,
            primary_file: with_file.then(|| AssetFile {
                url: "https://cdn.modrinth.com/pack.mrpack".to_string(),
                filename: "pack.mrpack".to_string(),
                size: 4096,
                sha1: Some("abc".to_string()),
                sha512: None,
            }),
            dependencies: Vec::new(),
        }
    }

    fn sample_local_manifest(loader: Option<ModpackLoader>) -> ModpackManifest {
        ModpackManifest {
            host: ProjectHost::Modrinth,
            name: "Local Test Pack".to_string(),
            version: "2.3.4".to_string(),
            summary: Some("Local pack".to_string()),
            author: None,
            game_version: "1.21.1".to_string(),
            loader,
            recommended_ram_mb: None,
            override_dirs: vec!["overrides".to_string()],
            files: vec![ModpackFile {
                source: ModpackFileSource::Direct {
                    path: "mods/a.jar".to_string(),
                    downloads: vec!["https://example.com/a.jar".to_string()],
                    size: 128,
                    sha1: None,
                    sha512: None,
                },
                required: true,
            }],
        }
    }

    #[test]
    fn test_modpack_page_logic() {
        // Isolate GTK initialization in a single-test subprocess so it does not
        // collide with other tests calling `gtk::init()` on different libtest threads.
        if std::env::var_os("ACL_MODPACK_GTK_TEST_CHILD").is_none() {
            let exe = std::env::current_exe().expect("failed to resolve current test executable");
            let output = std::process::Command::new(exe)
                .env("ACL_MODPACK_GTK_TEST_CHILD", "1")
                .args([
                    "--exact",
                    "pages::installer::modpack_config::tests::test_modpack_page_logic",
                    "--nocapture",
                ])
                .output()
                .expect("failed to spawn isolated GTK test subprocess");

            assert!(
                output.status.success(),
                "isolated GTK test subprocess failed (status: {:?}):\nstdout:\n{}\nstderr:\n{}",
                output.status,
                String::from_utf8_lossy(&output.stdout),
                String::from_utf8_lossy(&output.stderr),
            );
            return;
        }

        if gtk::init().is_err() {
            return;
        }

        let config = Arc::new(SharedState::new());
        let mut page = ModpackPage {
            config,
            client: reqwest::Client::new(),
            cache_dir: PathBuf::from("/tmp/cache"),
            source: None,
            default_name: "DefaultPack".to_string(),
            instance_name: String::new(),
            name_entry: adw::EntryRow::new(),
            all_versions: Vec::new(),
            selected_version: None,
            is_loading: false,
            request_id: 0,
        };

        // 1. Initial None source state
        assert!(!page.is_online());
        assert!(!page.is_local());
        assert_eq!(page.title(), "");
        assert_eq!(page.effective_name(), "DefaultPack");
        assert!(!page.can_install());
        assert_eq!(page.game_version_text(), "");
        assert_eq!(page.loader_text(), "");
        assert_eq!(page.pack_version_text(), "");
        assert_eq!(page.files_count_text(), "");

        // 2. effective_name trimming and fallback
        page.instance_name = "  Custom Pack  ".to_string();
        assert_eq!(page.effective_name(), "Custom Pack");
        page.instance_name = "   ".to_string();
        assert_eq!(page.effective_name(), "DefaultPack");

        // 3. Local modpack source
        page.source = Some(ModpackSource::Local {
            path: PathBuf::from("/tmp/local.mrpack"),
            manifest: sample_local_manifest(None),
        });
        assert!(!page.is_online());
        assert!(page.is_local());
        assert_eq!(page.title(), "Local Test Pack");
        assert!(page.can_install());
        assert_eq!(page.game_version_text(), "1.21.1");
        assert_eq!(page.loader_text(), "Vanilla");
        assert_eq!(page.pack_version_text(), "2.3.4");
        assert_eq!(page.files_count_text(), "1");

        // Invalid instance name disables install in Local mode
        page.instance_name = "invalid/name".to_string();
        assert!(!page.can_install());
        page.instance_name = "ValidLocalPack".to_string();
        assert!(page.can_install());

        // Local loader variants
        for (loader, expected) in [
            (
                Some(ModpackLoader::Fabric("0.16.0".to_string())),
                "Fabric 0.16.0",
            ),
            (
                Some(ModpackLoader::Quilt("0.26.0".to_string())),
                "Quilt 0.26.0",
            ),
            (
                Some(ModpackLoader::Forge("47.3.0".to_string())),
                "Forge 47.3.0",
            ),
            (
                Some(ModpackLoader::NeoForge("21.1.72".to_string())),
                "NeoForge 21.1.72",
            ),
        ] {
            page.source = Some(ModpackSource::Local {
                path: PathBuf::from("/tmp/local.mrpack"),
                manifest: sample_local_manifest(loader),
            });
            assert_eq!(page.loader_text(), expected);
        }

        // 4. Online modpack source
        page.source = Some(ModpackSource::Online(sample_online_item()));
        page.is_loading = true;
        page.selected_version = None;
        assert!(page.is_online());
        assert!(!page.is_local());
        assert_eq!(page.title(), "Online Test Pack");
        assert!(!page.can_install());
        assert_eq!(page.game_version_text(), "...");
        assert_eq!(page.loader_text(), "...");
        assert_eq!(page.pack_version_text(), "");
        assert_eq!(page.files_count_text(), "");
        assert!(!page.version_title().is_empty());
        assert!(!page.version_subtitle().is_empty());

        // Loaded but no version selected
        page.is_loading = false;
        assert!(!page.can_install());
        assert_eq!(page.game_version_text(), "-");
        assert_eq!(page.loader_text(), "-");
        assert!(!page.version_title().is_empty());
        assert!(!page.version_subtitle().is_empty());

        // Version selected without primary_file cannot install
        page.selected_version = Some(sample_asset_version(ReleaseType::Beta, false));
        assert!(!page.can_install());
        assert_eq!(page.version_title(), "Pack 1.0.0");
        assert!(page.version_subtitle().contains("1.0.0"));
        assert_eq!(page.game_version_text(), "1.20.1, 1.20.2");
        assert_eq!(page.loader_text(), "fabric, quilt");

        // Version selected with primary_file can install (unless loading or empty game_versions/loaders)
        let mut ver_with_file = sample_asset_version(ReleaseType::Alpha, true);
        page.selected_version = Some(ver_with_file.clone());
        assert!(page.can_install());
        assert!(page.version_subtitle().contains("1.0.0"));

        page.is_loading = true;
        assert!(!page.can_install());
        page.is_loading = false;

        ver_with_file.release_type = ReleaseType::Release;
        ver_with_file.game_versions.clear();
        ver_with_file.loaders.clear();
        page.selected_version = Some(ver_with_file);
        assert!(page.can_install());
        assert_eq!(page.game_version_text(), "-");
        assert_eq!(page.loader_text(), "-");
    }
}
