use gtk::prelude::*;
use relm4::prelude::*;
use relm4::{ComponentParts, SimpleComponent};
pub struct AccountsPage;

#[relm4::component(pub)]
impl SimpleComponent for AccountsPage {
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
        let model = AccountsPage;
        let widgets = view_output!();

        ComponentParts { model, widgets }
    }
}
