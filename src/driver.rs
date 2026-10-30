use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use inkwell::context::Context;

use crate::codegen::Codegen;
use crate::config::ModConfig;
use crate::sema::{Checker, ModuleResolver};
use crate::syntax::{Lexer, LogicalModule, Parser, Program, SourceFile};

pub fn get_snore_home_dir() -> Option<PathBuf> {
    env::var_os("HOME")
        .or_else(|| env::var_os("USERPROFILE"))
        .map(|home| PathBuf::from(home).join(".snore"))
}

pub fn discover_logical_modules(dirs: &[PathBuf]) -> Result<Vec<LogicalModule>, String> {
    let mut modules = Vec::new();
    let mut visited_dirs = Vec::new();

    fn walk_dir(
        dir: &Path,
        modules: &mut Vec<LogicalModule>,
        visited: &mut Vec<PathBuf>,
    ) -> Result<(), String> {
        let canonical = dir.canonicalize().map_err(|e| e.to_string())?;
        if visited.contains(&canonical) {
            return Ok(());
        }
        visited.push(canonical);

        let mut zzz_file_paths = Vec::new();
        let mut subdirs = Vec::new();
        let mut has_mod_yawn = false;

        let entries =
            fs::read_dir(dir).map_err(|e| format!("cannot read '{}': {}", dir.display(), e))?;

        for entry in entries {
            let entry = entry.map_err(|e| e.to_string())?;
            let path = entry.path();

            if path.is_dir() {
                subdirs.push(path);
            } else if path.file_name().and_then(|n| n.to_str()) == Some("mod.yawn") {
                has_mod_yawn = true;
            } else if path.extension().and_then(|e| e.to_str()) == Some("zzz") {
                zzz_file_paths.push(path);
            }
        }

        if !zzz_file_paths.is_empty() {
            if !has_mod_yawn {
                return Err(format!(
                    "missing required 'mod.yawn' in directory '{}'",
                    dir.display()
                ));
            }

            let mod_yawn_path = dir.join("mod.yawn");
            let yawn_source = fs::read_to_string(&mod_yawn_path)
                .map_err(|e| format!("failed to read '{}': {}", mod_yawn_path.display(), e))?;

            let mod_config =
                ModConfig::parse_and_validate(&yawn_source, &mod_yawn_path.display().to_string())?;

            let mut source_files = Vec::new();
            zzz_file_paths.sort();

            for path in zzz_file_paths {
                let code = fs::read_to_string(&path)
                    .map_err(|e| format!("failed to read '{}': {}", path.display(), e))?;

                let tokens = Lexer::new(code)
                    .tokenize()
                    .map_err(|e| format!("lexer error in '{}': {}", path.display(), e))?;

                let mut parser = Parser::new(tokens);
                let (extern_functions, definitions) = parser
                    .parse_file_declarations(&path.display().to_string())
                    .map_err(|e| e)?;

                source_files.push(SourceFile {
                    file_path: path.display().to_string(),
                    extern_functions,
                    definitions,
                });
            }

            modules.push(LogicalModule {
                dir_path: dir.display().to_string(),
                source_files,
                exports: mod_config.exports,
                imports: mod_config.imports,
            });
        }

        for subdir in subdirs {
            walk_dir(&subdir, modules, visited)?;
        }

        Ok(())
    }

    for dir in dirs {
        if dir.is_dir() {
            walk_dir(dir, &mut modules, &mut visited_dirs)?;
        }
    }

    Ok(modules)
}

pub fn compile_project(project_dir: &Path) -> Result<(), String> {
    let mut search_dirs = Vec::new();
    if let Some(snore_home) = get_snore_home_dir() {
        if snore_home.is_dir() {
            search_dirs.push(snore_home);
        }
    }
    search_dirs.push(project_dir.to_path_buf());

    let modules = discover_logical_modules(&search_dirs)
        .map_err(|error| format!("module discovery error: {}", error))?;

    if modules.is_empty() {
        return Err("no .zzz modules found to compile".to_string());
    }

    let resolver = ModuleResolver::new();
    let module_scopes = resolver
        .resolve_modules(&modules)
        .map_err(|error| format!("namespace resolution error: {}", error))?;

    for (module, scope) in modules.iter().zip(module_scopes.iter()) {
        let mut checker = Checker::new(scope);
        for file in &module.source_files {
            checker
                .check_file(file)
                .map_err(|error| format!("type error: {}", error))?;
        }
    }

    let mut all_externs = Vec::new();
    let mut all_definitions = Vec::new();
    let mut has_main = false;

    for module in &modules {
        for file in &module.source_files {
            all_externs.extend(file.extern_functions.clone());
            for def in &file.definitions {
                if def.name == "main" {
                    has_main = true;
                }
                all_definitions.push(def.clone());
            }
        }
    }

    if !has_main {
        return Err("program error: missing entry point '@main'".to_string());
    }

    let program = Program {
        variables: Vec::new(),
        extern_functions: all_externs,
        definitions: all_definitions,
    };

    let context = Context::create();
    let codegen = Codegen::new(&context, "main");

    let module = codegen
        .generate(&program)
        .map_err(|error| format!("compiler error: {}", error))?;

    if let Err(err) = module.verify() {
        return Err(format!("llvm verification error:\n{}", err));
    }

    let ir_path = Path::new("main.ll");
    if let Err(err) = module.print_to_file(ir_path) {
        return Err(format!("failed to write LLVM IR: {}", err));
    }

    let status = Command::new("clang")
        .args(["-O3", "main.ll", "-o", "main"])
        .status()
        .map_err(|e| format!("failed to execute clang: {}", e))?;

    if !status.success() {
        return Err(format!("clang failed with status {}", status));
    }

    println!("built main");
    Ok(())
}
