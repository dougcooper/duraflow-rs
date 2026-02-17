use dagx::{task, DagRunner, Task, TaskBuilder, Pending};
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::collections::HashMap;
use parking_lot::Mutex;
use serde::{Serialize, de::DeserializeOwned};

// ============================================================================
// 1. SHARED INFRASTRUCTURE
// ============================================================================

/// Mock database for persistence.
struct Db {
    storage: Mutex<HashMap<String, String>>,
}

impl Db {
    fn new() -> Self {
        Self { storage: Mutex::new(HashMap::new()) }
    }
    
    fn get<T: DeserializeOwned>(&self, key: &str) -> Option<T> {
        let lock = self.storage.lock();
        lock.get(key).and_then(|s| serde_json::from_str(s).ok())
    }
    
    fn save<T: Serialize>(&self, key: &str, value: &T) {
        let s = serde_json::to_string(value).unwrap();
        self.storage.lock().insert(key.to_string(), s);
    }
}

/// Shared context for progress and durability
struct Context {
    db: Arc<Db>,
    completed_count: Arc<AtomicUsize>,
}

// ============================================================================
// 2. THE GENERALIZED DECORATOR
// ============================================================================

/// The "Durable" decorator wraps any Task implementation.
struct Durable<Tk> {
    id: String,
    ctx: Arc<Context>,
    inner: Tk,
    progress_handler: Option<Arc<dyn Fn(&str, usize) + Send + Sync + 'static>>,
}

impl<Tk> Durable<Tk> {
    fn new(
        id: &str,
        ctx: Arc<Context>,
        inner: Tk,
        progress_handler: Option<Arc<dyn Fn(&str, usize) + Send + Sync + 'static>>,
    ) -> Self {
        Self { id: id.to_string(), ctx, inner, progress_handler }
    }
}

/// Implement Task for Durable<Tk> by delegating to Tk
impl<Tk> Task for Durable<Tk>
where
    Tk: Task + Send + 'static,
    Tk::Input: Send + Clone,
    Tk::Output: Serialize + DeserializeOwned + Send + Sync + 'static,
{
    type Input = Tk::Input;
    type Output = Tk::Output;

    async fn run(self, input: Self::Input) -> Self::Output {
        // 1. Check if result is already in the database
        if let Some(cached) = self.ctx.db.get::<Self::Output>(&self.id) {
            // Update progress even if we skip execution
            let completed = self.ctx.completed_count.fetch_add(1, Ordering::SeqCst) + 1;
            if let Some(cb) = &self.progress_handler {
                cb(&self.id, completed);
            }
            return cached;
        }

        // 2. Execute the actual task logic
        let result = self.inner.run(input).await;

        // 3. Persist the result and increment progress
        self.ctx.db.save(&self.id, &result);
        let completed = self.ctx.completed_count.fetch_add(1, Ordering::SeqCst) + 1;
        if let Some(cb) = &self.progress_handler {
            cb(&self.id, completed);
        }
        
        result
    }
    
    fn extract_and_run(
        self,
        receivers: Vec<Box<dyn std::any::Any + Send>>,
    ) -> impl Future<Output = Result<Self::Output, String>> + Send {
        let id = self.id.clone();
        let ctx = self.ctx.clone();
        let progress = self.progress_handler.clone();
        let inner = self.inner;
        async move {
            // 1. Return cached value if present (skip extraction/execution)
            if let Some(cached) = ctx.db.get::<Self::Output>(&id) {
                let completed = ctx.completed_count.fetch_add(1, Ordering::SeqCst) + 1;
                if let Some(cb) = &progress {
                    cb(&id, completed);
                }
                return Ok(cached);
            }

            // 2. Delegate to the inner task's extraction + run
            let result = inner.extract_and_run(receivers).await?;

            // 3. Persist and update progress
            ctx.db.save(&id, &result);
            let completed = ctx.completed_count.fetch_add(1, Ordering::SeqCst) + 1;
            if let Some(cb) = &progress {
                cb(&id, completed);
            }

            Ok(result)
        }
    }
}

// ============================================================================
// 3. ENHANCED BUILDER API
// ============================================================================

struct DurableDag<'a> {
    dag: &'a DagRunner,
    ctx: Arc<Context>,
    progress_handler: Option<Arc<dyn Fn(&str, usize) + Send + Sync + 'static>>,
}

impl<'a> DurableDag<'a> {
    fn new(dag: &'a DagRunner, ctx: Arc<Context>) -> Self {
        Self { dag, ctx, progress_handler: None }
    }

    /// Attach a progress handler that will be called with (task_id, completed_count)
    fn with_progress<F>(mut self, handler: F) -> Self
    where
        F: Fn(&str, usize) + Send + Sync + 'static,
    {
        self.progress_handler = Some(Arc::new(handler));
        self
    }

    /// Adds a durable task and returns a standard TaskBuilder.
    /// This allows you to chain .depends_on() just like normal dagx.
    fn add<Tk>(&self, id: &str, task: Tk) -> TaskBuilder<'_, Durable<Tk>, Pending>
    where
        Tk: Task + Sync + 'static,
        Tk::Input: Clone + 'static,
        Tk::Output: Serialize + DeserializeOwned + Send + Sync + Clone + 'static,
    {
        let durable_wrapper = Durable::new(
            id,
            self.ctx.clone(),
            task,
            self.progress_handler.clone(),
        );
        self.dag.add_task(durable_wrapper)
    }
}

// ============================================================================
// 4. USAGE EXAMPLE
// ============================================================================

// Define standard dagx tasks using the #[task] macro
struct LoadValue(i32);
#[task]
impl LoadValue {
    async fn run(&self) -> i32 { self.0 }
}

struct Add;
#[task]
impl Add {
    async fn run(a: &i32, b: &i32) -> i32 { a + b }
}

struct Multiply;
#[task]
impl Multiply {
    async fn run(a: &i32, b: &i32, c: &i32) -> i32 { a * b * c }
}

#[tokio::main]
async fn main() {
    let dag = DagRunner::new();
    let ctx = Arc::new(Context {
        db: Arc::new(Db::new()),
        completed_count: Arc::new(AtomicUsize::new(0)),
    });
    
    let d_dag = DurableDag::new(&dag, ctx.clone()).with_progress(|id, completed| {
        println!("progress: task={} completed_count={}", id, completed);
    });

    // 1. Source tasks (0 dependencies)
    let val1 = d_dag.add("v1", LoadValue(10));
    let val2 = d_dag.add("v2", LoadValue(20));
    let val3 = d_dag.add("v3", LoadValue(2));

    // 2. Task with 2 dependencies
    // Create the `sum` builder first so it can be referenced by other tasks
    let sum_builder = d_dag.add("sum_1_2", Add);

    // 3. Task with 3 dependencies
    let one = d_dag.add("v4", LoadValue(1));
    let product = d_dag.add("final_prod", Multiply).depends_on((&sum_builder, &val3, &one));

    // Now wire `sum` dependencies and obtain its handle
    let _sum = sum_builder.depends_on((&val1, &val2));

    // Execute
    dag.run(|fut| { tokio::spawn(fut); }).await.unwrap();

    println!("Completed tasks: {}", ctx.completed_count.load(Ordering::SeqCst));
    println!("Final Result: {}", dag.get(product).unwrap());

    // --- Resumption Simulation ---
    println!("\nRestarting DAG...");
    let dag2 = DagRunner::new();
    let d_dag2 = DurableDag::new(&dag2, ctx.clone()).with_progress(|id, completed| {
        println!("(resumed) progress: task={} completed_count={}", id, completed);
    });
    
    // We recreate the same structure
    let v1 = d_dag2.add("v1", LoadValue(10));
    let v2 = d_dag2.add("v2", LoadValue(20));
    let s = d_dag2.add("sum_1_2", Add).depends_on((&v1, &v2));

    dag2.run(|fut| { tokio::spawn(fut); }).await.unwrap();
    println!("Retrieved cached sum: {}", dag2.get(s).unwrap());
}