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

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicBool, AtomicUsize};

    async fn next_event(sub: &mut TaskSubscription) -> TaskEvent {
        tokio::time::timeout(Duration::from_secs(2), sub.next())
            .await
            .expect("timed out waiting for task event")
            .expect("task subscription closed unexpectedly")
    }

    #[test]
    fn test_task_state_is_finished() {
        assert!(!TaskState::Running.is_finished());
        assert!(TaskState::Succeeded.is_finished());
        assert!(TaskState::Failed("err".to_string()).is_finished());
        assert!(TaskState::Cancelled.is_finished());
    }

    #[tokio::test]
    async fn test_submit_start_stage_and_finish_success() {
        let tm = TaskManager::new();
        let mut sub = tm.subscribe();

        let (done_tx, done_rx) = tokio::sync::oneshot::channel::<()>();
        let done_rx = Arc::new(Mutex::new(Some(done_rx)));

        let task_id = tm.submit(
            "Install 1.21.1",
            vec![
                "Metadata".to_string(),
                "Assets".to_string(),
                "Download".to_string(),
            ],
            move |id, mgr| {
                let done_rx = Arc::clone(&done_rx);
                async move {
                    mgr.start_stage(id, 0);
                    mgr.start_stage(id, 1);
                    let rx = done_rx.lock().unwrap_or_else(|e| e.into_inner()).take();
                    if let Some(rx) = rx {
                        let _ = rx.await;
                    }
                    Ok(())
                }
            },
        );

        let second_id = tm.submit("No-op", vec![], |_, _| async { Ok(()) });
        assert!(second_id > task_id);

        // Drain events until task_id reaches Running stage 1, then release completion.
        let mut saw_running_initial = false;
        loop {
            if let TaskEvent::Upsert(snap) = next_event(&mut sub).await
                && snap.id == task_id
            {
                if snap.stages[0].status == StageStatus::Pending {
                    assert_eq!(snap.title, "Install 1.21.1");
                    assert_eq!(snap.state, TaskState::Running);
                    assert_eq!(snap.elapsed_secs, None);
                    saw_running_initial = true;
                } else if snap.stages[1].status == StageStatus::Running {
                    assert_eq!(snap.stages[0].status, StageStatus::Done);
                    assert_eq!(snap.stages[2].status, StageStatus::Pending);
                    break;
                }
            }
        }
        assert!(saw_running_initial);

        let _ = done_tx.send(());

        loop {
            if let TaskEvent::Upsert(snap) = next_event(&mut sub).await
                && snap.id == task_id
                && snap.state == TaskState::Succeeded
            {
                assert!(snap.elapsed_secs.is_some());
                assert!(snap.stages.iter().all(|s| s.status == StageStatus::Done));
                break;
            }
        }
    }

    #[tokio::test]
    async fn test_finish_failure_and_retry() {
        let tm = TaskManager::new();
        let mut sub = tm.subscribe();
        let attempts = Arc::new(AtomicUsize::new(0));

        let task_id = tm.submit(
            "Install Fabric",
            vec!["Step 1".to_string(), "Step 2".to_string()],
            {
                let attempts = Arc::clone(&attempts);
                move |id, mgr| {
                    let attempts = Arc::clone(&attempts);
                    async move {
                        let attempt = attempts.fetch_add(1, Ordering::SeqCst);
                        mgr.start_stage(id, 0);
                        if attempt == 0 {
                            mgr.update_stage(id, 0, 5, 10, Some(1024));
                            Err("network error".to_string())
                        } else {
                            mgr.start_stage(id, 1);
                            Ok(())
                        }
                    }
                }
            },
        );

        loop {
            if let TaskEvent::Upsert(snap) = next_event(&mut sub).await
                && snap.id == task_id
                && snap.state == TaskState::Failed("network error".to_string())
            {
                assert_eq!(snap.stages[0].status, StageStatus::Failed);
                assert_eq!(snap.stages[1].status, StageStatus::Pending);
                assert!(snap.elapsed_secs.is_some());
                break;
            }
        }

        tm.retry(task_id);

        let mut saw_reset = false;
        loop {
            if let TaskEvent::Upsert(snap) = next_event(&mut sub).await
                && snap.id == task_id
            {
                if snap.state == TaskState::Running && snap.stages[0].status == StageStatus::Pending
                {
                    assert_eq!(snap.stages[0].done, 0);
                    assert_eq!(snap.stages[0].total, 0);
                    assert_eq!(snap.stages[0].bytes_per_sec, None);
                    assert_eq!(snap.elapsed_secs, None);
                    saw_reset = true;
                } else if snap.state == TaskState::Succeeded {
                    assert!(saw_reset);
                    assert_eq!(snap.stages[0].status, StageStatus::Done);
                    assert_eq!(snap.stages[1].status, StageStatus::Done);
                    break;
                }
            }
        }

        assert_eq!(attempts.load(Ordering::SeqCst), 2);

        // Retrying a succeeded task should be a no-op.
        tm.retry(task_id);
        tokio::task::yield_now().await;
        assert_eq!(attempts.load(Ordering::SeqCst), 2);
    }

    #[tokio::test]
    async fn test_cancel_aborts_task_and_guards_against_finish() {
        struct DropFlag(Arc<AtomicBool>);
        impl Drop for DropFlag {
            fn drop(&mut self) {
                self.0.store(true, Ordering::SeqCst);
            }
        }

        let tm = TaskManager::new();
        let mut sub = tm.subscribe();
        let started = Arc::new(Notify::new());
        let dropped = Arc::new(AtomicBool::new(false));

        let task_id = tm.submit(
            "Long Task",
            vec!["Stage A".to_string(), "Stage B".to_string()],
            {
                let started = Arc::clone(&started);
                let dropped = Arc::clone(&dropped);
                move |id, mgr| {
                    let started = Arc::clone(&started);
                    let dropped = Arc::clone(&dropped);
                    async move {
                        let _guard = DropFlag(dropped);
                        mgr.start_stage(id, 0);
                        started.notify_one();
                        std::future::pending::<()>().await;
                        Ok(())
                    }
                }
            },
        );

        tokio::time::timeout(Duration::from_secs(2), started.notified())
            .await
            .expect("task should start");

        tm.cancel(task_id);

        loop {
            if let TaskEvent::Upsert(snap) = next_event(&mut sub).await
                && snap.id == task_id
                && snap.state == TaskState::Cancelled
            {
                assert_eq!(snap.stages[0].status, StageStatus::Cancelled);
                assert_eq!(snap.stages[1].status, StageStatus::Pending);
                assert!(snap.elapsed_secs.is_some());
                break;
            }
        }

        // Give the Tokio scheduler a moment to drop the aborted future.
        for _ in 0..10 {
            if dropped.load(Ordering::SeqCst) {
                break;
            }
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
        assert!(dropped.load(Ordering::SeqCst));

        // Late completion or repeat cancel must not alter Cancelled state.
        tm.finish(task_id, Ok(()));
        tm.finish(task_id, Err("late error".to_string()));
        tm.cancel(task_id);
        let tasks = tm.lock_tasks();
        assert_eq!(
            tasks.get(&task_id).map(|r| &r.snapshot.state),
            Some(&TaskState::Cancelled)
        );
    }

    #[tokio::test]
    async fn test_update_stage_flushes_and_remove_clear_finished() {
        let tm = TaskManager::new();
        let mut sub = tm.subscribe();

        let running_started = Arc::new(Notify::new());
        let running_release = Arc::new(Notify::new());

        let running_id = tm.submit("Running Task", vec!["Download".to_string()], {
            let running_started = Arc::clone(&running_started);
            let running_release = Arc::clone(&running_release);
            move |id, mgr| {
                let running_started = Arc::clone(&running_started);
                let running_release = Arc::clone(&running_release);
                async move {
                    mgr.update_stage(id, 0, 3, 10, Some(2048));
                    running_started.notify_one();
                    running_release.notified().await;
                    Ok(())
                }
            }
        });

        tokio::time::timeout(Duration::from_secs(2), running_started.notified())
            .await
            .expect("running task should update stage");

        // update_stage sets dirty and flushes within ~100ms.
        loop {
            if let TaskEvent::Upsert(snap) = next_event(&mut sub).await
                && snap.id == running_id
                && snap.stages[0].done == 3
            {
                assert_eq!(snap.stages[0].status, StageStatus::Running);
                assert_eq!(snap.stages[0].total, 10);
                assert_eq!(snap.stages[0].bytes_per_sec, Some(2048));
                break;
            }
        }

        // Removing a running task must be ignored.
        tm.remove(running_id);
        assert!(tm.lock_tasks().contains_key(&running_id));

        let done_id_1 = tm.submit("Done 1", vec![], |_, _| async { Ok(()) });
        let done_id_2 = tm.submit("Done 2", vec![], |_, _| async { Err("fail".to_string()) });

        let mut seen_done_1 = false;
        let mut seen_done_2 = false;
        while !(seen_done_1 && seen_done_2) {
            if let TaskEvent::Upsert(snap) = next_event(&mut sub).await {
                if snap.id == done_id_1 && snap.state == TaskState::Succeeded {
                    seen_done_1 = true;
                } else if snap.id == done_id_2
                    && snap.state == TaskState::Failed("fail".to_string())
                {
                    seen_done_2 = true;
                }
            }
        }

        tm.remove(done_id_1);
        loop {
            if let TaskEvent::Removed(id) = next_event(&mut sub).await
                && id == done_id_1
            {
                break;
            }
        }
        assert!(!tm.lock_tasks().contains_key(&done_id_1));

        tm.clear_finished();
        loop {
            if let TaskEvent::Removed(id) = next_event(&mut sub).await
                && id == done_id_2
            {
                break;
            }
        }
        assert!(!tm.lock_tasks().contains_key(&done_id_2));
        assert!(tm.lock_tasks().contains_key(&running_id));

        running_release.notify_one();
        loop {
            if let TaskEvent::Upsert(snap) = next_event(&mut sub).await
                && snap.id == running_id
                && snap.state == TaskState::Succeeded
            {
                break;
            }
        }
    }

    #[tokio::test]
    async fn test_subscribe_replays_existing_tasks() {
        let tm = TaskManager::new();
        let existing_id = tm.submit("Existing", vec!["Only".to_string()], |_, _| async {
            Ok(())
        });

        // Wait until the pre-existing task completes before subscribing.
        for _ in 0..50 {
            let done = tm
                .lock_tasks()
                .get(&existing_id)
                .is_some_and(|r| r.snapshot.state == TaskState::Succeeded);
            if done {
                break;
            }
            tokio::time::sleep(Duration::from_millis(5)).await;
        }

        let mut sub = tm.subscribe();
        match next_event(&mut sub).await {
            TaskEvent::Upsert(snap) => {
                assert_eq!(snap.id, existing_id);
                assert_eq!(snap.title, "Existing");
                assert_eq!(snap.state, TaskState::Succeeded);
            }
            other => panic!("expected Upsert replay, got {other:?}"),
        }
    }
}
