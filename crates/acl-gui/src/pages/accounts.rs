// SPDX-FileCopyrightText: 2026 The Adwaita Craft Launcher Authors
// SPDX-License-Identifier: GPL-3.0-or-later

use std::path::PathBuf;
use std::sync::Arc;

use acl_auth::microsoft::session::MicrosoftLoginSession;
use acl_auth::microsoft::{MicrosoftAuth, MicrosoftLoginResult};
use acl_auth::{Account, AccountStore, AccountType, CredentialStore, MICROSOFT_CLIENT_ID};
use gtk::prelude::*;
use libadwaita::prelude::*;
use relm4::ComponentParts;
use relm4::tokio::task::AbortHandle;
use relm4::{SharedState, prelude::*};
use tracing::{error, info};
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

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub enum MsLoginState {
    #[default]
    Idle,
    Starting,
    WaitingBrowser(String),
    Failed(String),
}

impl MsLoginState {
    fn state_text(&self) -> &str {
        match self {
            MsLoginState::Idle | MsLoginState::Starting => "Starting local login session...",
            MsLoginState::WaitingBrowser(_) => "Please complete the sign-in in your web browser.",
            MsLoginState::Failed(err) => err.as_str(),
        }
    }

    fn is_logging_in(&self) -> bool {
        !matches!(self, MsLoginState::Idle | MsLoginState::Failed(_))
    }

    fn is_failed(&self) -> bool {
        matches!(self, MsLoginState::Failed(_))
    }

    fn is_waiting_browser(&self) -> bool {
        matches!(self, MsLoginState::WaitingBrowser(_))
    }
}

pub struct AccountsPage {
    accounts: Arc<SharedState<AccountStore>>,
    accounts_path: PathBuf,
    client: reqwest::Client,
    account_rows: FactoryVecDeque<AccountRow>,
    ms_login_dialog: adw::AlertDialog,
    ms_login_state: MsLoginState,
    login_abort: Option<AbortHandle>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AccountsOutput {
    ShowToast(String),
}

#[derive(Debug)]
pub enum AccountsInput {
    AddOfflineAccount(String),
    StartMicrosoftLogin,
    CopyAuthUrl,
    CancelMicrosoftLogin,
    SelectAccount(Uuid),
    RemoveAccount(Uuid),
}

#[derive(Debug)]
pub enum AccountsCmdOutput {
    OpenBrowser(String),
    MicrosoftLoginFinished(Result<MicrosoftLoginResult, String>),
}

#[derive(Debug)]
pub struct AccountsInit {
    pub store: Arc<SharedState<AccountStore>>,
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

    fn start_ms_login(&mut self, client_id: &str, sender: &ComponentSender<Self>) {
        let client = self.client.clone();
        let client_id = client_id.to_string();

        let out = sender.command_sender().clone();

        let join_handle = relm4::spawn(async move {
            let res: Result<MicrosoftLoginResult, String> = async {
                let session = MicrosoftLoginSession::start(&client_id)
                    .await
                    .map_err(|err| format!("Failed to start login session: {err}"))?;

                let _ = out.send(AccountsCmdOutput::OpenBrowser(session.auth_url.to_string()));

                MicrosoftAuth::login(session, &client)
                    .await
                    .map_err(|e| format!("Authentication failed: {e}"))
            }
            .await;

            let _ = out.send(AccountsCmdOutput::MicrosoftLoginFinished(res));
        });

        self.login_abort = Some(join_handle.abort_handle());
    }
}

#[relm4::component(pub)]
impl Component for AccountsPage {
    type Init = AccountsInit;
    type Output = AccountsOutput;
    type Input = AccountsInput;
    type CommandOutput = AccountsCmdOutput;

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

                        connect_clicked[add_dialog, username_entry, page] => move |_| {
                                username_entry.set_text("");
                                add_dialog.present(Some(&page));
                        },
                    },
                }

            }


        },

        // Dialog for offline login.
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

        },

        #[local_ref]
        ms_login_dialog -> adw::AlertDialog {
            set_heading: Some("Microsoft Login"),
            #[watch]
            set_body: model.ms_login_state.state_text(),

            add_response: ("cancel", "Cancel"),
            set_close_response: "cancel",

            #[wrap(Some)]
            set_extra_child = &gtk::Box {
                set_orientation: gtk::Orientation::Vertical,
                set_spacing: 12,
                set_halign: gtk::Align::Center,
                set_margin_top: 8,

                adw::Spinner {
                    set_width_request: 32,
                    set_height_request: 32,
                    #[watch]
                    set_visible: model.ms_login_state.is_logging_in(),
                },

                gtk::Image {
                    set_icon_name: Some("dialog-error-symbolic"),
                    set_pixel_size: 32,
                    add_css_class: "error",

                    #[watch]
                    set_visible: model.ms_login_state.is_failed(),

                },

               // Copy link button.
                gtk::Button {
                    add_css_class: "pill",
                    #[watch]
                    set_visible: model.ms_login_state.is_waiting_browser(),

                    connect_clicked => AccountsInput::CopyAuthUrl,

                    adw::ButtonContent {
                        set_icon_name: "edit-copy-symbolic",
                        set_label: "Copy Login Link",
                    },

                }

            },

            connect_response: (Some("cancel"), {
                let sender = sender.clone();
                move |_, _| {
                    sender.input(AccountsInput::CancelMicrosoftLogin);
                }
            })

        }


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

        let mut model = AccountsPage {
            accounts: init.store,
            accounts_path: init.accounts_path,
            client: init.client,
            account_rows,
            ms_login_dialog: adw::AlertDialog::new(None, None),
            ms_login_state: MsLoginState::Idle,
            login_abort: None,
        };

        model.rebuild_rows();

        let ms_login_dialog = &model.ms_login_dialog;
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

            AccountsInput::StartMicrosoftLogin => {
                let Some(client_id) = MICROSOFT_CLIENT_ID else {
                    let _ = sender.output(AccountsOutput::ShowToast(
                        "Microsoft Client ID is not configured".to_string(),
                    ));
                    return;
                };

                // Cancel the previous task.
                if let Some(handle) = self.login_abort.take() {
                    handle.abort();
                }

                self.ms_login_state = MsLoginState::Starting;
                self.ms_login_dialog.present(Some(root));

                self.start_ms_login(client_id, &sender);
            }

            AccountsInput::CancelMicrosoftLogin => {
                if let Some(handle) = self.login_abort.take() {
                    handle.abort();
                    info!("Microsoft login cancelled by user");
                }
                self.ms_login_state = MsLoginState::Idle;
            }

            AccountsInput::CopyAuthUrl => {
                if let MsLoginState::WaitingBrowser(url) = &self.ms_login_state
                    && let Some(display) = gtk::gdk::Display::default()
                {
                    display.clipboard().set_text(url);
                    let _ = sender.output(AccountsOutput::ShowToast(
                        "Login link copied to clipboard".to_string(),
                    ));
                }
            }
        }
    }

    fn update_cmd(
        &mut self,
        msg: Self::CommandOutput,
        sender: ComponentSender<Self>,
        _root: &Self::Root,
    ) {
        match msg {
            AccountsCmdOutput::OpenBrowser(url) => {
                self.ms_login_state = MsLoginState::WaitingBrowser(url.clone());
                if let Err(err) = gtk::gio::AppInfo::launch_default_for_uri(
                    &url,
                    gtk::gio::AppLaunchContext::NONE,
                ) {
                    let msg = format!("Failed to open browser automatically: {err}");
                    error!("{msg}");
                    let _ = sender.output(AccountsOutput::ShowToast(msg));
                }
            }
            AccountsCmdOutput::MicrosoftLoginFinished(res) => {
                self.login_abort = None;
                match res {
                    Ok(login_res) => {
                        if let Err(err) =
                            CredentialStore::set(login_res.account.uuid, &login_res.tokens)
                        {
                            // Failed to save credentials; unable to create the account.
                            error!("{err}");
                            self.ms_login_state = MsLoginState::Failed(err.to_string());
                        } else {
                            self.ms_login_state = MsLoginState::Idle;
                            self.ms_login_dialog.force_close();

                            self.accounts.write().add_account(login_res.account);
                            self.save_accounts(&sender);
                            self.rebuild_rows();
                            let _ = sender.output(AccountsOutput::ShowToast(
                                "Microsoft account added successfully".to_string(),
                            ));
                        }
                    }
                    Err(err) => {
                        error!("{err}");
                        self.ms_login_state = MsLoginState::Failed(err);
                    }
                }
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

    #[test]
    fn test_ms_login_state_behavior() {
        assert_eq!(MsLoginState::default(), MsLoginState::Idle);

        let idle = MsLoginState::Idle;
        assert_eq!(idle.state_text(), "Starting local login session...");
        assert!(!idle.is_logging_in());
        assert!(!idle.is_failed());
        assert!(!idle.is_waiting_browser());

        let starting = MsLoginState::Starting;
        assert_eq!(starting.state_text(), "Starting local login session...");
        assert!(starting.is_logging_in());
        assert!(!starting.is_failed());
        assert!(!starting.is_waiting_browser());

        let waiting = MsLoginState::WaitingBrowser("https://example.com".to_string());
        assert_eq!(
            waiting.state_text(),
            "Please complete the sign-in in your web browser."
        );
        assert!(waiting.is_logging_in());
        assert!(!waiting.is_failed());
        assert!(waiting.is_waiting_browser());

        let failed = MsLoginState::Failed("Network timeout".to_string());
        assert_eq!(failed.state_text(), "Network timeout");
        assert!(!failed.is_logging_in());
        assert!(failed.is_failed());
        assert!(!failed.is_waiting_browser());
    }

    #[test]
    fn test_accounts_cmd_output_debug() {
        let open_browser = AccountsCmdOutput::OpenBrowser("https://login.live.com".to_string());
        assert!(format!("{open_browser:?}").contains("OpenBrowser"));
    }
}
