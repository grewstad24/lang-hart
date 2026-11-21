use std::env;
use std::path::PathBuf;

use hart::driver::compile_project;
use hart::lsp::run_lsp_server;

fn main() {
    let mut args = env::args().skip(1);
    let subcommand = match args.next() {
        Some(cmd) => cmd,
        None => {
            eprintln!("usage: hart <build|lsp> [args]");
            std::process::exit(1);
        }
    };

    match subcommand.as_str() {
        "build" => {
            let project_dir = match args.next() {
                Some(path) => PathBuf::from(path),
                None => {
                    eprintln!("usage: hart build <project-directory>");
                    std::process::exit(1);
                }
            };

            if !project_dir.is_dir() {
                eprintln!("'{}' is not a directory", project_dir.display());
                std::process::exit(1);
            }

            if let Err(error) = compile_project(&project_dir) {
                eprintln!("{}", error);
                std::process::exit(1);
            }
        }
        "lsp" => {
            if let Err(error) = run_lsp_server() {
                eprintln!("lsp error: {}", error);
                std::process::exit(1);
            }
        }
        unknown => {
            eprintln!("error: unknown subcommand '{}'. Expected 'build' or 'lsp'", unknown);
            std::process::exit(1);
        }
    }
}
