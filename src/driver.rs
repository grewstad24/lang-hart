use std::collections::{HashMap, HashSet};
use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Instant;

use inkwell::OptimizationLevel;
use inkwell::context::Context;
use inkwell::targets::{CodeModel, FileType, InitializationConfig, RelocMode, Target, TargetTriple};

use crate::codegen::Codegen;
use crate::sema::{Checker, ModuleResolver};
use crate::syntax::{
    Definition, ExportRule, Expression, ExternalFunction, IfArm, Lexer, LogicalModule, MatchArm,
    ModuleDecl, NamespacePath, Parameter, Parser, Program, Signature, SourceFile, StructFieldInit,
    Type, TypeAlias, format_error,
};

pub const X86_64_TARGET_TRIPLE: &str = "x86_64-unknown-linux-musl";
pub const AARCH64_TARGET_TRIPLE: &str = "aarch64-unknown-linux-musl";

fn format_duration(d: std::time::Duration) -> String {
    let nanos = d.as_nanos();
    if nanos < 1_000 {
        format!("{}ns", nanos)
    } else if nanos < 1_000_000 {
        format!("{:.2}µs", d.as_secs_f64() * 1_000_000.0)
    } else if nanos < 1_000_000_000 {
        format!("{:.2}ms", d.as_secs_f64() * 1_000.0)
    } else {
        format!("{:.2}s", d.as_secs_f64())
    }
}

fn run_tool_command(
    cmd_path: &Path,
    args: &[&str],
    tmp_dir: &str,
) -> std::io::Result<std::process::ExitStatus> {
    match Command::new(cmd_path)
        .env("TMPDIR", tmp_dir)
        .args(args)
        .status()
    {
        Err(err) if err.raw_os_error() == Some(8) => {
            // Exec format error: try executing via `sh` (common for APE binaries on Linux when binfmt_misc is not active)
            Command::new("sh")
                .arg(cmd_path)
                .env("TMPDIR", tmp_dir)
                .args(args)
                .status()
        }
        res => res,
    }
}

pub fn get_hart_home_dir() -> Option<PathBuf> {
    if let Some(hart_home) = env::var_os("HART_HOME") {
        return Some(PathBuf::from(hart_home));
    }
    env::var_os("HOME")
        .or_else(|| env::var_os("USERPROFILE"))
        .map(|home| PathBuf::from(home).join(".hart"))
}

fn get_cosmocc_dir() -> Result<PathBuf, String> {
    if let Some(path) = env::var_os("HART_COSMOCC") {
        let p = PathBuf::from(path);
        if p.join("bin").join("apelink").exists() {
            return Ok(p);
        }
    }
    if let Some(hart_home) = get_hart_home_dir() {
        let p = hart_home.join("cosmocc");
        if p.join("bin").join("apelink").exists() {
            return Ok(p);
        }
    }
    let local_cosmo = env::var_os("HOME")
        .map(|h| PathBuf::from(h).join(".local").join("opt").join("cosmocc"));
    if let Some(p) = local_cosmo {
        if p.join("bin").join("apelink").exists() {
            return Ok(p);
        }
    }
    let vendor_cosmo = PathBuf::from("vendor").join("cosmocc");
    if vendor_cosmo.join("bin").join("apelink").exists() {
        return Ok(vendor_cosmo);
    }
    Err("could not find Cosmopolitan toolchain ('cosmocc'). Ensure it is installed in '~/.hart/cosmocc' or '~/.local/opt/cosmocc'.".to_string())
}

pub fn discover_logical_modules(dirs: &[PathBuf]) -> Result<Vec<LogicalModule>, String> {
    let mut hart_files = Vec::new();
    let mut visited_dirs = Vec::new();

    fn collect_hart_files(
        dir: &Path,
        files: &mut Vec<PathBuf>,
        visited: &mut Vec<PathBuf>,
    ) -> Result<(), String> {
        let canonical = dir.canonicalize().map_err(|e| e.to_string())?;
        if visited.contains(&canonical) {
            return Ok(());
        }
        visited.push(canonical);

        let entries =
            fs::read_dir(dir).map_err(|e| format!("cannot read '{}': {}", dir.display(), e))?;

        let mut subdirs = Vec::new();
        for entry in entries {
            let entry = entry.map_err(|e| e.to_string())?;
            let path = entry.path();
            if path.is_dir() {
                subdirs.push(path);
            } else if path.extension().and_then(|e| e.to_str()) == Some("hart") {
                files.push(path);
            }
        }

        subdirs.sort();
        for subdir in subdirs {
            collect_hart_files(&subdir, files, visited)?;
        }

        Ok(())
    }

    for dir in dirs {
        if dir.is_dir() {
            collect_hart_files(dir, &mut hart_files, &mut visited_dirs)?;
        }
    }

    hart_files.sort();

    // Map named module -> (Option<Declaration (exports, imports, decl_path)>, Vec<SourceFile>)
    let mut named_modules: HashMap<
        String,
        (
            Option<(Vec<ExportRule>, Vec<NamespacePath>, PathBuf)>,
            Vec<SourceFile>,
        ),
    > = HashMap::new();
    let mut unnamed_modules: Vec<LogicalModule> = Vec::new();

    for path in hart_files {
        let code = fs::read_to_string(&path)
            .map_err(|e| format!("failed to read '{}': {}", path.display(), e))?;

        let (tokens, spans) =
            Lexer::new_with_file(code, path.display().to_string()).tokenize_with_spans()?;

        let mut parser = Parser::new_with_spans(tokens, spans);
        let parsed_file = parser
            .parse_file_declarations(&path.display().to_string())
            .map_err(|error| format_error(parser.current_span(), &error))?;

        match parsed_file.module_decl {
            Some(ModuleDecl::Declaration {
                name,
                exports,
                imports,
                span: _,
            }) => {
                let entry = named_modules
                    .entry(name.clone())
                    .or_insert_with(|| (None, Vec::new()));
                if entry.0.is_some() {
                    return Err(format!(
                        "{}: error: duplicate declaration of module '{}'",
                        path.display(),
                        name
                    ));
                }
                entry.0 = Some((exports, imports, path.clone()));
                entry.1.push(parsed_file.source_file);
            }
            Some(ModuleDecl::Member { name, span: _ }) => {
                let entry = named_modules
                    .entry(name.clone())
                    .or_insert_with(|| (None, Vec::new()));
                entry.1.push(parsed_file.source_file);
            }
            None => {
                let mut imports = parsed_file.imports;
                if !imports.contains(&NamespacePath::root()) {
                    imports.insert(0, NamespacePath::root());
                }
                unnamed_modules.push(LogicalModule {
                    name: None,
                    dir_path: path.parent().unwrap_or(Path::new("")).display().to_string(),
                    source_files: vec![parsed_file.source_file],
                    exports: parsed_file.exports,
                    imports,
                });
            }
        }
    }

    let mut result = unnamed_modules;

    let mut named_keys: Vec<String> = named_modules.keys().cloned().collect();
    named_keys.sort();

    for name in named_keys {
        let (decl, files) = named_modules.remove(&name).unwrap();
        match decl {
            Some((exports, mut imports, decl_path)) => {
                if !imports.contains(&NamespacePath::root()) {
                    imports.insert(0, NamespacePath::root());
                }
                result.push(LogicalModule {
                    name: Some(name),
                    dir_path: decl_path
                        .parent()
                        .unwrap_or(Path::new(""))
                        .display()
                        .to_string(),
                    source_files: files,
                    exports,
                    imports,
                });
            }
            None => {
                return Err(format!(
                    "module '{}' is referenced with 'mod {}' but never declared with 'decl mod {}'",
                    name, name, name
                ));
            }
        }
    }

    Ok(result)
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
        Expression::Symbol(v) => Ok(Expression::Symbol(v.clone())),
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
        Expression::Tuple(elements) => {
            let mut resolved = Vec::with_capacity(elements.len());
            for el in elements {
                resolved.push(canonicalize_expression(el, checker)?);
            }
            Ok(Expression::Tuple(resolved))
        }
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
            ty: ty.clone(),
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
        Expression::AddressOf { expression, is_mut } => Ok(Expression::AddressOf {
            expression: Box::new(canonicalize_expression(expression, checker)?),
            is_mut: *is_mut,
        }),
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
        Expression::Discard(e) => Ok(Expression::Discard(Box::new(canonicalize_expression(
            e, checker,
        )?))),
        Expression::Return(e) => Ok(Expression::Return(Box::new(canonicalize_expression(
            e, checker,
        )?))),
        Expression::Yield(e) => Ok(Expression::Yield(Box::new(canonicalize_expression(
            e, checker,
        )?))),
        Expression::Break => Ok(Expression::Break),
        Expression::Continue => Ok(Expression::Continue),
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
            target: Box::new(canonicalize_expression(target, checker)?),
            value: Box::new(canonicalize_expression(value, checker)?),
        }),
        Expression::Repeat {
            count,
            binder,
            body,
        } => Ok(Expression::Repeat {
            count: Box::new(canonicalize_expression(count, checker)?),
            binder: binder.clone(),
            body: Box::new(canonicalize_expression(body, checker)?),
        }),
        Expression::RepeatWhile { condition, body } => Ok(Expression::RepeatWhile {
            condition: Box::new(canonicalize_expression(condition, checker)?),
            body: Box::new(canonicalize_expression(body, checker)?),
        }),
        Expression::RepeatUntil { condition, body } => Ok(Expression::RepeatUntil {
            condition: Box::new(canonicalize_expression(condition, checker)?),
            body: Box::new(canonicalize_expression(body, checker)?),
        }),
        Expression::For {
            iterable,
            binder,
            body,
        } => Ok(Expression::For {
            iterable: Box::new(canonicalize_expression(iterable, checker)?),
            binder: binder.clone(),
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
        Expression::SizeOf(expr) => Ok(Expression::SizeOf(Box::new(canonicalize_expression(expr, checker)?))),
        Expression::Slice(expr) => Ok(Expression::Slice(Box::new(canonicalize_expression(expr, checker)?))),
    }
}

pub fn compile_project(project_dir: &Path) -> Result<(), String> {
    let total_start = Instant::now();

    let mut search_dirs = Vec::new();
    if let Some(hart_home) = get_hart_home_dir() {
        if hart_home.is_dir() {
            search_dirs.push(hart_home);
        }
    }
    let local_std = PathBuf::from("std");
    if local_std.is_dir() && !search_dirs.iter().any(|d| d.join("std").is_dir() || d == &local_std) {
        search_dirs.push(local_std);
    }
    search_dirs.push(project_dir.to_path_buf());

    let step_start = Instant::now();
    let mut modules = discover_logical_modules(&search_dirs)?;

    if modules.is_empty() {
        return Err("no .hart modules found to compile".to_string());
    }
    println!("  [1/6] Parse & discover modules   ({})", format_duration(step_start.elapsed()));

    let step_start = Instant::now();
    let resolver = ModuleResolver::new();
    let mut module_scopes = resolver.resolve_modules(&modules)?;
    println!("  [2/6] Resolve module scopes      ({})", format_duration(step_start.elapsed()));

    let step_start = Instant::now();
    crate::monomorphize::specialise_def_functions(&mut modules, &mut module_scopes)?;
    crate::monomorphize::monomorphize_modules(&mut modules, &mut module_scopes)?;
    crate::monomorphize::concretize_meta_types(&mut modules, &mut module_scopes)?;

    for _ in 0..3 {
        let mut shared_canonical_targets: HashMap<String, Type> = HashMap::new();
        for (module, scope) in modules.iter().zip(module_scopes.iter()) {
            let checker = Checker::new(scope);
            for (name, prov) in &scope.type_aliases {
                if !prov.item.is_meta {
                    if let Ok(target) = canonicalize_type(&prov.item.target, &checker) {
                        shared_canonical_targets.insert(name.clone(), target);
                    }
                }
            }
            for (name, prov) in &scope.home_type_aliases {
                if !prov.item.is_meta {
                    if let Ok(target) = canonicalize_type(&prov.item.target, &checker) {
                        shared_canonical_targets.insert(name.clone(), target);
                    }
                }
            }
            for file in &module.source_files {
                for alias in &file.type_aliases {
                    if !alias.is_meta {
                        if let Ok(target) = canonicalize_type(&alias.target, &checker) {
                            shared_canonical_targets.insert(alias.name.clone(), target);
                        }
                    }
                }
            }
        }

        for (module, scope) in modules.iter_mut().zip(module_scopes.iter_mut()) {
            for (name, target) in &shared_canonical_targets {
                if let Some(prov) = scope.type_aliases.get_mut(name) {
                    prov.item.target = target.clone();
                }
                if let Some(prov) = scope.home_type_aliases.get_mut(name) {
                    prov.item.target = target.clone();
                }
            }
            for file in &mut module.source_files {
                for alias in &mut file.type_aliases {
                    if let Some(target) = shared_canonical_targets.get(&alias.name) {
                        alias.target = target.clone();
                    }
                }
            }
        }
    }
    println!("  [3/6] Specialise & monomorphize  ({})", format_duration(step_start.elapsed()));

    let step_start = Instant::now();
    for (module, scope) in modules.iter_mut().zip(module_scopes.iter()) {
        let mut checker = Checker::new(scope);
        for file in &mut module.source_files {
            checker.check_file(file)?;
        }
    }
    println!("  [4/6] Type check                 ({})", format_duration(step_start.elapsed()));

    let step_start = Instant::now();
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
                if alias.is_meta {
                    continue;
                }
                let mut resolved_alias = alias.clone();
                if let Ok(target) = canonicalize_type(&alias.target, &checker) {
                    resolved_alias.target = target;
                }
                compiled_aliases
                    .entry(alias.name.clone())
                    .or_insert(resolved_alias);
            }
            for def in &file.definitions {
                if def.is_def {
                    continue;
                }
                let mut def_sig = def.signature.clone();
                if let Some(sig) = &mut def_sig {
                    if *sig.return_type == Type::Inferred {
                        if let Some(Type::Function { return_type, .. }) = checker.resolved_definitions.get(&def.name) {
                            *sig.return_type = *return_type.clone();
                        }
                    }
                }
                let resolved_sig = def_sig
                    .as_ref()
                    .map(|s| canonicalize_signature(s, &checker))
                    .transpose()?;
                let resolved_value = canonicalize_expression(&def.value, &checker)?;

                if def.name == "main" {
                    if def.is_def {
                        return Err(format!(
                            "{}: error: entry point 'main' must be a function ('func'), never a 'def'",
                            file.file_path
                        ));
                    }
                    if let Some(sig) = &resolved_sig {
                        if sig.parameters.len() > 1 {
                            return Err(format!(
                                "{}: error: entry point 'main' accepts either no arguments or a single argument typed 'Slice<String>'",
                                file.file_path
                            ));
                        }
                        if let Some(param) = sig.parameters.first() {
                            let param_str = format!("{}", param.ty);
                            if !param_str.contains("Slice") && !param_str.contains("String") {
                                return Err(format!(
                                    "{}: error: entry point 'main' parameter must be typed 'Slice<String>', found {}",
                                    file.file_path, param.ty
                                ));
                            }
                        }
                        let ret = &sig.return_type;
                        if **ret != Type::S32
                            && !matches!(**ret, Type::Symbol(ref name) if name == "none")
                        {
                            return Err(format!(
                                "{}: error: entry point 'main' must return 's32' or ':none', found {}",
                                file.file_path, ret
                            ));
                        }
                    }
                    has_main = true;
                    if !seen_def_names.insert("main".to_string()) {
                        continue;
                    }
                    compiled_definitions.push(Definition {
                        name: "main".to_string(),
                        signature: resolved_sig,
                        value: resolved_value,
                        is_def: false,
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
                    is_def: false,
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
        return Err("error: missing entry point 'main'".to_string());
    }

    let program = Program {
        variables: Vec::new(),
        extern_functions: compiled_externs,
        definitions: compiled_definitions,
        type_aliases: compiled_aliases.into_values().collect(),
    };

    Target::initialize_all(&InitializationConfig::default());

    // 1. x86_64 target machine
    let x86_triple = TargetTriple::create(X86_64_TARGET_TRIPLE);
    let x86_target = Target::from_triple(&x86_triple).map_err(|error| {
        format!(
            "failed to initialize target '{}': {}",
            X86_64_TARGET_TRIPLE, error
        )
    })?;
    let x86_target_machine = x86_target
        .create_target_machine(
            &x86_triple,
            "generic",
            "",
            OptimizationLevel::Default,
            RelocMode::Static,
            CodeModel::Default,
        )
        .ok_or_else(|| {
            format!(
                "failed to create target machine for '{}'",
                X86_64_TARGET_TRIPLE
            )
        })?;

    // 2. aarch64 target machine
    let aarch64_triple = TargetTriple::create(AARCH64_TARGET_TRIPLE);
    let aarch64_target = Target::from_triple(&aarch64_triple).map_err(|error| {
        format!(
            "failed to initialize target '{}': {}",
            AARCH64_TARGET_TRIPLE, error
        )
    })?;
    let aarch64_target_machine = aarch64_target
        .create_target_machine(
            &aarch64_triple,
            "generic",
            "",
            OptimizationLevel::Default,
            RelocMode::Static,
            CodeModel::Default,
        )
        .ok_or_else(|| {
            format!(
                "failed to create target machine for '{}'",
                AARCH64_TARGET_TRIPLE
            )
        })?;

    let main_span = program
        .definitions
        .iter()
        .find(|definition| definition.name == "main")
        .and_then(|definition| match &definition.value {
            Expression::Spanned { span, .. } => Some(span),
            _ => None,
        });

    // Generate x86_64 module & object file
    let context_x86 = Context::create();
    let codegen_x86 = Codegen::new(&context_x86, "main_x86", &x86_target_machine);
    let module_x86 = codegen_x86.generate(&program)?;
    if let Err(err) = module_x86.verify() {
        let message = format!("LLVM verification failed (x86_64): {}", err);
        return Err(main_span.map_or(message.clone(), |span| format_error(span, &message)));
    }
    let obj_path_x86 = Path::new("main.x86_64.o");
    x86_target_machine
        .write_to_file(&module_x86, FileType::Object, obj_path_x86)
        .map_err(|err| format!("failed to write x86_64 object file: {}", err))?;

    // Generate aarch64 module & object file
    let context_aarch64 = Context::create();
    let codegen_aarch64 = Codegen::new(&context_aarch64, "main_aarch64", &aarch64_target_machine);
    let module_aarch64 = codegen_aarch64.generate(&program)?;
    if let Err(err) = module_aarch64.verify() {
        let message = format!("LLVM verification failed (aarch64): {}", err);
        return Err(main_span.map_or(message.clone(), |span| format_error(span, &message)));
    }
    let obj_path_aarch64 = Path::new("main.aarch64.o");
    aarch64_target_machine
        .write_to_file(&module_aarch64, FileType::Object, obj_path_aarch64)
        .map_err(|err| format!("failed to write aarch64 object file: {}", err))?;

    let ir_path = Path::new("main.ll");
    let _ = module_x86.print_to_file(ir_path);
    println!("  [5/6] Code generation            ({})", format_duration(step_start.elapsed()));

    let step_start = Instant::now();
    let cosmo_dir = get_cosmocc_dir()?;
    let cosmo_bin = cosmo_dir.join("bin");

    let x86_cc = cosmo_bin.join("x86_64-unknown-cosmo-cc");
    let aarch64_cc = cosmo_bin.join("aarch64-unknown-cosmo-cc");
    let apelink = cosmo_bin.join("apelink");
    let ape_x86 = cosmo_bin.join("ape-x86_64.elf");
    let ape_aarch64 = cosmo_bin.join("ape-aarch64.elf");
    let ape_m1 = cosmo_bin.join("ape-m1.c");

    let elf_path_x86 = Path::new("main.x86_64.elf");
    let elf_path_aarch64 = Path::new("main.aarch64.elf");
    let tmp_dir = env::var("TMPDIR").unwrap_or_else(|_| "/tmp".to_string());

    let status_x86 = run_tool_command(
        &x86_cc,
        &[
            "-static",
            "-O3",
            obj_path_x86.to_str().unwrap(),
            "-o",
            elf_path_x86.to_str().unwrap(),
        ],
        &tmp_dir,
    )
    .map_err(|error| {
        let message = format!("failed to execute x86_64-unknown-cosmo-cc: {}", error);
        main_span.map_or(message.clone(), |span| format_error(span, &message))
    })?;

    if !status_x86.success() {
        let message = format!("x86_64-unknown-cosmo-cc failed with status {}", status_x86);
        return Err(main_span.map_or(message.clone(), |span| format_error(span, &message)));
    }

    let status_aarch64 = run_tool_command(
        &aarch64_cc,
        &[
            "-static",
            "-O3",
            obj_path_aarch64.to_str().unwrap(),
            "-o",
            elf_path_aarch64.to_str().unwrap(),
        ],
        &tmp_dir,
    )
    .map_err(|error| {
        let message = format!("failed to execute aarch64-unknown-cosmo-cc: {}", error);
        main_span.map_or(message.clone(), |span| format_error(span, &message))
    })?;

    if !status_aarch64.success() {
        let message = format!("aarch64-unknown-cosmo-cc failed with status {}", status_aarch64);
        return Err(main_span.map_or(message.clone(), |span| format_error(span, &message)));
    }

    let mut apelink_args: Vec<&str> = Vec::new();
    let ape_x86_str = ape_x86.to_str().unwrap();
    let ape_aarch64_str = ape_aarch64.to_str().unwrap();
    let ape_m1_str = ape_m1.to_str().unwrap();

    if ape_x86.exists() {
        apelink_args.extend(["-l", ape_x86_str]);
    }
    if ape_aarch64.exists() {
        apelink_args.extend(["-l", ape_aarch64_str]);
    }
    if ape_m1.exists() {
        apelink_args.extend(["-M", ape_m1_str]);
    }

    let elf_x86_str = elf_path_x86.to_str().unwrap();
    let elf_aarch64_str = elf_path_aarch64.to_str().unwrap();
    apelink_args.extend(["-o", "main", elf_x86_str, elf_aarch64_str]);

    let status_apelink = run_tool_command(&apelink, &apelink_args, &tmp_dir)
        .map_err(|error| {
            let message = format!("failed to execute apelink: {}", error);
            main_span.map_or(message.clone(), |span| format_error(span, &message))
        })?;

    if !status_apelink.success() {
        let message = format!("apelink failed with status {}", status_apelink);
        return Err(main_span.map_or(message.clone(), |span| format_error(span, &message)));
    }

    let _ = fs::remove_file(obj_path_x86);
    let _ = fs::remove_file(obj_path_aarch64);
    let _ = fs::remove_file(elf_path_x86);
    let _ = fs::remove_file(elf_path_aarch64);

    println!("  [6/6] Linking                    ({})", format_duration(step_start.elapsed()));

    println!("Finished build in {}", format_duration(total_start.elapsed()));
    Ok(())
}
