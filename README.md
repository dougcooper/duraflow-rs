# duraflow-rs

[![CI](https://github.com/dougcooper/duraflow-rs/actions/workflows/ci.yml/badge.svg)](https://github.com/dougcooper/duraflow-rs/actions/workflows/ci.yml) [![crates.io](https://img.shields.io/crates/v/duraflow-rs.svg)](https://crates.io/crates/duraflow-rs) [![docs.rs](https://docs.rs/duraflow-rs/badge.svg)](https://docs.rs/duraflow-rs)

Duraflow is a small helper library built on top of `dagx` that adds durability (persistence + resumption) and progress callbacks for DAG tasks.

Features
- Durable task wrapper that persists outputs to a `Storage` backend (in-memory or file-backed provided)
- `run_result` API to observe persistence errors without changing `dagx`'s `Task::run` signature
- Progress callback support

Quick example — durability, caching, progress, and FileStore

```no_run
use duraflow_rs::{DurableDag, Context, MemoryStore, FileStore, DuraflowError};
use dagx::{DagRunner, task};
use std::sync::{Arc, atomic::AtomicUsize};

// A small task that returns the contained value.
struct Load(i32);
#[task]
impl Load { async fn run(&self) -> i32 { self.0 } }

// Multiply task depends on a previous task's output (demonstrates .depends_on()).
struct Mul(i32);
#[task]
impl Mul { async fn run(&self) -> i32 { self.0 } }

#[tokio::main]
async fn main() -> Result<(), DuraflowError> {
    // --- Example 1: in-memory store + progress callback ---------------------------------
    let dag = DagRunner::new();
    let db = Arc::new(MemoryStore::new());
    let ctx = Arc::new(Context { db: db.clone(), completed_count: Arc::new(AtomicUsize::new(0)) });

    // progress handler will be called as tasks complete
    let d = DurableDag::new(&dag, ctx.clone()).with_progress(|id, completed| {
        eprintln!("progress: {} completed (total {})", id, completed);
    });

    // add two tasks and wire dependency
    let a = d.add("load-5", Load(5));
    let b = d.add("mul-2", Mul(2)).depends_on(&a);

    // run the DAG (tasks are persisted to MemoryStore)
    dag.run(|f| { tokio::spawn(f); }).await.unwrap();
    assert_eq!(dag.get(a).unwrap(), 5);
    assert_eq!(dag.get(b).unwrap(), 2);

    // the value was persisted — we can read it directly from the Context helper
    assert_eq!(ctx.get::<i32>("load-5").unwrap(), 5);

    // --- Example 2: cached path (re-run with a fresh DagRunner) ------------------------
    // Create a new runner but re-use the same `Context`/store: persisted outputs short-circuit
    let dag2 = DagRunner::new();
    let d2 = DurableDag::new(&dag2, ctx.clone());

    // NOTE: inner task contains a different value, but the persisted value for `load-5` wins
    let a2 = d2.add("load-5", Load(999));
    let b2 = d2.add("mul-3", Mul(3)).depends_on(&a2);
    dag2.run(|f| { tokio::spawn(f); }).await.unwrap();

    // cached result is returned (5), not 999
    assert_eq!(dag2.get(a2).unwrap(), 5);

    // --- Example 3: FileStore (persists to disk) --------------------------------------
    let dir = std::env::temp_dir().join("duraflow_example_store");
    let fs = FileStore::new(&dir).expect("create filestore");
    let ctx2 = Arc::new(Context { db: Arc::new(fs), completed_count: Arc::new(AtomicUsize::new(0)) });

    let dag3 = DagRunner::new();
    let d3 = DurableDag::new(&dag3, ctx2.clone());
    let f = d3.add("load-file", Load(7));
    dag3.run(|f| { tokio::spawn(f); }).await.unwrap();

    // value persisted to disk — subsequent processes can read it from the same directory
    assert_eq!(ctx2.get::<i32>("load-file").unwrap(), 7);

    // --- Example 4: inspect persistence errors with `run_result` -----------------------
    // Construct a Durable wrapper directly and call `run_result` to observe storage errors.
    let durable = duraflow_rs::Durable::new("one-off", ctx.clone(), Load(11), None);
    let out = durable.run_result(()).await?; // `()` is the Task::Input for `#[task]`-generated tasks
    assert_eq!(out, 11);

    Ok(())
}
```

Notes
- Persistence key = the task `id` you pass to `DurableDag::add`.
- Re-running a DAG that uses the same `Context`/store will short‑circuit already-persisted tasks (caching).
- Use `Durable::run_result(...)` when you need to observe persistence errors; `Task::run` still returns the task output directly.
- `FileStore` stores one file per key (directory-per-key); `MemoryStore` is useful for tests and examples.
Observing persistence errors

Use `run_result` on `Durable` directly to get a `Result` with `DuraflowError` if persistence fails.

Testing

- In-memory store: `MemoryStore`
- File-backed store: `FileStore` (directory-per-key storage)