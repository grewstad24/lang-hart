use std::collections::HashSet;
use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use inkwell::context::Context;

use crate::codegen::Codegen;
use crate::config::ModConfig;
use crate::sema::{Checker, ModuleResolver};
use crate::syntax::{
    Definition, DoIfArm, Expression, ExternalFunction, IfArm, Lexer, LogicalModule, Parameter,
    Parser, Program, Signature, SourceFile, Statement, StructFieldInit, Type,
};

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
                let (extern_functions, definitions, type_aliases) = parser
                    .parse_file_declarations(&path.display().to_string())
                    .map_err(|e| e)?;

                source_files.push(SourceFile {
                    file_path: path.display().to_string(),
                    extern_functions,
                    definitions,
                    type_aliases,
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

fn canonicalize_type(ty: &Type, checker: &Checker) -> Result<Type, String> {
    checker.resolve_type(ty)
}

fn canonicalize_signature(sig: &Signature, checker: &Checker) -> Result<Signature, String> {
    let mut resolved_params = Vec::with_capacity(sig.parameters.len());
    for p in &sig.parameters {
        resolved_params.push(Parameter {
            name: p.name.clone(),
            ty: canonicalize_type(&p.ty, checker)?,
        });
    }

    let resolved_ret = canonicalize_type(sig.return_type.as_ref(), checker)?;
    Ok(Signature {
        parameters: resolved_params,
        return_type: Box::new(resolved_ret),
        is_variadic: sig.is_variadic,
    })
}

fn canonicalize_statement(stmt: &Statement, checker: &Checker) -> Result<Statement, String> {
    match stmt {
        Statement::Compound(stmts) => {
            let mut resolved = Vec::with_capacity(stmts.len());
            for s in stmts {
                resolved.push(canonicalize_statement(s, checker)?);
            }
            Ok(Statement::Compound(resolved))
        }
        Statement::Declare { name, ty } => Ok(Statement::Declare {
            name: name.clone(),
            ty: canonicalize_type(ty, checker)?,
        }),
        Statement::Assign { expression, target } => Ok(Statement::Assign {
            expression: canonicalize_expression(expression, checker)?,
            target: target.clone(),
        }),
        Statement::Discard(expr) => Ok(Statement::Discard(canonicalize_expression(expr, checker)?)),
        Statement::Return(expr) => Ok(Statement::Return(canonicalize_expression(expr, checker)?)),
        Statement::If { arms } => {
            let mut resolved_arms = Vec::with_capacity(arms.len());
            for arm in arms {
                let cond = match &arm.condition {
                    crate::syntax::IfCondition::Expression(e) => {
                        crate::syntax::IfCondition::Expression(canonicalize_expression(e, checker)?)
                    }
                    crate::syntax::IfCondition::Else => crate::syntax::IfCondition::Else,
                };
                let body = Box::new(canonicalize_statement(&arm.body, checker)?);
                resolved_arms.push(DoIfArm {
                    condition: cond,
                    body,
                });
            }
            Ok(Statement::If {
                arms: resolved_arms,
            })
        }
    }
}

fn canonicalize_expression(expr: &Expression, checker: &Checker) -> Result<Expression, String> {
    match expr {
        Expression::Integer(v) => Ok(Expression::Integer(*v)),
        Expression::String(v) => Ok(Expression::String(v.clone())),
        Expression::Boolean(v) => Ok(Expression::Boolean(*v)),
        Expression::Atom(v) => Ok(Expression::Atom(v.clone())),
        Expression::Identifier(name) => Ok(Expression::Identifier(name.clone())),
        Expression::Array(elements) => {
            let mut resolved = Vec::with_capacity(elements.len());
            for el in elements {
                resolved.push(canonicalize_expression(el, checker)?);
            }
            Ok(Expression::Array(resolved))
        }
        Expression::Struct(fields) => {
            let mut resolved = Vec::with_capacity(fields.len());
            for f in fields {
                resolved.push(StructFieldInit {
                    name: f.name.clone(),
                    value: canonicalize_expression(&f.value, checker)?,
                });
            }
            Ok(Expression::Struct(resolved))
        }
        Expression::Signature(sig) => {
            Ok(Expression::Signature(canonicalize_signature(sig, checker)?))
        }
        Expression::Unary { operator, operand } => Ok(Expression::Unary {
            operator: operator.clone(),
            operand: Box::new(canonicalize_expression(operand, checker)?),
        }),
        Expression::Binary {
            left,
            operator,
            right,
        } => Ok(Expression::Binary {
            left: Box::new(canonicalize_expression(left, checker)?),
            operator: operator.clone(),
            right: Box::new(canonicalize_expression(right, checker)?),
        }),
        Expression::Comparison {
            left,
            operator,
            right,
        } => Ok(Expression::Comparison {
            left: Box::new(canonicalize_expression(left, checker)?),
            operator: operator.clone(),
            right: Box::new(canonicalize_expression(right, checker)?),
        }),
        Expression::Cast { expression, ty } => Ok(Expression::Cast {
            expression: Box::new(canonicalize_expression(expression, checker)?),
            ty: canonicalize_type(ty, checker)?,
        }),
        Expression::Call { target, arguments } => {
            let mut resolved_args = Vec::with_capacity(arguments.len());
            for arg in arguments {
                resolved_args.push(canonicalize_expression(arg, checker)?);
            }
            Ok(Expression::Call {
                target: Box::new(canonicalize_expression(target, checker)?),
                arguments: resolved_args,
            })
        }
        Expression::Index { expression, index } => Ok(Expression::Index {
            expression: Box::new(canonicalize_expression(expression, checker)?),
            index: Box::new(canonicalize_expression(index, checker)?),
        }),
        Expression::Member { expression, field } => Ok(Expression::Member {
            expression: Box::new(canonicalize_expression(expression, checker)?),
            field: field.clone(),
        }),
        Expression::AddressOf(e) => Ok(Expression::AddressOf(Box::new(canonicalize_expression(
            e, checker,
        )?))),
        Expression::Decay(e) => Ok(Expression::Decay(Box::new(canonicalize_expression(
            e, checker,
        )?))),
        Expression::Block { signature, body } => {
            let resolved_sig = match signature {
                Some(sig) => Some(canonicalize_signature(sig, checker)?),
                None => None,
            };
            let mut resolved_body = Vec::with_capacity(body.len());
            for stmt in body {
                resolved_body.push(canonicalize_statement(stmt, checker)?);
            }
            Ok(Expression::Block {
                signature: resolved_sig,
                body: resolved_body,
            })
        }
        Expression::If { arms } => {
            let mut resolved_arms = Vec::with_capacity(arms.len());
            for arm in arms {
                let cond = match &arm.condition {
                    crate::syntax::IfCondition::Expression(e) => {
                        crate::syntax::IfCondition::Expression(canonicalize_expression(e, checker)?)
                    }
                    crate::syntax::IfCondition::Else => crate::syntax::IfCondition::Else,
                };
                let body = Box::new(canonicalize_expression(&arm.body, checker)?);
                resolved_arms.push(IfArm {
                    condition: cond,
                    body,
                });
            }
            Ok(Expression::If {
                arms: resolved_arms,
            })
        }
    }
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

    let mut compiled_definitions: Vec<Definition> = Vec::new();
    let mut compiled_externs: Vec<ExternalFunction> = Vec::new();
    let mut seen_extern_names: HashSet<String> = HashSet::new();
    let mut seen_def_names: HashSet<String> = HashSet::new();
    let mut has_main = false;

    for (module, scope) in modules.iter().zip(module_scopes.iter()) {
        let checker = Checker::new(scope);

        for file in &module.source_files {
            for def in &file.definitions {
                let resolved_value = canonicalize_expression(&def.value, &checker)?;

                if def.name == "main" {
                    has_main = true;
                    if !seen_def_names.insert("main".to_string()) {
                        continue;
                    }
                    compiled_definitions.push(Definition {
                        name: "main".to_string(),
                        value: resolved_value,
                    });
                    continue;
                }

                if !seen_def_names.insert(def.name.clone()) {
                    continue;
                }

                compiled_definitions.push(Definition {
                    name: def.name.clone(),
                    value: resolved_value,
                });
            }

            for ext in &file.extern_functions {
                if !seen_extern_names.insert(ext.name.clone()) {
                    continue;
                }

                let resolved_sig = canonicalize_signature(&ext.signature, &checker)?;

                compiled_externs.push(ExternalFunction {
                    name: ext.name.clone(),
                    signature: resolved_sig,
                });
            }
        }
    }

    if !has_main {
        return Err("program error: missing entry point '@main'".to_string());
    }

    let program = Program {
        variables: Vec::new(),
        extern_functions: compiled_externs,
        definitions: compiled_definitions,
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
