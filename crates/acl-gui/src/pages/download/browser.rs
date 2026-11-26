// SPDX-FileCopyrightText: 2026 The Adwaita Craft Launcher Authors
// SPDX-License-Identifier: GPL-3.0-or-later

use std::{path::PathBuf, sync::Arc};

use acl_core::LauncherConfig;
use acl_minecraft::{
    AssetFilterState, AssetItemData, ProjectHost, ProjectType, SearchIndex, curseforge, modrinth,
};
use gtk::{glib::BoxedAnyObject, pango, prelude::*};

use relm4::{
    Component, ComponentParts, ComponentSender, RelmWidgetExt, SharedState, adw,
    typed_view::list::{RelmListItem, TypedListView},
};
use tracing::error;

use crate::{i18n::LocalizeError, pages::LoadState, t};

const PAGE_LIMIT: u32 = 20;
const CURSEFORGE_MAX_RESULTS: u32 = 10_000;
#[derive(Debug, Clone)]
pub struct AssetRowItem {
    data: AssetItemData,
}

pub struct AssetRowWidgets {
    icon: gtk::Image,
    title: gtk::Label,
    author: gtk::Label,
    description: gtk::Label,
    categories: gtk::Label,
    downloads: gtk::Label,
}

fn format_downloads(count: u64) -> String {
    if count >= 1_000_000 {
        format!("{:.1}M", count as f64 / 1_000_000.0)
    } else if count >= 1_000 {
        format!("{:.1}K", count as f64 / 1_000.0)
    } else {
        count.to_string()
    }
}

impl RelmListItem for AssetRowItem {
    type Root = gtk::Box;
    type Widgets = AssetRowWidgets;

    fn setup(_list_item: &gtk::ListItem) -> (Self::Root, Self::Widgets) {
        relm4::view! {
            root = gtk::Box {
                set_orientation: gtk::Orientation::Horizontal,
                set_spacing: 12,
                set_margin_all: 10,

                // Left: project icon (48x48)
                #[name = "icon"]
                gtk::Image {
                    set_icon_name: Some("application-x-addon-symbolic"),
                    set_pixel_size: 48,
                    set_valign: gtk::Align::Center,
                },

                // Middle: Title + Author /Introduction /Category Tags
                gtk::Box {
                    set_orientation: gtk::Orientation::Vertical,
                    set_spacing: 4,
                    set_hexpand: true,
                    set_valign: gtk::Align::Center,

                    // First line: title + author
                    gtk::Box {
                        set_orientation: gtk::Orientation::Horizontal,
                        set_spacing: 8,

                        #[name = "title"]
                        gtk::Label {
                            set_halign: gtk::Align::Start,
                            add_css_class: "heading",
                            set_ellipsize: pango::EllipsizeMode::End,
                        },

                        #[name = "author"]
                        gtk::Label {
                            set_halign: gtk::Align::Start,
                            set_valign: gtk::Align::Baseline,
                            add_css_class: "dim-label",
                            add_css_class: "caption",
                            set_ellipsize: pango::EllipsizeMode::End,
                        },

                    },

                    // Second line: Single line introduction (beyond automatic ellipses)
                    #[name = "description"]
                    gtk::Label {
                        set_halign: gtk::Align::Start,
                        set_ellipsize: pango::EllipsizeMode::End,
                        set_lines: 1,
                        add_css_class: "dim-label",
                    },

                    // The third line: classification and loader tags (for example: fabric · optimization)
                    #[name = "categories"]
                    gtk::Label {
                        set_halign: gtk::Align::Start,
                        set_ellipsize: pango::EllipsizeMode::End,
                        add_css_class: "caption",
                        add_css_class: "accent",
                    },

                },

                // Right: download volume + right arrow
                gtk::Box {
                    set_orientation: gtk::Orientation::Horizontal,
                    set_spacing: 6,
                    set_valign: gtk::Align::Center,

                    gtk::Image {
                        set_icon_name: Some("folder-download-symbolic"),
                        add_css_class: "dim-label",
                    },

                    #[name = "downloads"]
                    gtk::Label {
                        add_css_class: "dim-label",
                        add_css_class: "caption",
                        add_css_class: "numeric",
                    },

                    gtk::Image {
                        set_icon_name: Some("go-next-symbolic"),
                        add_css_class: "dim-label",
                        set_margin_start: 4,
                    },

                },

            }
        }

        (
            root,
            AssetRowWidgets {
                icon,
                title,
                author,
                description,
                categories,
                downloads,
            },
        )
    }

    fn bind(&mut self, widgets: &mut Self::Widgets, _root: &mut Self::Root) {
        widgets.title.set_label(&self.data.title);
        // Hide if there is no author (does this actually happen?)
        if self.data.author.is_empty() {
            widgets.author.set_visible(false);
        } else {
            widgets.author.set_visible(true);
            widgets
                .author
                .set_label(&format!("by {}", self.data.author));
        }

        widgets.description.set_label(&self.data.description);

        if self.data.display_categories.is_empty() {
            widgets.categories.set_visible(false);
        } else {
            widgets.categories.set_visible(true);
            widgets
                .categories
                .set_label(&self.data.display_categories.join(" · "));
        }

        widgets
            .downloads
            .set_label(&format_downloads(self.data.downloads));
        // placeholder icon
        widgets
            .icon
            .set_icon_name(Some("application-x-addon-symbolic"));
    }
}

pub struct BrowserPage {
    config: Arc<SharedState<LauncherConfig>>,
    client: reqwest::Client,
    cache_dir: PathBuf,
    filter: AssetFilterState,
    total_hits: u32,
    list_view: TypedListView<AssetRowItem, gtk::SingleSelection>,
    state: LoadState,
    request_id: u64,
}

pub struct BrowserInit {
    pub project_type: ProjectType,
    pub config: Arc<SharedState<LauncherConfig>>,
    pub client: reqwest::Client,
    pub cache_dir: PathBuf,
}

#[derive(Debug)]
pub enum BrowserInput {
    HostChanged(ProjectHost),
    QueryChanged(String),
    LoaderChanged(Option<String>),
    SortChanged(SearchIndex),
    PrevPage,
    NextPage,
    RefreshForce,
    SelectItem(AssetItemData),
    EnsureLoaded,
    Refresh,
}

#[derive(Debug, Clone)]
pub enum BrowserOutput {
    Selected(AssetItemData),
    ShowToast(String),
}

#[derive(Debug)]
pub enum BrowserCmdOutput {
    SearchLoaded {
        request_id: u64,
        result: Result<(Vec<AssetItemData>, u32), String>,
    },
}

impl BrowserPage {
    fn is_loading(&self) -> bool {
        self.state.is_loading()
    }

    fn content_page(&self) -> &'static str {
        let is_empty = self.list_view.is_empty();
        self.state.content_page(is_empty)
    }

    fn error_message(&self) -> &str {
        self.state.error_message()
    }

    fn clamp_total_hits(host: ProjectHost, total_count: u32) -> u32 {
        match host {
            ProjectHost::Modrinth => total_count,
            ProjectHost::CurseForge => total_count.min(CURSEFORGE_MAX_RESULTS),
        }
    }

    fn spawn_search(&mut self, sender: &ComponentSender<Self>, offset: u32, force_refresh: bool) {
        self.request_id = self.request_id.wrapping_add(1);
        let request_id = self.request_id;

        self.state = LoadState::Loading;
        // Since the bottom navigation bar is hidden during loading and error states,
        // incorrect page numbers won't be displayed, so there's no need to wait until completion to update.
        self.filter.offset = offset;
        let client = self.client.clone();
        let cache_dir = self.cache_dir.clone();
        let download_source = self.config.read().download_source;
        let filter = self.filter.clone();

        sender.oneshot_command(async move {
            let result = match filter.host {
                ProjectHost::Modrinth => {
                    let params = modrinth::SearchParams::from(&filter);
                    modrinth::SearchResponse::fetch_with_cache(
                        &client,
                        &params,
                        &cache_dir,
                        download_source,
                        force_refresh,
                    )
                    .await
                    .map(|resp| {
                        let items = resp.hits.into_iter().map(AssetItemData::from).collect();
                        (
                            items,
                            Self::clamp_total_hits(ProjectHost::Modrinth, resp.total_hits),
                        )
                    })
                    .map_err(|e| e.to_i18n())
                }
                ProjectHost::CurseForge => {
                    let params = curseforge::SearchParams::from(&filter);
                    curseforge::SearchResponse::fetch_with_cache(
                        &client,
                        &params,
                        &cache_dir,
                        force_refresh,
                    )
                    .await
                    .map(|resp| {
                        let items = resp.data.into_iter().map(AssetItemData::from).collect();
                        let total_hits = Self::clamp_total_hits(
                            ProjectHost::CurseForge,
                            resp.pagination.total_count,
                        );
                        (items, total_hits)
                    })
                    .map_err(|e| e.to_i18n())
                }
            };

            BrowserCmdOutput::SearchLoaded { request_id, result }
        });
    }

    fn current_page_display(&self) -> String {
        if self.total_hits == 0 {
            return "0 / 0".to_string();
        }
        let current = (self.filter.offset / PAGE_LIMIT).saturating_add(1);
        let total = self.total_hits.div_ceil(PAGE_LIMIT);
        format!("{current} / {total}")
    }

    fn can_go_prev(&self) -> bool {
        !self.is_loading() && self.filter.offset > 0
    }

    fn can_go_next(&self) -> bool {
        !self.is_loading() && self.filter.offset.saturating_add(self.filter.limit) < self.total_hits
    }
}

#[relm4::component(pub)]
impl Component for BrowserPage {
    type Init = BrowserInit;
    type Input = BrowserInput;
    type Output = BrowserOutput;
    type CommandOutput = BrowserCmdOutput;

    view! {
        #[root]
        gtk::Box {
            set_orientation: gtk::Orientation::Vertical,

            // Top search and filter bar
            adw::Clamp {
                set_maximum_size: 720,
                set_margin_top: 24,
                set_margin_start: 12,
                set_margin_end: 12,

                gtk::Box {
                    set_orientation: gtk::Orientation::Vertical,
                    set_spacing: 12,
                    // Host switcher + Search entry + Refresh button

                    gtk::Box {
                        set_orientation: gtk::Orientation::Horizontal,
                        set_spacing: 8,

                        gtk::SearchEntry {
                            set_hexpand: true,
                            set_search_delay: 800,
                            #[watch]
                            set_placeholder_text: Some(&t!("download-browser-search-projects","Search projects...")),
                            connect_search_changed[sender] => move |entry| {
                                sender.input(BrowserInput::QueryChanged(entry.text().to_string()));
                            },
                            connect_activate[sender] => move |entry| {
                                sender.input(BrowserInput::QueryChanged(entry.text().to_string()));
                            },
                        },

                        adw::ToggleGroup {
                            connect_active_name_notify[sender] => move |group| {
                                let host = match group.active_name().as_deref() {
                                    Some("curseforge") => ProjectHost::CurseForge,
                                    _ => ProjectHost::Modrinth,
                                };
                                sender.input(BrowserInput::HostChanged(host));
                            },

                            #[wrap(Clone::clone)]
                            add = &adw::Toggle {
                                set_name: Some("modrinth"),
                                set_label: Some("Modrinth"),
                            },

                            #[wrap(Clone::clone)]
                            add = &adw::Toggle {
                                set_name: Some("curseforge"),
                                set_label: Some("CurseForge"),
                            },

                            set_active_name: Some("modrinth"),

                        },

                        gtk::Button {
                            set_icon_name: "view-refresh-symbolic",
                            #[watch]
                            set_tooltip_text: Some(&t!("download-browser-refresh-list","Refresh list")),
                            #[watch]
                            set_sensitive: !model.is_loading(),
                            connect_clicked => BrowserInput::RefreshForce,
                        },

                    },

                    // Sort selector + Loader selector (hidden for ResourcePack)
                    gtk::Box {
                        set_orientation: gtk::Orientation::Horizontal,
                        set_spacing: 8,
                        gtk::DropDown {
                            #[watch]
                            #[block_signal(sort_handler)]
                            set_model: Some(&gtk::StringList::new(&[
                                &t!("download-browser-sort-relevance", "Relevance"),
                                &t!("download-browser-sort-downloads", "Downloads"),
                                &t!("download-browser-sort-follows", "Follows"),
                                &t!("download-browser-sort-newest", "Newest"),
                                &t!("download-browser-sort-updated", "Updated"),
                            ])),

                            #[watch]
                            #[block_signal(sort_handler)]
                            set_selected: match model.filter.sort_by {
                                SearchIndex::Relevance => 0,
                                SearchIndex::Downloads => 1,
                                SearchIndex::Follows => 2,
                                SearchIndex::Newest => 3,
                                SearchIndex::Updated => 4,
                            },

                            connect_selected_notify[sender] => move |dd| {
                                let sort = match dd.selected() {
                                    1 => SearchIndex::Downloads,
                                    2 => SearchIndex::Follows,
                                    3 => SearchIndex::Newest,
                                    4 => SearchIndex::Updated,
                                    _ => SearchIndex::Relevance,
                                };
                                sender.input(BrowserInput::SortChanged(sort));
                            } @sort_handler,
                        },

                        gtk::DropDown {
                            #[watch]
                            #[block_signal(loader_handler)]
                            set_model: Some(&gtk::StringList::new(&[
                                &t!("download-browser-loader-all", "All Loaders"),
                                "Fabric",
                                "Forge",
                                "NeoForge",
                                "Quilt",
                            ])),

                            #[watch]
                            #[block_signal(loader_handler)]
                            set_selected: match model.filter.loader.as_deref() {
                                Some("fabric") => 1,
                                Some("forge") => 2,
                                Some("neoforge") => 3,
                                Some("quilt") => 4,
                                _ => 0,
                            },

                            #[watch]
                            set_visible: matches!(model.filter.project_type, ProjectType::Mod | ProjectType::Modpack),
                            connect_selected_notify[sender] => move |dd| {
                                let loader = match dd.selected() {
                                    1 => Some("fabric".to_string()),
                                    2 => Some("forge".to_string()),
                                    3 => Some("neoforge".to_string()),
                                    4 => Some("quilt".to_string()),
                                    _ => None,
                                };
                                sender.input(BrowserInput::LoaderChanged(loader));
                            } @loader_handler,
                        },

                    },

                },

            },

            // Middle: Content area (loading / empty / error / virtualized project list)
            gtk::Stack {
                set_vexpand: true,
                set_transition_type: gtk::StackTransitionType::Crossfade,
                set_transition_duration: 150,

                add_named[Some("loading")] = &gtk::Box {
                    set_orientation: gtk::Orientation::Vertical,
                    set_halign: gtk::Align::Center,
                    set_valign: gtk::Align::Center,
                    set_spacing: 12,

                    adw::Spinner {
                        set_width_request: 32,
                        set_height_request: 32,
                    },

                    gtk::Label {
                        add_css_class: "dim-label",
                        #[watch]
                        set_label: &t!("download-browser-loading", "Searching projects..."),
                    },
                },

                add_named[Some("empty")] = &adw::StatusPage {
                    add_css_class: "compact",
                    set_icon_name: Some("edit-find-symbolic"),
                    #[watch]
                    set_title: &t!("download-browser-empty-title", "No Results Found"),
                    #[watch]
                    set_description: Some(&t!(
                        "download-browser-empty-description",
                        "Try a different search term or filter"
                    )),
                },

                add_named[Some("error")] = &adw::StatusPage {
                    add_css_class: "compact",
                    set_icon_name: Some("network-error-symbolic"),
                    #[watch]
                    set_title: &t!("download-browser-error-title", "Failed to Load Projects"),
                    #[watch]
                    set_description: Some(model.error_message()),

                    #[wrap(Some)]
                    set_child = &gtk::Button {
                        set_halign: gtk::Align::Center,
                        add_css_class: "pill",
                        add_css_class: "suggested-action",
                        #[watch]
                        set_label: &t!("download-button-retry", "Retry"),
                        connect_clicked => BrowserInput::RefreshForce,
                    },
                },

                add_named[Some("list")] = &gtk::ScrolledWindow {
                    set_vexpand: true,
                    set_hscrollbar_policy: gtk::PolicyType::Never,
                    set_margin_top: 12,
                    set_margin_bottom: 12,

                    #[name = "list_clamp"]
                    adw::ClampScrollable {
                        set_maximum_size: 720,
                        set_margin_start: 12,
                        set_margin_end: 12,
                    },
                },

                #[watch]
                set_visible_child_name: model.content_page(),
            },
            // Bottom: Pagination bar
            gtk::Box {
                set_orientation: gtk::Orientation::Horizontal,
                set_halign: gtk::Align::Center,
                set_spacing: 12,
                set_margin_bottom: 16,
                #[watch]
                set_visible: model.content_page() == "list",

                gtk::Button {
                    set_icon_name: "go-previous-symbolic",
                    #[watch]
                    set_sensitive: model.can_go_prev(),
                    connect_clicked => BrowserInput::PrevPage,
                },

                gtk::Label {
                    add_css_class: "numeric",
                    #[watch]
                    set_label: &model.current_page_display(),
                },

                gtk::Button {
                    set_icon_name: "go-next-symbolic",
                    #[watch]
                    set_sensitive: model.can_go_next(),
                    connect_clicked => BrowserInput::NextPage,
                },
            },
        }
    }

    fn init(
        init: Self::Init,
        root: Self::Root,
        sender: ComponentSender<Self>,
    ) -> ComponentParts<Self> {
        init.config
            .subscribe(sender.input_sender(), |_| BrowserInput::Refresh);

        let list_view: TypedListView<AssetRowItem, gtk::SingleSelection> = TypedListView::new();
        list_view.selection_model.set_autoselect(false);
        list_view.selection_model.set_can_unselect(true);
        list_view.view.add_css_class("navigation-sidebar");
        list_view.view.set_single_click_activate(true);

        list_view.view.connect_activate({
            let sender = sender.clone();
            move |view, pos| {
                let item = view
                    .model()
                    .and_then(|m| m.item(pos))
                    .and_then(|obj| obj.downcast::<BoxedAnyObject>().ok())
                    .and_then(|boxed| {
                        boxed
                            .try_borrow::<AssetRowItem>()
                            .ok()
                            .map(|row| row.data.clone())
                    });
                if let Some(data) = item {
                    sender.input(BrowserInput::SelectItem(data));
                }
            }
        });

        let filter = AssetFilterState {
            host: ProjectHost::Modrinth,
            project_type: init.project_type,
            query: String::new(),
            game_version: None,
            loader: None,
            category: None,
            sort_by: SearchIndex::Relevance,
            offset: 0,
            limit: PAGE_LIMIT,
        };

        let model = Self {
            config: init.config,
            client: init.client,
            cache_dir: init.cache_dir,
            filter,
            total_hits: 0,
            list_view,
            state: LoadState::Idle,
            request_id: 0,
        };

        let widgets = view_output!();

        // Attach virtualized list view into the scrollable clamp
        widgets.list_clamp.set_child(Some(&model.list_view.view));

        ComponentParts { model, widgets }
    }

    fn update(&mut self, msg: Self::Input, sender: ComponentSender<Self>, _root: &Self::Root) {
        match msg {
            BrowserInput::HostChanged(host) => {
                if self.filter.host != host {
                    self.filter.host = host;
                    self.total_hits = 0;
                    self.list_view.clear();
                    self.spawn_search(&sender, 0, false);
                }
            }
            BrowserInput::QueryChanged(query) => {
                let query = query.trim().to_string();
                if self.filter.query != query {
                    self.filter.query = query;
                    self.spawn_search(&sender, 0, false);
                }
            }
            BrowserInput::LoaderChanged(loader) => {
                if self.filter.loader != loader {
                    self.filter.loader = loader;
                    self.spawn_search(&sender, 0, false);
                }
            }
            BrowserInput::SortChanged(sort) => {
                if self.filter.sort_by != sort {
                    self.filter.sort_by = sort;
                    self.spawn_search(&sender, 0, false);
                }
            }
            BrowserInput::PrevPage => {
                if self.can_go_prev() {
                    let target_offset = self.filter.offset.saturating_sub(PAGE_LIMIT);
                    self.spawn_search(&sender, target_offset, false);
                }
            }
            BrowserInput::NextPage => {
                if self.can_go_next() {
                    let target_offset = self.filter.offset.saturating_add(PAGE_LIMIT);
                    self.spawn_search(&sender, target_offset, false);
                }
            }
            BrowserInput::RefreshForce => {
                self.spawn_search(&sender, self.filter.offset, true);
            }
            BrowserInput::SelectItem(data) => {
                self.list_view
                    .selection_model
                    .set_selected(gtk::INVALID_LIST_POSITION);
                let _ = sender.output(BrowserOutput::Selected(data));
            }
            BrowserInput::EnsureLoaded => {
                if matches!(self.state, LoadState::Idle | LoadState::Error(_)) {
                    self.spawn_search(&sender, self.filter.offset, false);
                }
            }
            BrowserInput::Refresh => {}
        }
    }

    fn update_cmd(
        &mut self,
        msg: Self::CommandOutput,
        sender: ComponentSender<Self>,
        _root: &Self::Root,
    ) {
        match msg {
            BrowserCmdOutput::SearchLoaded { request_id, result } => {
                // Ignore the expired old request results, keep state as loading and wait for the latest request to return
                if request_id != self.request_id {
                    return;
                }

                match result {
                    Ok((items, total_hits)) => {
                        self.state = LoadState::Ready;
                        self.total_hits = total_hits;
                        self.list_view.clear();
                        self.list_view
                            .extend_from_iter(items.into_iter().map(|data| AssetRowItem { data }));
                        // Scroll the list back to the top
                        if let Some(adj) = self.list_view.view.vadjustment() {
                            adj.set_value(0.0);
                        }
                    }
                    Err(err) => {
                        error!("Failed to search projects: {err}");
                        let msg = t!(
                            "download-browser-error-search-projects",
                            "Failed to search projects: {err}",
                            err = err
                        );
                        self.state = LoadState::Error(msg.clone());
                        let _ = sender.output(BrowserOutput::ShowToast(msg));
                    }
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_format_downloads() {
        assert_eq!(format_downloads(0), "0");
        assert_eq!(format_downloads(999), "999");
        assert_eq!(format_downloads(1_000), "1.0K");
        assert_eq!(format_downloads(1_540), "1.5K");
        assert_eq!(format_downloads(999_949), "999.9K");
        assert_eq!(format_downloads(1_000_000), "1.0M");
        assert_eq!(format_downloads(45_670_000), "45.7M");
    }

    #[test]
    fn test_clamp_total_hits() {
        // Modrinth total hits should never be clamped.
        for total in [0, 1, 9_999, 10_000, 10_001, 150_000, u32::MAX] {
            assert_eq!(
                BrowserPage::clamp_total_hits(ProjectHost::Modrinth, total),
                total
            );
        }

        // CurseForge total hits should be capped at CURSEFORGE_MAX_RESULTS (10,000).
        assert_eq!(BrowserPage::clamp_total_hits(ProjectHost::CurseForge, 0), 0);
        assert_eq!(
            BrowserPage::clamp_total_hits(ProjectHost::CurseForge, 9_999),
            9_999
        );
        assert_eq!(
            BrowserPage::clamp_total_hits(ProjectHost::CurseForge, CURSEFORGE_MAX_RESULTS),
            CURSEFORGE_MAX_RESULTS
        );
        assert_eq!(
            BrowserPage::clamp_total_hits(ProjectHost::CurseForge, 10_001),
            CURSEFORGE_MAX_RESULTS
        );
        assert_eq!(
            BrowserPage::clamp_total_hits(ProjectHost::CurseForge, 150_000),
            CURSEFORGE_MAX_RESULTS
        );
        assert_eq!(
            BrowserPage::clamp_total_hits(ProjectHost::CurseForge, u32::MAX),
            CURSEFORGE_MAX_RESULTS
        );
    }
}
