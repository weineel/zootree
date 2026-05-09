# Pre-commit rustfmt Hook Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 引入 cargo-husky 分发的 pre-commit hook，在 `git commit` 时自动对已暂存的 `.rs` 文件运行 `rustfmt` 并纳入本次提交，保证 CI 的 `cargo fmt --check` 不再被格式问题绊住。

**Architecture:** `.cargo-husky/hooks/pre-commit` 是一个 shell 脚本——stash 未暂存改动 → 对 staged `.rs` 文件跑 `rustfmt --edition 2021` → `git add` 结果 → 恢复 stash。Cargo.toml 新增 cargo-husky 作为 dev-dependency（`user-hooks` 特性），build.rs 会在首次运行 `cargo check --tests` 时把脚本复制到 `.git/hooks/`。README 增加贡献者提示。

**Tech Stack:** cargo-husky 1.x (user-hooks feature), bash, git stash, rustfmt。

**Spec:** `docs/superpowers/specs/2026-05-09-pre-commit-rustfmt-hook-design.md`

---

## 文件结构

| 操作 | 文件 | 职责 |
|------|------|------|
| 新增 | `.cargo-husky/hooks/pre-commit` | 格式化 staged `.rs` 文件的 shell 脚本，cargo-husky 的复制源 |
| 修改 | `Cargo.toml` | 新增 `[dev-dependencies]` 段，引入 cargo-husky（default-features = false + user-hooks）|
| 修改（自动）| `Cargo.lock` | 由 `cargo check` 自动更新 |
| 修改 | `README.md` | 末尾新增 Contributing 章节，说明首次激活 hook 的命令 |

---

## Task 1: 编写 pre-commit 脚本

**Files:**
- Create: `.cargo-husky/hooks/pre-commit`

- [ ] **Step 1: 创建脚本**

写入以下内容到 `.cargo-husky/hooks/pre-commit`：

```bash
#!/usr/bin/env bash
set -euo pipefail

# 收集 staged 的 .rs 文件（-z 以 NUL 分隔，兼容特殊文件名）
staged=$(git diff --cached --name-only --diff-filter=ACMR -z -- '*.rs')
[ -z "$staged" ] && exit 0

# 若工作区有未暂存的 .rs 改动，先 stash --keep-index 保护它们
stashed=0
if ! git diff --quiet -- '*.rs'; then
    git stash push --keep-index --quiet --message "pre-commit-fmt"
    stashed=1
fi

# 无论后续成功与否，最终都尝试恢复 stash
trap '[ "$stashed" = 1 ] && git stash pop --quiet' EXIT

# 只对 staged 文件跑 rustfmt
printf '%s' "$staged" | xargs -0 rustfmt --edition 2021

# 把格式化结果加回本次提交
printf '%s' "$staged" | xargs -0 git add --
```

- [ ] **Step 2: 赋予执行权限**

Run: `chmod +x .cargo-husky/hooks/pre-commit`
Expected: 无输出。

- [ ] **Step 3: 确认权限与 shebang**

Run: `ls -l .cargo-husky/hooks/pre-commit && head -1 .cargo-husky/hooks/pre-commit`
Expected: 文件权限包含 `x`（如 `-rwxr-xr-x`），首行为 `#!/usr/bin/env bash`。

- [ ] **Step 4: Commit**

```bash
git add .cargo-husky/hooks/pre-commit
git commit -m "feat: add pre-commit hook to format staged .rs files"
```

---

## Task 2: 引入 cargo-husky 依赖并激活 hook

**Files:**
- Modify: `Cargo.toml`（在 `[dependencies]` 段之后、`[profile.dist]` 之前新增 `[dev-dependencies]`）
- Auto-modify: `Cargo.lock`

- [ ] **Step 1: 修改 Cargo.toml**

在 `shellexpand = "3"` 那一行之后、`# The profile that 'dist' will build with` 注释之前，插入：

```toml

[dev-dependencies]
cargo-husky = { version = "1", default-features = false, features = ["user-hooks"] }
```

注意 `default-features = false` 必须保留——关掉 cargo-husky 默认安装的 pre-push hook，只保留 user-hooks 模式。

- [ ] **Step 2: 确认修改位置正确**

Run: `grep -A 2 '^\[dev-dependencies\]' Cargo.toml`
Expected: 输出 `[dev-dependencies]` 段和 cargo-husky 这一行。

- [ ] **Step 3: （重要）先清理可能干扰的 hook**

若 `.git/hooks/pre-commit` 已存在且缺少 cargo-husky 魔术注释，cargo-husky 会拒绝覆盖。首次激活前清掉确保干净状态。

Run: `rm -f .git/hooks/pre-commit`
Expected: 无输出。

- [ ] **Step 4: 触发 build.rs 安装 hook**

Run: `cargo check --tests`
Expected: 编译成功；`Cargo.lock` 被更新加入 cargo-husky 相关条目。

- [ ] **Step 5: 验证 hook 被安装**

Run: `ls -l .git/hooks/pre-commit && head -5 .git/hooks/pre-commit`
Expected: 文件存在且可执行；前几行包含 cargo-husky 的魔术注释（形如 `# This hook was set by cargo-husky`）以及我们脚本的 shebang/内容。

- [ ] **Step 6: 验证脚本内容一致**

Run: `diff <(tail -n +2 .git/hooks/pre-commit | grep -v '^# This hook') .cargo-husky/hooks/pre-commit`
Expected: 差异仅限 cargo-husky 加的头部注释，脚本主体完全一致。（如果 diff 非空但内容都是注释，符合预期。）

- [ ] **Step 7: Commit**

```bash
git add Cargo.toml Cargo.lock
git commit -m "build: add cargo-husky dev-dependency for hook installation"
```

---

## Task 3: 冒烟测试（在临时分支操作，不污染 main）

**Files:** 无永久改动。

此 task 全部步骤在临时分支完成，测试完删除分支。

- [ ] **Step 1: 切到临时分支**

```bash
git checkout -b tmp/hook-smoke-test
```

Expected: `Switched to a new branch 'tmp/hook-smoke-test'`。

- [ ] **Step 2: 场景 A——基本格式化**

制造格式不规范的改动。写入 `src/runner.rs` 末尾一个格式错误的函数（举例——engineer 应选择一个不影响已有代码的地方，或在一个新文件里写）：

```bash
cat >> src/runner.rs <<'EOF'

pub fn _smoke_test_badly_formatted(   x:i32,y:i32)->i32{x+y}
EOF
```

然后：

```bash
git add src/runner.rs
git commit -m "test: smoke — badly formatted function"
```

Expected: commit 成功。

Run: `git show --stat HEAD && git show HEAD -- src/runner.rs | tail -20`
Expected: 提交已包含格式修正后的版本——函数内空格、缩进符合 rustfmt。

- [ ] **Step 3: 场景 B——部分暂存**

在同一文件里做两处改动：一处 add，一处不 add。紧接 Step 2 之后继续操作 `src/runner.rs`：

```bash
# 改动 1：追加一行并 add
printf '\npub fn _smoke_staged(  a:i32)->i32{a}\n' >> src/runner.rs
git add src/runner.rs

# 改动 2：再追加一行但不 add
printf '\npub fn _smoke_unstaged(  b:i32)->i32{b}\n' >> src/runner.rs
```

Run: `git status --short`
Expected: `src/runner.rs` 同时在 staged 和 unstaged 区域（状态码形如 `MM`）。

```bash
git commit -m "test: smoke — partial staging"
```

Expected: commit 成功。

Run: `git show HEAD -- src/runner.rs | grep _smoke && git diff -- src/runner.rs`
Expected:
- commit 中 `_smoke_staged` 已格式化（空格规范）；
- `_smoke_unstaged` 未进 commit，仍在工作区 diff 中（保持原样或被 rustfmt 后的 unstaged 版本）。

**关键验证**：`_smoke_unstaged` 没有意外丢失。

- [ ] **Step 4: 场景 C——rustfmt 失败**

制造一个 rustfmt 无法解析的语法错误：

```bash
printf '\npub fn _smoke_broken() { let x = ; }\n' >> src/runner.rs
git add src/runner.rs
git commit -m "test: smoke — rustfmt failure"
```

Expected: commit 失败，rustfmt 输出 parse error，git 退出非零。

Run: `git status --short && git stash list`
Expected:
- 工作区状态正常，`_smoke_broken` 行仍在文件中（index 与 working tree 不丢）；
- `git stash list` 中没有遗留的 `pre-commit-fmt` 条目（trap 已恢复）。

- [ ] **Step 5: 场景 D——无 .rs 改动**

```bash
echo "tmp" >> README.md
git add README.md
git commit -m "test: smoke — no .rs changes"
```

Expected: commit 成功，无任何 rustfmt 相关输出（脚本第一步 `exit 0`）。

- [ ] **Step 6: 清理临时分支**

```bash
git checkout main
git branch -D tmp/hook-smoke-test
```

Expected: 回到 main；临时分支连同所有烟测 commit 一起被删除。

Run: `git log --oneline -3`
Expected: 最近三条 commit 不包含任何 `test: smoke` 内容，工作区干净。

---

## Task 4: 更新 README 添加贡献者提示

**Files:**
- Modify: `README.md`（在末尾追加 Contributing 章节）

- [ ] **Step 1: 追加章节**

在 `README.md` 末尾追加以下内容（章节标题用 `##`，与原文风格一致）。注意外层用四个反引号包裹，避免与内部三反引号冲突：

````markdown

## Contributing

首次克隆本仓库后，执行一次 `cargo check --tests` 以激活 pre-commit hook：

```bash
cargo check --tests
```

cargo-husky 会把 `.cargo-husky/hooks/pre-commit` 安装到 `.git/hooks/`。此后 `git commit` 会自动对已暂存的 `.rs` 文件运行 `rustfmt` 并把格式化结果纳入本次提交——CI 的 `cargo fmt --check` 通常就不会再被格式问题绊住。

若需要临时跳过 hook（例如紧急修复），可用 `git commit --no-verify`。
````

追加到 README 里时去掉外层的 ```` ```markdown ```` 与 ```` ``` ```` 包裹，只保留从 `## Contributing` 起到最后一行的实际内容。

- [ ] **Step 2: 验证章节已加入**

Run: `tail -12 README.md`
Expected: 显示新增的 Contributing 章节。

Run: `grep -n '^## Contributing' README.md`
Expected: 输出一行行号，章节在文件末尾区域。

- [ ] **Step 3: Commit**

```bash
git add README.md
git commit -m "docs: document pre-commit hook activation in README"
```

---

## 最终验证

- [ ] **Step 1: 确认最终 git 状态**

Run: `git log --oneline -4`
Expected（自下向上——最早 commit 在下）：

```
<hash> docs: document pre-commit hook activation in README
<hash> build: add cargo-husky dev-dependency for hook installation
<hash> feat: add pre-commit hook to format staged .rs files
<hash> docs: add pre-commit rustfmt hook design spec   ← 之前已提交
```

- [ ] **Step 2: 确认工作区干净**

Run: `git status`
Expected: `nothing to commit, working tree clean`。

- [ ] **Step 3: 确认 CI 关键命令仍通过**

Run: `cargo fmt --check`
Expected: 无输出，退出码 0。

Run: `cargo clippy -- -D warnings 2>&1 | tail -5`
Expected: 无 warning/error 输出。

Run: `cargo test 2>&1 | tail -10`
Expected: 所有测试通过。

- [ ] **Step 4: Hook 存活确认**

Run: `ls -l .git/hooks/pre-commit`
Expected: 文件仍存在且可执行（Task 2/3 之后不会被删除）。

---

## 完成标准

- `.cargo-husky/hooks/pre-commit` 脚本在仓库中被追踪，且带可执行位。
- `Cargo.toml` 包含 cargo-husky dev-dependency，`Cargo.lock` 已相应更新。
- `cargo check --tests` 能成功激活 hook 到 `.git/hooks/pre-commit`。
- 四个冒烟场景（基本格式化 / 部分暂存 / rustfmt 失败 / 无 .rs 改动）行为符合预期。
- README 有清晰的激活提示。
- `cargo fmt --check`、`cargo clippy -- -D warnings`、`cargo test` 全部通过。
