# Pre-commit rustfmt Hook 设计

## 背景

`cargo fmt --check` 已经是 CI 必过项（`.github/workflows/ci.yml`），但本地没有任何拦截机制，未格式化的代码仍能进入仓库——最近一次 `cargo fmt` 一次性改动了 22 个源文件就是证据。目标是在本地 `git commit` 时自动格式化已暂存的 `.rs` 文件，并把格式化结果并入本次提交，让 CI 不再因格式问题失败。

## 目标与非目标

**目标**
- `git commit` 时自动对已暂存的 `.rs` 文件跑 `rustfmt`。
- 格式化结果自动 `git add` 纳入本次提交，开发者无需额外操作。
- 未暂存的改动（包括同一文件中的未暂存部分）不被带入 commit。
- Hook 随仓库分发，开发者克隆后近乎零成本激活。

**非目标**
- 不替代 CI 检查：CI 的 `cargo fmt --check` 作为第二道防线保留。
- 不做 clippy、test 等其他钩子——本次只处理格式化。
- 不改变 `cargo fmt --check` 已经认定合规的代码的行为。

## 方案总览

两部分改动：

1. **Cargo.toml** 新增 `cargo-husky` 作为 `dev-dependency`（`user-hooks` 模式），负责在首次构建测试目标时把 `.cargo-husky/hooks/` 下的脚本安装到 `.git/hooks/`。
2. **`.cargo-husky/hooks/pre-commit`** 新增 shell 脚本：收集 staged `.rs` 文件 → stash 未暂存改动 → `rustfmt` → 重新 `git add` → 恢复 stash。

外加一处文档更新，告诉贡献者首次构建需跑一次 `cargo check --tests` 激活 hook。

## 详细设计

### Cargo.toml

```toml
[dev-dependencies]
cargo-husky = { version = "1", default-features = false, features = ["user-hooks"] }
```

- `default-features = false` 关掉 cargo-husky 自带的"装一个跑 cargo test 的 pre-push hook"默认行为。
- `user-hooks` 特性告诉 cargo-husky 从 `.cargo-husky/hooks/` 读取自定义脚本。

### `.cargo-husky/hooks/pre-commit`

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

设计要点：
- **`--keep-index` + stash**：让 rustfmt 只看到 index 里的版本，避免把未暂存改动一起格式化进 commit。
- **`trap EXIT`**：`set -e` 下 rustfmt 失败会非 0 退出被 git 拦住 commit；trap 保证 stash 一定被恢复，开发者不丢改动。
- **`rustfmt --edition 2021`** 而非 `cargo fmt`：避免 `cargo fmt` 扫全 crate；`--edition` 必须显式指定，否则 rustfmt 默认按 edition 2015 解析新语法会出错。
- **不检查 `rustfmt` 是否存在**：zootree 开发者必定装了 rustup，缺这个工具连 `cargo build` 都会失败，不需要额外兜底。
- **文件名包含空格/特殊字符**：`--name-only -z` + `xargs -0` 一路 NUL 分隔保证正确。

### 文档更新

在 `README.md` 新增或补充一段贡献者提示：

> 首次克隆本仓库后，执行一次 `cargo check --tests` 以激活 pre-commit hook（cargo-husky 会自动把 `.cargo-husky/hooks/pre-commit` 安装到 `.git/hooks/`）。此后 `git commit` 会自动对已暂存的 `.rs` 文件运行 rustfmt 并纳入本次提交。

## 边界情况

| 场景 | 预期行为 |
|------|----------|
| 没有 .rs 文件被暂存 | 脚本 `exit 0`，跳过所有逻辑。 |
| 只有 staged 改动，无 unstaged 改动 | 不 stash，直接 rustfmt + git add。 |
| 同一文件既 staged 又 unstaged | stash `--keep-index` 把 unstaged 部分移走，rustfmt 只看 index 版本；stash pop 恢复 unstaged 部分。 |
| stash pop 冲突 | 罕见——意味着 rustfmt 改动了 unstaged 部分也修改的行。git 标记冲突，用户手动解决；trap 不隐藏错误。 |
| rustfmt 失败（语法错误等） | `set -e` 下非 0 退出，commit 被拦；trap 恢复 stash，工作区状态不变。 |
| `git commit --amend` / rebase 中的 commit | pre-commit 正常触发，行为与普通 commit 一致。 |
| 开发者显式 `git commit --no-verify` | hook 被跳过——这是 git 原生行为，不拦截。 |

## CI 协作

CI 不变：`.github/workflows/ci.yml` 的 `cargo fmt --check` 作为第二道防线保留。当开发者：
- 还没跑过 `cargo check --tests`（hook 未激活）
- 用 `--no-verify` 绕过
- 通过 GitHub Web UI 直接编辑文件

这些情况仍会被 CI 拦住。

## 测试计划

人工 smoke test（不进 `tests/`）：

1. **激活**：删除 `.git/hooks/pre-commit`（如有），运行 `cargo check --tests`，确认 `.git/hooks/pre-commit` 被重新生成。
2. **基本格式化**：制造一个格式不规范的 `.rs` 改动，`git add && git commit`，提交成功且 diff 显示格式被修正。
3. **部分暂存**：同一文件添加两处改动——一处 `git add`，一处不 add，commit 后确认未暂存部分仍在工作区。
4. **rustfmt 失败**：插入语法错误，`git add && git commit`，commit 被拦，工作区状态不变（stash 已恢复）。
5. **无 .rs 改动**：只改 README 然后 commit，hook 应快速退出不干扰。

## 风险与权衡

- **cargo-husky 依赖引入**：多一个 dev-dependency，仅在 `cargo test` / `cargo check --tests` 时参与构建，对发布产物无影响。
- **首次激活需要一次构建**：cargo-husky 依赖 build.rs 触发，冷克隆后第一次 `git commit` 前若没跑过测试目标，hook 不生效——通过文档提示缓解，CI 兜底。
- **rustfmt 修改了未暂存部分共同修改的行**：stash pop 冲突由用户手动解决，少见但不 silent 处理。
- **相对其他方案（纯 shell 脚本 + `core.hooksPath`、手动 install 脚本）**：cargo-husky 的自动化程度最高，代价是多一层工具依赖——权衡下更适合以 Rust 工具链为默认前置条件的本项目。
