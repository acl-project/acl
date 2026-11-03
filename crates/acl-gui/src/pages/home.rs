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
use gtk::prelude::*;
use relm4::tokio::time::MissedTickBehavior;
use relm4::{Component, ComponentParts, ComponentSender, RelmWidgetExt, tokio};
use relm4::{SharedState, adw};
use tracing::{error, info, warn};
use uuid::Uuid;

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
            LaunchStage::Authenticating => Some("Authenticating account...".to_string()),
            LaunchStage::LoadingMetadata => Some("Loading version metadata...".to_string()),
            LaunchStage::CheckingJava => Some("Checking Java runtime...".to_string()),
            LaunchStage::CheckingIntegrity => Some("Verifying game files...".to_string()),
            LaunchStage::PreparingRuntime => Some("Preparing runtime...".to_string()),
            LaunchStage::Launching => Some("Launching Minecraft...".to_string()),
            LaunchStage::WaitingWindow => Some("Waiting for game window...".to_string()),
        }
    }
}

#[derive(Debug)]
pub enum HomeOutput {
    ShowToast(String),
    RequestClose,
}

#[derive(Debug)]
pub enum HomeInput {
    LaunchClicked,
    /// Trigger a watch update.
    Refresh,
}

#[derive(Debug)]
pub enum HomeCmdOutput {
    UpdateUsername(Uuid, String),
    StageChanged(LaunchStage),
    LaunchFinished(Result<String, String>),
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
}

pub struct HomeInit {
    pub mgr: Arc<SharedState<InstanceManager>>,
    pub accounts: Arc<SharedState<AccountStore>>,
    pub config: Arc<SharedState<LauncherConfig>>,
    pub client: reqwest::Client,
    pub dirs: LauncherDirs,
}

impl HomePage {
    fn status_description(&self) -> String {
        let mgr = self.mgr.read();
        let inst_text = match mgr.active_instance() {
            Some(inst) => format!("{} ({})", inst.config.name, inst.config.game_version),
            None => "No instance selected".to_string(),
        };

        let accounts = self.accounts.read();
        let acc_text = match accounts.active_account() {
            Some(acc) => acc.username.clone(),
            None => "No account selected".to_string(),
        };

        format!("Instance: {inst_text} • Account: {acc_text}")
    }
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
    ) -> Result<String, String> {
        match account.account_type {
            AccountType::Offline => Ok("0".to_string()),
            AccountType::Microsoft => {
                let tokens = CredentialStore::get(account.uuid).map_err(|err| err.to_string())?;
                // Return directly if the token has not expired
                if !tokens.is_expired() {
                    return Ok(tokens.access_token);
                }

                let Some(client_id) = client_id else {
                    return Err("Client ID not set".to_string());
                };

                let res = MicrosoftAuth::refresh(client, client_id, &tokens.refresh_token)
                    .await
                    .map_err(|err| err.to_string())?;

                if res.uuid.parse::<Uuid>().ok() != Some(account.uuid) {
                    return Err("UUIDs do not match".to_string());
                }

                CredentialStore::set(account.uuid, &res.tokens).map_err(|err| err.to_string())?;

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
                let _ = sender.output(HomeOutput::ShowToast(
                    "Please select an account first".to_string(),
                ));
                return;
            }
        };

        // Check whether an instance is selected.
        let instance = match self.mgr.read().active_instance().cloned() {
            Some(inst) => inst,
            None => {
                let _ = sender.output(HomeOutput::ShowToast(
                    "Please select an instance first".to_string(),
                ));
                return;
            }
        };

        let client = self.client.clone();
        let dirs = self.dirs.clone();
        let global_config = self.config.read().clone();

        sender.command(move |out, shutdown| {
            shutdown
                .register(async move {
                    let result: Result<String, String> = async {
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
                        let metadata = instance
                            .load_metadata()
                            .map_err(|e| format!("Failed to load version metadata: {e}"))?;

                        // Validate and select a matching Java runtime.
                        let _ = out.send(HomeCmdOutput::StageChanged(LaunchStage::CheckingJava));
                        let java = launch::select_java(&instance, &global_config, &metadata)
                            .map_err(|e| format!("Java error: {e}"))?;

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
                        .map_err(|e| format!("Failed to verify asset index: {e}"))?;

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
                        .map_err(|e| format!("Failed to verify resources: {e}"))?;

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
                        .map_err(|e| format!("Runtime task execution error: {e}"))?
                        .map_err(|e| format!("Failed to prepare runtime: {e}"))?;

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
                        .map_err(|e| format!("Failed to build launch command: {e}"))?;

                        let spawn_time = SystemTime::now();

                        let process = cmd
                            .spawn()
                            .await
                            .map_err(|e| format!("Failed to spawn game process: {e}"))?;

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
}

#[relm4::component(pub)]
impl Component for HomePage {
    type Init = HomeInit;
    type Input = HomeInput;
    type Output = HomeOutput;
    type CommandOutput = HomeCmdOutput;

    view! {
        gtk::Box {
            set_orientation: gtk::Orientation::Vertical,
            set_margin_all: 24,

            adw::StatusPage {
                set_vexpand: true,
                set_icon_name: Some("applications-games-symbolic"),
                set_title: "Adwaita Craft Launcher",
                #[watch]
                set_description: Some(&model.status_description()),
            },

            gtk::Box {
                set_orientation: gtk::Orientation::Horizontal,
                set_spacing: 16,
                set_valign: gtk::Align::Center,


                gtk::Revealer {
                    set_transition_type: gtk::RevealerTransitionType::Crossfade,
                    set_transition_duration: 200,
                    set_hexpand: true,
                    set_valign: gtk::Align::Center,
                    #[watch]
                    set_reveal_child: model.stage.is_launching(),

                    gtk::Box {
                        set_orientation: gtk::Orientation::Vertical,
                        set_spacing: 6,
                        set_valign: gtk::Align::Center,

                        gtk::Box {
                            set_orientation: gtk::Orientation::Horizontal,
                            set_spacing: 8,

                            gtk::Label {
                                set_halign: gtk::Align::Start,
                                set_hexpand: true,
                                add_css_class: "caption",
                                add_css_class: "dim-label",
                                #[watch]
                                set_label: model.stage.status_text().as_deref().unwrap_or_default(),
                            },

                            gtk::Label {
                                set_halign: gtk::Align::End,
                                add_css_class: "caption",
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
                    },
                },

                gtk::Button {
                    set_halign: gtk::Align::End,
                    set_valign: gtk::Align::Center,
                    add_css_class: "suggested-action",
                    add_css_class: "pill",
                    #[watch]
                    set_sensitive: !model.stage.is_launching(),
                    connect_clicked => HomeInput::LaunchClicked,

                    gtk::Box {
                        set_spacing: 8,
                        set_halign: gtk::Align::Center,

                        adw::Spinner {
                            #[watch]
                            set_visible: model.stage.is_launching(),
                        },
                        gtk::Image {
                            set_icon_name: Some("media-playback-start-symbolic"),
                            #[watch]
                            set_visible: !model.stage.is_launching(),
                        },
                        gtk::Label {
                            #[watch]
                            set_label: if model.stage.is_launching() {
                                "Launching..."
                            } else {
                                "Start Game"
                            },
                        },
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

        let model = HomePage {
            mgr: init.mgr,
            accounts: init.accounts,
            config: init.config,
            client: init.client,
            dirs: init.dirs,
            stage: LaunchStage::Idle,
        };
        let widgets = view_output!();
        ComponentParts { model, widgets }
    }

    fn update(&mut self, msg: Self::Input, sender: ComponentSender<Self>, _root: &Self::Root) {
        match msg {
            HomeInput::LaunchClicked => {
                self.launch(sender);
            }
            HomeInput::Refresh => {}
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
                    Err(err) => {
                        error!("Failed to launch game: {err}");
                        let _ = sender.output(HomeOutput::ShowToast(err));
                    }
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
                                    let msg = format!("Failed to save configuration: {err}");
                                    error!("{msg}");
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
                        let msg = format!("Instance \"{name}\" exited with code {:?}", s.code());
                        error!("{msg}");
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
                    let msg = format!("Error waiting for instance \"{name}\": {err}");
                    error!("{msg}");
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
                        let msg = format!("Failed to save accounts: {err}");
                        error!("{msg}");
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
}
