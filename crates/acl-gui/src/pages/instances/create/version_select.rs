// SPDX-FileCopyrightText: 2026 The Adwaita Craft Launcher Authors
// SPDX-License-Identifier: GPL-3.0-or-later

use std::{cell::RefCell, path::PathBuf, rc::Rc, sync::Arc};

use acl_core::LauncherConfig;
use acl_minecraft::{VERSION_MANIFEST_URL, VersionEntry, VersionManifest, VersionType};
use gtk::{glib::BoxedAnyObject, prelude::*};

use relm4::{
    Component, ComponentParts, ComponentSender, RelmWidgetExt, SharedState, adw,
    typed_view::list::{RelmListItem, TypedListView},
};
use tracing::error;

use crate::t;

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

    pub fn title(self) -> String {
        match self {
            VersionFilter::Release => t!("version-filter-release", "Release"),
            VersionFilter::Snapshot => t!("version-filter-snapshot", "Snapshot"),
            VersionFilter::Old => t!("version-filter-old", "Old"),
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

pub struct VersionSelectPage {
    config: Arc<SharedState<LauncherConfig>>,
    client: reqwest::Client,
    cache_dir: PathBuf,
    selected_version: Option<VersionEntry>,
    filter_state: Rc<RefCell<FilterState>>,
    version_list: TypedListView<VersionItem, gtk::SingleSelection>,
    is_loading: bool,
}

pub struct VersionSelectInit {
    pub config: Arc<SharedState<LauncherConfig>>,
    pub client: reqwest::Client,
    pub cache_dir: PathBuf,
}

#[derive(Debug)]
pub enum VersionSelectInput {
    SearchChanged(String),
    FilterChanged(VersionFilter),
    SelectVersion(VersionEntry),
    RefreshVersions,
    NextClicked,
    CancelClicked,
    Refresh,
}

#[derive(Debug, Clone)]
pub enum VersionSelectOutput {
    Cancel,                 // User clicked Back to exit creation
    Selected(VersionEntry), // User selected and clicked Next
    ShowToast(String),
}

#[derive(Debug)]
pub enum VersionSelectCmdOutput {
    ManifestLoaded(Result<VersionManifest, String>),
}

impl VersionSelectPage {
    fn spawn_fetch_manifest(&mut self, sender: &ComponentSender<Self>, force_refresh: bool) {
        self.is_loading = true;
        let client = self.client.clone();
        let cache_dir = self.cache_dir.clone();
        let download_source = self.config.read().download_source;
        let url = download_source.rewrite_url(VERSION_MANIFEST_URL);
        sender.oneshot_command(async move {
            let result = if force_refresh {
                match VersionManifest::fetch_from_url(&client, &url).await {
                    Ok(manifest) => {
                        manifest.cache_from_url(&cache_dir, &url);
                        Ok(manifest)
                    }

                    Err(err) => Err(err.to_string()),
                }
            } else {
                VersionManifest::fetch_with_cache(&client, &cache_dir, download_source)
                    .await
                    .map_err(|err| err.to_string())
            };

            VersionSelectCmdOutput::ManifestLoaded(result)
        });
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
}

#[relm4::component(pub)]
impl Component for VersionSelectPage {
    type Init = VersionSelectInit;
    type Output = VersionSelectOutput;
    type Input = VersionSelectInput;
    type CommandOutput = VersionSelectCmdOutput;

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
                            connect_clicked => VersionSelectInput::CancelClicked,

                            adw::ButtonContent {
                                set_icon_name: "go-previous-symbolic",
                                #[watch]
                                set_label: &t!("instances-create-button-back","Back"),
                            },
                        },

                        gtk::Button {
                            set_hexpand: true,
                            set_halign: gtk::Align::End,
                            add_css_class: "suggested-action",
                            #[watch]
                            set_sensitive: model.selected_version.is_some() && !model.is_loading,
                            connect_clicked => VersionSelectInput::NextClicked,
                            adw::ButtonContent {
                                set_icon_name: "go-next-symbolic",
                                #[watch]
                                set_label: &t!("instances-create-button-next", "Next"),
                            },
                        },


                    },

                    // Search bar and Filter ToggleGroup
                    gtk::Box {
                        set_spacing: 8,
                        gtk::SearchEntry {
                            set_hexpand: true,
                            #[watch]
                            set_placeholder_text: Some(&t!("instances-create-search-versions","Search versions...")),
                            connect_search_changed[sender] => move |entry| {
                                sender.input(VersionSelectInput::SearchChanged(entry.text().to_string()));
                            },
                        },

                        adw::ToggleGroup {
                            connect_active_name_notify[sender] => move |group| {
                                let filter = VersionFilter::from_toggle_name(group.active_name().as_deref());
                                sender.input(VersionSelectInput::FilterChanged(filter));
                            },

                            #[wrap(Clone::clone)]
                            add = &adw::Toggle {
                                set_name: Some(VersionFilter::Release.as_str()),
                                #[watch]
                                set_label: Some(&VersionFilter::Release.title()),
                            },
                            #[wrap(Clone::clone)]
                            add = &adw::Toggle {
                                set_name: Some(VersionFilter::Snapshot.as_str()),
                                #[watch]
                                set_label: Some(&VersionFilter::Snapshot.title()),
                            },
                            #[wrap(Clone::clone)]
                            add = &adw::Toggle {
                                set_name: Some(VersionFilter::Old.as_str()),
                                #[watch]
                                set_label: Some(&VersionFilter::Old.title()),
                            },

                            set_active_name: Some(VersionFilter::Release.as_str()),
                        },

                        gtk::Button {
                            set_icon_name: "view-refresh-symbolic",
                            #[watch]
                            set_tooltip_text: Some(&t!("instances-create-refresh-version","Refresh version list")),
                            #[watch]
                            set_sensitive: !model.is_loading,
                            connect_clicked => VersionSelectInput::RefreshVersions,
                        },
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
                    sender.input(VersionSelectInput::SelectVersion(entry));
                }
            }
        });

        init.config
            .subscribe(sender.input_sender(), |_| VersionSelectInput::Refresh);

        let mut model = VersionSelectPage {
            config: init.config,
            client: init.client,
            cache_dir: init.cache_dir,
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
            VersionSelectInput::SearchChanged(query) => {
                if let Ok(mut state) = self.filter_state.try_borrow_mut() {
                    state.query = query;
                }
                self.refilter();
            }
            VersionSelectInput::FilterChanged(filter) => {
                if let Ok(mut state) = self.filter_state.try_borrow_mut() {
                    state.type_filter = filter;
                }
                self.refilter();
            }

            VersionSelectInput::SelectVersion(entry) => {
                self.selected_version = Some(entry);
            }

            VersionSelectInput::RefreshVersions => {
                self.spawn_fetch_manifest(&sender, true);
            }
            VersionSelectInput::CancelClicked => {
                let _ = sender.output(VersionSelectOutput::Cancel);
            }
            VersionSelectInput::NextClicked => {
                if let Some(entry) = &self.selected_version {
                    let _ = sender.output(VersionSelectOutput::Selected(entry.clone()));
                }
            }
            VersionSelectInput::Refresh => todo!(),
        }
    }

    fn update_cmd(
        &mut self,
        msg: Self::CommandOutput,
        sender: ComponentSender<Self>,
        _root: &Self::Root,
    ) {
        match msg {
            VersionSelectCmdOutput::ManifestLoaded(result) => {
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
                        error!("Failed to load version manifest: {err}");
                        let msg = t!(
                            "instances-create-error-load-version-manifest",
                            "Failed to load version manifest: {err}",
                            err = err
                        );
                        let _ = sender.output(VersionSelectOutput::ShowToast(msg));
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
