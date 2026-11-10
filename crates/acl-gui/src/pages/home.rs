// SPDX-FileCopyrightText: 2026 The Adwaita Craft Launcher Authors
// SPDX-License-Identifier: GPL-3.0-or-later

use std::path::Path;
use std::sync::Arc;
use std::time::{Duration, SystemTime};

use acl_auth::microsoft::MicrosoftAuth;
use acl_auth::{Account, AccountStore, AccountType, CredentialStore, MICROSOFT_CLIENT_ID};
use acl_core::launch::LaunchCommand;
use acl_core::manager::InstanceManager;
use acl_core::{LauncherConfig, LauncherDirs, launch};
use acl_minecraft::{AssetIndex, download};
use adw::prelude::*;
use gtk::prelude::*;
use relm4::tokio::time::MissedTickBehavior;
use relm4::{Component, ComponentParts, ComponentSender, tokio};
use relm4::{SharedState, adw};
use thiserror::Error;
use tracing::{error, info, warn};
use uuid::Uuid;

use crate::i18n::LocalizeError;
use crate::t;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum LaunchStage {
    #[default]
    Idle,

    Authenticating,

    LoadingMetadata,

    CheckingJava,

    CheckingIntegrity,

    PreparingRuntime,

    Launching,

    WaitingWindow,
}

impl LaunchStage {
    fn is_launching(&self) -> bool {
        *self != LaunchStage::Idle
    }

    fn fraction(&self) -> f64 {
        match self {
            LaunchStage::Idle => 0.0,
            LaunchStage::Authenticating => 0.1,
            LaunchStage::LoadingMetadata => 0.2,
            LaunchStage::CheckingJava => 0.3,
            LaunchStage::CheckingIntegrity => 0.6,
            LaunchStage::PreparingRuntime => 0.8,
            LaunchStage::Launching => 0.9,
            LaunchStage::WaitingWindow => 0.99,
        }
    }
    fn status_text(&self) -> Option<String> {
        match self {
            LaunchStage::Idle => None,
            LaunchStage::Authenticating => Some(t!(
                "launch-stage-authenticating",
                "Authenticating account..."
            )),
            LaunchStage::LoadingMetadata => Some(t!(
                "launch-stage-loading-metadata",
                "Loading version metadata..."
            )),
            LaunchStage::CheckingJava => {
                Some(t!("launch-stage-checking-java", "Checking Java runtime..."))
            }
            LaunchStage::CheckingIntegrity => Some(t!(
                "launch-stage-checking-integrity",
                "Verifying game files..."
            )),
            LaunchStage::PreparingRuntime => {
                Some(t!("launch-stage-prepare-runtime", "Preparing runtime..."))
            }
            LaunchStage::Launching => Some(t!("launch-stage-launching", "Launching Minecraft...")),
            LaunchStage::WaitingWindow => Some(t!(
                "launch-stage-waiting-window",
                "Waiting for game window..."
            )),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum LaunchError {
    #[error("Microsoft session for \"{username}\" has expired")]
    RefreshTokenExpired { username: String },

    #[error("{0}")]
    Other(String),
}

impl From<String> for LaunchError {
    fn from(msg: String) -> Self {
        Self::Other(msg)
    }
}

#[derive(Debug)]
pub enum HomeOutput {
    ShowToast(String),
    RequestClose,
    PromptReauth(String),
    OpenInstanceDetail(String),
}

#[derive(Debug)]
pub enum HomeInput {
    LaunchClicked,
    /// Trigger a watch update.
    Refresh,
    OpenInstanceSettings,
    SelectInstance(usize),
    SelectAccount(usize),
}

#[derive(Debug)]
pub enum HomeCmdOutput {
    UpdateUsername(Uuid, String),
    StageChanged(LaunchStage),
    LaunchFinished(Result<String, LaunchError>),
    GameExited {
        id: String,
        name: String,
        status: std::io::Result<std::process::ExitStatus>,
    },
}

pub struct HomePage {
    mgr: Arc<SharedState<InstanceManager>>,
    accounts: Arc<SharedState<AccountStore>>,
    config: Arc<SharedState<LauncherConfig>>,
    client: reqwest::Client,
    dirs: LauncherDirs,
    stage: LaunchStage,

    instance_model: gtk::StringList,
    account_model: gtk::StringList,
}

pub struct HomeInit {
    pub mgr: Arc<SharedState<InstanceManager>>,
    pub accounts: Arc<SharedState<AccountStore>>,
    pub config: Arc<SharedState<LauncherConfig>>,
    pub client: reqwest::Client,
    pub dirs: LauncherDirs,
}

impl HomePage {
    /// Check whether the latest log contains "lwjgl version" or "lwjgl openal"
    /// Returning true means the window has already appeared.
    async fn check_window_appear(log_path: &Path, spawn_time: SystemTime) -> bool {
        let is_new_log = tokio::fs::metadata(log_path)
            .await
            .and_then(|m| m.modified())
            .is_ok_and(|mtime| mtime >= spawn_time);

        if !is_new_log {
            return false;
        }

        if let Ok(content) = tokio::fs::read_to_string(log_path).await {
            let content = content.to_lowercase();
            if content.contains("lwjgl version") || content.contains("lwjgl openal") {
                return true;
            }
        }

        false
    }

    async fn wait_window(
        game_dir: &Path,
        spawn_time: SystemTime,
        mut exit_rx: tokio::sync::oneshot::Receiver<()>,
    ) {
        let log_path = game_dir.join("logs").join("latest.log");

        let time_out = tokio::time::sleep(Duration::from_secs(30));
        tokio::pin!(time_out);

        let mut ticker = tokio::time::interval(Duration::from_millis(200));
        ticker.set_missed_tick_behavior(MissedTickBehavior::Skip);
        ticker.tick().await;

        loop {
            tokio::select! {
                _ = &mut time_out => break,
                _ = &mut exit_rx => break, // The process has exited; end the wait immediately!
                _ = ticker.tick() => {
                    if Self::check_window_appear(&log_path, spawn_time).await {
                        break;
                    }

                }
            }
        }
    }

    // Get access_token; refresh if expired and update username
    pub async fn access_token(
        account: &mut Account,
        client: &reqwest::Client,
        client_id: Option<&str>,
    ) -> Result<String, LaunchError> {
        match account.account_type {
            AccountType::Offline => Ok("0".to_string()),
            AccountType::Microsoft => {
                let tokens = CredentialStore::get(account.uuid).map_err(|err| err.to_i18n())?;
                // Return directly if the token has not expired
                if !tokens.is_expired() {
                    return Ok(tokens.access_token);
                }

                let Some(client_id) = client_id else {
                    return Err(t!("launch-error-client-id-empty", "Client ID not set").into());
                };

                let res = MicrosoftAuth::refresh(client, client_id, &tokens.refresh_token)
                    .await
                    .map_err(|err| match err {
                        acl_auth::AuthError::RefreshTokenExpired => {
                            LaunchError::RefreshTokenExpired {
                                username: account.username.clone(),
                            }
                        }
                        other => LaunchError::Other(other.to_i18n()),
                    })?;

                if res.uuid.parse::<Uuid>().ok() != Some(account.uuid) {
                    return Err(t!("launch-error-uuid-mismatch", "UUIDs do not match").into());
                }

                CredentialStore::set(account.uuid, &res.tokens).map_err(|err| err.to_i18n())?;

                // Update username
                if account.username != res.username {
                    account.username = res.username;
                }

                Ok(res.tokens.access_token)
            }
        }
    }

    fn launch(&mut self, sender: ComponentSender<Self>) {
        // Check whether an account is selected.
        let mut account = match self.accounts.read().active_account().cloned() {
            Some(acc) => acc,
            None => {
                let _ = sender.output(HomeOutput::ShowToast(t!(
                    "launch-error-account-not-select",
                    "Please select an account first"
                )));
                return;
            }
        };

        // Check whether an instance is selected.
        let instance = match self.mgr.read().active_instance().cloned() {
            Some(inst) => inst,
            None => {
                let _ = sender.output(HomeOutput::ShowToast(t!(
                    "launch-error-instance-not-select",
                    "Please select an instance first"
                )));
                return;
            }
        };

        let client = self.client.clone();
        let dirs = self.dirs.clone();
        let global_config = self.config.read().clone();

        sender.command(move |out, shutdown| {
            shutdown
                .register(async move {
                    let result: Result<String, LaunchError> = async {
                        // Log in to obtain an access token.
                        let _ = out.send(HomeCmdOutput::StageChanged(LaunchStage::Authenticating));
                        let old_username = account.username.clone();
                        let access_token =
                            Self::access_token(&mut account, &client, MICROSOFT_CLIENT_ID).await?;

                        if old_username != account.username {
                            let _ = out.send(HomeCmdOutput::UpdateUsername(
                                account.uuid,
                                account.username.clone(),
                            ));
                        }

                        // Load local version metadata.
                        let _ = out.send(HomeCmdOutput::StageChanged(LaunchStage::LoadingMetadata));
                        let metadata = instance.load_metadata().map_err(|e| {
                            t!(
                                "launch-error-metadata",
                                "Failed to load version metadata: {e}",
                                e = e
                            )
                        })?;

                        // Validate and select a matching Java runtime.
                        let _ = out.send(HomeCmdOutput::StageChanged(LaunchStage::CheckingJava));
                        let java = launch::select_java(&instance, &global_config, &metadata)
                            .map_err(|e| t!("launch-error-java", "Java error: {e}", e = e))?;

                        // Validate and concurrently compare all files (client.jar, libraries, assets).
                        let _ =
                            out.send(HomeCmdOutput::StageChanged(LaunchStage::CheckingIntegrity));
                        let asset_index = AssetIndex::fetch_and_verify(
                            &client,
                            &metadata.asset_index,
                            &dirs.assets_dir(),
                            global_config.download_source,
                        )
                        .await
                        .map_err(|e| {
                            t!(
                                "launch-error-asset-index",
                                "Failed to verify asset index: {e}",
                                e = e
                            )
                        })?;

                        let tasks = instance.download_tasks(&metadata, &asset_index, &dirs);
                        let tasks = instance.check_integrity(tasks);
                        download::download_all(
                            &client,
                            tasks,
                            global_config.download_source,
                            global_config.max_concurrent_downloads,
                            None,
                        )
                        .await
                        .map_err(|e| {
                            t!(
                                "launch-error-resources",
                                "Failed to verify resources: {e}",
                                e = e
                            )
                        })?;

                        // Extract native libraries and prepare legacy asset mappings.
                        let _ =
                            out.send(HomeCmdOutput::StageChanged(LaunchStage::PreparingRuntime));
                        let inst_clone = instance.clone();
                        let dirs_clone = dirs.clone();
                        let metadata_clone = metadata.clone();
                        tokio::task::spawn_blocking(move || {
                            inst_clone.prepare_runtime(&metadata_clone, &asset_index, &dirs_clone)
                        })
                        .await
                        .map_err(|e| {
                            t!(
                                "launch-error-runtime-execution",
                                "Runtime task execution error: {e}",
                                e = e
                            )
                        })?
                        .map_err(|e| {
                            t!(
                                "launch-error-prepare-runtime",
                                "Failed to prepare runtime: {e}",
                                e = e
                            )
                        })?;

                        // Assemble the launch command and start the game process.
                        let _ = out.send(HomeCmdOutput::StageChanged(LaunchStage::Launching));
                        let cmd = LaunchCommand::build(
                            &instance,
                            &global_config,
                            &dirs,
                            &metadata,
                            &account,
                            &access_token,
                            java.path,
                        )
                        .map_err(|e| {
                            t!(
                                "launch-error-build-command",
                                "Failed to build launch command: {e}",
                                e = e
                            )
                        })?;

                        let spawn_time = SystemTime::now();

                        let process = cmd.spawn().await.map_err(|e| {
                            t!(
                                "launch-error-spawn-process",
                                "Failed to spawn game process: {e}",
                                e = e
                            )
                        })?;

                        // A background standalone task monitors the process exit.
                        let inst_name = instance.config.name.clone();
                        let inst_id = instance.id().to_owned();
                        let out_bg = out.clone();

                        let (exit_tx, exit_rx) = tokio::sync::oneshot::channel::<()>();

                        tokio::spawn(async move {
                            let status = process.wait().await;
                            let _ = exit_tx.send(());
                            let _ = out_bg.send(HomeCmdOutput::GameExited {
                                id: inst_id,
                                name: inst_name,
                                status,
                            });
                        });

                        // Wait for the game window to appear.
                        let _ = out.send(HomeCmdOutput::StageChanged(LaunchStage::WaitingWindow));
                        Self::wait_window(instance.game_dir(), spawn_time, exit_rx).await;

                        Ok(instance.id().to_string())
                    }
                    .await;

                    let _ = out.send(HomeCmdOutput::LaunchFinished(result));
                })
                .drop_on_shutdown()
        });
    }

    /// Top title: displays the name of the currently selected instance; displays a hint when there is no instance.
    fn status_title(&self) -> String {
        let mgr = self.mgr.read();
        match mgr.active_instance() {
            Some(inst) => inst.config.name.clone(),
            None => t!("home-page-no-instance", "No instance selected"),
        }
    }

    /// Top subtitle: displays e.g. "Minecraft 1.16.5 · Fabric"; returns None when there is no instance.
    fn status_subtitle(&self) -> Option<String> {
        let mgr = self.mgr.read();
        mgr.active_instance().map(|inst| {
            format!(
                "Minecraft {} · {}",
                inst.config.game_version,
                inst.config.mod_loader.label()
            )
        })
    }

    fn refresh_models(&mut self) {
        let mgr = self.mgr.read();
        let inst_names: Vec<&str> = mgr
            .instances
            .iter()
            .map(|i| i.config.name.as_str())
            .collect();
        self.instance_model
            .splice(0, self.instance_model.n_items(), &inst_names);
        drop(mgr);

        let accounts = self.accounts.read();
        let acc_labels: Vec<String> = accounts
            .accounts()
            .iter()
            .map(|acc| {
                let type_label = match acc.account_type {
                    AccountType::Offline => t!("account-type-offline", "Offline"),
                    AccountType::Microsoft => t!("account-type-microsoft", "Microsoft"),
                };
                format!("{} · {}", acc.username, type_label)
            })
            .collect();

        let acc_refs: Vec<&str> = acc_labels.iter().map(|s| s.as_str()).collect();
        self.account_model
            .splice(0, self.account_model.n_items(), &acc_refs);
    }

    fn active_instance_index(&self) -> u32 {
        let mgr = self.mgr.read();
        mgr.config
            .active_id
            .as_deref()
            .and_then(|id| mgr.instances.iter().position(|i| i.id() == id))
            .map(|pos| pos as u32)
            .unwrap_or(gtk::INVALID_LIST_POSITION)
    }

    fn active_account_index(&self) -> u32 {
        let accounts = self.accounts.read();
        accounts
            .active_uuid()
            .and_then(|uuid| accounts.accounts().iter().position(|a| a.uuid == uuid))
            .map(|pos| pos as u32)
            .unwrap_or(gtk::INVALID_LIST_POSITION)
    }
}

#[relm4::component(pub)]
impl Component for HomePage {
    type Init = HomeInit;
    type Input = HomeInput;
    type Output = HomeOutput;
    type CommandOutput = HomeCmdOutput;

    view! {
        adw::StatusPage {
            set_vexpand: true,
            // Display compactly to prevent icons from becoming too large.
            add_css_class: "compact",
            set_icon_name: Some("input-gaming-symbolic"),

            #[wrap(Some)]
            set_child = &adw::Clamp {
                set_maximum_size: 420,
                set_tightening_threshold: 360,

                gtk::Box {

                    set_orientation: gtk::Orientation::Vertical,
                    set_spacing: 24,

                    gtk::Box {
                        set_orientation: gtk::Orientation::Vertical,
                        set_spacing: 6,

                        gtk::Label {
                            add_css_class: "title-1",
                            #[watch]
                            set_label: &model.status_title(),
                        },

                        gtk::Label {
                            add_css_class: "dim-label",
                            #[watch]
                            set_visible: model.mgr.read().active_instance().is_some(),
                            #[watch]
                            set_label: &model.status_subtitle().unwrap_or_default(),
                        },
                    },

                    adw::PreferencesGroup {
                        #[watch]
                        set_sensitive: !model.stage.is_launching(),

                        add = &adw::ActionRow {
                            #[watch]
                            set_title: &t!("navigation-page-instances", "Instances"),

                            add_prefix = &gtk::Image {
                                set_icon_name: Some("view-grid-symbolic"),
                            },


                            add_suffix = &gtk::Button {
                                set_valign: gtk::Align::Center,
                                add_css_class: "flat",
                                set_icon_name: "emblem-system-symbolic",
                                #[watch]
                                set_tooltip_text: Some(&t!("home-page-tooltip-instance-settings", "Instance settings")),
                                #[watch]
                                set_sensitive: model.mgr.read().active_instance().is_some(),
                                connect_clicked => HomeInput::OpenInstanceSettings,
                            },

                            add_suffix = &gtk::DropDown {
                                add_css_class: "flat",
                                add_css_class: "home-selector",
                                set_valign: gtk::Align::Center,
                                set_model: Some(&instance_model),
                                #[watch]
                                set_selected: model.active_instance_index(),
                                #[watch]
                                set_sensitive: !model.mgr.read().instances.is_empty(),

                                connect_selected_notify[sender] => move |dropdown| {
                                    let idx = dropdown.selected();
                                    if idx != gtk::INVALID_LIST_POSITION {
                                        sender.input(HomeInput::SelectInstance(idx as usize));
                                    }
                                },
                            },

                        },

                        add = &adw::ActionRow {
                            #[watch]
                            set_title: &t!("navigation-page-accounts", "Accounts"),

                            add_prefix = &gtk::Image {
                                set_icon_name: Some("avatar-default-symbolic"),
                            },

                            add_suffix = &gtk::DropDown {
                                add_css_class: "flat",
                                add_css_class: "home-selector",
                                set_valign: gtk::Align::Center,
                                set_model: Some(&account_model),
                                #[watch]
                                set_selected: model.active_account_index(),
                                #[watch]
                                set_sensitive: !model.accounts.read().accounts().is_empty(),

                                connect_selected_notify[sender] => move |dropdown| {
                                    let idx = dropdown.selected();
                                    if idx != gtk::INVALID_LIST_POSITION {
                                        sender.input(HomeInput::SelectAccount(idx as usize));
                                    }
                                },
                            },

                        },


                    },

                    // Use a Stack so that the button and the progress card switch in place at the same position.
                    gtk::Stack {
                        // Enable vertical equal-height: force the Stack's height to always be locked to the height of the tallest child.
                        set_vhomogeneous: true,
                        set_transition_type: gtk::StackTransitionType::Crossfade,
                        set_transition_duration: 150,

                        add_named[Some("button")] = &gtk::Box {
                            set_halign: gtk::Align::Center,
                            set_valign: gtk::Align::Center,

                            set_margin_top: 8,
                            set_margin_bottom: 8,

                            gtk::Button {
                                set_halign: gtk::Align::Center,
                                add_css_class: "suggested-action",
                                add_css_class: "pill",
                                connect_clicked => HomeInput::LaunchClicked,

                                gtk::Box {
                                    set_spacing: 8,
                                    set_halign: gtk::Align::Center,

                                    gtk::Image {
                                        set_icon_name: Some("media-playback-start-symbolic"),
                                    },
                                    gtk::Label {
                                        #[watch]
                                        set_label: &t!("home-page-button-start-game", "Start Game"),
                                    },
                                },
                            },
                        },

                        add_named[Some("progress")] =& gtk::Box {
                            set_orientation: gtk::Orientation::Vertical,
                            set_spacing: 8,
                            add_css_class: "card",
                            gtk::Box {
                                set_orientation: gtk::Orientation::Vertical,
                                set_spacing: 8,
                                set_margin_top: 14,
                                set_margin_bottom: 14,
                                set_margin_start: 16,
                                set_margin_end: 16,

                                gtk::Box {
                                    set_orientation: gtk::Orientation::Horizontal,
                                    set_spacing: 8,

                                    gtk::Label {
                                        set_halign: gtk::Align::Start,
                                        set_hexpand: true,
                                        #[watch]
                                        set_label: model.stage.status_text().as_deref().unwrap_or_default(),
                                    },

                                    gtk::Label {
                                        set_halign: gtk::Align::End,
                                        add_css_class: "dim-label",
                                        add_css_class: "numeric",
                                        #[watch]
                                        set_label: &format!("{:.0}%", model.stage.fraction() * 100.0),
                                    },

                                },

                                gtk::ProgressBar {
                                    set_hexpand: true,
                                    #[watch]
                                    set_fraction: model.stage.fraction(),
                                },
                            }
                        },

                        #[watch]
                        set_visible_child_name: if model.stage.is_launching() { "progress" } else { "button" },


                    },
                },
            },
        }
    }

    fn init(
        init: Self::Init,
        root: Self::Root,
        sender: ComponentSender<Self>,
    ) -> ComponentParts<Self> {
        // Subscribe to update events.
        init.mgr
            .subscribe(sender.input_sender(), |_| HomeInput::Refresh);
        init.accounts
            .subscribe(sender.input_sender(), |_| HomeInput::Refresh);
        init.config
            .subscribe(sender.input_sender(), |_| HomeInput::Refresh);

        let instance_model = gtk::StringList::new(&[]);
        let account_model = gtk::StringList::new(&[]);

        let mut model = HomePage {
            mgr: init.mgr,
            accounts: init.accounts,
            config: init.config,
            client: init.client,
            dirs: init.dirs,
            stage: LaunchStage::Idle,
            instance_model: instance_model.clone(),
            account_model: account_model.clone(),
        };

        model.refresh_models();

        let widgets = view_output!();
        ComponentParts { model, widgets }
    }

    fn update(&mut self, msg: Self::Input, sender: ComponentSender<Self>, _root: &Self::Root) {
        match msg {
            HomeInput::LaunchClicked => {
                self.launch(sender);
            }
            HomeInput::Refresh => {
                self.refresh_models();
            }
            HomeInput::OpenInstanceSettings => {
                if let Some(ins) = self.mgr.read().active_instance() {
                    let _ = sender.output(HomeOutput::OpenInstanceDetail(ins.id().to_string()));
                } else {
                    let _ = sender.output(HomeOutput::ShowToast(t!(
                        "home-page-error-empty-instance",
                        "Please select an instance"
                    )));
                }
            }
            HomeInput::SelectInstance(idx) => {
                let mut mgr = self.mgr.write();
                if let Some(id) = mgr.instances.get(idx).map(|i| i.id().to_owned())
                    && mgr.config.active_id.as_deref() != Some(id.as_str())
                {
                    if let Err(err) = mgr.set_active_instance(&id) {
                        error!("Failed to set active instance: {err}");
                        return;
                    }
                    if let Err(err) = mgr.save() {
                        error!("Failed to save instances config: {err}");
                    }
                }
            }
            HomeInput::SelectAccount(idx) => {
                let mut accounts = self.accounts.write();
                if let Some(uuid) = accounts.accounts().get(idx).map(|a| a.uuid)
                    && accounts.active_uuid() != Some(uuid)
                {
                    let _ = accounts.set_active_account(uuid);
                    if let Err(err) = accounts.save(&self.dirs.accounts_file()) {
                        error!("Failed to save accounts: {err}");
                    }
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
            HomeCmdOutput::StageChanged(stage) => self.stage = stage,
            HomeCmdOutput::LaunchFinished(result) => {
                self.stage = LaunchStage::Idle;
                match result {
                    Err(err) => match err {
                        LaunchError::RefreshTokenExpired { username } => {
                            let _ = sender.output(HomeOutput::PromptReauth(username));
                        }
                        LaunchError::Other(msg) => {
                            error!("Failed to launch game: {msg}");
                            let _ = sender.output(HomeOutput::ShowToast(msg));
                        }
                    },
                    Ok(id) => {
                        if let Some(ins) = self
                            .mgr
                            .write()
                            .instances
                            .iter_mut()
                            .find(|ins| ins.id() == id.as_str())
                            && let Err(err) = ins.update_last_play()
                        {
                            warn!("Failed to update instance last play time: {err}");
                        }

                        // Enable offline account creation after launching with an official Microsoft account.
                        if let Some(account) = self.accounts.read().active_account()
                            && matches!(account.account_type, AccountType::Microsoft)
                        {
                            let enable = self.config.read().enable_offline;

                            if !enable {
                                self.config.write().enable_offline = true;
                                if let Err(err) = self.config.read().save(&self.dirs.config_file())
                                {
                                    error!("Failed to save configuration: {err}");
                                    let msg = t!(
                                        "launch-error-save-configuration",
                                        "Failed to save configuration: {err}",
                                        err = err
                                    );

                                    let _ = sender.output(HomeOutput::ShowToast(msg));
                                }
                            }
                        }

                        // Only exit the launcher when the game starts successfully.
                        if self.config.read().close_launcher_on_launch {
                            let _ = sender.output(HomeOutput::RequestClose);
                        }
                    }
                }
            }
            HomeCmdOutput::GameExited { id, name, status } => match status {
                Ok(s) => {
                    if !s.success() {
                        if let Some(ins) = self
                            .mgr
                            .write()
                            .instances
                            .iter_mut()
                            .find(|ins| ins.id() == id.as_str())
                        {
                            ins.mark_game_abnormal();
                        }
                        error!("Instance \"{name}\" exited with code {:?}", s.code());
                        let msg = t!(
                            "launch-error-instance-exit-abnormal",
                            "Instance \"{name}\" exited with code {code}",
                            name = name,
                            code = s.code().unwrap_or_default()
                        );

                        let _ = sender.output(HomeOutput::ShowToast(msg));
                    }
                }
                Err(err) => {
                    if let Some(ins) = self
                        .mgr
                        .write()
                        .instances
                        .iter_mut()
                        .find(|ins| ins.id() == id.as_str())
                    {
                        ins.mark_game_abnormal();
                    }

                    error!("Error waiting for instance \"{name}\": {err}");
                    let msg = t!(
                        "launch-error-wait-instance",
                        "Error waiting for instance \"{name}\": {err}",
                        name = name,
                        err = err
                    );
                    let _ = sender.output(HomeOutput::ShowToast(msg));
                }
            },
            HomeCmdOutput::UpdateUsername(uuid, username) => {
                let mut accounts = self.accounts.write();

                if let Some(account) = accounts.account_mut(uuid)
                    && account.username != username
                {
                    info!("Update username {} to {}", account.username, username);
                    account.username = username;
                    if let Err(err) = accounts.save(&self.dirs.accounts_file()) {
                        error!("Failed to save accounts: {err}");
                        let msg = t!(
                            "home-page-error-save-accounts",
                            "Failed to save accounts: {err}",
                            err = err
                        );

                        let _ = sender.output(HomeOutput::ShowToast(msg));
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
    fn test_launch_stage_default_and_is_launching() {
        assert_eq!(LaunchStage::default(), LaunchStage::Idle);
        assert!(!LaunchStage::Idle.is_launching());

        let active_stages = [
            LaunchStage::Authenticating,
            LaunchStage::LoadingMetadata,
            LaunchStage::CheckingJava,
            LaunchStage::CheckingIntegrity,
            LaunchStage::PreparingRuntime,
            LaunchStage::Launching,
            LaunchStage::WaitingWindow,
        ];

        for stage in active_stages {
            assert!(stage.is_launching(), "{stage:?} should be launching");
        }
    }

    #[test]
    fn test_launch_stage_fraction() {
        assert_eq!(LaunchStage::Idle.fraction(), 0.0);
        assert!((LaunchStage::Authenticating.fraction() - 0.1).abs() < f64::EPSILON);
        assert!((LaunchStage::LoadingMetadata.fraction() - 0.2).abs() < f64::EPSILON);
        assert!((LaunchStage::CheckingJava.fraction() - 0.3).abs() < f64::EPSILON);
        assert!((LaunchStage::CheckingIntegrity.fraction() - 0.6).abs() < f64::EPSILON);
        assert!((LaunchStage::PreparingRuntime.fraction() - 0.8).abs() < f64::EPSILON);
        assert!((LaunchStage::Launching.fraction() - 0.9).abs() < f64::EPSILON);
        assert!((LaunchStage::WaitingWindow.fraction() - 0.99).abs() < f64::EPSILON);
    }

    #[test]
    fn test_launch_stage_status_text() {
        assert_eq!(LaunchStage::Idle.status_text(), None);
        assert_eq!(
            LaunchStage::Authenticating.status_text(),
            Some("Authenticating account...".to_string())
        );
        assert_eq!(
            LaunchStage::LoadingMetadata.status_text(),
            Some("Loading version metadata...".to_string())
        );
        assert_eq!(
            LaunchStage::CheckingJava.status_text(),
            Some("Checking Java runtime...".to_string())
        );
        assert_eq!(
            LaunchStage::CheckingIntegrity.status_text(),
            Some("Verifying game files...".to_string())
        );
        assert_eq!(
            LaunchStage::PreparingRuntime.status_text(),
            Some("Preparing runtime...".to_string())
        );
        assert_eq!(
            LaunchStage::Launching.status_text(),
            Some("Launching Minecraft...".to_string())
        );
        assert_eq!(
            LaunchStage::WaitingWindow.status_text(),
            Some("Waiting for game window...".to_string())
        );
    }

    #[test]
    fn test_launch_error_display() {
        let expired = LaunchError::RefreshTokenExpired {
            username: "Steve".to_string(),
        };
        assert_eq!(
            expired.to_string(),
            "Microsoft session for \"Steve\" has expired"
        );

        let other = LaunchError::Other("Network failure".to_string());
        assert_eq!(other.to_string(), "Network failure");
    }

    #[test]
    fn test_launch_error_from_string() {
        let err: LaunchError = "Custom error message".to_string().into();
        assert_eq!(err, LaunchError::Other("Custom error message".to_string()));
    }

    #[test]
    fn test_launch_error_traits() {
        let err1 = LaunchError::RefreshTokenExpired {
            username: "Alex".to_string(),
        };
        let err2 = err1.clone();
        assert_eq!(err1, err2);
        assert_ne!(
            err1,
            LaunchError::RefreshTokenExpired {
                username: "Steve".to_string(),
            }
        );
        assert!(format!("{err1:?}").contains("RefreshTokenExpired"));
    }

    #[test]
    fn test_home_input_variants_and_debug() {
        let input_instance = HomeInput::SelectInstance(42);
        assert_eq!(format!("{input_instance:?}"), "SelectInstance(42)");
        match input_instance {
            HomeInput::SelectInstance(idx) => assert_eq!(idx, 42),
            _ => panic!("Expected SelectInstance"),
        }

        let input_account = HomeInput::SelectAccount(7);
        assert_eq!(format!("{input_account:?}"), "SelectAccount(7)");
        match input_account {
            HomeInput::SelectAccount(idx) => assert_eq!(idx, 7),
            _ => panic!("Expected SelectAccount"),
        }

        assert_eq!(format!("{:?}", HomeInput::LaunchClicked), "LaunchClicked");
        assert_eq!(format!("{:?}", HomeInput::Refresh), "Refresh");
        assert_eq!(
            format!("{:?}", HomeInput::OpenInstanceSettings),
            "OpenInstanceSettings"
        );
    }

    #[test]
    fn test_home_output_debug() {
        let toast = HomeOutput::ShowToast("hello".to_string());
        assert_eq!(format!("{toast:?}"), "ShowToast(\"hello\")");

        let detail = HomeOutput::OpenInstanceDetail("inst_1".to_string());
        assert_eq!(format!("{detail:?}"), "OpenInstanceDetail(\"inst_1\")");

        let reauth = HomeOutput::PromptReauth("Steve".to_string());
        assert_eq!(format!("{reauth:?}"), "PromptReauth(\"Steve\")");

        let close = HomeOutput::RequestClose;
        assert_eq!(format!("{close:?}"), "RequestClose");
    }

    #[test]
    fn test_active_instance_index_calculation() {
        let compute =
            |instances: &[acl_core::instance::Instance], active_id: Option<&str>| -> u32 {
                active_id
                    .and_then(|id| instances.iter().position(|i| i.id() == id))
                    .map(|pos| pos as u32)
                    .unwrap_or(gtk::INVALID_LIST_POSITION)
            };

        let inst1 = acl_core::instance::Instance::new(
            "inst_1",
            "Instance One",
            "1.20.4",
            Path::new("/mock/instances"),
        )
        .unwrap();

        let inst2 = acl_core::instance::Instance::new(
            "inst_2",
            "Instance Two",
            "1.21.1",
            Path::new("/mock/instances"),
        )
        .unwrap();

        let instances = vec![inst1, inst2];

        // When active_id is None
        assert_eq!(compute(&instances, None), gtk::INVALID_LIST_POSITION);

        // When active_id matches first instance
        assert_eq!(compute(&instances, Some("inst_1")), 0);

        // When active_id matches second instance
        assert_eq!(compute(&instances, Some("inst_2")), 1);

        // When active_id does not match any instance
        assert_eq!(
            compute(&instances, Some("inst_3")),
            gtk::INVALID_LIST_POSITION
        );
    }

    #[test]
    fn test_active_account_index_calculation() {
        let acc1 = Account::new_offline("Steve").unwrap();
        let acc2 = Account::new_offline("Alex").unwrap();
        let accounts = vec![acc1.clone(), acc2.clone()];

        let compute = |list: &[Account], active: Option<Uuid>| -> u32 {
            active
                .and_then(|uuid| list.iter().position(|a| a.uuid == uuid))
                .map(|pos| pos as u32)
                .unwrap_or(gtk::INVALID_LIST_POSITION)
        };

        // When active UUID is None
        assert_eq!(compute(&accounts, None), gtk::INVALID_LIST_POSITION);

        // When active UUID matches first account
        assert_eq!(compute(&accounts, Some(acc1.uuid)), 0);

        // When active UUID matches second account
        assert_eq!(compute(&accounts, Some(acc2.uuid)), 1);

        // When active UUID is not in the list
        assert_eq!(
            compute(&accounts, Some(Uuid::new_v4())),
            gtk::INVALID_LIST_POSITION
        );
    }

    #[test]
    fn test_home_status_title_and_subtitle() {
        let dummy_dir = Path::new("/mock/instances");
        let inst =
            acl_core::instance::Instance::new("test_id", "My Test Instance", "1.21.1", dummy_dir)
                .unwrap();

        // When no active instance
        let no_inst: Option<&acl_core::instance::Instance> = None;
        let title_none = match no_inst {
            Some(i) => i.config.name.clone(),
            None => t!("home-page-no-instance", "No instance selected"),
        };
        assert_eq!(title_none, "No instance selected");

        let subtitle_none = no_inst.map(|i| {
            format!(
                "Minecraft {} · {}",
                i.config.game_version,
                i.config.mod_loader.label()
            )
        });
        assert_eq!(subtitle_none, None);

        // When active instance is present
        let some_inst = Some(&inst);
        let title_some = match some_inst {
            Some(i) => i.config.name.clone(),
            None => t!("home-page-no-instance", "No instance selected"),
        };
        assert_eq!(title_some, "My Test Instance");

        let subtitle_some = some_inst.map(|i| {
            format!(
                "Minecraft {} · {}",
                i.config.game_version,
                i.config.mod_loader.label()
            )
        });
        assert_eq!(
            subtitle_some,
            Some("Minecraft 1.21.1 · Vanilla".to_string())
        );
    }

    #[test]
    fn test_home_page_model_methods_if_gtk_available() {
        use std::path::PathBuf;

        if gtk::init().is_err() {
            return;
        }

        let dummy_dir = Path::new("/mock/instances");
        let inst =
            acl_core::instance::Instance::new("inst_1", "Fabric Instance", "1.20.1", dummy_dir)
                .unwrap();

        let mut mgr = InstanceManager::default();
        mgr.instances.push(inst);
        mgr.config.active_id = Some("inst_1".to_string());

        let mut store = AccountStore::new();
        let acc = Account::new_offline("Steve").unwrap();
        store.add_account(acc);

        let mgr_state = Arc::new(SharedState::new());
        *mgr_state.write() = mgr;

        let accounts_state = Arc::new(SharedState::new());
        *accounts_state.write() = store;

        let dirs = LauncherDirs::with_paths(
            PathBuf::from("/mock/config"),
            PathBuf::from("/mock/data"),
            PathBuf::from("/mock/cache"),
        );

        let page = HomePage {
            mgr: mgr_state,
            accounts: accounts_state,
            config: Arc::new(SharedState::new()),
            client: reqwest::Client::new(),
            dirs,
            stage: LaunchStage::Idle,
            instance_model: gtk::StringList::new(&[]),
            account_model: gtk::StringList::new(&[]),
        };

        assert_eq!(page.active_instance_index(), 0);
        assert_eq!(page.active_account_index(), 0);
        assert_eq!(page.status_title(), "Fabric Instance");
        assert_eq!(
            page.status_subtitle(),
            Some("Minecraft 1.20.1 · Vanilla".to_string())
        );
    }
}
