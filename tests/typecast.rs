use std::env;
use std::fs;
use std::path::PathBuf;
use std::process::Command;

#[test]
fn test_zero_output_project() {
    // 1. Resolve path to the target test project
    let project_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("projects")
        .join("typecast"); // Adjust to your project's folder name

    // 2. Set up an isolated temporary workspace
    let temp_dir = env::temp_dir().join("snore_test_zero");
    let _ = fs::remove_dir_all(&temp_dir);
    fs::create_dir_all(&temp_dir).expect("failed to create temp test directory");

    // 3. Compile the project and verify compiler exit status
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

    // 5. Execute binary and assert execution success and output
    let run_output = Command::new(&compiled_binary)
        .output()
        .expect("failed to run compiled binary");

    assert!(
        run_output.status.success(),
        "binary crashed with exit status: {}",
        run_output.status
    );

    let stdout = std::str::from_utf8(&run_output.stdout).expect("stdout is not valid UTF-8");
    assert_eq!(stdout.trim(), "0", "binary did not print '0'");

    // 6. Cleanup
    let _ = fs::remove_dir_all(&temp_dir);
}
