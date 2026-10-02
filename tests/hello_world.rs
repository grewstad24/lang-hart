use std::env;
use std::fs;
use std::path::PathBuf;
use std::process::Command;

#[test]
fn test_hello_world_project() {
    // 1. Resolve path to tests/projects/hello_world relative to crate root
    let project_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("projects")
        .join("hello_world");

    // 2. Set up a clean temporary working directory for build artifacts
    let temp_dir = env::temp_dir().join("snore_test_hello_world");
    let _ = fs::remove_dir_all(&temp_dir);
    fs::create_dir_all(&temp_dir).expect("failed to create temp test directory");

    // 3. Run the `snore` compiler from PATH: `snore <project_dir>`
    let compile_status = Command::new("snore")
        .arg(&project_dir)
        .current_dir(&temp_dir)
        .status()
        .expect("failed to execute 'snore' executable from PATH");

    assert!(
        compile_status.success(),
        "snore compiler failed with status: {}",
        compile_status
    );

    // 4. Locate compiled executable (`main` or `main.exe` on Windows)
    let compiled_binary = temp_dir.join(format!("main{}", env::consts::EXE_SUFFIX));
    assert!(
        compiled_binary.exists(),
        "expected compiled executable at '{}'",
        compiled_binary.display()
    );

    // 5. Execute compiled binary and verify stdout output
    let run_output = Command::new(&compiled_binary)
        .output()
        .expect("failed to execute compiled binary");

    assert!(
        run_output.status.success(),
        "compiled binary failed with status: {}",
        run_output.status
    );

    let stdout = String::from_utf8_lossy(&run_output.stdout);
    assert_eq!(stdout.trim(), "Hello World!");

    // Clean up temporary workspace
    let _ = fs::remove_dir_all(&temp_dir);
}
