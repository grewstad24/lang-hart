use std::env;
use std::fs;
use std::io::Write;
use std::path::PathBuf;
use std::process::{Command, Stdio};

#[test]
fn test_read_project() {
    // 1. Resolve path to the target test project
    let project_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("projects")
        .join("read");

    // 2. Set up an isolated temporary workspace
    let temp_dir = env::temp_dir().join("snore_test_read");
    let _ = fs::remove_dir_all(&temp_dir);
    fs::create_dir_all(&temp_dir).expect("failed to create temp test directory");

    // 3. Compile the project
    let compile_status = Command::new("snore")
        .arg(&project_dir)
        .current_dir(&temp_dir)
        .status()
        .expect("failed to execute 'snore' binary from PATH");

    assert!(
        compile_status.success(),
        "compilation failed with exit status: {compile_status}"
    );

    // 4. Ensure binary exists
    let compiled_binary = temp_dir.join(format!("main{}", env::consts::EXE_SUFFIX));
    assert!(
        compiled_binary.exists(),
        "compiled binary not found at '{}'",
        compiled_binary.display()
    );

    // 5. Spawn child with piped stdin and captured stdout
    let mut child = Command::new(&compiled_binary)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("failed to spawn compiled binary");

    // Write input and explicitly drop stdin to trigger EOF/unblock read
    if let Some(mut stdin) = child.stdin.take() {
        stdin
            .write_all(b"foo\n")
            .expect("failed to write to child stdin");
    }

    let run_output = child
        .wait_with_output()
        .expect("failed to wait on child process");

    assert!(
        run_output.status.success(),
        "binary crashed with exit status: {}",
        run_output.status
    );

    let stdout = std::str::from_utf8(&run_output.stdout).expect("stdout is not valid UTF-8");
    assert_eq!(
        stdout, "You said: foo\n",
        "binary output mismatch: expected 'You said: foo\\n', got '{stdout}'"
    );

    // 6. Cleanup
    let _ = fs::remove_dir_all(&temp_dir);
}
