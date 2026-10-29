use std::collections::{HashMap, HashSet};

use crate::ast::{
    ComparisonOperator, Definition, DoIfArm, Expression, ExternalFunction, IfArm, IfCondition,
    Parameter, Program, Signature, Statement, Type,
};

#[derive(Debug, Clone)]
struct BindingInfo {
    ty: Type,
    is_mutable: bool,
    is_initialized: bool,
}

pub struct Checker {
    extern_functions: HashMap<String, Signature>,
    definitions: HashMap<String, Expression>,

    resolved_extern_functions: HashMap<String, Signature>,
    resolving_extern_functions: HashSet<String>,

    resolved_definitions: HashMap<String, Type>,
    resolving_definitions: HashSet<String>,

    scopes: Vec<HashMap<String, BindingInfo>>,
    atom_table: HashMap<String, u32>,
}

impl Checker {
    pub fn new(program: &Program) -> Self {
        let extern_functions = program
            .extern_functions
            .iter()
            .map(|ext| (ext.name.clone(), ext.signature.clone()))
            .collect();

        let definitions = program
            .definitions
            .iter()
            .map(|def| (def.name.clone(), def.value.clone()))
            .collect();

        Self {
            extern_functions,
            definitions,

            resolved_extern_functions: HashMap::new(),
            resolving_extern_functions: HashSet::new(),

            resolved_definitions: HashMap::new(),
            resolving_definitions: HashSet::new(),

            scopes: Vec::new(),
            atom_table: HashMap::new(),
        }
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

    pub fn check(&mut self, program: &Program) -> Result<Program, String> {
        self.validate_unique_declarations(program)?;
        self.resolve_program(program)
    }

    fn validate_unique_declarations(&self, program: &Program) -> Result<(), String> {
        let mut seen_symbols = HashSet::new();

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

    fn declare_definitions(&mut self, program: &Program) -> Result<(), String> {
        for def in &program.definitions {
            if let Expression::Block { signature, .. } = &def.value {
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
                self.resolved_definitions.insert(def.name.clone(), fn_ty);
            }
        }
        Ok(())
    }

    fn resolve_program(&mut self, program: &Program) -> Result<Program, String> {
        let extern_names: Vec<String> = self.extern_functions.keys().cloned().collect();
        for name in &extern_names {
            self.resolve_external_function(name)?;
        }

        // Register function signatures first so recursive calls can see them
        self.declare_definitions(program)?;

        let definition_names: Vec<String> = self.definitions.keys().cloned().collect();
        for name in &definition_names {
            self.resolve_definition(name)?;
        }

        self.validate_main(program)?;

        let extern_functions = program
            .extern_functions
            .iter()
            .map(|ext| {
                Ok(ExternalFunction {
                    name: ext.name.clone(),
                    signature: self.resolve_external_function(&ext.name)?,
                })
            })
            .collect::<Result<Vec<_>, String>>()?;

        let mut definitions = Vec::new();
        for def in &program.definitions {
            let resolved_val = self.resolve_expression(&def.value)?;
            definitions.push(Definition {
                name: def.name.clone(),
                value: resolved_val,
            });
        }

        Ok(Program {
            variables: Vec::new(),
            extern_functions,
            definitions,
        })
    }

    fn validate_main(&self, program: &Program) -> Result<(), String> {
        let main_def = program
            .definitions
            .iter()
            .find(|d| d.name == "main")
            .ok_or_else(|| {
                "program is malformed: missing required entry point '@main'".to_string()
            })?;

        match &main_def.value {
            Expression::Block {
                signature: Some(sig),
                ..
            } => {
                if !sig.parameters.is_empty() {
                    return Err(format!(
                        "entry point '@main' cannot accept parameters, found {}",
                        sig.parameters.len()
                    ));
                }
                if sig.is_variadic {
                    return Err("entry point '@main' cannot be variadic".to_string());
                }
                if *sig.return_type != Type::S32 {
                    return Err(format!(
                        "entry point '@main' must have signature 'to s32', found return type {:?}",
                        sig.return_type
                    ));
                }
            }
            _ => {
                return Err(
                    "entry point '@main' must be a block with signature 'to s32'".to_string(),
                );
            }
        }

        Ok(())
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
                "cyclic dependency involving external function '{}'",
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
                if p.ty == Type::Void {
                    return Err(format!(
                        "parameter '{}' in extern function '{}' cannot be void",
                        p.name, name
                    ));
                }
                parameters.push(Parameter {
                    name: p.name.clone(),
                    ty: p.ty.clone(),
                });
            }

            Ok(Signature {
                parameters,
                return_type: raw.return_type.clone(),
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
            // If it's a function, typecheck its body if not already visited
            if matches!(ty, Type::Function { .. }) {
                if self.resolving_definitions.insert(name.to_string()) {
                    let expr = self.definitions.get(name).cloned().unwrap();
                    let _ = self.type_of_expression(&expr)?;
                }
                return Ok(self.resolved_definitions[name].clone());
            }
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
        let ty_result = self.type_of_expression(&expr);
        self.resolving_definitions.remove(name);

        let ty = ty_result?;
        self.resolved_definitions
            .insert(name.to_string(), ty.clone());
        Ok(ty)
    }

    fn lookup_binding(&self, name: &str) -> Option<BindingInfo> {
        self.scopes.iter().rev().find_map(|s| s.get(name).cloned())
    }

    fn lookup_binding_mut(&mut self, name: &str) -> Option<&mut BindingInfo> {
        self.scopes.iter_mut().rev().find_map(|s| s.get_mut(name))
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
                if self.definitions.contains_key(name) {
                    let def_type = self.resolve_definition(name)?;
                    return match def_type {
                        Type::Function {
                            parameters,
                            return_type,
                            is_variadic,
                        } => Ok(Type::Pointer {
                            element: Box::new(Type::Function {
                                parameters,
                                return_type,
                                is_variadic,
                            }),
                        }),
                        other => Ok(other),
                    };
                }
                if let Some(ext) = self.extern_functions.get(name).cloned() {
                    let param_types = ext.parameters.iter().map(|p| p.ty.clone()).collect();
                    return Ok(Type::Pointer {
                        element: Box::new(Type::Function {
                            parameters: param_types,
                            return_type: ext.return_type.clone(),
                            is_variadic: ext.is_variadic,
                        }),
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
                    if p.ty == Type::Void {
                        return Err(format!("parameter '{}' cannot be void", p.name));
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

                self.scopes.push(HashMap::new());

                if let Some(sig) = signature {
                    let mut seen_params = HashSet::new();
                    for p in &sig.parameters {
                        if !seen_params.insert(p.name.clone()) {
                            self.scopes.pop();
                            return Err(format!("duplicate parameter name '{}' in block", p.name));
                        }
                        if p.ty == Type::Void {
                            self.scopes.pop();
                            return Err(format!("block parameter '{}' cannot be void", p.name));
                        }
                        self.scopes.last_mut().unwrap().insert(
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
                self.scopes.pop();
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
                    return Err("expression 'if' must have a fallback 'else' arm".to_string());
                }

                let mut expected_ty: Option<Type> = None;

                for (i, arm) in arms.iter().enumerate() {
                    match &arm.condition {
                        IfCondition::Expression(cond) => {
                            let cond_ty = self.type_of_expression(cond)?;
                            if cond_ty != Type::Bool {
                                return Err(format!(
                                    "if condition must evaluate to Bool, found {:?}",
                                    cond_ty
                                ));
                            }
                        }
                        IfCondition::Else => {
                            if i != arms.len() - 1 {
                                return Err(
                                    "fallback 'else' arm must be the final arm in if".to_string()
                                );
                            }
                        }
                    }

                    let arm_ty = self.type_of_expression(&arm.body)?;
                    if let Some(target_ty) = &expected_ty {
                        if arm_ty != *target_ty {
                            return Err(format!(
                                "mismatched arm types in 'if': expected {:?}, found {:?}",
                                target_ty, arm_ty
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
                    return Err("cannot infer type of empty array".to_string());
                }

                let element_type = self.type_of_expression(&elements[0])?;
                for element in &elements[1..] {
                    let ty = self.type_of_expression(element)?;
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

            Expression::Unary { operator, operand } => {
                let ty = self.type_of_expression(operand)?;
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
                let left_type = self.type_of_expression(left)?;
                let right_type = self.type_of_expression(right)?;

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
                        return Err(
                            "atoms only support equality comparisons ('=' and '!=')".to_string()
                        );
                    }
                    if left_type != right_type {
                        return Err(format!(
                            "cannot compare distinct atom types: {:?} and {:?}",
                            left_type, right_type
                        ));
                    }
                    return Ok(Type::Bool);
                }

                if left_type != right_type {
                    return Err(format!(
                        "comparison operator requires matching types, found {:?} and {:?}",
                        left_type, right_type
                    ));
                }

                Ok(Type::Bool)
            }

            Expression::Cast { expression, ty } => {
                let source_type = self.type_of_expression(expression)?;

                if !Self::is_supported_cast(&source_type, ty) {
                    return Err(format!(
                        "unsupported cast from {:?} to {:?}",
                        source_type, ty
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
                            return Err(format!("cannot call non-function type {:?}", target_type));
                        }
                    },
                    _ => return Err(format!("cannot call non-function type {:?}", target_type)),
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
                    let arg_ty = self.type_of_expression(arg)?;
                    if arg_ty != *expected_ty {
                        return Err(format!(
                            "argument type mismatch: expected {:?}, found {:?}",
                            expected_ty, arg_ty
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
                    return Err("array index must be an integer".to_string());
                }

                match expression_type {
                    Type::Array { element, .. } => Ok(*element),
                    Type::Slice { element } => Ok(*element),
                    other => Err(format!("cannot index value of type {:?}", other)),
                }
            }

            Expression::Member { expression, field } => {
                let expr_type = self.type_of_expression(expression)?;
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
                            "cannot take address of unknown variable '{}'",
                            name
                        ))
                    }
                }
                _ => Err("address-of requires a variable or array index".to_string()),
            },

            Expression::Decay(expression) => {
                let inner_type = self.type_of_expression(expression)?;
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

    fn statement_definitely_returns(stmt: &Statement) -> bool {
        match stmt {
            Statement::Return(_) => true,
            Statement::Compound(stmts) => Self::definitely_returns(stmts),
            Statement::If { arms } => {
                !arms.is_empty()
                    && arms
                        .iter()
                        .all(|arm| Self::statement_definitely_returns(&arm.body))
            }
            _ => false,
        }
    }

    fn definitely_returns(statements: &[Statement]) -> bool {
        statements.iter().any(Self::statement_definitely_returns)
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
                        "variable '{}' cannot be declared with type void",
                        name
                    ));
                }
                if self.scopes.last().unwrap().contains_key(name) {
                    return Err(format!(
                        "variable '{}' already declared in this scope",
                        name
                    ));
                }
                self.scopes.last_mut().unwrap().insert(
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
                            "cannot reassign to immutable constant '{}'",
                            target
                        ));
                    }

                    if expr_ty != info.ty {
                        return Err(format!(
                            "type mismatch assigning to '{}': expected {:?}, found {:?}",
                            target, info.ty, expr_ty
                        ));
                    }

                    if let Some(info_mut) = self.lookup_binding_mut(target) {
                        info_mut.is_initialized = true;
                    }
                } else {
                    self.scopes.last_mut().unwrap().insert(
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
                if actual != *expected_type {
                    return Err(format!(
                        "return type mismatch: expected {:?}, found {:?}",
                        expected_type, actual
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
                                "doif condition must evaluate to Bool, found {:?}",
                                cond_ty
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

        if *expected_type != Type::Void && !Self::definitely_returns(statements) {
            return Err(format!(
                "missing return statement: non-void function expecting return type {:?} must return a value along all execution paths",
                expected_type
            ));
        }

        Ok(())
    }

    pub fn resolve_statements(
        &mut self,
        statements: &[Statement],
    ) -> Result<Vec<Statement>, String> {
        let mut resolved = Vec::new();
        for statement in statements {
            resolved.push(self.resolve_statement(statement)?);
        }
        Ok(resolved)
    }

    fn resolve_statement(&mut self, statement: &Statement) -> Result<Statement, String> {
        match statement {
            Statement::Compound(stmts) => {
                let resolved = self.resolve_statements(stmts)?;
                Ok(Statement::Compound(resolved))
            }

            Statement::Declare { name, ty } => Ok(Statement::Declare {
                name: name.clone(),
                ty: ty.clone(),
            }),

            Statement::Assign { expression, target } => {
                let resolved_expr = self.resolve_expression(expression)?;
                Ok(Statement::Assign {
                    expression: resolved_expr,
                    target: target.clone(),
                })
            }

            Statement::Discard(expression) => {
                let resolved_expr = self.resolve_expression(expression)?;
                Ok(Statement::Discard(resolved_expr))
            }

            Statement::Return(expression) => {
                Ok(Statement::Return(self.resolve_expression(expression)?))
            }

            Statement::If { arms } => {
                let mut resolved_arms = Vec::new();
                for arm in arms {
                    let condition = match &arm.condition {
                        IfCondition::Expression(expr) => {
                            IfCondition::Expression(self.resolve_expression(expr)?)
                        }
                        IfCondition::Else => IfCondition::Else,
                    };
                    let body = Box::new(self.resolve_statement(&arm.body)?);
                    resolved_arms.push(DoIfArm { condition, body });
                }
                Ok(Statement::If {
                    arms: resolved_arms,
                })
            }
        }
    }

    pub fn resolve_expression(&mut self, expression: &Expression) -> Result<Expression, String> {
        match expression {
            Expression::Integer(value) => Ok(Expression::Integer(*value)),
            Expression::Boolean(value) => Ok(Expression::Boolean(*value)),
            Expression::Atom(name) => {
                self.intern_atom(name);
                Ok(Expression::Atom(name.clone()))
            }
            Expression::String(value) => Ok(Expression::String(value.clone())),
            Expression::Identifier(name) => Ok(Expression::Identifier(name.clone())),

            Expression::Signature(sig) => Ok(Expression::Signature(sig.clone())),

            Expression::Block { signature, body } => {
                let resolved_body = self.resolve_statements(body)?;

                Ok(Expression::Block {
                    signature: signature.clone(),
                    body: resolved_body,
                })
            }

            Expression::If { arms } => {
                let mut resolved_arms = Vec::new();
                for arm in arms {
                    let condition = match &arm.condition {
                        IfCondition::Expression(expr) => {
                            IfCondition::Expression(self.resolve_expression(expr)?)
                        }
                        IfCondition::Else => IfCondition::Else,
                    };
                    let body = Box::new(self.resolve_expression(&arm.body)?);
                    resolved_arms.push(IfArm { condition, body });
                }
                Ok(Expression::If {
                    arms: resolved_arms,
                })
            }

            Expression::Array(elements) => Ok(Expression::Array(
                elements
                    .iter()
                    .map(|element| self.resolve_expression(element))
                    .collect::<Result<Vec<_>, _>>()?,
            )),

            Expression::Unary { operator, operand } => {
                let resolved_operand = self.resolve_expression(operand)?;
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
                let resolved_left = self.resolve_expression(left)?;
                let resolved_right = self.resolve_expression(right)?;
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
                let resolved_left = self.resolve_expression(left)?;
                let resolved_right = self.resolve_expression(right)?;
                Ok(Expression::Comparison {
                    left: Box::new(resolved_left),
                    operator: operator.clone(),
                    right: Box::new(resolved_right),
                })
            }

            Expression::Cast { expression, ty } => {
                let resolved_expression = self.resolve_expression(expression)?;
                Ok(Expression::Cast {
                    expression: Box::new(resolved_expression),
                    ty: ty.clone(),
                })
            }

            Expression::Call { target, arguments } => {
                let resolved_target = self.resolve_expression(target)?;
                let resolved_arguments = arguments
                    .iter()
                    .map(|arg| self.resolve_expression(arg))
                    .collect::<Result<Vec<_>, _>>()?;

                Ok(Expression::Call {
                    target: Box::new(resolved_target),
                    arguments: resolved_arguments,
                })
            }

            Expression::Index { expression, index } => {
                let resolved_expression = self.resolve_expression(expression)?;
                let resolved_index = self.resolve_expression(index)?;
                Ok(Expression::Index {
                    expression: Box::new(resolved_expression),
                    index: Box::new(resolved_index),
                })
            }

            Expression::Member { expression, field } => {
                let resolved_expression = self.resolve_expression(expression)?;
                Ok(Expression::Member {
                    expression: Box::new(resolved_expression),
                    field: field.clone(),
                })
            }

            Expression::AddressOf(expression) => {
                let resolved_expression = self.resolve_expression(expression)?;
                Ok(Expression::AddressOf(Box::new(resolved_expression)))
            }

            Expression::Decay(expression) => {
                let resolved_expression = self.resolve_expression(expression)?;
                Ok(Expression::Decay(Box::new(resolved_expression)))
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

        if matches!(source, Type::Atom(_)) || matches!(target, Type::Atom(_)) {
            return false;
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
