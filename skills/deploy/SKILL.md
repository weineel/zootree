---
name: deploy
description: 在 zootree 源码仓库安装当前 checkout 的 mise 开发版、预演或发布正式版本，以及跟踪指定 tag 的发布进度。
---

# 部署 zootree

在 zootree 仓库根目录执行。根据用户指定的目标选用下面的流程。单独收到 `deploy` 且上下文没有目标时，先问「安装本地开发版，还是发布正式版本？」。

## 本地开发版

运行 `mise run zootree:install-dev`，然后用 `mise x 'cargo:zootree@dev' -- zootree --version` 验证。报告实际版本和 `git rev-parse --short HEAD`，构建包含未提交改动时一并注明。该任务会把 mise 的 `dev` 链接指向当前 checkout 构建的二进制。

## 正式发布

1. 用 `git status --short --branch` 查看工作区和上游，用 `cargo release changes` 核对本次包含的提交。发布来源有歧义或存在未提交的改动时，先向用户说明。版本按请求选用 `patch`、`minor`、`major` 或指定版本号。
2. 用 `cargo release <版本>` 预演，核对输出中的版本、tag 和推送目标。用户只要求预演时，到此报告结果。若尚未明确版本，可先用 `patch` 预演，把算出的版本提交给用户决定。
3. 正式发布前核对待发布提交的 [CI 检查](../../.github/workflows/ci.yml) 结果。需要本地验证时运行 `cargo fmt --check`、`cargo clippy -- -D warnings` 和 `cargo test`。
4. 用户已要求发布该版本、预演通过后，执行 `cargo release <版本> --execute`。在非交互会话中可加 `--no-confirm`。仓库的 [release.toml](../../release.toml) 负责版本提交、打 tag 和推送。接着跟踪该 tag 的 [Release 工作流](../../.github/workflows/release.yml)。

## 跟踪发布

跟踪已有 tag 时直接从这里开始。用 `git rev-list -n 1 refs/tags/<tag>` 取发布提交。本地缺少 tag 时，用 `git ls-remote origin 'refs/tags/<tag>' 'refs/tags/<tag>^{}'` 查询，带注解的 tag 取 `^{}` 对应的提交。运行 `gh run list --workflow release.yml --event push --commit <SHA> --json databaseId,headBranch,status,url`，选择 `headBranch` 与 tag 全名相同的运行。刚推送的 tag 可能要稍等才会出现。

用 `gh run watch <run-id> --exit-status` 等待结束。正式版应确认 Homebrew 和 crates.io 的发布 job 均成功，再用 `gh release view <tag> --json url,assets` 核对发布页和产物。失败时用 `gh run view <run-id> --log-failed` 定位失败的 job，报告已发布的部分和剩余问题。若本地发版命令中断，先核对远端 tag 与已有运行，再决定从哪一步继续，以免重复发版。
