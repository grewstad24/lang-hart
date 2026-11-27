use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicUsize, Ordering};

static TEST_COUNTER: AtomicUsize = AtomicUsize::new(0);

fn temp_out_dir(name: &str) -> PathBuf {
    let id = TEST_COUNTER.fetch_add(1, Ordering::SeqCst);
    std::env::temp_dir().join(format!(
        "hart_e2e_{}_{}_{}",
        name,
        std::process::id(),
        id
    ))
}

fn assert_dir_ok(rel_dir: &str) {
    let path = Path::new("docs").join(rel_dir);
    let sanitized = rel_dir.replace('/', "_");
    let out_dir = temp_out_dir(&sanitized);
    let _ = fs::remove_dir_all(&out_dir);

    let bin_path = match hart::driver::compile_project_to(&path, &out_dir) {
        Ok(p) => p,
        Err(err) => {
            let _ = fs::remove_dir_all(&out_dir);
            panic!("expected '{}' to compile cleanly, but failed with:\n{}", rel_dir, err);
        }
    };

    let output = Command::new("sh")
        .env("HOME", std::env::temp_dir())
        .arg(&bin_path)
        .output()
        .unwrap_or_else(|e| panic!("failed to execute '{}': {}", bin_path.display(), e));
    let _ = fs::remove_dir_all(&out_dir);

    assert!(
        output.status.success(),
        "compiled binary for '{}' failed with status {}:\nstdout: {}\nstderr: {}",
        rel_dir,
        output.status,
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

fn assert_dir_err(rel_dir: &str, expected_substr: &str) {
    let path = Path::new("docs").join(rel_dir);
    let sanitized = rel_dir.replace('/', "_");
    let out_dir = temp_out_dir(&sanitized);
    let _ = fs::remove_dir_all(&out_dir);

    let res = hart::driver::compile_project_to(&path, &out_dir);
    let _ = fs::remove_dir_all(&out_dir);

    match res {
        Ok(_) => panic!(
            "expected '{}' to fail with '{}', but it compiled successfully",
            rel_dir, expected_substr
        ),
        Err(err) => {
            assert!(
                err.contains(expected_substr),
                "expected '{}' error containing '{}', got:\n{}",
                rel_dir,
                expected_substr,
                err
            );
        }
    }
}

#[test]
fn ok_types() {
    assert_dir_ok("types_and_casting/tests/ok_types");
}

#[test]
fn ok_atoms_and_coercion() {
    assert_dir_ok("types_and_casting/tests/ok_atoms_and_coercion");
}

#[test]
fn ok_control_flow() {
    assert_dir_ok("control_flow/tests/ok_control_flow");
}

#[test]
fn ok_for_and_defer() {
    assert_dir_ok("control_flow/tests/ok_for_and_defer");
}

#[test]
fn ok_def_intrinsics() {
    assert_dir_ok("functions_and_intrinsics/tests/ok_def_intrinsics");
}

#[test]
fn ok_modules() {
    assert_dir_ok("modules_and_namespaces/tests/ok_modules");
}

#[test]
fn ok_escape_safe() {
    assert_dir_ok("escape_analysis/tests/ok_escape_safe");
}

#[test]
fn ok_stdcore() {
    assert_dir_ok("stdcore/tests/ok_stdcore");
}

#[test]
fn err_ctx_uninitialized() {
    assert_dir_err(
        "functions_and_intrinsics/tests/err_ctx_uninitialized",
        "'@ctx' must be initialized before calling 'def' function",
    );
}

#[test]
fn err_opaque_deref() {
    assert_dir_err("types_and_casting/tests/err_opaque_deref", "cannot dereference untyped pointer '*opaque'");
}

#[test]
fn err_if_non_bool() {
    assert_dir_err("control_flow/tests/err_if_non_bool", "condition must evaluate to Bool");
}

#[test]
fn err_match_non_exhaustive() {
    assert_dir_err("control_flow/tests/err_match_non_exhaustive", "non-exhaustive match expression");
}

#[test]
fn err_break_outside_loop() {
    assert_dir_err("control_flow/tests/err_break_outside_loop", "'break' outside of loop");
}

#[test]
fn err_continue_outside_loop() {
    assert_dir_err("control_flow/tests/err_continue_outside_loop", "'continue' outside of loop");
}

#[test]
fn err_yield_type_mismatch() {
    assert_dir_err("control_flow/tests/err_yield_type_mismatch", "return type mismatch");
}

#[test]
fn err_caller_outside_def() {
    assert_dir_err(
        "functions_and_intrinsics/tests/err_caller_outside_def",
        "'@caller_line_number' can only be used inside a 'def' function",
    );
}

#[test]
fn err_main_is_def() {
    assert_dir_err(
        "functions_and_intrinsics/tests/err_main_is_def",
        "entry point 'main' must be a function ('func'), never a 'def'",
    );
}

#[test]
fn err_discard_error() {
    assert_dir_err("control_flow/tests/err_discard_error", "may be an error and cannot be discarded");
}

#[test]
fn err_immutable_reassign() {
    assert_dir_err("types_and_casting/tests/err_immutable_reassign", "cannot reassign to immutable constant");
}

#[test]
fn err_immutable_ptr_write() {
    assert_dir_err("types_and_casting/tests/err_immutable_ptr_write", "cannot mutate through immutable pointer");
}

#[test]
fn err_uninitialized_read() {
    assert_dir_err(
        "types_and_casting/tests/err_uninitialized_read",
        "mutable variable 'x' may be read before it is initialized",
    );
}

#[test]
fn err_escape_return() {
    assert_dir_err(
        "escape_analysis/tests/err_escape_return",
        "pointer to local stack variable cannot escape function via return",
    );
}

#[test]
fn err_escape_alias() {
    assert_dir_err(
        "escape_analysis/tests/err_escape_alias",
        "pointer to local stack variable cannot escape function via return",
    );
}

#[test]
fn err_escape_reassign() {
    assert_dir_err(
        "escape_analysis/tests/err_escape_reassign",
        "pointer to local stack variable cannot escape function via return",
    );
}

#[test]
fn err_escape_struct() {
    assert_dir_err(
        "escape_analysis/tests/err_escape_struct",
        "pointer to local stack variable cannot escape function via return",
    );
}

#[test]
fn err_escape_tuple() {
    assert_dir_err(
        "escape_analysis/tests/err_escape_tuple",
        "pointer to local stack variable cannot escape function via return",
    );
}

#[test]
fn err_escape_union() {
    assert_dir_err(
        "escape_analysis/tests/err_escape_union",
        "pointer to local stack variable cannot escape function via return",
    );
}

#[test]
fn err_escape_cast() {
    assert_dir_err(
        "escape_analysis/tests/err_escape_cast",
        "pointer to local stack variable cannot escape function via return",
    );
}

#[test]
fn err_escape_out_param() {
    assert_dir_err(
        "escape_analysis/tests/err_escape_out_param",
        "cannot store pointer to local stack variable into dereferenced pointer",
    );
}

#[test]
fn ok_inline_comments() {
    let src = "func add(a: # first # i32, b: # second # i32) -> i32 # eol\n{\n    let s = \"a # b\"\n    return a + b # done #\n}";
    let tokens = hart::syntax::Lexer::new(src.to_string())
        .tokenize()
        .expect("inline comments should lex cleanly");
    assert!(tokens.contains(&hart::syntax::Token::FnKeyword));
    assert!(tokens.contains(&hart::syntax::Token::Return));
    assert!(tokens.contains(&hart::syntax::Token::Identifier("i32".to_string())));
    assert!(tokens.contains(&hart::syntax::Token::String(b"a # b".to_vec())));
    assert!(!tokens.contains(&hart::syntax::Token::Identifier("first".to_string())));
    assert!(!tokens.contains(&hart::syntax::Token::Identifier("eol".to_string())));
}
