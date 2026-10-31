use std::sync::Arc;

use acl_auth::AccountStore;
use acl_core::launch::LaunchCommand;
use acl_core::manager::InstanceManager;
use acl_core::{LauncherConfig, LauncherDirs, launch};
use acl_minecraft::{AssetIndex, download};
use gtk::prelude::*;
use relm4::{Component, ComponentParts, ComponentSender, RelmWidgetExt, tokio};
use relm4::{SharedState, adw};

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
            LaunchStage::PreparingRuntime => 0.85,
            LaunchStage::Launching => 0.95,
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
        }
    }
}

#[derive(Debug)]
pub enum HomeOutput {
    ShowToast(String),
}

#[derive(Debug)]
pub enum HomeInput {
    LaunchClicked,
}

#[derive(Debug)]
pub enum HomeCmdOutput {
    StageChanged(LaunchStage),
    LaunchFinished(Result<(), String>),
    GameExited {
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
    fn launch(&mut self, sender: ComponentSender<Self>) {
        // Check whether an account is selected.
        let account = match self.accounts.read().active_account().cloned() {
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
                    let result: Result<(), String> = async {
                        // Log in to obtain an access token (not yet implemented).
                        let _ = out.send(HomeCmdOutput::StageChanged(LaunchStage::Authenticating));

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
                        )
                        .await
                        .map_err(|e| format!("Failed to verify asset index: {e}"))?;

                        let tasks = instance.download_tasks(&metadata, &asset_index, &dirs);
                        download::download_all(
                            &client,
                            tasks,
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
                            java.path,
                        )
                        .map_err(|e| format!("Failed to build launch command: {e}"))?;

                        let process = cmd
                            .spawn()
                            .await
                            .map_err(|e| format!("Failed to spawn game process: {e}"))?;

                        // Update the last played time.
                        let mut inst_mut = instance.clone();
                        let _ = inst_mut.update_last_play();

                        // A background standalone task monitors the process exit.
                        let inst_name = instance.config.name.clone();
                        let out_bg = out.clone();
                        tokio::spawn(async move {
                            let status = process.wait().await;
                            let _ = out_bg.send(HomeCmdOutput::GameExited {
                                name: inst_name,
                                status,
                            });
                        });

                        Ok(())
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
        _sender: ComponentSender<Self>,
    ) -> ComponentParts<Self> {
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
                if let Err(err) = result {
                    let _ = sender.output(HomeOutput::ShowToast(err));
                }
            }
            HomeCmdOutput::GameExited { name, status } => match status {
                Ok(s) => {
                    let _ = sender.output(HomeOutput::ShowToast(format!(
                        "Instance \"{name}\" exited with code {:?}",
                        s.code()
                    )));
                }
                Err(err) => {
                    let _ = sender.output(HomeOutput::ShowToast(format!(
                        "Error waiting for instance \"{name}\": {err}"
                    )));
                }
            },
        }
    }
}
