use std::collections::{HashMap, HashSet};

use crate::syntax::{
    ComparisonOperator, Definition, Expression, ExternalFunction, IfCondition, LogicalModule,
    NamespacePath, Parameter, Signature, SourceFile, Span, Statement, StructFieldType, Type,
    TypeAlias, format_error, unspanned_expression, unspanned_expression_mut,
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
    pub exported_symbols: HashSet<String>,
}

impl ModuleScope {
    pub fn new(dir_path: String) -> Self {
        Self {
            dir_path,
            definitions: HashMap::new(),
            extern_functions: HashMap::new(),
            type_aliases: HashMap::new(),
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

pub struct ModuleResolver {
    namespace_registry: HashMap<NamespacePath, ExportRegistry>,
}

#[derive(Default)]
struct ExportRegistry {
    definitions: HashMap<String, SymbolProvenance<Definition>>,
    extern_functions: HashMap<String, SymbolProvenance<ExternalFunction>>,
    type_aliases: HashMap<String, SymbolProvenance<TypeAlias>>,
}

pub fn expand_type_aliases_with_map(ty: &Type, aliases: &HashMap<String, TypeAlias>) -> Type {
    let mut visited = HashSet::new();

    fn expand(
        t: &Type,
        aliases: &HashMap<String, TypeAlias>,
        visited: &mut HashSet<String>,
    ) -> Type {
        match t {
            Type::Alias(name) => {
                if !visited.insert(name.clone()) {
                    return t.clone();
                }
                let res = if let Some(alias) = aliases.get(name) {
                    expand(&alias.target, aliases, visited)
                } else {
                    t.clone()
                };
                visited.remove(name);
                res
            }
            Type::Union(variants) => {
                let mut flat = Vec::new();
                for v in variants {
                    let res = expand(v, aliases, visited);
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
                        ty: expand(&f.ty, aliases, visited),
                    })
                    .collect();
                Type::Struct(flds)
            }
            Type::Pointer { element } => Type::Pointer {
                element: Box::new(expand(element, aliases, visited)),
            },
            Type::Slice { element } => Type::Slice {
                element: Box::new(expand(element, aliases, visited)),
            },
            Type::Array { element, size } => Type::Array {
                element: Box::new(expand(element, aliases, visited)),
                size: *size,
            },
            Type::Function {
                parameters,
                return_type,
                is_variadic,
            } => {
                let params = parameters
                    .iter()
                    .map(|p| expand(p, aliases, visited))
                    .collect();
                Type::Function {
                    parameters: params,
                    return_type: Box::new(expand(return_type, aliases, visited)),
                    is_variadic: *is_variadic,
                }
            }
            other => other.clone(),
        }
    }

    expand(ty, aliases, &mut visited)
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
                let target_ns = NamespacePath::from_segments(export_rule.to.clone());
                let registry = self
                    .namespace_registry
                    .entry(target_ns.clone())
                    .or_default();

                for sym in &export_rule.symbols {
                    let mut found_def = None;
                    let mut found_ext = None;
                    let mut found_alias = None;

                    for file in &module.source_files {
                        if let Some(def) = file.definitions.iter().find(|d| d.name == *sym) {
                            let mut resolved_def = def.clone();
                            if let Expression::Block {
                                signature: Some(sig),
                                ..
                            } = unspanned_expression_mut(&mut resolved_def.value)
                            {
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
                            "export error in '{}/mod.yawn': symbol '{}' is not defined in any .zzz file in this directory",
                            module.dir_path, sym
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

            let mut imports = vec![NamespacePath::root()];
            for imp in &module.imports {
                imports.push(NamespacePath::from_segments(imp.clone()));
            }

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
}

pub struct Checker<'a> {
    scope: &'a ModuleScope,
    current_file_path: String,

    resolved_extern_functions: HashMap<String, Signature>,
    resolving_extern_functions: HashSet<String>,

    resolved_definitions: HashMap<String, Type>,
    resolving_definitions: HashSet<String>,
    checked_bodies: HashSet<String>,

    local_scopes: Vec<HashMap<String, BindingInfo>>,
    atom_table: HashMap<String, u32>,
}

impl<'a> Checker<'a> {
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
            atom_table: HashMap::new(),
        };

        checker.predeclare_scope_signatures();
        checker
    }

    pub fn intern_atom(&mut self, name: &str) -> u32 {
        if let Some(&id) = self.atom_table.get(name) {
            id
        } else {
            let id = self.atom_table.len() as u32;
            self.atom_table.insert(name.to_string(), id);
            id
        }
    }

    pub fn canonical_type_key(ty: &Type) -> String {
        match ty {
            Type::Void => "00_void".to_string(),
            Type::Bool => "01_bool".to_string(),
            Type::U8 => "02_u8".to_string(),
            Type::U16 => "03_u16".to_string(),
            Type::U32 => "04_u32".to_string(),
            Type::U64 => "05_u64".to_string(),
            Type::S16 => "06_s16".to_string(),
            Type::S32 => "07_s32".to_string(),
            Type::S64 => "08_s64".to_string(),
            Type::Atom(name) => format!("09_atom_{}", name),
            Type::Alias(name) => format!("10_alias_{}", name),
            Type::Pointer { element } => format!("11_ptr_{}", Self::canonical_type_key(element)),
            Type::Slice { element } => format!("12_slice_{}", Self::canonical_type_key(element)),
            Type::Array { element, size } => {
                format!("13_arr_{}_{}", size, Self::canonical_type_key(element))
            }
            Type::Struct(fields) => {
                let inner = fields
                    .iter()
                    .map(|f| format!("{}:{}", f.name, Self::canonical_type_key(&f.ty)))
                    .collect::<Vec<_>>()
                    .join(",");
                format!("14_struct_{{{}}}", inner)
            }
            Type::Function {
                parameters,
                return_type,
                is_variadic,
            } => {
                let params = parameters
                    .iter()
                    .map(Self::canonical_type_key)
                    .collect::<Vec<_>>()
                    .join(",");
                format!(
                    "15_fn_({})_{}_{}",
                    params,
                    Self::canonical_type_key(return_type),
                    is_variadic
                )
            }
            Type::Union(variants) => {
                let inner = variants
                    .iter()
                    .map(Self::canonical_type_key)
                    .collect::<Vec<_>>()
                    .join("|");
                format!("16_union_{}", inner)
            }
        }
    }

    pub fn resolve_type(&self, ty: &Type) -> Result<Type, String> {
        let mut visited = HashSet::new();
        self.resolve_type_inner(ty, &mut visited)
    }

    fn resolve_type_inner(&self, ty: &Type, visited: &mut HashSet<String>) -> Result<Type, String> {
        match ty {
            Type::Alias(name) => {
                if !visited.insert(name.clone()) {
                    return Err(format!(
                        "in '{}': recursive cycle in type alias '{}'",
                        self.current_file_path, name
                    ));
                }
                let alias = self.scope.lookup_type_alias(name).ok_or_else(|| {
                    format!("in '{}': unknown type '{}'", self.current_file_path, name)
                })?;
                let resolved = self.resolve_type_inner(&alias.target, visited)?;
                visited.remove(name);
                Ok(resolved)
            }
            Type::Struct(fields) => {
                let resolved_fields = fields
                    .iter()
                    .map(|f| {
                        Ok(StructFieldType {
                            name: f.name.clone(),
                            ty: self.resolve_type_inner(&f.ty, visited)?,
                        })
                    })
                    .collect::<Result<Vec<_>, String>>()?;
                Ok(Type::Struct(resolved_fields))
            }
            Type::Pointer { element } => Ok(Type::Pointer {
                element: Box::new(self.resolve_type_inner(element, visited)?),
            }),
            Type::Array { element, size } => Ok(Type::Array {
                element: Box::new(self.resolve_type_inner(element, visited)?),
                size: *size,
            }),
            Type::Slice { element } => Ok(Type::Slice {
                element: Box::new(self.resolve_type_inner(element, visited)?),
            }),
            Type::Function {
                parameters,
                return_type,
                is_variadic,
            } => {
                let params = parameters
                    .iter()
                    .map(|p| self.resolve_type_inner(p, visited))
                    .collect::<Result<Vec<_>, _>>()?;
                let ret = self.resolve_type_inner(return_type, visited)?;
                Ok(Type::Function {
                    parameters: params,
                    return_type: Box::new(ret),
                    is_variadic: *is_variadic,
                })
            }
            Type::Union(variants) => {
                let mut flat = Vec::new();
                for v in variants {
                    let resolved = self.resolve_type_inner(v, visited)?;
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
                        "in '{}': sum type exceeds maximum of 256 variants (found {})",
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
            return Ok(Type::Void);
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

        if unique.len() == 1 {
            return Ok(unique.remove(0));
        }

        unique.sort_by_key(Self::canonical_type_key);
        Ok(Type::Union(unique))
    }

    /// Coerces an expression in-place to match `expected_ty` (resolved).
    /// Handles both Union upcasting and Array-Literal-to-Slice-of-Union element coercion.
    fn coerce_expression_to_type(
        &mut self,
        expr: &mut Expression,
        actual_ty: &Type,
        expected_ty: &Type,
    ) -> Result<Type, String> {
        let res_actual = self.resolve_type(actual_ty)?;
        let res_expected = self.resolve_type(expected_ty)?;

        if res_actual == res_expected {
            return Ok(res_actual);
        }

        // Case 1: Array literal `[e1, e2, ...]` passed to `[]ExpectedElem`
        // If each element is compatible with `ExpectedElem`, coerce the elements in-place!
        if let (
            Expression::Array(elements),
            Type::Slice {
                element: expected_elem,
            },
        ) = (&mut *expr, &res_expected)
        {
            let mut all_compatible = true;
            for el in elements.iter_mut() {
                let el_ty = self.type_of_expression(el)?;
                if !self.are_types_compatible(&el_ty, expected_elem)? {
                    all_compatible = false;
                    break;
                }
            }

            if all_compatible {
                for el in elements.iter_mut() {
                    let el_ty = self.type_of_expression(el)?;
                    self.coerce_expression_to_type(el, &el_ty, expected_elem)?;
                }
                return Ok(res_expected);
            }
        }

        // Case 2: Value or Sub-Union passed to a Union
        if let Type::Union(_) = res_expected {
            if Self::types_compatible(&res_actual, &res_expected) {
                let orig = std::mem::replace(expr, Expression::Boolean(false));
                *expr = Expression::Cast {
                    expression: Box::new(orig),
                    ty: res_expected.clone(),
                };
                return Ok(res_expected);
            }
        }

        Ok(res_actual)
    }

    fn predeclare_scope_signatures(&mut self) {
        for (name, prov) in &self.scope.extern_functions {
            self.resolved_extern_functions
                .insert(name.clone(), prov.item.signature.clone());
        }

        for (name, prov) in &self.scope.definitions {
            if let Expression::Block { signature, .. } = unspanned_expression(&prov.item.value) {
                let mut param_types = Vec::new();
                let mut expected_return = Type::Void;
                let mut is_variadic = false;

                if let Some(sig) = signature {
                    for p in &sig.parameters {
                        let res_ty = self.resolve_type(&p.ty).unwrap_or_else(|_| p.ty.clone());
                        param_types.push(res_ty);
                    }
                    expected_return = self
                        .resolve_type(&sig.return_type)
                        .unwrap_or_else(|_| *sig.return_type.clone());
                    is_variadic = sig.is_variadic;
                }

                let fn_ty = Type::Function {
                    parameters: param_types,
                    return_type: Box::new(expected_return),
                    is_variadic,
                };
                self.resolved_definitions.insert(name.clone(), fn_ty);
            }
        }
    }

    pub fn check_file(&mut self, file: &mut SourceFile) -> Result<(), String> {
        self.current_file_path = file.file_path.clone();

        for alias in &mut file.type_aliases {
            alias.target = self
                .resolve_type(&alias.target)
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
        if !self.checked_bodies.insert(def.name.clone()) {
            return Ok(());
        }

        match unspanned_expression_mut(&mut def.value) {
            Expression::Block { signature, body } => {
                let mut expected_return = Type::Void;

                self.local_scopes.push(HashMap::new());

                if let Some(sig) = signature {
                    let mut seen_params = HashSet::new();
                    for p in &mut sig.parameters {
                        if !seen_params.insert(p.name.clone()) {
                            self.local_scopes.pop();
                            return Err(format!(
                                "in '{}': duplicate parameter name '{}' in block",
                                self.current_file_path, p.name
                            ));
                        }
                        let resolved_param_ty = self.resolve_type(&p.ty)?;
                        if resolved_param_ty == Type::Void {
                            self.local_scopes.pop();
                            return Err(format!(
                                "in '{}': block parameter '{}' cannot be void",
                                self.current_file_path, p.name
                            ));
                        }
                        p.ty = resolved_param_ty.clone();
                        self.local_scopes.last_mut().unwrap().insert(
                            p.name.clone(),
                            BindingInfo {
                                ty: resolved_param_ty,
                                is_mutable: false,
                                is_initialized: true,
                            },
                        );
                    }
                    expected_return = self.resolve_type(sig.return_type.as_ref())?;
                    *sig.return_type = expected_return.clone();
                }

                let validate_res = self.validate_block(body, &expected_return);
                self.local_scopes.pop();
                validate_res?;
            }
            expr => {
                let ty = self.type_of_expression(expr)?;
                self.resolved_definitions.insert(def.name.clone(), ty);
            }
        }

        Ok(())
    }

    fn resolve_external_function(&mut self, name: &str) -> Result<Signature, String> {
        let raw = self.scope.lookup_extern(name).ok_or_else(|| {
            format!(
                "in '{}': unknown external function '{}'",
                self.current_file_path, name
            )
        })?;

        if !self.resolving_extern_functions.insert(name.to_string()) {
            return Err(format!(
                "in '{}': cyclic dependency involving external function '{}'",
                self.current_file_path, name
            ));
        }

        let mut seen_params = HashSet::new();
        let mut parameters = Vec::new();

        for p in &raw.signature.parameters {
            if !seen_params.insert(p.name.clone()) {
                return Err(format!(
                    "in '{}': duplicate parameter name '{}' in extern function '{}'",
                    self.current_file_path, p.name, name
                ));
            }
            let resolved_p_ty = self.resolve_type(&p.ty)?;
            if resolved_p_ty == Type::Void {
                return Err(format!(
                    "in '{}': parameter '{}' in extern function '{}' cannot be void",
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

    fn resolve_definition(&mut self, name: &str) -> Result<Type, String> {
        if let Some(ty) = self.resolved_definitions.get(name) {
            return Ok(ty.clone());
        }

        let def = self.scope.lookup_definition(name).ok_or_else(|| {
            format!(
                "in '{}': unknown identifier '{}' (not defined in this module or imported in mod.yawn)",
                self.current_file_path, name
            )
        })?;

        if !self.resolving_definitions.insert(name.to_string()) {
            return Err(format!(
                "in '{}': cyclic dependency involving global constant '@{}'",
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

    pub fn type_of_expression(&mut self, expression: &mut Expression) -> Result<Type, String> {
        match expression {
            Expression::Spanned { expression, span } => self
                .type_of_expression(expression)
                .map_err(|error| format_error(span, &error)),
            Expression::Integer(_) => Ok(Type::S32),
            Expression::Boolean(_) => Ok(Type::Bool),
            Expression::Atom(name) => {
                self.intern_atom(name);
                Ok(Type::Atom(name.clone()))
            }
            Expression::String(_) => Ok(Type::Slice {
                element: Box::new(Type::U8),
            }),

            Expression::Struct(fields) => {
                let mut struct_fields = Vec::with_capacity(fields.len());
                let mut seen_fields = HashSet::new();

                for field in fields {
                    if !seen_fields.insert(field.name.clone()) {
                        return Err(format!(
                            "in '{}': duplicate field '@{}' in struct literal",
                            self.current_file_path, field.name
                        ));
                    }
                    let field_ty = self.type_of_expression(&mut field.value)?;
                    struct_fields.push(StructFieldType {
                        name: field.name.clone(),
                        ty: field_ty,
                    });
                }

                Ok(Type::Struct(struct_fields))
            }

            Expression::Identifier(name) => {
                if self.scope.lookup_type_alias(name).is_some() {
                    return Err(format!(
                        "in '{}': type alias '{}' cannot be used as an expression",
                        self.current_file_path, name
                    ));
                }

                if let Some(info) = self.lookup_binding(name) {
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
                    "in '{}': unknown identifier '{}' is not defined in this module or imported in mod.yawn",
                    self.current_file_path, name
                ))
            }

            Expression::Signature(sig) => {
                let mut param_types = Vec::new();
                let mut seen_params = HashSet::new();

                for p in &mut sig.parameters {
                    if !seen_params.insert(p.name.clone()) {
                        return Err(format!(
                            "in '{}': duplicate parameter name '{}' in signature",
                            self.current_file_path, p.name
                        ));
                    }
                    let resolved_p_ty = self.resolve_type(&p.ty)?;
                    if resolved_p_ty == Type::Void {
                        return Err(format!(
                            "in '{}': parameter '{}' cannot be void",
                            self.current_file_path, p.name
                        ));
                    }
                    p.ty = resolved_p_ty.clone();
                    param_types.push(resolved_p_ty);
                }
                let ret_ty = self.resolve_type(sig.return_type.as_ref())?;
                *sig.return_type = ret_ty.clone();
                Ok(Type::Function {
                    parameters: param_types,
                    return_type: Box::new(ret_ty),
                    is_variadic: sig.is_variadic,
                })
            }

            Expression::Block { signature, body } => {
                let mut param_types = Vec::new();
                let mut expected_return = Type::Void;
                let mut is_variadic = false;

                self.local_scopes.push(HashMap::new());

                if let Some(sig) = signature {
                    let mut seen_params = HashSet::new();
                    for p in &mut sig.parameters {
                        if !seen_params.insert(p.name.clone()) {
                            self.local_scopes.pop();
                            return Err(format!(
                                "in '{}': duplicate parameter name '{}' in block",
                                self.current_file_path, p.name
                            ));
                        }
                        let resolved_p_ty = self.resolve_type(&p.ty)?;
                        if resolved_p_ty == Type::Void {
                            self.local_scopes.pop();
                            return Err(format!(
                                "in '{}': block parameter '{}' cannot be void",
                                self.current_file_path, p.name
                            ));
                        }
                        p.ty = resolved_p_ty.clone();
                        self.local_scopes.last_mut().unwrap().insert(
                            p.name.clone(),
                            BindingInfo {
                                ty: resolved_p_ty.clone(),
                                is_mutable: false,
                                is_initialized: true,
                            },
                        );
                        param_types.push(resolved_p_ty);
                    }
                    expected_return = self.resolve_type(sig.return_type.as_ref())?;
                    *sig.return_type = expected_return.clone();
                    is_variadic = sig.is_variadic;
                }

                let validate_res = self.validate_block(body, &expected_return);
                self.local_scopes.pop();
                validate_res?;

                Ok(Type::Function {
                    parameters: param_types,
                    return_type: Box::new(expected_return),
                    is_variadic,
                })
            }

            Expression::If { arms } => {
                let has_else = arms
                    .iter()
                    .any(|a| matches!(a.condition, IfCondition::Else));
                if !has_else {
                    return Err(format!(
                        "in '{}': expression 'if' must have a fallback 'else' arm",
                        self.current_file_path
                    ));
                }

                let num_arms = arms.len();
                let mut arm_types = Vec::new();

                for (i, arm) in arms.iter_mut().enumerate() {
                    match &mut arm.condition {
                        IfCondition::Expression(cond) => {
                            let cond_ty = self.type_of_expression(cond)?;
                            if cond_ty != Type::Bool {
                                return Err(format!(
                                    "in '{}': if condition must evaluate to Bool, found {:?}",
                                    self.current_file_path, cond_ty
                                ));
                            }
                        }
                        IfCondition::Else => {
                            if i != num_arms - 1 {
                                return Err(format!(
                                    "in '{}': fallback 'else' arm must be the final arm in if",
                                    self.current_file_path
                                ));
                            }
                        }
                    }

                    let arm_ty = self.type_of_expression(&mut arm.body)?;
                    arm_types.push(self.resolve_type(&arm_ty)?);
                }

                let unified_return_ty = self.unify_to_sum_type(&arm_types)?;

                if let Type::Union(_) = unified_return_ty {
                    for (i, arm) in arms.iter_mut().enumerate() {
                        let arm_ty = &arm_types[i];
                        if arm_ty != &unified_return_ty {
                            let orig = std::mem::replace(
                                &mut arm.body,
                                Box::new(Expression::Boolean(false)),
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
                let target_variants = match &target_ty {
                    Type::Union(vars) => vars.clone(),
                    other => {
                        return Err(format!(
                            "in '{}': match target must evaluate to a sum type, found {:?}",
                            self.current_file_path, other
                        ));
                    }
                };

                if arms.is_empty() {
                    return Err(format!(
                        "in '{}': match expression must have at least one arm",
                        self.current_file_path
                    ));
                }

                let mut unhandled = target_variants.clone();
                let mut arm_types = Vec::new();

                for arm in arms.iter_mut() {
                    let arm_var_ty = self.resolve_type(&arm.variant_type)?;
                    arm.variant_type = arm_var_ty.clone();

                    let covered_variants = match &arm_var_ty {
                        Type::Union(inner) => inner.clone(),
                        single => vec![single.clone()],
                    };

                    for v in &covered_variants {
                        if !target_variants.contains(v) {
                            return Err(format!(
                                "in '{}': match arm variant {:?} is not a member of sum type {:?}",
                                self.current_file_path, v, target_ty
                            ));
                        }

                        let pos = unhandled.iter().position(|u| u == v).ok_or_else(|| {
                            format!(
                                "in '{}': unreachable or duplicate match arm for variant {:?}",
                                self.current_file_path, v
                            )
                        })?;
                        unhandled.remove(pos);
                    }

                    self.local_scopes.push(HashMap::new());
                    self.local_scopes.last_mut().unwrap().insert(
                        binding_name.clone(),
                        BindingInfo {
                            ty: arm_var_ty,
                            is_mutable: false,
                            is_initialized: true,
                        },
                    );

                    let arm_res_ty = self.type_of_expression(&mut arm.body);
                    self.local_scopes.pop();
                    let arm_ty = self.resolve_type(&arm_res_ty?)?;
                    arm_types.push(arm_ty);
                }

                if !unhandled.is_empty() {
                    return Err(format!(
                        "in '{}': non-exhaustive match expression: missing cases for {:?}",
                        self.current_file_path, unhandled
                    ));
                }

                let unified_return_ty = self.unify_to_sum_type(&arm_types)?;

                if let Type::Union(_) = unified_return_ty {
                    for (i, arm) in arms.iter_mut().enumerate() {
                        let arm_ty = &arm_types[i];
                        if arm_ty != &unified_return_ty {
                            let orig = std::mem::replace(
                                &mut arm.body,
                                Box::new(Expression::Boolean(false)),
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
                if elements.is_empty() {
                    return Err(format!(
                        "in '{}': cannot infer type of empty array",
                        self.current_file_path
                    ));
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
                            let orig = std::mem::replace(element, Expression::Boolean(false));
                            *element = Expression::Cast {
                                expression: Box::new(orig),
                                ty: unified_elem_ty.clone(),
                            };
                        }
                    }
                }

                Ok(Type::Slice {
                    element: Box::new(unified_elem_ty),
                })
            }

            Expression::Unary { operator, operand } => {
                let raw_ty = self.type_of_expression(operand)?;
                let ty = self.resolve_type(&raw_ty)?;
                if !Self::is_integer_type(&ty) {
                    return Err(format!(
                        "in '{}': unary operator {:?} requires an integer operand, found {:?}",
                        self.current_file_path, operator, ty
                    ));
                }
                Ok(ty)
            }

            Expression::Binary {
                left,
                operator,
                right,
            } => {
                let raw_left = self.type_of_expression(left)?;
                let raw_right = self.type_of_expression(right)?;
                let left_type = self.resolve_type(&raw_left)?;
                let right_type = self.resolve_type(&raw_right)?;

                if !Self::is_integer_type(&left_type) || !Self::is_integer_type(&right_type) {
                    return Err(format!(
                        "in '{}': binary operator {:?} requires integer operands",
                        self.current_file_path, operator
                    ));
                }

                if left_type != right_type {
                    return Err(format!(
                        "in '{}': binary operator {:?} requires matching types, found {:?} and {:?}",
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
                let left_type = self.resolve_type(&raw_left)?;
                let right_type = self.resolve_type(&raw_right)?;

                if matches!(left_type, Type::Atom(_)) || matches!(right_type, Type::Atom(_)) {
                    if !matches!(
                        operator,
                        ComparisonOperator::Equal | ComparisonOperator::NotEqual
                    ) {
                        return Err(format!(
                            "in '{}': atoms only support equality comparisons ('=' and '!=')",
                            self.current_file_path
                        ));
                    }
                    if left_type != right_type {
                        return Err(format!(
                            "in '{}': cannot compare distinct atom types: {:?} and {:?}",
                            self.current_file_path, left_type, right_type
                        ));
                    }
                    return Ok(Type::Bool);
                }

                if left_type != right_type {
                    return Err(format!(
                        "in '{}': comparison operator requires matching types, found {:?} and {:?}",
                        self.current_file_path, left_type, right_type
                    ));
                }

                Ok(Type::Bool)
            }

            Expression::Cast { expression, ty } => {
                let raw_source = self.type_of_expression(expression)?;
                let source_type = self.resolve_type(&raw_source)?;
                let resolved_target = self.resolve_type(ty)?;
                *ty = resolved_target.clone();
                if !Self::is_supported_cast(&source_type, &resolved_target) {
                    return Err(format!(
                        "in '{}': unsupported cast from {:?} to {:?}",
                        self.current_file_path, source_type, resolved_target
                    ));
                }
                Ok(resolved_target)
            }

            Expression::Call { target, arguments } => {
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
                            "in '{}': cannot call non-function type {:?}",
                            self.current_file_path, target_type
                        ));
                    }
                };

                if is_variadic {
                    if arguments.len() < param_types.len() {
                        return Err(format!(
                            "in '{}': variadic call expects at least {} arguments, found {}",
                            self.current_file_path,
                            param_types.len(),
                            arguments.len()
                        ));
                    }
                } else if arguments.len() != param_types.len() {
                    return Err(format!(
                        "in '{}': call expects {} arguments, found {}",
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
                            "in '{}': argument type mismatch: expected {:?}, found {:?}",
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
                        "in '{}': array index must be an integer",
                        self.current_file_path
                    ));
                }

                match expression_type {
                    Type::Array { element, .. } => Ok(*element),
                    Type::Slice { element } => Ok(*element),
                    other => Err(format!(
                        "in '{}': cannot index value of type {:?}",
                        self.current_file_path, other
                    )),
                }
            }

            Expression::Member { expression, field } => {
                let raw_expr_type = self.type_of_expression(expression)?;
                let expr_type = self.resolve_type(&raw_expr_type)?;

                match expr_type {
                    Type::Struct(fields) => {
                        if let Some(f) = fields.iter().find(|f| f.name == *field) {
                            Ok(f.ty.clone())
                        } else {
                            Err(format!(
                                "in '{}': struct has no field named '{}'",
                                self.current_file_path, field
                            ))
                        }
                    }
                    Type::Slice { element } => match field.as_str() {
                        "ptr" => Ok(Type::Pointer { element }),
                        "len" => Ok(Type::U64),
                        _ => Err(format!(
                            "in '{}': unknown slice member '.{}'",
                            self.current_file_path, field
                        )),
                    },
                    Type::Array { element, .. } => match field.as_str() {
                        "ptr" => Ok(Type::Pointer { element }),
                        "len" => Ok(Type::U64),
                        _ => Err(format!(
                            "in '{}': unknown array member '.{}'",
                            self.current_file_path, field
                        )),
                    },
                    other => Err(format!(
                        "in '{}': member access '.{}' unsupported on type {:?}",
                        self.current_file_path, field, other
                    )),
                }
            }

            Expression::AddressOf(expression) => match expression.as_ref() {
                Expression::Index { .. } => {
                    let element_type = self.type_of_expression(expression)?;
                    Ok(Type::Pointer {
                        element: Box::new(element_type),
                    })
                }
                Expression::Identifier(name) => {
                    if let Some(info) = self.lookup_binding(name) {
                        Ok(Type::Pointer {
                            element: Box::new(info.ty),
                        })
                    } else if let Some(ty) = self.resolved_definitions.get(name) {
                        Ok(Type::Pointer {
                            element: Box::new(ty.clone()),
                        })
                    } else {
                        Err(format!(
                            "in '{}': cannot take address of unknown variable '{}'",
                            self.current_file_path, name
                        ))
                    }
                }
                _ => Err(format!(
                    "in '{}': address-of requires a variable or array index",
                    self.current_file_path
                )),
            },

            Expression::Decay(expression) => {
                let raw_type = self.type_of_expression(expression)?;
                let inner_type = self.resolve_type(&raw_type)?;
                match inner_type {
                    Type::Array { element, .. } => Ok(Type::Pointer { element }),
                    Type::Slice { element } => Ok(Type::Pointer { element }),
                    Type::Pointer { .. } => Ok(inner_type),
                    other => Err(format!(
                        "in '{}': decay operator '^' expects an array or slice type, found {:?}",
                        self.current_file_path, other
                    )),
                }
            }
        }
    }

    pub fn are_types_compatible(&self, provided: &Type, expected: &Type) -> Result<bool, String> {
        let res_prov = self.resolve_type(provided)?;
        let res_exp = self.resolve_type(expected)?;
        Ok(Self::types_compatible(&res_prov, &res_exp))
    }

    pub fn types_compatible(provided: &Type, expected: &Type) -> bool {
        if provided == expected {
            return true;
        }

        // Subset Union Subtyping: A <= B if all variants in A are in B
        if let Type::Union(expected_vars) = expected {
            if let Type::Union(provided_vars) = provided {
                return provided_vars.iter().all(|pv| expected_vars.contains(pv));
            } else {
                return expected_vars.contains(provided);
            }
        }

        match (provided, expected) {
            (Type::Struct(fields1), Type::Struct(fields2)) => {
                if fields1.len() != fields2.len() {
                    return false;
                }
                for (f1, f2) in fields1.iter().zip(fields2.iter()) {
                    if f1.name != f2.name || !Self::types_compatible(&f1.ty, &f2.ty) {
                        return false;
                    }
                }
                true
            }
            // Pointers and Slices require exact element layout equality in memory
            (Type::Pointer { element: e1 }, Type::Pointer { element: e2 }) => e1 == e2,
            (
                Type::Array {
                    element: e1,
                    size: s1,
                },
                Type::Array {
                    element: e2,
                    size: s2,
                },
            ) => s1 == s2 && e1 == e2,
            (Type::Slice { element: e1 }, Type::Slice { element: e2 }) => e1 == e2,
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
                v1 == v2
                    && p1.len() == p2.len()
                    && p1
                        .iter()
                        .zip(p2.iter())
                        .all(|(a, b)| Self::types_compatible(a, b))
                    && Self::types_compatible(r1, r2)
            }
            _ => false,
        }
    }

    fn statement_definitely_returns(stmt: &Statement) -> bool {
        match stmt {
            Statement::Spanned { statement, .. } => Self::statement_definitely_returns(statement),
            Statement::Return(_) => true,
            Statement::Compound(stmts) => stmts.iter().any(Self::statement_definitely_returns),
            Statement::If { arms } => {
                !arms.is_empty()
                    && arms
                        .iter()
                        .all(|arm| Self::statement_definitely_returns(&arm.body))
            }
            _ => false,
        }
    }

    fn validate_statement(
        &mut self,
        stmt: &mut Statement,
        expected_type: &Type,
    ) -> Result<(), String> {
        if let Statement::Spanned { statement, span } = stmt {
            return self
                .validate_statement(statement, expected_type)
                .map_err(|error| format_error(span, &error));
        }
        self.validate_statement_inner(stmt, expected_type)
    }

    fn validate_statement_inner(
        &mut self,
        stmt: &mut Statement,
        expected_type: &Type,
    ) -> Result<(), String> {
        match stmt {
            Statement::Spanned { statement, span } => {
                return self
                    .validate_statement(statement, expected_type)
                    .map_err(|error| format_error(span, &error));
            }
            Statement::Compound(stmts) => {
                for s in stmts {
                    self.validate_statement(s, expected_type)?;
                }
            }

            Statement::Declare { name, ty } => {
                let resolved_ty = self.resolve_type(ty)?;
                if resolved_ty == Type::Void {
                    return Err(format!(
                        "in '{}': variable '{}' cannot be declared with type void",
                        self.current_file_path, name
                    ));
                }
                if self.local_scopes.last().unwrap().contains_key(name) {
                    return Err(format!(
                        "in '{}': variable '{}' already declared in this scope",
                        self.current_file_path, name
                    ));
                }
                *ty = resolved_ty.clone();
                self.local_scopes.last_mut().unwrap().insert(
                    name.clone(),
                    BindingInfo {
                        ty: resolved_ty,
                        is_mutable: true,
                        is_initialized: false,
                    },
                );
            }

            Statement::Assign { expression, target } => {
                let mut expr_ty = self.type_of_expression(expression)?;

                if self.scope.lookup_definition(target).is_some() {
                    return Err(format!(
                        "in '{}': cannot assign to global constant '@{}'",
                        self.current_file_path, target
                    ));
                }
                if self.scope.lookup_extern(target).is_some() {
                    return Err(format!(
                        "in '{}': cannot assign to external function '{}'",
                        self.current_file_path, target
                    ));
                }
                if self.scope.lookup_type_alias(target).is_some() {
                    return Err(format!(
                        "in '{}': cannot assign to type alias '{}'",
                        self.current_file_path, target
                    ));
                }

                if let Some(info) = self.lookup_binding(target) {
                    if !info.is_mutable {
                        return Err(format!(
                            "in '{}': cannot reassign to immutable constant '{}'",
                            self.current_file_path, target
                        ));
                    }
                    expr_ty = self.coerce_expression_to_type(expression, &expr_ty, &info.ty)?;
                    if !self.are_types_compatible(&expr_ty, &info.ty)? {
                        return Err(format!(
                            "in '{}': type mismatch assigning to '{}': expected {:?}, found {:?}",
                            self.current_file_path, target, info.ty, expr_ty
                        ));
                    }

                    if let Some(info_mut) = self.lookup_binding_mut(target) {
                        info_mut.is_initialized = true;
                    }
                } else {
                    let resolved_expr_ty = self.resolve_type(&expr_ty)?;
                    self.local_scopes.last_mut().unwrap().insert(
                        target.clone(),
                        BindingInfo {
                            ty: resolved_expr_ty,
                            is_mutable: false,
                            is_initialized: true,
                        },
                    );
                }
            }

            Statement::Return(expr) => {
                let mut actual = self.type_of_expression(expr)?;
                actual = self.coerce_expression_to_type(expr, &actual, expected_type)?;
                if !self.are_types_compatible(&actual, expected_type)? {
                    return Err(format!(
                        "in '{}': return type mismatch: expected {:?}, found {:?}",
                        self.current_file_path, expected_type, actual
                    ));
                }
            }

            Statement::Discard(expr) => {
                self.type_of_expression(expr)?;
            }

            Statement::If { arms } => {
                for arm in arms {
                    if let IfCondition::Expression(cond) = &mut arm.condition {
                        let cond_ty = self.type_of_expression(cond)?;
                        if cond_ty != Type::Bool {
                            return Err(format!(
                                "in '{}': doif condition must evaluate to Bool, found {:?}",
                                self.current_file_path, cond_ty
                            ));
                        }
                    }
                    self.validate_statement(&mut arm.body, expected_type)?;
                }
            }
        }
        Ok(())
    }

    fn validate_block(
        &mut self,
        statements: &mut [Statement],
        expected_type: &Type,
    ) -> Result<(), String> {
        for stmt in statements.iter_mut() {
            self.validate_statement(stmt, expected_type)?;
        }

        if *expected_type != Type::Void
            && !statements.iter().any(Self::statement_definitely_returns)
        {
            return Err(format!(
                "in '{}': missing return statement: non-void function expecting return type {:?} must return a value along all execution paths",
                self.current_file_path, expected_type
            ));
        }

        Ok(())
    }

    pub fn is_integer_type(ty: &Type) -> bool {
        matches!(
            ty,
            Type::S16 | Type::S32 | Type::S64 | Type::U8 | Type::U16 | Type::U32 | Type::U64
        )
    }

    pub fn is_supported_cast(source: &Type, target: &Type) -> bool {
        if source == target {
            return true;
        }

        if let Type::Union(target_vars) = target {
            if let Type::Union(source_vars) = source {
                if source_vars.iter().all(|sv| target_vars.contains(sv)) {
                    return true;
                }
            } else if target_vars.contains(source) {
                return true;
            }
        }

        if let Type::Union(variants) = source {
            if variants.contains(target) {
                return true;
            }
        }

        if matches!(source, Type::Atom(_)) || matches!(target, Type::Atom(_)) {
            return false;
        }

        if (*source == Type::Bool && Self::is_integer_type(target))
            || (Self::is_integer_type(source) && *target == Type::Bool)
        {
            return true;
        }

        if (matches!(source, Type::Pointer { .. } | Type::Function { .. })
            && Self::is_integer_type(target))
            || (Self::is_integer_type(source)
                && matches!(target, Type::Pointer { .. } | Type::Function { .. }))
        {
            return true;
        }

        if Self::is_integer_type(source) && Self::is_integer_type(target) {
            return true;
        }

        false
    }
}
