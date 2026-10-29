use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use inkwell::context::Context;

use snore::codegen::Codegen;
use snore::lexer::Lexer;
use snore::parser::Parser;

// Helper function to locate the `~/.snore` directory.
fn get_snore_home_dir() -> Option<PathBuf> {
    env::var_os("HOME")
        .or_else(|| env::var_os("USERPROFILE"))
        .map(|home| PathBuf::from(home).join(".snore"))
}

fn collect_sources(dirs: &[PathBuf]) -> std::io::Result<Vec<PathBuf>> {
    let mut files = Vec::new();

    fn visit(dir: &Path, files: &mut Vec<PathBuf>) -> std::io::Result<()> {
        if !dir.is_dir() {
            return Ok(());
        }

        for entry in fs::read_dir(dir)? {
            let entry = entry?;
            let path = entry.path();

            if path.is_dir() {
                visit(&path, files)?;
            } else if path.extension().is_some_and(|ext| ext == "zzz") {
                files.push(path);
            }
        }

        Ok(())
    }

    for dir in dirs {
        visit(dir, &mut files)?;
    }

    files.sort();
    files.dedup();

    Ok(files)
}

fn main() {
    let project_dir = match env::args().nth(1) {
        Some(path) => PathBuf::from(path),

        None => {
            eprintln!("usage: snore <project-directory>");
            std::process::exit(1);
        }
    };

    if !project_dir.is_dir() {
        eprintln!("'{}' is not a directory", project_dir.display());
        std::process::exit(1);
    }

    let mut search_dirs = Vec::new();

    if let Some(snore_home) = get_snore_home_dir() {
        if snore_home.is_dir() {
            search_dirs.push(snore_home);
        }
    }

    search_dirs.push(project_dir.clone());

    let source_files = collect_sources(&search_dirs).unwrap_or_else(|error| {
        eprintln!("failed to read source directories: {}", error);
        std::process::exit(1);
    });

    if source_files.is_empty() {
        eprintln!("no .zzz files found in project or ~/.snore");
        std::process::exit(1);
    }

    let mut source = String::new();

    for path in &source_files {
        let file_source = fs::read_to_string(path).unwrap_or_else(|error| {
            eprintln!("failed to read '{}': {}", path.display(), error);
            std::process::exit(1);
        });

        source.push_str(&file_source);
        source.push_str("\n\n");
    }

    let tokens = Lexer::new(source).tokenize().unwrap_or_else(|error| {
        eprintln!("lexer error: {}", error);
        std::process::exit(1);
    });

    let mut parser = Parser::new(tokens);

    let program = parser.parse_program().unwrap_or_else(|error| {
        eprintln!("parser error: {}", error);
        std::process::exit(1);
    });

    let context = Context::create();

    let codegen = Codegen::new(&context, "main");

    let module = codegen.generate(&program).unwrap_or_else(|error| {
        eprintln!("compiler error: {}", error);
        std::process::exit(1);
    });

    if let Err(err) = module.verify() {
        eprintln!("llvm verification error:\n{}", err.to_string());
        std::process::exit(1);
    }

    let ir_path = Path::new("main.ll");
    if let Err(err) = module.print_to_file(ir_path) {
        eprintln!("failed to write LLVM IR: {}", err);
        std::process::exit(1);
    }

    let status = Command::new("clang")
        .args(["main.ll", "-o", "main"])
        .status()
        .expect("failed to execute clang");

    if !status.success() {
        eprintln!("clang failed with status {}", status);
        std::process::exit(1);
    }

    println!("built main");
}
