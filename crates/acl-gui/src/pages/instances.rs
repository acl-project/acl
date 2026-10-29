use gtk::prelude::*;
use relm4::prelude::*;
use relm4::{ComponentParts, SimpleComponent};
pub struct InstancesPage;

#[relm4::component(pub)]
impl SimpleComponent for InstancesPage {
    type Init = ();
    type Output = ();
    type Input = ();

    view! {
        gtk::Box {
            set_orientation: gtk::Orientation::Vertical,
            set_margin_all: 24,

        }
    }

    fn init(
        _init: Self::Init,
        root: Self::Root,
        _sender: relm4::prelude::ComponentSender<Self>,
    ) -> relm4::prelude::ComponentParts<Self> {
        let model = InstancesPage;
        let widgets = view_output!();

        ComponentParts { model, widgets }
    }
}
