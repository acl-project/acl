// SPDX-FileCopyrightText: 2026 The Adwaita Craft Launcher Authors
// SPDX-License-Identifier: GPL-3.0-or-later

mod list;

use std::sync::Arc;

use acl_core::LauncherConfig;
use acl_core::manager::InstanceManager;
use relm4::{ComponentParts, SimpleComponent};
use relm4::{SharedState, prelude::*};

use crate::pages::instances::list::{ListOutput, ListPage, ListPageInit};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum InstancesView {
    #[default]
    List,
}

impl InstancesView {
    fn as_str(self) -> &'static str {
        match self {
            Self::List => "list",
        }
    }
}

pub struct InstancesPage {
    current_view: InstancesView,
    list_page: Controller<ListPage>,
}

#[derive(Debug, Clone)]
pub enum InstancesPageOutput {
    ShowToast(String),
    OpenDetail(String),
}

#[derive(Debug, Clone)]
pub enum InstancesPageInput {
    ChangeView(InstancesView),
    ShowToast(String),
    OpenDetail(String),
}

pub struct InstancesInit {
    pub mgr: Arc<SharedState<InstanceManager>>,
    pub config: Arc<SharedState<LauncherConfig>>,
}

#[relm4::component(pub)]
impl SimpleComponent for InstancesPage {
    type Init = InstancesInit;
    type Output = InstancesPageOutput;
    type Input = InstancesPageInput;

    view! {
        #[root]
        gtk::Stack {
            set_transition_type: gtk::StackTransitionType::SlideLeftRight,

            add_named[Some(InstancesView::List.as_str())] = model.list_page.widget(),

            #[watch]
            set_visible_child_name: model.current_view.as_str(),

        }
    }

    fn init(
        init: Self::Init,
        root: Self::Root,
        sender: relm4::prelude::ComponentSender<Self>,
    ) -> relm4::prelude::ComponentParts<Self> {
        let list_page = ListPage::builder()
            .launch(ListPageInit {
                mgr: Arc::clone(&init.mgr),
                config: Arc::clone(&init.config),
            })
            .forward(sender.input_sender(), |msg| match msg {
                ListOutput::ShowToast(message) => InstancesPageInput::ShowToast(message),
                ListOutput::OpenDetail(id) => InstancesPageInput::OpenDetail(id),
            });

        let model = InstancesPage {
            current_view: InstancesView::List,
            list_page,
        };

        let widgets = view_output!();

        ComponentParts { model, widgets }
    }

    fn update(&mut self, msg: Self::Input, sender: ComponentSender<Self>) {
        match msg {
            InstancesPageInput::ChangeView(view) => {
                self.current_view = view;
            }
            InstancesPageInput::ShowToast(message) => {
                let _ = sender.output(InstancesPageOutput::ShowToast(message));
            }
            InstancesPageInput::OpenDetail(id) => {
                let _ = sender.output(InstancesPageOutput::OpenDetail(id));
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_instances_view_as_str_and_default() {
        assert_eq!(InstancesView::default(), InstancesView::List);
        assert_eq!(InstancesView::List.as_str(), "list");
    }
}
