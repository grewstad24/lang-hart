use std::collections::{HashMap, HashSet};

use crate::syntax::{
    ComparisonOperator, Definition, Expression, ExternalFunction, IfCondition, LogicalModule,
    MatchArm, NamespacePath, Parameter, Signature, SourceFile, Span, StructFieldInit,
    StructFieldType, Type, TypeAlias, format_error, unspanned_expression,
    unspanned_expression_mut,
};

fn expression_span(expression: &Expression) -> Option<&Span> {
    match expression {
        Expression::Spanned { span, .. } => Some(span),
        _ => None,
    }
}

#[derive(Debug, Clone)]
pub struct SymbolProvenance<T> {
    pub item: T,
    pub originating_file: String,
}

#[derive(Debug, Clone)]
pub struct ModuleScope {
    pub dir_path: String,
    pub definitions: HashMap<String, SymbolProvenance<Definition>>,
    pub extern_functions: HashMap<String, SymbolProvenance<ExternalFunction>>,
    pub type_aliases: HashMap<String, SymbolProvenance<TypeAlias>>,
    pub home_type_aliases: HashMap<String, SymbolProvenance<TypeAlias>>,
    pub exported_symbols: HashSet<String>,
}

impl ModuleScope {
    pub fn new(dir_path: String) -> Self {
        Self {
            dir_path,
            definitions: HashMap::new(),
            extern_functions: HashMap::new(),
            type_aliases: HashMap::new(),
            home_type_aliases: HashMap::new(),
            exported_symbols: HashSet::new(),
        }
    }

    pub fn lookup_definition(&self, name: &str) -> Option<&Definition> {
        self.definitions.get(name).map(|p| &p.item)
    }

    pub fn lookup_extern(&self, name: &str) -> Option<&ExternalFunction> {
        self.extern_functions.get(name).map(|p| &p.item)
    }

    pub fn lookup_type_alias(&self, name: &str) -> Option<&TypeAlias> {
        self.type_aliases.get(name).map(|p| &p.item)
    }
}

pub fn context_type() -> Type {
    fn sorted_union(mut vars: Vec<Type>) -> Type {
        vars.sort_by_key(Checker::canonical_type_key);
        Type::Union(vars)
    }
    fn err_sym(sym: &str) -> Type {
        Type::Tuple(vec![
            Type::Symbol("error".to_string()),
            Type::Symbol(sym.to_string()),
        ])
    }

    let mem_error = Type::Symbol("error".to_string());
    let io_error_variants = vec![
        err_sym("permission_denied"),
        err_sym("not_found"),
        err_sym("interrupted"),
        err_sym("io_error"),
        err_sym("bad_fd"),
        err_sym("would_block"),
        err_sym("out_of_memory"),
        err_sym("access_denied"),
        err_sym("already_exists"),
        err_sym("not_a_directory"),
        err_sym("is_a_directory"),
        err_sym("invalid_argument"),
        err_sym("no_space"),
        err_sym("broken_pipe"),
        err_sym("unknown"),
    ];
    let with_io_err = |ok_ty: Type| -> Type {
        let mut vars = io_error_variants.clone();
        vars.push(ok_ty);
        sorted_union(vars)
    };
    let ctx_ptr = Type::Pointer {
        element: Box::new(Type::Alias("Context".to_string())),
        is_mut: false,
    };
    let log_level_variants = sorted_union(vec![
        Type::Symbol("info".to_string()),
        Type::Symbol("warn".to_string()),
        Type::Symbol("error".to_string()),
        Type::Symbol("never".to_string()),
    ]);
    let open_mode_variants = sorted_union(vec![
        Type::Symbol("read".to_string()),
        Type::Symbol("write".to_string()),
        Type::Symbol("append".to_string()),
        Type::Symbol("read_write".to_string()),
    ]);
    let seek_whence_variants = sorted_union(vec![
        Type::Symbol("start".to_string()),
        Type::Symbol("current".to_string()),
        Type::Symbol("end".to_string()),
    ]);
    let log_level_struct = Type::Struct(vec![
        StructFieldType {
            name: "info".to_string(),
            ty: Type::Alias("Bool".to_string()),
        },
        StructFieldType {
            name: "warn".to_string(),
            ty: Type::Alias("Bool".to_string()),
        },
        StructFieldType {
            name: "error".to_string(),
            ty: Type::Alias("Bool".to_string()),
        },
    ]);
    Type::Struct(vec![
        StructFieldType {
            name: "logger".to_string(),
            ty: Type::Function {
                parameters: vec![
                    ctx_ptr.clone(),
                    log_level_variants,
                    log_level_struct.clone(),
                    Type::U64,
                    Type::Alias("String".to_string()),
                    Type::Alias("Generic".to_string()),
                ],
                return_type: Box::new(Type::Symbol("none".to_string())),
                is_variadic: false,
            },
        },
        StructFieldType {
            name: "log_level".to_string(),
            ty: log_level_struct,
        },
        StructFieldType {
            name: "heap_alloc".to_string(),
            ty: Type::Function {
                parameters: vec![ctx_ptr.clone(), Type::USize],
                return_type: Box::new(sorted_union(vec![
                    mem_error.clone(),
                    Type::Pointer {
                        element: Box::new(Type::Opaque),
                        is_mut: true,
                    },
                ])),
                is_variadic: false,
            },
        },
        StructFieldType {
            name: "heap_realloc".to_string(),
            ty: Type::Function {
                parameters: vec![
                    ctx_ptr.clone(),
                    Type::Pointer {
                        element: Box::new(Type::Opaque),
                        is_mut: false,
                    },
                    Type::USize,
                ],
                return_type: Box::new(sorted_union(vec![
                    mem_error,
                    Type::Pointer {
                        element: Box::new(Type::Opaque),
                        is_mut: true,
                    },
                ])),
                is_variadic: false,
            },
        },
        StructFieldType {
            name: "heap_dealloc".to_string(),
            ty: Type::Function {
                parameters: vec![
                    ctx_ptr.clone(),
                    Type::Pointer {
                        element: Box::new(Type::Opaque),
                        is_mut: false,
                    },
                ],
                return_type: Box::new(Type::Symbol("none".to_string())),
                is_variadic: false,
            },
        },
        StructFieldType {
            name: "printer".to_string(),
            ty: Type::Function {
                parameters: vec![ctx_ptr.clone(), Type::Alias("Generic".to_string())],
                return_type: Box::new(with_io_err(Type::Symbol("none".to_string()))),
                is_variadic: false,
            },
        },
        StructFieldType {
            name: "panic".to_string(),
            ty: Type::Function {
                parameters: vec![ctx_ptr.clone(), Type::Alias("Generic".to_string())],
                return_type: Box::new(Type::Never),
                is_variadic: false,
            },
        },
        StructFieldType {
            name: "open".to_string(),
            ty: Type::Function {
                parameters: vec![
                    ctx_ptr.clone(),
                    Type::Alias("String".to_string()),
                    open_mode_variants,
                ],
                return_type: Box::new(with_io_err(Type::S32)),
                is_variadic: false,
            },
        },
        StructFieldType {
            name: "close".to_string(),
            ty: Type::Function {
                parameters: vec![ctx_ptr.clone(), Type::S32],
                return_type: Box::new(with_io_err(Type::Symbol("none".to_string()))),
                is_variadic: false,
            },
        },
        StructFieldType {
            name: "read".to_string(),
            ty: Type::Function {
                parameters: vec![
                    ctx_ptr.clone(),
                    Type::S32,
                    Type::Pointer {
                        element: Box::new(Type::U8),
                        is_mut: true,
                    },
                    Type::USize,
                ],
                return_type: Box::new(with_io_err(Type::USize)),
                is_variadic: false,
            },
        },
        StructFieldType {
            name: "write".to_string(),
            ty: Type::Function {
                parameters: vec![
                    ctx_ptr.clone(),
                    Type::S32,
                    Type::Pointer {
                        element: Box::new(Type::U8),
                        is_mut: false,
                    },
                    Type::USize,
                ],
                return_type: Box::new(with_io_err(Type::USize)),
                is_variadic: false,
            },
        },
        StructFieldType {
            name: "seek".to_string(),
            ty: Type::Function {
                parameters: vec![ctx_ptr, Type::S32, Type::S64, seek_whence_variants],
                return_type: Box::new(with_io_err(Type::U64)),
                is_variadic: false,
            },
        },
    ])
}

pub struct ModuleResolver {
    namespace_registry: HashMap<NamespacePath, ExportRegistry>,
}

#[derive(Default)]
struct ExportRegistry {
    definitions: HashMap<String, SymbolProvenance<Definition>>,
    extern_functions: HashMap<String, SymbolProvenance<ExternalFunction>>,
    type_aliases: HashMap<String, SymbolProvenance<TypeAlias>>,
    home_type_aliases: HashMap<String, SymbolProvenance<TypeAlias>>,
}

pub fn expand_type_aliases_with_map(ty: &Type, aliases: &HashMap<String, TypeAlias>) -> Type {
    let mut visited = HashSet::new();

    fn expand(
        t: &Type,
        aliases: &HashMap<String, TypeAlias>,
        visited: &mut HashSet<String>,
        indirect: bool,
    ) -> Type {
        match t {
            Type::Alias(name) => {
                if indirect {
                    return t.clone();
                }
                if !visited.insert(name.clone()) {
                    return t.clone();
                }
                let res = if let Some(alias) = aliases.get(name) {
                    expand(&alias.target, aliases, visited, indirect)
                } else {
                    t.clone()
                };
                visited.remove(name);
                res
            }
            Type::Union(variants) => {
                let mut flat = Vec::new();
                for v in variants {
                    let res = expand(v, aliases, visited, indirect);
                    if let Type::Union(inner) = res {
                        flat.extend(inner);
                    } else {
                        flat.push(res);
                    }
                }
                let mut unique = Vec::new();
                for v in flat {
                    if !unique.contains(&v) {
                        unique.push(v);
                    }
                }
                unique.sort_by_key(Checker::canonical_type_key);
                if unique.len() == 1 {
                    unique.remove(0)
                } else {
                    Type::Union(unique)
                }
            }
            Type::Struct(fields) => {
                let flds = fields
                    .iter()
                    .map(|f| StructFieldType {
                        name: f.name.clone(),
                        ty: expand(&f.ty, aliases, visited, indirect),
                    })
                    .collect();
                Type::Struct(flds)
            }
            Type::Tuple(elements) => {
                let elems = elements
                    .iter()
                    .map(|e| expand(e, aliases, visited, indirect))
                    .collect();
                Type::Tuple(elems)
            }
            Type::Pointer { element, is_mut } => Type::Pointer {
                element: Box::new(expand(element, aliases, visited, true)),
                is_mut: *is_mut,
            },
            Type::Array { element, size } => Type::Array {
                element: Box::new(expand(element, aliases, visited, indirect)),
                size: *size,
            },
            Type::Function {
                parameters,
                return_type,
                is_variadic,
            } => {
                let params = parameters
                    .iter()
                    .map(|p| expand(p, aliases, visited, indirect))
                    .collect();
                Type::Function {
                    parameters: params,
                    return_type: Box::new(expand(return_type, aliases, visited, indirect)),
                    is_variadic: *is_variadic,
                }
            }
            Type::GenericInstance {
                name,
                type_arguments,
            } => {
                let new_args = type_arguments
                    .iter()
                    .map(|a| expand(a, aliases, visited, indirect))
                    .collect();
                Type::GenericInstance {
                    name: name.clone(),
                    type_arguments: new_args,
                }
            }
            other => other.clone(),
        }
    }

    expand(ty, aliases, &mut visited, false)
}

impl ModuleResolver {
    pub fn new() -> Self {
        Self {
            namespace_registry: HashMap::new(),
        }
    }

    pub fn resolve_modules(
        mut self,
        modules: &[LogicalModule],
    ) -> Result<Vec<ModuleScope>, String> {
        for module in modules {
            let mut module_aliases = HashMap::new();
            for file in &module.source_files {
                for alias in &file.type_aliases {
                    module_aliases.insert(alias.name.clone(), alias.clone());
                }
            }

            for export_rule in &module.exports {
                let target_ns = export_rule.target.clone();
                let registry = self
                    .namespace_registry
                    .entry(target_ns.clone())
                    .or_default();

                for file in &module.source_files {
                    for alias in &file.type_aliases {
                        registry.home_type_aliases.insert(
                            alias.name.clone(),
                            SymbolProvenance {
                                item: alias.clone(),
                                originating_file: file.file_path.clone(),
                            },
                        );
                    }
                }

                for sym in &export_rule.symbols {
                    let mut found_def = None;
                    let mut found_ext = None;
                    let mut found_alias = None;

                    for file in &module.source_files {
                        if let Some(def) = file.definitions.iter().find(|d| d.name == *sym) {
                            let mut resolved_def = def.clone();
                            if let Some(sig) = &mut resolved_def.signature {
                                for p in &mut sig.parameters {
                                    p.ty = expand_type_aliases_with_map(&p.ty, &module_aliases);
                                }
                                *sig.return_type =
                                    expand_type_aliases_with_map(&sig.return_type, &module_aliases);
                            }
                            found_def = Some(SymbolProvenance {
                                item: resolved_def,
                                originating_file: file.file_path.clone(),
                            });
                            break;
                        } else if let Some(ext) =
                            file.extern_functions.iter().find(|e| e.name == *sym)
                        {
                            let mut resolved_ext = ext.clone();
                            for p in &mut resolved_ext.signature.parameters {
                                p.ty = expand_type_aliases_with_map(&p.ty, &module_aliases);
                            }
                            *resolved_ext.signature.return_type = expand_type_aliases_with_map(
                                &resolved_ext.signature.return_type,
                                &module_aliases,
                            );
                            found_ext = Some(SymbolProvenance {
                                item: resolved_ext,
                                originating_file: file.file_path.clone(),
                            });
                            break;
                        } else if let Some(alias) =
                            file.type_aliases.iter().find(|a| a.name == *sym)
                        {
                            let mut resolved_alias = alias.clone();
                            resolved_alias.target = expand_type_aliases_with_map(
                                &resolved_alias.target,
                                &module_aliases,
                            );
                            found_alias = Some(SymbolProvenance {
                                item: resolved_alias,
                                originating_file: file.file_path.clone(),
                            });
                            break;
                        }
                    }

                    if let Some(prov) = found_def {
                        if let Some(existing) = registry.definitions.get(sym) {
                            let message = format!(
                                "export collision in namespace '{}': symbol '{}' in '{}' conflicts with '{}'",
                                target_ns, sym, prov.originating_file, existing.originating_file
                            );
                            return Err(expression_span(&prov.item.value)
                                .map_or(message.clone(), |span| format_error(span, &message)));
                        }
                        registry.definitions.insert(sym.clone(), prov);
                    } else if let Some(prov) = found_ext {
                        if let Some(existing) = registry.extern_functions.get(sym) {
                            let message = format!(
                                "export collision in namespace '{}': extern '{}' in '{}' conflicts with '{}'",
                                target_ns, sym, prov.originating_file, existing.originating_file
                            );
                            return Err(format_error(&prov.item.span, &message));
                        }
                        registry.extern_functions.insert(sym.clone(), prov);
                    } else if let Some(prov) = found_alias {
                        if let Some(existing) = registry.type_aliases.get(sym) {
                            let message = format!(
                                "export collision in namespace '{}': type alias '{}' in '{}' conflicts with '{}'",
                                target_ns, sym, prov.originating_file, existing.originating_file
                            );
                            return Err(format_error(&prov.item.span, &message));
                        }
                        registry.type_aliases.insert(sym.clone(), prov);
                    } else {
                        let message = format!(
                            "export error: symbol '{}' is not defined in any .hart file in this module",
                            sym
                        );
                        return Err(format_error(&export_rule.span, &message));
                    }
                }
            }
        }

        let mut resolved_scopes = Vec::new();

        for module in modules {
            let mut scope = ModuleScope::new(module.dir_path.clone());

            for export_rule in &module.exports {
                for sym in &export_rule.symbols {
                    scope.exported_symbols.insert(sym.clone());
                }
            }

            for file in &module.source_files {
                for def in &file.definitions {
                    if let Some(existing) = scope.definitions.get(&def.name) {
                        let message = format!(
                            "duplicate definition of symbol '{}' in '{}', already defined in '{}'",
                            def.name, file.file_path, existing.originating_file
                        );
                        return Err(expression_span(&def.value)
                            .map_or(message.clone(), |span| format_error(span, &message)));
                    }
                    scope.definitions.insert(
                        def.name.clone(),
                        SymbolProvenance {
                            item: def.clone(),
                            originating_file: file.file_path.clone(),
                        },
                    );
                }

                for ext in &file.extern_functions {
                    if let Some(existing) = scope.extern_functions.get(&ext.name) {
                        let message = format!(
                            "duplicate extern function '{}' in '{}', already defined in '{}'",
                            ext.name, file.file_path, existing.originating_file
                        );
                        return Err(format_error(&ext.span, &message));
                    }
                    scope.extern_functions.insert(
                        ext.name.clone(),
                        SymbolProvenance {
                            item: ext.clone(),
                            originating_file: file.file_path.clone(),
                        },
                    );
                }

                for alias in &file.type_aliases {
                    if let Some(existing) = scope.type_aliases.get(&alias.name) {
                        let message = format!(
                            "duplicate type alias '{}' in '{}', already defined in '{}'",
                            alias.name, file.file_path, existing.originating_file
                        );
                        return Err(format_error(&alias.span, &message));
                    }
                    scope.type_aliases.insert(
                        alias.name.clone(),
                        SymbolProvenance {
                            item: alias.clone(),
                            originating_file: file.file_path.clone(),
                        },
                    );
                }
            }

            let imports = module.imports.clone();

            for ns in &imports {
                if let Some(exported) = self.namespace_registry.get(ns) {
                    for (name, prov) in &exported.definitions {
                        if !scope.definitions.contains_key(name) {
                            scope.definitions.insert(name.clone(), prov.clone());
                        }
                    }
                    for (name, prov) in &exported.extern_functions {
                        if !scope.extern_functions.contains_key(name) {
                            scope.extern_functions.insert(name.clone(), prov.clone());
                        }
                    }
                    for (name, prov) in &exported.type_aliases {
                        if !scope.type_aliases.contains_key(name) {
                            scope.type_aliases.insert(name.clone(), prov.clone());
                        }
                    }
                    for (name, prov) in &exported.home_type_aliases {
                        if !scope.home_type_aliases.contains_key(name) {
                            scope.home_type_aliases.insert(name.clone(), prov.clone());
                        }
                    }
                }
            }

            resolved_scopes.push(scope);
        }

        Ok(resolved_scopes)
    }
}

#[derive(Debug, Clone)]
struct BindingInfo {
    ty: Type,
    is_mutable: bool,
    is_initialized: bool,
    may_point_to_stack: bool,
}

pub struct Checker<'a> {
    pub scope: &'a ModuleScope,
    pub current_file_path: String,

    resolved_extern_functions: HashMap<String, Signature>,
    resolving_extern_functions: HashSet<String>,

    pub resolved_definitions: HashMap<String, Type>,
    resolving_definitions: HashSet<String>,
    checked_bodies: HashSet<String>,

    local_scopes: Vec<HashMap<String, BindingInfo>>,
    symbol_table: HashMap<String, u32>,
    current_function_return_type: Option<Type>,
    is_return_inferred: bool,
    loop_depth: usize,
    block_yield_stack: Vec<Vec<Type>>,
    counter: usize,
    pub current_definition_name: Option<String>,
    pub current_definition_is_def: bool,
}

impl<'a> Checker<'a> {
    pub fn generic_type() -> Type {
        Type::Alias("Generic".to_string())
    }

    pub fn record_type() -> Type {
        Type::Alias("Record".to_string())
    }

    pub fn find_target_member_for_downcast(
        source_variant: &Type,
        target_members: &[Type],
        aliases: &HashMap<String, Type>,
    ) -> Option<Type> {
        if let Some(target) = target_members.iter().find(|t| *t == source_variant) {
            return Some(target.clone());
        }

        let compatible_matches: Vec<&Type> = target_members
            .iter()
            .filter(|target| Self::types_compatible_with_aliases(source_variant, target, aliases))
            .collect();
        if !compatible_matches.is_empty() {
            return Some(compatible_matches[0].clone());
        }

        let cast_matches: Vec<&Type> = target_members
            .iter()
            .filter(|target| Self::is_supported_cast_with_aliases(source_variant, target, aliases))
            .collect();
        if !cast_matches.is_empty() {
            return Some(cast_matches[0].clone());
        }

        None
    }

    pub fn new(scope: &'a ModuleScope) -> Self {
        let mut checker = Self {
            scope,
            current_file_path: String::new(),
            resolved_extern_functions: HashMap::new(),
            resolving_extern_functions: HashSet::new(),
            resolved_definitions: HashMap::new(),
            resolving_definitions: HashSet::new(),
            checked_bodies: HashSet::new(),
            local_scopes: Vec::new(),
            symbol_table: HashMap::new(),
            current_function_return_type: None,
            is_return_inferred: false,
            loop_depth: 0,
            block_yield_stack: Vec::new(),
            counter: 0,
            current_definition_name: None,
            current_definition_is_def: false,
        };

        checker.predeclare_scope_signatures();
        checker
    }

    pub fn enter_scope(&mut self) {
        self.local_scopes.push(HashMap::new());
    }

    pub fn exit_scope(&mut self) {
        self.local_scopes.pop();
    }

    pub fn intern_symbol(&mut self, name: &str) -> u32 {
        if let Some(&id) = self.symbol_table.get(name) {
            id
        } else {
            let id = self.symbol_table.len() as u32;
            self.symbol_table.insert(name.to_string(), id);
            id
        }
    }

    pub fn is_slice_type(&self, ty: &Type) -> Result<bool, String> {
        let resolved = self.resolve_type(ty)?;
        if let Type::Struct(fields) = &resolved {
            if fields.len() == 2 && fields[0].name == "ptr" && fields[1].name == "len" {
                let f0 = self.resolve_type(&fields[0].ty)?;
                let f1 = self.resolve_type(&fields[1].ty)?;
                if matches!(f0, Type::Pointer { .. }) && matches!(f1, Type::USize) {
                    return Ok(true);
                }
            }
        }
        Ok(false)
    }

    pub fn slice_element_type(&self, ty: &Type) -> Result<Type, String> {
        let resolved = self.resolve_type(ty)?;
        if let Type::Struct(fields) = &resolved {
            if fields.len() == 2 && fields[0].name == "ptr" {
                let f0 = self.resolve_type(&fields[0].ty)?;
                if let Type::Pointer { element, .. } = f0 {
                    return Ok(*element);
                }
            }
        }
        Err(format!("expected slice type, found {}", ty))
    }

    pub fn slice_pointer_type(&self, ty: &Type) -> Result<Type, String> {
        let resolved = self.resolve_type(ty)?;
        if let Type::Struct(fields) = &resolved {
            if fields.len() == 2 && fields[0].name == "ptr" {
                return self.resolve_type(&fields[0].ty);
            }
        }
        Err(format!("expected slice type, found {}", ty))
    }

    pub fn is_string_type(&self, ty: &Type) -> Result<bool, String> {
        let resolved = self.resolve_type(ty)?;
        if self.is_slice_type(&resolved)? {
            let elem = self.slice_element_type(&resolved)?;
            return Ok(self.resolve_type(&elem)? == Type::U8);
        }
        Ok(false)
    }

    pub fn is_record_type(&self, ty: &Type) -> Result<bool, String> {
        let resolved = self.resolve_type(ty)?;
        if let Type::Pointer { element, .. } = &resolved {
            let elem_resolved = self.resolve_type(element)?;
            if let Type::Struct(fields) = elem_resolved {
                if fields.len() == 3
                    && fields[0].name == "len"
                    && fields[1].name == "key"
                    && fields[2].name == "val"
                {
                    return Ok(true);
                }
            }
        }
        if let Type::Alias(name) = ty {
            if name == "Record" {
                return Ok(true);
            }
        }
        Ok(false)
    }

    pub fn bool_type() -> Type {
        Type::Union(vec![
            Type::Symbol("false".to_string()),
            Type::Symbol("true".to_string()),
        ])
    }

    pub fn is_bool_type(&self, ty: &Type) -> Result<bool, String> {
        let resolved = self.resolve_type(ty)?;
        if let Type::Union(variants) = &resolved {
            if variants.len() == 2
                && variants.contains(&Type::Symbol("false".to_string()))
                && variants.contains(&Type::Symbol("true".to_string()))
            {
                return Ok(true);
            }
        }
        if matches!(&resolved, Type::Symbol(s) if s == "true" || s == "false") {
            return Ok(true);
        }
        Ok(false)
    }

    pub fn type_may_contain_error(&self, ty: &Type) -> Result<bool, String> {
        let resolved = self.resolve_type(ty)?;
        match &resolved {
            Type::Symbol(s) if s == "error" => Ok(true),
            Type::Tuple(elems) if !elems.is_empty() => {
                let first = self.resolve_type(&elems[0])?;
                if matches!(&first, Type::Symbol(s) if s == "error") {
                    Ok(true)
                } else {
                    Ok(false)
                }
            }
            Type::Union(variants) => {
                for v in variants {
                    if self.type_may_contain_error(v)? {
                        return Ok(true);
                    }
                }
                Ok(false)
            }
            _ => Ok(false),
        }
    }

    pub fn canonical_type_key(ty: &Type) -> String {
        Self::canonical_type_key_depth(ty, 0)
    }

    fn canonical_type_key_depth(ty: &Type, depth: usize) -> String {
        if depth > 32 {
            return "...".to_string();
        }
        match ty {
            Type::Opaque => "00_opaque".to_string(),
            Type::Inferred => "00_inferred".to_string(),
            Type::Never => "00_never".to_string(),
            Type::U8 => "02_u8".to_string(),
            Type::U16 => "03_u16".to_string(),
            Type::U32 => "04_u32".to_string(),
            Type::U64 => "05_u64".to_string(),
            Type::USize => "05a_usize".to_string(),
            Type::S16 => "06_s16".to_string(),
            Type::S32 => "07_s32".to_string(),
            Type::S64 => "08_s64".to_string(),
            Type::SSize => "08a_ssize".to_string(),
            Type::Symbol(name) => format!("09_symbol_{}", name),
            Type::Alias(name) => format!("10_alias_{}", name),
            Type::GenericInstance {
                name,
                type_arguments,
            } => {
                let inner = type_arguments
                    .iter()
                    .map(|a| Self::canonical_type_key_depth(a, depth + 1))
                    .collect::<Vec<_>>()
                    .join(",");
                format!("10_generic_{}<{}>", name, inner)
            }
            Type::Pointer { element, is_mut } => format!(
                "11_ptr_{}_{}",
                if *is_mut { "mut" } else { "const" },
                Self::canonical_type_key_depth(element, depth + 1)
            ),
            Type::Array { element, size } => {
                format!("13_arr_{}_{}", size, Self::canonical_type_key_depth(element, depth + 1))
            }
            Type::Struct(fields) => {
                let inner = fields
                    .iter()
                    .map(|f| format!("{}:{}", f.name, Self::canonical_type_key_depth(&f.ty, depth + 1)))
                    .collect::<Vec<_>>()
                    .join(",");
                format!("14_struct_{{{}}}", inner)
            }
            Type::Tuple(elements) => {
                let inner = elements
                    .iter()
                    .map(|e| Self::canonical_type_key_depth(e, depth + 1))
                    .collect::<Vec<_>>()
                    .join(",");
                format!("14_tuple_({})", inner)
            }
            Type::Function {
                parameters,
                return_type,
                is_variadic,
            } => {
                let params = parameters
                    .iter()
                    .map(|p| Self::canonical_type_key_depth(p, depth + 1))
                    .collect::<Vec<_>>()
                    .join(",");
                format!(
                    "15_fn_({})_{}_{}",
                    params,
                    Self::canonical_type_key_depth(return_type, depth + 1),
                    is_variadic
                )
            }
            Type::Union(variants) => {
                let inner = variants
                    .iter()
                    .map(|v| Self::canonical_type_key_depth(v, depth + 1))
                    .collect::<Vec<_>>()
                    .join("|");
                format!("16_union_{}", inner)
            }
        }
    }

    pub fn resolve_type(&self, ty: &Type) -> Result<Type, String> {
        let mut visited = HashSet::new();
        self.resolve_type_inner(ty, &mut visited, false)
    }

    fn validate_user_type(&self, ty: &Type) -> Result<(), String> {
        match ty {
            Type::Alias(name) => {
                if self.scope.lookup_type_alias(name).is_none() {
                    return Err(format!(
                        "{}: error: unknown type '{}'",
                        self.current_file_path, name
                    ));
                }
                Ok(())
            }
            Type::Pointer { element, .. } => self.validate_user_type(element),
            Type::Array { element, .. } => self.validate_user_type(element),
            Type::Union(variants) => {
                for v in variants {
                    self.validate_user_type(v)?;
                }
                Ok(())
            }
            Type::Struct(fields) => {
                for f in fields {
                    self.validate_user_type(&f.ty)?;
                }
                Ok(())
            }
            Type::Tuple(elements) => {
                for e in elements {
                    self.validate_user_type(e)?;
                }
                Ok(())
            }
            Type::GenericInstance { type_arguments, .. } => {
                for arg in type_arguments {
                    self.validate_user_type(arg)?;
                }
                Ok(())
            }
            _ => Ok(()),
        }
    }

    fn resolve_type_inner(
        &self,
        ty: &Type,
        visited: &mut HashSet<String>,
        indirect: bool,
    ) -> Result<Type, String> {
        match ty {
            Type::Alias(name) => {
                if indirect {
                    return Ok(Type::Alias(name.clone()));
                }
                if !visited.insert(name.clone()) {
                    return Err(format!(
                        "{}: error: recursive type alias '{}' has an infinitely-sized by-value cycle",
                        self.current_file_path, name
                    ));
                }
                let target = self
                    .scope
                    .lookup_type_alias(name)
                    .map(|a| &a.target)
                    .or_else(|| self.scope.home_type_aliases.get(name).map(|p| &p.item.target))
                    .ok_or_else(|| {
                        format!("{}: error: unknown type '{}'", self.current_file_path, name)
                    })?;
                let resolved = self.resolve_type_inner(target, visited, indirect)?;
                visited.remove(name);
                Ok(resolved)
            }
            Type::GenericInstance {
                name,
                type_arguments,
            } => {
                let resolved_args: Vec<Type> = type_arguments
                    .iter()
                    .map(|arg| self.resolve_type_inner(arg, visited, indirect))
                    .collect::<Result<_, _>>()?;
                let mangled_suffix = resolved_args
                    .iter()
                    .map(crate::monomorphize::mangle_type)
                    .collect::<Vec<_>>()
                    .join("__");
                let concrete_name = crate::monomorphize::sanitize_identifier(&format!(
                    "{}__{}",
                    name, mangled_suffix
                ));
                if self.scope.lookup_type_alias(&concrete_name).is_some() {
                    return self.resolve_type_inner(&Type::Alias(concrete_name), visited, indirect);
                }
                if let Some(template) = self.scope.lookup_type_alias(name) {
                    if template.is_meta {
                        let mut substitutions = HashMap::new();
                        for (param, arg) in template.type_parameters.iter().zip(resolved_args.iter()) {
                            substitutions.insert(param.clone(), arg.clone());
                        }
                        let substituted = crate::monomorphize::substitute_type(&template.target, &substitutions);
                        return self.resolve_type_inner(&substituted, visited, indirect);
                    }
                }
                self.resolve_type_inner(&Type::Alias(concrete_name), visited, indirect)
            }
            Type::Struct(fields) => {
                let resolved_fields = fields
                    .iter()
                    .map(|f| {
                        Ok(StructFieldType {
                            name: f.name.clone(),
                            ty: self.resolve_type_inner(&f.ty, visited, indirect)?,
                        })
                    })
                    .collect::<Result<Vec<_>, String>>()?;
                Ok(Type::Struct(resolved_fields))
            }
            Type::Tuple(elements) => {
                let resolved_elements = elements
                    .iter()
                    .map(|e| self.resolve_type_inner(e, visited, indirect))
                    .collect::<Result<Vec<_>, String>>()?;
                Ok(Type::Tuple(resolved_elements))
            }
            Type::Pointer { element, is_mut } => Ok(Type::Pointer {
                element: Box::new(self.resolve_type_inner(element, visited, true)?),
                is_mut: *is_mut,
            }),
            Type::Array { element, size } => Ok(Type::Array {
                element: Box::new(self.resolve_type_inner(element, visited, indirect)?),
                size: *size,
            }),
            Type::Function {
                parameters,
                return_type,
                is_variadic,
            } => {
                let params = parameters
                    .iter()
                    .map(|p| self.resolve_type_inner(p, visited, true))
                    .collect::<Result<Vec<_>, _>>()?;
                let ret = self.resolve_type_inner(return_type, visited, true)?;
                Ok(Type::Function {
                    parameters: params,
                    return_type: Box::new(ret),
                    is_variadic: *is_variadic,
                })
            }
            Type::Union(variants) => {
                let mut flat = Vec::new();
                for v in variants {
                    let resolved = self.resolve_type_inner(v, visited, false)?;
                    if let Type::Union(inner) = resolved {
                        flat.extend(inner);
                    } else {
                        flat.push(resolved);
                    }
                }

                let mut unique: Vec<Type> = Vec::new();
                for v in flat {
                    if !unique.contains(&v) {
                        unique.push(v);
                    }
                }

                if unique.len() > 256 {
                    return Err(format!(
                        "{}: error: sum type exceeds maximum of 256 variants (found {})",
                        self.current_file_path,
                        unique.len()
                    ));
                }

                unique
                    .sort_by(|a, b| Self::canonical_type_key(a).cmp(&Self::canonical_type_key(b)));

                if unique.len() == 1 {
                    Ok(unique.into_iter().next().unwrap())
                } else {
                    Ok(Type::Union(unique))
                }
            }
            concrete => Ok(concrete.clone()),
        }
    }

    pub fn unify_to_sum_type(&self, types: &[Type]) -> Result<Type, String> {
        if types.is_empty() {
            return Ok(Type::Symbol("none".to_string()));
        }

        let mut flattened = Vec::new();
        for t in types {
            let resolved = self.resolve_type(t)?;
            match resolved {
                Type::Union(variants) => {
                    for v in variants {
                        flattened.push(self.resolve_type(&v)?);
                    }
                }
                other => flattened.push(other),
            }
        }

        let mut unique: Vec<Type> = Vec::new();
        for t in flattened {
            if !unique.iter().any(|u| u == &t) {
                unique.push(t);
            }
        }

        let mut non_never: Vec<Type> = unique.into_iter().filter(|t| *t != Type::Never).collect();
        if non_never.is_empty() {
            return Ok(Type::Never);
        }

        if non_never.len() == 1 {
            return Ok(non_never.remove(0));
        }

        non_never.sort_by_key(Self::canonical_type_key);
        Ok(Type::Union(non_never))
    }

    fn check_struct_fields(
        &mut self,
        fields: &mut Vec<StructFieldInit>,
    ) -> Result<Vec<StructFieldType>, String> {
        if fields.iter().any(|f| f.name.is_none()) {
            let mut expanded_fields: Vec<StructFieldInit> = Vec::new();
            for mut field in fields.drain(..) {
                if field.name.is_none() {
                    let val_ty = self.type_of_expression(&mut field.value)?;
                    let resolved = self.resolve_type(&val_ty)?;
                    match resolved {
                        Type::Struct(s_fields) => {
                            for sf in s_fields {
                                let member_expr = Expression::Member {
                                    expression: Box::new(field.value.clone()),
                                    field: sf.name.clone(),
                                };
                                if let Some(existing) = expanded_fields.iter_mut().find(|f| f.name.as_ref() == Some(&sf.name)) {
                                    existing.value = member_expr;
                                } else {
                                    expanded_fields.push(StructFieldInit {
                                        name: Some(sf.name),
                                        value: member_expr,
                                    });
                                }
                            }
                        }
                        _ => return Err(format!("{}: error: cannot spread non-struct type {} in struct literal", self.current_file_path, resolved)),
                    }
                } else {
                    let name = field.name.as_ref().unwrap();
                    if let Some(existing) = expanded_fields.iter_mut().find(|f| f.name.as_ref() == Some(name)) {
                        existing.value = field.value;
                    } else {
                        expanded_fields.push(field);
                    }
                }
            }
            *fields = expanded_fields;
        }

        let mut struct_fields = Vec::with_capacity(fields.len());
        let mut seen_fields = HashSet::new();

        for field in fields {
            let name = field.name.clone().unwrap();
            if !seen_fields.insert(name.clone()) {
                return Err(format!(
                    "{}: error: duplicate field '@{}' in struct literal",
                    self.current_file_path, name
                ));
            }
            let field_ty = self.type_of_expression(&mut field.value)?;
            struct_fields.push(StructFieldType {
                name,
                ty: field_ty,
            });
        }

        Ok(struct_fields)
    }

    /// Enforced semantics: whenever there is a type mismatch, and there is a supported cast
    /// between the mismatched type and the expected type where only one possible cast would
    /// make it correct, that cast will be inserted implicitly.
    pub fn try_coerce_unambiguous(
        &self,
        expr: &mut Expression,
        actual_ty: &Type,
        expected_ty: &Type,
    ) -> Result<Option<Type>, String> {
        let res_actual = self.resolve_type(actual_ty)?;
        let res_expected = self.resolve_type(expected_ty)?;

        if res_actual == res_expected {
            return Ok(Some(res_actual));
        }

        let aliases = self.alias_targets();

        // 1. Literal integer to expected integer type
        if let Expression::Integer(val) = unspanned_expression(expr) {
            if Self::is_integer_type(&res_expected) {
                let fits = match res_expected {
                    Type::U8 => *val >= 0 && *val <= u8::MAX as i64,
                    Type::U16 => *val >= 0 && *val <= u16::MAX as i64,
                    Type::U32 => *val >= 0 && *val <= u32::MAX as i64,
                    Type::U64 | Type::USize => *val >= 0,
                    Type::S16 => *val >= i16::MIN as i64 && *val <= i16::MAX as i64,
                    Type::S32 => *val >= i32::MIN as i64 && *val <= i32::MAX as i64,
                    Type::S64 | Type::SSize => true,
                    _ => false,
                };
                if fits {
                    let orig = std::mem::replace(expr, Expression::Symbol("false".to_string()));
                    *expr = Expression::Cast {
                        expression: Box::new(orig),
                        ty: expected_ty.clone(),
                    };
                    return Ok(Some(res_expected));
                }
            }
        }

        // 2. Safe lossless integer widening
        if Self::is_lossless_integer_widening(&res_actual, &res_expected) {
            let orig = std::mem::replace(expr, Expression::Symbol("false".to_string()));
            *expr = Expression::Cast {
                expression: Box::new(orig),
                ty: expected_ty.clone(),
            };
            return Ok(Some(res_expected));
        }

        // 3. Types compatible with aliases
        if Self::types_compatible_with_aliases(&res_actual, &res_expected, &aliases) {
            let orig = std::mem::replace(expr, Expression::Symbol("false".to_string()));
            *expr = Expression::Cast {
                expression: Box::new(orig),
                ty: expected_ty.clone(),
            };
            return Ok(Some(res_expected));
        }

        // 4. Expected is a union: if exactly one variant is supported, insert cast
        if let Type::Union(target_vars) = &res_expected {
            if let Some(_variant_idx) = Self::castable_union_variant_with_aliases(&res_actual, target_vars, &aliases) {
                let orig = std::mem::replace(expr, Expression::Symbol("false".to_string()));
                *expr = Expression::Cast {
                    expression: Box::new(orig),
                    ty: expected_ty.clone(),
                };
                return Ok(Some(res_expected));
            }
            return Ok(None);
        }

        // 5. Expected is a single concrete type (unambiguous target):
        // If there is a supported cast between the mismatched type and the expected type,
        // only this single cast makes it correct. Insert it implicitly.
        if Self::is_supported_cast_with_aliases(&res_actual, &res_expected, &aliases) {
            let orig = std::mem::replace(expr, Expression::Symbol("false".to_string()));
            *expr = Expression::Cast {
                expression: Box::new(orig),
                ty: expected_ty.clone(),
            };
            return Ok(Some(res_expected));
        }

        Ok(None)
    }

    fn coerce_expression_to_type(
        &mut self,
        expr: &mut Expression,
        actual_ty: &Type,
        expected_ty: &Type,
    ) -> Result<Type, String> {
        let res_actual = self.resolve_type(actual_ty)?;
        let res_expected = self.resolve_type(expected_ty)?;

        if let Some(coerced) = self.try_coerce_unambiguous(expr, &res_actual, &res_expected)? {
            return Ok(coerced);
        }

        Ok(res_actual)
    }

    fn coerce_binary_operands_unambiguous(
        &self,
        left: &mut Expression,
        left_type: &mut Type,
        right: &mut Expression,
        right_type: &mut Type,
    ) -> bool {
        if left_type == right_type {
            return true;
        }

        let aliases = self.alias_targets();

        let right_is_lit = matches!(unspanned_expression(right), Expression::Integer(_));
        let left_is_lit = matches!(unspanned_expression(left), Expression::Integer(_));

        if right_is_lit && !left_is_lit && Self::is_integer_type(left_type) {
            let orig = std::mem::replace(right, Expression::Symbol("false".to_string()));
            *right = Expression::Cast {
                expression: Box::new(orig),
                ty: left_type.clone(),
            };
            *right_type = left_type.clone();
            return true;
        }

        if left_is_lit && !right_is_lit && Self::is_integer_type(right_type) {
            let orig = std::mem::replace(left, Expression::Symbol("false".to_string()));
            *left = Expression::Cast {
                expression: Box::new(orig),
                ty: right_type.clone(),
            };
            *left_type = right_type.clone();
            return true;
        }

        if Self::is_lossless_integer_widening(left_type, right_type) {
            let orig = std::mem::replace(left, Expression::Symbol("false".to_string()));
            *left = Expression::Cast {
                expression: Box::new(orig),
                ty: right_type.clone(),
            };
            *left_type = right_type.clone();
            return true;
        }

        if Self::is_lossless_integer_widening(right_type, left_type) {
            let orig = std::mem::replace(right, Expression::Symbol("false".to_string()));
            *right = Expression::Cast {
                expression: Box::new(orig),
                ty: left_type.clone(),
            };
            *right_type = left_type.clone();
            return true;
        }

        let can_cast_left_to_right = Self::is_supported_cast_with_aliases(left_type, right_type, &aliases);
        let can_cast_right_to_left = Self::is_supported_cast_with_aliases(right_type, left_type, &aliases);

        if can_cast_left_to_right && !can_cast_right_to_left {
            let orig = std::mem::replace(left, Expression::Symbol("false".to_string()));
            *left = Expression::Cast {
                expression: Box::new(orig),
                ty: right_type.clone(),
            };
            *left_type = right_type.clone();
            return true;
        }

        if can_cast_right_to_left && !can_cast_left_to_right {
            let orig = std::mem::replace(right, Expression::Symbol("false".to_string()));
            *right = Expression::Cast {
                expression: Box::new(orig),
                ty: left_type.clone(),
            };
            *right_type = left_type.clone();
            return true;
        }

        if Self::is_integer_type(left_type) && Self::is_integer_type(right_type) {
            let w_left = Self::integer_bit_width(left_type);
            let w_right = Self::integer_bit_width(right_type);
            if let (Some(wl), Some(wr)) = (w_left, w_right) {
                if wl < wr {
                    let orig = std::mem::replace(left, Expression::Symbol("false".to_string()));
                    *left = Expression::Cast {
                        expression: Box::new(orig),
                        ty: right_type.clone(),
                    };
                    *left_type = right_type.clone();
                    return true;
                } else if wr < wl {
                    let orig = std::mem::replace(right, Expression::Symbol("false".to_string()));
                    *right = Expression::Cast {
                        expression: Box::new(orig),
                        ty: left_type.clone(),
                    };
                    *right_type = left_type.clone();
                    return true;
                }
            }
        }

        if matches!(left_type, Type::Pointer { .. }) && matches!(right_type, Type::Pointer { .. }) {
            let left_comp = Self::types_compatible_with_aliases(left_type, right_type, &aliases);
            let right_comp = Self::types_compatible_with_aliases(right_type, left_type, &aliases);
            if left_comp && !right_comp {
                let orig = std::mem::replace(left, Expression::Symbol("false".to_string()));
                *left = Expression::Cast {
                    expression: Box::new(orig),
                    ty: right_type.clone(),
                };
                *left_type = right_type.clone();
                return true;
            }
            if right_comp && !left_comp {
                let orig = std::mem::replace(right, Expression::Symbol("false".to_string()));
                *right = Expression::Cast {
                    expression: Box::new(orig),
                    ty: left_type.clone(),
                };
                *right_type = left_type.clone();
                return true;
            }
        }

        false
    }

    fn predeclare_scope_signatures(&mut self) {
        for (name, prov) in &self.scope.extern_functions {
            self.resolved_extern_functions
                .insert(name.clone(), prov.item.signature.clone());
        }

        for (name, prov) in &self.scope.definitions {
            if prov.item.is_def {
                continue;
            }
            if let Some(sig) = &prov.item.signature {
                let mut param_types = Vec::new();
                for p in &sig.parameters {
                    let res_ty = self.resolve_type(&p.ty).unwrap_or_else(|_| p.ty.clone());
                    param_types.push(res_ty);
                }
                let expected_return = self
                    .resolve_type(&sig.return_type)
                    .unwrap_or_else(|_| *sig.return_type.clone());
                let fn_ty = Type::Function {
                    parameters: param_types,
                    return_type: Box::new(expected_return),
                    is_variadic: sig.is_variadic,
                };
                self.resolved_definitions.insert(name.clone(), fn_ty);
            }
        }
    }

    pub fn check_file(&mut self, file: &mut SourceFile) -> Result<(), String> {
        self.current_file_path = file.file_path.clone();

        for alias in &mut file.type_aliases {
            if alias.is_meta {
                continue;
            }
            let mut visited = HashSet::from([alias.name.clone()]);
            alias.target = self
                .resolve_type_inner(&alias.target, &mut visited, false)
                .map_err(|error| format_error(&alias.span, &error))?;
        }

        for ext in &mut file.extern_functions {
            let resolved_sig = self
                .resolve_external_function(&ext.name)
                .map_err(|error| format_error(&ext.span, &error))?;
            ext.signature = resolved_sig;
        }

        for def in &mut file.definitions {
            self.check_definition_body(def).map_err(|error| {
                if let Expression::Spanned { span, .. } = &def.value {
                    format_error(span, &error)
                } else {
                    error
                }
            })?;
        }

        Ok(())
    }

    fn check_definition_body(&mut self, def: &mut Definition) -> Result<(), String> {
        self.current_definition_name = Some(def.name.clone());
        if def.name == "main" && def.is_def {
            return Err(format!(
                "{}: error: entry point 'main' must be a function ('func'), never a 'def'",
                self.current_file_path
            ));
        }
        if def.is_def {
            return Ok(());
        }

        if !self.checked_bodies.insert(def.name.clone()) {
            if let Some(sig) = &mut def.signature {
                if *sig.return_type == Type::Inferred {
                    if let Some(Type::Function { return_type, .. }) = self.resolved_definitions.get(&def.name) {
                        *sig.return_type = *return_type.clone();
                    }
                }
            }
            return Ok(());
        }

        if let Some(sig) = &mut def.signature {
            let mut seen_params = HashSet::new();
            self.enter_scope();
            for p in &mut sig.parameters {
                self.validate_user_type(&p.ty)?;
                if !seen_params.insert(p.name.clone()) {
                    self.exit_scope();
                    return Err(format!(
                        "{}: error: duplicate parameter name '{}' in function '{}'",
                        self.current_file_path, p.name, def.name
                    ));
                }
                let resolved_param_ty = self.resolve_type(&p.ty)?;
                if resolved_param_ty == Type::Never {
                    self.exit_scope();
                    return Err(format!(
                        "{}: error: parameter '{}' cannot be never in function '{}'",
                        self.current_file_path, p.name, def.name
                    ));
                }
                p.ty = resolved_param_ty.clone();
                self.local_scopes.last_mut().unwrap().insert(
                    p.name.clone(),
                    BindingInfo {
                        ty: resolved_param_ty,
                        is_mutable: false,
                        is_initialized: true,
                        may_point_to_stack: false,
                    },
                );
            }
            let is_ctx_initialized = def.is_def || def.is_specialized_def;
            self.local_scopes.last_mut().unwrap().insert(
                "@ctx".to_string(),
                BindingInfo {
                    ty: context_type(),
                    is_mutable: true,
                    is_initialized: is_ctx_initialized,
                    may_point_to_stack: false,
                },
            );
            self.validate_user_type(&sig.return_type)?;
            let expected_return = self.resolve_type(sig.return_type.as_ref())?;
            *sig.return_type = expected_return.clone();

            let old_return_type = self
                .current_function_return_type
                .replace(expected_return.clone());
            let old_is_return_inferred = self.is_return_inferred;
            self.is_return_inferred = expected_return == Type::Inferred;
            let old_is_def = self.current_definition_is_def;
            self.current_definition_is_def = def.is_def || def.is_specialized_def;

            let body_ty_res = self.type_of_expression(&mut def.value);
            let captured_return = self.current_function_return_type.take();
            self.current_function_return_type = old_return_type;
            self.is_return_inferred = old_is_return_inferred;
            self.current_definition_is_def = old_is_def;

            let mut body_ty = match body_ty_res {
                Ok(ty) => ty,
                Err(e) => {
                    self.exit_scope();
                    return Err(e);
                }
            };
            if body_ty != Type::Never
                && self.type_contains_pointer(&body_ty)
                && self.expr_may_point_to_stack(&def.value)
            {
                self.exit_scope();
                return Err(format!(
                    "{}: error: pointer to local stack variable cannot escape function '{}' via return",
                    self.current_file_path, def.name
                ));
            }
            self.exit_scope();
            if expected_return != Type::Inferred && body_ty != Type::Never {
                body_ty =
                    self.coerce_expression_to_type(&mut def.value, &body_ty, &expected_return)?;
                if !self.are_types_compatible(&body_ty, &expected_return)? {
                    return Err(format!(
                        "{}: error: function '{}' return type mismatch: expected {}, found {}",
                        self.current_file_path, def.name, expected_return, body_ty
                    ));
                }
            } else if expected_return == Type::Inferred {
                let deduced = match (captured_return, body_ty) {
                    (Some(ret), body) if ret != Type::Inferred && body != Type::Never => {
                        self.unify_to_sum_type(&[ret, body])?
                    }
                    (Some(ret), _) if ret != Type::Inferred => ret,
                    (_, body) => {
                        if body == Type::Never {
                            Type::Symbol("none".to_string())
                        } else {
                            body
                        }
                    }
                };
                *sig.return_type = deduced.clone();
                let param_types = sig.parameters.iter().map(|p| p.ty.clone()).collect();
                self.resolved_definitions.insert(
                    def.name.clone(),
                    Type::Function {
                        parameters: param_types,
                        return_type: Box::new(deduced),
                        is_variadic: sig.is_variadic,
                    },
                );
            }
        } else {
            let ty = self.type_of_expression(&mut def.value)?;
            self.resolved_definitions.insert(def.name.clone(), ty);
        }

        Ok(())
    }

    fn resolve_external_function(&mut self, name: &str) -> Result<Signature, String> {
        let raw = self.scope.lookup_extern(name).ok_or_else(|| {
            format!(
                "{}: error: unknown external function '{}'",
                self.current_file_path, name
            )
        })?;

        if !self.resolving_extern_functions.insert(name.to_string()) {
            return Err(format!(
                "{}: error: cyclic dependency involving external function '{}'",
                self.current_file_path, name
            ));
        }

        let mut seen_params = HashSet::new();
        let mut parameters = Vec::new();

        for p in &raw.signature.parameters {
            if !seen_params.insert(p.name.clone()) {
                return Err(format!(
                    "{}: error: duplicate parameter name '{}' in extern function '{}'",
                    self.current_file_path, p.name, name
                ));
            }
            let resolved_p_ty = self.resolve_type(&p.ty)?;
            if resolved_p_ty == Type::Never {
                return Err(format!(
                    "{}: error: parameter '{}' in extern function '{}' cannot be never",
                    self.current_file_path, p.name, name
                ));
            }
            parameters.push(Parameter {
                name: p.name.clone(),
                ty: resolved_p_ty,
            });
        }

        let signature = Signature {
            parameters,
            return_type: Box::new(self.resolve_type(raw.signature.return_type.as_ref())?),
            is_variadic: raw.signature.is_variadic,
        };

        self.resolving_extern_functions.remove(name);
        self.resolved_extern_functions
            .insert(name.to_string(), signature.clone());

        Ok(signature)
    }

    pub fn resolve_definition(&mut self, name: &str) -> Result<Type, String> {
        if let Some(ty) = self.resolved_definitions.get(name) {
            if let Type::Function { return_type, .. } = ty {
                if **return_type != Type::Inferred {
                    return Ok(ty.clone());
                }
            } else {
                return Ok(ty.clone());
            }
        }

        let def = self.scope.lookup_definition(name).cloned().ok_or_else(|| {
            format!(
                "{}: error: unknown identifier '{}' (not defined in this module or imported)",
                self.current_file_path, name
            )
        })?;

        if def.is_def {
            return Err(format!(
                "{}: error: 'def' function '{}' cannot be referenced as a runtime value",
                self.current_file_path,
                name
            ));
        }

        if let Some(sig) = &def.signature {
            let param_types = sig
                .parameters
                .iter()
                .map(|p| self.resolve_type(&p.ty))
                .collect::<Result<Vec<_>, _>>()?;
            let ret_ty = self.resolve_type(sig.return_type.as_ref())?;
            if ret_ty != Type::Inferred {
                let fn_ty = Type::Function {
                    parameters: param_types,
                    return_type: Box::new(ret_ty),
                    is_variadic: sig.is_variadic,
                };
                self.resolved_definitions
                    .insert(name.to_string(), fn_ty.clone());
                return Ok(fn_ty);
            }
            let mut def_clone = def.clone();
            self.check_definition_body(&mut def_clone)?;
            if let Some(resolved) = self.resolved_definitions.get(name) {
                return Ok(resolved.clone());
            }
        }

        if !self.resolving_definitions.insert(name.to_string()) {
            return Err(format!(
                "{}: error: cyclic dependency involving global constant '@{}'",
                self.current_file_path, name
            ));
        }

        let mut expr_clone = def.value.clone();
        let ty_result = self.type_of_expression(&mut expr_clone);
        self.resolving_definitions.remove(name);

        let ty = self.resolve_type(&ty_result?)?;
        self.resolved_definitions
            .insert(name.to_string(), ty.clone());
        Ok(ty)
    }

    fn lookup_binding(&self, name: &str) -> Option<BindingInfo> {
        self.local_scopes
            .iter()
            .rev()
            .find_map(|s| s.get(name).cloned())
    }

    fn lookup_binding_mut(&mut self, name: &str) -> Option<&mut BindingInfo> {
        self.local_scopes
            .iter_mut()
            .rev()
            .find_map(|s| s.get_mut(name))
    }

    fn initialization_state(&self) -> Vec<HashMap<String, bool>> {
        self.local_scopes
            .iter()
            .map(|scope| {
                scope
                    .iter()
                    .map(|(name, binding)| (name.clone(), binding.is_initialized))
                    .collect()
            })
            .collect()
    }

    fn restore_initialization_state(&mut self, scopes: Vec<HashMap<String, BindingInfo>>) {
        self.local_scopes = scopes;
    }

    fn merge_initialization_states(&mut self, paths: &[Vec<HashMap<String, bool>>]) {
        for (scope_index, scope) in self.local_scopes.iter_mut().enumerate() {
            for (name, binding) in scope {
                binding.is_initialized = paths.iter().all(|path| {
                    path.get(scope_index)
                        .and_then(|state| state.get(name))
                        .copied()
                        .unwrap_or(false)
                });
            }
        }
    }

    fn type_contains_pointer(&self, ty: &Type) -> bool {
        let resolved = self.resolve_type(ty).unwrap_or_else(|_| ty.clone());
        match resolved {
            Type::Pointer { .. } => true,
            Type::Union(vars) => vars.iter().any(|v| self.type_contains_pointer(v)),
            Type::Tuple(elems) => elems.iter().any(|e| self.type_contains_pointer(e)),
            Type::Struct(fields) => fields.iter().any(|f| self.type_contains_pointer(&f.ty)),
            Type::Array { element, .. } => self.type_contains_pointer(&element),
            _ => false,
        }
    }

    fn is_diverging_expr(expr: &Expression) -> bool {
        matches!(
            unspanned_expression(expr),
            Expression::Return(_) | Expression::Break | Expression::Continue | Expression::Yield(_)
        )
    }

    fn expr_may_point_to_stack(&self, expr: &Expression) -> bool {
        match unspanned_expression(expr) {
            Expression::AddressOf { expression, .. } => {
                match unspanned_expression(expression.as_ref()) {
                    Expression::Identifier(name) => self.lookup_binding(name).is_some(),
                    Expression::Context => true,
                    Expression::Dereference(inner) => {
                        // Taking address of dereference: ptr.*. & is just ptr
                        self.expr_may_point_to_stack(inner.as_ref())
                    }
                    Expression::Index { expression: base, .. } => {
                        self.expr_may_point_to_stack(base.as_ref())
                    }
                    Expression::Member { expression: base, .. } => {
                        self.expr_may_point_to_stack(base.as_ref())
                    }
                    _ => false,
                }
            }
            Expression::Identifier(name) => {
                if let Some(info) = self.lookup_binding(name) {
                    info.may_point_to_stack
                } else {
                    false
                }
            }
            Expression::Cast { expression, ty } => {
                self.type_contains_pointer(ty) && self.expr_may_point_to_stack(expression.as_ref())
            }
            Expression::AsTypeOf { expression, .. } => {
                self.expr_may_point_to_stack(expression.as_ref())
            }
            Expression::Block(stmts) => {
                stmts.last().map_or(false, |s| self.expr_may_point_to_stack(s))
            }
            Expression::If { arms } => {
                let non_diverging: Vec<_> = arms
                    .iter()
                    .filter(|arm| !Self::is_diverging_expr(&arm.body))
                    .collect();
                !non_diverging.is_empty()
                    && non_diverging
                        .iter()
                        .all(|arm| self.expr_may_point_to_stack(&arm.body))
            }
            Expression::Match { arms, .. } => {
                let non_diverging: Vec<_> = arms
                    .iter()
                    .filter(|arm| !Self::is_diverging_expr(&arm.body))
                    .collect();
                !non_diverging.is_empty()
                    && non_diverging
                        .iter()
                        .all(|arm| self.expr_may_point_to_stack(&arm.body))
            }
            Expression::Tuple(elements) => {
                elements.iter().any(|elem| self.expr_may_point_to_stack(elem))
            }
            Expression::Struct(fields) => {
                fields.iter().any(|f| self.expr_may_point_to_stack(&f.value))
            }
            _ => false,
        }
    }

    fn check_lvalue_mutability(&mut self, expr: &Expression) -> Result<(), String> {
        match unspanned_expression(expr) {
            Expression::Identifier(name) => {
                if self.scope.lookup_definition(name.as_str()).is_some() {
                    return Err(format!(
                        "{}: error: cannot reassign to module-level constant '{}'",
                        self.current_file_path, name
                    ));
                }
                if self.scope.lookup_extern(name.as_str()).is_some() {
                    return Err(format!(
                        "{}: error: cannot assign to external function '{}'",
                        self.current_file_path, name
                    ));
                }
                if self.scope.lookup_type_alias(name.as_str()).is_some() {
                    return Err(format!(
                        "{}: error: cannot assign to type alias '{}'",
                        self.current_file_path, name
                    ));
                }

                if let Some(info) = self.lookup_binding(name.as_str()) {
                    if !info.is_mutable {
                        return Err(format!(
                            "{}: error: cannot reassign to immutable constant '{}'",
                            self.current_file_path, name
                        ));
                    }
                } else {
                    return Err(format!(
                        "{}: error: unknown variable '{}'",
                        self.current_file_path, name
                    ));
                }
                Ok(())
            }
            Expression::Dereference(ptr_expr) => {
                let mut ptr_clone = ptr_expr.as_ref().clone();
                let raw_ptr_ty = self.type_of_expression(&mut ptr_clone)?;
                let ptr_ty = self.resolve_type(&raw_ptr_ty)?;
                let Type::Pointer { element, is_mut } = ptr_ty else {
                    return Err(format!(
                        "{}: error: cannot dereference non-pointer type {} in assignment target",
                        self.current_file_path, ptr_ty
                    ));
                };
                if !is_mut {
                    return Err(format!(
                        "{}: error: cannot mutate through immutable pointer '*{}'",
                        self.current_file_path, Self::canonical_type_key(&element)
                    ));
                }
                if *element == Type::Opaque {
                    return Err(format!(
                        "{}: error: cannot assign through untyped pointer '*opaque'",
                        self.current_file_path
                    ));
                }
                Ok(())
            }
            Expression::Member { expression: base, .. } => {
                self.check_lvalue_mutability(base.as_ref())
            }
            Expression::Index { expression: base, .. } => {
                let mut base_clone = base.as_ref().clone();
                let raw_base_ty = self.type_of_expression(&mut base_clone)?;
                let base_ty = self.resolve_type(&raw_base_ty)?;
                match base_ty {
                    Type::Array { .. } => self.check_lvalue_mutability(base.as_ref()),
                    Type::Pointer { is_mut: true, .. } => Ok(()),
                    Type::Pointer { is_mut: false, .. } => Err(format!(
                        "{}: error: cannot mutate through immutable pointer in assignment target",
                        self.current_file_path
                    )),
                    other if self.is_slice_type(&other)? => {
                        let ptr_ty = self.slice_pointer_type(&other)?;
                        if let Type::Pointer { is_mut: true, .. } = ptr_ty {
                            Ok(())
                        } else {
                            Err(format!(
                                "{}: error: cannot mutate through immutable slice in assignment target",
                                self.current_file_path
                            ))
                        }
                    }
                    other => Err(format!(
                        "{}: error: cannot index type {} in assignment target",
                        self.current_file_path, other
                    )),
                }
            }
            Expression::Context => Ok(()),
            _ => Err(format!(
                "{}: error: invalid assignment target: expression is not an lvalue",
                self.current_file_path
            )),
        }
    }

    pub fn type_of_expression(&mut self, expression: &mut Expression) -> Result<Type, String> {
        match expression {
            Expression::Spanned { expression, span } => self
                .type_of_expression(expression)
                .map_err(|error| format_error(span, &error)),
            Expression::Spread(inner) => self.type_of_expression(inner),
            Expression::Integer(_) => Ok(Type::S32),
            Expression::Symbol(name) => {
                self.intern_symbol(name);
                Ok(Type::Symbol(name.clone()))
            }
            Expression::String(_) => Ok(Type::Alias("String".to_string())),
            Expression::Context => {
                if let Some(info) = self.lookup_binding("@ctx") {
                    if !info.is_initialized {
                        return Err(format!(
                            "{}: error: context '@ctx' is accessed before it is initialized",
                            self.current_file_path
                        ));
                    }
                    return Ok(info.ty);
                }
                Ok(context_type())
            }
            Expression::CallerLineNumber => {
                if !self.current_definition_is_def {
                    return Err(format!(
                        "{}: error: '@caller_line_number' can only be used inside a 'def' function",
                        self.current_file_path
                    ));
                }
                Ok(Type::U64)
            }
            Expression::CallerFileName => {
                if !self.current_definition_is_def {
                    return Err(format!(
                        "{}: error: '@caller_file_name' can only be used inside a 'def' function",
                        self.current_file_path
                    ));
                }
                Ok(Type::Alias("String".to_string()))
            }
            Expression::ThisLineNumber(_) => Ok(Type::U64),
            Expression::ThisFileName(_) => Ok(Type::Alias("String".to_string())),

            Expression::Struct(fields) => {
                let struct_fields = self.check_struct_fields(fields)?;
                Ok(Type::Struct(struct_fields))
            }

            Expression::Tuple(elements) => {
                if elements.iter().any(|e| matches!(crate::syntax::unspanned_expression(e), Expression::Spread(_))) {
                    let mut expanded = Vec::new();
                    for el in elements.drain(..) {
                        let is_spread = matches!(crate::syntax::unspanned_expression(&el), Expression::Spread(_));
                        if is_spread {
                            let mut inner = match el {
                                Expression::Spread(inner) => *inner,
                                Expression::Spanned { expression, .. } => match *expression {
                                    Expression::Spread(inner) => *inner,
                                    _ => unreachable!(),
                                },
                                _ => unreachable!(),
                            };
                            let inner_ty = self.type_of_expression(&mut inner)?;
                            let resolved = self.resolve_type(&inner_ty)?;
                            match resolved {
                                Type::Array { size, .. } => {
                                    for i in 0..size {
                                        expanded.push(Expression::Index {
                                            expression: Box::new(inner.clone()),
                                            index: Box::new(Expression::Integer(i as i64)),
                                        });
                                    }
                                }
                                Type::Tuple(tys) => {
                                    for i in 0..tys.len() {
                                        expanded.push(Expression::Member {
                                            expression: Box::new(inner.clone()),
                                            field: i.to_string(),
                                        });
                                    }
                                }
                                _ => return Err(format!("{}: error: spread operator in tuple requires fixed-size array or tuple, found {}", self.current_file_path, resolved)),
                            }
                        } else {
                            expanded.push(el);
                        }
                    }
                    *elements = expanded;
                }

                let mut elem_types = Vec::with_capacity(elements.len());
                for elem in elements.iter_mut() {
                    let ty = self.type_of_expression(elem)?;
                    elem_types.push(ty);
                }
                Ok(Type::Tuple(elem_types))
            }

            Expression::Identifier(name) => {
                if self.scope.lookup_type_alias(name).is_some() {
                    return Err(format!(
                        "{}: error: type alias '{}' cannot be used as an expression",
                        self.current_file_path, name
                    ));
                }

                if let Some(info) = self.lookup_binding(name) {
                    if info.is_mutable && !info.is_initialized {
                        return Err(format!(
                            "{}: error: mutable variable '{}' may be read before it is initialized",
                            self.current_file_path, name
                        ));
                    }
                    return Ok(info.ty);
                }

                if self.scope.definitions.contains_key(name) {
                    let def_type = self.resolve_definition(name)?;
                    return Ok(def_type);
                }

                if let Some(ext) = self.scope.lookup_extern(name) {
                    let param_types = ext
                        .signature
                        .parameters
                        .iter()
                        .map(|p| self.resolve_type(&p.ty))
                        .collect::<Result<Vec<_>, _>>()?;
                    let ret_ty = self.resolve_type(ext.signature.return_type.as_ref())?;
                    return Ok(Type::Function {
                        parameters: param_types,
                        return_type: Box::new(ret_ty),
                        is_variadic: ext.signature.is_variadic,
                    });
                }

                Err(format!(
                    "{}: error: unknown identifier '{}' is not defined in this module or imported",
                    self.current_file_path, name
                ))
            }

            Expression::Block(exprs) => {
                let enclosing_scopes = self.local_scopes.clone();
                self.enter_scope();
                self.block_yield_stack.push(Vec::new());

                let mut block_ty = Type::Symbol("none".to_string());
                let mut diverged = false;
                let len = exprs.len();
                for (i, expr) in exprs.iter_mut().enumerate() {
                    let ty = self.type_of_expression(expr)?;
                    if ty == Type::Never {
                        diverged = true;
                    }
                    if i == len - 1 {
                        block_ty = ty;
                    }
                }

                let yields = self.block_yield_stack.pop().unwrap_or_default();
                self.local_scopes = enclosing_scopes;
                if !yields.is_empty() {
                    let unified = self.unify_to_sum_type(&yields)?;
                    Ok(unified)
                } else if diverged {
                    Ok(Type::Never)
                } else {
                    Ok(block_ty)
                }
            }

            Expression::Defer(expr) => {
                let _ = self.type_of_expression(expr)?;
                Ok(Type::Symbol("none".to_string()))
            }

            Expression::Discard(expr) => {
                let ty = self.type_of_expression(expr)?;
                if self.type_may_contain_error(&ty)? {
                    return Err(format!(
                        "{}: error: expression of type '{}' may be an error and cannot be discarded",
                        self.current_file_path, ty
                    ));
                }
                if ty == Type::Never {
                    Ok(Type::Never)
                } else {
                    Ok(Type::Symbol("none".to_string()))
                }
            }

            Expression::Return(expr) => {
                let expected = self.current_function_return_type.clone().ok_or_else(|| {
                    format!(
                        "{}: error: 'return' outside of function",
                        self.current_file_path
                    )
                })?;
                let mut actual = self.type_of_expression(expr)?;
                if actual != Type::Never
                    && self.type_contains_pointer(&actual)
                    && self.expr_may_point_to_stack(expr)
                {
                    return Err(format!(
                        "{}: error: pointer to local stack variable cannot escape function via return",
                        self.current_file_path
                    ));
                }
                if self.is_return_inferred {
                    if expected == Type::Inferred {
                        self.current_function_return_type = Some(actual);
                    } else {
                        let unified = self.unify_to_sum_type(&[expected, actual])?;
                        self.current_function_return_type = Some(unified);
                    }
                    return Ok(Type::Never);
                }
                actual = self.coerce_expression_to_type(expr, &actual, &expected)?;
                if !self.are_types_compatible(&actual, &expected)? {
                    return Err(format!(
                        "{}: error: return type mismatch: expected {}, found {}",
                        self.current_file_path, expected, actual
                    ));
                }
                Ok(Type::Never)
            }

            Expression::Yield(expr) => {
                let actual = self.type_of_expression(expr)?;
                if let Some(yields) = self.block_yield_stack.last_mut() {
                    yields.push(actual);
                } else {
                    return Err(format!(
                        "{}: error: 'yield' outside of block",
                        self.current_file_path
                    ));
                }
                Ok(Type::Never)
            }

            Expression::Break => {
                if self.loop_depth == 0 {
                    return Err(format!(
                        "{}: error: 'break' outside of loop",
                        self.current_file_path
                    ));
                }
                Ok(Type::Never)
            }

            Expression::Continue => {
                if self.loop_depth == 0 {
                    return Err(format!(
                        "{}: error: 'continue' outside of loop",
                        self.current_file_path
                    ));
                }
                Ok(Type::Never)
            }

            Expression::Let { name, value } => {
                if self.local_scopes.last().unwrap().contains_key(name) {
                    return Err(format!(
                        "{}: error: variable '{}' already declared in this scope",
                        self.current_file_path, name
                    ));
                }
                let raw_ty = self.type_of_expression(value)?;
                let resolved_ty = self.resolve_type(&raw_ty)?;
                if resolved_ty == Type::Never {
                    return Err(format!(
                        "{}: error: cannot bind variable '{}' to never expression",
                        self.current_file_path, name
                    ));
                }
                let may_point_to_stack = self.expr_may_point_to_stack(value);
                self.local_scopes.last_mut().unwrap().insert(
                    name.clone(),
                    BindingInfo {
                        ty: resolved_ty,
                        is_mutable: false,
                        is_initialized: true,
                        may_point_to_stack,
                    },
                );
                Ok(Type::Symbol("none".to_string()))
            }

            Expression::Var { name, ty, value } => {
                if self.local_scopes.last().unwrap().contains_key(name) {
                    return Err(format!(
                        "{}: error: variable '{}' already declared in this scope",
                        self.current_file_path, name
                    ));
                }
                if let Some(val) = value {
                    let raw_ty = self.type_of_expression(val)?;
                    let resolved_ty = self.resolve_type(&raw_ty)?;
                    if resolved_ty == Type::Never {
                        return Err(format!(
                            "{}: error: cannot bind variable '{}' to never expression",
                            self.current_file_path, name
                        ));
                    }
                    let final_ty = if let Some(annotated_ty) = ty {
                        self.validate_user_type(annotated_ty)?;
                        let res_annotated = self.resolve_type(annotated_ty)?;
                        *annotated_ty = res_annotated.clone();
                        let coerced =
                            self.coerce_expression_to_type(val, &resolved_ty, &res_annotated)?;
                        if !self.are_types_compatible(&coerced, &res_annotated)? {
                            return Err(format!(
                                "{}: error: type mismatch for variable '{}': expected {}, found {}",
                                self.current_file_path, name, res_annotated, coerced
                            ));
                        }
                        res_annotated
                    } else {
                        resolved_ty
                    };
                    let may_point_to_stack = self.expr_may_point_to_stack(val);
                    self.local_scopes.last_mut().unwrap().insert(
                        name.clone(),
                        BindingInfo {
                            ty: final_ty,
                            is_mutable: true,
                            is_initialized: true,
                            may_point_to_stack,
                        },
                    );
                } else if let Some(annotated_ty) = ty {
                    self.validate_user_type(annotated_ty)?;
                    let resolved_ty = self.resolve_type(annotated_ty)?;
                    if resolved_ty == Type::Never {
                        return Err(format!(
                            "{}: error: variable '{}' cannot be declared with type never",
                            self.current_file_path, name
                        ));
                    }
                    *annotated_ty = resolved_ty.clone();
                    self.local_scopes.last_mut().unwrap().insert(
                        name.clone(),
                        BindingInfo {
                            ty: resolved_ty,
                            is_mutable: true,
                            is_initialized: false,
                            may_point_to_stack: false,
                        },
                    );
                } else {
                    return Err(format!(
                        "{}: error: variable '{}' must specify either a type or an initial value",
                        self.current_file_path, name
                    ));
                }
                Ok(Type::Symbol("none".to_string()))
            }

            Expression::Assign { target, value } => match unspanned_expression_mut(target) {
                Expression::Context => {
                    let mut val_ty = self.type_of_expression(value)?;
                    let expected_ty = context_type();
                    val_ty = self.coerce_expression_to_type(value, &val_ty, &expected_ty)?;
                    if !self.are_types_compatible(&val_ty, &expected_ty)? {
                        return Err(format!(
                            "{}: error: type mismatch assigning to '@ctx': expected {}, found {}",
                            self.current_file_path, expected_ty, val_ty
                        ));
                    }
                    if let Some(info_mut) = self.lookup_binding_mut("@ctx") {
                        info_mut.is_initialized = true;
                    }
                    Ok(Type::Symbol("none".to_string()))
                }
                Expression::Identifier(name) => {
                    let name = name.clone();
                    let mut val_ty = self.type_of_expression(value)?;

                    if self.scope.lookup_definition(&name).is_some() {
                        return Err(format!(
                            "{}: error: cannot assign to global constant '@{}'",
                            self.current_file_path, name
                        ));
                    }
                    if self.scope.lookup_extern(&name).is_some() {
                        return Err(format!(
                            "{}: error: cannot assign to external function '{}'",
                            self.current_file_path, name
                        ));
                    }
                    if self.scope.lookup_type_alias(&name).is_some() {
                        return Err(format!(
                            "{}: error: cannot assign to type alias '{}'",
                            self.current_file_path, name
                        ));
                    }

                    if let Some(info) = self.lookup_binding(&name) {
                        if !info.is_mutable {
                            return Err(format!(
                                "{}: error: cannot reassign to immutable constant '{}'",
                                self.current_file_path, name
                            ));
                        }
                        val_ty = self.coerce_expression_to_type(value, &val_ty, &info.ty)?;
                        if !self.are_types_compatible(&val_ty, &info.ty)? {
                            return Err(format!(
                                "{}: error: type mismatch assigning to '{}': expected {}, found {}",
                                self.current_file_path, name, info.ty, val_ty
                            ));
                        }

                        let may_point_to_stack = self.expr_may_point_to_stack(value);
                        if let Some(info_mut) = self.lookup_binding_mut(&name) {
                            info_mut.is_initialized = true;
                            info_mut.may_point_to_stack = may_point_to_stack;
                        }
                    } else {
                        let may_point_to_stack = self.expr_may_point_to_stack(value);
                        self.local_scopes.last_mut().unwrap().insert(
                            name.clone(),
                            BindingInfo {
                                ty: val_ty.clone(),
                                is_mutable: false,
                                is_initialized: true,
                                may_point_to_stack,
                            },
                        );
                    }
                    Ok(Type::Symbol("none".to_string()))
                }
                Expression::Dereference(ptr_expr) => {
                    let raw_ptr_ty = self.type_of_expression(ptr_expr.as_mut())?;
                    let ptr_ty = self.resolve_type(&raw_ptr_ty)?;
                    let Type::Pointer { element, is_mut } = ptr_ty else {
                        return Err(format!(
                            "{}: error: cannot dereference non-pointer type {} in assignment target",
                            self.current_file_path, ptr_ty
                        ));
                    };
                    if !is_mut {
                        return Err(format!(
                            "{}: error: cannot mutate through immutable pointer '*{}'",
                            self.current_file_path, Self::canonical_type_key(&element)
                        ));
                    }
                    if *element == Type::Opaque {
                        return Err(format!(
                            "{}: error: cannot assign through untyped pointer '*opaque'",
                            self.current_file_path
                        ));
                    }
                    let target_ty = self.resolve_type(&element)?;
                    let mut val_ty = self.type_of_expression(value)?;
                    val_ty = self.coerce_expression_to_type(value, &val_ty, &target_ty)?;
                    if !self.are_types_compatible(&val_ty, &target_ty)? {
                        return Err(format!(
                            "{}: error: type mismatch in pointer dereference assignment: expected {}, found {}",
                            self.current_file_path, target_ty, val_ty
                        ));
                    }
                    if self.type_contains_pointer(&target_ty)
                        && !self.expr_may_point_to_stack(ptr_expr.as_ref())
                        && self.expr_may_point_to_stack(value)
                    {
                        return Err(format!(
                            "{}: error: cannot store pointer to local stack variable into dereferenced pointer",
                            self.current_file_path
                        ));
                    }
                    Ok(Type::Symbol("none".to_string()))
                }
                Expression::Member { expression: base, field } => {
                    let field = field.clone();
                    let is_context_base = matches!(unspanned_expression(base.as_ref()), Expression::Context);
                    let raw_base_ty = if is_context_base {
                        context_type()
                    } else {
                        self.type_of_expression(base.as_mut())?
                    };
                    let base_ty = self.resolve_type(&raw_base_ty)?;
                    let target_ty = match &base_ty {
                        Type::Struct(flds) => {
                            let target_field = flds
                                .iter()
                                .find(|f| f.name == field)
                                .ok_or_else(|| {
                                    format!(
                                        "{}: error: struct has no field named '{}'",
                                        self.current_file_path, field
                                    )
                                })?;
                            self.resolve_type(&target_field.ty)?
                        }
                        Type::Tuple(elements) => {
                            let idx: usize = field.parse().map_err(|_| {
                                format!(
                                    "{}: error: tuple access expects unsigned integer index, found '{}'",
                                    self.current_file_path, field
                                )
                            })?;
                            if idx >= elements.len() {
                                return Err(format!(
                                    "{}: error: tuple index {} out of bounds for tuple of length {}",
                                    self.current_file_path, idx, elements.len()
                                ));
                            }
                            self.resolve_type(&elements[idx])?
                        }
                        other => {
                            return Err(format!(
                                "{}: error: cannot assign to field '{}' of non-struct type {}",
                                self.current_file_path, field, other
                            ));
                        }
                    };

                    self.check_lvalue_mutability(base.as_ref())?;

                    let mut val_ty = self.type_of_expression(value)?;
                    val_ty = self.coerce_expression_to_type(value, &val_ty, &target_ty)?;
                    if !self.are_types_compatible(&val_ty, &target_ty)? {
                        return Err(format!(
                            "{}: error: type mismatch assigning to field '{}': expected {}, found {}",
                            self.current_file_path, field, target_ty, val_ty
                        ));
                    }
                    if is_context_base {
                        if let Some(info_mut) = self.lookup_binding_mut("@ctx") {
                            info_mut.is_initialized = true;
                        }
                    }
                    Ok(Type::Symbol("none".to_string()))
                }
                _ => Err(format!(
                    "{}: error: invalid assignment target: only variables, pointer dereferences ('ptr.*'), and struct fields are supported as assignment targets",
                    self.current_file_path
                )),
            },

            Expression::Repeat {
                count,
                binder,
                body,
            } => {
                let raw_count_ty = self.type_of_expression(count)?;
                let count_ty = self.resolve_type(&raw_count_ty)?;

                if !Self::is_integer_type(&count_ty) {
                    return Err(format!(
                        "{}: error: repeat count must be an integer, found {}",
                        self.current_file_path, count_ty
                    ));
                }

                let scopes_before_loop = self.local_scopes.clone();
                if let Some(binder_name) = binder {
                    self.enter_scope();
                    self.local_scopes.last_mut().unwrap().insert(
                        binder_name.clone(),
                        BindingInfo {
                            ty: count_ty.clone(),
                            is_mutable: false,
                            is_initialized: true,
                            may_point_to_stack: false,
                        },
                    );
                }

                self.loop_depth += 1;
                let body_result = self.type_of_expression(body);
                self.loop_depth -= 1;
                self.restore_initialization_state(scopes_before_loop);
                body_result?;
                Ok(Type::Symbol("none".to_string()))
            }

            Expression::RepeatWhile { condition, body } => {
                let raw_cond_ty = self.type_of_expression(condition)?;
                let cond_ty = self.resolve_type(&raw_cond_ty)?;

                if !self.is_bool_type(&cond_ty)? {
                    return Err(format!(
                        "{}: error: repeat while condition must evaluate to Bool (:true | :false), found {}",
                        self.current_file_path, cond_ty
                    ));
                }

                let scopes_before_loop = self.local_scopes.clone();
                self.loop_depth += 1;
                let body_result = self.type_of_expression(body);
                self.loop_depth -= 1;
                self.restore_initialization_state(scopes_before_loop);
                body_result?;
                Ok(Type::Symbol("none".to_string()))
            }

            Expression::RepeatUntil { condition, body } => {
                let scopes_before_loop = self.local_scopes.clone();
                self.loop_depth += 1;
                let body_result = self.type_of_expression(body);
                self.loop_depth -= 1;
                let raw_cond_ty = self.type_of_expression(condition)?;
                let cond_ty = self.resolve_type(&raw_cond_ty)?;

                if !self.is_bool_type(&cond_ty)? {
                    return Err(format!(
                        "{}: error: repeat until condition must evaluate to Bool (:true | :false), found {}",
                        self.current_file_path, cond_ty
                    ));
                }

                self.restore_initialization_state(scopes_before_loop);
                body_result?;
                Ok(Type::Symbol("none".to_string()))
            }

            Expression::For {
                iterable,
                binder,
                body,
            } => {
                let raw_iter_ty = self.type_of_expression(iterable)?;
                let iter_ty = self.resolve_type(&raw_iter_ty)?;

                let element_ty = match &iter_ty {
                    Type::Array { element, .. } => self.resolve_type(element)?,
                    other if self.is_slice_type(other)? => {
                        let elem = self.slice_element_type(other)?;
                        self.resolve_type(&elem)?
                    }
                    other => {
                        return Err(format!(
                            "{}: error: for loop expects slice or array iterable, found {}",
                            self.current_file_path, other
                        ));
                    }
                };

                let scopes_before_loop = self.local_scopes.clone();
                if let Some(binder_name) = binder {
                    self.enter_scope();
                    self.local_scopes.last_mut().unwrap().insert(
                        binder_name.clone(),
                        BindingInfo {
                            ty: element_ty,
                            is_mutable: false,
                            is_initialized: true,
                            may_point_to_stack: false,
                        },
                    );
                }

                self.loop_depth += 1;
                let body_result = self.type_of_expression(body);
                self.loop_depth -= 1;
                self.restore_initialization_state(scopes_before_loop);
                body_result?;
                Ok(Type::Symbol("none".to_string()))
            }

            Expression::If { arms } => {
                let has_else = arms
                    .last()
                    .map(|arm| matches!(arm.condition, IfCondition::Else))
                    .unwrap_or(false);
                if !has_else {
                    arms.push(crate::syntax::IfArm {
                        condition: IfCondition::Else,
                        body: Box::new(Expression::Symbol("none".to_string())),
                    });
                }
                let num_arms = arms.len();

                let mut arm_types = Vec::new();
                let scopes_before_if = self.local_scopes.clone();
                let mut reachable_states = Vec::new();

                for (i, arm) in arms.iter_mut().enumerate() {
                    self.local_scopes = scopes_before_if.clone();
                    match &mut arm.condition {
                        IfCondition::Expression(cond) => {
                            let cond_ty = self.type_of_expression(cond)?;
                            if !self.is_bool_type(&cond_ty)? {
                                return Err(format!(
                                    "{}: error: if condition must evaluate to Bool (:true | :false), found {}",
                                    self.current_file_path, cond_ty
                                ));
                            }
                        }
                        IfCondition::Else => {
                            if i != num_arms - 1 {
                                return Err(format!(
                                    "{}: error: fallback 'else' arm must be the final arm in if",
                                    self.current_file_path
                                ));
                            }
                        }
                    }

                    let arm_ty = self.type_of_expression(&mut arm.body)?;
                    let resolved_arm_ty = self.resolve_type(&arm_ty)?;
                    arm_types.push(resolved_arm_ty.clone());

                    if resolved_arm_ty != Type::Never {
                        reachable_states.push(self.initialization_state());
                    }
                }

                self.local_scopes = scopes_before_if;
                if !reachable_states.is_empty() {
                    self.merge_initialization_states(&reachable_states);
                }

                let unified_return_ty = self.unify_to_sum_type(&arm_types)?;

                if let Type::Union(_) = unified_return_ty {
                    for (i, arm) in arms.iter_mut().enumerate() {
                        let arm_ty = &arm_types[i];
                        if arm_ty != &Type::Never && arm_ty != &unified_return_ty {
                            let orig = std::mem::replace(
                                &mut arm.body,
                                Box::new(Expression::Symbol("false".to_string())),
                            );
                            arm.body = Box::new(Expression::Cast {
                                expression: orig,
                                ty: unified_return_ty.clone(),
                            });
                        }
                    }
                }

                Ok(unified_return_ty)
            }

            Expression::Match {
                target,
                binding_name,
                arms,
            } => {
                let raw_target_ty = self.type_of_expression(target)?;
                let target_ty = self.resolve_type(&raw_target_ty)?;
                let aliases = self.alias_targets();

                if arms.is_empty() {
                    return Err(format!(
                        "{}: error: match expression must have at least one arm",
                        self.current_file_path
                    ));
                }

                if !matches!(&target_ty, Type::Union(_)) {
                    let mut matched_idx = None;
                    for (idx, arm) in arms.iter().enumerate() {
                        if let Some(raw_ty) = &arm.variant_type {
                            let resolved = self.resolve_type(raw_ty)?;
                            let matches = if (self.is_slice_type(&target_ty).unwrap_or(false)
                                || matches!(target_ty, Type::Array { .. })
                                || matches!(target_ty, Type::Tuple(_)))
                                && self.is_slice_type(&resolved).unwrap_or(false)
                            {
                                if let Ok(r_elem) = self.slice_element_type(&resolved) {
                                    if self.is_slice_type(&target_ty).unwrap_or(false) {
                                        let t_elem = self.slice_element_type(&target_ty).unwrap();
                                        Self::types_compatible_with_aliases(&t_elem, &r_elem, &aliases)
                                    } else {
                                        Self::is_array_to_slice_compatible(&target_ty, &resolved, &aliases)
                                    }
                                } else {
                                    false
                                }
                            } else if matches!(target_ty, Type::Symbol(_))
                                && self.is_string_type(&resolved).unwrap_or(false)
                            {
                                true
                            } else {
                                Self::types_compatible_with_aliases(&target_ty, &resolved, &aliases)
                                    || Self::types_compatible_with_aliases(&resolved, &target_ty, &aliases)
                            };
                            if matches {
                                matched_idx = Some(idx);
                                break;
                            }
                        } else {
                            matched_idx = Some(idx);
                            break;
                        }
                    }

                    let matched_idx = matched_idx.ok_or_else(|| {
                        format!(
                            "{}: error: no match arm for type {}",
                            self.current_file_path, target_ty
                        )
                    })?;

                    let (target_expr, b_name, matched_arm) = match std::mem::replace(
                        expression,
                        Expression::Symbol("false".to_string()),
                    ) {
                        Expression::Match {
                            target,
                            binding_name,
                            mut arms,
                        } => {
                            let arm = arms.remove(matched_idx);
                            (*target, binding_name, arm)
                        }
                        _ => unreachable!(),
                    };

                    let is_same_ident = match unspanned_expression(&target_expr) {
                        Expression::Identifier(id) => id == &b_name,
                        _ => false,
                    };

                    *expression = if b_name.is_empty() || b_name == "_" {
                        Expression::Block(vec![
                            Expression::Discard(Box::new(target_expr)),
                            *matched_arm.body,
                        ])
                    } else {
                        let bound_expr = if (matches!(target_ty, Type::Array { .. }) || matches!(target_ty, Type::Tuple(_)))
                            && matched_arm.variant_type.as_ref().map(|vt| self.is_slice_type(vt).unwrap_or(false)).unwrap_or(false)
                        {
                            let slice_expr = Expression::Slice(Box::new(target_expr));
                            if let Some(vt) = &matched_arm.variant_type {
                                Expression::Cast {
                                    expression: Box::new(slice_expr),
                                    ty: vt.clone(),
                                }
                            } else {
                                slice_expr
                            }
                        } else if matches!(target_ty, Type::Symbol(_))
                            && matched_arm.variant_type.as_ref().map(|vt| self.is_string_type(vt).unwrap_or(false)).unwrap_or(false)
                        {
                            if let Expression::Symbol(sym_name) = unspanned_expression(&target_expr) {
                                Expression::String(sym_name.clone().into_bytes())
                            } else {
                                Expression::Cast {
                                    expression: Box::new(target_expr),
                                    ty: matched_arm.variant_type.clone().unwrap(),
                                }
                            }
                        } else if let Some(vt) = &matched_arm.variant_type {
                            let resolved_vt = self.resolve_type(vt)?;
                            if target_ty != resolved_vt && Self::types_compatible_with_aliases(&target_ty, &resolved_vt, &aliases) {
                                Expression::Cast {
                                    expression: Box::new(target_expr),
                                    ty: vt.clone(),
                                }
                            } else {
                                target_expr
                            }
                        } else {
                            target_expr
                        };

                        if is_same_ident && matches!(unspanned_expression(&bound_expr), Expression::Identifier(id) if id == &b_name) {
                            *matched_arm.body
                        } else {
                            Expression::Block(vec![
                                Expression::Let {
                                    name: b_name,
                                    value: Box::new(bound_expr),
                                },
                                *matched_arm.body,
                            ])
                        }
                    };

                    return self.type_of_expression(expression);
                }

                let target_variants = match &target_ty {
                    Type::Union(vars) => vars.clone(),
                    _ => unreachable!(),
                };

                let mut unhandled = target_variants.clone();
                let mut arm_types = Vec::new();
                let mut active_arms = Vec::new();

                for mut arm in std::mem::take(arms) {
                    if let Some(raw_ty) = &arm.variant_type {
                        let resolved = self.resolve_type(raw_ty)?;

                        let covered_variants = match &resolved {
                            Type::Union(inner) => inner.clone(),
                            single => vec![single.clone()],
                        };

                        let mut matching_covered = Vec::new();
                        for v in &covered_variants {
                            let mut matched_targets = Vec::new();
                            for tv in &target_variants {
                                if Self::types_compatible_with_aliases(tv, v, &aliases)
                                    && Self::types_compatible_with_aliases(v, tv, &aliases)
                                {
                                    matched_targets.push(tv.clone());
                                }
                            }
                            if matched_targets.is_empty() {
                                for tv in &unhandled {
                                    let is_symbol_to_str = matches!(tv, Type::Symbol(_))
                                        && Self::is_string_type_with_aliases(v, &aliases);
                                    if !is_symbol_to_str
                                        && Self::types_compatible_with_aliases(tv, v, &aliases)
                                    {
                                        matched_targets.push(tv.clone());
                                    }
                                }
                            }
                            for mt in matched_targets {
                                if !matching_covered.contains(&mt) {
                                    matching_covered.push(mt.clone());
                                }
                                if let Some(pos) = unhandled.iter().position(|u| {
                                    Self::types_compatible_with_aliases(u, &mt, &aliases)
                                        && Self::types_compatible_with_aliases(&mt, u, &aliases)
                                }) {
                                    unhandled.remove(pos);
                                }
                            }
                        }

                        if matching_covered.is_empty() {
                            continue;
                        }

                        let narrowed_ty = if matching_covered.len() == 1 {
                            matching_covered[0].clone()
                        } else {
                            Type::Union(matching_covered)
                        };
                        arm.variant_type = Some(narrowed_ty.clone());

                        self.enter_scope();
                        let may_point_to_stack = self.expr_may_point_to_stack(&arm.body);
                        self.local_scopes.last_mut().unwrap().insert(
                            binding_name.clone(),
                            BindingInfo {
                                ty: narrowed_ty,
                                is_mutable: false,
                                is_initialized: true,
                                may_point_to_stack,
                            },
                        );

                        let arm_res_ty = self.type_of_expression(&mut arm.body);
                        self.exit_scope();
                        let arm_ty = self.resolve_type(&arm_res_ty?)?;
                        arm_types.push(arm_ty);
                        active_arms.push(arm);
                    } else {
                        if unhandled.is_empty() {
                            continue;
                        }

                        let narrowed_ty = if unhandled.len() == 1 {
                            unhandled[0].clone()
                        } else {
                            let mut sorted_remaining = unhandled.clone();
                            sorted_remaining.sort_by_key(Self::canonical_type_key);
                            Type::Union(sorted_remaining)
                        };

                        arm.variant_type = Some(narrowed_ty.clone());
                        unhandled.clear();

                        self.enter_scope();
                        let may_point_to_stack = self.expr_may_point_to_stack(&arm.body);
                        self.local_scopes.last_mut().unwrap().insert(
                            binding_name.clone(),
                            BindingInfo {
                                ty: narrowed_ty,
                                is_mutable: false,
                                is_initialized: true,
                                may_point_to_stack,
                            },
                        );

                        let arm_res_ty = self.type_of_expression(&mut arm.body);
                        self.exit_scope();
                        let arm_ty = self.resolve_type(&arm_res_ty?)?;
                        arm_types.push(arm_ty);
                        active_arms.push(arm);
                    }
                }

                if !unhandled.is_empty() {
                    let missing = unhandled
                        .iter()
                        .map(|t| t.to_string())
                        .collect::<Vec<_>>()
                        .join(" | ");
                    return Err(format!(
                        "{}: error: non-exhaustive match expression: missing cases for {}",
                        self.current_file_path, missing
                    ));
                }

                *arms = active_arms;

                let unified_return_ty = self.unify_to_sum_type(&arm_types)?;

                if let Type::Union(_) = unified_return_ty {
                    for (i, arm) in arms.iter_mut().enumerate() {
                        let arm_ty = &arm_types[i];
                        if arm_ty != &Type::Never && arm_ty != &unified_return_ty {
                            let orig = std::mem::replace(
                                &mut arm.body,
                                Box::new(Expression::Symbol("false".to_string())),
                            );
                            arm.body = Box::new(Expression::Cast {
                                expression: orig,
                                ty: unified_return_ty.clone(),
                            });
                        }
                    }
                }

                Ok(unified_return_ty)
            }

            Expression::Array(elements) => {
                if elements.iter().any(|e| matches!(crate::syntax::unspanned_expression(e), Expression::Spread(_))) {
                    let mut expanded = Vec::new();
                    for el in elements.drain(..) {
                        let is_spread = matches!(crate::syntax::unspanned_expression(&el), Expression::Spread(_));
                        if is_spread {
                            let mut inner = match el {
                                Expression::Spread(inner) => *inner,
                                Expression::Spanned { expression, .. } => match *expression {
                                    Expression::Spread(inner) => *inner,
                                    _ => unreachable!(),
                                },
                                _ => unreachable!(),
                            };
                            let inner_ty = self.type_of_expression(&mut inner)?;
                            let resolved = self.resolve_type(&inner_ty)?;
                            match resolved {
                                Type::Array { size, .. } => {
                                    for i in 0..size {
                                        expanded.push(Expression::Index {
                                            expression: Box::new(inner.clone()),
                                            index: Box::new(Expression::Integer(i as i64)),
                                        });
                                    }
                                }
                                Type::Tuple(tys) => {
                                    for i in 0..tys.len() {
                                        expanded.push(Expression::Member {
                                            expression: Box::new(inner.clone()),
                                            field: i.to_string(),
                                        });
                                    }
                                }
                                _ => return Err(format!("{}: error: spread operator in array requires fixed-size array or tuple, found {}", self.current_file_path, resolved)),
                            }
                        } else {
                            expanded.push(el);
                        }
                    }
                    *elements = expanded;
                }

                if elements.is_empty() {
                    return Ok(Type::Array {
                        element: Box::new(Type::U8),
                        size: 0,
                    });
                }

                let mut elem_types = Vec::new();
                for element in elements.iter_mut() {
                    let t = self.type_of_expression(element)?;
                    elem_types.push(self.resolve_type(&t)?);
                }

                let unified_elem_ty = self.unify_to_sum_type(&elem_types)?;

                if let Type::Union(_) = unified_elem_ty {
                    for (i, element) in elements.iter_mut().enumerate() {
                        let el_ty = &elem_types[i];
                        if el_ty != &unified_elem_ty {
                            let orig = std::mem::replace(element, Expression::Symbol("false".to_string()));
                            *element = Expression::Cast {
                                expression: Box::new(orig),
                                ty: unified_elem_ty.clone(),
                            };
                        }
                    }
                }

                Ok(Type::Array {
                    element: Box::new(unified_elem_ty),
                    size: elements.len(),
                })
            }

            Expression::Binary {
                left,
                operator,
                right,
            } => {
                let raw_left = self.type_of_expression(left)?;
                let raw_right = self.type_of_expression(right)?;
                let mut left_type = self.resolve_type(&raw_left)?;
                let mut right_type = self.resolve_type(&raw_right)?;

                if !Self::is_integer_type(&left_type) || !Self::is_integer_type(&right_type) {
                    return Err(format!(
                        "{}: error: binary operator {} requires integer operands",
                        self.current_file_path, operator
                    ));
                }

                if left_type != right_type {
                    self.coerce_binary_operands_unambiguous(
                        left.as_mut(),
                        &mut left_type,
                        right.as_mut(),
                        &mut right_type,
                    );
                }

                if left_type != right_type {
                    return Err(format!(
                        "{}: error: binary operator {} requires matching types, found {} and {}",
                        self.current_file_path, operator, left_type, right_type
                    ));
                }

                Ok(left_type)
            }

            Expression::Comparison {
                left,
                operator,
                right,
            } => {
                let raw_left = self.type_of_expression(left)?;
                let raw_right = self.type_of_expression(right)?;
                let mut left_type = self.resolve_type(&raw_left)?;
                let mut right_type = self.resolve_type(&raw_right)?;
                let bool_type = Self::bool_type();

                if (self.is_bool_type(&left_type)? || self.is_bool_type(&right_type)?)
                    && (matches!(left_type, Type::Symbol(_))
                        || matches!(right_type, Type::Symbol(_))
                        || self.is_bool_type(&left_type)?
                        || self.is_bool_type(&right_type)?)
                {
                    if !matches!(
                        operator,
                        ComparisonOperator::Equal | ComparisonOperator::NotEqual
                    ) {
                        return Err(format!(
                            "{}: error: booleans and symbols only support equality comparisons ('=' and '!=')",
                            self.current_file_path
                        ));
                    }
                    if left_type != bool_type && self.is_bool_type(&left_type)? {
                        let orig = std::mem::replace(left.as_mut(), Expression::Symbol("false".to_string()));
                        *left = Box::new(Expression::Cast {
                            expression: Box::new(orig),
                            ty: bool_type.clone(),
                        });
                        left_type = bool_type.clone();
                    }
                    if right_type != bool_type && self.is_bool_type(&right_type)? {
                        let orig = std::mem::replace(right.as_mut(), Expression::Symbol("false".to_string()));
                        *right = Box::new(Expression::Cast {
                            expression: Box::new(orig),
                            ty: bool_type.clone(),
                        });
                        right_type = bool_type.clone();
                    }
                    if left_type == right_type {
                        return Ok(bool_type);
                    }
                }

                if matches!(left_type, Type::Symbol(_)) || matches!(right_type, Type::Symbol(_)) {
                    if !matches!(
                        operator,
                        ComparisonOperator::Equal | ComparisonOperator::NotEqual
                    ) {
                        return Err(format!(
                            "{}: error: symbols only support equality comparisons ('=' and '!=')",
                            self.current_file_path
                        ));
                    }
                    if left_type != right_type {
                        return Err(format!(
                            "{}: error: cannot compare distinct symbol types: {} and {}",
                            self.current_file_path, left_type, right_type
                        ));
                    }
                    return Ok(bool_type);
                }

                if left_type != right_type {
                    self.coerce_binary_operands_unambiguous(
                        left.as_mut(),
                        &mut left_type,
                        right.as_mut(),
                        &mut right_type,
                    );
                }

                if left_type != right_type {
                    return Err(format!(
                        "{}: error: comparison operator requires matching types, found {} and {}",
                        self.current_file_path, left_type, right_type
                    ));
                }

                Ok(bool_type)
            }

            Expression::Cast { .. } => {
                let (source_type, resolved_target, desugar_info) = {
                    let Expression::Cast {
                        expression: inner_expr,
                        ty,
                    } = expression
                    else {
                        unreachable!()
                    };
                    let raw_source = self.type_of_expression(inner_expr)?;
                    let source_type = self.resolve_type(&raw_source)?;
                    self.validate_user_type(ty)?;
                    let resolved_target = self.resolve_type(ty)?;
                    *ty = resolved_target.clone();

                    let desugar_info = if let Type::Union(source_variants) = &source_type {
                        if &source_type != &resolved_target {
                            let target_members: Vec<Type> = match &resolved_target {
                                Type::Union(members) => members.clone(),
                                scalar => vec![scalar.clone()],
                            };
                            let aliases = self.alias_targets();
                            let mut arm_mappings = Vec::with_capacity(source_variants.len());
                            for s_v in source_variants {
                                if let Some(target_member) = Self::find_target_member_for_downcast(
                                    s_v,
                                    &target_members,
                                    &aliases,
                                ) {
                                    arm_mappings.push((s_v.clone(), target_member));
                                } else {
                                    return Err(format!(
                                        "{}: error: unsupported cast from {} to {}",
                                        self.current_file_path, source_type, resolved_target
                                    ));
                                }
                            }
                            Some(arm_mappings)
                        } else {
                            None
                        }
                    } else {
                        None
                    };

                    (source_type, resolved_target, desugar_info)
                };

                if let Some(arm_mappings) = desugar_info {
                    self.counter += 1;
                    let binding_name = format!("__z{}", self.counter);
                    let mut arms = Vec::with_capacity(arm_mappings.len());
                    for (s_v, t_m) in arm_mappings {
                        let body = if t_m == resolved_target {
                            Box::new(Expression::Cast {
                                expression: Box::new(Expression::Identifier(binding_name.clone())),
                                ty: resolved_target.clone(),
                            })
                        } else {
                            Box::new(Expression::Cast {
                                expression: Box::new(Expression::Cast {
                                    expression: Box::new(Expression::Identifier(
                                        binding_name.clone(),
                                    )),
                                    ty: t_m,
                                }),
                                ty: resolved_target.clone(),
                            })
                        };
                        arms.push(MatchArm {
                            variant_type: Some(s_v),
                            body,
                        });
                    }

                    let orig_inner = match std::mem::replace(expression, Expression::Symbol("false".to_string()))
                    {
                        Expression::Cast { expression: inner, .. } => inner,
                        _ => unreachable!(),
                    };

                    *expression = Expression::Match {
                        target: orig_inner,
                        binding_name,
                        arms,
                    };

                    return self.type_of_expression(expression);
                }

                if let Type::Union(target_vars) = &resolved_target {
                    if Self::castable_union_variant_with_aliases(&source_type, target_vars, &self.alias_targets()).is_none() {
                        return Err(format!(
                            "{}: error: cannot uniquely promote {} into union {}",
                            self.current_file_path, source_type, resolved_target
                        ));
                    }
                } else if !Self::is_supported_cast_with_aliases(
                    &source_type,
                    &resolved_target,
                    &self.alias_targets(),
                ) {
                    return Err(format!(
                        "{}: error: unsupported cast from {} to {}",
                        self.current_file_path, source_type, resolved_target
                    ));
                }
                Ok(resolved_target)
            }

            Expression::Call { target, arguments } => {
                if let Expression::Member { expression: receiver, field } = target.as_mut() {
                    let is_fn_in_scope = crate::syntax::is_intrinsic_function(field)
                        || self.scope.definitions.contains_key(field)
                        || self.scope.lookup_extern(field).is_some()
                        || self.resolved_definitions.contains_key(field);

                    let has_field = if let Ok(raw_recv_ty) = self.type_of_expression(receiver) {
                        if let Ok(res_recv_ty) = self.resolve_type(&raw_recv_ty) {
                            match res_recv_ty {
                                Type::Struct(flds) => flds.iter().any(|f| f.name == *field),
                                Type::Pointer { element, .. } => {
                                    if let Ok(elem_ty) = self.resolve_type(&element) {
                                        if let Type::Struct(flds) = elem_ty {
                                            flds.iter().any(|f| f.name == *field)
                                        } else {
                                            false
                                        }
                                    } else {
                                        false
                                    }
                                }
                                Type::Tuple(elements) => {
                                    if let Ok(idx) = field.parse::<usize>() {
                                        idx < elements.len()
                                    } else {
                                        false
                                    }
                                }
                                _ => false,
                            }
                        } else {
                            false
                        }
                    } else {
                        false
                    };

                    if is_fn_in_scope && !has_field {
                        // UFCS fallback: receiver.method(args...) -> method(receiver, args...)
                        let field_name = field.clone();
                        let recv = *receiver.clone();
                        let mut new_args = vec![recv];
                        new_args.append(arguments);
                        *expression = Expression::Call {
                            target: Box::new(Expression::Identifier(field_name)),
                            arguments: new_args,
                        };
                        return self.type_of_expression(expression);
                    }
                }

                if arguments.iter().any(|e| matches!(crate::syntax::unspanned_expression(e), Expression::Spread(_))) {
                    let mut expanded = Vec::new();
                    for el in arguments.drain(..) {
                        let is_spread = matches!(crate::syntax::unspanned_expression(&el), Expression::Spread(_));
                        if is_spread {
                            let mut inner = match el {
                                Expression::Spread(inner) => *inner,
                                Expression::Spanned { expression, .. } => match *expression {
                                    Expression::Spread(inner) => *inner,
                                    _ => unreachable!(),
                                },
                                _ => unreachable!(),
                            };
                            let inner_ty = self.type_of_expression(&mut inner)?;
                            let resolved = self.resolve_type(&inner_ty)?;
                            match resolved {
                                Type::Array { size, .. } => {
                                    for i in 0..size {
                                        expanded.push(Expression::Index {
                                            expression: Box::new(inner.clone()),
                                            index: Box::new(Expression::Integer(i as i64)),
                                        });
                                    }
                                }
                                Type::Tuple(tys) => {
                                    for i in 0..tys.len() {
                                        expanded.push(Expression::Member {
                                            expression: Box::new(inner.clone()),
                                            field: i.to_string(),
                                        });
                                    }
                                }
                                _ => return Err(format!("{}: error: spread operator in function arguments requires fixed-size array or tuple, found {}", self.current_file_path, resolved)),
                            }
                        } else {
                            expanded.push(el);
                        }
                    }
                    *arguments = expanded;
                }

                if let Expression::Identifier(name) = crate::syntax::unspanned_expression(target) {
                    if name == "@asTypeOf" && arguments.len() == 2 {
                        let target_expr = arguments.pop().unwrap();
                        let src_expr = arguments.pop().unwrap();
                        *expression = Expression::AsTypeOf {
                            expression: Box::new(src_expr),
                            target: Box::new(target_expr),
                        };
                        return self.type_of_expression(expression);
                    }
                    if name == "@sizeOf" && arguments.len() == 1 {
                        let src_expr = arguments.pop().unwrap();
                        *expression = Expression::SizeOf(Box::new(src_expr));
                        return self.type_of_expression(expression);
                    }
                }

                let is_calling_def = if let Expression::Identifier(fn_name) = crate::syntax::unspanned_expression(target) {
                    if let Some(def) = self.scope.lookup_definition(fn_name) {
                        def.is_def || def.is_specialized_def
                    } else {
                        false
                    }
                } else {
                    false
                };
                if is_calling_def {
                    if let Some(info) = self.lookup_binding("@ctx") {
                        if !info.is_initialized {
                            return Err(format!(
                                "{}: error: '@ctx' must be initialized before calling 'def' function",
                                self.current_file_path
                            ));
                        }
                    }
                }

                let raw_target_ty = self.type_of_expression(target)?;
                let target_type = self.resolve_type(&raw_target_ty)?;

                let (param_types, return_type, is_variadic) = match &target_type {
                    Type::Function {
                        parameters,
                        return_type,
                        is_variadic,
                    } => (parameters.clone(), *return_type.clone(), *is_variadic),
                    _ => {
                        return Err(format!(
                            "{}: error: cannot call non-function type {}",
                            self.current_file_path, target_type
                        ));
                    }
                };

                if is_variadic {
                    if arguments.len() < param_types.len() {
                        return Err(format!(
                            "{}: error: variadic call expects at least {} arguments, found {}",
                            self.current_file_path,
                            param_types.len(),
                            arguments.len()
                        ));
                    }
                } else if arguments.len() != param_types.len() {
                    return Err(format!(
                        "{}: error: call expects {} arguments, found {}",
                        self.current_file_path,
                        param_types.len(),
                        arguments.len()
                    ));
                }

                for (arg, expected_ty) in arguments.iter_mut().zip(param_types.iter()) {
                    let res_expected = self.resolve_type(expected_ty)?;
                    let mut arg_ty = self.type_of_expression(arg)?;
                    arg_ty = self.coerce_expression_to_type(arg, &arg_ty, &res_expected)?;

                    if !self.are_types_compatible(&arg_ty, &res_expected)? {
                        return Err(format!(
                            "{}: error: argument type mismatch: expected {}, found {}",
                            self.current_file_path, res_expected, arg_ty
                        ));
                    }
                }

                for arg in arguments.iter_mut().skip(param_types.len()) {
                    let _ = self.type_of_expression(arg)?;
                }

                Ok(return_type)
            }

            Expression::Index { expression, index } => {
                let raw_expr_ty = self.type_of_expression(expression)?;
                let expression_type = self.resolve_type(&raw_expr_ty)?;
                let raw_idx_ty = self.type_of_expression(index)?;
                let index_type = self.resolve_type(&raw_idx_ty)?;

                if !Self::is_integer_type(&index_type) {
                    return Err(format!(
                        "{}: error: array index must be an integer",
                        self.current_file_path
                    ));
                }

                match expression_type {
                    Type::Array { element, .. } => Ok(*element),
                    Type::Pointer { element, .. } => Ok(*element),
                    ref other if self.is_slice_type(other)? => {
                        let elem = self.slice_element_type(other)?;
                        Ok(elem)
                    }
                    other => Err(format!(
                        "{}: error: cannot index value of type {}",
                        self.current_file_path, other
                    )),
                }
            }

            Expression::Dereference(expression) => {
                let raw_pointer_type = self.type_of_expression(expression)?;
                match self.resolve_type(&raw_pointer_type)? {
                    Type::Pointer { element, .. } => {
                        if *element == Type::Opaque {
                            return Err(format!(
                                "{}: error: cannot dereference untyped pointer '*opaque'",
                                self.current_file_path
                            ));
                        }
                        Ok(*element)
                    }
                    Type::Function { .. } => Ok(raw_pointer_type),
                    other => Err(format!(
                        "{}: error: cannot dereference non-pointer type {}",
                        self.current_file_path, other
                    )),
                }
            }

            Expression::Member { expression, field } => {
                let raw_expr_type = self.type_of_expression(expression)?;
                let expr_type = self.resolve_type(&raw_expr_type)?;

                if field == "@sizeOf" {
                    let has_field = match &expr_type {
                        Type::Struct(fields) => fields.iter().any(|f| f.name == *field),
                        Type::Pointer { element, .. } => {
                            if let Ok(elem_ty) = self.resolve_type(element) {
                                if let Type::Struct(fields) = elem_ty {
                                    fields.iter().any(|f| f.name == *field)
                                } else {
                                    false
                                }
                            } else {
                                false
                            }
                        }
                        _ => false,
                    };
                    if !has_field {
                        return Ok(Type::USize);
                    }
                }

                match expr_type {
                    Type::Struct(fields) => {
                        if let Some(f) = fields.iter().find(|f| f.name == *field) {
                            Ok(f.ty.clone())
                        } else {
                            Err(format!(
                                "{}: error: struct has no field named '{}'",
                                self.current_file_path, field
                            ))
                        }
                    }
                    Type::Tuple(elements) => {
                        let idx: usize = field.parse().map_err(|_| {
                            format!(
                                "{}: error: tuple access expects unsigned integer index, found '{}'",
                                self.current_file_path, field
                            )
                        })?;
                        if idx >= elements.len() {
                            return Err(format!(
                                "{}: error: tuple index {} out of bounds for tuple of length {}",
                                self.current_file_path, idx, elements.len()
                            ));
                        }
                        Ok(elements[idx].clone())
                    }
                    Type::Array { element, .. } => match field.as_str() {
                        "ptr" => Ok(Type::Pointer {
                            element,
                            is_mut: false,
                        }),
                        "len" => Ok(Type::USize),
                        _ => Err(format!(
                            "{}: error: unknown array member '.{}'",
                            self.current_file_path, field
                        )),
                    },
                    other => Err(format!(
                        "{}: error: member access '.{}' unsupported on type {}",
                        self.current_file_path, field, other
                    )),
                }
            }

            Expression::AddressOf { expression, is_mut } => {
                if *is_mut {
                    self.check_lvalue_mutability(expression.as_ref())?;
                }
                let target_ty = match expression.as_mut() {
                    Expression::Index { .. } => {
                        self.type_of_expression(expression)?
                    }
                    Expression::Dereference(pointer_expression) => {
                        let pointer_type = self.type_of_expression(pointer_expression)?;
                        match self.resolve_type(&pointer_type)? {
                            Type::Pointer { element, .. } if *element == Type::Opaque => {
                                return Err(format!(
                                    "{}: error: cannot dereference untyped pointer '*opaque'",
                                    self.current_file_path
                                ));
                            }
                            Type::Pointer { element, .. } => *element,
                            other => {
                                return Err(format!(
                                    "{}: error: cannot take address of dereference of {}",
                                    self.current_file_path, other
                                ));
                            }
                        }
                    }
                    Expression::Identifier(name) => {
                        if let Some(info) = self.lookup_binding(name) {
                            info.ty
                        } else if let Some(ty) = self.resolved_definitions.get(name) {
                            return Ok(ty.clone());
                        } else if self.scope.definitions.contains_key(name) {
                            let def_ty = self.resolve_definition(name)?;
                            return Ok(def_ty);
                        } else if let Some(ext) = self.scope.lookup_extern(name) {
                            let param_types = ext
                                .signature
                                .parameters
                                .iter()
                                .map(|p| self.resolve_type(&p.ty))
                                .collect::<Result<Vec<_>, _>>()?;
                            let ret_ty = self.resolve_type(ext.signature.return_type.as_ref())?;
                            return Ok(Type::Function {
                                parameters: param_types,
                                return_type: Box::new(ret_ty),
                                is_variadic: ext.signature.is_variadic,
                            });
                        } else {
                            return Err(format!(
                                "{}: error: cannot take address of unknown variable or function '{}'",
                                self.current_file_path, name
                            ));
                        }
                    }
                    Expression::Member { .. } => {
                        self.type_of_expression(expression)?
                    }
                    Expression::Context => {
                        self.type_of_expression(expression)?
                    }
                    _ => {
                        return Err(format!(
                            "{}: error: address-of requires an lvalue (variable, struct member, array index, or dereference)",
                            self.current_file_path
                        ));
                    }
                };
                Ok(Type::Pointer {
                    element: Box::new(target_ty),
                    is_mut: *is_mut,
                })
            }

            Expression::SizeOf(inner) => {
                let _ = self.type_of_expression(inner)?;
                Ok(Type::USize)
            }

            Expression::AsTypeOf { expression: inner, target } => {
                let source_type = self.type_of_expression(inner)?;
                let target_type = self.type_of_expression(target)?;
                let resolved_target = self.resolve_type(&target_type)?;

                let coerced_source = self.coerce_expression_to_type(inner, &source_type, &resolved_target)?;
                if !Self::is_supported_cast_with_aliases(&coerced_source, &resolved_target, &self.alias_targets()) {
                    return Err(format!(
                        "{}: error: unsupported cast from {} to {}",
                        self.current_file_path, coerced_source, resolved_target
                    ));
                }

                *expression = Expression::Cast {
                    expression: inner.clone(),
                    ty: resolved_target.clone(),
                };
                self.type_of_expression(expression)
            }

            Expression::Slice(expr) => {
                let ty = self.type_of_expression(expr)?;
                let resolved = self.resolve_type(&ty)?;
                match resolved {
                    Type::Array { element, .. } => {
                        Ok(Type::Struct(vec![
                            StructFieldType {
                                name: "ptr".to_string(),
                                ty: Type::Pointer {
                                    element,
                                    is_mut: false,
                                },
                            },
                            StructFieldType {
                                name: "len".to_string(),
                                ty: Type::USize,
                            },
                        ]))
                    }
                    Type::Tuple(elements) => {
                        if elements.is_empty() {
                            return Err(format!(
                                "{}: error: slice cannot be created from an empty tuple",
                                self.current_file_path
                            ));
                        }
                        let first = &elements[0];
                        for elem in &elements[1..] {
                            if !self.are_types_compatible(elem, first)? {
                                return Err(format!(
                                    "{}: error: slice on tuple requires all elements to have compatible types, expected {}, found {}",
                                    self.current_file_path, first, elem
                                ));
                            }
                        }
                        Ok(Type::Struct(vec![
                            StructFieldType {
                                name: "ptr".to_string(),
                                ty: Type::Pointer {
                                    element: Box::new(first.clone()),
                                    is_mut: false,
                                },
                            },
                            StructFieldType {
                                name: "len".to_string(),
                                ty: Type::USize,
                            },
                        ]))
                    }
                    other => Err(format!(
                        "{}: error: slice requires an array or tuple, found {}",
                        self.current_file_path, other
                    )),
                }
            }
        }
    }

    pub fn are_types_compatible(&self, provided: &Type, expected: &Type) -> Result<bool, String> {
        let res_prov = self.resolve_type(provided)?;
        let res_exp = self.resolve_type(expected)?;
        Ok(Self::types_compatible_with_aliases(
            &res_prov,
            &res_exp,
            &self.alias_targets(),
        ))
    }

    pub fn types_compatible(provided: &Type, expected: &Type) -> bool {
        Self::types_compatible_with_aliases(provided, expected, &HashMap::new())
    }

    fn alias_targets(&self) -> HashMap<String, Type> {
        let mut map: HashMap<String, Type> = self.scope
            .type_aliases
            .iter()
            .map(|(name, provenance)| (name.clone(), provenance.item.target.clone()))
            .collect();
        for (name, provenance) in &self.scope.home_type_aliases {
            map.entry(name.clone()).or_insert_with(|| provenance.item.target.clone());
        }
        map
    }

    pub fn types_equivalent_with_aliases(
        t1: &Type,
        t2: &Type,
        aliases: &HashMap<String, Type>,
    ) -> bool {
        fn equivalent(
            t1: &Type,
            t2: &Type,
            aliases: &HashMap<String, Type>,
            visited: &mut HashSet<(Type, Type)>,
        ) -> bool {
            if t1 == t2 {
                return true;
            }
            if t1 == &Type::Inferred || t2 == &Type::Inferred {
                return true;
            }
            if t1 == &Type::Never || t2 == &Type::Never {
                return false;
            }
            let pair = (t1.clone(), t2.clone());
            if !visited.insert(pair) {
                return true;
            }

            match (t1, t2) {
                (Type::Alias(left), Type::Alias(right)) => {
                    if left == right {
                        true
                    } else {
                        match (aliases.get(left), aliases.get(right)) {
                            (Some(l_ty), Some(r_ty)) => equivalent(l_ty, r_ty, aliases, visited),
                            (Some(l_ty), None) => equivalent(l_ty, t2, aliases, visited),
                            (None, Some(r_ty)) => equivalent(t1, r_ty, aliases, visited),
                            (None, None) => false,
                        }
                    }
                }
                (Type::Alias(name), other) => {
                    aliases.get(name).is_some_and(|target| equivalent(target, other, aliases, visited))
                }
                (other, Type::Alias(name)) => {
                    aliases.get(name).is_some_and(|target| equivalent(other, target, aliases, visited))
                }
                (Type::Pointer { element: e1, is_mut: m1 }, Type::Pointer { element: e2, is_mut: m2 }) => {
                    m1 == m2 && equivalent(e1, e2, aliases, visited)
                }
                (Type::Array { element: e1, size: s1 }, Type::Array { element: e2, size: s2 }) => {
                    s1 == s2 && equivalent(e1, e2, aliases, visited)
                }
                (Type::Struct(f1), Type::Struct(f2)) => {
                    f1.len() == f2.len()
                        && f1.iter().zip(f2.iter()).all(|(a, b)| a.name == b.name && equivalent(&a.ty, &b.ty, aliases, visited))
                }
                (Type::Tuple(elems1), Type::Tuple(elems2)) => {
                    elems1.len() == elems2.len()
                        && elems1.iter().zip(elems2.iter()).all(|(a, b)| equivalent(a, b, aliases, visited))
                }
                (Type::Function { parameters: p1, return_type: r1, is_variadic: v1 },
                 Type::Function { parameters: p2, return_type: r2, is_variadic: v2 }) => {
                    v1 == v2
                        && p1.len() == p2.len()
                        && p1.iter().zip(p2.iter()).all(|(a, b)| equivalent(a, b, aliases, visited))
                        && equivalent(r1, r2, aliases, visited)
                }
                (Type::Union(u1), Type::Union(u2)) => {
                    u1.len() == u2.len()
                        && u1.iter().all(|v1| u2.iter().any(|v2| equivalent(v1, v2, aliases, visited)))
                        && u2.iter().all(|v2| u1.iter().any(|v1| equivalent(v1, v2, aliases, visited)))
                }
                _ => false,
            }
        }

        let mut visited = HashSet::new();
        equivalent(t1, t2, aliases, &mut visited)
    }

    pub fn resolve_alias_root<'b>(mut ty: &'b Type, aliases: &'b HashMap<String, Type>) -> &'b Type {
        let mut visited = HashSet::new();
        while let Type::Alias(name) = ty {
            if !visited.insert(name.as_str()) {
                break;
            }
            if let Some(target) = aliases.get(name) {
                ty = target;
            } else {
                break;
            }
        }
        ty
    }

    pub fn is_slice_type_with_aliases<'b>(ty: &'b Type, aliases: &'b HashMap<String, Type>) -> bool {
        let resolved = Self::resolve_alias_root(ty, aliases);
        match resolved {
            Type::GenericInstance { name, type_arguments } if name == "Slice" && type_arguments.len() == 1 => true,
            Type::Struct(fields) => {
                if fields.len() == 2 && fields[0].name == "ptr" && fields[1].name == "len" {
                    let f0 = Self::resolve_alias_root(&fields[0].ty, aliases);
                    let f1 = Self::resolve_alias_root(&fields[1].ty, aliases);
                    if matches!(f0, Type::Pointer { .. }) && matches!(f1, Type::USize) {
                        return true;
                    }
                }
                false
            }
            _ => false,
        }
    }

    pub fn slice_element_type_with_aliases<'b>(
        ty: &'b Type,
        aliases: &'b HashMap<String, Type>,
    ) -> Option<&'b Type> {
        let resolved = Self::resolve_alias_root(ty, aliases);
        match resolved {
            Type::GenericInstance { name, type_arguments } if name == "Slice" && type_arguments.len() == 1 => {
                Some(&type_arguments[0])
            }
            Type::Struct(fields) => {
                if fields.len() == 2 && fields[0].name == "ptr" {
                    let f0 = Self::resolve_alias_root(&fields[0].ty, aliases);
                    if let Type::Pointer { element, .. } = f0 {
                        return Some(element.as_ref());
                    }
                }
                None
            }
            _ => None,
        }
    }

    pub fn is_string_type_with_aliases<'b>(ty: &'b Type, aliases: &'b HashMap<String, Type>) -> bool {
        if Self::is_slice_type_with_aliases(ty, aliases) {
            if let Some(elem) = Self::slice_element_type_with_aliases(ty, aliases) {
                return *Self::resolve_alias_root(elem, aliases) == Type::U8;
            }
        }
        false
    }

    pub fn is_array_to_slice_compatible<'b>(
        provided: &'b Type,
        expected: &'b Type,
        aliases: &'b HashMap<String, Type>,
    ) -> bool {
        if !Self::is_slice_type_with_aliases(expected, aliases) {
            return false;
        }
        let target_elem = match Self::slice_element_type_with_aliases(expected, aliases) {
            Some(elem) => elem,
            None => return false,
        };
        let resolved_provided = Self::resolve_alias_root(provided, aliases);
        match resolved_provided {
            Type::Array { element, .. } => {
                Self::types_compatible_with_aliases(element, target_elem, aliases)
            }
            Type::Tuple(elems) => {
                if elems.is_empty() {
                    return false;
                }
                elems.iter().all(|elem| {
                    Self::types_compatible_with_aliases(elem, target_elem, aliases)
                })
            }
            _ => false,
        }
    }

    pub fn is_array_to_slice_castable<'b>(
        provided: &'b Type,
        expected: &'b Type,
        aliases: &'b HashMap<String, Type>,
    ) -> bool {
        if !Self::is_slice_type_with_aliases(expected, aliases) {
            return false;
        }
        let target_elem = match Self::slice_element_type_with_aliases(expected, aliases) {
            Some(elem) => elem,
            None => return false,
        };
        let resolved_provided = Self::resolve_alias_root(provided, aliases);
        match resolved_provided {
            Type::Array { element, .. } => {
                Self::is_supported_cast_with_aliases(element, target_elem, aliases)
            }
            Type::Tuple(elems) => {
                if elems.is_empty() {
                    return false;
                }
                elems.iter().all(|elem| {
                    Self::is_supported_cast_with_aliases(elem, target_elem, aliases)
                })
            }
            _ => false,
        }
    }

    pub fn is_symbol_to_string_compatible<'b>(
        provided: &'b Type,
        expected: &'b Type,
        aliases: &'b HashMap<String, Type>,
    ) -> bool {
        let resolved_provided = Self::resolve_alias_root(provided, aliases);
        if !matches!(resolved_provided, Type::Symbol(_)) {
            return false;
        }
        Self::is_string_type_with_aliases(expected, aliases)
    }

    pub fn is_record_type_with_aliases<'b>(ty: &'b Type, aliases: &'b HashMap<String, Type>) -> bool {
        if let Type::Alias(name) = ty {
            if name == "Record" {
                return true;
            }
        }
        let resolved = Self::resolve_alias_root(ty, aliases);
        if let Type::Alias(name) = resolved {
            if name == "Record" {
                return true;
            }
        }
        if let Type::Pointer { element, .. } = resolved {
            let elem_resolved = Self::resolve_alias_root(element, aliases);
            if let Type::Struct(fields) = elem_resolved {
                if fields.len() == 3
                    && fields[0].name == "len"
                    && fields[1].name == "key"
                    && fields[2].name == "val"
                {
                    return true;
                }
            }
        }
        false
    }

    pub fn is_struct_to_record_castable<'b>(
        source: &'b Type,
        target: &'b Type,
        aliases: &'b HashMap<String, Type>,
    ) -> bool {
        if !Self::is_record_type_with_aliases(target, aliases) {
            return false;
        }
        let resolved_source = Self::resolve_alias_root(source, aliases);
        if let Type::Struct(fields) = resolved_source {
            let generic_ty = Self::generic_type();
            fields.iter().all(|f| {
                Self::is_supported_cast_with_aliases(&f.ty, &generic_ty, aliases)
            })
        } else {
            false
        }
    }

    pub fn is_struct_to_record_compatible<'b>(
        provided: &'b Type,
        expected: &'b Type,
        aliases: &'b HashMap<String, Type>,
    ) -> bool {
        Self::is_struct_to_record_castable(provided, expected, aliases)
    }

    pub fn types_compatible_with_aliases(
        provided: &Type,
        expected: &Type,
        aliases: &HashMap<String, Type>,
    ) -> bool {
        fn compatible(
            provided: &Type,
            expected: &Type,
            aliases: &HashMap<String, Type>,
            visited: &mut HashSet<(Type, Type)>,
        ) -> bool {
            if provided == expected {
                return true;
            }

            if provided == &Type::Inferred || expected == &Type::Inferred {
                return true;
            }

            if provided == &Type::Never {
                return true;
            }

            if matches!(provided, Type::Alias(_)) || matches!(expected, Type::Alias(_)) {
                let pair = (provided.clone(), expected.clone());
                if !visited.insert(pair.clone()) {
                    return true;
                }
                let result = match (provided, expected) {
                    (Type::Alias(left), Type::Alias(right)) => {
                        if left == right {
                            true
                        } else {
                            match (aliases.get(left), aliases.get(right)) {
                                (Some(left_type), Some(right_type)) => {
                                    compatible(left_type, right_type, aliases, visited)
                                }
                                (Some(left_type), None) => {
                                    compatible(left_type, expected, aliases, visited)
                                }
                                (None, Some(right_type)) => {
                                    compatible(provided, right_type, aliases, visited)
                                }
                                (None, None) => false,
                            }
                        }
                    }
                    (Type::Alias(name), other) => aliases
                        .get(name)
                        .is_some_and(|alias_type| compatible(alias_type, other, aliases, visited)),
                    (other, Type::Alias(name)) => aliases
                        .get(name)
                        .is_some_and(|alias_type| compatible(other, alias_type, aliases, visited)),
                    _ => unreachable!(),
                };
                visited.remove(&pair);
                return result;
            }

            if let Type::Union(_) = provided {
                if !matches!(expected, Type::Union(_)) {
                    return false;
                }
            }

            if let Type::Union(expected_vars) = expected {
                let provided_vars = match provided {
                    Type::Union(variants) => variants.as_slice(),
                    variant => std::slice::from_ref(variant),
                };
                return provided_vars.iter().all(|variant| {
                    let exact_count = expected_vars
                        .iter()
                        .filter(|target| Checker::types_equivalent_with_aliases(variant, target, aliases))
                        .count();
                    if exact_count == 1 {
                        return true;
                    }
                    expected_vars
                        .iter()
                        .filter(|target| compatible(variant, target, aliases, visited))
                        .count()
                        == 1
                });
            }

            if Checker::is_slice_type_with_aliases(provided, aliases)
                && Checker::is_slice_type_with_aliases(expected, aliases)
            {
                if let (Some(e1), Some(e2)) = (
                    Checker::slice_element_type_with_aliases(provided, aliases),
                    Checker::slice_element_type_with_aliases(expected, aliases),
                ) {
                    if compatible(e1, e2, aliases, visited) {
                        return true;
                    }
                }
            }

            if Checker::is_array_to_slice_compatible(provided, expected, aliases) {
                return true;
            }

            if Checker::is_symbol_to_string_compatible(provided, expected, aliases) {
                return true;
            }

            if Checker::is_struct_to_record_compatible(provided, expected, aliases) {
                return true;
            }

            match (provided, expected) {
                (Type::Struct(fields1), Type::Struct(fields2)) => {
                    if fields1.len() != fields2.len() {
                        return false;
                    }
                    for (f1, f2) in fields1.iter().zip(fields2.iter()) {
                        if f1.name != f2.name || !compatible(&f1.ty, &f2.ty, aliases, visited) {
                            return false;
                        }
                    }
                    true
                }
                (Type::Tuple(elems1), Type::Tuple(elems2)) => {
                    if elems1.len() != elems2.len() {
                        return false;
                    }
                    for (e1, e2) in elems1.iter().zip(elems2.iter()) {
                        if !compatible(e1, e2, aliases, visited) {
                            return false;
                        }
                    }
                    true
                }
                (
                    Type::Pointer {
                        element: e1,
                        is_mut: m1,
                    },
                    Type::Pointer {
                        element: e2,
                        is_mut: m2,
                    },
                ) => {
                    if *m2 && !*m1 {
                        return false;
                    }
                    if **e1 == Type::Opaque || **e2 == Type::Opaque {
                        return true;
                    }
                    Checker::types_equivalent_with_aliases(e1, e2, aliases)
                }
                (
                    Type::Array {
                        element: e1,
                        size: s1,
                    },
                    Type::Array {
                        element: e2,
                        size: s2,
                    },
                ) => s1 == s2 && Checker::types_equivalent_with_aliases(e1, e2, aliases),
                (
                    Type::Function {
                        parameters: p1,
                        return_type: r1,
                        is_variadic: v1,
                    },
                    Type::Function {
                        parameters: p2,
                        return_type: r2,
                        is_variadic: v2,
                    },
                ) => {
                    *v1 == *v2
                        && p1.len() == p2.len()
                        && p1
                            .iter()
                            .zip(p2.iter())
                            .all(|(a, b)| compatible(a, b, aliases, visited))
                        && compatible(r1, r2, aliases, visited)
                }
                _ => {
                    if Checker::is_lossless_integer_widening(provided, expected) {
                        return true;
                    }
                    false
                }
            }
        }

        compatible(provided, expected, aliases, &mut HashSet::new())
    }

    pub fn is_integer_type(ty: &Type) -> bool {
        matches!(
            ty,
            Type::S16
                | Type::S32
                | Type::S64
                | Type::SSize
                | Type::U8
                | Type::U16
                | Type::U32
                | Type::U64
                | Type::USize
        )
    }

    pub fn is_signed_integer_type(ty: &Type) -> bool {
        matches!(ty, Type::S16 | Type::S32 | Type::S64 | Type::SSize)
    }

    pub fn is_unsigned_integer_type(ty: &Type) -> bool {
        matches!(ty, Type::U8 | Type::U16 | Type::U32 | Type::U64 | Type::USize)
    }

    pub fn integer_bit_width(ty: &Type) -> Option<u32> {
        match ty {
            Type::U8 => Some(8),
            Type::S16 | Type::U16 => Some(16),
            Type::S32 | Type::U32 => Some(32),
            Type::S64 | Type::U64 | Type::USize | Type::SSize => Some(64),
            _ => None,
        }
    }

    pub fn is_lossless_integer_widening(source: &Type, target: &Type) -> bool {
        if !Self::is_integer_type(source) || !Self::is_integer_type(target) {
            return false;
        }
        let same_signedness = (Self::is_signed_integer_type(source) && Self::is_signed_integer_type(target))
            || (Self::is_unsigned_integer_type(source) && Self::is_unsigned_integer_type(target));
        if !same_signedness {
            return false;
        }
        match (Self::integer_bit_width(source), Self::integer_bit_width(target)) {
            (Some(s_bits), Some(t_bits)) => s_bits < t_bits,
            _ => false,
        }
    }

    pub fn is_supported_cast(source: &Type, target: &Type) -> bool {
        Self::is_supported_cast_with_aliases(source, target, &HashMap::new())
    }

    pub fn is_supported_cast_with_aliases(
        source: &Type,
        target: &Type,
        aliases: &HashMap<String, Type>,
    ) -> bool {
        fn supported(
            source: &Type,
            target: &Type,
            aliases: &HashMap<String, Type>,
            visited: &mut HashSet<(Type, Type)>,
        ) -> bool {
            if source == target {
                return true;
            }

            if source == &Type::Never {
                return true;
            }

            if matches!(source, Type::Alias(_)) || matches!(target, Type::Alias(_)) {
                let pair = (source.clone(), target.clone());
                if !visited.insert(pair.clone()) {
                    return true;
                }
                let result = match (source, target) {
                    (Type::Alias(left), Type::Alias(right)) => {
                        match (aliases.get(left), aliases.get(right)) {
                            (Some(left_type), Some(right_type)) => {
                                supported(left_type, right_type, aliases, visited)
                            }
                            (Some(left_type), None) => {
                                supported(left_type, target, aliases, visited)
                            }
                            (None, Some(right_type)) => {
                                supported(source, right_type, aliases, visited)
                            }
                            (None, None) => false,
                        }
                    }
                    (Type::Alias(name), other) => aliases
                        .get(name)
                        .is_some_and(|alias_type| supported(alias_type, other, aliases, visited)),
                    (other, Type::Alias(name)) => aliases
                        .get(name)
                        .is_some_and(|alias_type| supported(other, alias_type, aliases, visited)),
                    _ => unreachable!(),
                };
                visited.remove(&pair);
                return result;
            }

            if let Type::Union(target_variants) = target {
                let source_variants = match source {
                    Type::Union(variants) => variants.as_slice(),
                    source => std::slice::from_ref(source),
                };
                return source_variants.iter().all(|source| {
                    target_variants.iter().any(|target| {
                        supported(source, target, aliases, visited)
                    })
                });
            }

            if let Type::Union(variants) = source {
                return variants
                    .iter()
                    .all(|variant| supported(variant, target, aliases, visited));
            }

            if Checker::is_slice_type_with_aliases(source, aliases)
                && Checker::is_slice_type_with_aliases(target, aliases)
            {
                if let (Some(e1), Some(e2)) = (
                    Checker::slice_element_type_with_aliases(source, aliases),
                    Checker::slice_element_type_with_aliases(target, aliases),
                ) {
                    if supported(e1, e2, aliases, visited) {
                        return true;
                    }
                }
            }

            if Checker::is_array_to_slice_castable(source, target, aliases) {
                return true;
            }

            if Checker::is_symbol_to_string_compatible(source, target, aliases) {
                return true;
            }

            if Checker::is_struct_to_record_castable(source, target, aliases) {
                return true;
            }

            match (source, target) {
                (Type::Struct(source_fields), Type::Struct(target_fields)) => {
                    if source_fields.len() != target_fields.len() {
                        return false;
                    }
                    return target_fields.iter().all(|target| {
                        source_fields
                            .iter()
                            .find(|source| source.name == target.name)
                            .is_some_and(|source| supported(&source.ty, &target.ty, aliases, visited))
                    });
                }
                (Type::Tuple(source_elems), Type::Tuple(target_elems)) => {
                    return source_elems.len() == target_elems.len()
                        && source_elems
                            .iter()
                            .zip(target_elems)
                            .all(|(source, target)| {
                                supported(source, target, aliases, visited)
                            });
                }
                (
                    Type::Array {
                        element: source_element,
                        size: source_size,
                    },
                    Type::Array {
                        element: target_element,
                        size: target_size,
                    },
                ) => {
                    return source_size == target_size
                        && supported(source_element, target_element, aliases, visited);
                }
                (
                    Type::Pointer {
                        element: source,
                        is_mut: source_mut,
                    },
                    Type::Pointer {
                        element: target,
                        is_mut: target_mut,
                    },
                ) => {
                    // a *mut T can be casted into a *T but not vice versa
                    if *target_mut && !*source_mut {
                        return false;
                    }
                    if **source == Type::Opaque || **target == Type::Opaque {
                        return true;
                    }
                    return Checker::types_equivalent_with_aliases(source, target, aliases);
                }
                (
                    Type::Function {
                        parameters: p1,
                        return_type: r1,
                        is_variadic: v1,
                    },
                    Type::Function {
                        parameters: p2,
                        return_type: r2,
                        is_variadic: v2,
                    },
                ) => {
                    return v1 == v2
                        && p1.len() == p2.len()
                        && p1
                            .iter()
                            .zip(p2.iter())
                            .all(|(a, b)| supported(a, b, aliases, visited))
                        && supported(r1, r2, aliases, visited);
                }
                _ => {}
            }

            if matches!(source, Type::Symbol(_)) || matches!(target, Type::Symbol(_)) {
                if Checker::is_symbol_to_string_compatible(source, target, aliases) {
                    return true;
                }
                return false;
            }

            if matches!(source, Type::Pointer { .. } | Type::Function { .. })
                && Checker::is_integer_type(target)
            {
                return true;
            }

            if Checker::is_integer_type(source)
                && matches!(target, Type::Pointer { .. } | Type::Function { .. })
            {
                return true;
            }

            if Checker::is_integer_type(source) && Checker::is_integer_type(target) {
                return true;
            }

            false
        }

        supported(source, target, aliases, &mut HashSet::new())
    }

    pub fn castable_union_variant(source: &Type, targets: &[Type]) -> Option<usize> {
        Self::castable_union_variant_with_aliases(source, targets, &HashMap::new())
    }

    pub fn castable_union_variant_with_aliases(
        source: &Type,
        targets: &[Type],
        aliases: &HashMap<String, Type>,
    ) -> Option<usize> {
        if let Some(index) = targets.iter().position(|target| target == source) {
            return Some(index);
        }
        let equiv_matches: Vec<usize> = targets
            .iter()
            .enumerate()
            .filter_map(|(index, target)| {
                Self::types_equivalent_with_aliases(source, target, aliases).then_some(index)
            })
            .collect();
        if equiv_matches.len() == 1 {
            return Some(equiv_matches[0]);
        }
        let implicit_matches: Vec<usize> = targets
            .iter()
            .enumerate()
            .filter_map(|(index, target)| {
                Self::types_compatible_with_aliases(source, target, aliases).then_some(index)
            })
            .collect();
        if implicit_matches.len() == 1 {
            return Some(implicit_matches[0]);
        }
        if !implicit_matches.is_empty() {
            return None;
        }
        let matches: Vec<usize> = targets
            .iter()
            .enumerate()
            .filter_map(|(index, target)| {
                if matches!(source, Type::Pointer { .. }) && Self::is_integer_type(target) {
                    return None;
                }
                if matches!(source, Type::Pointer { .. }) && matches!(target, Type::Pointer { .. }) {
                    if !Self::types_compatible_with_aliases(source, target, aliases) {
                        return None;
                    }
                }
                Self::is_supported_cast_with_aliases(source, target, aliases).then_some(index)
            })
            .collect();
        if matches.len() == 1 {
            Some(matches[0])
        } else {
            None
        }
    }
}

