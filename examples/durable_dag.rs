use dagx::{DagRunner, Task, task};
use duraflow_rs::{Context, DurableDag, MemoryStore};
use std::sync::Arc;
use std::sync::atomic::AtomicUsize;

// Example tasks
struct LoadValue(i32);
#[task]
impl LoadValue {
    async fn run(&self) -> i32 {
        self.0
    }
}

struct Add;
#[task]
impl Add {
    async fn run(a: &i32, b: &i32) -> i32 {
        a + b
    }
}

struct Multiply;
#[task]
impl Multiply {
    async fn run(a: &i32, b: &i32, c: &i32) -> i32 {
        a * b * c
    }
}

#[tokio::main]
async fn main() {
    let dag = DagRunner::new();
    let db: Arc<dyn duraflow_rs::Storage + Send + Sync> = Arc::new(MemoryStore::new());
    let ctx = Arc::new(Context {
        db,
        completed_count: Arc::new(AtomicUsize::new(0)),
    });

    let d_dag = DurableDag::new(&dag, ctx.clone()).with_progress(|id, completed| {
        println!("progress: task={} completed_count={}", id, completed);
    });

    // Build DAG (same as before)
    let val1 = d_dag.add("v1", LoadValue(10));
    let val2 = d_dag.add("v2", LoadValue(20));
    let val3 = d_dag.add("v3", LoadValue(2));

    let sum_builder = d_dag.add("sum_1_2", Add);
    let one = d_dag.add("v4", LoadValue(1));
    let product = d_dag
        .add("final_prod", Multiply)
        .depends_on((&sum_builder, &val3, &one));
    let _sum = sum_builder.depends_on((&val1, &val2));

    dag.run(|fut| {
        tokio::spawn(fut);
    })
    .await
    .unwrap();

    println!(
        "Completed tasks: {}",
        ctx.completed_count
            .load(std::sync::atomic::Ordering::SeqCst)
    );
    println!("Final Result: {}", dag.get(product).unwrap());

    // Resumption
    println!("\nRestarting DAG...");
    let dag2 = DagRunner::new();
    let d_dag2 = DurableDag::new(&dag2, ctx.clone()).with_progress(|id, completed| {
        println!(
            "(resumed) progress: task={} completed_count={}",
            id, completed
        );
    });

    let v1 = d_dag2.add("v1", LoadValue(10));
    let v2 = d_dag2.add("v2", LoadValue(20));
    let s = d_dag2.add("sum_1_2", Add).depends_on((&v1, &v2));

    dag2.run(|fut| {
        tokio::spawn(fut);
    })
    .await
    .unwrap();
    println!("Retrieved cached sum: {}", dag2.get(s).unwrap());
}
