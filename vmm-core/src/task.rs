//! Background task system for non-blocking long-running operations.
//!
//! Borrowed from Proxmox's task/UPID pattern: every long operation gets
//! a unique task ID with progress tracking and completion notification.
//! Uses std::thread (not tokio) to match the existing VNC thread pattern.

use std::sync::{Arc, Mutex};
use std::thread::{self, JoinHandle};
use uuid::Uuid;

/// A task's current state.
#[derive(Debug, Clone, PartialEq)]
pub enum TaskStatus {
    /// Task is queued / waiting to run.
    Pending,
    /// Task is actively running.
    Running,
    /// Task completed successfully.
    Completed,
    /// Task failed with an error message.
    Failed(String),
    /// Task was cancelled by the user.
    Cancelled,
}

/// Progress information for a running task.
#[derive(Debug, Clone)]
pub struct TaskProgress {
    /// 0.0 to 1.0 (or -1.0 for indeterminate).
    pub fraction: f64,
    /// Human-readable progress message (e.g., "Copying disk... 45%").
    pub message: String,
}

impl Default for TaskProgress {
    fn default() -> Self {
        Self {
            fraction: -1.0, // indeterminate
            message: String::new(),
        }
    }
}

/// A background task that can be tracked.
#[derive(Debug, Clone)]
pub struct TaskInfo {
    /// Unique task ID.
    pub id: Uuid,
    /// Human-readable description (e.g., "Cloning VM 'AegisOS'").
    pub description: String,
    /// Category for grouping (e.g., "clone", "export", "import", "backup").
    pub category: String,
    /// When the task was created.
    pub created_at: std::time::Instant,
    /// Current status.
    pub status: TaskStatus,
    /// Progress (updated by the running task).
    pub progress: TaskProgress,
}

/// Shared state for a single task — accessible from both the worker thread
/// and the GUI thread.
#[derive(Debug, Clone)]
pub struct TaskHandle {
    inner: Arc<Mutex<TaskInfo>>,
}

impl TaskHandle {
    fn new(info: TaskInfo) -> Self {
        Self {
            inner: Arc::new(Mutex::new(info)),
        }
    }

    /// Lock the task state. Returns None if the mutex is poisoned.
    ///
    /// SECURITY: CWE-662 (Improper Synchronization) — A poisoned mutex means a prior
    /// panic left state potentially corrupted. Recovering via `into_inner()` would
    /// silently use corrupted data (e.g., wrong status, garbled progress), which could
    /// cause the GUI to display misleading info or the worker to skip cancellation checks.
    /// Instead, we treat poison as a dead task.
    fn lock(&self) -> Option<std::sync::MutexGuard<'_, TaskInfo>> {
        match self.inner.lock() {
            Ok(guard) => Some(guard),
            Err(_poison) => {
                tracing::error!(
                    "Task mutex poisoned — prior panic left state corrupted (CWE-662). \
                     Treating task as dead."
                );
                None
            },
        }
    }

    /// Get a snapshot of the task info.
    /// Returns a failed-state placeholder if the mutex is poisoned (CWE-662).
    pub fn info(&self) -> TaskInfo {
        match self.lock() {
            Some(info) => info.clone(),
            None => TaskInfo {
                id: Uuid::nil(),
                description: "(task state corrupted)".to_string(),
                category: String::new(),
                created_at: std::time::Instant::now(),
                status: TaskStatus::Failed("Internal error: task state corrupted".to_string()),
                progress: TaskProgress::default(),
            },
        }
    }

    /// Update progress (called from worker thread).
    /// SECURITY: CWE-662 — No-op on poisoned mutex rather than using corrupted state.
    pub fn set_progress(&self, fraction: f64, message: impl Into<String>) {
        if let Some(mut info) = self.lock() {
            info.progress.fraction = fraction;
            info.progress.message = message.into();
        }
    }

    /// Mark task as completed (called from worker thread).
    /// SECURITY: CWE-662 — No-op on poisoned mutex rather than using corrupted state.
    pub fn set_completed(&self) {
        if let Some(mut info) = self.lock() {
            info.status = TaskStatus::Completed;
            info.progress.fraction = 1.0;
        }
    }

    /// Mark task as failed (called from worker thread).
    /// SECURITY: CWE-662 — No-op on poisoned mutex rather than using corrupted state.
    pub fn set_failed(&self, error: impl Into<String>) {
        if let Some(mut info) = self.lock() {
            info.status = TaskStatus::Failed(error.into());
        }
    }

    /// Check if the task was cancelled (worker should check this periodically).
    /// SECURITY: CWE-662 — Returns true on poisoned mutex (safe side: stop work).
    pub fn is_cancelled(&self) -> bool {
        match self.lock() {
            Some(info) => info.status == TaskStatus::Cancelled,
            None => true, // Treat poisoned state as "stop working"
        }
    }

    /// Get the task ID.
    /// Returns Uuid::nil() if the mutex is poisoned (CWE-662).
    pub fn id(&self) -> Uuid {
        match self.lock() {
            Some(info) => info.id,
            None => Uuid::nil(),
        }
    }

    /// Get the task status.
    /// Returns Failed status if the mutex is poisoned (CWE-662).
    pub fn status(&self) -> TaskStatus {
        match self.lock() {
            Some(info) => info.status.clone(),
            None => TaskStatus::Failed("Internal error: task state corrupted".to_string()),
        }
    }

    /// Request cancellation from the GUI side.
    /// SECURITY: CWE-662 — No-op on poisoned mutex (task is already dead).
    pub fn request_cancel(&self) {
        if let Some(mut info) = self.lock() {
            if info.status == TaskStatus::Running || info.status == TaskStatus::Pending {
                info.status = TaskStatus::Cancelled;
            }
        }
    }
}

/// Manages all background tasks.
pub struct TaskManager {
    /// All tasks (active + completed, kept for history).
    tasks: Vec<TaskHandle>,
    /// JoinHandles for spawned threads — stored for cleanup (CWE-404).
    join_handles: Vec<(Uuid, JoinHandle<()>)>,
    /// Maximum number of completed tasks to keep in history.
    max_history: usize,
}

impl Default for TaskManager {
    fn default() -> Self {
        Self::new()
    }
}

impl TaskManager {
    pub fn new() -> Self {
        Self {
            tasks: Vec::new(),
            join_handles: Vec::new(),
            max_history: 50,
        }
    }

    /// Maximum number of concurrent active tasks to prevent resource exhaustion (CWE-400).
    const MAX_ACTIVE_TASKS: usize = 64;

    /// Spawn a new background task.
    /// The `work` closure receives a TaskHandle for progress updates.
    ///
    /// SECURITY: The closure is wrapped in `catch_unwind` so that panics in
    /// worker threads are caught and surfaced as `TaskStatus::Failed` instead
    /// of silently leaving a task stuck in `Running` forever (CWE-755).
    /// JoinHandles are stored for orderly cleanup (CWE-404).
    pub fn spawn<F>(&mut self, description: &str, category: &str, work: F) -> TaskHandle
    where
        F: FnOnce(TaskHandle) + Send + std::panic::UnwindSafe + 'static,
    {
        // SECURITY: Cap active tasks to prevent resource exhaustion (CWE-400)
        if self.active_count() >= Self::MAX_ACTIVE_TASKS {
            let handle = TaskHandle::new(TaskInfo {
                id: Uuid::new_v4(),
                description: description.to_string(),
                category: category.to_string(),
                created_at: std::time::Instant::now(),
                status: TaskStatus::Failed("Too many active tasks (max 64)".to_string()),
                progress: TaskProgress::default(),
            });
            tracing::warn!(
                "Task rejected: too many active tasks ({})",
                Self::MAX_ACTIVE_TASKS
            );
            return handle;
        }

        let info = TaskInfo {
            id: Uuid::new_v4(),
            description: description.to_string(),
            category: category.to_string(),
            created_at: std::time::Instant::now(),
            status: TaskStatus::Running,
            progress: TaskProgress::default(),
        };

        let handle = TaskHandle::new(info);
        let worker_handle = handle.clone();
        let task_id = handle.id();

        self.tasks.push(handle.clone());

        // SECURITY (CWE-755): Wrap work in catch_unwind so panics don't leave
        // a task permanently stuck in Running status.
        match thread::Builder::new()
            .name(format!("task-{}", task_id))
            .spawn(move || {
                let panic_handle = worker_handle.clone();
                let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                    work(worker_handle);
                }));
                if let Err(panic_info) = result {
                    let msg = if let Some(s) = panic_info.downcast_ref::<&str>() {
                        format!("Task panicked: {}", s)
                    } else if let Some(s) = panic_info.downcast_ref::<String>() {
                        format!("Task panicked: {}", s)
                    } else {
                        "Task panicked (unknown cause)".to_string()
                    };
                    tracing::error!("{}", msg);
                    panic_handle.set_failed(msg);
                }
            }) {
            Ok(jh) => {
                // SECURITY (CWE-404): Store JoinHandle for cleanup instead of dropping it.
                self.join_handles.push((task_id, jh));
            },
            Err(e) => {
                tracing::error!("Failed to spawn task thread: {}", e);
                handle.set_failed(format!("Thread spawn failed: {}", e));
            },
        }

        self.prune_history();
        // Clean up finished JoinHandles to avoid unbounded growth.
        self.reap_finished_threads();
        handle
    }

    /// Get all tasks (for the task panel to display).
    pub fn tasks(&self) -> &[TaskHandle] {
        &self.tasks
    }

    /// Get active (running/pending) tasks.
    pub fn active_tasks(&self) -> Vec<TaskHandle> {
        self.tasks
            .iter()
            .filter(|t| {
                let status = t.status();
                status == TaskStatus::Running || status == TaskStatus::Pending
            })
            .cloned()
            .collect()
    }

    /// Get the count of active tasks.
    pub fn active_count(&self) -> usize {
        self.tasks
            .iter()
            .filter(|t| {
                let status = t.status();
                status == TaskStatus::Running || status == TaskStatus::Pending
            })
            .count()
    }

    /// Cancel a task by ID (sets the cancelled flag, worker should check and stop).
    /// SECURITY: CWE-662 — Uses TaskHandle::request_cancel which handles poisoned mutex safely.
    pub fn cancel(&self, task_id: &Uuid) {
        if let Some(handle) = self.tasks.iter().find(|t| t.id() == *task_id) {
            handle.request_cancel();
        }
    }

    /// Remove completed/failed/cancelled tasks from history.
    pub fn clear_history(&mut self) {
        self.tasks.retain(|t| {
            let status = t.status();
            status == TaskStatus::Running || status == TaskStatus::Pending
        });
    }

    /// Reap JoinHandles for threads that have finished (CWE-404).
    /// This prevents unbounded growth of stored handles.
    fn reap_finished_threads(&mut self) {
        self.join_handles.retain(|(_, jh)| !jh.is_finished());
    }

    /// Join all remaining threads. Called on drop for orderly shutdown (CWE-404).
    pub fn join_all(&mut self) {
        for (id, jh) in self.join_handles.drain(..) {
            if let Err(e) = jh.join() {
                tracing::warn!("Task thread {:?} panicked during join: {:?}", id, e);
            }
        }
    }

    /// Prune old completed tasks if over the history limit.
    fn prune_history(&mut self) {
        let completed: Vec<usize> = self
            .tasks
            .iter()
            .enumerate()
            .filter(|(_, t)| {
                let status = t.status();
                status == TaskStatus::Completed
                    || matches!(status, TaskStatus::Failed(_))
                    || status == TaskStatus::Cancelled
            })
            .map(|(i, _)| i)
            .collect();

        if completed.len() > self.max_history {
            let remove_count = completed.len() - self.max_history;
            let to_remove: Vec<usize> = completed.into_iter().take(remove_count).collect();
            // Remove in reverse to maintain indices
            for i in to_remove.into_iter().rev() {
                self.tasks.remove(i);
            }
        }
    }
}

/// SECURITY (CWE-404): Ensure all spawned threads are joined on shutdown
/// to prevent resource leaks and allow orderly cleanup.
impl Drop for TaskManager {
    fn drop(&mut self) {
        self.join_all();
    }
}
