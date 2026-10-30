use std::collections::{HashMap, HashSet};

use crate::syntax::{
    ComparisonOperator, Definition, Expression, ExternalFunction, IfCondition, LogicalModule,
    NamespacePath, Parameter, Signature, SourceFile, Statement, Type,
};

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
    pub exported_symbols: HashSet<String>,
}

impl ModuleScope {
    pub fn new(dir_path: String) -> Self {
        Self {
            dir_path,
            definitions: HashMap::new(),
            extern_functions: HashMap::new(),
            exported_symbols: HashSet::new(),
        }
    }

    pub fn lookup_definition(&self, name: &str) -> Option<&Definition> {
        self.definitions.get(name).map(|p| &p.item)
    }

    pub fn lookup_extern(&self, name: &str) -> Option<&ExternalFunction> {
        self.extern_functions.get(name).map(|p| &p.item)
    }
}

pub struct ModuleResolver {
    namespace_registry: HashMap<NamespacePath, ExportRegistry>,
}

#[derive(Default)]
struct ExportRegistry {
    definitions: HashMap<String, SymbolProvenance<Definition>>,
    extern_functions: HashMap<String, SymbolProvenance<ExternalFunction>>,
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
        // Step 1: Collect exports strictly declared in mod.yawn
        for module in modules {
            for export_rule in &module.exports {
                let target_ns = NamespacePath::from_segments(export_rule.to.clone());
                let registry = self
                    .namespace_registry
                    .entry(target_ns.clone())
                    .or_default();

                for sym in &export_rule.symbols {
                    let mut found_def = None;
                    let mut found_ext = None;

                    for file in &module.source_files {
                        if let Some(def) = file.definitions.iter().find(|d| d.name == *sym) {
                            found_def = Some(SymbolProvenance {
                                item: def.clone(),
                                originating_file: file.file_path.clone(),
                            });
                            break;
                        } else if let Some(ext) =
                            file.extern_functions.iter().find(|e| e.name == *sym)
                        {
                            found_ext = Some(SymbolProvenance {
                                item: ext.clone(),
                                originating_file: file.file_path.clone(),
                            });
                            break;
                        }
                    }

                    if let Some(prov) = found_def {
                        if let Some(existing) = registry.definitions.get(sym) {
                            return Err(format!(
                                "export collision in namespace '{}': symbol '{}' in '{}' conflicts with '{}'",
                                target_ns, sym, prov.originating_file, existing.originating_file
                            ));
                        }
                        registry.definitions.insert(sym.clone(), prov);
                    } else if let Some(prov) = found_ext {
                        if let Some(existing) = registry.extern_functions.get(sym) {
                            return Err(format!(
                                "export collision in namespace '{}': extern '{}' in '{}' conflicts with '{}'",
                                target_ns, sym, prov.originating_file, existing.originating_file
                            ));
                        }
                        registry.extern_functions.insert(sym.clone(), prov);
                    } else {
                        return Err(format!(
                            "export error in '{}/mod.yawn': symbol '{}' is not defined in any .zzz file in this directory",
                            module.dir_path, sym
                        ));
                    }
                }
            }
        }

        let mut resolved_scopes = Vec::new();

        // Step 2: Build isolated module scopes
        for module in modules {
            let mut scope = ModuleScope::new(module.dir_path.clone());

            // Add exports to tracked set
            for export_rule in &module.exports {
                for sym in &export_rule.symbols {
                    scope.exported_symbols.insert(sym.clone());
                }
            }

            // Populate file-local definitions and externs
            for file in &module.source_files {
                for def in &file.definitions {
                    if let Some(existing) = scope.definitions.get(&def.name) {
                        return Err(format!(
                            "duplicate definition of symbol '{}' in '{}', already defined in '{}'",
                            def.name, file.file_path, existing.originating_file
                        ));
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
                        return Err(format!(
                            "duplicate extern function '{}' in '{}', already defined in '{}'",
                            ext.name, file.file_path, existing.originating_file
                        ));
                    }
                    scope.extern_functions.insert(
                        ext.name.clone(),
                        SymbolProvenance {
                            item: ext.clone(),
                            originating_file: file.file_path.clone(),
                        },
                    );
                }
            }

            // Import strictly from namespaces declared in mod.yawn
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

    fn predeclare_scope_signatures(&mut self) {
        for (name, prov) in &self.scope.extern_functions {
            self.resolved_extern_functions
                .insert(name.clone(), prov.item.signature.clone());
        }

        for (name, prov) in &self.scope.definitions {
            if let Expression::Block { signature, .. } = &prov.item.value {
                let mut param_types = Vec::new();
                let mut expected_return = Type::Void;
                let mut is_variadic = false;

                if let Some(sig) = signature {
                    for p in &sig.parameters {
                        param_types.push(p.ty.clone());
                    }
                    expected_return = *sig.return_type.clone();
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

    pub fn check_file(&mut self, file: &SourceFile) -> Result<(), String> {
        self.current_file_path = file.file_path.clone();

        for ext in &file.extern_functions {
            self.resolve_external_function(&ext.name)?;
        }

        for def in &file.definitions {
            self.check_definition_body(def)?;
        }

        Ok(())
    }

    fn check_definition_body(&mut self, def: &Definition) -> Result<(), String> {
        if !self.checked_bodies.insert(def.name.clone()) {
            return Ok(());
        }

        match &def.value {
            Expression::Block { signature, body } => {
                let mut expected_return = Type::Void;

                self.local_scopes.push(HashMap::new());

                if let Some(sig) = signature {
                    let mut seen_params = HashSet::new();
                    for p in &sig.parameters {
                        if !seen_params.insert(p.name.clone()) {
                            self.local_scopes.pop();
                            return Err(format!(
                                "in '{}': duplicate parameter name '{}' in block",
                                self.current_file_path, p.name
                            ));
                        }
                        if p.ty == Type::Void {
                            self.local_scopes.pop();
                            return Err(format!(
                                "in '{}': block parameter '{}' cannot be void",
                                self.current_file_path, p.name
                            ));
                        }
                        self.local_scopes.last_mut().unwrap().insert(
                            p.name.clone(),
                            BindingInfo {
                                ty: p.ty.clone(),
                                is_mutable: false,
                                is_initialized: true,
                            },
                        );
                    }
                    expected_return = *sig.return_type.clone();
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
        if let Some(signature) = self.resolved_extern_functions.get(name) {
            return Ok(signature.clone());
        }

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
            if p.ty == Type::Void {
                return Err(format!(
                    "in '{}': parameter '{}' in extern function '{}' cannot be void",
                    self.current_file_path, p.name, name
                ));
            }
            parameters.push(Parameter {
                name: p.name.clone(),
                ty: p.ty.clone(),
            });
        }

        let signature = Signature {
            parameters,
            return_type: raw.signature.return_type.clone(),
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

        let ty_result = self.type_of_expression(&def.value);
        self.resolving_definitions.remove(name);

        let ty = ty_result?;
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

    fn type_of_expression(&mut self, expression: &Expression) -> Result<Type, String> {
        match expression {
            Expression::Integer(_) => Ok(Type::S32),
            Expression::Boolean(_) => Ok(Type::Bool),
            Expression::Atom(name) => {
                self.intern_atom(name);
                Ok(Type::Atom(name.clone()))
            }
            Expression::String(_) => Ok(Type::Slice {
                element: Box::new(Type::U8),
            }),

            Expression::Identifier(name) => {
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
                        .map(|p| p.ty.clone())
                        .collect();
                    return Ok(Type::Function {
                        parameters: param_types,
                        return_type: ext.signature.return_type.clone(),
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

                for p in &sig.parameters {
                    if !seen_params.insert(p.name.clone()) {
                        return Err(format!(
                            "in '{}': duplicate parameter name '{}' in signature",
                            self.current_file_path, p.name
                        ));
                    }
                    if p.ty == Type::Void {
                        return Err(format!(
                            "in '{}': parameter '{}' cannot be void",
                            self.current_file_path, p.name
                        ));
                    }
                    param_types.push(p.ty.clone());
                }
                Ok(Type::Function {
                    parameters: param_types,
                    return_type: sig.return_type.clone(),
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
                    for p in &sig.parameters {
                        if !seen_params.insert(p.name.clone()) {
                            self.local_scopes.pop();
                            return Err(format!(
                                "in '{}': duplicate parameter name '{}' in block",
                                self.current_file_path, p.name
                            ));
                        }
                        if p.ty == Type::Void {
                            self.local_scopes.pop();
                            return Err(format!(
                                "in '{}': block parameter '{}' cannot be void",
                                self.current_file_path, p.name
                            ));
                        }
                        self.local_scopes.last_mut().unwrap().insert(
                            p.name.clone(),
                            BindingInfo {
                                ty: p.ty.clone(),
                                is_mutable: false,
                                is_initialized: true,
                            },
                        );
                        param_types.push(p.ty.clone());
                    }
                    expected_return = *sig.return_type.clone();
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

                let mut expected_ty: Option<Type> = None;

                for (i, arm) in arms.iter().enumerate() {
                    match &arm.condition {
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
                            if i != arms.len() - 1 {
                                return Err(format!(
                                    "in '{}': fallback 'else' arm must be the final arm in if",
                                    self.current_file_path
                                ));
                            }
                        }
                    }

                    let arm_ty = self.type_of_expression(&arm.body)?;
                    if let Some(target_ty) = &expected_ty {
                        if arm_ty != *target_ty {
                            return Err(format!(
                                "in '{}': mismatched arm types in 'if': expected {:?}, found {:?}",
                                self.current_file_path, target_ty, arm_ty
                            ));
                        }
                    } else {
                        expected_ty = Some(arm_ty);
                    }
                }

                Ok(expected_ty.unwrap_or(Type::Void))
            }

            Expression::Array(elements) => {
                if elements.is_empty() {
                    return Err(format!(
                        "in '{}': cannot infer type of empty array",
                        self.current_file_path
                    ));
                }

                let element_type = self.type_of_expression(&elements[0])?;
                for element in &elements[1..] {
                    let ty = self.type_of_expression(element)?;
                    if ty != element_type {
                        return Err(format!(
                            "in '{}': array elements have mismatched types: expected {:?}, found {:?}",
                            self.current_file_path, element_type, ty
                        ));
                    }
                }

                Ok(Type::Array {
                    element: Box::new(element_type),
                    size: elements.len(),
                })
            }

            Expression::Unary { operator, operand } => {
                let ty = self.type_of_expression(operand)?;
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
                let left_type = self.type_of_expression(left)?;
                let right_type = self.type_of_expression(right)?;

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
                let left_type = self.type_of_expression(left)?;
                let right_type = self.type_of_expression(right)?;

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
                let source_type = self.type_of_expression(expression)?;
                if !Self::is_supported_cast(&source_type, ty) {
                    return Err(format!(
                        "in '{}': unsupported cast from {:?} to {:?}",
                        self.current_file_path, source_type, ty
                    ));
                }
                Ok(ty.clone())
            }

            Expression::Call { target, arguments } => {
                let target_type = self.type_of_expression(target)?;

                let (param_types, return_type, is_variadic) = match &target_type {
                    Type::Function {
                        parameters,
                        return_type,
                        is_variadic,
                    } => (parameters.clone(), *return_type.clone(), *is_variadic),
                    Type::Pointer { element } => match element.as_ref() {
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
                    },
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

                for (arg, expected_ty) in arguments.iter().zip(param_types.iter()) {
                    let arg_ty = self.type_of_expression(arg)?;
                    if !Self::types_compatible(&arg_ty, expected_ty) {
                        return Err(format!(
                            "in '{}': argument type mismatch: expected {:?}, found {:?}",
                            self.current_file_path, expected_ty, arg_ty
                        ));
                    }
                }

                for arg in arguments.iter().skip(param_types.len()) {
                    let _ = self.type_of_expression(arg)?;
                }

                Ok(return_type)
            }

            Expression::Index { expression, index } => {
                let expression_type = self.type_of_expression(expression)?;
                let index_type = self.type_of_expression(index)?;

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
                let expr_type = self.type_of_expression(expression)?;
                match expr_type {
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
                let inner_type = self.type_of_expression(expression)?;
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

    pub fn types_compatible(provided: &Type, expected: &Type) -> bool {
        if provided == expected {
            return true;
        }

        match (provided, expected) {
            (Type::Function { .. }, Type::Pointer { element }) => {
                if let Type::Function { .. } = element.as_ref() {
                    provided == element.as_ref()
                } else {
                    false
                }
            }
            (Type::Pointer { element }, Type::Function { .. }) => {
                if let Type::Function { .. } = element.as_ref() {
                    element.as_ref() == expected
                } else {
                    false
                }
            }
            _ => false,
        }
    }

    fn statement_definitely_returns(stmt: &Statement) -> bool {
        match stmt {
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

    fn validate_statement(&mut self, stmt: &Statement, expected_type: &Type) -> Result<(), String> {
        match stmt {
            Statement::Compound(stmts) => {
                for s in stmts {
                    self.validate_statement(s, expected_type)?;
                }
            }

            Statement::Declare { name, ty } => {
                if *ty == Type::Void {
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
                self.local_scopes.last_mut().unwrap().insert(
                    name.clone(),
                    BindingInfo {
                        ty: ty.clone(),
                        is_mutable: true,
                        is_initialized: false,
                    },
                );
            }

            Statement::Assign { expression, target } => {
                let expr_ty = self.type_of_expression(expression)?;

                if let Some(info) = self.lookup_binding(target) {
                    if !info.is_mutable {
                        return Err(format!(
                            "in '{}': cannot reassign to immutable constant '{}'",
                            self.current_file_path, target
                        ));
                    }
                    if !Self::types_compatible(&expr_ty, &info.ty) {
                        return Err(format!(
                            "in '{}': type mismatch assigning to '{}': expected {:?}, found {:?}",
                            self.current_file_path, target, info.ty, expr_ty
                        ));
                    }
                    if let Some(info_mut) = self.lookup_binding_mut(target) {
                        info_mut.is_initialized = true;
                    }
                } else {
                    self.local_scopes.last_mut().unwrap().insert(
                        target.clone(),
                        BindingInfo {
                            ty: expr_ty,
                            is_mutable: false,
                            is_initialized: true,
                        },
                    );
                }
            }

            Statement::Return(expr) => {
                let actual = self.type_of_expression(expr)?;
                if !Self::types_compatible(&actual, expected_type) {
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
                    if let IfCondition::Expression(cond) = &arm.condition {
                        let cond_ty = self.type_of_expression(cond)?;
                        if cond_ty != Type::Bool {
                            return Err(format!(
                                "in '{}': doif condition must evaluate to Bool, found {:?}",
                                self.current_file_path, cond_ty
                            ));
                        }
                    }
                    self.validate_statement(&arm.body, expected_type)?;
                }
            }
        }
        Ok(())
    }

    fn validate_block(
        &mut self,
        statements: &[Statement],
        expected_type: &Type,
    ) -> Result<(), String> {
        for stmt in statements {
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
