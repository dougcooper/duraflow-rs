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

Create & add your crates.io token

1. Create a new API token on https://crates.io/me ("new token").
2. In GitHub go to **Settings → Secrets** → **Actions** → **New repository secret**.
   - Name: `CRATES_IO_TOKEN`
   - Value: the token you copied from crates.io
3. The `Publish crate` workflow will fail with `please provide a non-empty token` if the secret is not set.

Local publish (alternative)

- Locally you can run `cargo login <token>` once and then `cargo publish` without passing a token on the command line.

MSRV (minimum supported Rust version)

- Declared in `Cargo.toml`: `rust-version = "1.81"`.
- The CI `msrv` job verifies the crate builds/tests on Rust 1.81.
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
