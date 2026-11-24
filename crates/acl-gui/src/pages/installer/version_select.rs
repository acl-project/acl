// SPDX-FileCopyrightText: 2026 The Adwaita Craft Launcher Authors
// SPDX-License-Identifier: GPL-3.0-or-later

use std::sync::Arc;

use acl_core::LauncherConfig;
use acl_minecraft::{AssetVersion, group_versions_by_game_version};
use gtk::prelude::*;
use libadwaita::prelude::*;
use relm4::ComponentParts;
use relm4::{SharedState, prelude::*};

use crate::t;
pub struct VersionSelectInit {
    pub config: Arc<SharedState<LauncherConfig>>,
}

#[derive(Debug)]
pub enum VersionSelectInput {
    /// When opening this page, complete data is passed in by InstallerPage
    Load {
        title: String,
        target_mc_version: Option<String>,
        versions: Vec<AssetVersion>,
    },

    Selected(Box<AssetVersion>),
    Back,
    Refresh,
}

#[derive(Debug, Clone)]
pub enum VersionSelectOutput {
    Back,
    Selected(Box<AssetVersion>),
}

pub struct VersionSelectPage {
    title: String,
    groups_container: adw::PreferencesGroup,
    expander_rows: Vec<adw::ExpanderRow>,
}

impl VersionSelectPage {
    fn rebuild_list(
        &mut self,
        target_mc_version: Option<&str>,
        versions: &[AssetVersion],
        sender: &ComponentSender<Self>,
    ) {
        for row in self.expander_rows.drain(..) {
            self.groups_container.remove(&row);
        }

        let mut groups = group_versions_by_game_version(versions);

        // If there is a target instance version, bring this group to the front
        if let Some(target_mc) = target_mc_version
            && let Some(pos) = groups.iter().position(|(gv, _)| gv == target_mc)
        {
            let target_group = groups.remove(pos);
            groups.insert(0, target_group);
        }

        for (idx, (mc_version, list)) in groups.into_iter().enumerate() {
            let is_target = target_mc_version == Some(mc_version.as_str());
            // The target instance version is expanded by default;
            // If there is no target version, the latest group is expanded by default
            let should_expand = is_target || (target_mc_version.is_none() && idx == 0);

            let expander = adw::ExpanderRow::builder()
                .title(format!("Minecraft {mc_version}"))
                .subtitle(format!("{} versions", list.len()))
                .expanded(should_expand)
                .build();

            for ver in list {
                let mut parts = vec![
                    ver.release_type.label().to_string(),
                    ver.version_number.clone(),
                ];

                if !ver.loaders.is_empty() {
                    parts.push(ver.loaders.join(", "));
                }

                if ver.primary_file.is_none() {
                    parts.push(t!("installer-version-manual-only", "Manual download"));
                }

                let subtitle = parts.join(" · ");

                let row = adw::ActionRow::builder()
                    .title(&ver.name)
                    .subtitle(subtitle)
                    .activatable(true)
                    .build();
                row.add_suffix(&gtk::Image::from_icon_name("go-next-symbolic"));

                let sender_clone = sender.clone();
                let ver_clone = ver.clone();
                row.connect_activated(move |_| {
                    sender_clone.input(VersionSelectInput::Selected(Box::new(ver_clone.clone())));
                });

                expander.add_row(&row);
            }

            self.groups_container.add(&expander);
            self.expander_rows.push(expander);
        }
    }
}

#[relm4::component(pub)]
impl SimpleComponent for VersionSelectPage {
    type Init = VersionSelectInit;
    type Input = VersionSelectInput;
    type Output = VersionSelectOutput;

    view! {
        #[root]
        adw::ToolbarView {
            add_top_bar = &adw::HeaderBar {
                #[wrap(Some)]
                set_title_widget = &adw::WindowTitle {
                    #[watch]
                    set_title: &model.title,
                },

                pack_start = &gtk::Button {
                    connect_clicked => VersionSelectInput::Back,
                    adw::ButtonContent {
                        set_icon_name: "go-previous-symbolic",
                        #[watch]
                        set_label: &t!("instances-create-button-back", "Back"),
                    },
                },
            },

            #[wrap(Some)]
            set_content = &adw::PreferencesPage {
                #[local_ref]
                groups_container -> adw::PreferencesGroup {},
            }

        }
    }

    fn init(
        init: Self::Init,
        root: Self::Root,
        sender: ComponentSender<Self>,
    ) -> ComponentParts<Self> {
        init.config
            .subscribe(sender.input_sender(), |_| VersionSelectInput::Refresh);

        let model = Self {
            title: String::new(),
            groups_container: adw::PreferencesGroup::new(),
            expander_rows: Vec::new(),
        };

        let groups_container = &model.groups_container;
        let widgets = view_output!();
        ComponentParts { model, widgets }
    }

    fn update(&mut self, msg: Self::Input, sender: ComponentSender<Self>) {
        match msg {
            VersionSelectInput::Load {
                title,
                target_mc_version,
                versions,
            } => {
                self.title = title;
                self.rebuild_list(target_mc_version.as_deref(), &versions, &sender);
            }
            VersionSelectInput::Selected(ver) => {
                let _ = sender.output(VersionSelectOutput::Selected(ver));
            }
            VersionSelectInput::Back => {
                let _ = sender.output(VersionSelectOutput::Back);
            }
            VersionSelectInput::Refresh => {}
        }
    }
}
