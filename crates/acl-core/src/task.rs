// SPDX-FileCopyrightText: 2026 The Adwaita Craft Launcher Authors
// SPDX-License-Identifier: GPL-3.0-or-later

use std::{
    collections::{HashMap, VecDeque},
    sync::{
        Arc, Mutex,
        atomic::{AtomicU64, Ordering},
    },
    time::Duration,
};

use tokio::{
    sync::{Notify, broadcast},
    task::AbortHandle,
};

pub type TaskId = u64;

/// Point-in-time snapshot of a background task sent to the UI for rendering.
#[derive(Debug, Clone)]
pub struct TaskSnapshot {
    /// Unique task identifier, used as the key in both `TaskManager` and UI `FactoryHashMap`.
    pub id: TaskId,
    /// Display title shown on the main row (e.g., instance name or modpack name).
    pub title: String,
    /// Ordered list of steps in this task; length stays fixed after creation so UI rows can update via `zip`.
    pub stages: Vec<StageSnapshot>,
    /// Overall task state, used by the UI to place the row in "In Progress" or "Finished" and pick the action button.
    pub state: TaskState,
    /// The total time spent on this task
    pub elapsed_secs: Option<u64>,
}

/// Point-in-time snapshot of a single stage inside a task.
#[derive(Debug, Clone)]
pub struct StageSnapshot {
    /// Display name of this step shown in the expanded area (already localized by the caller).
    pub label: String,
    /// Step state (`Pending` / `Running` / `Done` / `Failed` / `Cancelled`), driving the row's status icon.
    pub status: StageStatus,
    /// Completed work units in this step (e.g., downloaded file count).
    pub done: u64,
    /// Total work units in this step (`<= 1` means single-step operation; UI hides the progress bar and counter).
    pub total: u64,
    /// Current download throughput in bytes/sec during download stages; `None` for non-download steps.
    pub bytes_per_sec: Option<u64>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StageStatus {
    Pending,
    Running,
    Done,
    Cancelled,
    Failed,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TaskState {
    Running,
    Succeeded,
    Failed(String),
    Cancelled,
}

impl TaskState {
    pub fn is_finished(&self) -> bool {
        !matches!(self, Self::Running)
    }
}

#[derive(Debug, Clone)]
pub enum TaskEvent {
    Upsert(TaskSnapshot),
    /// Used to notify UI to delete tasks
    Removed(TaskId),
}

/// Internal record for a single task managed by `TaskManager`.
struct TaskRecord {
    /// Latest state and stage progress of this task.
    snapshot: TaskSnapshot,
    /// Marks whether progress changed since the last 100ms broadcast flush.
    dirty: bool,
    /// Handle used to abort the running tokio task on cancellation.
    abort_handle: Option<AbortHandle>,
    /// Closure that spawns (or retries) the task and returns its abort handle.
    spawn: Box<dyn Fn() -> AbortHandle + Send + Sync>,
    /// Task start time
    started_at: std::time::Instant,
}

/// Shared background task manager
pub struct TaskManager {
    /// Monotonic counter generating unique `TaskId` values.
    next_id: AtomicU64,
    /// All active and finished tasks keyed by `TaskId`.
    tasks: Mutex<HashMap<TaskId, TaskRecord>>,
    /// Broadcast channel delivering `TaskEvent` updates to UI subscribers.
    events: broadcast::Sender<TaskEvent>,
    /// Wakes the 100ms background flusher when any task becomes dirty.
    flush_notify: Arc<Notify>,
}

impl TaskManager {
    /// The caller needs to have entered the Tokio runtime
    pub fn new() -> Arc<Self> {
        let (events, _) = broadcast::channel(256);
        let flush_notify = Arc::new(Notify::new());

        let manager = Arc::new(Self {
            next_id: AtomicU64::new(1),
            tasks: Mutex::new(HashMap::new()),
            events,
            flush_notify: Arc::clone(&flush_notify),
        });

        // Weak reference to allow TaskManager to be dropped
        let weak = Arc::downgrade(&manager);
        tokio::spawn(async move {
            loop {
                flush_notify.notified().await;
                // Notify once every 100ms
                tokio::time::sleep(Duration::from_millis(100)).await;
                let Some(tm) = weak.upgrade() else {
                    break;
                };
                tm.flush_dirty();
            }
        });

        manager
    }

    /// Forcefully take the task inside the lock
    fn lock_tasks(&self) -> std::sync::MutexGuard<'_, HashMap<TaskId, TaskRecord>> {
        self.tasks.lock().unwrap_or_else(|e| e.into_inner())
    }

    fn flush_dirty(&self) {
        let mut tasks = self.lock_tasks();
        let snapshots: Vec<TaskSnapshot> = {
            tasks
                .values_mut()
                .filter(|record| record.dirty)
                .map(|record| {
                    record.dirty = false;
                    record.snapshot.clone()
                })
                .collect()
        };

        for snapshot in snapshots {
            let _ = self.events.send(TaskEvent::Upsert(snapshot));
        }
    }

    pub fn subscribe(self: &Arc<Self>) -> TaskSubscription {
        let tasks = self.lock_tasks();
        let rx = self.events.subscribe();
        let pending = tasks
            .values()
            .map(|record| TaskEvent::Upsert(record.snapshot.clone()))
            .collect();
        TaskSubscription {
            manager: Arc::clone(self),
            rx,
            pending,
        }
    }

    pub fn submit<F, Fut>(
        self: &Arc<Self>,
        title: impl Into<String>,
        stage_labels: Vec<String>,
        run: F,
    ) -> TaskId
    where
        // Stereotyped writing
        // It is no match for concept
        F: Fn(TaskId, Arc<Self>) -> Fut + Send + Sync + 'static,
        Fut: Future<Output = Result<(), String>> + Send + 'static,
    {
        let id = self.next_id.fetch_add(1, Ordering::Relaxed);

        let stages = stage_labels
            .into_iter()
            .map(|label| StageSnapshot {
                label,
                status: StageStatus::Pending,
                done: 0,
                total: 0,
                bytes_per_sec: None,
            })
            .collect();

        let snapshot = TaskSnapshot {
            id,
            title: title.into(),
            stages,
            state: TaskState::Running,
            elapsed_secs: None,
        };

        let run = Arc::new(run);
        let weak = Arc::downgrade(self);
        let spawn: Box<dyn Fn() -> AbortHandle + Send + Sync> = Box::new(move || {
            let run = Arc::clone(&run);
            let weak = weak.clone();
            tokio::spawn(async move {
                let Some(tm) = weak.upgrade() else {
                    return;
                };
                let result = run(id, Arc::clone(&tm)).await;
                tm.finish(id, result);
            })
            .abort_handle()
        });

        let mut tasks = self.lock_tasks();
        let abort_handle = spawn();
        let _ = self.events.send(TaskEvent::Upsert(snapshot.clone()));
        tasks.insert(
            id,
            TaskRecord {
                snapshot,
                dirty: false,
                abort_handle: Some(abort_handle),
                spawn,
                started_at: std::time::Instant::now(),
            },
        );

        id
    }

    fn finish(&self, id: TaskId, result: Result<(), String>) {
        let mut tasks = self.lock_tasks();
        let Some(record) = tasks.get_mut(&id) else {
            return;
        };

        // Guard against racing with `cancel` right as the task completes.
        if record.snapshot.state != TaskState::Running {
            return;
        }

        match result {
            Ok(()) => {
                for stage in &mut record.snapshot.stages {
                    if matches!(stage.status, StageStatus::Pending | StageStatus::Running) {
                        stage.status = StageStatus::Done;
                    }
                }
                record.snapshot.state = TaskState::Succeeded;
            }
            Err(err) => {
                for stage in &mut record.snapshot.stages {
                    if stage.status == StageStatus::Running {
                        stage.status = StageStatus::Failed;
                    }
                }
                record.snapshot.state = TaskState::Failed(err);
            }
        }

        record.snapshot.elapsed_secs = Some(record.started_at.elapsed().as_secs());
        record.abort_handle = None;
        record.dirty = false;
        let _ = self.events.send(TaskEvent::Upsert(record.snapshot.clone()));
    }

    pub fn cancel(&self, id: TaskId) {
        let mut tasks = self.lock_tasks();
        let Some(record) = tasks.get_mut(&id) else {
            return;
        };
        if record.snapshot.state != TaskState::Running {
            return;
        }
        if let Some(handle) = record.abort_handle.take() {
            handle.abort();
        }

        record.snapshot.stages.iter_mut().for_each(|stage| {
            if stage.status == StageStatus::Running {
                stage.status = StageStatus::Cancelled;
            }
        });

        record.snapshot.state = TaskState::Cancelled;
        record.snapshot.elapsed_secs = Some(record.started_at.elapsed().as_secs());
        record.dirty = false;
        let _ = self.events.send(TaskEvent::Upsert(record.snapshot.clone()));
    }

    pub fn retry(&self, id: TaskId) {
        let mut tasks = self.lock_tasks();
        let Some(record) = tasks.get_mut(&id) else {
            return;
        };

        if !matches!(
            record.snapshot.state,
            TaskState::Failed(_) | TaskState::Cancelled
        ) {
            return;
        }

        for stage in &mut record.snapshot.stages {
            stage.status = StageStatus::Pending;
            stage.done = 0;
            stage.total = 0;
            stage.bytes_per_sec = None;
        }

        record.snapshot.state = TaskState::Running;
        record.snapshot.elapsed_secs = None;
        record.dirty = false;
        record.started_at = std::time::Instant::now();
        record.abort_handle = Some((record.spawn)());
        let _ = self.events.send(TaskEvent::Upsert(record.snapshot.clone()));
    }

    pub fn remove(&self, id: TaskId) {
        let mut tasks = self.lock_tasks();
        let can_remove = tasks
            .get(&id)
            .is_some_and(|record| record.snapshot.state.is_finished());
        if can_remove && tasks.remove(&id).is_some() {
            let _ = self.events.send(TaskEvent::Removed(id));
        }
    }

    pub fn clear_finished(&self) {
        let mut tasks = self.lock_tasks();
        let finished_ids: Vec<TaskId> = tasks
            .iter()
            .filter(|(_, record)| record.snapshot.state.is_finished())
            .map(|(id, _)| *id)
            .collect();

        for id in finished_ids {
            if tasks.remove(&id).is_some() {
                let _ = self.events.send(TaskEvent::Removed(id));
            }
        }
    }

    pub fn start_stage(&self, id: TaskId, stage_idx: usize) {
        let mut tasks = self.lock_tasks();
        let Some(record) = tasks.get_mut(&id) else {
            return;
        };
        if record.snapshot.state != TaskState::Running {
            return;
        }

        for (idx, stage) in record.snapshot.stages.iter_mut().enumerate() {
            if idx < stage_idx
                && matches!(stage.status, StageStatus::Pending | StageStatus::Running)
            {
                stage.status = StageStatus::Done;
            } else if idx == stage_idx {
                stage.status = StageStatus::Running;
            }
        }

        record.dirty = false;
        let _ = self.events.send(TaskEvent::Upsert(record.snapshot.clone()));
    }

    pub fn update_stage(
        &self,
        id: TaskId,
        stage_idx: usize,
        done: u64,
        total: u64,
        bytes_per_sec: Option<u64>,
    ) {
        let mut tasks = self.lock_tasks();
        let Some(record) = tasks.get_mut(&id) else {
            return;
        };
        if record.snapshot.state != TaskState::Running {
            return;
        }

        if let Some(stage) = record.snapshot.stages.get_mut(stage_idx) {
            if stage.status == StageStatus::Pending {
                stage.status = StageStatus::Running;
            }
            stage.done = done;
            stage.total = total;
            stage.bytes_per_sec = bytes_per_sec;
            record.dirty = true;
            self.flush_notify.notify_one();
        }
    }
}

impl Drop for TaskManager {
    fn drop(&mut self) {
        self.flush_notify.notify_one();
    }
}

/// Subscriber stream that yields an initial snapshot of existing tasks followed by live events,
/// automatically resynchronizing if the broadcast buffer lags.
pub struct TaskSubscription {
    manager: Arc<TaskManager>,
    rx: broadcast::Receiver<TaskEvent>,
    pending: VecDeque<TaskEvent>,
}

impl TaskSubscription {
    /// Returns the next task event, or `None` once the `TaskManager` has been dropped.
    /// This ensures that even if the channel is full, it will not crash if a message is missed.
    pub async fn next(&mut self) -> Option<TaskEvent> {
        loop {
            if let Some(ev) = self.pending.pop_front() {
                return Some(ev);
            }

            match self.rx.recv().await {
                Ok(ev) => return Some(ev),
                Err(broadcast::error::RecvError::Lagged(_)) => {
                    let tasks = self.manager.lock_tasks();
                    self.pending.extend(
                        tasks
                            .values()
                            .map(|record| TaskEvent::Upsert(record.snapshot.clone())),
                    );
                }
                Err(broadcast::error::RecvError::Closed) => return None,
            }
        }
    }
}
