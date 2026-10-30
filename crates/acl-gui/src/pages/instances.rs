mod create;
mod list;

use std::sync::Arc;

use acl_core::manager::InstanceManager;
use relm4::{ComponentParts, SimpleComponent};
use relm4::{SharedState, prelude::*};

use crate::pages::instances::list::{ListMsg, ListPage};

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
}

#[derive(Debug, Clone)]
pub enum InstancesPageMsg {
    ShowToast(String),
}

#[relm4::component(pub)]
impl SimpleComponent for InstancesPage {
    type Init = Arc<SharedState<InstanceManager>>;
    type Output = InstancesPageMsg;
    type Input = ();

    view! {
        #[root]
        gtk::Stack {
            set_transition_type: gtk::StackTransitionType::SlideLeftRight,
            #[watch]
            set_visible_child_name: model.current_view.as_str(),

            add_named[Some(InstancesView::List.as_str())] = model.list_page.widget(),

        }
    }

    fn init(
        instances_mgr: Self::Init,
        root: Self::Root,
        sender: relm4::prelude::ComponentSender<Self>,
    ) -> relm4::prelude::ComponentParts<Self> {
        let list_page =
            ListPage::builder()
                .launch(instances_mgr)
                .forward(sender.output_sender(), |msg| match msg {
                    ListMsg::ShowToast(message) => InstancesPageMsg::ShowToast(message),
                });

        let model = InstancesPage {
            current_view: InstancesView::List,
            list_page,
        };

        let widgets = view_output!();

        ComponentParts { model, widgets }
    }
}
