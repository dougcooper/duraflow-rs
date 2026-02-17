use duraflow_rs::{FileStore, MemoryStore, Storage};
use dagx::{task, Task};
use std::sync::Arc;
use std::fs;
use std::time::{SystemTime, UNIX_EPOCH};

#[test]
fn filestore_persists_and_reads() {
    let base = std::env::temp_dir();
    let dir = base.join(format!("duraflow_test_{}", SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_nanos()));
    fs::create_dir_all(&dir).unwrap();

    let store = FileStore::new(&dir).unwrap();

    store.save_raw("foo", "bar").unwrap();
    assert_eq!(store.get_raw("foo"), Some("bar".to_string()));

    // cleanup
    let _ = fs::remove_dir_all(&dir);
}

#[tokio::test]
async fn concurrent_tasks_progress_and_storage() {
    use dagx::task;
    use dagx::DagRunner;
    use duraflow_rs::DurableDag;
    use std::sync::atomic::AtomicUsize;

    struct V(i32);
    #[task]
    impl V {
        async fn run(&self) -> i32 { self.0 }
    }

    let dag = DagRunner::new();
    let db = MemoryStore::new();
    let ctx = Arc::new(duraflow_rs::Context { db: Arc::new(db), completed_count: Arc::new(AtomicUsize::new(0)) });
    let d = DurableDag::new(&dag, ctx.clone());

    // create 10 independent source tasks
    let mut handles = Vec::new();
    for i in 0..10 {
        let h = d.add(&format!("k{}", i), V(i));
        handles.push(h);
    }

    dag.run(|fut| { tokio::spawn(fut); }).await.unwrap();

    // verify outputs and progress
    for (i, h) in handles.into_iter().enumerate() {
        assert_eq!(dag.get(h).unwrap(), i as i32);
    }

    assert_eq!(ctx.completed_count.load(std::sync::atomic::Ordering::SeqCst), 10);
}
