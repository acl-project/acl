// SPDX-FileCopyrightText: 2026 The Adwaita Craft Launcher Authors
// SPDX-License-Identifier: GPL-3.0-or-later

use std::cell::RefCell;
use std::rc::Rc;
use std::sync::Arc;

use acl_minecraft::FabricLoaderEntry;
use gtk::{glib::BoxedAnyObject, prelude::*};

use acl_core::{LauncherConfig, ModLoader};
use relm4::tokio::task::AbortHandle;
use relm4::{
    Component, ComponentParts, ComponentSender, RelmWidgetExt, SharedState, adw,
    typed_view::list::{RelmListItem, TypedListView},
};
use tracing::error;

use crate::i18n::LocalizeError;
use crate::t;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LoaderStability {
    Stable, // Fabric stable or Forge Recommended
    Beta,   // Beta version, latest snapshot, or Latest
}

impl LoaderStability {
    fn label(&self) -> String {
        match self {
            Self::Beta => t!("loader-stability-beta", "Beta"),
            Self::Stable => t!("loader-stability-stable", "Stable"),
        }
    }
}

#[derive(Debug, Clone)]
pub struct LoaderVersionItem {
    pub loader: ModLoader,
    pub version: String,
    pub stability: LoaderStability,
}

pub struct LoaderVersionItemWidgets {
    title: gtk::Label,
    subtitle: gtk::Label,
}

impl RelmListItem for LoaderVersionItem {
    type Root = gtk::Box;
    type Widgets = LoaderVersionItemWidgets;

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

        (root, LoaderVersionItemWidgets { title, subtitle })
    }
    fn bind(&mut self, widgets: &mut Self::Widgets, _root: &mut Self::Root) {
        widgets
            .title
            .set_label(&format!("{} {}", self.loader.label(), self.version));
        widgets.subtitle.set_label(&self.stability.label());
    }
}

pub fn matches_version_entry(entry: &LoaderVersionItem, search_query: &str) -> bool {
    let query = search_query.trim();
    if query.is_empty() {
        return true;
    }

    entry
        .version
        .to_ascii_lowercase()
        .contains(&query.to_ascii_lowercase())
}

#[derive(Debug)]
pub enum LoaderSelectInput {
    // Trigger loading: specify the loader and MC version
    Load {
        loader: ModLoader,
        game_version: String,
    },
    SearchChanged(String),
    SelectVersion(LoaderVersionItem),
    ClearSelection,
    BackClicked,
    RefreshUi,
    RefreshVersions,
}

#[derive(Debug, Clone)]
pub enum LoaderSelectOutput {
    Back,
    // Return after selection (version being None means cancel this Loader)
    Selected {
        loader: ModLoader,
        version: Option<String>,
    },
    ShowToast(String),
}

#[derive(Debug, Clone)]
pub enum LoaderSelectCmdOutput {
    Finish(Result<Vec<LoaderVersionItem>, String>),
}

pub struct LoaderSelectPage {
    config: Arc<SharedState<LauncherConfig>>,
    client: reqwest::Client,
    cache_dir: std::path::PathBuf,
    current_loader: ModLoader,
    current_game_version: String,
    version_list: TypedListView<LoaderVersionItem, gtk::SingleSelection>,
    is_loading: bool,
    loading_abort: Option<AbortHandle>,
    query: Rc<RefCell<String>>,
}

impl LoaderSelectPage {
    fn spawn_fetch_loader(&mut self, sender: &ComponentSender<Self>, force_refresh: bool) {
        self.is_loading = true;
        let loader = self.current_loader;
        let game_version = self.current_game_version.clone();
        let client = self.client.clone();
        let cache_dir = self.cache_dir.clone();
        let download_source = self.config.read().download_source;
        let out = sender.command_sender().clone();

        let join_handle = relm4::spawn(async move {
            let result: Result<Vec<LoaderVersionItem>, String> = match loader {
                ModLoader::Fabric => FabricLoaderEntry::fetch_list_with_cache(
                    &client,
                    &game_version,
                    &cache_dir,
                    download_source,
                    force_refresh,
                )
                .await
                .map(|entries| {
                    entries
                        .into_iter()
                        .map(|entry| LoaderVersionItem {
                            loader: ModLoader::Fabric,
                            version: entry.loader.version,
                            stability: if entry.loader.stable {
                                LoaderStability::Stable
                            } else {
                                LoaderStability::Beta
                            },
                        })
                        .collect()
                })
                .map_err(|err| err.to_i18n()),
                _ => Err(t!(
                    "instances-create-error-unsupported-loader",
                    "Not implemented or unsupported loader"
                )),
            };

            let _ = out.send(LoaderSelectCmdOutput::Finish(result));
        });

        self.loading_abort = Some(join_handle.abort_handle());
    }
}
pub struct LoaderSelectInit {
    pub config: Arc<SharedState<LauncherConfig>>,
    pub client: reqwest::Client,
    pub cache_dir: std::path::PathBuf,
}

#[relm4::component(pub)]
impl Component for LoaderSelectPage {
    type Init = LoaderSelectInit;
    type Input = LoaderSelectInput;
    type Output = LoaderSelectOutput;
    type CommandOutput = LoaderSelectCmdOutput;

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
                            connect_clicked => LoaderSelectInput::BackClicked,

                            adw::ButtonContent {
                                set_icon_name: "go-previous-symbolic",
                                #[watch]
                                set_label: &t!("instances-create-button-back","Back"),
                            },
                        },

                        gtk::Button {
                            set_hexpand: true,
                            set_halign: gtk::Align::End,

                            connect_clicked => LoaderSelectInput::ClearSelection,
                            adw::ButtonContent {
                                set_icon_name: "edit-clear-symbolic",
                                #[watch]
                                set_label: &t!("instances-create-button-clear", "Clear"),
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
                                sender.input(LoaderSelectInput::SearchChanged(entry.text().to_string()));
                            },
                        },


                        gtk::Button {
                            set_icon_name: "view-refresh-symbolic",
                            #[watch]
                            set_tooltip_text: Some(&t!("instances-create-refresh-version","Refresh version list")),
                            #[watch]
                            set_sensitive: !model.is_loading,
                            connect_clicked => LoaderSelectInput::RefreshVersions,
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
        init.config
            .subscribe(sender.input_sender(), |_| LoaderSelectInput::RefreshUi);
        let query = Rc::new(RefCell::new(String::new()));
        let mut version_list: TypedListView<LoaderVersionItem, gtk::SingleSelection> =
            TypedListView::new();
        version_list.selection_model.set_autoselect(false);
        version_list.selection_model.set_can_unselect(true);

        version_list.add_filter({
            let query = Rc::clone(&query);

            move |item| match query.try_borrow() {
                Ok(q) => matches_version_entry(item, &q),
                _ => true,
            }
        });

        version_list.selection_model.connect_selected_item_notify({
            let sender = sender.clone();
            move |selection| {
                let selected = selection
                    .selected_item()
                    .and_then(|obj| obj.downcast::<BoxedAnyObject>().ok())
                    .and_then(|boxed| {
                        boxed
                            .try_borrow::<LoaderVersionItem>()
                            .ok()
                            .map(|item| item.clone())
                    });

                if let Some(entry) = selected {
                    sender.input(LoaderSelectInput::SelectVersion(entry));
                }
            }
        });
        version_list.view.add_css_class("navigation-sidebar");

        let model = LoaderSelectPage {
            config: init.config,
            client: init.client,
            cache_dir: init.cache_dir,
            current_loader: ModLoader::Vanilla,
            current_game_version: String::new(),
            version_list,
            is_loading: false,
            loading_abort: None,
            query,
        };

        let widgets = view_output!();

        widgets.list_clamp.set_child(Some(&model.version_list.view));

        ComponentParts { model, widgets }
    }

    fn update(&mut self, msg: Self::Input, sender: ComponentSender<Self>, _root: &Self::Root) {
        match msg {
            LoaderSelectInput::BackClicked => {
                let _ = sender.output(LoaderSelectOutput::Back);
            }
            LoaderSelectInput::ClearSelection => {
                let _ = sender.output(LoaderSelectOutput::Selected {
                    loader: ModLoader::Vanilla,
                    version: None,
                });
            }
            LoaderSelectInput::RefreshUi => {}
            LoaderSelectInput::RefreshVersions => {
                if let Some(handle) = self.loading_abort.take() {
                    handle.abort();
                }
                self.spawn_fetch_loader(&sender, true);
            }
            LoaderSelectInput::SearchChanged(q) => {
                if let Ok(mut query) = self.query.try_borrow_mut() {
                    *query = q;
                }
                self.version_list.notify_filter_changed(0);
            }
            LoaderSelectInput::SelectVersion(item) => {
                let _ = sender.output(LoaderSelectOutput::Selected {
                    loader: item.loader,
                    version: Some(item.version),
                });
            }
            LoaderSelectInput::Load {
                loader,
                game_version,
            } => {
                self.current_loader = loader;
                self.current_game_version = game_version;
                // Cancel ongoing tasks to prevent contamination between different versions and different mod loaders.
                if let Some(handle) = self.loading_abort.take() {
                    handle.abort();
                }
                self.version_list.clear();
                self.spawn_fetch_loader(&sender, false);
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
            LoaderSelectCmdOutput::Finish(result) => {
                self.is_loading = false;
                match result {
                    Ok(items) => {
                        self.version_list.clear();
                        self.version_list.extend_from_iter(items);
                    }
                    Err(err) => {
                        error!("Failed to fetch loader entries: {err}");
                        let _ = sender.output(LoaderSelectOutput::ShowToast(err));
                    }
                }
            }
        }
    }
}
