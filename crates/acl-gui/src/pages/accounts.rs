// SPDX-FileCopyrightText: 2026 The Adwaita Craft Launcher Authors
// SPDX-License-Identifier: GPL-3.0-or-later

use std::path::PathBuf;
use std::sync::Arc;

use acl_auth::{Account, AccountStore, AccountType, CredentialStore, MICROSOFT_CLIENT_ID};
use acl_core::LauncherConfig;
use gtk::prelude::*;
use libadwaita::prelude::*;
use relm4::ComponentParts;
use relm4::{SharedState, prelude::*};
use tracing::error;
use uuid::Uuid;

const XBOX_STORE_URL: &str =
    "https://www.xbox.com/games/store/minecraft-java-bedrock-edition-for-pc/9nxp44l49shj";

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
    config: Arc<SharedState<LauncherConfig>>,
    accounts_path: PathBuf,
    account_rows: FactoryVecDeque<AccountRow>,
    add_offline_dialog: adw::AlertDialog,
    username_entry: gtk::Entry,
    offline_disabled_dialog: adw::AlertDialog,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AccountsOutput {
    ShowToast(String),
    RequestMicrosoftLogin,
}

#[derive(Debug)]
pub enum AccountsInput {
    AddOfflineAccount(String),
    RequestAddOffline,
    StartMicrosoftLogin,
    OpenXboxStore,
    SelectAccount(Uuid),
    RemoveAccount(Uuid),
    Refresh,
}

#[derive(Debug)]
pub struct AccountsInit {
    pub store: Arc<SharedState<AccountStore>>,
    pub config: Arc<SharedState<LauncherConfig>>,
    pub accounts_path: PathBuf,
    pub client: reqwest::Client,
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

#[relm4::component(pub)]
impl Component for AccountsPage {
    type Init = AccountsInit;
    type Output = AccountsOutput;
    type Input = AccountsInput;
    type CommandOutput = ();

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
                set_header_suffix = &gtk::Box {
                    set_orientation: gtk::Orientation::Horizontal,
                    set_spacing: 8,
                    set_valign: gtk::Align::Center,

                    gtk::Button {
                        add_css_class: "flat",
                        set_valign: gtk::Align::Center,
                        set_tooltip_text: Some("Sign in with a Microsoft account"),

                        #[watch]
                        set_sensitive: MICROSOFT_CLIENT_ID.is_some(),
                        connect_clicked => AccountsInput::StartMicrosoftLogin,

                        adw::ButtonContent {
                            set_icon_name: "avatar-default-symbolic",
                            set_label: "Microsoft Login",
                        }
                    },


                    gtk::Button {
                        add_css_class: "flat",
                        set_valign: gtk::Align::Center,
                        set_tooltip_text: Some("Add an offline account"),

                        adw::ButtonContent {
                            set_icon_name: "list-add-symbolic",
                            set_label: "Add Offline",
                        },

                      connect_clicked => AccountsInput::RequestAddOffline,
                    },
                }

            }


        },

        // Dialog for offline login.
        #[local_ref]
        add_offline_dialog -> adw::AlertDialog {

            set_heading: Some("Add Offline Account"),
            set_body: "Enter a Minecraft username (3-16 characters, letters, numbers, and underscores).",

            add_response: ("cancel", "Cancel"),
            add_response: ("add", "Add"),
            set_response_appearance: ("add", adw::ResponseAppearance::Suggested),
            set_default_response: Some("add"),
            set_close_response: "cancel",

            #[local_ref]
            #[wrap(Some)]
            set_extra_child = username_entry -> gtk::Entry {
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

        },

        #[local_ref]
        offline_disabled_dialog -> adw::AlertDialog {
            set_heading: Some("Offline Mode Unavailable"),
            set_body: "You must launch the game at least once with an official Microsoft account before adding an offline account.",

            add_response: ("cancel", "Cancel"),
            add_response: ("store", "Get Minecraft"),

            set_close_response: "cancel",

            set_response_appearance: ("store", adw::ResponseAppearance::Suggested),
            set_default_response: Some("store"),

            connect_response: (Some("store"), {
                let sender = sender.clone();
                move |_, _| {
                    sender.input(AccountsInput::OpenXboxStore);
                }
            }),

        },


    }

    fn init(
        init: Self::Init,
        root: Self::Root,
        sender: relm4::prelude::ComponentSender<Self>,
    ) -> relm4::prelude::ComponentParts<Self> {
        let account_rows = FactoryVecDeque::builder()
            .launch(adw::PreferencesGroup::default())
            .forward(sender.input_sender(), |output| match output {
                AccountRowOutput::Select(uuid) => AccountsInput::SelectAccount(uuid),
                AccountRowOutput::Remove(uuid) => AccountsInput::RemoveAccount(uuid),
            });

        init.store
            .subscribe(sender.input_sender(), |_| AccountsInput::Refresh);

        let mut model = AccountsPage {
            accounts: init.store,
            config: init.config,
            accounts_path: init.accounts_path,
            account_rows,
            add_offline_dialog: adw::AlertDialog::new(None, None),
            username_entry: gtk::Entry::new(),
            offline_disabled_dialog: adw::AlertDialog::new(None, None),
        };

        model.rebuild_rows();

        let add_offline_dialog = &model.add_offline_dialog;
        let username_entry = &model.username_entry;
        let offline_disabled_dialog = &model.offline_disabled_dialog;
        let accounts_group = model.account_rows.widget();
        let widgets = view_output!();

        ComponentParts { model, widgets }
    }

    fn update(&mut self, msg: Self::Input, sender: ComponentSender<Self>, root: &Self::Root) {
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
                if let Err(err) = CredentialStore::delete(uuid) {
                    let msg = format!("Failed to remove tokens: {err}");
                    error!("{msg}");
                    let _ = sender.output(AccountsOutput::ShowToast(msg));
                }

                if let Err(err) = self.accounts.write().remove_account(uuid) {
                    let msg = format!("Failed to remove account: {err}");
                    error!("{msg}");
                    let _ = sender.output(AccountsOutput::ShowToast(msg));
                    return;
                }
                self.save_accounts(&sender);
                self.rebuild_rows();
            }

            AccountsInput::OpenXboxStore => {
                if let Err(err) = gtk::gio::AppInfo::launch_default_for_uri(
                    XBOX_STORE_URL,
                    gtk::gio::AppLaunchContext::NONE,
                ) {
                    let msg = format!("Failed to open browser: {err}");
                    error!("{msg}");
                    let _ = sender.output(AccountsOutput::ShowToast(msg));
                }
            }

            AccountsInput::RequestAddOffline => {
                let enable = self.config.read().enable_offline;
                if enable {
                    self.username_entry.set_text("");
                    self.add_offline_dialog.present(Some(root));
                } else {
                    self.offline_disabled_dialog.present(Some(root));
                }
            }
            AccountsInput::Refresh => {
                self.rebuild_rows();
            }
            AccountsInput::StartMicrosoftLogin => {
                let _ = sender.output(AccountsOutput::RequestMicrosoftLogin);
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
    fn test_accounts_output_equality() {
        let msg1 = AccountsOutput::ShowToast("Test".to_string());
        let msg2 = AccountsOutput::ShowToast("Test".to_string());
        let msg3 = AccountsOutput::ShowToast("Other".to_string());
        assert_eq!(msg1, msg2);
        assert_ne!(msg1, msg3);
    }
}
