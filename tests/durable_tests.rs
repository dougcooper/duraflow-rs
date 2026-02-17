use duraflow_rs::{Durable, Context, MemoryStore, Storage};
use dagx::{task, Task};
use std::sync::Arc;
use std::sync::atomic::AtomicUsize;
use parking_lot::Mutex;

// Failing store for tests
struct FailingStore;
impl Storage for FailingStore {
    fn get_raw(&self, _key: &str) -> Option<String> { None }
    fn save_raw(&self, _key: &str, _value: &str) -> std::io::Result<()> {
        Err(std::io::Error::new(std::io::ErrorKind::Other, "simulated write failure"))
    }
}

struct Const42;
#[task]
impl Const42 {
    async fn run(&self) -> i32 { 42 }
}

#[tokio::test]
async fn extract_and_run_propagates_persist_error() {
    let ctx = Arc::new(Context { db: Arc::new(FailingStore), completed_count: Arc::new(AtomicUsize::new(0)) });
    let durable = Durable::new("t1", ctx.clone(), Const42, None);

    let res = durable.extract_and_run(vec![]).await;
    assert!(res.is_err());
    assert!(res.err().unwrap().contains("persist failed"));
}

#[tokio::test]
async fn run_result_propagates_persist_error() {
    let ctx = Arc::new(Context { db: Arc::new(FailingStore), completed_count: Arc::new(AtomicUsize::new(0)) });
    let durable = Durable::new("t2", ctx.clone(), Const42, None);

    let res = durable.run_result(()).await;
    assert!(res.is_err());
    match res.err().unwrap() {
        duraflow_rs::DuraflowError::Persist { key, .. } => assert_eq!(key, "t2"),
        other => panic!("unexpected error variant: {:?}", other),
    }
}

#[tokio::test]
async fn persist_and_cache_work_and_progress_called() {
    let db = MemoryStore::new();
    let ctx = Arc::new(Context { db: Arc::new(db), completed_count: Arc::new(AtomicUsize::new(0)) });

    let progress_calls = Arc::new(Mutex::new(Vec::new()));
    let progress_clone = progress_calls.clone();
    let handler = move |id: &str, count: usize| {
        progress_clone.lock().push((id.to_string(), count));
    };

    let durable = Durable::new("t3", ctx.clone(), Const42, Some(Arc::new(handler)));

    // first run -> persists
    let out = durable.extract_and_run(vec![]).await.unwrap();
    assert_eq!(out, 42);

    // stored value should be readable
    let stored: Option<i32> = ctx.get("t3");
    assert_eq!(stored, Some(42));

    // progress handler should have been called at least once
    let calls = progress_calls.lock();
    assert!(calls.iter().any(|(id, _)| id == "t3"));

    // run_result should return cached value and not error
    let durable2 = Durable::new("t3", ctx.clone(), Const42, None);
    let res = durable2.run_result(()).await.unwrap();
    assert_eq!(res, 42);
}
