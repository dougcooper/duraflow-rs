# duraflow-rs

Duraflow is a small helper library built on top of `dagx` that adds durability (persistence + resumption) and progress callbacks for DAG tasks.

Features
- Durable task wrapper that persists outputs to a `Storage` backend (in-memory or file-backed provided)
- `run_result` API to observe persistence errors without changing `dagx`'s `Task::run` signature
- Progress callback support

Quick example

```no_run
use duraflow_rs::{DurableDag, Context, MemoryStore};
use dagx::{DagRunner, task};
use std::sync::{Arc, atomic::AtomicUsize};

struct Load(i32);
#[task]
impl Load { async fn run(&self) -> i32 { self.0 } }

#[tokio::main]
async fn main() {
    let dag = DagRunner::new();
    let db = Arc::new(MemoryStore::new());
    let ctx = Arc::new(Context { db, completed_count: Arc::new(AtomicUsize::new(0)) });
    let d = DurableDag::new(&dag, ctx.clone());

    let a = d.add("v1", Load(5));
    dag.run(|f| { tokio::spawn(f); }).await.unwrap();
    println!("result = {}", dag.get(a).unwrap());
}
```

Observing persistence errors

Use `run_result` on `Durable` directly to get a `Result` with `DuraflowError` if persistence fails.

Testing

- In-memory store: `MemoryStore`
- File-backed store: `FileStore` (directory-per-key storage)

Run tests:

```bash
cargo test
```
