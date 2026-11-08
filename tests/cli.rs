use std::process::{Command, Output};

fn run(args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_jumeaux"))
        .args(args)
        .output()
        .expect("failed to execute jumeaux")
}

#[test]
fn argument_errors_exit_with_one_and_write_to_stderr() {
    for args in [
        &[][..],
        &["unknown"],
        &["run"],
        &["server", "--port", "abc"],
    ] {
        let output = run(args);
        assert_eq!(output.status.code(), Some(1), "args: {args:?}");
        assert!(output.stdout.is_empty(), "args: {args:?}");
        assert!(!output.stderr.is_empty(), "args: {args:?}");
        assert!(
            !String::from_utf8_lossy(&output.stderr).contains("未実装"),
            "args: {args:?}"
        );
    }
}

#[test]
fn help_and_version_exit_with_zero_and_write_to_stdout() {
    for args in [
        &["--help"][..],
        &["--version"],
        &["run", "--help"],
        &["retry", "--help"],
        &["init", "--help"],
        &["server", "--help"],
        &["viewer", "--help"],
    ] {
        let output = run(args);
        assert_eq!(output.status.code(), Some(0), "args: {args:?}");
        assert!(!output.stdout.is_empty(), "args: {args:?}");
        assert!(output.stderr.is_empty(), "args: {args:?}");
    }
}

#[test]
fn excessive_verbosity_is_an_argument_error_for_all_logging_commands() {
    for command in ["run", "retry", "server"] {
        for flags in [&["-vvvv"][..], &["-vv", "-vv"], &["-v", "-v", "-v", "-v"]] {
            let mut args = vec![command];
            if command != "server" {
                args.push("input");
            }
            args.extend_from_slice(flags);
            let output = run(&args);
            assert_eq!(output.status.code(), Some(1), "args: {args:?}");
            assert!(output.stdout.is_empty(), "args: {args:?}");
            let stderr = String::from_utf8_lossy(&output.stderr);
            assert!(
                stderr.contains("error:"),
                "args: {args:?}, stderr: {stderr}"
            );
            assert!(!stderr.contains("未実装"), "args: {args:?}");
        }
    }
}

#[test]
fn abbreviated_options_reach_retry_report_loading() {
    let output = run(&["retry", "r", "--desc=x", "--thr=1"]);
    assert_eq!(output.status.code(), Some(1));
    assert!(output.stdout.is_empty());
    assert!(String::from_utf8_lossy(&output.stderr).contains("IOError: r:"));
}
