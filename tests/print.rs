use std::env;
use std::fs;
use std::path::PathBuf;
use std::process::Command;

#[test]
fn test_print_project() {
    // 1. Resolve path to tests/projects/print relative to crate root
    let project_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("projects")
        .join("print");

    // 2. Set up a clean temporary workspace
    let temp_dir = env::temp_dir().join("snore_test_print");
    let _ = fs::remove_dir_all(&temp_dir);
    fs::create_dir_all(&temp_dir).expect("failed to create temp test directory");

    // 3. Compile the project using `snore` from PATH
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

    // 4. Locate compiled executable
    let compiled_binary = temp_dir.join(format!("main{}", env::consts::EXE_SUFFIX));
    assert!(
        compiled_binary.exists(),
        "expected compiled executable at '{}'",
        compiled_binary.display()
    );

    // 5. Execute compiled binary and verify stdout
    let run_output = Command::new(&compiled_binary)
        .output()
        .expect("failed to execute compiled binary");

    assert!(
        run_output.status.success(),
        "compiled binary execution failed with status: {}",
        run_output.status
    );

    let stdout = std::str::from_utf8(&run_output.stdout).expect("stdout contained invalid UTF-8");

    assert_eq!(
        stdout.trim(),
        "42 x 4 = 168",
        "unexpected output from binary"
    );

    // 6. Clean up workspace
    let _ = fs::remove_dir_all(&temp_dir);
}
