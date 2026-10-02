use std::collections::{HashMap, HashSet};

use crate::ast::{
    Expression, ExternalFunction, Function, FunctionKind, FunctionSignature, Main, Parameter,
    Program, Statement, Type, TypeExpression, Variable,
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

            unsafe_context: false,
        }
    }

    pub fn check(&mut self, program: &Program) -> Result<Program, String> {
        self.resolve_program(program)
    }

    fn resolve_program(&mut self, program: &Program) -> Result<Program, String> {
        let variable_names: Vec<String> = self.variables.keys().cloned().collect();

        for name in variable_names {
            self.resolve_variable(&name)?;
        }

        let extern_names: Vec<String> = self.extern_functions.keys().cloned().collect();

        for name in extern_names {
            self.resolve_external_function(&name)?;
        }

        let function_names: Vec<String> = self.functions.keys().cloned().collect();

        for name in function_names {
            self.resolve_function(&name)?;
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

        let mut main_assigned = HashSet::new();
        let main = Main {
            body: self.resolve_statements(
                &program.main.body,
                &HashMap::new(),
                &mut main_assigned,
            )?,
        };

        let mut functions = Vec::new();

        for function in &program.functions {
            let signature = self.resolve_function(&function.name)?;

            let mut locals = HashMap::new();
            let mut assigned_in_scope = HashSet::new();

            for (parameter, parameter_type) in
                function.parameters.iter().zip(signature.parameters.iter())
            {
                let ty = Self::concrete_type(parameter_type)?;
                locals.insert(parameter.name.clone(), ty);
                assigned_in_scope.insert(parameter.name.clone());
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

            self.unsafe_context = false;

            let body = self.resolve_statements(&function.body, &locals, &mut assigned_in_scope)?;

            functions.push(Function {
                kind: function.kind.clone(),
                name: function.name.clone(),
                parameters,
                return_type: TypeExpression::Concrete(Self::concrete_type(&signature.return_type)?),
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
            let mut locals = HashMap::new();
            let mut resolved_parameters = Vec::new();

            for parameter in &parameters {
                let ty = self.resolve_type_expression(&parameter.ty, &locals)?;

                locals.insert(parameter.name.clone(), ty.clone());

                resolved_parameters.push(TypeExpression::Concrete(ty));
            }

            let resolved_return_type =
                TypeExpression::Concrete(self.resolve_type_expression(&return_type, &locals)?);

            Ok(FunctionSignature {
                parameters: resolved_parameters,
                return_type: resolved_return_type,
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
        locals: &HashMap<String, Type>,
    ) -> Result<Type, String> {
        match type_expression {
            TypeExpression::Concrete(ty) => Ok(ty.clone()),

            TypeExpression::Of(expression) => self.type_of_expression(expression, locals),
        }
    }

    fn type_of_expression(
        &mut self,
        expression: &Expression,
        locals: &HashMap<String, Type>,
    ) -> Result<Type, String> {
        match expression {
            Expression::Integer(_) => Ok(Type::S32),

            Expression::Boolean(_) => Ok(Type::Bool),

            Expression::String(value) => Ok(Type::Array {
                element: Box::new(Type::U8),
                size: value.len(),
            }),

            Expression::Identifier(name) => {
                if let Some(ty) = locals.get(name) {
                    return Ok(ty.clone());
                }

                self.resolve_variable(name)
            }

            Expression::Array(elements) => {
                if elements.is_empty() {
                    return Err("cannot infer type of empty array".to_string());
                }

                let element_type = self.type_of_expression(&elements[0], locals)?;

                for element in &elements[1..] {
                    let ty = self.type_of_expression(element, locals)?;

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
                let ty = self.type_of_expression(operand, locals)?;

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
                let left_type = self.type_of_expression(left, locals)?;

                let right_type = self.type_of_expression(right, locals)?;

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
                let left_type = self.type_of_expression(left, locals)?;

                let right_type = self.type_of_expression(right, locals)?;

                if left_type != right_type {
                    return Err(format!(
                        "comparison operator requires matching types, found {:?} and {:?}",
                        left_type, right_type
                    ));
                }

                Ok(Type::Bool)
            }

            Expression::Cast { expression, ty } => {
                let source_type = self.type_of_expression(expression, locals)?;

                let target_type = self.resolve_type_expression(ty, locals)?;

                if !Self::is_supported_cast(&source_type, &target_type) {
                    return Err(format!(
                        "unsupported cast from {:?} to {:?}",
                        source_type, target_type
                    ));
                }

                Ok(target_type)
            }

            Expression::Call { name, arguments } => {
                let (signature, is_extern) = if self.extern_functions.contains_key(name) {
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

                if is_extern && !self.unsafe_context {
                    return Err(format!(
                        "external function '{}' may only be called from unsafe code",
                        name
                    ));
                }

                if !is_extern
                    && self
                        .functions
                        .get(name)
                        .map(|function| function.kind == FunctionKind::Unsafe)
                        .unwrap_or(false)
                    && !self.unsafe_context
                {
                    return Err(format!(
                        "unsafe function '{}' may only be called from unsafe code",
                        name
                    ));
                }

                if arguments.len() != signature.parameters.len() {
                    return Err(format!(
                        "function '{}' expects {} arguments, found {}",
                        name,
                        signature.parameters.len(),
                        arguments.len()
                    ));
                }

                for (argument, parameter_type) in arguments.iter().zip(signature.parameters.iter())
                {
                    let argument_type = self.type_of_expression(argument, locals)?;

                    let parameter_type = Self::concrete_type(parameter_type)?;

                    if argument_type != parameter_type {
                        return Err(format!(
                            "function '{}' argument type mismatch: expected {:?}, found {:?}",
                            name, parameter_type, argument_type
                        ));
                    }
                }

                Self::concrete_type(&signature.return_type)
            }

            Expression::Index { expression, index } => {
                let expression_type = self.type_of_expression(expression, locals)?;

                let index_type = self.type_of_expression(index, locals)?;

                if !Self::is_integer_type(&index_type) {
                    return Err("array index must be an integer".to_string());
                }

                match expression_type {
                    Type::Array { element, .. } => Ok(*element),

                    other => Err(format!("cannot index value of type {:?}", other)),
                }
            }

            Expression::AddressOf(expression) => match expression.as_ref() {
                Expression::Index { .. } => {
                    let element_type = self.type_of_expression(expression, locals)?;

                    Ok(Type::Pointer {
                        element: Box::new(element_type),
                    })
                }

                Expression::Identifier(name) => {
                    let expression_type = if let Some(ty) = locals.get(name) {
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
        }
    }

    fn resolve_statements(
        &mut self,
        statements: &[Statement],
        locals: &HashMap<String, Type>,
        assigned_names: &mut HashSet<String>,
    ) -> Result<Vec<Statement>, String> {
        statements
            .iter()
            .map(|statement| self.resolve_statement(statement, locals, assigned_names))
            .collect()
    }

    fn resolve_statement(
        &mut self,
        statement: &Statement,
        locals: &HashMap<String, Type>,
        assigned_names: &mut HashSet<String>,
    ) -> Result<Statement, String> {
        match statement {
            Statement::Bind { expression, names } => {
                let resolved_expr = self.resolve_expression(expression, locals)?;

                for name in names {
                    if assigned_names.contains(name) {
                        return Err(format!(
                            "cannot reassign to variable '{}': reassignments are prohibited",
                            name
                        ));
                    }
                    assigned_names.insert(name.clone());
                }

                Ok(Statement::Bind {
                    expression: resolved_expr,
                    names: names.clone(),
                })
            }

            Statement::Return(expression) => Ok(Statement::Return(
                self.resolve_expression(expression, locals)?,
            )),

            Statement::Exit(expression) => Ok(Statement::Exit(
                self.resolve_expression(expression, locals)?,
            )),

            Statement::Unsafe(statements) => {
                if self.unsafe_context {
                    return Err("nested unsafe blocks are not allowed".to_string());
                }

                let previous_context = self.unsafe_context;
                self.unsafe_context = true;

                let result = self.resolve_statements(statements, locals, assigned_names);

                self.unsafe_context = previous_context;

                Ok(Statement::Unsafe(result?))
            }
        }
    }

    fn resolve_expression(
        &mut self,
        expression: &Expression,
        locals: &HashMap<String, Type>,
    ) -> Result<Expression, String> {
        match expression {
            Expression::Integer(value) => Ok(Expression::Integer(*value)),

            Expression::Boolean(value) => Ok(Expression::Boolean(*value)),

            Expression::String(value) => Ok(Expression::String(value.clone())),

            Expression::Identifier(name) => {
                self.type_of_expression(expression, locals)?;

                Ok(Expression::Identifier(name.clone()))
            }

            Expression::Array(elements) => Ok(Expression::Array(
                elements
                    .iter()
                    .map(|element| self.resolve_expression(element, locals))
                    .collect::<Result<Vec<_>, _>>()?,
            )),

            Expression::TypeOf(_) => {
                Err("type expression cannot be used as a runtime value".to_string())
            }

            Expression::Unary { operator, operand } => {
                let resolved_operand = self.resolve_expression(operand, locals)?;

                self.type_of_expression(expression, locals)?;

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
                let resolved_left = self.resolve_expression(left, locals)?;

                let resolved_right = self.resolve_expression(right, locals)?;

                self.type_of_expression(expression, locals)?;

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
                let resolved_left = self.resolve_expression(left, locals)?;

                let resolved_right = self.resolve_expression(right, locals)?;

                self.type_of_expression(expression, locals)?;

                Ok(Expression::Comparison {
                    left: Box::new(resolved_left),
                    operator: operator.clone(),
                    right: Box::new(resolved_right),
                })
            }

            Expression::Cast { expression, ty } => {
                let resolved_expression = self.resolve_expression(expression, locals)?;

                let resolved_type = self.resolve_type_expression(ty, locals)?;

                self.type_of_expression(expression, locals)?;

                Ok(Expression::Cast {
                    expression: Box::new(resolved_expression),
                    ty: TypeExpression::Concrete(resolved_type),
                })
            }

            Expression::Call { name, arguments } => {
                let resolved_arguments = arguments
                    .iter()
                    .map(|argument| self.resolve_expression(argument, locals))
                    .collect::<Result<Vec<_>, _>>()?;

                let _call_type = self.type_of_expression(expression, locals)?;

                Ok(Expression::Call {
                    name: name.clone(),
                    arguments: resolved_arguments,
                })
            }

            Expression::Index { expression, index } => {
                let resolved_expression = self.resolve_expression(expression, locals)?;

                let resolved_index = self.resolve_expression(index, locals)?;

                self.type_of_expression(expression, locals)?;

                Ok(Expression::Index {
                    expression: Box::new(resolved_expression),
                    index: Box::new(resolved_index),
                })
            }

            Expression::AddressOf(expression) => {
                let resolved_expression = self.resolve_expression(expression, locals)?;

                self.type_of_expression(expression, locals)?;

                Ok(Expression::AddressOf(Box::new(resolved_expression)))
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

        Self::is_integer_type(source) && Self::is_integer_type(target)
    }
}
