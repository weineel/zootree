//! Daemon skeleton tests. The Daemon runs in-process on a unix socket inside a
//! temporary directory; assertions go through HTTP, the discovery file and
//! the filesystem only. Nothing here touches the real ~/.config/zootree.

use std::os::unix::fs::{FileTypeExt, PermissionsExt};
use std::time::{Duration, Instant};
use tempfile::TempDir;
use zootree::config::ConfigManager;
use zootree::daemon::client::{request, HttpResponse};
use zootree::daemon::endpoint::{self, Discovery, Liveness};
use zootree::daemon::{self, DaemonHandle, DaemonOptions, DaemonState};

const T: Duration = Duration::from_secs(5);

fn options(tmp: &TempDir) -> DaemonOptions {
    let mgr = ConfigManager::with_base_dir(tmp.path().to_path_buf());
    let mut options = DaemonOptions::new(mgr.base_dir.clone());
    options.idle_timeout = Duration::from_secs(60);
    options
}

fn get(daemon: &DaemonHandle, path: &str) -> HttpResponse {
    request(&daemon.socket, "GET", path, Some(&daemon.token), T).unwrap()
}

fn wait_for(mut condition: impl FnMut() -> bool) -> bool {
    let deadline = Instant::now() + Duration::from_secs(5);
    while Instant::now() < deadline {
        if condition() {
            return true;
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    false
}

#[test]
fn status_reports_versions_and_capabilities() {
    let tmp = TempDir::new().unwrap();
    let daemon = daemon::start(options(&tmp)).unwrap();

    let response = get(&daemon, "/status");
    assert_eq!(response.status, 200);
    let body = response.json().unwrap();
    assert_eq!(body["version"], env!("CARGO_PKG_VERSION"));
    assert_eq!(body["protocol_version"], 1);
    assert_eq!(body["min_compatible_protocol_version"], 1);
    assert!(body["capabilities"]
        .as_array()
        .unwrap()
        .contains(&"status".into()));
    assert_eq!(body["pid"], std::process::id());
    assert!(body["started_at"].is_string());

    daemon.shutdown();
    daemon.join().unwrap();
}

#[test]
fn requests_without_a_valid_token_are_rejected_with_a_stable_code() {
    let tmp = TempDir::new().unwrap();
    let daemon = daemon::start(options(&tmp)).unwrap();

    for token in [None, Some("wrong")] {
        let response = request(&daemon.socket, "GET", "/status", token, T).unwrap();
        assert_eq!(response.status, 401);
        assert_eq!(response.json().unwrap()["error"]["code"], "unauthorized");
    }
    let response = request(&daemon.socket, "POST", "/shutdown", None, T).unwrap();
    assert_eq!(response.status, 401);
    // Still up after the rejected shutdown.
    assert_eq!(get(&daemon, "/status").status, 200);

    let response = get(&daemon, "/nope");
    assert_eq!(response.status, 404);
    assert_eq!(response.json().unwrap()["error"]["code"], "not_found");

    daemon.shutdown();
    daemon.join().unwrap();
}

#[test]
fn socket_discovery_file_and_directory_are_private_to_the_user() {
    let tmp = TempDir::new().unwrap();
    let daemon = daemon::start(options(&tmp)).unwrap();
    let mode = |p: &std::path::Path| std::fs::metadata(p).unwrap().permissions().mode() & 0o777;

    assert_eq!(mode(&daemon.paths.socket), 0o600);
    assert_eq!(mode(&daemon.paths.discovery), 0o600);
    assert_eq!(mode(&daemon.paths.dir), 0o700);

    daemon.shutdown();
    daemon.join().unwrap();
}

#[test]
fn discovery_file_describes_the_running_daemon() {
    let tmp = TempDir::new().unwrap();
    let daemon = daemon::start(options(&tmp)).unwrap();

    let discovery = Discovery::read(&daemon.paths.discovery).unwrap().unwrap();
    assert_eq!(discovery.pid, std::process::id());
    assert_eq!(discovery.endpoint, daemon.socket);
    assert_eq!(discovery.token, daemon.token);
    assert_eq!(discovery.protocol_version, 1);
    assert!(chrono::DateTime::parse_from_rfc3339(&discovery.started_at).is_ok());

    match daemon::inspect(&daemon.paths).unwrap() {
        DaemonState::Running(d) => assert_eq!(d, discovery),
        other => panic!("expected running, got {other:?}"),
    }

    daemon.shutdown();
    daemon.join().unwrap();
}

#[test]
fn second_start_is_rejected_while_first_is_running() {
    let tmp = TempDir::new().unwrap();
    let first = daemon::start(options(&tmp)).unwrap();

    let error = daemon::start(options(&tmp))
        .err()
        .expect("must be rejected");
    assert!(
        format!("{error:#}").contains("already running"),
        "unexpected message: {error:#}"
    );
    // The first instance is unharmed.
    assert_eq!(get(&first, "/status").status, 200);

    first.shutdown();
    first.join().unwrap();
}

#[test]
fn concurrent_starts_yield_exactly_one_daemon() {
    let tmp = TempDir::new().unwrap();
    let results: Vec<_> = (0..8)
        .map(|_| {
            let options = options(&tmp);
            std::thread::spawn(move || daemon::start(options))
        })
        .collect::<Vec<_>>()
        .into_iter()
        .map(|t| t.join().unwrap())
        .collect();

    let winners: Vec<_> = results.into_iter().filter_map(|r| r.ok()).collect();
    assert_eq!(winners.len(), 1);
    assert_eq!(get(&winners[0], "/status").status, 200);
    for winner in winners {
        winner.shutdown();
        winner.join().unwrap();
    }
}

#[test]
fn shutdown_request_stops_the_daemon_and_removes_its_own_files() {
    let tmp = TempDir::new().unwrap();
    let daemon = daemon::start(options(&tmp)).unwrap();
    let paths = daemon.paths.clone();

    let response = request(&daemon.socket, "POST", "/shutdown", Some(&daemon.token), T).unwrap();
    assert_eq!(response.status, 200);
    daemon.join().unwrap();

    assert!(!paths.socket.exists());
    assert!(!paths.discovery.exists());
    assert!(matches!(
        daemon::inspect(&paths).unwrap(),
        DaemonState::NotRunning
    ));

    // A fresh start works afterwards.
    let again = daemon::start(options(&tmp)).unwrap();
    again.shutdown();
    again.join().unwrap();
}

#[test]
fn stale_socket_and_discovery_from_a_dead_daemon_are_taken_over() {
    let tmp = TempDir::new().unwrap();
    let options = options(&tmp);
    let paths = options.paths().unwrap();
    paths.ensure_dir().unwrap();

    // A socket file nobody listens on, plus a discovery file pointing at it.
    drop(std::os::unix::net::UnixListener::bind(&paths.socket).unwrap());
    assert!(paths.socket.exists());
    Discovery {
        pid: 999_999,
        endpoint: paths.socket.clone(),
        token: "stale".to_string(),
        started_at: "2020-01-01T00:00:00Z".to_string(),
        protocol_version: 1,
    }
    .write(&paths.discovery)
    .unwrap();
    assert!(matches!(
        daemon::inspect(&paths).unwrap(),
        DaemonState::NotRunning
    ));

    let daemon = daemon::start(options).unwrap();
    assert_ne!(daemon.token, "stale");
    assert_eq!(get(&daemon, "/status").status, 200);
    assert_eq!(
        Discovery::read(&paths.discovery).unwrap().unwrap().token,
        daemon.token
    );

    daemon.shutdown();
    daemon.join().unwrap();
}

#[test]
fn something_at_the_endpoint_that_is_not_a_socket_is_never_removed() {
    let tmp = TempDir::new().unwrap();
    let options = options(&tmp);
    let paths = options.paths().unwrap();
    paths.ensure_dir().unwrap();
    std::fs::write(&paths.socket, "precious").unwrap();

    let error = daemon::start(options).err().expect("must refuse");
    assert!(format!("{error:#}").contains("not a socket"));
    assert_eq!(std::fs::read_to_string(&paths.socket).unwrap(), "precious");
}

#[test]
fn an_endpoint_whose_liveness_cannot_be_judged_is_left_in_place() {
    let tmp = TempDir::new().unwrap();
    let options = options(&tmp);
    let paths = options.paths().unwrap();
    paths.ensure_dir().unwrap();

    // Someone else's live listener we are not permitted to connect to.
    let _foreign = std::os::unix::net::UnixListener::bind(&paths.socket).unwrap();
    std::fs::set_permissions(&paths.socket, std::fs::Permissions::from_mode(0o000)).unwrap();
    if std::os::unix::net::UnixStream::connect(&paths.socket).is_ok() {
        // Running as root: permissions do not apply, nothing to test.
        return;
    }
    assert!(matches!(
        endpoint::probe(&paths.socket, Duration::from_secs(2)),
        Liveness::Unknown(_)
    ));

    let error = daemon::start(options).err().expect("must decline");
    assert!(format!("{error:#}").contains("cannot tell"), "{error:#}");
    assert!(std::fs::symlink_metadata(&paths.socket)
        .unwrap()
        .file_type()
        .is_socket());
}

#[test]
fn only_refusal_or_absence_prove_death() {
    use std::io::{Error, ErrorKind};
    for kind in [ErrorKind::ConnectionRefused, ErrorKind::NotFound] {
        assert_eq!(
            endpoint::classify_connect_error(&Error::from(kind)),
            Liveness::Dead
        );
    }
    for kind in [
        ErrorKind::PermissionDenied,
        ErrorKind::TimedOut,
        ErrorKind::WouldBlock,
        ErrorKind::Other,
    ] {
        assert!(matches!(
            endpoint::classify_connect_error(&Error::from(kind)),
            Liveness::Unknown(_)
        ));
    }
}

#[test]
fn daemons_of_different_major_versions_use_different_endpoints_and_coexist() {
    assert_eq!(endpoint::endpoint_name_for_version("1.4.2"), "v1");
    assert_eq!(endpoint::endpoint_name_for_version("2.0.0"), "v2");
    assert_eq!(endpoint::endpoint_name_for_version("0.1.4"), "v0.1");
    assert_eq!(endpoint::endpoint_name_for_version("0.2.0"), "v0.2");

    let tmp = TempDir::new().unwrap();
    let mut one = options(&tmp);
    one.endpoint_name = endpoint::endpoint_name_for_version("1.0.0");
    let mut two = options(&tmp);
    two.endpoint_name = endpoint::endpoint_name_for_version("2.0.0");

    let a = daemon::start(one).unwrap();
    let b = daemon::start(two).unwrap();
    assert_ne!(a.socket, b.socket);
    assert_eq!(get(&a, "/status").status, 200);
    assert_eq!(get(&b, "/status").status, 200);

    a.shutdown();
    b.shutdown();
    a.join().unwrap();
    b.join().unwrap();
}

#[test]
fn idle_daemon_exits_after_the_timeout() {
    let tmp = TempDir::new().unwrap();
    let mut options = options(&tmp);
    options.idle_timeout = Duration::from_millis(300);
    let daemon = daemon::start(options).unwrap();
    let paths = daemon.paths.clone();

    // join() only returns if the Daemon exited on its own.
    daemon.join().unwrap();
    assert!(!paths.socket.exists());
    assert!(!paths.discovery.exists());
}

#[test]
fn running_operation_prevents_idle_exit_until_it_finishes() {
    let tmp = TempDir::new().unwrap();
    let mut options = options(&tmp);
    options.idle_timeout = Duration::from_millis(300);
    let daemon = daemon::start(options).unwrap();

    let operation = daemon.activity().begin_operation();
    std::thread::sleep(Duration::from_millis(900));
    assert_eq!(get(&daemon, "/status").status, 200, "exited while busy");

    drop(operation);
    let paths = daemon.paths.clone();
    daemon.join().unwrap();
    assert!(!paths.socket.exists());
}

#[test]
fn recent_requests_postpone_idle_exit() {
    let tmp = TempDir::new().unwrap();
    let mut options = options(&tmp);
    options.idle_timeout = Duration::from_millis(600);
    let daemon = daemon::start(options).unwrap();

    for _ in 0..4 {
        std::thread::sleep(Duration::from_millis(250));
        assert_eq!(get(&daemon, "/status").status, 200);
    }
    assert!(wait_for(|| !daemon.paths.socket.exists()));
    daemon.join().unwrap();
}
