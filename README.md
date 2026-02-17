# duraflow-rs

[![CI](https://github.com/dougcooper/duraflow-rs/actions/workflows/ci.yml/badge.svg)](https://github.com/dougcooper/duraflow-rs/actions/workflows/ci.yml) [![crates.io](https://img.shields.io/crates/v/duraflow-rs.svg)](https://crates.io/crates/duraflow-rs) [![docs.rs](https://docs.rs/duraflow-rs/badge.svg)](https://docs.rs/duraflow-rs)

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

CI & publishing

- A GitHub Actions workflow runs on push/PR to `main` (build + test + fmt check): `.github/workflows/ci.yml`.
- This CI also runs an MSRV job (Rust 1.81) to ensure backward compatibility.
- To publish to crates.io create a repository secret named `CRATES_IO_TOKEN` (your crates.io API token) and either:
  1. Push a tag like `v1.0.0` (the `publish` workflow will run on `refs/tags/v*`), or
  2. Use the repository's "Actions" tab and trigger the `Publish crate` workflow manually (workflow_dispatch).

Example: create a tag and push

```bash
git tag v1.0.0
git push origin v1.0.0
```

The `publish` workflow will run tests and then call `cargo publish` using the `CRATES_IO_TOKEN` secret.

MSRV (minimum supported Rust version)

- Declared in `Cargo.toml`: `rust-version = "1.71"`.
- The CI `msrv` job verifies the crate builds/tests on Rust 1.71.
- If you need to raise/lower the MSRV, update `rust-version` and adjust the CI job accordingly.

Enable local git hooks (pre-commit)

- This repository provides a pre-commit hook that runs `cargo fmt -- --check` and `cargo clippy --all -- -D warnings`.
- To enable the hook locally run:

```bash
scripts/install-hooks.sh
# or: git config core.hooksPath .githooks
```

- To bypass the hook for a single commit set `SKIP_HOOKS=1`, e.g.:

```bash
SKIP_HOOKS=1 git commit -m "skip hooks"
```

Note: Git hooks are a local setting (not enabled automatically for new clones); run the installer after cloning to activate them.
