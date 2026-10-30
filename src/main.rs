use std::env;
use std::path::PathBuf;

use snore::driver::compile_project;

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

    if let Err(error) = compile_project(&project_dir) {
        eprintln!("{}", error);
        std::process::exit(1);
    }
}
