//! duraflow-rs — durable, resumable dag tasks built on top of `dagx`
//!
//! High level features:
//! - Durable task decorator that persists task outputs to a `Storage` backend
//! - `run_result` API for callers to observe persistence errors without changing `Task::run`
//! - Built-in `MemoryStore` and `FileStore` backends
//!
//! Example (simple):
//!
//! ```no_run
//! #[tokio::main]
//! async fn main() {
//!     use duraflow_rs::{DurableDag, Context, MemoryStore};
//!     use dagx::{DagRunner, task, Task};
//!     use std::sync::{Arc, atomic::AtomicUsize};
//!
//!     // define a task
//!     struct Load(i32);
//!     #[task]
//!     impl Load { async fn run(&self) -> i32 { self.0 } }
//!
//!     let dag = DagRunner::new();
//!     let db = Arc::new(MemoryStore::new());
//!     let ctx = Arc::new(Context { db, completed_count: Arc::new(AtomicUsize::new(0)) });
//!     let d = DurableDag::new(&dag, ctx.clone());
//!     let a = d.add("v1", Load(5));
//!     dag.run(|f| { tokio::spawn(f); }).await.unwrap();
//!     assert_eq!(dag.get(a).unwrap(), 5);
//! }
//! ```

use dagx::{DagRunner, Pending, Task, TaskBuilder};
use serde::{Serialize, de::DeserializeOwned};
use std::sync::Arc;
use std::sync::atomic::AtomicUsize;

mod store;
pub use store::{FileStore, MemoryStore, Storage};

/// Typed error for duraflow-rs operations
#[derive(Debug)]
pub enum DuraflowError {
    Io(std::io::Error),
    Serialize(serde_json::Error),
    Persist { key: String, source: std::io::Error },
    Other(String),
}

impl std::fmt::Display for DuraflowError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            DuraflowError::Io(e) => write!(f, "io error: {}", e),
            DuraflowError::Serialize(e) => write!(f, "serialize error: {}", e),
            DuraflowError::Persist { key, source } => {
                write!(f, "persist failed for {}: {}", key, source)
            }
            DuraflowError::Other(s) => write!(f, "{}", s),
        }
    }
}

impl std::error::Error for DuraflowError {}

impl From<std::io::Error> for DuraflowError {
    fn from(e: std::io::Error) -> Self {
        DuraflowError::Io(e)
    }
}

impl From<serde_json::Error> for DuraflowError {
    fn from(e: serde_json::Error) -> Self {
        DuraflowError::Serialize(e)
    }
}

/// Shared context for progress and durability
pub struct Context {
    pub db: Arc<dyn Storage + Send + Sync>,
    pub completed_count: Arc<AtomicUsize>,
}

impl Context {
    /// Typed helper: deserialize stored JSON into T
    pub fn get<T: DeserializeOwned>(&self, key: &str) -> Option<T> {
        self.db
            .get_raw(key)
            .and_then(|s| serde_json::from_str(&s).ok())
    }

    /// Typed helper: serialize value and persist as JSON
    /// Returns an io::Error if serialization or storage fails.
    pub fn save<T: Serialize>(&self, key: &str, value: &T) -> std::io::Result<()> {
        let s = serde_json::to_string(value).map_err(|e| {
            std::io::Error::other(format!("serialize error: {}", e))
        })?;
        self.db.save_raw(key, &s)
    }
}

// Internal progress callback shorthand
type ProgressCb = Arc<dyn Fn(&str, usize) + Send + Sync + 'static>;

// Helper: check cache and mark completion if present
fn try_cached_and_mark<O: DeserializeOwned>(
    ctx: &Context,
    id: &str,
    cb: &Option<ProgressCb>,
) -> Option<O> {
    if let Some(v) = ctx.get::<O>(id) {
        let completed = ctx
            .completed_count
            .fetch_add(1, std::sync::atomic::Ordering::SeqCst)
            + 1;
        if let Some(cb) = cb {
            cb(id, completed);
        }
        return Some(v);
    }
    None
}

// Helper: persist value and mark completion
fn persist_and_mark<O: Serialize>(
    ctx: &Context,
    id: &str,
    value: &O,
    cb: &Option<ProgressCb>,
) -> Result<(), DuraflowError> {
    ctx.save(id, value).map_err(|e| DuraflowError::Persist {
        key: id.to_string(),
        source: e,
    })?;
    let completed = ctx
        .completed_count
        .fetch_add(1, std::sync::atomic::Ordering::SeqCst)
        + 1;
    if let Some(cb) = cb {
        cb(id, completed);
    }
    Ok(())
}

// -----------------------------------------------------------------------------
// Durable decorator
// -----------------------------------------------------------------------------

/// The "Durable" decorator wraps any Task implementation and persists outputs.
pub struct Durable<Tk> {
    id: String,
    ctx: Arc<Context>,
    inner: Tk,
    progress_handler: Option<ProgressCb>,
}

impl<Tk> Durable<Tk> {
    pub fn new(
        id: &str,
        ctx: Arc<Context>,
        inner: Tk,
        progress_handler: Option<ProgressCb>,
    ) -> Self {
        Self {
            id: id.to_string(),
            ctx,
            inner,
            progress_handler,
        }
    }

    /// Run the task but return a Result so callers can observe persistence errors.
    pub async fn run_result(
        self,
        input: Tk::Input,
    ) -> Result<Tk::Output, DuraflowError>
    where
        Tk: Task + Send + 'static,
        Tk::Input: Send + Clone,
        Tk::Output: Serialize + DeserializeOwned + Send + Sync + 'static,
    {
        // 1. Cached path
        if let Some(cached) =
            try_cached_and_mark::<Tk::Output>(&self.ctx, &self.id, &self.progress_handler)
        {
            return Ok(cached);
        }

        // 2. Run inner
        let result = self.inner.run(input).await;

        // 3. Persist — propagate typed error to caller
        persist_and_mark(&self.ctx, &self.id, &result, &self.progress_handler)?;

        Ok(result)
    }
}

impl<Tk> Task for Durable<Tk>
where
    Tk: Task + Send + 'static,
    Tk::Input: Send + Clone,
    Tk::Output: Serialize + DeserializeOwned + Send + Sync + 'static,
{
    type Input = Tk::Input;
    type Output = Tk::Output;

    #[allow(clippy::manual_async_fn)]
    fn run(self, input: Self::Input) -> impl std::future::Future<Output = Self::Output> + Send {
        async move {
            // 1. Check cache (DRY via helper)
            if let Some(cached) =
                try_cached_and_mark::<Self::Output>(&self.ctx, &self.id, &self.progress_handler)
            {
                return cached;
            }

            // 2. Run inner task
            let result = self.inner.run(input).await;

            // 3. Persist + progress — do not panic, log on failure
            if let Err(e) = persist_and_mark(&self.ctx, &self.id, &result, &self.progress_handler) {
                eprintln!("persist failed for {}: {}", self.id, e);
            }

            result
        }
    }

    fn extract_and_run(
        self,
        receivers: Vec<Box<dyn std::any::Any + Send>>,
    ) -> impl std::future::Future<Output = Result<Self::Output, String>> + Send {
        let id = self.id.clone();
        let ctx = self.ctx.clone();
        let progress = self.progress_handler.clone();
        let inner = self.inner;

        async move {
            // 1. Cached path (DRY via helper)
            if let Some(cached) = try_cached_and_mark::<Self::Output>(&ctx, &id, &progress) {
                return Ok(cached);
            }

            // 2. Delegate to inner extraction
            let result = inner.extract_and_run(receivers).await?;

            // 3. Persist and update — propagate storage error to caller (map to string)
            if let Err(e) = persist_and_mark(&ctx, &id, &result, &progress) {
                return Err(e.to_string());
            }

            Ok(result)
        }
    }
}

// -----------------------------------------------------------------------------
// DurableDag builder wrapper
// -----------------------------------------------------------------------------

pub struct DurableDag<'a> {
    pub dag: &'a DagRunner,
    pub ctx: Arc<Context>,
    pub progress_handler: Option<ProgressCb>,
}

impl<'a> DurableDag<'a> {
    pub fn new(dag: &'a DagRunner, ctx: Arc<Context>) -> Self {
        Self {
            dag,
            ctx,
            progress_handler: None,
        }
    }

    /// Attach a progress handler that will be called with (task_id, completed_count)
    pub fn with_progress<F>(mut self, handler: F) -> Self
    where
        F: Fn(&str, usize) + Send + Sync + 'static,
    {
        self.progress_handler = Some(Arc::new(handler));
        self
    }

    /// Adds a durable task and returns a standard TaskBuilder.
    /// This allows you to chain .depends_on() just like normal dagx.
    pub fn add<Tk>(&self, id: &str, task: Tk) -> TaskBuilder<'_, Durable<Tk>, Pending>
    where
        Tk: Task + Sync + 'static,
        Tk::Input: Clone + 'static,
        Tk::Output: Serialize + DeserializeOwned + Send + Sync + Clone + 'static,
    {
        let durable_wrapper =
            Durable::new(id, self.ctx.clone(), task, self.progress_handler.clone());
        self.dag.add_task(durable_wrapper)
    }
}
