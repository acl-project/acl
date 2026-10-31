mod create;
mod list;

use std::sync::Arc;

use acl_core::manager::InstanceManager;
use acl_core::{LauncherConfig, LauncherDirs};
use acl_minecraft::VersionEntry;
use relm4::{ComponentParts, SimpleComponent};
use relm4::{SharedState, prelude::*};

use crate::pages::instances::create::{CreateOutput, CreatePage, CreatePageInit};
use crate::pages::instances::list::{ListInput, ListOutput, ListPage, ListPageInit};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum InstancesView {
    #[default]
    List,
    Create,
}

impl InstancesView {
    fn as_str(self) -> &'static str {
        match self {
            Self::List => "list",
            Self::Create => "create",
        }
    }
}

pub struct InstancesPage {
    current_view: InstancesView,
    list_page: Controller<ListPage>,
    create_page: Controller<CreatePage>,
}

#[derive(Debug, Clone)]
pub enum InstancesPageOutput {
    ShowToast(String),
}

#[derive(Debug, Clone)]
pub enum InstancesPageInput {
    ChangeView(InstancesView),
    ShowToast(String),
    InstanceCreated(String, VersionEntry),
}

pub struct InstancesInit {
    pub mgr: Arc<SharedState<InstanceManager>>,
    pub config: Arc<SharedState<LauncherConfig>>,
    pub client: reqwest::Client,
    pub dirs: LauncherDirs,
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
            add_named[Some(InstancesView::Create.as_str())] = model.create_page.widget(),

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
                config: init.config,
                client: init.client.clone(),
                dirs: init.dirs.clone(),
            })
            .forward(sender.input_sender(), |msg| match msg {
                ListOutput::ShowToast(message) => InstancesPageInput::ShowToast(message),
                ListOutput::OpenCreate => InstancesPageInput::ChangeView(InstancesView::Create),
            });

        let create_page = CreatePage::builder()
            .launch(CreatePageInit {
                mgr: init.mgr,
                client: init.client,
                cache_dir: init.dirs.cache_dir().to_path_buf(),
            })
            .forward(sender.input_sender(), |msg| match msg {
                CreateOutput::Cancel => InstancesPageInput::ChangeView(InstancesView::List),
                CreateOutput::Created(id, entry) => InstancesPageInput::InstanceCreated(id, entry),
                CreateOutput::ShowToast(text) => InstancesPageInput::ShowToast(text),
            });

        let model = InstancesPage {
            current_view: InstancesView::List,
            list_page,
            create_page,
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
            InstancesPageInput::InstanceCreated(id, entry) => {
                self.list_page.emit(ListInput::Refresh);
                self.list_page.emit(ListInput::StartDownload(id, entry));
                sender.input(InstancesPageInput::ChangeView(InstancesView::List));
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
        assert_eq!(InstancesView::Create.as_str(), "create");
    }
}
