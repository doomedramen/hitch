//! Integration test: `hitch` with no args prints help and exits successfully.

#[test]
fn hitch_no_args_prints_help_and_exits_success() {
    let exe = std::env::var("CARGO_BIN_EXE_hitch").expect("CARGO_BIN_EXE_hitch should be set");
    // Test-only: spawns the built hitch binary directly to check its no-args
    // behavior; nulls stdin for the same reason as the shared test harness.
    #[allow(clippy::disallowed_methods)]
    let out = std::process::Command::new(exe)
        .stdin(std::process::Stdio::null())
        .output()
        .expect("hitch should run");

    assert!(
        out.status.success(),
        "expected exit 0, got {:?}\nstderr:\n{}",
        out.status.code(),
        String::from_utf8_lossy(&out.stderr)
    );

    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(
        stdout.contains("Usage:") && stdout.contains("hitch"),
        "expected help output, got:\n{}",
        stdout
    );
}
