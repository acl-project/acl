// SPDX-FileCopyrightText: 2026 The Adwaita Craft Launcher Authors
// SPDX-License-Identifier: GPL-3.0-or-later

use std::{path::PathBuf, sync::Arc};

use acl_core::ModLoader;
use acl_core::{Instance, LauncherConfig, manager::InstanceManager};
use acl_minecraft::{
    AssetItemData, AssetVersion, DependencyType, ProjectType, ReleaseType, match_best_version,
};
use gtk::prelude::*;
use libadwaita::prelude::*;
use relm4::ComponentParts;
use relm4::{SharedState, prelude::*};
use tracing::error;

use crate::t;
/// Install target mode
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum TargetMode {
    #[default]
    ExistingInstance,
    CustomDirectory,
}

/// Final confirmation of the target location for installation
#[derive(Debug, Clone)]
pub enum InstallTarget {
    Instance(Box<Instance>),
    CustomDir(PathBuf),
}

/// Parsed dependency display item
#[derive(Debug, Clone)]
pub struct ResolvedDep {
    pub project_id: String,
    pub dependency_type: DependencyType,
    pub item: Option<AssetItemData>,
}

pub struct AssetConfigInit {
    pub mgr: Arc<SharedState<InstanceManager>>,
    pub config: Arc<SharedState<LauncherConfig>>,
    pub client: reqwest::Client,
    pub cache_dir: PathBuf,
}

#[derive(Debug)]
pub enum AssetConfigInput {
    Open(Box<AssetItemData>),
    /// Return after manually selecting the version from VersionSelectPage
    SetManualVersion(Box<AssetVersion>),
    /// Switch mode: install to instance/custom directory
    ModeChanged(TargetMode),
    /// Switch target instance index
    InstanceSelected(usize),
    ChooseCustomDir,
    /// Folder selection popup callback
    CustomDirPicked(PathBuf),
    OpenVersionSelect,
    OpenDependency(ResolvedDep),
    ConfirmInstall,
    BackClicked,
    Refresh,
    /// If the instance list is updated, the drop-down box will be automatically updated.
    InstancesUpdated,
    OpenManualDownload,
}

#[derive(Debug, Clone)]
pub enum AssetConfigOutput {
    Back,
    OpenVersionSelect {
        title: String,
        target_mc_version: Option<String>,
        versions: Vec<AssetVersion>,
    },
    OpenDependency(Box<ResolvedDep>),
    Install {
        item: Box<AssetItemData>,
        version: Box<AssetVersion>,
        target: InstallTarget,
    },
    ShowToast(String),
}

#[derive(Debug)]
pub enum AssetConfigCmdOutput {
    VersionsLoaded {
        request_id: u64,
        result: Result<Vec<AssetVersion>, String>,
    },
    /// Callback after batch pulling of pre-project information is completed
    DepsLoaded {
        request_id: u64,
        deps: Result<Vec<ResolvedDep>, String>,
    },
}

pub struct AssetConfigPage {
    mgr: Arc<SharedState<InstanceManager>>,
    config: Arc<SharedState<LauncherConfig>>,
    client: reqwest::Client,
    cache_dir: PathBuf,

    item: Option<AssetItemData>,
    all_versions: Vec<AssetVersion>,
    is_auto_matched: bool,
    is_loading: bool,
    request_id: u64,
    dep_request_id: u64,

    selected_version: Option<AssetVersion>,
    selected_instance_idx: usize,

    target_mode: TargetMode,
    custom_dir: Option<PathBuf>,

    dep_group: adw::PreferencesGroup,
    dep_rows: Vec<adw::ActionRow>,

    instance_model: gtk::StringList,
}

impl AssetConfigPage {
    fn manual_download_url(&self) -> Option<String> {
        self.selected_version
            .as_ref()
            .and_then(super::manual_download_url)
    }

    fn selected_instance(&self) -> Option<Instance> {
        self.mgr
            .read()
            .instances
            .get(self.selected_instance_idx)
            .cloned()
    }

    fn can_install(&self) -> bool {
        if self.is_loading {
            return false;
        }

        let has_file = self
            .selected_version
            .as_ref()
            .and_then(|v| v.primary_file.as_ref())
            .is_some();
        if !has_file {
            return false;
        }

        match self.target_mode {
            TargetMode::ExistingInstance => self.selected_instance().is_some(),
            TargetMode::CustomDirectory => self.custom_dir.is_some(),
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
            let source = if self.is_auto_matched {
                t!("installer-asset-auto-matched", "Auto-matched")
            } else {
                t!("installer-asset-manual-selected", "Manual")
            };

            let release = match ver.release_type {
                ReleaseType::Release => t!("installer-asset-release", "Release"),
                ReleaseType::Beta => t!("installer-asset-beta", "Beta"),
                ReleaseType::Alpha => t!("installer-asset-alpha", "Alpha"),
            };

            return format!("{source} · {} · {release}", ver.version_number);
        }

        let is_vanilla_for_mod = self.target_mode == TargetMode::ExistingInstance
            && self
                .item
                .as_ref()
                .is_some_and(|i| i.project_type == ProjectType::Mod)
            && self
                .selected_instance()
                .is_some_and(|inst| inst.config.mod_loader == ModLoader::Vanilla);

        if is_vanilla_for_mod {
            t!(
                "installer-asset-vanilla-unsupported",
                "Vanilla instance does not support mods"
            )
        } else {
            t!(
                "installer-asset-choose-manually",
                "Click to choose a version manually"
            )
        }
    }

    fn sync_instances(&mut self) {
        let mgr = self.mgr.read();
        let labels: Vec<String> = mgr
            .instances
            .iter()
            .map(|inst| {
                format!(
                    "{} ({} · {})",
                    inst.config.name,
                    inst.config.game_version,
                    inst.config.mod_loader.label()
                )
            })
            .collect();

        // No changes avoid updating
        let unchanged = self.instance_model.n_items() as usize == labels.len()
            && labels.iter().enumerate().all(|(i, label)| {
                self.instance_model
                    .string(i as u32)
                    .is_some_and(|s| s.as_str() == label)
            });

        if !unchanged {
            let refs: Vec<&str> = labels.iter().map(String::as_str).collect();
            self.instance_model
                .splice(0, self.instance_model.n_items(), &refs);
        }
        // The lock is taken here, the has_instances method cannot be used
        if mgr.instances.is_empty() {
            self.selected_instance_idx = 0;
            self.target_mode = TargetMode::CustomDirectory;
        } else if self.selected_instance_idx >= mgr.instances.len() {
            self.selected_instance_idx = 0;
        }
    }

    fn has_instances(&self) -> bool {
        !self.mgr.read().instances.is_empty()
    }

    fn select_active_instance(&mut self) {
        let mgr = self.mgr.read();
        self.selected_instance_idx = mgr
            .config
            .active_id
            .as_deref()
            .and_then(|active_id| mgr.instances.iter().position(|inst| inst.id() == active_id))
            .unwrap_or(0);
    }

    fn spawn_fetch_versions(&mut self, sender: &ComponentSender<Self>) {
        let Some(item) = self.item.clone() else {
            return;
        };

        // Increment request_id to avoid old data affecting new ones
        self.request_id = self.request_id.wrapping_add(1);
        let request_id = self.request_id;
        self.is_loading = true;

        let client = self.client.clone();
        let cache_dir = self.cache_dir.clone();
        let download_source = self.config.read().download_source;

        sender.oneshot_command(async move {
            let result = super::fetch_asset_versions(
                &client,
                item.host,
                &item.project_id,
                &cache_dir,
                download_source,
                false,
            )
            .await;

            AssetConfigCmdOutput::VersionsLoaded { request_id, result }
        });
    }

    fn recompute_auto_version(&mut self, sender: &ComponentSender<Self>) {
        let Some(item) = &self.item else {
            return;
        };

        let selected_inst = self.selected_instance();
        let (mc_ver, loader) = match self.target_mode {
            TargetMode::ExistingInstance => match &selected_inst {
                Some(inst) => (
                    Some(inst.config.game_version.as_str()),
                    inst.config.mod_loader.as_modrinth_loader(),
                ),
                None => (None, None),
            },
            TargetMode::CustomDirectory => (None, None),
        };

        self.selected_version =
            match_best_version(&self.all_versions, item.project_type, mc_ver, loader);
        self.is_auto_matched = self.selected_version.is_some();
        self.rebuild_dependencies(sender);
    }

    fn render_dependencies(&mut self, deps: &[ResolvedDep], sender: &ComponentSender<Self>) {
        for row in self.dep_rows.drain(..) {
            self.dep_group.remove(&row);
        }

        self.dep_group.set_visible(!deps.is_empty());

        for dep in deps {
            let is_loaded = dep.item.is_some();

            let title = dep.item.as_ref().map_or_else(
                || t!("installer-asset-dep-loading", "Loading..."),
                |i| i.title.clone(),
            );

            let subtitle = match dep.dependency_type {
                DependencyType::Required => t!("installer-asset-dep-required", "Required"),
                DependencyType::Optional => t!("installer-asset-dep-optional", "Optional"),
                _ => String::new(),
            };

            let row = adw::ActionRow::builder()
                .title(title)
                .subtitle(subtitle)
                .activatable(is_loaded)
                .build();

            if is_loaded {
                row.add_suffix(&gtk::Image::from_icon_name("go-next-symbolic"));
                let sender_clone = sender.clone();
                let dep_clone = dep.clone();
                row.connect_activated(move |_| {
                    sender_clone.input(AssetConfigInput::OpenDependency(dep_clone.clone()));
                });
            } else {
                row.add_suffix(&adw::Spinner::new());
            }

            self.dep_group.add(&row);
            self.dep_rows.push(row);
        }
    }

    fn render_dependencies_error(&mut self, err: &str) {
        for row in self.dep_rows.drain(..) {
            self.dep_group.remove(&row);
        }

        let row = adw::ActionRow::builder()
            .title(t!(
                "installer-asset-dep-load-failed",
                "Failed to load dependencies"
            ))
            .subtitle(err)
            .build();
        row.add_prefix(&gtk::Image::from_icon_name("dialog-error-symbolic"));

        self.dep_group.set_visible(true);
        self.dep_group.add(&row);
        self.dep_rows.push(row);
    }

    fn rebuild_dependencies(&mut self, sender: &ComponentSender<Self>) {
        let deps: Vec<ResolvedDep> = self
            .selected_version
            .as_ref()
            .map(|ver| {
                ver.dependencies
                    .iter()
                    .filter(|d| {
                        matches!(
                            d.dependency_type,
                            DependencyType::Required | DependencyType::Optional
                        )
                    })
                    .map(|d| ResolvedDep {
                        project_id: d.project_id.clone(),
                        dependency_type: d.dependency_type,
                        item: None, // There are currently no resolved dependencies
                    })
                    .collect()
            })
            .unwrap_or_default();

        self.render_dependencies(&deps, sender);

        self.dep_request_id = self.dep_request_id.wrapping_add(1);
        let request_id = self.dep_request_id;

        if deps.is_empty() {
            return;
        }

        let Some(host) = self.selected_version.as_ref().map(|v| v.host) else {
            return;
        };

        let ids: Vec<String> = deps.iter().map(|d| d.project_id.clone()).collect();

        let client = self.client.clone();
        let cache_dir = self.cache_dir.clone();
        let download_source = self.config.read().download_source;

        sender.oneshot_command(async move {
            let map = match super::fetch_project_details(
                &client,
                host,
                &ids,
                &cache_dir,
                download_source,
                false,
            )
            .await
            {
                Ok(m) => m,
                Err(e) => {
                    return AssetConfigCmdOutput::DepsLoaded {
                        request_id,
                        deps: Err(e),
                    };
                }
            };

            let resolved = deps
                .into_iter()
                .map(|mut d| {
                    d.item = map.get(&d.project_id).cloned();
                    d
                })
                .collect();

            AssetConfigCmdOutput::DepsLoaded {
                request_id,
                deps: Ok(resolved),
            }
        });
    }
}

#[relm4::component(pub)]
impl Component for AssetConfigPage {
    type Init = AssetConfigInit;
    type Input = AssetConfigInput;
    type Output = AssetConfigOutput;
    type CommandOutput = AssetConfigCmdOutput;

    view! {
        #[root]
        adw::ToolbarView {
            add_top_bar = &adw::HeaderBar {
                #[wrap(Some)]
                set_title_widget = &adw::WindowTitle {
                    #[watch]
                    set_title: model
                        .item
                        .as_ref()
                        .map_or("", |i| i.title.as_str()),
                },

                pack_start = &gtk::Button {
                    connect_clicked => AssetConfigInput::BackClicked,
                    adw::ButtonContent {
                        set_icon_name: "go-previous-symbolic",
                        #[watch]
                        set_label: &t!("instances-create-button-back", "Back"),
                    }
                },

                pack_end = &gtk::Button {
                    add_css_class: "suggested-action",
                    #[watch]
                    set_sensitive: model.can_install(),
                    connect_clicked => AssetConfigInput::ConfirmInstall,
                    #[watch]
                    set_label: &t!("instances-create-button-create", "Start Install"),
                }

            },

            #[wrap(Some)]
            set_content = &adw::PreferencesPage {
                // Installation location
                adw::PreferencesGroup {
                    #[watch]
                    set_title: &t!("installer-asset-target-title", "Install Destination"),

                    adw::ComboRow {
                        #[watch]
                        set_title: &t!("installer-asset-install-mode", "Install Mode"),
                        #[watch]
                        set_sensitive: model.has_instances(),

                        #[watch]
                        #[block_signal(mode_handler)]
                        set_model: Some(&gtk::StringList::new(&[
                            &t!("installer-asset-mode-instance", "Install to Instance"),
                            &t!("installer-asset-mode-custom", "Custom Folder"),
                        ])),
                        #[watch]
                        #[block_signal(mode_handler)]
                        set_selected: match model.target_mode {
                            TargetMode::ExistingInstance => 0,
                            TargetMode::CustomDirectory => 1,
                        },

                        connect_selected_notify[sender] => move |combo| {
                            let mode = match combo.selected() {
                                1 => TargetMode::CustomDirectory,
                                _ => TargetMode::ExistingInstance,
                            };
                            sender.input(AssetConfigInput::ModeChanged(mode));
                        } @mode_handler,
                    },

                    adw::ComboRow {
                        #[watch]
                        set_title: &t!("installer-asset-target-instance", "Target Instance"),
                        set_model: Some(&model.instance_model),
                        #[watch]
                        set_visible: model.target_mode == TargetMode::ExistingInstance && model.has_instances(),

                        #[watch]
                        #[block_signal(instance_handler)]
                        set_selected: if model.has_instances() {
                            model.selected_instance_idx as u32
                        } else {
                            gtk::INVALID_LIST_POSITION
                        },

                        connect_selected_notify[sender] => move |combo| {
                            let idx = combo.selected();

                            if idx != gtk::INVALID_LIST_POSITION {
                                sender.input(AssetConfigInput::InstanceSelected(idx as usize));
                            }

                        } @instance_handler,
                    },

                    adw::ActionRow {
                        #[watch]
                        set_title: &t!("installer-asset-custom-folder", "Destination Folder"),
                        #[watch]
                        set_subtitle: &model
                            .custom_dir
                            .as_ref()
                            .map(|p| p.display().to_string())
                            .unwrap_or_else(|| t!("installer-asset-no-folder", "Not selected")),
                        #[watch]
                        set_visible: model.target_mode == TargetMode::CustomDirectory,
                        set_activatable: true,
                        connect_activated => AssetConfigInput::ChooseCustomDir,

                        add_suffix = &gtk::Button {
                            set_icon_name: "folder-open-symbolic",
                            set_valign: gtk::Align::Center,
                            add_css_class: "flat",
                            connect_clicked => AssetConfigInput::ChooseCustomDir,
                        },
                    },
                },

                // Version card (click to enter VersionSelectPage for manual selection)
                adw::PreferencesGroup {
                    #[watch]
                    set_title: &t!("installer-asset-version-title", "Version"),

                    adw::ActionRow {
                        #[watch]
                        set_title: &model.version_title(),
                        #[watch]
                        set_subtitle: &model.version_subtitle(),
                        #[watch]
                        set_activatable: !model.is_loading && !model.all_versions.is_empty(),
                        connect_activated => AssetConfigInput::OpenVersionSelect,
                        add_suffix = &gtk::Image::from_icon_name("go-next-symbolic"),
                    },

                    adw::ActionRow {
                        #[watch]
                        set_visible: !model.is_loading && model.manual_download_url().is_some(),
                        #[watch]
                        set_title: &t!(
                            "installer-asset-manual-download-title",
                            "Manual download required"
                        ),
                        #[watch]
                        set_subtitle: &t!(
                            "installer-asset-manual-download-subtitle",
                            "The author disabled third-party downloads for this file. Download it from CurseForge in your browser."
                        ),
                        add_prefix = &gtk::Image {
                            set_icon_name: Some("dialog-warning-symbolic"),
                            add_css_class: "warning",
                        },
                        add_suffix = &gtk::Button {
                            set_valign: gtk::Align::Center,
                            #[watch]
                            set_label: &t!("installer-asset-open-curseforge", "Open"),
                            connect_clicked => AssetConfigInput::OpenManualDownload,
                        },
                    },

                },

                // Dependency remind (automatically hidden when no dependencies)
                #[local_ref]
                dep_group -> adw::PreferencesGroup {
                    #[watch]
                    set_title: &t!("installer-asset-deps-title", "Dependencies"),
                }

            },
        },
    }

    fn init(
        init: Self::Init,
        root: Self::Root,
        sender: ComponentSender<Self>,
    ) -> ComponentParts<Self> {
        init.config
            .subscribe(sender.input_sender(), |_| AssetConfigInput::Refresh);
        init.mgr.subscribe(sender.input_sender(), |_| {
            AssetConfigInput::InstancesUpdated
        });

        let dep_group = adw::PreferencesGroup::new();
        dep_group.set_visible(false);

        let mut model = Self {
            mgr: init.mgr,
            config: init.config,
            client: init.client,
            cache_dir: init.cache_dir,
            item: None,
            all_versions: Vec::new(),
            is_auto_matched: false,
            is_loading: false,
            request_id: 0,
            dep_request_id: 0,
            selected_version: None,
            selected_instance_idx: 0,
            target_mode: TargetMode::ExistingInstance,
            custom_dir: None,
            dep_group,
            dep_rows: Vec::new(),
            instance_model: gtk::StringList::new(&[]),
        };

        model.sync_instances();
        model.select_active_instance();

        let dep_group = &model.dep_group;
        let widgets = view_output!();
        ComponentParts { model, widgets }
    }

    fn update(&mut self, msg: Self::Input, sender: ComponentSender<Self>, root: &Self::Root) {
        match msg {
            AssetConfigInput::Open(item) => {
                self.item = Some(*item);
                self.all_versions.clear();
                self.selected_version = None;
                self.is_auto_matched = false;
                self.sync_instances();
                self.select_active_instance();
                if self.has_instances() {
                    self.target_mode = TargetMode::ExistingInstance;
                }
                self.rebuild_dependencies(&sender);
                self.spawn_fetch_versions(&sender);
            }

            AssetConfigInput::InstancesUpdated => {
                self.sync_instances();
                self.select_active_instance();
                if self.is_auto_matched && !self.is_loading {
                    self.recompute_auto_version(&sender);
                }
            }

            AssetConfigInput::SetManualVersion(ver) => {
                self.selected_version = Some(*ver);
                self.is_auto_matched = false;
                self.rebuild_dependencies(&sender);
            }

            AssetConfigInput::ModeChanged(mode) => {
                if self.target_mode != mode {
                    self.target_mode = mode;
                    self.recompute_auto_version(&sender);
                }
            }

            AssetConfigInput::InstanceSelected(idx) => {
                if self.selected_instance_idx != idx {
                    self.selected_instance_idx = idx;
                    self.recompute_auto_version(&sender);
                }
            }

            AssetConfigInput::ChooseCustomDir => {
                let dialog = gtk::FileDialog::builder()
                    .title(t!(
                        "installer-asset-choose-folder",
                        "Choose Destination Folder"
                    ))
                    .build();

                let window = root.root().and_then(|r| r.downcast::<gtk::Window>().ok());
                let sender_clone = sender.clone();

                dialog.select_folder(window.as_ref(), gtk::gio::Cancellable::NONE, move |res| {
                    if let Ok(file) = res
                        && let Some(path) = file.path()
                    {
                        sender_clone.input(AssetConfigInput::CustomDirPicked(path));
                    }
                });
            }

            AssetConfigInput::CustomDirPicked(path) => {
                self.custom_dir = Some(path);
            }

            AssetConfigInput::OpenVersionSelect => {
                let Some(item) = &self.item else {
                    return;
                };

                let target_mc_version = match self.target_mode {
                    TargetMode::ExistingInstance => self
                        .selected_instance()
                        .map(|inst| inst.config.game_version),
                    TargetMode::CustomDirectory => None,
                };

                let _ = sender.output(AssetConfigOutput::OpenVersionSelect {
                    title: item.title.clone(),
                    target_mc_version,
                    versions: self.all_versions.clone(),
                });
            }

            AssetConfigInput::OpenDependency(dep) => {
                let _ = sender.output(AssetConfigOutput::OpenDependency(Box::new(dep)));
            }

            AssetConfigInput::ConfirmInstall => {
                let (Some(item), Some(version)) =
                    (self.item.clone(), self.selected_version.clone())
                else {
                    return;
                };

                let target = match self.target_mode {
                    TargetMode::ExistingInstance => match self.selected_instance() {
                        Some(inst) => InstallTarget::Instance(Box::new(inst)),
                        None => return,
                    },
                    TargetMode::CustomDirectory => match self.custom_dir.clone() {
                        Some(path) => InstallTarget::CustomDir(path),
                        None => return,
                    },
                };

                let _ = sender.output(AssetConfigOutput::Install {
                    item: Box::new(item),
                    version: Box::new(version),
                    target,
                });
            }

            AssetConfigInput::BackClicked => {
                let _ = sender.output(AssetConfigOutput::Back);
            }

            AssetConfigInput::OpenManualDownload => {
                if let Some(url) = self.manual_download_url()
                    && let Err(err) = gtk::gio::AppInfo::launch_default_for_uri(
                        &url,
                        gtk::gio::AppLaunchContext::NONE,
                    )
                {
                    error!("Failed to open CurseForge download page: {err}");
                    let _ = sender.output(AssetConfigOutput::ShowToast(t!(
                        "installer-asset-error-open-browser",
                        "Failed to open browser: {err}",
                        err = err
                    )));
                }
            }

            AssetConfigInput::Refresh => {}
        }
    }

    fn update_cmd(
        &mut self,
        msg: Self::CommandOutput,
        sender: ComponentSender<Self>,
        _root: &Self::Root,
    ) {
        match msg {
            AssetConfigCmdOutput::VersionsLoaded { request_id, result } => {
                if request_id != self.request_id {
                    return;
                }

                self.is_loading = false;

                match result {
                    Ok(versions) => {
                        self.all_versions = versions;
                        self.recompute_auto_version(&sender);
                    }
                    Err(err) => {
                        error!("Failed to fetch asset versions: {err}");
                        let _ = sender.output(AssetConfigOutput::ShowToast(err));
                    }
                }
            }
            AssetConfigCmdOutput::DepsLoaded { request_id, deps } => {
                if request_id != self.dep_request_id {
                    return;
                }

                match deps {
                    Ok(resolved) => self.render_dependencies(&resolved, &sender),
                    Err(err) => {
                        error!("Failed to fetch asset dependencies: {err}");
                        let _ = sender.output(AssetConfigOutput::ShowToast(err.clone()));
                        self.render_dependencies_error(&err);
                    }
                }
            }
        }
    }
}
