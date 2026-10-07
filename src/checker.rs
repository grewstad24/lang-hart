use std::collections::{HashMap, HashSet};

use crate::ast::{
    ConditionalBranch, Definition, Expression, ExternalFunction, Parameter, Program, Signature,
    Statement, Type, TypeExpression, Variable,
};

pub struct Checker {
    variables: HashMap<String, TypeExpression>,
    extern_functions: HashMap<String, Signature>,
    definitions: HashMap<String, Expression>,

    resolved_variables: HashMap<String, Type>,
    resolving_variables: HashSet<String>,

    resolved_extern_functions: HashMap<String, Signature>,
    resolving_extern_functions: HashSet<String>,

    resolved_definitions: HashMap<String, Type>,
    resolving_definitions: HashSet<String>,

    initialized_globals: HashSet<String>,
}

impl Checker {
    pub fn new(program: &Program) -> Self {
        let variables = program
            .variables
            .iter()
            .map(|variable| (variable.name.clone(), variable.ty.clone()))
            .collect();

        let extern_functions = program
            .extern_functions
            .iter()
            .map(|external| (external.name.clone(), external.signature.clone()))
            .collect();

        let definitions = program
            .definitions
            .iter()
            .map(|def| (def.name.clone(), def.value.clone()))
            .collect();

        Self {
            variables,
            extern_functions,
            definitions,

            resolved_variables: HashMap::new(),
            resolving_variables: HashSet::new(),

            resolved_extern_functions: HashMap::new(),
            resolving_extern_functions: HashSet::new(),

            resolved_definitions: HashMap::new(),
            resolving_definitions: HashSet::new(),

            initialized_globals: HashSet::new(),
        }
    }

    pub fn check(&mut self, program: &Program) -> Result<Program, String> {
        self.validate_unique_declarations(program)?;
        self.resolve_program(program)
    }

    fn validate_unique_declarations(&self, program: &Program) -> Result<(), String> {
        let mut seen_symbols = HashSet::new();

        for var in &program.variables {
            if !seen_symbols.insert(var.name.clone()) {
                return Err(format!(
                    "duplicate declaration of global variable '{}'",
                    var.name
                ));
            }
        }

        for ext in &program.extern_functions {
            if !seen_symbols.insert(ext.name.clone()) {
                return Err(format!(
                    "symbol conflict: extern '{}' already defined",
                    ext.name
                ));
            }
        }

        for def in &program.definitions {
            if !seen_symbols.insert(def.name.clone()) {
                return Err(format!(
                    "symbol conflict: definition '@{}' already defined",
                    def.name
                ));
            }
        }

        Ok(())
    }

    fn resolve_program(&mut self, program: &Program) -> Result<Program, String> {
        let variable_names: Vec<String> = self.variables.keys().cloned().collect();
        for name in &variable_names {
            self.resolve_variable(name)?;
        }

        let extern_names: Vec<String> = self.extern_functions.keys().cloned().collect();
        for name in &extern_names {
            self.resolve_external_function(name)?;
        }

        let definition_names: Vec<String> = self.definitions.keys().cloned().collect();
        for name in &definition_names {
            self.resolve_definition(name)?;
        }

        self.validate_main()?;

        let variables = program
            .variables
            .iter()
            .map(|variable| {
                Ok(Variable {
                    name: variable.name.clone(),
                    ty: TypeExpression::Concrete(self.resolve_variable(&variable.name)?),
                })
            })
            .collect::<Result<Vec<_>, String>>()?;

        let extern_functions = program
            .extern_functions
            .iter()
            .map(|external| {
                Ok(ExternalFunction {
                    name: external.name.clone(),
                    signature: self.resolve_external_function(&external.name)?,
                })
            })
            .collect::<Result<Vec<_>, String>>()?;

        let mut definitions = Vec::new();
        for def in &program.definitions {
            let resolved_val = self.resolve_expression(&def.value, &HashMap::new())?;
            definitions.push(Definition {
                name: def.name.clone(),
                value: resolved_val,
            });
        }

        Ok(Program {
            variables,
            extern_functions,
            definitions,
        })
    }

    fn validate_main(&self) -> Result<(), String> {
        let main_type = self.resolved_definitions.get("main").ok_or_else(|| {
            "program is malformed: missing required entry point '@main'".to_string()
        })?;

        match main_type {
            Type::Function {
                parameters,
                return_type,
                is_variadic,
            } => {
                if !parameters.is_empty() {
                    return Err(format!(
                        "entry point '@main' cannot accept parameters, found {}",
                        parameters.len()
                    ));
                }
                if *is_variadic {
                    return Err("entry point '@main' cannot be variadic".to_string());
                }
                if **return_type != Type::Void && **return_type != Type::S32 {
                    return Err(format!(
                        "entry point '@main' must return void or s32, found {:?}",
                        return_type
                    ));
                }
            }
            _ => {
                return Err("entry point '@main' must evaluate to a function/block".to_string());
            }
        }

        Ok(())
    }

    fn resolve_variable(&mut self, name: &str) -> Result<Type, String> {
        if let Some(ty) = self.resolved_variables.get(name) {
            return Ok(ty.clone());
        }

        if !self.variables.contains_key(name) {
            return Err(format!("unknown variable '{}'", name));
        }

        if !self.resolving_variables.insert(name.to_string()) {
            return Err(format!(
                "cyclic type expression involving variable '{}'",
                name
            ));
        }

        let type_expression = self.variables.get(name).cloned().unwrap();
        let result = self.resolve_type_expression(&type_expression, &HashMap::new());
        self.resolving_variables.remove(name);

        let ty = result?;
        if ty == Type::Void {
            return Err(format!("global variable '{}' cannot be of type void", name));
        }

        self.resolved_variables.insert(name.to_string(), ty.clone());
        Ok(ty)
    }

    fn resolve_external_function(&mut self, name: &str) -> Result<Signature, String> {
        if let Some(signature) = self.resolved_extern_functions.get(name) {
            return Ok(signature.clone());
        }

        if !self.extern_functions.contains_key(name) {
            return Err(format!("unknown external function '{}'", name));
        }

        if !self.resolving_extern_functions.insert(name.to_string()) {
            return Err(format!(
                "cyclic type expression involving external function '{}'",
                name
            ));
        }

        let raw = self.extern_functions.get(name).cloned().unwrap();

        let result: Result<Signature, String> = (|| {
            let mut seen_params = HashSet::new();
            let mut parameters = Vec::new();

            for p in &raw.parameters {
                if !seen_params.insert(p.name.clone()) {
                    return Err(format!(
                        "duplicate parameter name '{}' in extern function '{}'",
                        p.name, name
                    ));
                }
                let param_type = self.resolve_type_expression(&p.ty, &HashMap::new())?;
                if param_type == Type::Void {
                    return Err(format!(
                        "parameter '{}' in extern function '{}' cannot be void",
                        p.name, name
                    ));
                }
                parameters.push(Parameter {
                    name: p.name.clone(),
                    ty: TypeExpression::Concrete(param_type),
                });
            }

            let return_type = Box::new(TypeExpression::Concrete(
                self.resolve_type_expression(&raw.return_type, &HashMap::new())?,
            ));

            Ok(Signature {
                parameters,
                return_type,
                is_variadic: raw.is_variadic,
            })
        })();

        self.resolving_extern_functions.remove(name);
        let signature = result?;
        self.resolved_extern_functions
            .insert(name.to_string(), signature.clone());

        Ok(signature)
    }

    fn resolve_definition(&mut self, name: &str) -> Result<Type, String> {
        if let Some(ty) = self.resolved_definitions.get(name) {
            return Ok(ty.clone());
        }

        if !self.definitions.contains_key(name) {
            return Err(format!("unknown definition '@{}'", name));
        }

        if !self.resolving_definitions.insert(name.to_string()) {
            return Err(format!(
                "cyclic dependency involving definition '@{}'",
                name
            ));
        }

        let expr = self.definitions.get(name).cloned().unwrap();
        let ty_result = self.type_of_expression(&expr, &HashMap::new());
        self.resolving_definitions.remove(name);

        let ty = ty_result?;
        self.resolved_definitions
            .insert(name.to_string(), ty.clone());
        Ok(ty)
    }

    fn resolve_type_expression(
        &mut self,
        type_expression: &TypeExpression,
        params: &HashMap<String, Type>,
    ) -> Result<Type, String> {
        match type_expression {
            TypeExpression::Concrete(ty) => Ok(ty.clone()),
            TypeExpression::Of(expression) => self.type_of_expression(expression, params),
        }
    }

    fn type_of_expression(
        &mut self,
        expression: &Expression,
        params: &HashMap<String, Type>,
    ) -> Result<Type, String> {
        match expression {
            Expression::Integer(_) => Ok(Type::S32),
            Expression::Boolean(_) => Ok(Type::Bool),
            Expression::String(_) => Ok(Type::Slice {
                element: Box::new(Type::U8),
            }),

            Expression::Identifier(name) => {
                if let Some(ty) = params.get(name) {
                    return Ok(ty.clone());
                }
                if self.variables.contains_key(name) {
                    return self.resolve_variable(name);
                }
                if self.definitions.contains_key(name) {
                    return self.resolve_definition(name);
                }
                if let Some(ext) = self.extern_functions.get(name).cloned() {
                    let param_types = ext
                        .parameters
                        .iter()
                        .map(|p| self.resolve_type_expression(&p.ty, params))
                        .collect::<Result<Vec<_>, _>>()?;
                    let ret = self.resolve_type_expression(&ext.return_type, params)?;
                    return Ok(Type::Function {
                        parameters: param_types,
                        return_type: Box::new(ret),
                        is_variadic: ext.is_variadic,
                    });
                }
                Err(format!("unknown identifier '{}'", name))
            }

            Expression::Signature(sig) => {
                let mut param_types = Vec::new();
                let mut seen_params = HashSet::new();

                for p in &sig.parameters {
                    if !seen_params.insert(p.name.clone()) {
                        return Err(format!(
                            "duplicate parameter name '{}' in signature",
                            p.name
                        ));
                    }
                    let p_ty = self.resolve_type_expression(&p.ty, params)?;
                    if p_ty == Type::Void {
                        return Err(format!("parameter '{}' cannot be void", p.name));
                    }
                    param_types.push(p_ty);
                }
                let ret = self.resolve_type_expression(&sig.return_type, params)?;
                Ok(Type::Function {
                    parameters: param_types,
                    return_type: Box::new(ret),
                    is_variadic: sig.is_variadic,
                })
            }

            Expression::Block { signature, body } => {
                let mut local_params = params.clone();
                let mut param_types = Vec::new();
                let mut expected_return = Type::Void;
                let mut is_variadic = false;

                if let Some(sig) = signature {
                    let mut seen_params = HashSet::new();
                    for p in &sig.parameters {
                        if !seen_params.insert(p.name.clone()) {
                            return Err(format!("duplicate parameter name '{}' in block", p.name));
                        }
                        let p_ty = self.resolve_type_expression(&p.ty, params)?;
                        if p_ty == Type::Void {
                            return Err(format!("block parameter '{}' cannot be void", p.name));
                        }
                        local_params.insert(p.name.clone(), p_ty.clone());
                        param_types.push(p_ty);
                    }
                    expected_return = self.resolve_type_expression(&sig.return_type, params)?;
                    is_variadic = sig.is_variadic;
                }

                self.validate_block_return_types(body, &local_params, &expected_return)?;

                Ok(Type::Function {
                    parameters: param_types,
                    return_type: Box::new(expected_return),
                    is_variadic,
                })
            }

            Expression::Array(elements) => {
                if elements.is_empty() {
                    return Err("cannot infer type of empty array".to_string());
                }

                let element_type = self.type_of_expression(&elements[0], params)?;
                for element in &elements[1..] {
                    let ty = self.type_of_expression(element, params)?;
                    if ty != element_type {
                        return Err(format!(
                            "array elements have mismatched types: expected {:?}, found {:?}",
                            element_type, ty
                        ));
                    }
                }

                Ok(Type::Array {
                    element: Box::new(element_type),
                    size: elements.len(),
                })
            }

            Expression::TypeOf(_) => {
                Err("type expression cannot be used as a runtime value".to_string())
            }

            Expression::Unary { operator, operand } => {
                let ty = self.type_of_expression(operand, params)?;
                if !Self::is_integer_type(&ty) {
                    return Err(format!(
                        "unary operator {:?} requires an integer operand, found {:?}",
                        operator, ty
                    ));
                }
                Ok(ty)
            }

            Expression::Binary {
                left,
                operator,
                right,
            } => {
                let left_type = self.type_of_expression(left, params)?;
                let right_type = self.type_of_expression(right, params)?;

                if !Self::is_integer_type(&left_type) || !Self::is_integer_type(&right_type) {
                    return Err(format!(
                        "binary operator {:?} requires integer operands",
                        operator
                    ));
                }

                if left_type != right_type {
                    return Err(format!(
                        "binary operator {:?} requires matching types, found {:?} and {:?}",
                        operator, left_type, right_type
                    ));
                }

                Ok(left_type)
            }

            Expression::Comparison { left, right, .. } => {
                let left_type = self.type_of_expression(left, params)?;
                let right_type = self.type_of_expression(right, params)?;

                if left_type != right_type {
                    return Err(format!(
                        "comparison operator requires matching types, found {:?} and {:?}",
                        left_type, right_type
                    ));
                }

                Ok(Type::Bool)
            }

            Expression::Cast { expression, ty } => {
                let source_type = self.type_of_expression(expression, params)?;
                let target_type = self.resolve_type_expression(ty, params)?;

                if !Self::is_supported_cast(&source_type, &target_type) {
                    return Err(format!(
                        "unsupported cast from {:?} to {:?}",
                        source_type, target_type
                    ));
                }

                Ok(target_type)
            }

            Expression::Call { target, arguments } => {
                let target_type = self.type_of_expression(target, params)?;

                let (param_types, return_type, is_variadic) = match &target_type {
                    Type::Function {
                        parameters,
                        return_type,
                        is_variadic,
                    } => (parameters.clone(), *return_type.clone(), *is_variadic),
                    _ => {
                        return Err(format!("cannot call non-function type {:?}", target_type));
                    }
                };

                if is_variadic {
                    if arguments.len() < param_types.len() {
                        return Err(format!(
                            "variadic call expects at least {} arguments, found {}",
                            param_types.len(),
                            arguments.len()
                        ));
                    }
                } else if arguments.len() != param_types.len() {
                    return Err(format!(
                        "call expects {} arguments, found {}",
                        param_types.len(),
                        arguments.len()
                    ));
                }

                for (arg, expected_ty) in arguments.iter().zip(param_types.iter()) {
                    let arg_ty = self.type_of_expression(arg, params)?;
                    if arg_ty != *expected_ty {
                        return Err(format!(
                            "argument type mismatch: expected {:?}, found {:?}",
                            expected_ty, arg_ty
                        ));
                    }
                }

                for arg in arguments.iter().skip(param_types.len()) {
                    let _ = self.type_of_expression(arg, params)?;
                }

                Ok(return_type)
            }

            Expression::Index { expression, index } => {
                let expression_type = self.type_of_expression(expression, params)?;
                let index_type = self.type_of_expression(index, params)?;

                if !Self::is_integer_type(&index_type) {
                    return Err("array index must be an integer".to_string());
                }

                match expression_type {
                    Type::Array { element, .. } => Ok(*element),
                    Type::Slice { element } => Ok(*element),
                    other => Err(format!("cannot index value of type {:?}", other)),
                }
            }

            Expression::Member { expression, field } => {
                let expr_type = self.type_of_expression(expression, params)?;
                match expr_type {
                    Type::Slice { element } => match field.as_str() {
                        "ptr" => Ok(Type::Pointer { element }),
                        "len" => Ok(Type::U64),
                        _ => Err(format!("unknown slice member '.{}'", field)),
                    },
                    Type::Array { element, .. } => match field.as_str() {
                        "ptr" => Ok(Type::Pointer { element }),
                        "len" => Ok(Type::U64),
                        _ => Err(format!("unknown array member '.{}'", field)),
                    },
                    other => Err(format!(
                        "member access '.{}' unsupported on type {:?}",
                        field, other
                    )),
                }
            }

            Expression::AddressOf(expression) => match expression.as_ref() {
                Expression::Index { .. } => {
                    let element_type = self.type_of_expression(expression, params)?;
                    Ok(Type::Pointer {
                        element: Box::new(element_type),
                    })
                }
                Expression::Identifier(name) => {
                    let expression_type = if let Some(ty) = params.get(name) {
                        ty.clone()
                    } else {
                        self.resolve_variable(name)?
                    };
                    Ok(Type::Pointer {
                        element: Box::new(expression_type),
                    })
                }
                _ => Err("address-of requires a variable or array index".to_string()),
            },

            Expression::Decay(expression) => {
                let inner_type = self.type_of_expression(expression, params)?;
                match inner_type {
                    Type::Array { element, .. } => Ok(Type::Pointer { element }),
                    Type::Slice { element } => Ok(Type::Pointer { element }),
                    Type::Pointer { .. } => Ok(inner_type),
                    other => Err(format!(
                        "decay operator '^' expects an array or slice type, found {:?}",
                        other
                    )),
                }
            }
        }
    }

    fn validate_block_return_types(
        &mut self,
        statements: &[Statement],
        params: &HashMap<String, Type>,
        expected_type: &Type,
    ) -> Result<(), String> {
        for stmt in statements {
            match stmt {
                Statement::Return(expr) => {
                    let actual = self.type_of_expression(expr, params)?;
                    if actual != *expected_type {
                        return Err(format!(
                            "return type mismatch: expected {:?}, found {:?}",
                            expected_type, actual
                        ));
                    }
                }
                Statement::If {
                    branches,
                    else_body,
                } => {
                    for b in branches {
                        self.validate_block_return_types(&b.body, params, expected_type)?;
                    }
                    if let Some(eb) = else_body {
                        self.validate_block_return_types(eb, params, expected_type)?;
                    }
                }
                _ => {}
            }
        }
        Ok(())
    }

    fn resolve_statements(
        &mut self,
        statements: &[Statement],
        params: &HashMap<String, Type>,
    ) -> Result<Vec<Statement>, String> {
        let mut resolved = Vec::new();
        for statement in statements {
            resolved.push(self.resolve_statement(statement, params)?);
        }
        Ok(resolved)
    }

    fn resolve_statement(
        &mut self,
        statement: &Statement,
        params: &HashMap<String, Type>,
    ) -> Result<Statement, String> {
        match statement {
            Statement::Bind { expression, names } => {
                let resolved_expr = self.resolve_expression(expression, params)?;
                let expr_type = self.type_of_expression(&resolved_expr, params)?;

                for name in names {
                    if !self.variables.contains_key(name) {
                        return Err(format!(
                            "unknown variable '{}': only declared globals may be bound",
                            name
                        ));
                    }

                    if self.initialized_globals.contains(name) {
                        return Err(format!(
                            "cannot reassign to global variable '{}': globals may only be written once",
                            name
                        ));
                    }
                }

                if names.len() == 1 {
                    let name = &names[0];
                    let var_type = self.resolve_variable(name)?;

                    if expr_type != var_type {
                        return Err(format!(
                            "type mismatch binding to global '{}': expected {:?}, found {:?}",
                            name, var_type, expr_type
                        ));
                    }

                    self.initialized_globals.insert(name.clone());
                } else {
                    match expr_type {
                        Type::Array { element, size } => {
                            if size != names.len() {
                                return Err(format!(
                                    "cannot unpack array of size {} into {} bindings",
                                    size,
                                    names.len()
                                ));
                            }

                            for name in names {
                                let var_type = self.resolve_variable(name)?;
                                if *element != var_type {
                                    return Err(format!(
                                        "type mismatch unpacking into global '{}': expected {:?}, found {:?}",
                                        name, var_type, *element
                                    ));
                                }
                                self.initialized_globals.insert(name.clone());
                            }
                        }
                        other => {
                            return Err(format!(
                                "cannot unpack value of non-array type {:?} into multiple bindings",
                                other
                            ));
                        }
                    }
                }

                Ok(Statement::Bind {
                    expression: resolved_expr,
                    names: names.clone(),
                })
            }

            Statement::Discard(expression) => {
                let resolved_expr = self.resolve_expression(expression, params)?;
                self.type_of_expression(&resolved_expr, params)?;
                Ok(Statement::Discard(resolved_expr))
            }

            Statement::Return(expression) => Ok(Statement::Return(
                self.resolve_expression(expression, params)?,
            )),

            Statement::Exit(expression) => {
                let resolved_expr = self.resolve_expression(expression, params)?;
                let ty = self.type_of_expression(&resolved_expr, params)?;
                if !Self::is_integer_type(&ty) {
                    return Err(format!("exit code must be an integer, found {:?}", ty));
                }
                Ok(Statement::Exit(resolved_expr))
            }

            Statement::If {
                branches,
                else_body,
            } => {
                let mut resolved_branches = Vec::new();
                let mut branch_initialized_sets = Vec::new();
                let initial_state = self.initialized_globals.clone();

                for branch in branches {
                    let resolved_condition = self.resolve_expression(&branch.condition, params)?;
                    let cond_type = self.type_of_expression(&resolved_condition, params)?;

                    if cond_type != Type::Bool {
                        return Err(format!(
                            "if condition must be of type Bool, found {:?}",
                            cond_type
                        ));
                    }

                    self.initialized_globals = initial_state.clone();
                    let resolved_body = self.resolve_statements(&branch.body, params)?;
                    branch_initialized_sets.push(self.initialized_globals.clone());

                    resolved_branches.push(ConditionalBranch {
                        condition: resolved_condition,
                        body: resolved_body,
                    });
                }

                let resolved_else_body = if let Some(body) = else_body {
                    self.initialized_globals = initial_state.clone();
                    let resolved = self.resolve_statements(body, params)?;
                    branch_initialized_sets.push(self.initialized_globals.clone());
                    Some(resolved)
                } else {
                    None
                };

                if else_body.is_some() && !branch_initialized_sets.is_empty() {
                    let mut common = branch_initialized_sets[0].clone();
                    for s in &branch_initialized_sets[1..] {
                        common.retain(|k| s.contains(k));
                    }
                    self.initialized_globals = common;
                } else {
                    self.initialized_globals = initial_state;
                }

                Ok(Statement::If {
                    branches: resolved_branches,
                    else_body: resolved_else_body,
                })
            }
        }
    }

    fn resolve_expression(
        &mut self,
        expression: &Expression,
        params: &HashMap<String, Type>,
    ) -> Result<Expression, String> {
        match expression {
            Expression::Integer(value) => Ok(Expression::Integer(*value)),
            Expression::Boolean(value) => Ok(Expression::Boolean(*value)),
            Expression::String(value) => Ok(Expression::String(value.clone())),

            Expression::Identifier(name) => {
                self.type_of_expression(expression, params)?;
                Ok(Expression::Identifier(name.clone()))
            }

            Expression::Signature(sig) => {
                let mut resolved_params = Vec::new();
                for p in &sig.parameters {
                    let ty = self.resolve_type_expression(&p.ty, params)?;
                    resolved_params.push(Parameter {
                        name: p.name.clone(),
                        ty: TypeExpression::Concrete(ty),
                    });
                }
                let ret_ty = self.resolve_type_expression(&sig.return_type, params)?;
                Ok(Expression::Signature(Signature {
                    parameters: resolved_params,
                    return_type: Box::new(TypeExpression::Concrete(ret_ty)),
                    is_variadic: sig.is_variadic,
                }))
            }

            Expression::Block { signature, body } => {
                let mut local_params = params.clone();
                let resolved_sig = if let Some(sig) = signature {
                    let mut resolved_params = Vec::new();
                    for p in &sig.parameters {
                        let ty = self.resolve_type_expression(&p.ty, params)?;
                        local_params.insert(p.name.clone(), ty.clone());
                        resolved_params.push(Parameter {
                            name: p.name.clone(),
                            ty: TypeExpression::Concrete(ty),
                        });
                    }
                    let ret_ty = self.resolve_type_expression(&sig.return_type, params)?;
                    Some(Signature {
                        parameters: resolved_params,
                        return_type: Box::new(TypeExpression::Concrete(ret_ty)),
                        is_variadic: sig.is_variadic,
                    })
                } else {
                    None
                };

                let resolved_body = self.resolve_statements(body, &local_params)?;

                Ok(Expression::Block {
                    signature: resolved_sig,
                    body: resolved_body,
                })
            }

            Expression::Array(elements) => Ok(Expression::Array(
                elements
                    .iter()
                    .map(|element| self.resolve_expression(element, params))
                    .collect::<Result<Vec<_>, _>>()?,
            )),

            Expression::TypeOf(_) => {
                Err("type expression cannot be used as a runtime value".to_string())
            }

            Expression::Unary { operator, operand } => {
                let resolved_operand = self.resolve_expression(operand, params)?;
                self.type_of_expression(expression, params)?;
                Ok(Expression::Unary {
                    operator: operator.clone(),
                    operand: Box::new(resolved_operand),
                })
            }

            Expression::Binary {
                left,
                operator,
                right,
            } => {
                let resolved_left = self.resolve_expression(left, params)?;
                let resolved_right = self.resolve_expression(right, params)?;
                self.type_of_expression(expression, params)?;
                Ok(Expression::Binary {
                    left: Box::new(resolved_left),
                    operator: operator.clone(),
                    right: Box::new(resolved_right),
                })
            }

            Expression::Comparison {
                left,
                operator,
                right,
            } => {
                let resolved_left = self.resolve_expression(left, params)?;
                let resolved_right = self.resolve_expression(right, params)?;
                self.type_of_expression(expression, params)?;
                Ok(Expression::Comparison {
                    left: Box::new(resolved_left),
                    operator: operator.clone(),
                    right: Box::new(resolved_right),
                })
            }

            Expression::Cast { expression, ty } => {
                let resolved_expression = self.resolve_expression(expression, params)?;
                let resolved_type = self.resolve_type_expression(ty, params)?;
                self.type_of_expression(expression, params)?;
                Ok(Expression::Cast {
                    expression: Box::new(resolved_expression),
                    ty: TypeExpression::Concrete(resolved_type),
                })
            }

            Expression::Call { target, arguments } => {
                let resolved_target = self.resolve_expression(target, params)?;
                let resolved_arguments = arguments
                    .iter()
                    .map(|argument| self.resolve_expression(argument, params))
                    .collect::<Result<Vec<_>, _>>()?;

                let _call_type = self.type_of_expression(expression, params)?;

                Ok(Expression::Call {
                    target: Box::new(resolved_target),
                    arguments: resolved_arguments,
                })
            }

            Expression::Index { expression, index } => {
                let resolved_expression = self.resolve_expression(expression, params)?;
                let resolved_index = self.resolve_expression(index, params)?;
                self.type_of_expression(expression, params)?;

                Ok(Expression::Index {
                    expression: Box::new(resolved_expression),
                    index: Box::new(resolved_index),
                })
            }

            Expression::Member { expression, field } => {
                let resolved_expression = self.resolve_expression(expression, params)?;
                self.type_of_expression(expression, params)?;

                Ok(Expression::Member {
                    expression: Box::new(resolved_expression),
                    field: field.clone(),
                })
            }

            Expression::AddressOf(expression) => {
                let resolved_expression = self.resolve_expression(expression, params)?;
                self.type_of_expression(expression, params)?;
                Ok(Expression::AddressOf(Box::new(resolved_expression)))
            }

            Expression::Decay(expression) => {
                let resolved_expression = self.resolve_expression(expression, params)?;
                self.type_of_expression(expression, params)?;
                Ok(Expression::Decay(Box::new(resolved_expression)))
            }
        }
    }

    pub fn concrete_type(type_expression: &TypeExpression) -> Result<Type, String> {
        match type_expression {
            TypeExpression::Concrete(ty) => Ok(ty.clone()),
            TypeExpression::Of(_) => {
                Err("unresolved type expression reached code generation".to_string())
            }
        }
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

        if (*source == Type::Bool && Self::is_integer_type(target))
            || (Self::is_integer_type(source) && *target == Type::Bool)
        {
            return true;
        }

        if (matches!(source, Type::Pointer { .. }) && Self::is_integer_type(target))
            || (Self::is_integer_type(source) && matches!(target, Type::Pointer { .. }))
        {
            return true;
        }

        if Self::is_integer_type(source) && Self::is_integer_type(target) {
            return true;
        }

        false
    }
}
