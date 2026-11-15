use std::collections::{HashMap, HashSet};
use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use inkwell::OptimizationLevel;
use inkwell::context::Context;
use inkwell::targets::{CodeModel, InitializationConfig, RelocMode, Target, TargetTriple};

use crate::codegen::Codegen;
use crate::sema::{Checker, ModuleResolver};
use crate::syntax::{
    Definition, ExportRule, Expression, ExternalFunction, IfArm, Lexer, LogicalModule, MatchArm,
    ModuleDecl, NamespacePath, Parameter, Parser, Program, Signature, SourceFile, StructFieldInit,
    Type, TypeAlias, format_error,
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
    let mut zzz_files = Vec::new();
    let mut visited_dirs = Vec::new();

    fn collect_zzz_files(
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
            } else if path.extension().and_then(|e| e.to_str()) == Some("zzz") {
                files.push(path);
            }
        }

        subdirs.sort();
        for subdir in subdirs {
            collect_zzz_files(&subdir, files, visited)?;
        }

        Ok(())
    }

    for dir in dirs {
        if dir.is_dir() {
            collect_zzz_files(dir, &mut zzz_files, &mut visited_dirs)?;
        }
    }

    zzz_files.sort();

    // Map named module -> (Option<Declaration (exports, imports, decl_path)>, Vec<SourceFile>)
    let mut named_modules: HashMap<
        String,
        (
            Option<(Vec<ExportRule>, Vec<NamespacePath>, PathBuf)>,
            Vec<SourceFile>,
        ),
    > = HashMap::new();
    let mut unnamed_modules: Vec<LogicalModule> = Vec::new();

    for path in zzz_files {
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
                        "duplicate declaration of module '{}' in '{}'",
                        name,
                        path.display()
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
        Expression::SizeOf(ty) => Ok(Expression::SizeOf(canonicalize_type(ty, checker)?)),
        Expression::TypeOf(expr) => Ok(Expression::TypeOf(Box::new(canonicalize_expression(expr, checker)?))),
        Expression::Slice(expr) => Ok(Expression::Slice(Box::new(canonicalize_expression(expr, checker)?))),
        Expression::Type(ty) => Ok(Expression::Type(canonicalize_type(ty, checker)?)),
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
    let mut module_scopes = resolver.resolve_modules(&modules)?;

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
                if def.is_meta || def.is_def {
                    continue;
                }
                let resolved_sig = def
                    .signature
                    .as_ref()
                    .map(|s| canonicalize_signature(s, &checker))
                    .transpose()?;
                let resolved_value = canonicalize_expression(&def.value, &checker)?;

                if def.name == "main" {
                    if def.is_def {
                        return Err(format!(
                            "in '{}': entry point 'main' must be a function ('func'), never a 'def'",
                            file.file_path
                        ));
                    }
                    if let Some(sig) = &resolved_sig {
                        let ret = &sig.return_type;
                        if **ret != Type::S32
                            && !matches!(**ret, Type::Symbol(ref name) if name == "none")
                        {
                            return Err(format!(
                                "in '{}': entry point 'main' must return 's32' or ':none', found {:?}",
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
                        is_meta: false,
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
                    is_meta: false,
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
