// SPDX-FileCopyrightText: 2026 The Adwaita Craft Launcher Authors
// SPDX-License-Identifier: GPL-3.0-or-later

// Thanks to CurseForge, we have to write an extra page.

use std::{collections::HashSet, path::PathBuf, sync::Arc};

use acl_core::{LauncherConfig, manager::InstanceManager};
use acl_minecraft::ManualModpackFile;
use gtk::prelude::*;
use libadwaita::prelude::*;
use relm4::ComponentParts;
use relm4::{SharedState, prelude::*};
use tracing::error;

use crate::i18n::LocalizeError;
use crate::t;

pub struct ManualInit {
    pub mgr: Arc<SharedState<InstanceManager>>,
    pub config: Arc<SharedState<LauncherConfig>>,
}

#[derive(Debug)]
pub enum ManualInput {
    Open {
        instance_id: String,
        launch_after: bool,
    },
    OpenUrl {
        project_id: u64,
        file_id: u64,
        url: String,
    },
    OpenNextBatch,
    PickFiles,
    PickFolder,
    ImportFiles(Vec<PathBuf>),
    ImportFromDir(PathBuf),
    Recheck,
    SkipFile {
        project_id: u64,
        file_id: u64,
    },
    SkipAllOptional,
    OpenInstanceFolder,
    RequestIgnoreAll,
    CancelIgnoreAll,
    ConfirmIgnoreAll,
    LaunchAnyway,
    Finish,
    Back,
    Refresh,
}

#[derive(Debug, Clone)]
pub enum ManualOutput {
    Back,
    /// Complete and launch, or launch anyway: notify AppModel to switch back to the main screen and instruct HomePage to skip this check on launch.
    LaunchBypass(String),
    ShowToast(String),
}

#[derive(Debug)]
pub enum ManualCmdOutput {
    Checked {
        request_id: u64,
        result: Result<Vec<ManualModpackFile>, String>,
    },
    Imported {
        request_id: u64,
        imported: u32,
        missing: Result<Vec<ManualModpackFile>, String>,
    },
}

pub struct ManualPage {
    mgr: Arc<SharedState<InstanceManager>>,

    instance_id: String,
    instance_name: String,
    instance_dir: PathBuf,
    launch_after: bool,

    files: Vec<ManualModpackFile>,
    /// `(project_id, file_id)` pairs for which "Open Webpage" has been clicked during the current page session.
    opened: HashSet<(u64, u64)>,

    is_busy: bool,
    confirm_ignore_all: bool,
    request_id: u64,

    required_group: adw::PreferencesGroup,
    required_rows: Vec<adw::ActionRow>,
    optional_group: adw::PreferencesGroup,
    optional_rows: Vec<adw::ActionRow>,
}

impl ManualPage {
    fn required_count(&self) -> usize {
        let mut count: usize = 0;
        for file in &self.files {
            if file.required {
                count = count.saturating_add(1);
            }
        }

        count
    }

    fn optional_count(&self) -> usize {
        let mut count: usize = 0;
        for file in &self.files {
            if !file.required {
                count = count.saturating_add(1);
            }
        }
        count
    }

    fn has_missing_required(&self) -> bool {
        self.required_count() > 0
    }

    fn subtitle(&self) -> String {
        if self.is_busy {
            return t!(
                "installer-manual-subtitle-checking",
                "{name} · Checking files...",
                name = self.instance_name
            );
        }

        let required = self.required_count();
        if required > 0 {
            return t!(
                "installer-manual-subtitle-required",
                "{name} · {count} required remaining",
                name = self.instance_name,
                count = required
            );
        }

        let optional = self.optional_count();
        if optional > 0 {
            return t!(
                "installer-manual-subtitle-optional",
                "{name} · {count} optional remaining",
                name = self.instance_name,
                count = optional
            );
        }

        t!(
            "installer-manual-subtitle-ready",
            "{name} · Ready",
            name = self.instance_name
        )
    }

    fn finish_label(&self) -> String {
        if self.launch_after {
            t!("installer-manual-finish-and-launch", "Finish & Launch")
        } else {
            t!("installer-manual-finish", "Finish")
        }
    }

    fn rebuild_rows(&mut self, sender: &ComponentSender<Self>) {
        for row in self.required_rows.drain(..) {
            self.required_group.remove(&row);
        }
        for row in self.optional_rows.drain(..) {
            self.optional_group.remove(&row);
        }

        for file in &self.files {
            let title = if let Some(name) = &file.project_name {
                name.clone()
            } else if let Some(name) = &file.file_name {
                name.clone()
            } else {
                format!("A Mod With No Name (#{})", file.project_id)
            };

            let mut parts = Vec::new();
            if let Some(name) = &file.file_name {
                parts.push(name.clone());
            }
            parts.push(format!("{}/", file.target_dir));

            if file.reason == acl_minecraft::ManualDownloadReason::MetadataNotFound {
                parts.push(t!(
                    "installer-manual-reason-no-metadata",
                    "Metadata unavailable (place manually and skip)"
                ));
            }
            if self.opened.contains(&(file.project_id, file.file_id)) {
                parts.push(t!("installer-manual-status-opened", "Opened"));
            }

            let row = adw::ActionRow::builder()
                .use_markup(false)
                .title(title)
                .subtitle(parts.join(" · "))
                .build();

            let open_btn = gtk::Button::builder()
                .valign(gtk::Align::Center)
                .icon_name("external-link-symbolic")
                .tooltip_text(t!("installer-manual-open-page", "Open download page"))
                .build();
            open_btn.add_css_class("flat");

            let sender_open = sender.clone();
            let project_id = file.project_id;
            let file_id = file.file_id;
            let url = file.page_url.clone();

            open_btn.connect_clicked(move |_| {
                sender_open.input(ManualInput::OpenUrl {
                    project_id,
                    file_id,
                    url: url.clone(),
                });
            });

            let skip_btn = gtk::Button::builder()
                .valign(gtk::Align::Center)
                .label(t!("installer-manual-skip", "Skip"))
                .build();
            skip_btn.add_css_class("flat");

            let sender_skip = sender.clone();
            skip_btn.connect_clicked(move |_| {
                sender_skip.input(ManualInput::SkipFile {
                    project_id,
                    file_id,
                });
            });

            row.add_suffix(&open_btn);
            row.add_suffix(&skip_btn);

            if file.required {
                self.required_group.add(&row);
                self.required_rows.push(row);
            } else {
                self.optional_group.add(&row);
                self.optional_rows.push(row);
            }
        }
    }

    fn spawn_recheck(&mut self, sender: &ComponentSender<Self>) {
        self.request_id = self.request_id.wrapping_add(1);
        let request_id = self.request_id;
        self.is_busy = true;

        let dir = self.instance_dir.clone();
        let files = self.files.clone();

        sender.oneshot_command(async move {
            let result = relm4::tokio::task::spawn_blocking(move || {
                acl_core::manual::find_missing(&dir, &files).map_err(|e| e.to_i18n())
            })
            .await;

            let result = match result {
                Ok(res) => res,
                Err(err) => Err(err.to_string()),
            };

            ManualCmdOutput::Checked { request_id, result }
        });
    }

    fn spawn_import_files(&mut self, paths: Vec<PathBuf>, sender: &ComponentSender<Self>) {
        self.request_id = self.request_id.wrapping_add(1);
        let request_id = self.request_id;
        self.is_busy = true;

        let dir = self.instance_dir.clone();
        let files = self.files.clone();

        sender.oneshot_command(async move {
            let result = relm4::tokio::task::spawn_blocking(move || {
                let imported = acl_core::manual::import_files(&dir, &files, &paths)
                    .map_err(|e| e.to_i18n())?;
                let missing =
                    acl_core::manual::find_missing(&dir, &files).map_err(|e| e.to_i18n())?;
                Ok::<(u32, Vec<ManualModpackFile>), String>((imported, missing))
            })
            .await;

            match result {
                Ok(Ok((imported, missing))) => ManualCmdOutput::Imported {
                    request_id,
                    imported,
                    missing: Ok(missing),
                },
                Ok(Err(err)) => ManualCmdOutput::Imported {
                    request_id,
                    imported: 0,
                    missing: Err(err),
                },
                Err(err) => ManualCmdOutput::Imported {
                    request_id,
                    imported: 0,
                    missing: Err(err.to_string()),
                },
            }
        });
    }

    fn spawn_import_from_dir(&mut self, scan_dir: PathBuf, sender: &ComponentSender<Self>) {
        self.request_id = self.request_id.wrapping_add(1);
        let request_id = self.request_id;
        self.is_busy = true;

        let dir = self.instance_dir.clone();
        let files = self.files.clone();

        sender.oneshot_command(async move {
            let result = relm4::tokio::task::spawn_blocking(move || {
                let imported = acl_core::manual::import_from_dir(&dir, &files, &scan_dir)
                    .map_err(|e| e.to_i18n())?;
                let missing =
                    acl_core::manual::find_missing(&dir, &files).map_err(|e| e.to_i18n())?;
                Ok::<(u32, Vec<ManualModpackFile>), String>((imported, missing))
            })
            .await;

            match result {
                Ok(Ok((imported, missing))) => ManualCmdOutput::Imported {
                    request_id,
                    imported,
                    missing: Ok(missing),
                },
                Ok(Err(err)) => ManualCmdOutput::Imported {
                    request_id,
                    imported: 0,
                    missing: Err(err),
                },
                Err(err) => ManualCmdOutput::Imported {
                    request_id,
                    imported: 0,
                    missing: Err(err.to_string()),
                },
            }
        });
    }
}

#[relm4::component(pub)]
impl Component for ManualPage {
    type Init = ManualInit;
    type Input = ManualInput;
    type Output = ManualOutput;
    type CommandOutput = ManualCmdOutput;

    view! {
        #[root]
        adw::ToolbarView {
            add_controller = gtk::DropTarget::new(
                gtk::glib::Type::INVALID,
                gtk::gdk::DragAction::COPY,
            ) {
                set_types: &[
                    gtk::gdk::FileList::static_type(),
                    gtk::gio::File::static_type(),
                ],

                connect_drop[sender] => move |_, value, _, _| {
                    let mut paths = Vec::new();
                    if let Ok(files) = value.get::<gtk::gdk::FileList>() {
                        for file in files.files() {
                            if let Some(path) = file.path() {
                                paths.push(path);
                            }
                        }
                    } else if let Ok(file) = value.get::<gtk::gio::File>() && let Some(path) = file.path() {
                        paths.push(path);
                    }

                    if paths.len() == 1 && paths.first().is_some_and(|p| p.is_dir()) {
                        if let Some(dir) = paths.into_iter().next() {
                            sender.input(ManualInput::ImportFromDir(dir));
                        }
                    } else if !paths.is_empty() {
                        sender.input(ManualInput::ImportFiles(paths));
                    }

                    true
                },

            },

            add_top_bar = &adw::HeaderBar {
                #[wrap(Some)]
                set_title_widget = &adw::WindowTitle {
                    #[watch]
                    set_title: &t!("installer-manual-title", "Manual Downloads"),
                    #[watch]
                    set_subtitle: &model.subtitle(),
                },

                pack_start = &gtk::Button {
                    connect_clicked => ManualInput::Back,
                    adw::ButtonContent {
                        set_icon_name: "go-previous-symbolic",
                        #[watch]
                        set_label: &t!("instances-create-button-back", "Back"),
                    }
                },

                pack_end = &gtk::Button {
                    add_css_class: "suggested-action",
                    #[watch]
                    set_sensitive: !model.is_busy && !model.has_missing_required(),
                    #[watch]
                    set_label: &model.finish_label(),
                    connect_clicked => ManualInput::Finish,
                }

            },

            // Inline "Ignore All" confirmation bar
            add_top_bar = &gtk::Revealer {
                set_transition_type: gtk::RevealerTransitionType::SlideDown,
                #[watch]
                set_reveal_child: model.confirm_ignore_all,

                gtk::Box {
                    set_orientation: gtk::Orientation::Horizontal,
                    set_spacing: 12,
                    set_margin_top: 8,
                    set_margin_bottom: 8,
                    set_margin_start: 16,
                    set_margin_end: 16,

                    gtk::Image {
                        set_icon_name: Some("dialog-warning-symbolic"),
                        add_css_class: "warning",
                    },

                    gtk::Label {
                        set_hexpand: true,
                        set_halign: gtk::Align::Start,
                        set_wrap: true,
                        #[watch]
                        set_label: &t!(
                            "installer-manual-ignore-confirm-text",
                            "Ignore all remaining files? Launching without required mods may fail."
                        ),
                    },

                    gtk::Button {
                        #[watch]
                        set_label: &t!("installer-manual-cancel", "Cancel"),
                        connect_clicked => ManualInput::CancelIgnoreAll,
                    },

                    gtk::Button {
                        add_css_class: "destructive-action",
                        #[watch]
                        set_label: &t!("installer-manual-confirm-ignore", "Confirm Ignore"),
                        connect_clicked => ManualInput::ConfirmIgnoreAll,
                    },

                },
            },

            #[wrap(Some)]
            set_content = &adw::PreferencesPage {
                // Import and quick actions group
                adw::PreferencesGroup {
                    #[watch]
                    set_visible: !model.files.is_empty(),
                    #[watch]
                    set_title: &t!("installer-manual-actions-title", "Import & Actions"),
                    #[watch]
                    set_description: Some(&t!(
                        "installer-manual-actions-description",
                        "CurseForge disabled automatic downloads for these files. Download them in your browser, then drag them here or import below."
                    )),

                    adw::ActionRow {
                        #[watch]
                        set_title: &t!("installer-manual-import-title", "Import Downloaded Files"),
                        #[watch]
                        set_subtitle: &t!(
                            "installer-manual-import-subtitle",
                            "Files are matched by checksum and copied to the right folder automatically"
                        ),

                        add_suffix = &gtk::Button {
                            set_valign: gtk::Align::Center,
                            #[watch]
                            set_sensitive: !model.is_busy,
                            #[watch]
                            set_label: &t!("installer-manual-pick-files", "Select Files"),
                            connect_clicked => ManualInput::PickFiles,
                        },

                        add_suffix = &gtk::Button {
                            set_valign: gtk::Align::Center,
                            #[watch]
                            set_sensitive: !model.is_busy,
                            #[watch]
                            set_label: &t!("installer-manual-pick-folder", "Scan Folder"),
                            connect_clicked => ManualInput::PickFolder,
                        },

                        add_suffix = &gtk::Button {
                            set_valign: gtk::Align::Center,
                            add_css_class: "flat",
                            set_icon_name: "view-refresh-symbolic",
                            #[watch]
                            set_sensitive: !model.is_busy,
                            #[watch]
                            set_tooltip_text: Some(&t!("installer-manual-recheck", "Recheck")),
                            connect_clicked => ManualInput::Recheck,
                        },

                    },

                    adw::ActionRow {
                        #[watch]
                        set_title: &t!("installer-manual-batch-open-title", "Open Download Pages"),
                        #[watch]
                        set_subtitle: &t!(
                            "installer-manual-batch-open-subtitle",
                            "Opens up to 5 unopened CurseForge pages in your browser"
                        ),

                        add_suffix = &gtk::Button {
                            set_valign: gtk::Align::Center,
                            #[watch]
                            set_sensitive: !model.is_busy,
                            #[watch]
                            set_label: &t!("installer-manual-open-next-batch", "Open Next Batch"),
                            connect_clicked => ManualInput::OpenNextBatch,
                        }
                    },

                },

                #[local_ref]
                add = required_group -> adw::PreferencesGroup {
                    #[watch]
                    set_visible: model.required_count() > 0,
                    #[watch]
                    set_title: &t!(
                        "installer-manual-required-title",
                        "Required ({count})",
                        count = model.required_count()
                    ),

                    #[wrap(Some)]
                    set_header_suffix = &gtk::Button {
                        add_css_class: "flat",
                        set_valign: gtk::Align::Center,
                        connect_clicked => ManualInput::OpenInstanceFolder,
                        adw::ButtonContent {
                            set_icon_name: "folder-open-symbolic",
                            #[watch]
                            set_label: &t!("installer-manual-open-folder", "Open Instance Folder"),
                        },
                    },
                },

                #[local_ref]
                add = optional_group -> adw::PreferencesGroup {
                    #[watch]
                    set_visible: model.optional_count() > 0,
                    #[watch]
                    set_title: &t!(
                        "installer-manual-optional-title",
                        "Optional ({count})",
                        count = model.optional_count()
                    ),

                    #[wrap(Some)]
                    set_header_suffix = &gtk::Button {
                        add_css_class: "flat",
                        set_valign: gtk::Align::Center,
                        #[watch]
                        set_sensitive: !model.is_busy,
                        #[watch]
                        set_label: &t!("installer-manual-skip-optional", "Skip Optional"),
                        connect_clicked => ManualInput::SkipAllOptional,
                    },
                },

                // All-ready prompt (displayed when the list is empty)
                adw::PreferencesGroup {
                    #[watch]
                    set_visible: model.files.is_empty() && !model.is_busy,

                    adw::ActionRow {
                        #[watch]
                        set_title: &t!("installer-manual-all-ready-title", "All files are ready"),
                        #[watch]
                        set_subtitle: &t!(
                            "installer-manual-all-ready-subtitle",
                            "You can finish and launch the instance now"
                        ),
                        add_prefix = &gtk::Image {
                            set_icon_name: Some("emblem-ok-symbolic"),
                            add_css_class: "success",
                        },
                    },

                },
                // Bottom bypass options group
                adw::PreferencesGroup {
                    #[watch]
                    set_visible: !model.files.is_empty(),
                    #[watch]
                    set_title: &t!("installer-manual-bypass-title", "Bypass"),

                    adw::ActionRow {
                        #[watch]
                        set_visible: model.launch_after && model.has_missing_required(),
                        #[watch]
                        set_title: &t!("installer-manual-launch-anyway-title", "Launch Anyway"),
                        #[watch]
                        set_subtitle: &t!(
                            "installer-manual-launch-anyway-subtitle",
                            "Skip this check once without clearing the remaining list"
                        ),

                        add_suffix = &gtk::Button {
                            set_valign: gtk::Align::Center,
                            #[watch]
                            set_sensitive: !model.is_busy,
                            #[watch]
                            set_label: &t!("installer-manual-launch-anyway-button", "Launch Anyway"),
                            connect_clicked => ManualInput::LaunchAnyway,
                        },
                    },

                    adw::ActionRow {
                        #[watch]
                        set_title: &t!("installer-manual-ignore-all-title", "Ignore Remaining Files"),
                        #[watch]
                        set_subtitle: &t!(
                            "installer-manual-ignore-all-subtitle",
                            "Clear the manual download list and stop prompting for this instance"
                        ),

                        add_suffix = &gtk::Button {
                            set_valign: gtk::Align::Center,
                            add_css_class: "destructive-action",
                            #[watch]
                            set_sensitive: !model.is_busy,
                            #[watch]
                            set_label: &t!("installer-manual-ignore-all-button", "Ignore All"),
                            connect_clicked => ManualInput::RequestIgnoreAll,
                        },
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
        init.config
            .subscribe(sender.input_sender(), |_| ManualInput::Refresh);

        let model = Self {
            mgr: init.mgr,
            instance_id: String::new(),
            instance_name: String::new(),
            instance_dir: PathBuf::new(),
            launch_after: false,
            files: Vec::new(),
            opened: HashSet::new(),
            is_busy: false,
            confirm_ignore_all: false,
            request_id: 0,
            required_group: adw::PreferencesGroup::new(),
            required_rows: Vec::new(),
            optional_group: adw::PreferencesGroup::new(),
            optional_rows: Vec::new(),
        };

        let required_group = &model.required_group;
        let optional_group = &model.optional_group;
        let widgets = view_output!();

        ComponentParts { model, widgets }
    }

    fn update(&mut self, msg: Self::Input, sender: ComponentSender<Self>, root: &Self::Root) {
        match msg {
            ManualInput::Open {
                instance_id,
                launch_after,
            } => {
                let instance_info = {
                    let mgr = self.mgr.read();
                    mgr.instance(&instance_id)
                        .map(|inst| (inst.config.name.clone(), inst.game_dir().to_path_buf()))
                };

                let Some((name, dir)) = instance_info else {
                    let _ = sender.output(ManualOutput::ShowToast(t!(
                        "installer-manual-error-instance-not-found",
                        "Instance not found"
                    )));
                    let _ = sender.output(ManualOutput::Back);
                    return;
                };

                self.instance_id = instance_id;
                self.instance_name = name;
                self.instance_dir = dir;
                self.launch_after = launch_after;
                self.opened.clear();
                self.confirm_ignore_all = false;
                self.is_busy = false;

                match acl_core::manual::load(&self.instance_dir) {
                    Ok(files) => {
                        self.files = files;
                        self.rebuild_rows(&sender);
                        if !self.files.is_empty() {
                            self.spawn_recheck(&sender);
                        }
                    }
                    Err(err) => {
                        error!("Failed to load manual downloads list: {err}");
                        self.files.clear();
                        self.rebuild_rows(&sender);
                        let _ = sender.output(ManualOutput::ShowToast(err.to_i18n()));
                    }
                }
            }
            ManualInput::OpenUrl {
                project_id,
                file_id,
                url,
            } => {
                if let Err(err) = gtk::gio::AppInfo::launch_default_for_uri(
                    &url,
                    gtk::gio::AppLaunchContext::NONE,
                ) {
                    error!("Failed to open CurseForge download page '{url}': {err}");
                    let _ = sender.output(ManualOutput::ShowToast(t!(
                        "installer-manual-error-open-url",
                        "Failed to open browser: {err}",
                        err = err
                    )));
                    return;
                }
                self.opened.insert((project_id, file_id));
                self.rebuild_rows(&sender);
            }
            ManualInput::OpenNextBatch => {
                const BATCH_SIZE: usize = 5;

                let mut batch: Vec<(u64, u64, String)> = Vec::new();
                // Prioritize required files first, then optional files.
                for required_pass in [true, false] {
                    for file in &self.files {
                        if batch.len() >= BATCH_SIZE {
                            break;
                        }

                        if file.required == required_pass
                            && !self.opened.contains(&(file.project_id, file.file_id))
                        {
                            batch.push((file.project_id, file.file_id, file.page_url.clone()));
                        }
                    }
                }

                if batch.is_empty() {
                    let _ = sender.output(ManualOutput::ShowToast(t!(
                        "installer-manual-all-pages-opened",
                        "All download pages have already been opened"
                    )));
                    return;
                }

                let mut failed = false;
                for (project_id, file_id, url) in batch {
                    match gtk::gio::AppInfo::launch_default_for_uri(
                        &url,
                        gtk::gio::AppLaunchContext::NONE,
                    ) {
                        Ok(()) => {
                            self.opened.insert((project_id, file_id));
                        }
                        Err(err) => {
                            error!("Failed to open CurseForge download page '{url}': {err}");
                            failed = true;
                        }
                    }
                }

                self.rebuild_rows(&sender);
                if failed {
                    let _ = sender.output(ManualOutput::ShowToast(t!(
                        "installer-manual-error-open-some-urls",
                        "Failed to open one or more download pages"
                    )));
                }
            }
            ManualInput::PickFiles => {
                let dialog = gtk::FileDialog::builder()
                    .title(t!(
                        "installer-manual-pick-files-title",
                        "Select Downloaded Files"
                    ))
                    .build();
                let window = root.root().and_then(|r| r.downcast::<gtk::Window>().ok());
                let sender_clone = sender.clone();

                dialog.open_multiple(window.as_ref(), gtk::gio::Cancellable::NONE, move |res| {
                    let Ok(list) = res else {
                        return;
                    };
                    let mut paths = Vec::new();
                    for i in 0..list.n_items() {
                        if let Some(obj) = list.item(i)
                            && let Ok(file) = obj.downcast::<gtk::gio::File>()
                            && let Some(path) = file.path()
                        {
                            paths.push(path);
                        }
                    }
                    if !paths.is_empty() {
                        sender_clone.input(ManualInput::ImportFiles(paths));
                    }
                });
            }

            ManualInput::PickFolder => {
                let dialog = gtk::FileDialog::builder()
                    .title(t!(
                        "installer-manual-pick-folder-title",
                        "Select Folder to Scan"
                    ))
                    .build();
                let window = root.root().and_then(|r| r.downcast::<gtk::Window>().ok());
                let sender_clone = sender.clone();

                dialog.select_folder(window.as_ref(), gtk::gio::Cancellable::NONE, move |res| {
                    if let Ok(file) = res
                        && let Some(path) = file.path()
                    {
                        sender_clone.input(ManualInput::ImportFromDir(path));
                    }
                });
            }

            ManualInput::ImportFiles(paths) => {
                if !self.is_busy && !self.files.is_empty() {
                    self.spawn_import_files(paths, &sender);
                }
            }

            ManualInput::ImportFromDir(dir) => {
                if !self.is_busy && !self.files.is_empty() {
                    self.spawn_import_from_dir(dir, &sender);
                }
            }

            ManualInput::Recheck => {
                if !self.is_busy && !self.files.is_empty() {
                    self.spawn_recheck(&sender);
                }
            }

            ManualInput::SkipFile {
                project_id,
                file_id,
            } => {
                self.files
                    .retain(|f| !(f.project_id == project_id && f.file_id == file_id));

                if let Err(err) = acl_core::manual::save(&self.instance_dir, &self.files) {
                    error!("Failed to save manual downloads list: {err}");
                    let _ = sender.output(ManualOutput::ShowToast(err.to_i18n()));
                }
                self.rebuild_rows(&sender);
            }

            ManualInput::SkipAllOptional => {
                self.files.retain(|f| f.required);

                if let Err(err) = acl_core::manual::save(&self.instance_dir, &self.files) {
                    error!("Failed to save manual downloads list: {err}");
                    let _ = sender.output(ManualOutput::ShowToast(err.to_i18n()));
                }
                self.rebuild_rows(&sender);
            }

            ManualInput::OpenInstanceFolder => {
                let file = gtk::gio::File::for_path(&self.instance_dir);
                if let Err(err) = gtk::gio::AppInfo::launch_default_for_uri(
                    &file.uri(),
                    gtk::gio::AppLaunchContext::NONE,
                ) {
                    error!(
                        "Failed to open instance directory '{}': {err}",
                        self.instance_dir.display()
                    );
                    let _ = sender.output(ManualOutput::ShowToast(t!(
                        "installer-manual-error-open-folder",
                        "Failed to open instance folder: {err}",
                        err = err
                    )));
                }
            }

            ManualInput::RequestIgnoreAll => {
                self.confirm_ignore_all = true;
            }

            ManualInput::CancelIgnoreAll => {
                self.confirm_ignore_all = false;
            }

            ManualInput::ConfirmIgnoreAll => {
                self.confirm_ignore_all = false;
                if let Err(err) = acl_core::manual::clear(&self.instance_dir) {
                    error!("Failed to clear manual downloads list: {err}");
                    let _ = sender.output(ManualOutput::ShowToast(err.to_i18n()));
                    return;
                }
                self.files.clear();
                self.opened.clear();
                self.rebuild_rows(&sender);

                if self.launch_after {
                    let _ = sender.output(ManualOutput::LaunchBypass(self.instance_id.clone()));
                } else {
                    let _ = sender.output(ManualOutput::Back);
                }
            }

            ManualInput::LaunchAnyway => {
                self.confirm_ignore_all = false;
                let _ = sender.output(ManualOutput::LaunchBypass(self.instance_id.clone()));
            }

            ManualInput::Finish => {
                if self.has_missing_required() {
                    return;
                }
                self.confirm_ignore_all = false;
                if let Err(err) = acl_core::manual::clear(&self.instance_dir) {
                    error!("Failed to clear manual downloads list: {err}");
                    let _ = sender.output(ManualOutput::ShowToast(err.to_i18n()));
                    return;
                }

                self.files.clear();
                self.opened.clear();
                self.rebuild_rows(&sender);

                if self.launch_after {
                    let _ = sender.output(ManualOutput::LaunchBypass(self.instance_id.clone()));
                } else {
                    let _ = sender.output(ManualOutput::Back);
                }
            }

            ManualInput::Back => {
                self.confirm_ignore_all = false;
                let _ = sender.output(ManualOutput::Back);
            }

            ManualInput::Refresh => {
                self.rebuild_rows(&sender);
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
            ManualCmdOutput::Checked { request_id, result } => {
                if request_id != self.request_id {
                    return;
                }
                self.is_busy = false;

                match result {
                    Ok(missing) => {
                        if missing.len() != self.files.len()
                            && let Err(err) = acl_core::manual::save(&self.instance_dir, &missing)
                        {
                            error!("Failed to update manual downloads list: {err}");
                        }

                        self.files = missing;
                        self.rebuild_rows(&sender);
                    }
                    Err(err) => {
                        let _ = sender.output(ManualOutput::ShowToast(err));
                    }
                }
            }
            ManualCmdOutput::Imported {
                request_id,
                imported,
                missing,
            } => {
                if request_id != self.request_id {
                    return;
                }
                self.is_busy = false;

                match missing {
                    Ok(missing) => {
                        if missing.len() != self.files.len()
                            && let Err(err) = acl_core::manual::save(&self.instance_dir, &missing)
                        {
                            error!("Failed to update manual downloads list: {err}");
                        }
                        self.files = missing;
                        self.rebuild_rows(&sender);

                        let toast = if imported > 0 {
                            t!(
                                "installer-manual-imported-count",
                                "Imported {count} file(s)",
                                count = imported
                            )
                        } else {
                            t!("installer-manual-imported-none", "No matching files found")
                        };
                        let _ = sender.output(ManualOutput::ShowToast(toast));
                    }
                    Err(err) => {
                        let _ = sender.output(ManualOutput::ShowToast(err));
                    }
                }
            }
        }
    }
}
