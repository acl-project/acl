use std::path::PathBuf;
use std::sync::Arc;

use acl_auth::{Account, AccountStore, AccountType};
use gtk::prelude::*;
use libadwaita::prelude::*;
use relm4::{ComponentParts, SimpleComponent};
use relm4::{SharedState, prelude::*};
use tracing::error;
use uuid::Uuid;

#[derive(Debug, Clone)]
pub struct AccountRow {
    account: Account,
    is_active: bool,
}

#[derive(Debug)]
pub enum AccountRowInput {
    SetActive(bool),
}

#[derive(Debug)]
pub enum AccountRowOutput {
    Select(Uuid),
    Remove(Uuid),
}

impl AccountRow {
    fn account_type_label(&self) -> &'static str {
        match self.account.account_type {
            AccountType::Offline => "Offline",
            AccountType::Microsoft => "Microsoft",
        }
    }
}

#[relm4::factory(pub)]
impl FactoryComponent for AccountRow {
    type Init = (Account, bool);
    type Input = AccountRowInput;
    type Output = AccountRowOutput;
    type CommandOutput = ();
    type ParentWidget = adw::PreferencesGroup;

    view! {
        #[root]
        #[name = "row"]
        adw::ActionRow {
            #[watch]
            set_title: &self.account.username,
            #[watch]
            set_subtitle: self.account_type_label(),

            set_selectable: false,
            set_focus_on_click: false,
            // Clicking the action row selects the current user.
            set_activatable: true,
            connect_activated[sender, uuid = self.account.uuid] => move |_| {
                let _ = sender.output(AccountRowOutput::Select(uuid));
            },

            // Reserved avatar icon slot for player skin head
            add_prefix = &adw::Avatar {
                set_size: 36,
                #[watch]
                set_text: Some(&self.account.username),
                set_show_initials: false,
            },

            #[name = "select_check"]
            add_suffix = &gtk::CheckButton {
                set_valign: gtk::Align::Center,
                add_css_class: "selection-mode",
                set_can_focus: false,
                set_can_target: false,
                #[watch]
                set_active: self.is_active,
            },

            add_suffix = &gtk::Button {
                set_icon_name: "user-trash-symbolic",
                set_valign: gtk::Align::Center,
                add_css_class: "flat",
                set_tooltip_text: Some("Remove account"),
                connect_clicked[delete_dialog, row] => move |_| {
                    delete_dialog.present(Some(&row));
                },
            },

        },

        #[name = "delete_dialog"]
        adw::AlertDialog {
            set_heading: Some("Delete Account"),
            #[watch]
            set_body: &format!("Are you sure you want to delete account \"{}\"?", self.account.username),

            add_response: ("cancel", "Cancel"),
            add_response: ("delete", "Delete"),
            set_response_appearance: ("delete", adw::ResponseAppearance::Destructive),
            set_default_response: Some("cancel"),
            set_close_response: "cancel",

            connect_response: (Some("delete"), {
                let sender = sender.clone();
                let uuid = self.account.uuid;
                move |_, _| {
                    let _ = sender.output(AccountRowOutput::Remove(uuid));
                }
            }),

        }

    }

    fn init_model(
        (account, is_active): Self::Init,
        _index: &Self::Index,
        _sender: FactorySender<Self>,
    ) -> Self {
        Self { account, is_active }
    }

    fn update(&mut self, msg: Self::Input, _sender: FactorySender<Self>) {
        match msg {
            AccountRowInput::SetActive(active) => {
                self.is_active = active;
            }
        }
    }
}

pub struct AccountsPage {
    accounts: Arc<SharedState<AccountStore>>,
    accounts_path: PathBuf,
    account_rows: FactoryVecDeque<AccountRow>,
}

impl AccountsPage {
    fn save_accounts(&self, sender: &ComponentSender<Self>) {
        if let Err(err) = self.accounts.read().save(&self.accounts_path) {
            let msg = format!("Failed to save accounts: {err}");
            error!("{msg}");
            let _ = sender.output(AccountsOutput::ShowToast(msg));
        }
    }

    fn rebuild_rows(&mut self) {
        let store = self.accounts.read();
        let active_uuid = store.active_uuid();
        let mut guard = self.account_rows.guard();
        guard.clear();
        for account in store.accounts() {
            let is_active = Some(account.uuid) == active_uuid;
            guard.push_back((account.clone(), is_active));
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AccountsOutput {
    ShowToast(String),
}

#[derive(Debug)]
pub enum AccountsInput {
    AddOfflineAccount(String),
    SelectAccount(Uuid),
    RemoveAccount(Uuid),
}

#[relm4::component(pub)]
impl SimpleComponent for AccountsPage {
    type Init = (Arc<SharedState<AccountStore>>, PathBuf);
    type Output = AccountsOutput;
    type Input = AccountsInput;

    view! {
        #[root]
        #[name = "page"]
        adw::PreferencesPage {
            set_title: "Accounts",

            #[local_ref]
            accounts_group -> adw::PreferencesGroup {
                set_title: "Accounts",
                set_description: Some("Select an account to launch Minecraft"),

                #[wrap(Some)]
                set_header_suffix = &gtk::Button {
                    add_css_class: "flat",
                    set_valign: gtk::Align::Center,
                    set_tooltip_text: Some("Add an account"),

                    adw::ButtonContent {
                        set_icon_name: "list-add-symbolic",
                        set_label: "Add Account",
                    },

                    connect_clicked[add_dialog, username_entry, page] => move |_| {
                            username_entry.set_text("");
                            add_dialog.present(Some(&page));
                    },
                },

            }


        },

        #[name = "add_dialog"]
        adw::AlertDialog {
            set_heading: Some("Add Offline Account"),
            set_body: "Enter a Minecraft username (3-16 characters, letters, numbers, and underscores).",

            add_response: ("cancel", "Cancel"),
            add_response: ("add", "Add"),
            set_response_appearance: ("add", adw::ResponseAppearance::Suggested),
            set_default_response: Some("add"),
            set_close_response: "cancel",

            #[name = "username_entry"]
            #[wrap(Some)]
            set_extra_child = &gtk::Entry {
                set_placeholder_text: Some("Username (e.g. Steve)"),
                set_activates_default: true,
            },

            connect_response: (Some("add"), {
                let sender = sender.clone();
                let username_entry = username_entry.clone();
                move |_, _| {
                    let username = username_entry.text().trim().to_owned();
                    sender.input(AccountsInput::AddOfflineAccount(username));
                }
            }),

        }

    }

    fn init(
        (accounts, accounts_path): Self::Init,
        root: Self::Root,
        sender: relm4::prelude::ComponentSender<Self>,
    ) -> relm4::prelude::ComponentParts<Self> {
        let account_rows = FactoryVecDeque::builder()
            .launch(adw::PreferencesGroup::default())
            .forward(sender.input_sender(), |output| match output {
                AccountRowOutput::Select(uuid) => AccountsInput::SelectAccount(uuid),
                AccountRowOutput::Remove(uuid) => AccountsInput::RemoveAccount(uuid),
            });

        let mut model = AccountsPage {
            accounts,
            accounts_path,
            account_rows,
        };

        model.rebuild_rows();

        let accounts_group = model.account_rows.widget();
        let widgets = view_output!();

        ComponentParts { model, widgets }
    }

    fn update(&mut self, msg: Self::Input, sender: ComponentSender<Self>) {
        match msg {
            AccountsInput::AddOfflineAccount(username) => match Account::new_offline(username) {
                Ok(account) => {
                    self.accounts.write().add_account(account);
                    self.save_accounts(&sender);
                    self.rebuild_rows();
                }

                Err(err) => {
                    let msg = format!("Invalid username: {err}");
                    error!("{msg}");
                    let _ = sender.output(AccountsOutput::ShowToast(msg));
                }
            },

            AccountsInput::SelectAccount(uuid) => {
                let res = self.accounts.write().set_active_account(uuid).map(|_| ());

                if let Err(err) = res {
                    let msg = format!("Failed to select account: {err}");
                    error!("{msg}");
                    let _ = sender.output(AccountsOutput::ShowToast(msg));
                    return;
                }
                self.save_accounts(&sender);

                let mut guard = self.account_rows.guard();
                for i in 0..guard.len() {
                    if let Some(row) = guard.get_mut(i) {
                        row.is_active = row.account.uuid == uuid;
                    }
                }
            }

            AccountsInput::RemoveAccount(uuid) => {
                if let Err(err) = self.accounts.write().remove_account(uuid) {
                    let msg = format!("Failed to remove account: {err}");
                    error!("{msg}");
                    let _ = sender.output(AccountsOutput::ShowToast(msg));
                    return;
                }
                self.save_accounts(&sender);
                self.rebuild_rows();
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_account_type_label() {
        let offline_row = AccountRow {
            account: Account {
                uuid: Uuid::nil(),
                username: "Steve".to_string(),
                account_type: AccountType::Offline,
            },
            is_active: false,
        };
        assert_eq!(offline_row.account_type_label(), "Offline");

        let ms_row = AccountRow {
            account: Account {
                uuid: Uuid::nil(),
                username: "Alex".to_string(),
                account_type: AccountType::Microsoft,
            },
            is_active: true,
        };
        assert_eq!(ms_row.account_type_label(), "Microsoft");
    }

    #[test]
    fn test_accounts_msg_equality() {
        let msg1 = AccountsOutput::ShowToast("Test".to_string());
        let msg2 = AccountsOutput::ShowToast("Test".to_string());
        let msg3 = AccountsOutput::ShowToast("Other".to_string());
        assert_eq!(msg1, msg2);
        assert_ne!(msg1, msg3);
    }
}
