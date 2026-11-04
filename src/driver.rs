use std::collections::{HashMap, HashSet};
use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use inkwell::OptimizationLevel;
use inkwell::context::Context;
use inkwell::targets::{CodeModel, InitializationConfig, RelocMode, Target, TargetTriple};

use crate::codegen::Codegen;
use crate::config::ModConfig;
use crate::sema::{Checker, ModuleResolver};
use crate::syntax::{
    Definition, Expression, ExternalFunction, IfArm, Lexer, LogicalModule, MatchArm, Parameter,
    Parser, Program, Signature, SourceFile, StructFieldInit, Type, TypeAlias, format_error,
};

pub const DEFAULT_TARGET_TRIPLE: &str = "x86_64-unknown-linux-musl";

pub fn get_snore_home_dir() -> Option<PathBuf> {
    if let Some(snore_home) = env::var_os("SNORE_HOME") {
        return Some(PathBuf::from(snore_home));
    }
    let default_home = env::var_os("HOME")
        .or_else(|| env::var_os("USERPROFILE"))
        .map(|home| PathBuf::from(home).join(".snore"));
    if let Some(ref path) = default_home {
        if path.is_dir() {
            return default_home;
        }
    }
    let tmp_snore = PathBuf::from("/tmp/.snore");
    if tmp_snore.is_dir() {
        return Some(tmp_snore);
    }
    default_home
}

fn get_musl_lib_dir() -> Result<PathBuf, String> {
    let snore_home = get_snore_home_dir()
        .ok_or_else(|| "could not resolve snore home directory (~/.snore)".to_string())?;
    let musl_lib = snore_home.join("musl").join("lib");

    let required_artifacts = ["libc.a", "crt1.o", "crti.o", "crtn.o"];
    for artifact in &required_artifacts {
        let path = musl_lib.join(artifact);
        if !path.exists() {
            return Err(format!(
                "missing required musl artifact '{}' at '{}'. Build and install musl into '~/.snore/musl' first.",
                artifact,
                path.display()
            ));
        }
    }

    Ok(musl_lib)
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

                let (tokens, spans) =
                    Lexer::new_with_file(code, path.display().to_string()).tokenize_with_spans()?;

                let mut parser = Parser::new_with_spans(tokens, spans);
                let parsed = parser.parse_file_declarations(&path.display().to_string());
                let (extern_functions, definitions, type_aliases) =
                    parsed.map_err(|error| format_error(parser.current_span(), &error))?;

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

fn canonicalize_expression(expr: &Expression, checker: &Checker) -> Result<Expression, String> {
    match expr {
        Expression::Spanned { expression, span } => Ok(Expression::Spanned {
            expression: Box::new(
                canonicalize_expression(expression, checker)
                    .map_err(|error| format_error(span, &error))?,
            ),
            span: span.clone(),
        }),
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
        Expression::Record(fields) => {
            let mut resolved = Vec::with_capacity(fields.len());
            for field in fields {
                resolved.push(StructFieldInit {
                    name: field.name.clone(),
                    value: canonicalize_expression(&field.value, checker)?,
                });
            }
            Ok(Expression::Record(resolved))
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
        Expression::Dereference(e) => Ok(Expression::Dereference(Box::new(
            canonicalize_expression(e, checker)?,
        ))),
        Expression::Block(exprs) => {
            let mut resolved = Vec::with_capacity(exprs.len());
            for e in exprs {
                resolved.push(canonicalize_expression(e, checker)?);
            }
            Ok(Expression::Block(resolved))
        }
        Expression::Discard(e) => Ok(Expression::Discard(Box::new(canonicalize_expression(e, checker)?))),
        Expression::Return(opt_e) => {
            let resolved = match opt_e {
                Some(e) => Some(Box::new(canonicalize_expression(e, checker)?)),
                None => None,
            };
            Ok(Expression::Return(resolved))
        }
        Expression::Let { name, value } => Ok(Expression::Let {
            name: name.clone(),
            value: Box::new(canonicalize_expression(value, checker)?),
        }),
        Expression::Var { name, ty, value } => {
            let resolved_ty = match ty {
                Some(t) => Some(canonicalize_type(t, checker)?),
                None => None,
            };
            let resolved_val = match value {
                Some(v) => Some(Box::new(canonicalize_expression(v, checker)?)),
                None => None,
            };
            Ok(Expression::Var {
                name: name.clone(),
                ty: resolved_ty,
                value: resolved_val,
            })
        }
        Expression::Assign { target, value } => Ok(Expression::Assign {
            target: target.clone(),
            value: Box::new(canonicalize_expression(value, checker)?),
        }),
        Expression::DoTimes { count, body } => Ok(Expression::DoTimes {
            count: Box::new(canonicalize_expression(count, checker)?),
            body: Box::new(canonicalize_expression(body, checker)?),
        }),
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
        Expression::Match {
            target,
            binding_name,
            arms,
        } => {
            let resolved_target = canonicalize_expression(target, checker)?;
            let mut resolved_arms = Vec::with_capacity(arms.len());
            for arm in arms {
                let variant_type = match &arm.variant_type {
                    Some(ty) => Some(canonicalize_type(ty, checker)?),
                    None => None,
                };
                resolved_arms.push(MatchArm {
                    variant_type,
                    body: Box::new(canonicalize_expression(&arm.body, checker)?),
                });
            }
            Ok(Expression::Match {
                target: Box::new(resolved_target),
                binding_name: binding_name.clone(),
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

    let mut modules = discover_logical_modules(&search_dirs)?;

    if modules.is_empty() {
        return Err("no .zzz modules found to compile".to_string());
    }

    let resolver = ModuleResolver::new();
    let module_scopes = resolver.resolve_modules(&modules)?;

    for (module, scope) in modules.iter_mut().zip(module_scopes.iter()) {
        let mut checker = Checker::new(scope);
        for file in &mut module.source_files {
            checker.check_file(file)?;
        }
    }

    let mut compiled_definitions: Vec<Definition> = Vec::new();
    let mut compiled_externs: Vec<ExternalFunction> = Vec::new();
    let mut compiled_aliases: HashMap<String, TypeAlias> = HashMap::new();
    let mut seen_extern_names: HashSet<String> = HashSet::new();
    let mut seen_def_names: HashSet<String> = HashSet::new();
    let mut has_main = false;

    for (module, scope) in modules.iter().zip(module_scopes.iter()) {
        let checker = Checker::new(scope);

        for file in &module.source_files {
            for alias in &file.type_aliases {
                compiled_aliases
                    .entry(alias.name.clone())
                    .or_insert_with(|| alias.clone());
            }
            for def in &file.definitions {
                let resolved_sig = def
                    .signature
                    .as_ref()
                    .map(|s| canonicalize_signature(s, &checker))
                    .transpose()?;
                let resolved_value = canonicalize_expression(&def.value, &checker)?;

                if def.name == "main" {
                    has_main = true;
                    if !seen_def_names.insert("main".to_string()) {
                        continue;
                    }
                    compiled_definitions.push(Definition {
                        name: "main".to_string(),
                        signature: resolved_sig,
                        value: resolved_value,
                    });
                    continue;
                }

                if !seen_def_names.insert(def.name.clone()) {
                    continue;
                }

                compiled_definitions.push(Definition {
                    name: def.name.clone(),
                    signature: resolved_sig,
                    value: resolved_value,
                });
            }

            for ext in &file.extern_functions {
                if !seen_extern_names.insert(ext.name.clone()) {
                    continue;
                }

                let resolved_sig = canonicalize_signature(&ext.signature, &checker)
                    .map_err(|error| format_error(&ext.span, &error))?;

                compiled_externs.push(ExternalFunction {
                    name: ext.name.clone(),
                    signature: resolved_sig,
                    span: ext.span.clone(),
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
        type_aliases: compiled_aliases.into_values().collect(),
    };

    Target::initialize_all(&InitializationConfig::default());
    let target_triple = TargetTriple::create(DEFAULT_TARGET_TRIPLE);
    let target = Target::from_triple(&target_triple).map_err(|error| {
        format!(
            "failed to initialize target '{}': {}",
            DEFAULT_TARGET_TRIPLE, error
        )
    })?;
    let target_machine = target
        .create_target_machine(
            &target_triple,
            "generic",
            "",
            OptimizationLevel::Default,
            RelocMode::Static,
            CodeModel::Default,
        )
        .ok_or_else(|| {
            format!(
                "failed to create target machine for '{}'",
                DEFAULT_TARGET_TRIPLE
            )
        })?;

    let context = Context::create();
    let codegen = Codegen::new(&context, "main", &target_machine);

    let module = codegen.generate(&program)?;

    let main_span = program
        .definitions
        .iter()
        .find(|definition| definition.name == "main")
        .and_then(|definition| match &definition.value {
            Expression::Spanned { span, .. } => Some(span),
            _ => None,
        });

    if let Err(err) = module.verify() {
        let message = format!("LLVM verification failed: {}", err);
        return Err(main_span.map_or(message.clone(), |span| format_error(span, &message)));
    }

    let ir_path = Path::new("main.ll");
    if let Err(err) = module.print_to_file(ir_path) {
        return Err(format!("failed to write LLVM IR: {}", err));
    }

    let musl_lib = get_musl_lib_dir()?;
    let musl_lib_str = musl_lib
        .to_str()
        .ok_or_else(|| "invalid UTF-8 in musl library path".to_string())?;
    let target_argument = format!("--target={}", DEFAULT_TARGET_TRIPLE);

    let status = Command::new("clang")
        .args([
            target_argument.as_str(),
            "-O3",
            "-static",
            "-B",
            musl_lib_str,
            "-L",
            musl_lib_str,
            "main.ll",
            "-o",
            "main",
        ])
        .status()
        .map_err(|error| {
            let message = format!("failed to execute clang: {}", error);
            main_span.map_or(message.clone(), |span| format_error(span, &message))
        })?;

    if !status.success() {
        let message = format!("clang failed with status {}", status);
        return Err(main_span.map_or(message.clone(), |span| format_error(span, &message)));
    }

    println!("built main");
    Ok(())
}
