use std::{collections::HashMap, sync::Arc};

use acl_core::{Instance, LauncherConfig, LauncherDirs, manager::InstanceManager};

use acl_minecraft::{AssetIndex, VersionEntry, download::DownloadProgress};
use libadwaita::prelude::*;
use relm4::{SharedState, gtk, prelude::*, tokio};
use tracing::error;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum InstallStage {
    /// Normal and usable
    #[default]
    Idle,
    /// Stage 1: Fetch the version JSON
    FetchingMetadata,
    /// Stage 2: Fetch the asset index JSON
    FetchingAssetIndex,
    /// Stage 3: Concurrently download the Jar / libraries / assets
    Downloading(DownloadProgress),
    /// Stage 4: Extract Natives and map legacy assets
    PreparingRuntime,
}

impl InstallStage {
    fn is_installing(&self) -> bool {
        *self != InstallStage::Idle
    }

    fn fraction(&self) -> f64 {
        match self {
            InstallStage::Idle => 0.0,
            InstallStage::FetchingMetadata => 0.02,
            InstallStage::FetchingAssetIndex => 0.05,
            InstallStage::Downloading(p) => {
                let ratio = if p.total_bytes > 0 {
                    (p.completed_bytes as f64 / p.total_bytes as f64).clamp(0.0, 1.0)
                } else {
                    0.0
                };

                0.05 + ratio * 0.9
            }

            InstallStage::PreparingRuntime => 0.98,
        }
    }
    fn status_text(&self) -> Option<String> {
        match self {
            InstallStage::Idle => None,
            InstallStage::FetchingMetadata => Some("Fetching metadata...".to_owned()),
            InstallStage::FetchingAssetIndex => Some("Fetching asset index...".to_owned()),
            InstallStage::Downloading(p) => Some(format!(
                "Downloading ({}/{})",
                p.completed_files, p.total_files
            )),
            InstallStage::PreparingRuntime => Some("Preparing runtime...".to_owned()),
        }
    }
}

#[derive(Debug, Clone)]
pub struct InstanceRow {
    instance: Instance,
    is_active: bool,
    stage: InstallStage,
}

#[derive(Debug)]
pub enum InstanceRowInput {
    SetActive(bool),
    SetStage(InstallStage),
}

#[derive(Debug)]
pub enum InstanceRowOutput {
    Select(String),
    Remove(String),
}

impl InstanceRow {
    fn subtitle(&self) -> String {
        let base = format!(
            "{} • {}",
            self.instance.config.game_version,
            self.instance.config.mod_loader.label()
        );

        if let Some(status) = self.stage.status_text() {
            format!("{base} • {status}")
        } else {
            base
        }
    }
}

#[relm4::factory(pub)]
impl FactoryComponent for InstanceRow {
    type Init = (Instance, bool);
    type Input = InstanceRowInput;
    type Output = InstanceRowOutput;
    type CommandOutput = ();
    type ParentWidget = adw::PreferencesGroup;

    view! {
        #[root]
        #[name = "row"]
        adw::ActionRow {
            set_title: &self.instance.config.name,
            #[watch]
            set_subtitle: &self.subtitle(),

            set_selectable: false,
            set_focus_on_click: false,

            // Cannot be clicked during installation.
            #[watch]
            set_activatable: !self.stage.is_installing(),

            #[watch]
            set_class_active: ("dim-label", self.stage.is_installing()),

            connect_activated[sender, id = self.instance.id().to_owned()] => move |_| {
                let _ = sender.output(InstanceRowOutput::Select(id.clone()));
            },

            add_suffix = &gtk::ProgressBar {
                set_valign: gtk::Align::Center,
                set_width_request: 120,

                #[watch]
                set_visible: self.stage.is_installing(),

                #[watch]
                set_fraction: self.stage.fraction(),
            },

            #[name = "select_check"]
            add_suffix = &gtk::CheckButton {
                #[watch]
                set_visible: !self.stage.is_installing(),

                set_valign: gtk::Align::Center,
                add_css_class: "selection-mode",
                set_can_focus: false,
                set_can_target: false,
                #[watch]
                set_active: self.is_active,
            },

            add_suffix = &gtk::Button {
                #[watch]
                set_visible: !self.stage.is_installing(),

                set_icon_name: "user-trash-symbolic",
                set_valign: gtk::Align::Center,
                add_css_class: "flat",
                set_tooltip_text: Some("Remove instance"),
                connect_clicked[delete_dialog, row] => move |_| {
                    delete_dialog.present(Some(&row));
                },
            },



        },

        #[name = "delete_dialog"]
        adw::AlertDialog {
            set_heading: Some("Delete Instance"),
            #[watch]
            set_body: &format!("Are you sure you want to delete instance \"{}\"?", self.instance.config.name),

            add_response: ("cancel", "Cancel"),
            add_response: ("delete", "Delete"),
            set_response_appearance: ("delete", adw::ResponseAppearance::Destructive),
            set_default_response: Some("cancel"),
            set_close_response: "cancel",

            connect_response: (Some("delete"), {
                let sender = sender.clone();
                let id = self.instance.id().to_owned();
                move |_, _| {
                    let _ = sender.output(InstanceRowOutput::Remove(id.clone()));
                }
            }),

        }
    }

    fn init_model(
        (instance, is_active): Self::Init,
        _index: &Self::Index,
        _sender: FactorySender<Self>,
    ) -> Self {
        Self {
            instance,
            is_active,
            stage: InstallStage::Idle,
        }
    }

    fn update(&mut self, msg: Self::Input, _sender: FactorySender<Self>) {
        match msg {
            InstanceRowInput::SetActive(active) => {
                self.is_active = active;
            }
            InstanceRowInput::SetStage(stage) => {
                self.stage = stage;
            }
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ListInput {
    SelectInstance(String),
    RemoveInstance(String),
    StartDownload(String, VersionEntry),
    Refresh,
}

/// Management page for installed instances.
pub struct ListPage {
    mgr: Arc<SharedState<InstanceManager>>,
    config: Arc<SharedState<LauncherConfig>>,
    client: reqwest::Client,
    dirs: LauncherDirs,
    instance_rows: FactoryVecDeque<InstanceRow>,
    /// Record the state of each instance to prevent it from being lost when rebuilding rows.
    install_stages: HashMap<String, InstallStage>,
}

#[derive(Debug, Clone)]
pub enum ListOutput {
    ShowToast(String),
    OpenCreate,
}

impl ListPage {
    fn save_config(&self, sender: &ComponentSender<Self>) {
        if let Err(err) = self.mgr.read().save() {
            let msg = format!("Failed to save instances: {err}");
            error!("{msg}");
            let _ = sender.output(ListOutput::ShowToast(msg));
        }
    }

    fn rebuild_rows(&mut self) {
        let instances = self.mgr.read();
        let active_id = instances.config.active_id.clone();
        let mut guard = self.instance_rows.guard();
        guard.clear();
        for instance in &instances.instances {
            let is_active = Some(instance.id().to_string()) == active_id;
            guard.push_back((instance.clone(), is_active));
        }

        drop(guard);
        // Synchronize existing stages.
        for (i, row) in self.instance_rows.iter().enumerate() {
            if let Some(&stage) = self.install_stages.get(row.instance.id()) {
                self.instance_rows
                    .send(i, InstanceRowInput::SetStage(stage));
            }
        }
    }

    fn update_row_stage(&mut self, id: &str, stage: InstallStage) {
        if stage == InstallStage::Idle {
            self.install_stages.remove(id);
        } else {
            self.install_stages.insert(id.to_string(), stage);
        }

        if let Some(index) = self
            .instance_rows
            .iter()
            .position(|r| r.instance.id() == id)
        {
            self.instance_rows
                .send(index, InstanceRowInput::SetStage(stage));
        }
    }

    fn spawn_install(&mut self, id: String, entry: VersionEntry, sender: &ComponentSender<Self>) {
        let instance = self
            .mgr
            .read()
            .instances
            .iter()
            .find(|i| i.id() == id)
            .cloned();
        let Some(instance) = instance else {
            return;
        };

        self.update_row_stage(&id, InstallStage::FetchingMetadata);

        let client = self.client.clone();
        let dirs = self.dirs.clone();
        let max_concurrent = self.config.read().max_concurrent_downloads;

        sender.command(move |out, shutdown| {
            shutdown
                .register(async move {
                    let result: Result<(), String> = async {
                        // Fetch metadata
                        let _ = out.send(ListCmdOutput::StageChanged {
                            id: id.clone(),
                            stage: InstallStage::FetchingMetadata,
                        });
                        let metadata = instance
                            .fetch_metadata(&client, &entry, &dirs)
                            .await
                            .map_err(|e| e.to_string())?;

                        // Fetch asset index
                        let _ = out.send(ListCmdOutput::StageChanged {
                            id: id.clone(),
                            stage: InstallStage::FetchingAssetIndex,
                        });
                        let asset_index = AssetIndex::fetch_and_verify(
                            &client,
                            &metadata.asset_index,
                            &dirs.assets_dir(),
                        )
                        .await
                        .map_err(|e| e.to_string())?;

                        // Collect download tasks
                        let tasks = instance.download_tasks(&metadata, &asset_index, &dirs);
                        let total_files = tasks.len();
                        let total_bytes = tasks.iter().map(|t| t.size).sum();

                        let _ = out.send(ListCmdOutput::StageChanged {
                            id: id.clone(),
                            stage: InstallStage::Downloading(DownloadProgress {
                                completed_files: 0,
                                total_files,
                                completed_bytes: 0,
                                total_bytes,
                            }),
                        });

                        // Create a channel to obtain and asynchronously send download progress.
                        let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
                        let out_prog = out.clone();
                        let id_prog = id.clone();
                        let forward_handle = tokio::spawn(async move {
                            while let Some(prog) = rx.recv().await {
                                let _ = out_prog.send(ListCmdOutput::StageChanged {
                                    id: id_prog.clone(),
                                    stage: InstallStage::Downloading(prog),
                                });
                            }
                        });

                        acl_minecraft::download::download_all(
                            &client,
                            tasks,
                            max_concurrent,
                            Some(tx),
                        )
                        .await
                        .map_err(|e| e.to_string())?;
                        let _ = forward_handle.await; // Ownership of tx has already been moved; tx will be dropped once the download completes.

                        let _ = out.send(ListCmdOutput::StageChanged {
                            id: id.clone(),
                            stage: InstallStage::PreparingRuntime,
                        });
                        let inst_c = instance.clone();
                        let dirs_c = dirs.clone();

                        // Prepare runtime
                        tokio::task::spawn_blocking(move || {
                            inst_c.prepare_runtime(&metadata, &asset_index, &dirs_c)
                        })
                        .await
                        .map_err(|e| e.to_string())?
                        .map_err(|e| e.to_string())?;

                        Ok(())
                    }
                    .await;

                    let _ = out.send(ListCmdOutput::InstallFinished { id, result });
                })
                .drop_on_shutdown()
        });
    }
}

pub struct ListPageInit {
    pub mgr: Arc<SharedState<InstanceManager>>,
    pub config: Arc<SharedState<LauncherConfig>>,
    pub client: reqwest::Client,
    pub dirs: LauncherDirs,
}

#[derive(Debug)]
pub enum ListCmdOutput {
    StageChanged {
        id: String,
        stage: InstallStage,
    },
    InstallFinished {
        id: String,
        result: Result<(), String>,
    },
}

#[relm4::component(pub)]
impl Component for ListPage {
    type Init = ListPageInit;
    type Input = ListInput;
    type Output = ListOutput;
    type CommandOutput = ListCmdOutput;

    view! {
        #[root]
        #[name = "page"]
        adw::PreferencesPage {
            set_title: "Instances",

            #[local_ref]
            instances_group -> adw::PreferencesGroup {
                set_title: "Instances",
                set_description: Some("Select an instance to launch Minecraft"),

                #[wrap(Some)]
                set_header_suffix = &gtk::Button {
                    add_css_class: "flat",
                    set_valign: gtk::Align::Center,
                    set_tooltip_text: Some("Create an instance"),

                    adw::ButtonContent {
                        set_icon_name: "list-add-symbolic",
                        set_label: "Create",
                    },

                    connect_clicked[sender] => move |_| {
                        let _ = sender.output(ListOutput::OpenCreate);
                    },
                },

            },

        },


    }

    fn init(
        init: Self::Init,
        root: Self::Root,
        sender: ComponentSender<Self>,
    ) -> ComponentParts<Self> {
        let instance_rows = FactoryVecDeque::builder()
            .launch(adw::PreferencesGroup::default())
            .forward(sender.input_sender(), |output| match output {
                InstanceRowOutput::Select(id) => ListInput::SelectInstance(id),
                InstanceRowOutput::Remove(id) => ListInput::RemoveInstance(id),
            });

        let mut model = Self {
            mgr: init.mgr,
            client: init.client,
            config: init.config,
            dirs: init.dirs,
            instance_rows,
            install_stages: HashMap::new(),
        };

        model.rebuild_rows();

        let instances_group = model.instance_rows.widget();
        let widgets = view_output!();

        ComponentParts { model, widgets }
    }

    fn update(&mut self, msg: Self::Input, sender: ComponentSender<Self>, _root: &Self::Root) {
        match msg {
            ListInput::SelectInstance(id) => {
                if self.install_stages.contains_key(&id) {
                    return; // Selection is disabled while downloading.
                }
                let res = self.mgr.write().set_active_instance(&id).map(|_| ());

                if let Err(err) = res {
                    let msg = format!("Failed to select instance: {err}");
                    error!("{msg}");
                    let _ = sender.output(ListOutput::ShowToast(msg));
                    return;
                }
                self.save_config(&sender);

                for (i, row) in self.instance_rows.iter().enumerate() {
                    let is_active = row.instance.id() == id;
                    self.instance_rows
                        .send(i, InstanceRowInput::SetActive(is_active));
                }
            }
            ListInput::RemoveInstance(id) => {
                if let Err(err) = self.mgr.write().remove_instance(&id) {
                    let msg = format!("Failed to remove instance: {err}");
                    error!("{msg}");
                    let _ = sender.output(ListOutput::ShowToast(msg));
                    return;
                }
                self.save_config(&sender);
                self.rebuild_rows();
            }
            ListInput::Refresh => {
                self.rebuild_rows();
            }
            ListInput::StartDownload(id, entry) => {
                self.spawn_install(id, entry, &sender);
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
            ListCmdOutput::StageChanged { id, stage } => {
                self.update_row_stage(&id, stage);
            }

            ListCmdOutput::InstallFinished { id, result } => {
                self.update_row_stage(&id, InstallStage::Idle);

                match result {
                    Ok(()) => {
                        let _ = sender.output(ListOutput::ShowToast(format!(
                            "Instance \"{id}\" is ready to play"
                        )));
                    }
                    Err(err) => {
                        error!("Instance '{id}' installation failed: {err}");
                        let _ = sender.output(ListOutput::ShowToast(err));
                        // Automatically clean up incomplete instances when the download fails.
                        let _ = self.mgr.write().remove_instance(&id);
                        self.save_config(&sender);
                        self.rebuild_rows();
                    }
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use acl_minecraft::download::DownloadProgress;

    #[test]
    fn test_install_stage_default_and_is_installing() {
        assert_eq!(InstallStage::default(), InstallStage::Idle);
        assert!(!InstallStage::Idle.is_installing());
        assert!(InstallStage::FetchingMetadata.is_installing());
        assert!(InstallStage::FetchingAssetIndex.is_installing());
        assert!(
            InstallStage::Downloading(DownloadProgress {
                completed_files: 0,
                total_files: 10,
                completed_bytes: 0,
                total_bytes: 100,
            })
            .is_installing()
        );
        assert!(InstallStage::PreparingRuntime.is_installing());
    }

    #[test]
    fn test_install_stage_fraction() {
        assert_eq!(InstallStage::Idle.fraction(), 0.0);
        assert_eq!(InstallStage::FetchingMetadata.fraction(), 0.02);
        assert_eq!(InstallStage::FetchingAssetIndex.fraction(), 0.05);
        assert_eq!(InstallStage::PreparingRuntime.fraction(), 0.98);

        // When total_bytes is 0
        let zero_progress = InstallStage::Downloading(DownloadProgress {
            completed_files: 0,
            total_files: 0,
            completed_bytes: 0,
            total_bytes: 0,
        });
        assert_eq!(zero_progress.fraction(), 0.05);

        // 50% completed: 0.05 + 0.5 * 0.9 = 0.50
        let half_progress = InstallStage::Downloading(DownloadProgress {
            completed_files: 5,
            total_files: 10,
            completed_bytes: 50,
            total_bytes: 100,
        });
        assert!((half_progress.fraction() - 0.50).abs() < f64::EPSILON);

        // Clamped at 100% when completed_bytes exceeds total_bytes
        let overflow_progress = InstallStage::Downloading(DownloadProgress {
            completed_files: 11,
            total_files: 10,
            completed_bytes: 120,
            total_bytes: 100,
        });
        assert!((overflow_progress.fraction() - 0.95).abs() < f64::EPSILON);
    }

    #[test]
    fn test_install_stage_status_text() {
        assert_eq!(InstallStage::Idle.status_text(), None);
        assert_eq!(
            InstallStage::FetchingMetadata.status_text(),
            Some("Fetching metadata...".to_string())
        );
        assert_eq!(
            InstallStage::FetchingAssetIndex.status_text(),
            Some("Fetching asset index...".to_string())
        );
        assert_eq!(
            InstallStage::PreparingRuntime.status_text(),
            Some("Preparing runtime...".to_string())
        );

        let downloading = InstallStage::Downloading(DownloadProgress {
            completed_files: 3,
            total_files: 10,
            completed_bytes: 300,
            total_bytes: 1000,
        });
        assert_eq!(
            downloading.status_text(),
            Some("Downloading (3/10)".to_string())
        );
    }
}
