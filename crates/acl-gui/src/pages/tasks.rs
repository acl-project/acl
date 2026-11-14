// SPDX-FileCopyrightText: 2026 The Adwaita Craft Launcher Authors
// SPDX-License-Identifier: GPL-3.0-or-later

use std::sync::Arc;

use acl_core::{
    LauncherConfig,
    task::{StageStatus, TaskEvent, TaskId, TaskManager, TaskSnapshot, TaskState},
};
use libadwaita::prelude::*;
use relm4::{SharedState, factory::FactoryHashMap, prelude::*};

use crate::{t, widgets::stage_row::StageRow};

fn format_speed(bps: u64) -> String {
    const KB: f64 = 1024.0;
    const MB: f64 = KB * 1024.0;
    let b = bps as f64;
    if b >= MB {
        format!("{:.1} MB/s", b / MB)
    } else {
        format!("{:.0} KB/s", b / KB)
    }
}

fn format_duration(secs: u64) -> String {
    let mins = secs / 60;
    let rem = secs % 60;
    if mins > 0 {
        format!("{mins}:{rem:02}")
    } else {
        format!("{rem}s")
    }
}

pub struct TaskRow {
    snap: TaskSnapshot, // "snap" stands for snapshot — not that terrible package manager.
    stages: Vec<Controller<StageRow>>,
}

#[derive(Debug, Clone)]
pub enum TaskRowInput {
    Update(TaskSnapshot),
    Primary,
    CopyError,
    Refresh,
}

#[derive(Debug, Clone, Copy)]
pub enum TaskRowOutput {
    Cancel,
    Retry,
    Dismiss,
}

impl TaskRow {
    /// Calculate the percentage of the total progress based on the current progress
    fn percent_text(&self) -> String {
        let count = self.snap.stages.len();
        if count == 0 {
            return "0%".to_string();
        }

        let sum: f64 = self
            .snap
            .stages
            .iter()
            .map(|st| match st.status {
                StageStatus::Done => 1.0,
                StageStatus::Running if st.total > 0 => {
                    (st.done as f64 / st.total as f64).clamp(0.0, 1.0)
                }
                _ => 0.0,
            })
            .sum();

        let pct = ((sum / count as f64) * 100.0).clamp(0.0, 100.0);
        format!("{pct:.0}%")
    }

    fn subtitle(&self) -> String {
        match &self.snap.state {
            TaskState::Running => {
                let total = self.snap.stages.len();
                if let Some((idx, stage)) = self
                    .snap
                    .stages
                    .iter()
                    .enumerate()
                    .find(|(_, stage)| stage.status == StageStatus::Running)
                {
                    let step = idx.saturating_add(1);
                    if let Some(bps) = stage.bytes_per_sec.filter(|&b| b > 0) {
                        format!(
                            "{} · {}",
                            t!(
                                "tasks-subtitle-stage",
                                "Stage {current}/{total} · {label}",
                                current = step,
                                total = total,
                                label = stage.label
                            ),
                            format_speed(bps)
                        )
                    } else {
                        t!(
                            "tasks-subtitle-stage",
                            "Stage {current}/{total} · {label}",
                            current = step,
                            total = total,
                            label = stage.label
                        )
                    }
                } else {
                    t!("tasks-subtitle-starting", "Starting...")
                }
            }
            TaskState::Succeeded => {
                if let Some(secs) = self.snap.elapsed_secs {
                    t!(
                        "tasks-subtitle-completed-time",
                        "Completed · {time}",
                        time = format_duration(secs)
                    )
                } else {
                    t!("tasks-subtitle-completed", "Completed")
                }
            }
            TaskState::Failed(_) => t!("tasks-subtitle-failed", "Failed"),
            TaskState::Cancelled => t!("tasks-subtitle-cancelled", "Cancelled"),
        }
    }

    fn primary_icon(&self) -> &'static str {
        match self.snap.state {
            TaskState::Running => "process-stop-symbolic",
            TaskState::Failed(_) => "view-refresh-symbolic",
            TaskState::Cancelled | TaskState::Succeeded => "user-trash-symbolic",
        }
    }

    fn primary_tooltip(&self) -> String {
        match self.snap.state {
            TaskState::Running => t!("tasks-action-cancel", "Cancel"),
            TaskState::Failed(_) => t!("tasks-action-retry", "Retry"),
            TaskState::Cancelled | TaskState::Succeeded => t!("tasks-action-remove", "Remove"),
        }
    }

    fn error_text(&self) -> &str {
        match &self.snap.state {
            TaskState::Failed(err) => err.as_str(),
            _ => "",
        }
    }
}

#[relm4::factory(pub)]
impl FactoryComponent for TaskRow {
    type Init = TaskSnapshot;
    type Input = TaskRowInput;
    type Output = (TaskId, TaskRowOutput);
    type CommandOutput = ();
    type ParentWidget = adw::PreferencesGroup;
    type Index = TaskId;

    view! {
        #[root]
        #[name = "row"]
        adw::ExpanderRow {
            set_use_markup: false,
            set_title: &self.snap.title,
            #[watch]
            set_subtitle: &self.subtitle(),

            add_suffix = &gtk::Label {
                set_valign: gtk::Align::Center,
                add_css_class: "numeric",
                add_css_class: "dim-label",
                #[watch]
                set_visible: !self.snap.state.is_finished(),
                #[watch]
                set_label: &self.percent_text(),
            },

            add_suffix = &gtk::Button {
                set_valign: gtk::Align::Center,
                add_css_class: "flat",
                add_css_class: "circular",
                #[watch]
                set_icon_name: self.primary_icon(),
                #[watch]
                set_tooltip_text: Some(&self.primary_tooltip()),
                connect_clicked => TaskRowInput::Primary,
            },

           // The row that displays the error message should be the last one added
           // so that it can be displayed at the bottom.
            add_row = &adw::ActionRow {
                set_use_markup: false,
                add_css_class: "error",
                #[watch]
                set_visible: matches!(self.snap.state, TaskState::Failed(_)),
                #[watch]
                set_title: self.error_text(),

                // Provide buttons for easy copying
                add_suffix = &gtk::Button {
                    set_valign: gtk::Align::Center,
                    add_css_class: "flat",
                    set_icon_name: "edit-copy-symbolic",

                    #[watch]
                    set_tooltip_text: Some(&t!("tasks-action-copy-error", "Copy error")),

                    connect_clicked => TaskRowInput::CopyError,
                }

            },


        }
    }

    fn init_widgets(
        &mut self,
        _index: &Self::Index,
        root: Self::Root,
        _returned_widget: &<Self::ParentWidget as relm4::factory::FactoryView>::ReturnedWidget,
        sender: FactorySender<Self>,
    ) -> Self::Widgets {
        // Build stage rows before view_output!() so error_row stays at the bottom.
        for stage in &self.stages {
            root.add_row(stage.widget());
        }
        // Automatically expand if ongoing
        root.set_expanded(!matches!(
            self.snap.state,
            TaskState::Succeeded | TaskState::Cancelled
        ));
        let widgets = view_output!();
        widgets
    }

    fn init_model(snap: Self::Init, _index: &Self::Index, _sender: FactorySender<Self>) -> Self {
        // StageRow does not update the language
        // This is to ensure the simplicity of the code
        // And no one should switch languages in the middle of downloading.
        let stages: Vec<Controller<StageRow>> = snap
            .stages
            .iter()
            .map(|st| StageRow::builder().launch(st.clone()).detach())
            .collect();

        Self { snap, stages }
    }

    fn update(&mut self, msg: Self::Input, sender: FactorySender<Self>) {
        match msg {
            TaskRowInput::Update(snap) => {
                for (stage_row, stage) in self.stages.iter().zip(&snap.stages) {
                    stage_row.emit(stage.clone());
                }
                self.snap = snap;
            }
            TaskRowInput::Primary => {
                let action = match self.snap.state {
                    TaskState::Running => TaskRowOutput::Cancel,
                    TaskState::Failed(_) => TaskRowOutput::Retry,
                    TaskState::Cancelled | TaskState::Succeeded => TaskRowOutput::Dismiss,
                };
                let _ = sender.output((self.snap.id, action));
            }
            TaskRowInput::CopyError => {
                if let TaskState::Failed(err) = &self.snap.state
                    && let Some(display) = gtk::gdk::Display::default()
                {
                    display.clipboard().set_text(err);
                }
            }
            TaskRowInput::Refresh => {}
        }
    }
}

#[derive(Clone)]
pub struct TasksInit {
    pub config: Arc<SharedState<LauncherConfig>>,
    pub tasks: Arc<TaskManager>,
}

#[derive(Debug, Clone)]
pub enum TasksInput {
    Refresh,
    Action(TaskId, TaskRowOutput),
    ClearFinished,
}

#[derive(Debug, Clone)]
pub enum TasksOutput {}

pub struct TasksPage {
    tasks: Arc<TaskManager>,
    running: FactoryHashMap<TaskId, TaskRow>,
    finished: FactoryHashMap<TaskId, TaskRow>,
}

impl TasksPage {}

#[relm4::component(pub)]
impl Component for TasksPage {
    type Init = TasksInit;
    type Input = TasksInput;
    type Output = TasksOutput;
    type CommandOutput = TaskEvent;

    view! {
        #[root]
        gtk::Stack {
            set_transition_type: gtk::StackTransitionType::Crossfade,
            add_named[Some("empty")] = &adw::StatusPage {
            set_vexpand: true,
                add_css_class: "compact",
                set_icon_name: Some("folder-download-symbolic"),
                #[watch]
                set_title: &t!("tasks-empty-title", "No Tasks"),
                #[watch]
                set_description: Some(&t!(
                    "tasks-empty-description",
                    "Downloads and instance installations will appear here"
                )),
            },

            add_named[Some("list")] = &adw::PreferencesPage {
                #[local_ref]
                add = running_group -> adw::PreferencesGroup {
                    #[watch]
                    set_title: &t!("tasks-group-running", "In Progress"),
                    #[watch]
                    set_visible: !model.running.is_empty(),
                },

                #[local_ref]
                add = finished_group -> adw::PreferencesGroup {
                    #[watch]
                    set_title: &t!("tasks-group-finished", "Finished"),
                    #[watch]
                    set_visible: !model.finished.is_empty(),

                    #[wrap(Some)]
                    set_header_suffix = &gtk::Button {
                        add_css_class: "flat",
                        set_valign: gtk::Align::Center,
                        #[watch]
                        set_label: &t!("tasks-button-clear", "Clear Finished"),
                        connect_clicked => TasksInput::ClearFinished,
                    },
                },
            },

            #[watch]
            set_visible_child_name: if model.running.is_empty() && model.finished.is_empty() {
                                        "empty"
                                    } else {
                                        "list"
                                    },

        },
    }

    fn init(
        init: Self::Init,
        root: Self::Root,
        sender: ComponentSender<Self>,
    ) -> ComponentParts<Self> {
        init.config
            .subscribe(sender.input_sender(), |_| TasksInput::Refresh);

        let forward_action = |(id, action)| TasksInput::Action(id, action);
        let running = FactoryHashMap::builder()
            .launch(adw::PreferencesGroup::default())
            .forward(sender.input_sender(), forward_action);

        let finished = FactoryHashMap::builder()
            .launch(adw::PreferencesGroup::default())
            .forward(sender.input_sender(), forward_action);

        // Start the subscribed asynchronous task and continue to receive snapshots
        let mut subscription = init.tasks.subscribe();
        sender.command(move |out, shutdown| {
            shutdown
                .register(async move {
                    while let Some(event) = subscription.next().await {
                        if out.send(event).is_err() {
                            break;
                        }
                    }
                })
                .drop_on_shutdown()
        });

        let model = Self {
            tasks: init.tasks,
            running,
            finished,
        };

        let running_group = model.running.widget();
        let finished_group = model.finished.widget();
        let widgets = view_output!();

        ComponentParts { model, widgets }
    }

    fn update(&mut self, msg: Self::Input, _sender: ComponentSender<Self>, _root: &Self::Root) {
        match msg {
            TasksInput::Action(id, action) => match action {
                TaskRowOutput::Cancel => self.tasks.cancel(id),
                TaskRowOutput::Dismiss => self.tasks.remove(id),
                TaskRowOutput::Retry => self.tasks.retry(id),
            },
            TasksInput::ClearFinished => self.tasks.clear_finished(),
            TasksInput::Refresh => {
                self.finished.broadcast(TaskRowInput::Refresh);
                self.running.broadcast(TaskRowInput::Refresh);
            }
        }
    }

    fn update_cmd(
        &mut self,
        msg: Self::CommandOutput,
        _sender: ComponentSender<Self>,
        _root: &Self::Root,
    ) {
        match msg {
            TaskEvent::Removed(id) => {
                self.running.remove(&id);
                self.finished.remove(&id);
            }
            TaskEvent::Upsert(snap) => {
                let id = snap.id;
                let (other, target) = if snap.state.is_finished() {
                    (&mut self.running, &mut self.finished)
                } else {
                    (&mut self.finished, &mut self.running)
                };

                // If the task moved between active and finished (completed or retried), remove from the old group.
                other.remove(&id);

                if target.get(&id).is_some() {
                    target.send(&id, TaskRowInput::Update(snap));
                } else {
                    // Construct a TaskRow
                    target.insert(id, snap);
                }
            }
        }
    }
}
