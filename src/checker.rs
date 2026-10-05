use std::collections::{HashMap, HashSet};

use crate::ast::{
    ConditionalBranch, Expression, ExternalFunction, Function, FunctionKind, FunctionSignature,
    Main, Parameter, Program, Statement, Type, TypeExpression, Variable,
};

struct FunctionInfo {
    kind: FunctionKind,
    parameters: Vec<Parameter>,
    return_type: TypeExpression,
}

pub struct Checker {
    variables: HashMap<String, TypeExpression>,
    extern_functions: HashMap<String, FunctionSignature>,
    functions: HashMap<String, FunctionInfo>,

    resolved_variables: HashMap<String, Type>,
    resolving_variables: HashSet<String>,

    resolved_extern_functions: HashMap<String, FunctionSignature>,
    resolving_extern_functions: HashSet<String>,

    resolved_functions: HashMap<String, FunctionSignature>,
    resolving_functions: HashSet<String>,

    initialized_globals: HashSet<String>,
    unsafe_context: bool,
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

        let functions = program
            .functions
            .iter()
            .map(|function| {
                (
                    function.name.clone(),
                    FunctionInfo {
                        kind: function.kind.clone(),
                        parameters: function.parameters.clone(),
                        return_type: function.return_type.clone(),
                    },
                )
            })
            .collect();

        Self {
            variables,
            extern_functions,
            functions,

            resolved_variables: HashMap::new(),
            resolving_variables: HashSet::new(),

            resolved_extern_functions: HashMap::new(),
            resolving_extern_functions: HashSet::new(),

            resolved_functions: HashMap::new(),
            resolving_functions: HashSet::new(),

            initialized_globals: HashSet::new(),
            unsafe_context: false,
        }
    }

    pub fn check(&mut self, program: &Program) -> Result<Program, String> {
        self.resolve_program(program)
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

        let function_names: Vec<String> = self.functions.keys().cloned().collect();

        for name in &function_names {
            self.resolve_function(name)?;
        }

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

        self.unsafe_context = false;

        let main = Main {
            body: self.resolve_statements(&program.main.body, &HashMap::new())?,
        };

        let mut functions = Vec::new();

        for function in &program.functions {
            let signature = self.resolve_function(&function.name)?;

            let mut params_map = HashMap::new();

            for (parameter, parameter_type) in
                function.parameters.iter().zip(signature.parameters.iter())
            {
                let ty = Self::concrete_type(parameter_type)?;
                params_map.insert(parameter.name.clone(), ty);
            }

            let parameters = function
                .parameters
                .iter()
                .zip(signature.parameters.iter())
                .map(|(parameter, ty)| {
                    Ok(Parameter {
                        name: parameter.name.clone(),
                        ty: TypeExpression::Concrete(Self::concrete_type(ty)?),
                    })
                })
                .collect::<Result<Vec<_>, String>>()?;

            self.unsafe_context = function.kind == FunctionKind::Unsafe;

            let body = self.resolve_statements(&function.body, &params_map)?;

            self.unsafe_context = false;

            functions.push(Function {
                kind: function.kind.clone(),
                name: function.name.clone(),
                parameters,
                return_type: signature.return_type.clone(),
                body,
            });
        }

        Ok(Program {
            variables,
            extern_functions,
            main,
            functions,
        })
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

        let type_expression = self
            .variables
            .get(name)
            .cloned()
            .ok_or_else(|| format!("unknown variable '{}'", name))?;

        let result = self.resolve_type_expression(&type_expression, &HashMap::new());

        self.resolving_variables.remove(name);

        let ty = result?;

        self.resolved_variables.insert(name.to_string(), ty.clone());

        Ok(ty)
    }

    fn resolve_external_function(&mut self, name: &str) -> Result<FunctionSignature, String> {
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

        let raw = self
            .extern_functions
            .get(name)
            .cloned()
            .ok_or_else(|| format!("unknown external function '{}'", name))?;

        let result: Result<FunctionSignature, String> = (|| {
            let parameters = raw
                .parameters
                .iter()
                .map(|ty| {
                    Ok(TypeExpression::Concrete(
                        self.resolve_type_expression(ty, &HashMap::new())?,
                    ))
                })
                .collect::<Result<Vec<_>, String>>()?;

            let return_type = TypeExpression::Concrete(
                self.resolve_type_expression(&raw.return_type, &HashMap::new())?,
            );

            Ok(FunctionSignature {
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

    fn resolve_function(&mut self, name: &str) -> Result<FunctionSignature, String> {
        if let Some(signature) = self.resolved_functions.get(name) {
            return Ok(signature.clone());
        }

        if !self.functions.contains_key(name) {
            return Err(format!("unknown function '{}'", name));
        }

        if !self.resolving_functions.insert(name.to_string()) {
            return Err(format!(
                "cyclic type expression involving function '{}'",
                name
            ));
        }

        let (parameters, return_type) = {
            let function = self
                .functions
                .get(name)
                .ok_or_else(|| format!("unknown function '{}'", name))?;

            (function.parameters.clone(), function.return_type.clone())
        };

        let result: Result<FunctionSignature, String> = (|| {
            let mut params_map = HashMap::new();
            let mut resolved_parameters = Vec::new();

            for parameter in &parameters {
                let ty = self.resolve_type_expression(&parameter.ty, &params_map)?;

                params_map.insert(parameter.name.clone(), ty.clone());

                resolved_parameters.push(TypeExpression::Concrete(ty));
            }

            let resolved_return_type =
                TypeExpression::Concrete(self.resolve_type_expression(&return_type, &params_map)?);

            Ok(FunctionSignature {
                parameters: resolved_parameters,
                return_type: resolved_return_type,
                is_variadic: false,
            })
        })();

        self.resolving_functions.remove(name);

        let signature = result?;

        self.resolved_functions
            .insert(name.to_string(), signature.clone());

        Ok(signature)
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

                self.resolve_variable(name)
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
                            "array elements have different types: expected {:?}, found {:?}",
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

            Expression::Comparison {
                left,
                operator: _,
                right,
            } => {
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

            Expression::Call { name, arguments } => {
                let (signature, is_unsafe) = if self.extern_functions.contains_key(name) {
                    (self.resolve_external_function(name)?, true)
                } else {
                    let function_kind = self
                        .functions
                        .get(name)
                        .map(|function| function.kind.clone())
                        .ok_or_else(|| format!("unknown function '{}'", name))?;

                    (
                        self.resolve_function(name)?,
                        function_kind == FunctionKind::Unsafe,
                    )
                };

                if is_unsafe && !self.unsafe_context {
                    return Err(format!(
                        "function '{}' is unsafe and may only be called from unsafe code",
                        name
                    ));
                }

                if signature.is_variadic {
                    if arguments.len() < signature.parameters.len() {
                        return Err(format!(
                            "variadic function '{}' expects at least {} arguments, found {}",
                            name,
                            signature.parameters.len(),
                            arguments.len()
                        ));
                    }
                } else {
                    if arguments.len() != signature.parameters.len() {
                        return Err(format!(
                            "function '{}' expects {} arguments, found {}",
                            name,
                            signature.parameters.len(),
                            arguments.len()
                        ));
                    }
                }

                for (argument, parameter_type) in arguments.iter().zip(signature.parameters.iter())
                {
                    let argument_type = self.type_of_expression(argument, params)?;
                    let parameter_type = Self::concrete_type(parameter_type)?;

                    if argument_type != parameter_type {
                        return Err(format!(
                            "function '{}' argument type mismatch: expected {:?}, found {:?}",
                            name, parameter_type, argument_type
                        ));
                    }
                }

                for argument in arguments.iter().skip(signature.parameters.len()) {
                    let _var_type = self.type_of_expression(argument, params)?;
                }

                Self::concrete_type(&signature.return_type)
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

            Statement::Exit(expression) => Ok(Statement::Exit(
                self.resolve_expression(expression, params)?,
            )),

            Statement::Unsafe(statements) => {
                if self.unsafe_context {
                    return Err("nested unsafe blocks are not allowed".to_string());
                }

                let previous_context = self.unsafe_context;
                self.unsafe_context = true;

                let result = self.resolve_statements(statements, params);

                self.unsafe_context = previous_context;

                Ok(Statement::Unsafe(result?))
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

            Expression::Call { name, arguments } => {
                let resolved_arguments = arguments
                    .iter()
                    .map(|argument| self.resolve_expression(argument, params))
                    .collect::<Result<Vec<_>, _>>()?;

                let _call_type = self.type_of_expression(expression, params)?;

                Ok(Expression::Call {
                    name: name.clone(),
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
