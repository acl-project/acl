use std::{path::PathBuf, sync::Arc};

use acl_core::{Instance, InstanceConfig, manager::InstanceManager};
use acl_minecraft::{VERSION_MANIFEST_URL, VersionEntry, VersionManifest, VersionType};
use gtk::prelude::*;
use libadwaita::prelude::*;

use relm4::{
    Component, ComponentParts, ComponentSender, FactorySender, RelmWidgetExt, SharedState, adw,
    factory::{FactoryComponent, FactoryVecDeque},
};
use tracing::{error, info};

#[derive(Debug, Clone)]
pub struct VersionRow {
    entry: VersionEntry,
    is_selected: bool,
}

#[derive(Debug)]
pub enum VersionRowInput {
    SetSelected(bool),
}

#[derive(Debug)]
pub enum VersionRowOutput {
    Select(String),
}

#[relm4::factory(pub)]
impl FactoryComponent for VersionRow {
    type Init = (VersionEntry, bool);
    type Input = VersionRowInput;
    type Output = VersionRowOutput;
    type CommandOutput = ();
    type ParentWidget = adw::PreferencesGroup;

    view! {
        #[root]
        adw::ActionRow {
            set_title: &self.entry.id,
            set_subtitle: &self.entry.version_type.label(),
            set_activatable: true,

            connect_activated[sender, id = self.entry.id.clone()] => move |_| {
                    let _ = sender.output(VersionRowOutput::Select(id.clone()));
            },

            #[name = "check"]
            add_suffix = &gtk::CheckButton {
                set_valign: gtk::Align::Center,
                add_css_class: "selection-mode",
                set_can_focus: false,
                set_can_target: false,
                #[watch]
                set_active: self.is_selected,
            },
        }
    }

    fn init_model(
        (entry, is_selected): Self::Init,
        _index: &Self::Index,
        _sender: FactorySender<Self>,
    ) -> Self {
        Self { entry, is_selected }
    }

    fn update(&mut self, msg: Self::Input, _sender: FactorySender<Self>) {
        match msg {
            VersionRowInput::SetSelected(selected) => {
                self.is_selected = selected;
            }
        }
    }
}

/// Filter category for Minecraft version types.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum VersionFilter {
    #[default]
    Release,
    Snapshot,
    Old,
}

impl VersionFilter {
    pub fn from_toggle_name(name: Option<&str>) -> Self {
        match name {
            Some("snapshot") => Self::Snapshot,
            Some("old") => Self::Old,
            _ => Self::Release,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            VersionFilter::Release => "release",
            VersionFilter::Snapshot => "snapshot",
            VersionFilter::Old => "old",
        }
    }

    pub fn title(self) -> &'static str {
        match self {
            VersionFilter::Release => "Release",
            VersionFilter::Snapshot => "Snapshot",
            VersionFilter::Old => "Old",
        }
    }

    pub fn matches(self, version_type: &VersionType) -> bool {
        match self {
            Self::Release => *version_type == VersionType::Release,
            Self::Snapshot => *version_type == VersionType::Snapshot,
            Self::Old => {
                matches!(version_type, VersionType::OldBeta | VersionType::OldAlpha)
            }
        }
    }
}

pub fn matches_version_entry(
    entry: &VersionEntry,
    search_query: &str,
    type_filter: VersionFilter,
) -> bool {
    if !type_filter.matches(&entry.version_type) {
        return false;
    }

    let query = search_query.trim();
    if query.is_empty() {
        return true;
    }

    entry
        .id
        .to_ascii_lowercase()
        .contains(&query.to_ascii_lowercase())
}

pub struct CreatePageInit {
    pub mgr: Arc<SharedState<InstanceManager>>,
    pub client: reqwest::Client,
    pub cache_dir: PathBuf,
}

#[derive(Debug)]
pub enum CreateInput {
    NameChanged(String),
    SearchChanged(String),
    FilterChanged(VersionFilter),
    SelectVersion(String),
    RefreshVersions,
    CreateClicked,
    CancelClicked,
}

#[derive(Debug, Clone)]
pub enum CreateOutput {
    Cancel,
    Created(String),
    ShowToast(String),
}

#[derive(Debug)]
pub enum CreateCmdOutput {
    ManifestLoaded(Result<VersionManifest, String>),
}

pub struct CreatePage {
    mgr: Arc<SharedState<InstanceManager>>,
    client: reqwest::Client,
    cache_dir: PathBuf,

    instance_name: String,
    search_query: String,
    type_filter: VersionFilter,
    selected_version: Option<String>,

    all_versions: Vec<VersionEntry>,
    version_rows: FactoryVecDeque<VersionRow>,
    is_loading: bool,
}

impl CreatePage {
    fn can_create(&self) -> bool {
        let trimmed = self.instance_name.trim();
        !trimmed.is_empty()
            && InstanceConfig::validate_instance_id(trimmed).is_ok()
            && self.selected_version.is_some()
            && !self.is_loading
    }

    fn rebuild_rows(&mut self) {
        let mut guard = self.version_rows.guard();
        guard.clear();

        for entry in &self.all_versions {
            if matches_version_entry(entry, &self.search_query, self.type_filter) {
                let is_selected = Some(&entry.id) == self.selected_version.as_ref();
                guard.push_back((entry.clone(), is_selected));
            }
        }
    }

    fn spawn_fetch_manifest(&mut self, sender: &ComponentSender<Self>, force_refresh: bool) {
        self.is_loading = true;
        let client = self.client.clone();
        let cache_dir = self.cache_dir.clone();

        sender.oneshot_command(async move {
            let result = if force_refresh {
                match VersionManifest::fetch_from_url(&client, VERSION_MANIFEST_URL).await {
                    Ok(manifest) => {
                        manifest.cache(&cache_dir);
                        Ok(manifest)
                    }

                    Err(err) => Err(err.to_string()),
                }
            } else {
                VersionManifest::fetch_with_cache(&client, &cache_dir)
                    .await
                    .map_err(|err| err.to_string())
            };

            CreateCmdOutput::ManifestLoaded(result)
        });
    }
}

#[relm4::component(pub)]
impl Component for CreatePage {
    type Init = CreatePageInit;
    type Input = CreateInput;
    type Output = CreateOutput;
    type CommandOutput = CreateCmdOutput;

    view! {
        #[root]
        adw::Clamp {
            set_maximum_size: 720,
            set_vexpand: true,
            set_margin_all: 24,

            gtk::Box {
            set_orientation: gtk::Orientation::Vertical,
            set_spacing: 16,

            // Top Action Bar
            gtk::Box {
                set_orientation: gtk::Orientation::Horizontal,

                gtk::Button {
                        connect_clicked => CreateInput::CancelClicked,

                        adw::ButtonContent {
                            set_icon_name: "go-previous-symbolic",
                            set_label: "Back",
                        },
                    },

                gtk::Button {
                    set_hexpand: true,
                    set_halign: gtk::Align::End,
                    add_css_class: "suggested-action",
                    #[watch]
                    set_sensitive: model.can_create(),
                    connect_clicked => CreateInput::CreateClicked,

                    adw::ButtonContent {
                        set_icon_name: "list-add-symbolic",
                        set_label: "Create",
                    },
                },
            },


            // Instance settings section

            adw::PreferencesGroup {
                set_title: "Instance Settings",

                adw::EntryRow {
                            set_title: "Instance Name",
                            connect_changed[sender] => move |entry| {
                                sender.input(CreateInput::NameChanged(entry.text().to_string()));
                            },
                },
            },


            // Game version browser

            #[local_ref]
            versions_group -> adw::PreferencesGroup {
                set_title: "Game Version",
                set_description: Some("Select the Minecraft version you want to install"),

                #[wrap(Some)]
                set_header_suffix = &gtk::Box {
                    set_spacing: 8,
                    gtk::SearchEntry {
                        set_placeholder_text: Some("Search versions..."),
                        connect_search_changed[sender] => move |entry| {
                            sender.input(CreateInput::SearchChanged(entry.text().to_string()));
                        },
                    },

                    adw::ToggleGroup {
                        set_active_name: Some("release"),
                        connect_active_name_notify[sender] => move |group| {
                            let filter =  VersionFilter::from_toggle_name(group.active_name().as_deref());
                            sender.input(CreateInput::FilterChanged(filter));
                        },

                        add = adw::Toggle {
                            set_name: Some(VersionFilter::Release.as_str()),
                            set_label: Some(VersionFilter::Release.title())
                        },

                        add = adw::Toggle {
                            set_name: Some(VersionFilter::Snapshot.as_str()),
                            set_label: Some(VersionFilter::Snapshot.title())
                        },

                        add = adw::Toggle {
                            set_name: Some(VersionFilter::Old.as_str()),
                            set_label: Some(VersionFilter::Old.title())
                        },

                    },

                    gtk::Button {
                        set_icon_name: "view-refresh-symbolic",
                        #[watch]
                        set_sensitive: !model.is_loading,
                        connect_clicked => CreateInput::RefreshVersions,
                    },

                }

            }

        }

        },

    }

    fn init(
        init: Self::Init,
        root: Self::Root,
        sender: ComponentSender<Self>,
    ) -> ComponentParts<Self> {
        let version_rows = FactoryVecDeque::builder()
            .launch(adw::PreferencesGroup::default())
            .forward(sender.input_sender(), |output| match output {
                VersionRowOutput::Select(id) => CreateInput::SelectVersion(id),
            });

        let mut model = Self {
            mgr: init.mgr,
            client: init.client,
            cache_dir: init.cache_dir,
            instance_name: String::new(),
            search_query: String::new(),
            type_filter: VersionFilter::Release,
            selected_version: None,
            all_versions: Vec::new(),
            version_rows,
            is_loading: false,
        };

        model.spawn_fetch_manifest(&sender, false);

        let versions_group = model.version_rows.widget();
        let widgets = view_output!();

        ComponentParts { model, widgets }
    }

    fn update(&mut self, msg: Self::Input, sender: ComponentSender<Self>, _root: &Self::Root) {
        match msg {
            CreateInput::NameChanged(name) => {
                self.instance_name = name;
            }
            CreateInput::SearchChanged(query) => {
                self.search_query = query;
                self.rebuild_rows();
            }
            CreateInput::FilterChanged(filter) => {
                self.type_filter = filter;
                self.rebuild_rows();
            }

            CreateInput::SelectVersion(id) => {
                self.selected_version = Some(id.clone());

                // Update row checkbox states cleanly without recreating widgets
                for (i, row) in self.version_rows.iter().enumerate() {
                    let is_selected = row.entry.id == id;
                    self.version_rows
                        .send(i, VersionRowInput::SetSelected(is_selected));
                }
            }

            CreateInput::RefreshVersions => {
                self.spawn_fetch_manifest(&sender, true);
            }
            CreateInput::CancelClicked => {
                let _ = sender.output(CreateOutput::Cancel);
            }

            // Create version metadata without downloading resource files, file downloads are handled upstream.
            CreateInput::CreateClicked => {
                let id = self.instance_name.trim().to_string();
                let Some(version_id) = self.selected_version.as_ref() else {
                    let _ = sender.output(CreateOutput::ShowToast(
                        "Please select a Minecraft version".to_string(),
                    ));
                    return;
                };

                let versions_dir = self.mgr.read().versions_dir.clone();
                match Instance::create(&id, &id, version_id, &versions_dir) {
                    Ok(_) => {
                        info!("Created instance '{}' with version '{}'", id, version_id);
                        if let Err(err) = self.mgr.write().refresh() {
                            let msg = format!("Failed to refresh instances: {err}");
                            error!("{msg}");
                            let _ = sender.output(CreateOutput::ShowToast(msg));
                        }
                        let _ = sender.output(CreateOutput::Created(id));
                    }
                    Err(err) => {
                        let msg = format!("Failed to create instance: {err}");
                        error!("{msg}");
                        let _ = sender.output(CreateOutput::ShowToast(msg));
                    }
                }
            }
        }
    }

    fn update_cmd(
        &mut self,
        msg: Self::CommandOutput,
        sender: ComponentSender<Self>,
        _root: &Self::Root,
    ) {
        match msg {
            CreateCmdOutput::ManifestLoaded(result) => {
                self.is_loading = false;
                match result {
                    Ok(manifest) => {
                        self.all_versions = manifest.versions;
                        // Auto-select latest release version
                        if self.selected_version.is_none() {
                            self.selected_version = Some(manifest.latest.release);
                        }
                        self.rebuild_rows();
                    }

                    Err(err) => {
                        let msg = format!("Failed to load version manifest: {err}");
                        error!("{msg}");
                        let _ = sender.output(CreateOutput::ShowToast(msg));
                    }
                }
            }
        }
    }
}
