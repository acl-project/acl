use std::{cell::RefCell, path::PathBuf, rc::Rc, sync::Arc};

use acl_core::{Instance, InstanceConfig, manager::InstanceManager};
use acl_minecraft::{VERSION_MANIFEST_URL, VersionEntry, VersionManifest, VersionType};
use gtk::{glib::BoxedAnyObject, prelude::*};
use libadwaita::prelude::*;

use relm4::{
    Component, ComponentParts, ComponentSender, RelmWidgetExt, SharedState, adw,
    typed_view::list::{RelmListItem, TypedListView},
};
use tracing::{error, info};

/// A single Minecraft version shown in the virtualized version list.
#[derive(Debug, Clone)]
pub struct VersionItem {
    entry: VersionEntry,
}

pub struct VersionItemWidgets {
    title: gtk::Label,
    subtitle: gtk::Label,
}

impl RelmListItem for VersionItem {
    type Root = gtk::Box;
    type Widgets = VersionItemWidgets;

    fn setup(_list_item: &gtk::ListItem) -> (Self::Root, Self::Widgets) {
        relm4::view! {
            root = gtk::Box {
                set_orientation: gtk::Orientation::Vertical,
                set_spacing: 2,
                set_margin_all: 6,

                #[name = "title"]
                gtk::Label {
                    set_halign: gtk::Align::Start,
                },

                #[name = "subtitle"]
                gtk::Label {
                    set_halign: gtk::Align::Start,
                    add_css_class: "dim-label",
                    add_css_class: "caption",
                },
            }
        }

        (root, VersionItemWidgets { title, subtitle })
    }
    fn bind(&mut self, widgets: &mut Self::Widgets, _root: &mut Self::Root) {
        widgets.title.set_label(&self.entry.id);
        widgets.subtitle.set_label(self.entry.version_type.label());
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

/// Search and type filter shared with the list view's filter closure.
#[derive(Debug, Default)]
struct FilterState {
    query: String,
    type_filter: VersionFilter,
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
    SelectVersion(VersionEntry),
    RefreshVersions,
    CreateClicked,
    CancelClicked,
}

#[derive(Debug, Clone)]
pub enum CreateOutput {
    Cancel,
    Created(String, VersionEntry),
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
    selected_version: Option<VersionEntry>,

    filter_state: Rc<RefCell<FilterState>>,
    version_list: TypedListView<VersionItem, gtk::SingleSelection>,
    is_loading: bool,
}

impl CreatePage {
    fn effective_instance_name(&self) -> Option<&str> {
        let trimmed = self.instance_name.trim();
        if !trimmed.is_empty() {
            Some(trimmed)
        } else {
            self.selected_version.as_ref().map(|v| v.id.as_str())
        }
    }

    fn can_create(&self) -> bool {
        !self.is_loading
            && self.selected_version.is_some()
            && self
                .effective_instance_name()
                .is_some_and(|name| InstanceConfig::validate_instance_id(name).is_ok())
    }

    fn selected_version_label(&self) -> &str {
        self.selected_version
            .as_ref()
            .map_or("None", |v| v.id.as_str())
    }

    /// Highlight `selected_version` in the list if it is currently visible.
    fn sync_selection(&self) {
        let selection = &self.version_list.selection_model;
        let position = self.selected_version.as_ref().and_then(|selected| {
            (0..selection.n_items()).find(|&i| {
                selection
                    .item(i)
                    .and_then(|obj| obj.downcast::<BoxedAnyObject>().ok())
                    .is_some_and(|boxed| {
                        boxed
                            .try_borrow::<VersionItem>()
                            .is_ok_and(|item| item.entry.id == selected.id)
                    })
            })
        });
        selection.set_selected(position.unwrap_or(gtk::INVALID_LIST_POSITION));
    }

    /// Re-evaluate the list filter without recreating any row widgets.
    fn refilter(&self) {
        self.version_list.notify_filter_changed(0);
        self.sync_selection();
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
        gtk::Box {
            set_orientation: gtk::Orientation::Vertical,

            adw::Clamp {
                set_maximum_size: 720,
                set_margin_top: 24,
                set_margin_start: 12,
                set_margin_end: 12,

                gtk::Box {
                    set_orientation: gtk::Orientation::Vertical,
                    set_spacing: 12,
                     // Top action bar
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

                        adw::ActionRow {
                            set_title: "Game Version",
                            #[watch]
                            set_subtitle: model.selected_version_label(),
                        },

                    },

                    // Version browser toolbar
                    gtk::Box {
                        set_spacing: 8,
                        gtk::SearchEntry {
                            set_hexpand: true,
                            set_placeholder_text: Some("Search versions..."),
                            connect_search_changed[sender] => move |entry| {
                                sender.input(CreateInput::SearchChanged(entry.text().to_string()));
                            },
                        },

                        adw::ToggleGroup {
                            connect_active_name_notify[sender] => move |group| {
                                let filter = VersionFilter::from_toggle_name(group.active_name().as_deref());
                                sender.input(CreateInput::FilterChanged(filter));
                            },

                            add = adw::Toggle {
                                set_name: Some(VersionFilter::Release.as_str()),
                                set_label: Some(VersionFilter::Release.title()),
                            },
                            add = adw::Toggle {
                                set_name: Some(VersionFilter::Snapshot.as_str()),
                                set_label: Some(VersionFilter::Snapshot.title()),
                            },
                            add = adw::Toggle {
                                set_name: Some(VersionFilter::Old.as_str()),
                                set_label: Some(VersionFilter::Old.title()),
                            },

                            set_active_name: Some(VersionFilter::Release.as_str()),
                        },

                        gtk::Button {
                            set_icon_name: "view-refresh-symbolic",
                            set_tooltip_text: Some("Refresh version list"),
                            #[watch]
                            set_sensitive: !model.is_loading,
                            connect_clicked => CreateInput::RefreshVersions,
                        },
                    },
                },
            },

            // Virtualized version list: only visible rows are instantiated
            gtk::ScrolledWindow {
                set_vexpand: true,
                set_hscrollbar_policy: gtk::PolicyType::Never,
                set_margin_top: 12,
                set_margin_bottom: 24,

                #[name = "list_clamp"]
                adw::ClampScrollable {
                    set_maximum_size: 720,
                    set_margin_start: 12,
                    set_margin_end: 12,
                },

            }

        },

    }

    fn init(
        init: Self::Init,
        root: Self::Root,
        sender: ComponentSender<Self>,
    ) -> ComponentParts<Self> {
        let filter_state = Rc::new(RefCell::new(FilterState::default()));

        let mut version_list: TypedListView<VersionItem, gtk::SingleSelection> =
            TypedListView::new();
        version_list.selection_model.set_autoselect(false);
        version_list.selection_model.set_can_unselect(true);
        version_list.view.add_css_class("navigation-sidebar");

        version_list.add_filter({
            let state = Rc::clone(&filter_state);
            move |item| match state.try_borrow() {
                Ok(state) => matches_version_entry(&item.entry, &state.query, state.type_filter),
                Err(_) => true,
            }
        });

        // Read the selected version id synchronously so filtering can't race with it.
        version_list.selection_model.connect_selected_item_notify({
            let sender = sender.clone();
            move |selection| {
                let id = selection
                    .selected_item()
                    .and_then(|obj| obj.downcast::<BoxedAnyObject>().ok())
                    .and_then(|boxed| {
                        boxed
                            .try_borrow::<VersionItem>()
                            .ok()
                            .map(|item| item.entry.clone())
                    });

                // `None` means the selected row was filtered out; keep the previous choice.
                if let Some(entry) = id {
                    sender.input(CreateInput::SelectVersion(entry));
                }
            }
        });

        let mut model = Self {
            mgr: init.mgr,
            client: init.client,
            cache_dir: init.cache_dir,
            instance_name: String::new(),
            selected_version: None,
            filter_state,
            version_list,
            is_loading: false,
        };

        model.spawn_fetch_manifest(&sender, false);

        let widgets = view_output!();
        widgets.list_clamp.set_child(Some(&model.version_list.view));

        ComponentParts { model, widgets }
    }

    fn update(&mut self, msg: Self::Input, sender: ComponentSender<Self>, _root: &Self::Root) {
        match msg {
            CreateInput::NameChanged(name) => {
                self.instance_name = name;
            }
            CreateInput::SearchChanged(query) => {
                if let Ok(mut state) = self.filter_state.try_borrow_mut() {
                    state.query = query;
                }
                self.refilter();
            }
            CreateInput::FilterChanged(filter) => {
                if let Ok(mut state) = self.filter_state.try_borrow_mut() {
                    state.type_filter = filter;
                }
                self.refilter();
            }

            CreateInput::SelectVersion(entry) => {
                self.selected_version = Some(entry);
            }

            CreateInput::RefreshVersions => {
                self.spawn_fetch_manifest(&sender, true);
            }
            CreateInput::CancelClicked => {
                let _ = sender.output(CreateOutput::Cancel);
            }

            // Create version metadata without downloading resource files, file downloads are handled upstream.
            CreateInput::CreateClicked => {
                let Some(version_entry) = self.selected_version.clone() else {
                    let _ = sender.output(CreateOutput::ShowToast(
                        "Please select a Minecraft version".to_string(),
                    ));
                    return;
                };

                let Some(id) = self.effective_instance_name().map(str::to_string) else {
                    let _ = sender.output(CreateOutput::ShowToast(
                        "Please enter an instance name or select a version".to_string(),
                    ));
                    return;
                };

                let versions_dir = self.mgr.read().versions_dir.clone();
                match Instance::create(&id, &id, &version_entry.id, &versions_dir) {
                    Ok(_) => {
                        info!(
                            "Created instance '{}' with version '{}'",
                            id, version_entry.id
                        );
                        if let Err(err) = self.mgr.write().refresh() {
                            let msg = format!("Failed to refresh instances: {err}");
                            error!("{msg}");
                            let _ = sender.output(CreateOutput::ShowToast(msg));
                        } else {
                            self.selected_version = None;
                            self.instance_name.clear();
                            let _ = sender.output(CreateOutput::Created(id, version_entry));
                        }
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
                        // Auto-select latest release version
                        if self.selected_version.is_none() {
                            self.selected_version =
                                manifest.find_version(&manifest.latest.release).cloned();
                        }

                        self.version_list.clear();
                        self.version_list.extend_from_iter(
                            manifest
                                .versions
                                .into_iter()
                                .map(|entry| VersionItem { entry }),
                        );

                        self.sync_selection();
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

#[cfg(test)]
mod tests {
    use super::*;

    fn make_entry(id: &str, version_type: VersionType) -> VersionEntry {
        VersionEntry {
            id: id.to_string(),
            version_type,
            url: String::new(),
            time: String::new(),
            release_time: String::new(),
            sha1: String::new(),
            compliance_level: 1,
        }
    }

    #[test]
    fn test_version_filter_from_toggle_name() {
        assert_eq!(
            VersionFilter::from_toggle_name(Some("snapshot")),
            VersionFilter::Snapshot
        );
        assert_eq!(
            VersionFilter::from_toggle_name(Some("old")),
            VersionFilter::Old
        );
        assert_eq!(
            VersionFilter::from_toggle_name(Some("release")),
            VersionFilter::Release
        );
        assert_eq!(
            VersionFilter::from_toggle_name(None),
            VersionFilter::Release
        );
        assert_eq!(
            VersionFilter::from_toggle_name(Some("unknown")),
            VersionFilter::Release
        );
    }

    #[test]
    fn test_version_filter_as_str_and_title() {
        assert_eq!(VersionFilter::default(), VersionFilter::Release);

        assert_eq!(VersionFilter::Release.as_str(), "release");
        assert_eq!(VersionFilter::Release.title(), "Release");

        assert_eq!(VersionFilter::Snapshot.as_str(), "snapshot");
        assert_eq!(VersionFilter::Snapshot.title(), "Snapshot");

        assert_eq!(VersionFilter::Old.as_str(), "old");
        assert_eq!(VersionFilter::Old.title(), "Old");
    }

    #[test]
    fn test_version_filter_matches() {
        let release = VersionFilter::Release;
        assert!(release.matches(&VersionType::Release));
        assert!(!release.matches(&VersionType::Snapshot));
        assert!(!release.matches(&VersionType::OldBeta));
        assert!(!release.matches(&VersionType::OldAlpha));

        let snapshot = VersionFilter::Snapshot;
        assert!(!snapshot.matches(&VersionType::Release));
        assert!(snapshot.matches(&VersionType::Snapshot));
        assert!(!snapshot.matches(&VersionType::OldBeta));
        assert!(!snapshot.matches(&VersionType::OldAlpha));

        let old = VersionFilter::Old;
        assert!(!old.matches(&VersionType::Release));
        assert!(!old.matches(&VersionType::Snapshot));
        assert!(old.matches(&VersionType::OldBeta));
        assert!(old.matches(&VersionType::OldAlpha));
    }

    #[test]
    fn test_matches_version_entry() {
        let entry_release = make_entry("1.21.1", VersionType::Release);
        let entry_snapshot = make_entry("24w33a", VersionType::Snapshot);
        let entry_old = make_entry("b1.7.3", VersionType::OldBeta);

        // Matching with empty or whitespace search query
        assert!(matches_version_entry(
            &entry_release,
            "",
            VersionFilter::Release
        ));
        assert!(matches_version_entry(
            &entry_release,
            "   ",
            VersionFilter::Release
        ));
        assert!(!matches_version_entry(
            &entry_release,
            "",
            VersionFilter::Snapshot
        ));

        // Substring and case insensitivity
        assert!(matches_version_entry(
            &entry_release,
            "21",
            VersionFilter::Release
        ));
        assert!(matches_version_entry(
            &entry_release,
            "1.21.1",
            VersionFilter::Release
        ));
        assert!(!matches_version_entry(
            &entry_release,
            "1.20",
            VersionFilter::Release
        ));

        assert!(matches_version_entry(
            &entry_snapshot,
            "24w",
            VersionFilter::Snapshot
        ));
        assert!(matches_version_entry(
            &entry_snapshot,
            "24W33A",
            VersionFilter::Snapshot
        ));
        assert!(!matches_version_entry(
            &entry_snapshot,
            "24w",
            VersionFilter::Release
        ));

        assert!(matches_version_entry(
            &entry_old,
            "b1.7",
            VersionFilter::Old
        ));
        assert!(matches_version_entry(
            &entry_old,
            "B1.7",
            VersionFilter::Old
        ));
        assert!(!matches_version_entry(
            &entry_old,
            "b1.7",
            VersionFilter::Snapshot
        ));
    }
}
