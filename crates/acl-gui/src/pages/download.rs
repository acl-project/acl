// SPDX-FileCopyrightText: 2026 The Adwaita Craft Launcher Authors
// SPDX-License-Identifier: GPL-3.0-or-later

use std::path::PathBuf;
use std::str::FromStr;
use std::sync::Arc;

use crate::icon_cache::IconCache;
use crate::pages::download::browser::{BrowserInit, BrowserInput, BrowserOutput, BrowserPage};
use crate::pages::download::game::{GameInit, GameInput, GameOutput, GamePage};
use crate::t;
use acl_core::LauncherConfig;
use acl_minecraft::{AssetItemData, ProjectType, VersionEntry};
use adw::prelude::*;
use libadwaita::NavigationPage;
use relm4::prelude::*;
use relm4::{SharedState, SimpleComponent};

pub mod browser;
pub mod game;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum NavPage {
    #[default]
    Game,
    Mod,
    Modpack,
    ResourcePack,
    Shader,
}

impl NavPage {
    fn as_str(self) -> &'static str {
        match self {
            Self::Game => "game",
            Self::Mod => "mod",
            Self::ResourcePack => "resource-pack",
            Self::Shader => "shader",
            Self::Modpack => "modpack",
        }
    }

    fn title(self) -> String {
        match self {
            Self::Game => t!("download-navigation-page-game", "Game"),
            Self::Mod => t!("download-navigation-page-mod", "Mods"),
            Self::ResourcePack => t!("download-navigation-page-resource-pack", "Resource Packs"),
            Self::Shader => t!("download-navigation-page-shader", "Shaders"),
            Self::Modpack => t!("download-navigation-page-modpack", "Modpacks"),
        }
    }
}

impl FromStr for NavPage {
    type Err = ();

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "game" => Ok(Self::Game),
            "mod" => Ok(Self::Mod),
            "resource-pack" => Ok(Self::ResourcePack),
            "shader" => Ok(Self::Shader),
            "modpack" => Ok(Self::Modpack),
            _ => Err(()),
        }
    }
}

pub struct DownloadPage {
    current_page: NavPage,
    stack: adw::ViewStack,
    game_page: Controller<GamePage>,
    mod_page: Controller<BrowserPage>,
    modpack_page: Controller<BrowserPage>,
    resource_pack_page: Controller<BrowserPage>,
    shader_page: Controller<BrowserPage>,
}

impl DownloadPage {
    fn activate_current_page(&self) {
        match self.current_page {
            NavPage::Game => self.game_page.emit(GameInput::EnsureLoaded),
            NavPage::Mod => self.mod_page.emit(BrowserInput::EnsureLoaded),
            NavPage::Modpack => self.modpack_page.emit(BrowserInput::EnsureLoaded),
            NavPage::ResourcePack => self.resource_pack_page.emit(BrowserInput::EnsureLoaded),
            NavPage::Shader => self.shader_page.emit(BrowserInput::EnsureLoaded),
        }
    }
}

pub struct DownloadInit {
    pub config: Arc<SharedState<LauncherConfig>>,
    pub client: reqwest::Client,
    pub cache_dir: PathBuf,
    pub icon_cache: Arc<IconCache>,
}

#[derive(Debug)]
pub enum DownloadInput {
    Navigate(NavPage),
    Refresh,
    SelectGameVersion(VersionEntry),
    ShowToast(String),
    SelectAsset(AssetItemData),
    Activate,
}

#[derive(Debug)]
pub enum DownloadOutput {
    ShowToast(String),
    Back,
    SelectGameVersion(VersionEntry),
    SelectAsset(Box<AssetItemData>),
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
                        add_titled_with_icon: (model.modpack_page.widget(), Some(NavPage::Modpack.as_str()), &NavPage::Modpack.title(), "package-x-generic-symbolic"),
                        add_titled_with_icon: (model.mod_page.widget(), Some(NavPage::Mod.as_str()), &NavPage::Mod.title(), "application-x-addon-symbolic"),
                        add_titled_with_icon: (model.resource_pack_page.widget(), Some(NavPage::ResourcePack.as_str()), &NavPage::ResourcePack.title(), "applications-graphics-symbolic"),
                        add_titled_with_icon: (model.shader_page.widget(), Some(NavPage::Shader.as_str()), &NavPage::Shader.title(), "weather-clear-symbolic"),

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
                config: Arc::clone(&init.config),
                client: init.client.clone(),
                cache_dir: init.cache_dir.clone(),
            })
            .forward(sender.input_sender(), |msg| match msg {
                GameOutput::Selected(version) => DownloadInput::SelectGameVersion(version),
                GameOutput::ShowToast(message) => DownloadInput::ShowToast(message),
            });

        let launch_browser = |project_type: ProjectType| {
            BrowserPage::builder()
                .launch(BrowserInit {
                    project_type,
                    config: Arc::clone(&init.config),
                    client: init.client.clone(),
                    cache_dir: init.cache_dir.clone(),
                    icon_cache: Arc::clone(&init.icon_cache),
                })
                .forward(sender.input_sender(), |msg| match msg {
                    BrowserOutput::Selected(item) => DownloadInput::SelectAsset(item),
                    BrowserOutput::ShowToast(m) => DownloadInput::ShowToast(m),
                })
        };

        let mod_page = launch_browser(ProjectType::Mod);
        let resource_pack_page = launch_browser(ProjectType::ResourcePack);
        let shader_page = launch_browser(ProjectType::Shader);
        let modpack_page = launch_browser(ProjectType::Modpack);

        let model = DownloadPage {
            current_page: NavPage::Game,
            stack: adw::ViewStack::new(),
            game_page,
            mod_page,
            modpack_page,
            resource_pack_page,
            shader_page,
        };

        let stack = &model.stack;

        let widgets = view_output!();

        model.stack.connect_visible_child_name_notify({
            let sender = sender.clone();
            let split_view = root.clone();
            move |stack| {
                if let Some(name) = stack.visible_child_name()
                    && let Ok(page) = name.parse::<NavPage>()
                {
                    sender.input(DownloadInput::Navigate(page));
                }
                split_view.set_show_content(true);
            }
        });

        ComponentParts { model, widgets }
    }

    fn update(&mut self, msg: Self::Input, sender: ComponentSender<Self>) {
        match msg {
            DownloadInput::Refresh => {
                self.stack
                    .page(self.game_page.widget())
                    .set_title(Some(&NavPage::Game.title()));
                self.stack
                    .page(self.modpack_page.widget())
                    .set_title(Some(&NavPage::Modpack.title()));
                self.stack
                    .page(self.mod_page.widget())
                    .set_title(Some(&NavPage::Mod.title()));
                self.stack
                    .page(self.resource_pack_page.widget())
                    .set_title(Some(&NavPage::ResourcePack.title()));
                self.stack
                    .page(self.shader_page.widget())
                    .set_title(Some(&NavPage::Shader.title()));
            }
            DownloadInput::SelectGameVersion(version) => {
                let _ = sender.output(DownloadOutput::SelectGameVersion(version));
            }
            DownloadInput::ShowToast(message) => {
                let _ = sender.output(DownloadOutput::ShowToast(message));
            }
            DownloadInput::SelectAsset(item) => {
                let _ = sender.output(DownloadOutput::SelectAsset(Box::new(item)));
            }
            DownloadInput::Navigate(nav) => {
                self.current_page = nav;
                self.activate_current_page();
            }
            DownloadInput::Activate => {
                self.activate_current_page();
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_download_nav_page_default_as_str_and_title() {
        assert_eq!(NavPage::default(), NavPage::Game);

        let cases = [
            (NavPage::Game, "game", "Game"),
            (NavPage::Mod, "mod", "Mods"),
            (NavPage::Modpack, "modpack", "Modpacks"),
            (NavPage::ResourcePack, "resource-pack", "Resource Packs"),
            (NavPage::Shader, "shader", "Shaders"),
        ];
        for (page, expected_str, expected_title) in cases {
            assert_eq!(page.as_str(), expected_str);
            assert_eq!(page.title(), expected_title);
        }
    }

    #[test]
    fn test_download_nav_page_from_str() {
        assert_eq!("game".parse::<NavPage>(), Ok(NavPage::Game));
        assert_eq!("mod".parse::<NavPage>(), Ok(NavPage::Mod));
        assert_eq!("modpack".parse::<NavPage>(), Ok(NavPage::Modpack));
        assert_eq!(
            "resource-pack".parse::<NavPage>(),
            Ok(NavPage::ResourcePack)
        );
        assert_eq!("shader".parse::<NavPage>(), Ok(NavPage::Shader));

        assert_eq!("".parse::<NavPage>(), Err(()));
        assert_eq!("unknown".parse::<NavPage>(), Err(()));
    }
}
