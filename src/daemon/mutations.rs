//! State-changing endpoints other than done and cancel: start, reopen,
//! add-repo, create, archived-workspace deletion and repo registration and
//! removal. Each one is an Operation (same lifecycle as done and cancel) and
//! answers 202 with the new Operation.
//!
//! The Daemon never runs a foreground attach. For start and reopen the
//! service layer hands back an `AttachRequest`; it travels in the Operation's
//! result as `attach` (`{program, args}`) next to `agent_cli`, and the
//! Consumer performs it in its own terminal.
//!
//! Busy rule: the Operation registry is keyed by a string. Workspace
//! Operations use the Workspace name, as in done and cancel. Operations with
//! no single Workspace use keys that cannot be Workspace names (names are
//! `[A-Za-z0-9_-]`): `repo:<name>` for repo registration and removal.

use super::api;
use super::error::ApiError;
use super::operations::{self, Job};
use super::server::AppState;
use crate::config::global::MultiplexerKind;
use crate::config::name;
use crate::config::workspace::WorkspaceStatus;
use crate::config::ConfigManager;
use crate::core::name_gen::NameGenerator;
use crate::core::reopen::{NonInteractiveReopenPrompt, ReopenOptions, ReopenSources};
use crate::core::workspace_repository::{AddRepositoryRequest, TerminalUpdate};
use crate::service::add_repo::add_repo_to_workspace;
use crate::service::create::{create_workspace, CreateRequest};
use crate::service::prune::{prune_workspaces, PruneRequest, PruneTarget};
use crate::service::reopen::{reopen_workspace, ReopenRequest};
use crate::service::repos::{
    register_repo, remove_repo, repo_add_base_name, RegisterRepoRequest, RemoveRepoRequest,
};
use crate::service::start::{start_workspace, SessionOutcome, StartRequest};
use crate::service::{EnvRunner, EnvSnapshot, RequestContext};
use axum::body::Bytes;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::routing::{delete, post};
use axum::{Json, Router};
use serde::Deserialize;
use serde_json::{json, Value};
use std::collections::{BTreeMap, BTreeSet};

type Accepted = Result<(StatusCode, Json<Value>), ApiError>;

/// Routes added to the main router.
pub(super) fn routes() -> Router<AppState> {
    Router::new()
        .route("/workspaces", post(create))
        .route("/workspaces/{name}", delete(prune))
        .route("/workspaces/{name}/start", post(start))
        .route("/workspaces/{name}/reopen", post(reopen))
        .route("/workspaces/{name}/add-repo", post(add_repo))
        .route("/repos", post(register))
        .route("/repos/{name}", delete(unregister))
}

// ---- request bodies -------------------------------------------------------

/// `run_agent` on the wire: absent, null or `false` run no agent; `true` runs
/// the configured default agent; a string runs that command or alias.
#[derive(Debug, Clone, Deserialize)]
#[serde(untagged)]
enum RunAgent {
    Flag(bool),
    Command(String),
}

fn run_agent(value: Option<RunAgent>) -> Option<Option<String>> {
    match value {
        None | Some(RunAgent::Flag(false)) => None,
        Some(RunAgent::Flag(true)) => Some(None),
        Some(RunAgent::Command(command)) => Some(Some(command)),
    }
}

/// The environment snapshot sent by the client. Absent: the Daemon's own
/// whitelisted environment (never an empty one, which would strip PATH).
fn snapshot(env: Option<BTreeMap<String, String>>) -> EnvSnapshot {
    match env {
        Some(vars) => EnvSnapshot::from_pairs(vars),
        None => EnvSnapshot::capture(),
    }
}

fn parse<T: for<'de> Deserialize<'de> + Default>(raw: &Bytes) -> Result<T, ApiError> {
    operations::parse_body(raw)
}

#[derive(Debug, Default, Deserialize)]
#[serde(default, deny_unknown_fields)]
struct StartBody {
    env: Option<BTreeMap<String, String>>,
    multiplexer: Option<MultiplexerKind>,
    no_multiplexer: bool,
    run_agent: Option<RunAgent>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(default, deny_unknown_fields)]
struct ReopenBody {
    env: Option<BTreeMap<String, String>>,
    multiplexer: Option<MultiplexerKind>,
    /// `current` or `REPO:BRANCH`, as `--from` on the command line.
    from: Vec<String>,
    /// Repos whose existing worktree may be overwritten.
    overwrite: Vec<String>,
    run_agent: Option<RunAgent>,
    skip_hooks: bool,
    no_multiplexer: bool,
    dry_run: bool,
}

#[derive(Debug, Default, Deserialize)]
#[serde(default, deny_unknown_fields)]
struct AddRepoBody {
    env: Option<BTreeMap<String, String>>,
    /// Required: a registered repository name.
    repo: String,
    target_branch: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(untagged)]
enum RepoSpec {
    Name(String),
    Entry {
        name: String,
        target_branch: Option<String>,
    },
}

#[derive(Debug, Default, Deserialize)]
#[serde(default, deny_unknown_fields)]
struct CreateBody {
    env: Option<BTreeMap<String, String>>,
    /// Required.
    title: Option<String>,
    /// Generated when absent.
    name: Option<String>,
    description: Option<String>,
    branch: Option<String>,
    multiplexer: Option<MultiplexerKind>,
    /// Names or `{name, target_branch}`; either `repos` or `template` is required.
    repos: Option<Vec<RepoSpec>>,
    template: Option<String>,
    /// Start the Workspace right after creating it.
    start: bool,
    run_agent: Option<RunAgent>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(default, deny_unknown_fields)]
struct EnvOnlyBody {
    env: Option<BTreeMap<String, String>>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(default, deny_unknown_fields)]
struct RegisterBody {
    env: Option<BTreeMap<String, String>>,
    /// Defaults to the directory name of `path`.
    name: Option<String>,
    /// Required.
    path: String,
    default_target_branch: Option<String>,
}

// ---- submission -----------------------------------------------------------

/// Runs synchronous preparation off the async runtime.
async fn prepare<T, F>(state: &AppState, work: F) -> Result<T, ApiError>
where
    F: FnOnce(&ConfigManager) -> Result<T, ApiError> + Send + 'static,
    T: Send + 'static,
{
    let mgr = ConfigManager::with_base_dir(state.base_dir.clone());
    tokio::task::spawn_blocking(move || work(&mgr))
        .await
        .map_err(|e| ApiError::internal(format!("check task failed: {e}")))?
}

/// Claims `key` (409 `busy` when an Operation already runs on it), starts the
/// job on a blocking thread and answers 202 with the new Operation.
fn spawn(state: &AppState, kind: &'static str, key: &str, job: Job) -> Accepted {
    let handle = state
        .operations
        .start(kind, key, &state.activity)
        .map_err(|busy| {
            ApiError::busy(
                format!("{key} already has operation {} running", busy.operation_id),
                busy.operation_id,
            )
        })?;
    let view = state.operations.view(handle.id());
    let mgr = ConfigManager::with_base_dir(state.base_dir.clone());
    let runner = state.runner.clone();
    // Detached on purpose: closing the client connection never stops the work.
    // A panic drops the handle, which records the failure.
    tokio::task::spawn_blocking(move || {
        let runner = runner.make();
        let sink = handle.sink();
        let ctx = RequestContext::new(&sink, job.env);
        match (job.work)(&mgr, &runner, &ctx) {
            Ok(result) => handle.succeed(result),
            Err(e) => handle.fail(operations::OperationError::from_service_error(&e)),
        }
    });
    Ok((StatusCode::ACCEPTED, Json(view.unwrap_or(Value::Null))))
}

fn session_json(outcome: &SessionOutcome, agent_cli: Option<String>) -> Value {
    json!({
        "workspace": outcome.workspace,
        "dry_run": outcome.dry_run,
        "attach": outcome.attach.as_ref().map(|a| json!({"program": a.program, "args": a.args})),
        "agent_cli": agent_cli,
    })
}

/// The agent command stored on the Workspace, for the Consumer's attach.
fn stored_agent_cli(mgr: &ConfigManager, name: &str) -> Option<String> {
    mgr.load_workspace(name)
        .ok()
        .and_then(|(_, ws)| ws.agent_cli)
}

fn require(mgr: &ConfigManager, name: &str) -> Result<(), ApiError> {
    api::require_workspace(mgr, name)
}

// ---- handlers -------------------------------------------------------------

async fn start(State(state): State<AppState>, Path(name): Path<String>, body: Bytes) -> Accepted {
    let body: StartBody = parse(&body)?;
    let check = name.clone();
    prepare(&state, move |mgr| require(mgr, &check)).await?;
    let req = StartRequest {
        name: name.clone(),
        multiplexer: body.multiplexer,
        no_multiplexer: body.no_multiplexer,
        run_agent: run_agent(body.run_agent),
    };
    let job = Job {
        env: snapshot(body.env),
        work: Box::new(move |mgr, runner, ctx| {
            let outcome = start_workspace(mgr, runner, ctx, &req)?;
            Ok(session_json(
                &outcome,
                stored_agent_cli(mgr, &outcome.workspace),
            ))
        }),
    };
    spawn(&state, "start", &name, job)
}

async fn reopen(State(state): State<AppState>, Path(name): Path<String>, body: Bytes) -> Accepted {
    let body: ReopenBody = parse(&body)?;
    let sources = ReopenSources::parse(&body.from)
        .map_err(|e| ApiError::bad_request(format!("invalid from: {e:#}")))?;
    let check = name.clone();
    prepare(&state, move |mgr| require(mgr, &check)).await?;
    let req = ReopenRequest {
        name: name.clone(),
        options: ReopenOptions {
            multiplexer: body.multiplexer,
            sources,
            overwrite_repos: body.overwrite.into_iter().collect::<BTreeSet<_>>(),
        },
        run_agent: run_agent(body.run_agent),
        skip_hooks: body.skip_hooks,
        no_multiplexer: body.no_multiplexer,
        dry_run: body.dry_run,
    };
    let job = Job {
        env: snapshot(body.env),
        work: Box::new(move |mgr, runner, ctx| {
            // Questions have no one to answer them: the non-interactive
            // prompt turns each into an error naming the missing option.
            let outcome =
                reopen_workspace(mgr, runner, ctx, &mut NonInteractiveReopenPrompt, &req)?;
            Ok(session_json(
                &outcome,
                stored_agent_cli(mgr, &outcome.workspace),
            ))
        }),
    };
    spawn(&state, "reopen", &name, job)
}

async fn add_repo(
    State(state): State<AppState>,
    Path(name): Path<String>,
    body: Bytes,
) -> Accepted {
    let body: AddRepoBody = parse(&body)?;
    if body.repo.trim().is_empty() {
        return Err(ApiError::bad_request("repo is required"));
    }
    let check = name.clone();
    prepare(&state, move |mgr| require(mgr, &check)).await?;
    let req = AddRepositoryRequest {
        workspace: name.clone(),
        repo: body.repo,
        target_branch: body.target_branch,
    };
    let job = Job {
        env: snapshot(body.env),
        work: Box::new(move |mgr, runner, ctx| {
            let global = mgr.load_global_config()?;
            let result = add_repo_to_workspace(mgr, &global, runner, ctx, &req)?;
            Ok(json!({
                "repo": result.repo,
                "workspace": result.workspace,
                "target_branch": result.target_branch,
                "workspace_branch": result.workspace_branch,
                "worktree_path": result.worktree_path,
                "terminal": match result.terminal {
                    TerminalUpdate::Updated => "updated",
                    TerminalUpdate::Absent => "absent",
                },
            }))
        }),
    };
    spawn(&state, "add-repo", &name, job)
}

async fn create(State(state): State<AppState>, body: Bytes) -> Accepted {
    let body: CreateBody = parse(&body)?;
    // Missing parameters are the caller's mistake; there is no wizard here.
    let title = body.title.clone().unwrap_or_default();
    if title.trim().is_empty() {
        return Err(ApiError::bad_request("title is required"));
    }
    if body.repos.is_none() && body.template.is_none() {
        return Err(ApiError::bad_request(
            "either repos or template is required",
        ));
    }
    if body.repos.as_ref().is_some_and(Vec::is_empty) && body.template.is_none() {
        return Err(ApiError::bad_request("repos must not be empty"));
    }
    if let Some(name) = &body.name {
        if !name::is_config_name(name) {
            return Err(ApiError::bad_request(format!(
                "workspace name '{name}' must use only ASCII letters, numbers, '-' and '_'"
            )));
        }
    }
    // The busy key must be known before the Operation starts, so a generated
    // name is picked here and passed to the service as if it were given.
    let given = body.name.clone();
    let name = prepare(&state, move |mgr| match given {
        Some(name) => Ok(name),
        None => {
            let existing: Vec<String> = mgr
                .list_workspaces(None::<&[WorkspaceStatus]>)
                .map_err(|e| ApiError::internal(format!("{e:#}")))?
                .iter()
                .map(|w| w.name.clone())
                .collect();
            Ok(NameGenerator::new().generate_avoiding(&existing))
        }
    })
    .await?;
    let req = CreateRequest {
        title: Some(title),
        name: Some(name.clone()),
        description: body.description,
        branch: body.branch,
        multiplexer: body.multiplexer,
        repos: body.repos.map(|repos| {
            repos
                .into_iter()
                .map(|r| match r {
                    RepoSpec::Name(name) => (name, None),
                    RepoSpec::Entry {
                        name,
                        target_branch,
                    } => (name, target_branch),
                })
                .collect()
        }),
        template: body.template,
        start: body.start,
        run_agent: run_agent(body.run_agent),
    };
    let job = Job {
        env: snapshot(body.env),
        work: Box::new(move |mgr, runner, ctx| {
            let runner = EnvRunner::new(runner, &ctx.env);
            let outcome = create_workspace(mgr, &runner, ctx, &req)?;
            let name = outcome.workspace.name.clone();
            let mut result = json!({
                "workspace": name,
                "status": "pending",
                "title": outcome.workspace.title,
                "branch": outcome.workspace.branch,
                "repos": outcome.workspace.repos.iter().map(|r| &r.name).collect::<Vec<_>>(),
                "started": false,
                "attach": null,
                "agent_cli": outcome.workspace.agent_cli,
            });
            // Same sequence as the command line: create, then start. A start
            // failure leaves the Workspace pending and fails the Operation.
            if outcome.after_create.should_start() {
                let started = start_workspace(
                    mgr,
                    &runner,
                    ctx,
                    &StartRequest {
                        name: name.clone(),
                        multiplexer: None,
                        no_multiplexer: false,
                        run_agent: outcome.after_create.run_agent_arg(),
                    },
                )?;
                result["status"] = json!("in_progress");
                result["started"] = json!(true);
                result["attach"] = session_json(&started, None)["attach"].clone();
                result["agent_cli"] = json!(stored_agent_cli(mgr, &name));
            }
            Ok(result)
        }),
    };
    spawn(&state, "create", &name, job)
}

/// Deletes one archived (done or canceled) Workspace: its directory and its
/// stored configuration.
async fn prune(State(state): State<AppState>, Path(name): Path<String>, body: Bytes) -> Accepted {
    let body: EnvOnlyBody = parse(&body)?;
    let check = name.clone();
    let status = prepare(&state, move |mgr| {
        require(mgr, &check)?;
        let (status, _) = mgr
            .load_workspace(&check)
            .map_err(|e| ApiError::internal(format!("{e:#}")))?;
        match status {
            WorkspaceStatus::Done | WorkspaceStatus::Canceled => Ok(status),
            other => Err(ApiError::bad_request(format!(
                "workspace '{check}' is {}; only done or canceled workspaces can be deleted",
                other.as_str()
            ))),
        }
    })
    .await?;
    let req = PruneRequest {
        all: false,
        targets: vec![PruneTarget {
            name: name.clone(),
            status,
        }],
    };
    let job = Job {
        env: snapshot(body.env),
        work: Box::new(move |mgr, _runner, ctx| {
            let outcome = prune_workspaces(mgr, ctx, &req)?;
            Ok(json!({ "pruned": outcome.pruned }))
        }),
    };
    spawn(&state, "prune", &name, job)
}

async fn register(State(state): State<AppState>, body: Bytes) -> Accepted {
    let body: RegisterBody = parse(&body)?;
    if body.path.trim().is_empty() {
        return Err(ApiError::bad_request("path is required"));
    }
    let name = &body.name;
    if let Some(name) = name {
        if !name::is_config_name(name) {
            return Err(ApiError::bad_request(format!("invalid repo name {name:?}")));
        }
    }
    let key = format!("repo:{}", repo_add_base_name(&body.name, &body.path));
    let req = RegisterRepoRequest {
        name: body.name,
        path: body.path,
        default_target_branch: body.default_target_branch,
    };
    let job = Job {
        env: snapshot(body.env),
        work: Box::new(move |mgr, _runner, ctx| {
            let outcome = register_repo(mgr, ctx, &req)?;
            Ok(json!({ "name": outcome.name, "path": outcome.path }))
        }),
    };
    spawn(&state, "repo-add", &key, job)
}

async fn unregister(
    State(state): State<AppState>,
    Path(name): Path<String>,
    body: Bytes,
) -> Accepted {
    let body: EnvOnlyBody = parse(&body)?;
    if !name::is_config_name(&name) {
        return Err(ApiError::bad_request(format!("invalid repo name {name:?}")));
    }
    let check = name.clone();
    prepare(&state, move |mgr| {
        let known = mgr
            .list_repos()
            .map_err(|e| ApiError::internal(format!("{e:#}")))?;
        if known.contains(&check) {
            Ok(())
        } else {
            Err(ApiError::not_found(format!("repo '{check}' not found")))
        }
    })
    .await?;
    let req = RemoveRepoRequest { name: name.clone() };
    let job = Job {
        env: snapshot(body.env),
        work: Box::new(move |mgr, _runner, ctx| {
            remove_repo(mgr, ctx, &req)?;
            Ok(json!({ "name": req.name }))
        }),
    };
    spawn(&state, "repo-remove", &format!("repo:{name}"), job)
}
