use std::collections::HashMap;
use std::os::unix::process::ExitStatusExt;
use std::process::{ExitStatus, Output};
use zootree::runner::{
    CommandRunner, CommandSpec, MockRunner, OutputLine, OutputStream, RealRunner,
};

fn sh(script: &str) -> CommandSpec {
    CommandSpec {
        program: "sh".into(),
        args: vec!["-c".into(), script.into()],
        cwd: None,
        env: HashMap::new(),
        env_remove: vec![],
    }
}

#[test]
fn mock_runner_replays_stdout_then_stderr_lines_in_order() {
    let runner = MockRunner::new();
    runner.push_response(Output {
        status: ExitStatus::from_raw(0),
        stdout: b"one\ntwo\n".to_vec(),
        stderr: b"warn\n".to_vec(),
    });
    let mut lines = Vec::new();
    let output = runner
        .run_streaming(&sh("ignored"), &mut |l| lines.push(l))
        .unwrap();
    assert_eq!(
        lines,
        vec![
            OutputLine {
                stream: OutputStream::Stdout,
                text: "one".into()
            },
            OutputLine {
                stream: OutputStream::Stdout,
                text: "two".into()
            },
            OutputLine {
                stream: OutputStream::Stderr,
                text: "warn".into()
            },
        ]
    );
    assert_eq!(output.stdout, b"one\ntwo\n");
    assert_eq!(runner.take_calls().len(), 1);
}

#[test]
fn mock_runner_streaming_keeps_full_output_on_failure() {
    let runner = MockRunner::new();
    runner.push_response(Output {
        status: ExitStatus::from_raw(1 << 8),
        stdout: Vec::new(),
        stderr: b"boom\nmore\n".to_vec(),
    });
    let mut count = 0;
    let output = runner.run_streaming(&sh("x"), &mut |_| count += 1).unwrap();
    assert!(!output.status.success());
    assert_eq!(count, 2);
    assert_eq!(output.stderr, b"boom\nmore\n");
}

#[test]
fn real_runner_forwards_lines_while_running_and_captures_everything() {
    let mut lines = Vec::new();
    let output = RealRunner
        .run_streaming(
            &sh("echo first; echo err 1>&2; echo last; exit 3"),
            &mut |l| lines.push(l),
        )
        .unwrap();
    assert_eq!(output.status.code(), Some(3));
    assert_eq!(output.stdout, b"first\nlast\n");
    assert_eq!(output.stderr, b"err\n");
    let stdout: Vec<_> = lines
        .iter()
        .filter(|l| l.stream == OutputStream::Stdout)
        .map(|l| l.text.as_str())
        .collect();
    assert_eq!(stdout, vec!["first", "last"]);
    assert!(lines
        .iter()
        .any(|l| l.stream == OutputStream::Stderr && l.text == "err"));
}

#[test]
fn real_runner_delivers_a_line_before_the_command_exits() {
    let start = std::time::Instant::now();
    let mut first_seen = None;
    RealRunner
        .run_streaming(&sh("echo early; sleep 1"), &mut |_| {
            first_seen.get_or_insert(start.elapsed());
        })
        .unwrap();
    assert!(first_seen.unwrap() < std::time::Duration::from_millis(800));
}
