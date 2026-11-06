// SPDX-FileCopyrightText: 2026 The Adwaita Craft Launcher Authors
// SPDX-License-Identifier: GPL-3.0-or-later

use std::{path::PathBuf, sync::Arc};

use acl_auth::microsoft::session::MicrosoftLoginSession;
use acl_auth::microsoft::{MicrosoftAuth, MicrosoftLoginResult};
use acl_auth::{AccountStore, CredentialStore, MICROSOFT_CLIENT_ID};
use gtk::prelude::*;
use libadwaita::prelude::*;
use relm4::ComponentParts;
use relm4::tokio::task::AbortHandle;
use relm4::{SharedState, prelude::*};
use tracing::{error, info};

use crate::t;

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

pub struct MsLoginInit {
    /// The mounted parent window.
    pub parent: gtk::Widget,
    pub store: Arc<SharedState<AccountStore>>,
    pub accounts_path: PathBuf,
    pub client: reqwest::Client,
}

#[derive(Debug)]
pub enum MsLoginInput {
    Start,
    /// When the Refresh Token becomes invalid, first show a popup prompting the user to log in again.
    PromptReauth {
        username: String,
    },
    CopyAuthUrl,
    Cancel,
}

#[derive(Debug)]
pub enum MsLoginCmdOutput {
    OpenBrowser(String),
    LoginFinished(Result<MicrosoftLoginResult, String>),
}

#[derive(Debug)]
pub enum MsLoginOutput {
    ShowToast(String),
}

pub struct MsLoginDialog {
    parent: gtk::Widget,
    accounts: Arc<SharedState<AccountStore>>,
    accounts_path: PathBuf,
    client: reqwest::Client,
    reauth_dialog: adw::AlertDialog,
    reauth_username: String,
    login_state: MsLoginState,
    login_abort: Option<AbortHandle>,
}

impl MsLoginDialog {
    fn save_accounts(&self, sender: &ComponentSender<Self>) {
        if let Err(err) = self.accounts.read().save(&self.accounts_path) {
            let msg = format!("Failed to save accounts: {err}");
            error!("{msg}");
            let _ = sender.output(MsLoginOutput::ShowToast(msg));
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

                let _ = out.send(MsLoginCmdOutput::OpenBrowser(session.auth_url.to_string()));

                MicrosoftAuth::login(session, &client)
                    .await
                    .map_err(|e| format!("Authentication failed: {e}"))
            }
            .await;

            let _ = out.send(MsLoginCmdOutput::LoginFinished(res));
        });

        self.login_abort = Some(join_handle.abort_handle());
    }
}

#[relm4::component(pub)]
impl Component for MsLoginDialog {
    type Init = MsLoginInit;
    type Output = MsLoginOutput;
    type Input = MsLoginInput;
    type CommandOutput = MsLoginCmdOutput;

    view! {
        #[root]
        adw::AlertDialog {
            #[watch]
            set_heading: Some(&t!("ms-login-dialog-heading","Microsoft Login")),
            #[watch]
            set_body: model.login_state.state_text(),

            add_response: ("cancel", &t!("ms-login-dialog-cancel","Cancel")),
            #[watch]
            set_response_label: ("cancel", &t!("ms-login-dialog-cancel","Cancel")),
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
                    set_visible: model.login_state.is_logging_in(),
                },

                gtk::Image {
                    set_icon_name: Some("dialog-error-symbolic"),
                    set_pixel_size: 32,
                    add_css_class: "error",

                    #[watch]
                    set_visible: model.login_state.is_failed(),

                },

               // Copy link button.
                gtk::Button {
                    add_css_class: "pill",
                    #[watch]
                    set_visible: model.login_state.is_waiting_browser(),

                    connect_clicked => MsLoginInput::CopyAuthUrl,

                    adw::ButtonContent {
                        set_icon_name: "edit-copy-symbolic",
                        #[watch]
                        set_label: &t!("ms-login-dialog-copy-link","Copy Login Link"),
                    },

                }

            },

            connect_response: (Some("cancel"), {
                let sender = sender.clone();
                move |_, _| {
                    sender.input(MsLoginInput::Cancel);
                }
            })

        },

        #[local_ref]
        reauth_dialog -> adw::AlertDialog {
            #[watch]
            set_heading: Some(&t!("reauth-dialog-heading","Session Expired")),
            #[watch]
            set_body: &t!(
                "reauth-dialog-body",
                "The Microsoft session for \"{username}\" has expired. Please sign in again.",
                username = model.reauth_username
            ),

            add_response: ("cancel", &t!("reauth-dialog-cancel","Cancel")),
            add_response: ("login", &t!("reauth-dialog-sign-in","Sign In")),

            #[watch]
            set_response_label: ("cancel",  &t!("reauth-dialog-cancel","Cancel")),
            #[watch]
            set_response_label: ("login", &t!("reauth-dialog-sign-in","Sign In")),

            set_response_appearance: ("login", adw::ResponseAppearance::Suggested),

            set_default_response: Some("login"),
            set_close_response: "cancel",

            connect_response: (Some("login"), {
                let sender = sender.clone();
                move |_, _| {
                    sender.input(MsLoginInput::Start);
                }
            }),
        }

    }

    fn init(
        init: Self::Init,
        root: Self::Root,
        sender: ComponentSender<Self>,
    ) -> ComponentParts<Self> {
        let model = MsLoginDialog {
            parent: init.parent,
            accounts: init.store,
            accounts_path: init.accounts_path,
            client: init.client,
            reauth_dialog: adw::AlertDialog::new(None, None),
            reauth_username: String::new(),
            login_state: MsLoginState::Idle,
            login_abort: None,
        };

        let reauth_dialog = &model.reauth_dialog;

        let widgets = view_output!();

        ComponentParts { model, widgets }
    }

    fn update(&mut self, msg: Self::Input, sender: ComponentSender<Self>, root: &Self::Root) {
        match msg {
            MsLoginInput::PromptReauth { username } => {
                self.reauth_username = username;
                self.reauth_dialog.present(Some(&self.parent));
            }
            MsLoginInput::Start => {
                let Some(client_id) = MICROSOFT_CLIENT_ID else {
                    let _ = sender.output(MsLoginOutput::ShowToast(
                        "Microsoft Client ID is not configured".to_string(),
                    ));
                    return;
                };

                // Cancel the previous task.
                if let Some(handle) = self.login_abort.take() {
                    handle.abort();
                }

                self.login_state = MsLoginState::Starting;
                root.present(Some(&self.parent));

                self.start_ms_login(client_id, &sender);
            }
            MsLoginInput::Cancel => {
                if let Some(handle) = self.login_abort.take() {
                    handle.abort();
                    info!("Microsoft login cancelled by user");
                }
                self.login_state = MsLoginState::Idle;
            }

            MsLoginInput::CopyAuthUrl => {
                if let MsLoginState::WaitingBrowser(url) = &self.login_state
                    && let Some(display) = gtk::gdk::Display::default()
                {
                    display.clipboard().set_text(url);
                    let _ = sender.output(MsLoginOutput::ShowToast(
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
        root: &Self::Root,
    ) {
        match msg {
            MsLoginCmdOutput::OpenBrowser(url) => {
                self.login_state = MsLoginState::WaitingBrowser(url.clone());
                if let Err(err) = gtk::gio::AppInfo::launch_default_for_uri(
                    &url,
                    gtk::gio::AppLaunchContext::NONE,
                ) {
                    let msg = format!("Failed to open browser automatically: {err}");
                    error!("{msg}");
                    let _ = sender.output(MsLoginOutput::ShowToast(msg));
                }
            }
            MsLoginCmdOutput::LoginFinished(res) => {
                self.login_abort = None;
                match res {
                    Ok(login_res) => {
                        if let Err(err) =
                            CredentialStore::set(login_res.account.uuid, &login_res.tokens)
                        {
                            // Failed to save credentials; unable to create the account.
                            error!("{err}");
                            self.login_state = MsLoginState::Failed(err.to_string());
                        } else {
                            self.login_state = MsLoginState::Idle;
                            root.force_close();

                            self.accounts.write().add_account(login_res.account);
                            self.save_accounts(&sender);
                            let _ = sender.output(MsLoginOutput::ShowToast(
                                "Microsoft account added successfully".to_string(),
                            ));
                        }
                    }
                    Err(err) => {
                        error!("{err}");
                        self.login_state = MsLoginState::Failed(err);
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
}
