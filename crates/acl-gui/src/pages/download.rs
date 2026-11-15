// SPDX-FileCopyrightText: 2026 The Adwaita Craft Launcher Authors
// SPDX-License-Identifier: GPL-3.0-or-later

use std::path::PathBuf;
use std::sync::Arc;

use crate::pages::download::game::{GameInit, GameOutput, GamePage};
use crate::t;
use acl_core::LauncherConfig;
use acl_minecraft::VersionEntry;
use adw::prelude::*;
use libadwaita::NavigationPage;
use relm4::prelude::*;
use relm4::{SharedState, SimpleComponent};

pub mod game;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum NavPage {
    #[default]
    Game,
}

impl NavPage {
    fn as_str(self) -> &'static str {
        match self {
            Self::Game => "game",
        }
    }

    fn title(self) -> String {
        match self {
            Self::Game => t!("download-navigation-page-game", "Game"),
        }
    }
}

pub struct DownloadPage {
    current_page: NavPage,
    stack: adw::ViewStack,
    game_page: Controller<GamePage>,
}

pub struct DownloadInit {
    pub config: Arc<SharedState<LauncherConfig>>,
    pub client: reqwest::Client,
    pub cache_dir: PathBuf,
}

#[derive(Debug)]
pub enum DownloadInput {
    Refresh,
    SelectGameVersion(VersionEntry),
    ShowToast(String),
}

#[derive(Debug)]
pub enum DownloadOutput {
    ShowToast(String),
    Back,
    SelectGameVersion(VersionEntry),
}

#[relm4::component(pub)]
impl SimpleComponent for DownloadPage {
    type Init = DownloadInit;
    type Input = DownloadInput;
    type Output = DownloadOutput;

    view! {
        #[root]
        adw::NavigationSplitView {
            #[wrap(Some)]
            set_sidebar = &adw::NavigationPage {

                #[watch]
                set_title: &t!("download-page-title", "Download Center"),

                #[wrap(Some)]
                set_child = &adw::ToolbarView {
                    add_top_bar = &adw::HeaderBar {
                        pack_start = &gtk::Button {
                            set_icon_name: "go-previous-symbolic",
                            #[watch]
                            set_tooltip_text: Some(&t!("instance-detail-button-back", "Back")),
                            connect_clicked[sender] => move |_|{
                                let _ = sender.output(DownloadOutput::Back);
                            }
                        },

                    },
                    #[wrap(Some)]
                    set_content = &adw::ViewSwitcherSidebar {
                        set_stack: Some(stack),
                    },
                },
            },

            #[wrap(Some)]
            set_content = &NavigationPage {

                #[watch]
                set_title: &model.current_page.title(),

                #[wrap(Some)]
                set_child = &adw::ToolbarView {

                    add_top_bar = &adw::HeaderBar {},

                    #[wrap(Some)]
                    #[local_ref]
                    set_content = stack ->adw::ViewStack {

                        add_titled_with_icon: (model.game_page.widget(), Some(NavPage::Game.as_str()), &NavPage::Game.title(), "input-gaming-symbolic"),

                        #[watch]
                        set_visible_child_name: model.current_page.as_str(),
                    },
                }
            }

        },
    }

    fn init(
        init: Self::Init,
        root: Self::Root,
        sender: ComponentSender<Self>,
    ) -> ComponentParts<Self> {
        init.config
            .subscribe(sender.input_sender(), |_| DownloadInput::Refresh);

        let game_page = GamePage::builder()
            .launch(GameInit {
                config: init.config,
                client: init.client,
                cache_dir: init.cache_dir,
            })
            .forward(sender.input_sender(), |msg| match msg {
                GameOutput::Selected(version) => DownloadInput::SelectGameVersion(version),
                GameOutput::ShowToast(message) => DownloadInput::ShowToast(message),
            });

        let model = DownloadPage {
            current_page: NavPage::Game,
            stack: adw::ViewStack::new(),
            game_page,
        };

        let stack = &model.stack;

        let widgets = view_output!();

        model.stack.connect_visible_child_name_notify({
            let split_view = root.clone();
            move |_stack| {
                split_view.set_show_content(true);
            }
        });

        ComponentParts { model, widgets }
    }

    fn update(&mut self, msg: Self::Input, sender: ComponentSender<Self>) {
        match msg {
            DownloadInput::Refresh => self
                .stack
                .page(self.game_page.widget())
                .set_title(Some(&NavPage::Game.title())),
            DownloadInput::SelectGameVersion(version) => {
                let _ = sender.output(DownloadOutput::SelectGameVersion(version));
            }
            DownloadInput::ShowToast(message) => {
                let _ = sender.output(DownloadOutput::ShowToast(message));
            }
        }
    }
}
