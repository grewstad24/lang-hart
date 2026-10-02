use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use inkwell::context::Context;

mod ast;
mod checker;
mod codegen;
mod lexer;
mod parser;

use crate::codegen::Codegen;
use crate::lexer::Lexer;
use crate::parser::Parser;

fn collect_sources(dir: &Path) -> std::io::Result<Vec<PathBuf>> {
    let mut files = Vec::new();

    fn visit(dir: &Path, files: &mut Vec<PathBuf>) -> std::io::Result<()> {
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

    visit(dir, &mut files)?;

    files.sort();

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

    let source_files = collect_sources(&project_dir).unwrap_or_else(|error| {
        eprintln!(
            "failed to read project directory '{}': {}",
            project_dir.display(),
            error
        );
        std::process::exit(1);
    });

    if source_files.is_empty() {
        eprintln!("no .zzz files found in '{}'", project_dir.display());
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

    let bitcode_path = Path::new("main.bc");

    module.write_bitcode_to_path(bitcode_path);

    let status = Command::new("clang")
        .args(["main.bc", "-o", "main"])
        .status()
        .expect("failed to execute clang");

    if !status.success() {
        eprintln!("clang failed with status {}", status);
        std::process::exit(1);
    }

    println!("built main");
}
