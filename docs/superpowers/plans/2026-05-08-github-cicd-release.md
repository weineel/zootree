# GitHub CI/CD Release Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Set up a fully automated release pipeline so `cargo release minor` is the only command needed to ship a new version of zootree to GitHub Releases, Homebrew tap, and crates.io.

**Architecture:** `cargo-release` handles local version bumping, committing, tagging, and pushing. `cargo-dist` generates a GitHub Actions workflow that triggers on `v*` tags, runs tests as a gate, builds cross-platform binaries for 5 targets, creates the GitHub Release, updates the Homebrew tap, and publishes to crates.io. A separate `ci.yml` runs fmt + clippy + tests on every push and PR.

**Tech Stack:** cargo-dist, cargo-release, GitHub Actions, crates.io, Homebrew tap (`weineel/homebrew-tap`)

---

## File Map

| File | Action | Purpose |
|------|--------|---------|
| `Cargo.toml` | Modify | Add `[workspace]` + `[workspace.metadata.dist]` (done by `cargo dist init`) |
| `release.toml` | Create | cargo-release config: bump, commit, tag, push — no publish |
| `.github/workflows/release.yml` | Create (generated) | Tag-triggered build + publish pipeline |
| `.github/workflows/ci.yml` | Create | Push/PR continuous testing |

---

### Task 1: Install cargo-dist and cargo-release

**Files:** none

- [ ] **Step 1: Install cargo-dist**

```bash
cargo install cargo-dist
```

- [ ] **Step 2: Verify cargo-dist is installed**

```bash
cargo dist --version
```

Expected: prints a version string like `cargo-dist 0.28.x`.

- [ ] **Step 3: Install cargo-release**

```bash
cargo install cargo-release
```

- [ ] **Step 4: Verify cargo-release is installed**

```bash
cargo release --version
```

Expected: prints a version string like `cargo-release 0.x.x`.

---

### Task 2: Run `cargo dist init` to configure Cargo.toml and generate release.yml

**Files:**
- Modify: `Cargo.toml`
- Create: `.github/workflows/release.yml`

- [ ] **Step 1: Create the workflows directory**

```bash
mkdir -p .github/workflows
```

- [ ] **Step 2: Run cargo dist init with our configuration**

```bash
cargo dist init \
  --ci=github \
  --installer=shell \
  --installer=homebrew \
  --tap=weineel/homebrew-tap \
  --publish-jobs=crates-io \
  -t x86_64-apple-darwin \
  -t aarch64-apple-darwin \
  -t x86_64-unknown-linux-gnu \
  -t aarch64-unknown-linux-gnu \
  -t x86_64-pc-windows-msvc \
  --yes
```

This command:
- Adds `[workspace]` and `[workspace.metadata.dist]` to `Cargo.toml`
- Generates `.github/workflows/release.yml`

- [ ] **Step 3: Verify Cargo.toml was updated**

```bash
grep -A 15 '\[workspace.metadata.dist\]' Cargo.toml
```

Expected output (versions may differ):
```toml
[workspace.metadata.dist]
cargo-dist-version = "0.28.x"
ci = ["github"]
targets = [
  "x86_64-apple-darwin",
  ...
]
installers = ["shell", "homebrew"]
tap = "weineel/homebrew-tap"
publish-jobs = ["crates-io"]
```

- [ ] **Step 4: Verify release.yml was generated**

```bash
ls -la .github/workflows/release.yml
```

Expected: file exists.

- [ ] **Step 5: Verify all 5 targets appear in release.yml**

```bash
grep -c "apple-darwin\|linux-gnu\|windows-msvc" .github/workflows/release.yml
```

Expected: number ≥ 5.

- [ ] **Step 6: Check which token the Homebrew tap step uses**

```bash
grep -A 5 -i "homebrew\|tap" .github/workflows/release.yml | grep -i "token\|secret"
```

If the output shows `GITHUB_TOKEN`, proceed to Step 7. If it already shows `HOMEBREW_TAP_TOKEN`, skip Step 7.

- [ ] **Step 7: Replace GITHUB_TOKEN with HOMEBREW_TAP_TOKEN in the tap publish step**

`GITHUB_TOKEN` is scoped to the current repo only and cannot push to `weineel/homebrew-tap`. Open `.github/workflows/release.yml`, find the step that pushes to the Homebrew tap, and replace:

```yaml
# Before (example — exact line may differ)
token: ${{ secrets.GITHUB_TOKEN }}
```

with:

```yaml
token: ${{ secrets.HOMEBREW_TAP_TOKEN }}
```

Only replace the token reference in the Homebrew tap step, not any other `GITHUB_TOKEN` usage in the file.

- [ ] **Step 8: Commit**

```bash
git add Cargo.toml Cargo.lock .github/workflows/release.yml
git commit -m "chore: add cargo-dist release pipeline"
```

---

### Task 3: Create release.toml

**Files:**
- Create: `release.toml`

- [ ] **Step 1: Create release.toml at the project root**

```toml
sign-commit = false
sign-tag = false
push = true
publish = false
tag = true
pre-release-commit-message = "chore: release {{version}}"
tag-message = "v{{version}}"
```

`publish = false` is critical: cargo-release must NOT publish to crates.io. cargo-dist handles that. Duplicate publishing would fail on the second attempt.

- [ ] **Step 2: Dry-run to verify cargo-release reads the config correctly**

```bash
cargo release patch --dry-run
```

Expected: output shows version bump `0.1.0 → 0.1.1`, commit message `chore: release 0.1.1`, tag `v0.1.1`, push enabled, publish skipped. No errors.

- [ ] **Step 3: Commit**

```bash
git add release.toml
git commit -m "chore: add cargo-release configuration"
```

---

### Task 4: Create ci.yml

**Files:**
- Create: `.github/workflows/ci.yml`

- [ ] **Step 1: Create .github/workflows/ci.yml**

```yaml
name: CI

on:
  push:
    branches: [main]
  pull_request:

jobs:
  test:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4
      - uses: dtolnay/rust-toolchain@stable
        with:
          components: clippy, rustfmt
      - run: cargo fmt --check
      - run: cargo clippy -- -D warnings
      - run: cargo test
```

- [ ] **Step 2: Verify the file is valid YAML**

```bash
python3 -c "import yaml, sys; yaml.safe_load(open('.github/workflows/ci.yml')); print('valid')"
```

Expected: prints `valid`. If python3 is not available, visually confirm the indentation is correct.

- [ ] **Step 3: Commit**

```bash
git add .github/workflows/ci.yml
git commit -m "ci: add CI workflow for push and PR"
```

---

### Task 5: Push and verify CI

**Files:** none

- [ ] **Step 1: Push all commits to main**

```bash
git push
```

- [ ] **Step 2: Check that CI workflow triggers**

Open https://github.com/weineel/zootree/actions and confirm the `CI` workflow starts running.

- [ ] **Step 3: Verify CI passes**

Wait for the workflow to complete. All three steps (fmt, clippy, test) should show green checkmarks.

If `cargo fmt --check` fails: run `cargo fmt` locally, commit the formatting changes, and push again.

If `cargo clippy -- -D warnings` fails: fix the warnings locally, commit, push again.

---

### Task 6: Verify the release workflow with cargo dist plan

**Files:** none

- [ ] **Step 1: Run cargo dist plan to verify the release config is correct**

```bash
cargo dist plan
```

Expected: output lists the 5 build targets, shell installer, and homebrew installer. No errors.

If this fails with a config error, check that `Cargo.toml` has `[workspace]` section (cargo dist init should have added it). If missing, add manually:

```toml
[workspace]
members = ["."]
```

---

## Developer Workflow (after setup)

Once setup is complete, releasing is one command:

```bash
# Patch release: 0.1.0 → 0.1.1
cargo release patch

# Minor release: 0.1.0 → 0.2.0
cargo release minor

# Major release: 0.1.0 → 1.0.0
cargo release major
```

This bumps version in `Cargo.toml`, commits with `chore: release X.Y.Z`, tags `vX.Y.Z`, and pushes — triggering the release workflow automatically.
