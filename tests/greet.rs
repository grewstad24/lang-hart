use std::env;
use std::fs;
use std::io::Write;
use std::path::PathBuf;
use std::process::{Command, Stdio};

#[test]
fn test_greet_program() {
    let project_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("projects")
        .join("greet");

    let temp_dir = env::temp_dir().join("snore_test_greet");
    let _ = fs::remove_dir_all(&temp_dir);
    fs::create_dir_all(&temp_dir).expect("failed to create temp test directory");

    // 1. Compile
    let compile_status = Command::new("snore")
        .arg(&project_dir)
        .current_dir(&temp_dir)
        .status()
        .expect("failed to execute 'snore'");

    assert!(
        compile_status.success(),
        "compiler failed: {compile_status}"
    );

    let bin = temp_dir.join(format!("main{}", env::consts::EXE_SUFFIX));
    assert!(bin.exists(), "binary missing at '{}'", bin.display());

    // 2. Run with piped stdin/stdout
    let mut child = Command::new(&bin)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .expect("failed to spawn binary");

    // Send input and close pipe
    if let Some(mut stdin) = child.stdin.take() {
        stdin
            .write_all(b"Alice\n")
            .expect("failed to write to stdin");
    }

    let output = child.wait_with_output().expect("failed to read output");
    assert!(output.status.success());

    let stdout = std::str::from_utf8(&output.stdout).expect("invalid utf-8");
    assert_eq!(stdout, "Enter name: Hello, Alice\n");

    let _ = fs::remove_dir_all(&temp_dir);
}
