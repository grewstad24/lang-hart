use std::collections::HashMap;

use inkwell::builder::Builder;
use inkwell::context::Context;
use inkwell::module::Module;
use inkwell::types::{BasicMetadataTypeEnum, BasicType, BasicTypeEnum, FunctionType, StructType};
use inkwell::values::{
    ArrayValue, BasicValueEnum, FunctionValue, IntValue, PointerValue, StructValue,
};
use inkwell::{AddressSpace, IntPredicate};

use crate::ast::{
    BinaryOperator, ComparisonOperator, ConditionalBranch, Definition, Expression,
    ExternalFunction, Program, Signature, Statement, Type, UnaryOperator,
};
use crate::checker::Checker;

#[derive(Debug, Clone)]
enum Binding<'ctx> {
    Void,

    Integer {
        ty: Type,
        value: IntValue<'ctx>,
    },

    Boolean {
        value: IntValue<'ctx>,
    },

    Array {
        ty: Type,
        value: ArrayValue<'ctx>,
        storage: Option<PointerValue<'ctx>>,
    },

    Pointer {
        ty: Type,
        value: PointerValue<'ctx>,
    },

    Slice {
        ty: Type,
        value: StructValue<'ctx>,
        storage: Option<PointerValue<'ctx>>,
    },

    Function {
        value: FunctionValue<'ctx>,
    },
}

#[derive(Debug, Clone)]
struct LocalVar<'ctx> {
    ty: Type,
    alloca: PointerValue<'ctx>,
}

pub struct Codegen<'ctx> {
    context: &'ctx Context,
    module: Module<'ctx>,
    builder: Builder<'ctx>,

    local_scopes: Vec<HashMap<String, LocalVar<'ctx>>>,
    extern_functions: HashMap<String, Signature>,

    string_counter: usize,
    branch_counter: usize,
    anon_counter: usize,
}

impl<'ctx> Codegen<'ctx> {
    pub fn new(context: &'ctx Context, module_name: &str) -> Self {
        Self {
            context,
            module: context.create_module(module_name),
            builder: context.create_builder(),

            local_scopes: Vec::new(),
            extern_functions: HashMap::new(),

            string_counter: 0,
            branch_counter: 0,
            anon_counter: 0,
        }
    }

    pub fn generate(mut self, program: &Program) -> Result<Module<'ctx>, String> {
        let mut checker = Checker::new(program);
        let resolved_program = checker.check(program)?;

        self.collect_declarations(&resolved_program)?;

        self.codegen_externals(&resolved_program.extern_functions)?;
        self.codegen_definitions(&resolved_program.definitions)?;

        Ok(self.module)
    }

    fn collect_declarations(&mut self, program: &Program) -> Result<(), String> {
        for external in &program.extern_functions {
            self.extern_functions
                .insert(external.name.clone(), external.signature.clone());
        }
        Ok(())
    }

    fn codegen_externals(&mut self, extern_functions: &[ExternalFunction]) -> Result<(), String> {
        for external in extern_functions {
            let param_types: Vec<BasicMetadataTypeEnum> = external
                .signature
                .parameters
                .iter()
                .map(|param| {
                    let ty = Checker::concrete_type(&param.ty)?;
                    let llvm_ty = self.llvm_type(&ty)?;
                    Ok(llvm_ty.into())
                })
                .collect::<Result<Vec<_>, String>>()?;

            let ret_type = Checker::concrete_type(&external.signature.return_type)?;

            let fn_type = match self.llvm_type(&ret_type) {
                Ok(basic_ty) => basic_ty.fn_type(&param_types, external.signature.is_variadic),
                Err(_) => self
                    .context
                    .void_type()
                    .fn_type(&param_types, external.signature.is_variadic),
            };

            self.module.add_function(&external.name, fn_type, None);
        }

        Ok(())
    }

    fn codegen_definitions(&mut self, definitions: &[Definition]) -> Result<(), String> {
        for def in definitions {
            if def.name == "main" {
                continue;
            }
            if let Expression::Block { signature, .. } = &def.value {
                let fn_type = self.signature_to_function_type(signature.as_ref())?;
                self.module.add_function(&def.name, fn_type, None);
            }
        }

        for def in definitions {
            if def.name == "main" {
                self.codegen_main(def)?;
            } else if let Expression::Block { signature, body } = &def.value {
                self.codegen_named_function(&def.name, signature.as_ref(), body)?;
            } else {
                let binding = self.codegen_expression(&def.value)?;
                let ty = self.binding_type(&binding)?;
                let llvm_ty = self.llvm_type(&ty)?;
                let global =
                    self.module
                        .add_global(llvm_ty, Some(AddressSpace::default()), &def.name);
                global.set_initializer(&self.zero_value(&ty)?);
            }
        }

        Ok(())
    }

    fn signature_to_function_type(
        &self,
        signature: Option<&Signature>,
    ) -> Result<FunctionType<'ctx>, String> {
        if let Some(sig) = signature {
            let param_types = sig
                .parameters
                .iter()
                .map(|p| {
                    let ty = Checker::concrete_type(&p.ty)?;
                    let llvm_ty = self.llvm_type(&ty)?;
                    Ok(BasicMetadataTypeEnum::from(llvm_ty))
                })
                .collect::<Result<Vec<_>, String>>()?;

            let ret_type = Checker::concrete_type(&sig.return_type)?;
            if ret_type == Type::Void {
                Ok(self
                    .context
                    .void_type()
                    .fn_type(&param_types, sig.is_variadic))
            } else {
                Ok(self
                    .llvm_type(&ret_type)?
                    .fn_type(&param_types, sig.is_variadic))
            }
        } else {
            Ok(self.context.void_type().fn_type(&[], false))
        }
    }

    fn lookup_local(&self, name: &str) -> Option<LocalVar<'ctx>> {
        for scope in self.local_scopes.iter().rev() {
            if let Some(var) = scope.get(name) {
                return Some(var.clone());
            }
        }
        None
    }

    fn codegen_named_function(
        &mut self,
        name: &str,
        signature: Option<&Signature>,
        body: &[Statement],
    ) -> Result<(), String> {
        let llvm_function = self
            .module
            .get_function(name)
            .ok_or_else(|| format!("missing declared function '{}'", name))?;

        let entry = self.context.append_basic_block(llvm_function, "entry");
        self.builder.position_at_end(entry);

        self.local_scopes.clear();
        self.local_scopes.push(HashMap::new());

        if let Some(sig) = signature {
            for (index, parameter) in sig.parameters.iter().enumerate() {
                let ty = Checker::concrete_type(&parameter.ty)?;
                let llvm_ty = self.llvm_type(&ty)?;
                let param_val = llvm_function
                    .get_nth_param(index as u32)
                    .ok_or_else(|| format!("missing parameter {}", index))?;

                let alloca = self
                    .builder
                    .build_alloca(llvm_ty, &parameter.name)
                    .map_err(|e| e.to_string())?;

                self.builder
                    .build_store(alloca, param_val)
                    .map_err(|e| e.to_string())?;

                self.local_scopes
                    .last_mut()
                    .unwrap()
                    .insert(parameter.name.clone(), LocalVar { ty, alloca });
            }
        }

        self.codegen_statements(body)?;

        if !self.has_terminator(llvm_function) {
            let ret_ty = if let Some(sig) = signature {
                Checker::concrete_type(&sig.return_type)?
            } else {
                Type::Void
            };

            if ret_ty == Type::Void {
                self.builder.build_return(None).map_err(|e| e.to_string())?;
            } else {
                let zero = self.zero_value(&ret_ty)?;
                self.builder
                    .build_return(Some(&zero))
                    .map_err(|e| e.to_string())?;
            }
        }

        self.local_scopes.pop();
        Ok(())
    }

    fn codegen_main(&mut self, main_def: &Definition) -> Result<(), String> {
        let i32_type = self.context.i32_type();
        let function_type = i32_type.fn_type(&[], false);

        let function = self.module.add_function("main", function_type, None);
        let entry = self.context.append_basic_block(function, "entry");

        self.builder.position_at_end(entry);
        self.local_scopes.clear();
        self.local_scopes.push(HashMap::new());

        match &main_def.value {
            Expression::Block { body, .. } => {
                self.codegen_statements(body)?;
            }
            expr => {
                self.codegen_expression(expr)?;
            }
        }

        if !self.has_terminator(function) {
            self.builder
                .build_return(Some(&i32_type.const_zero()))
                .map_err(|error| error.to_string())?;
        }

        self.local_scopes.pop();
        Ok(())
    }

    fn codegen_statements(&mut self, statements: &[Statement]) -> Result<(), String> {
        for statement in statements {
            self.codegen_statement(statement)?;

            if let Some(current_block) = self.builder.get_insert_block() {
                if current_block.get_terminator().is_some() {
                    break;
                }
            }
        }
        Ok(())
    }

    fn codegen_statement(&mut self, statement: &Statement) -> Result<(), String> {
        match statement {
            Statement::Declare { name, ty, .. } => {
                let concrete_ty = Checker::concrete_type(ty)?;
                let llvm_ty = self.llvm_type(&concrete_ty)?;

                let alloca = self
                    .builder
                    .build_alloca(llvm_ty, name)
                    .map_err(|e| e.to_string())?;

                self.local_scopes.last_mut().unwrap().insert(
                    name.clone(),
                    LocalVar {
                        ty: concrete_ty,
                        alloca,
                    },
                );
                Ok(())
            }

            Statement::Assign {
                expression,
                targets,
            } => {
                let mut current_binding = self.codegen_expression(expression)?;

                for target in targets {
                    let local = self
                        .lookup_local(target)
                        .ok_or_else(|| format!("unknown local variable '{}'", target))?;

                    let value = self.binding_basic_value(&current_binding)?;
                    self.builder
                        .build_store(local.alloca, value)
                        .map_err(|e| e.to_string())?;

                    let loaded = self
                        .builder
                        .build_load(self.llvm_type(&local.ty)?, local.alloca, "assign_prop")
                        .map_err(|e| e.to_string())?;

                    current_binding = self.binding_from_basic_value(&local.ty, loaded)?;
                }

                Ok(())
            }

            Statement::Discard(expression) => {
                self.codegen_expression(expression)?;
                Ok(())
            }

            Statement::Return(expression) => {
                let binding = self.codegen_expression(expression)?;
                self.build_return(&binding)
            }

            Statement::Exit(expression) => {
                let binding = self.codegen_expression(expression)?;
                let value = self.binding_int_value(&binding)?;

                self.builder
                    .build_return(Some(&value))
                    .map_err(|error| error.to_string())?;

                Ok(())
            }

            Statement::If {
                branches,
                else_body,
            } => self.codegen_if_statement(branches, else_body),
        }
    }

    fn codegen_if_statement(
        &mut self,
        branches: &[ConditionalBranch],
        else_body: &Option<Vec<Statement>>,
    ) -> Result<(), String> {
        let current_block = self
            .builder
            .get_insert_block()
            .ok_or_else(|| "cannot generate if statement outside basic block".to_string())?;

        let current_fn = current_block
            .get_parent()
            .ok_or_else(|| "current block has no parent function".to_string())?;

        let id = self.branch_counter;
        self.branch_counter += 1;

        let end_bb = self
            .context
            .append_basic_block(current_fn, &format!("if.end.{}", id));

        let first_cond_bb = self
            .context
            .append_basic_block(current_fn, &format!("if.test.0.{}", id));

        self.builder
            .build_unconditional_branch(first_cond_bb)
            .map_err(|e| e.to_string())?;

        let mut next_cond_bb = first_cond_bb;

        for (i, branch) in branches.iter().enumerate() {
            self.builder.position_at_end(next_cond_bb);

            let cond_binding = self.codegen_expression(&branch.condition)?;
            let cond_value = match cond_binding {
                Binding::Boolean { value } => value,
                _ => return Err("if condition must evaluate to a boolean".to_string()),
            };

            let then_bb = self
                .context
                .append_basic_block(current_fn, &format!("if.then.{}.{}", i, id));

            let false_bb = if i + 1 < branches.len() {
                self.context
                    .append_basic_block(current_fn, &format!("if.test.{}.{}", i + 1, id))
            } else if else_body.is_some() {
                self.context
                    .append_basic_block(current_fn, &format!("if.else.{}", id))
            } else {
                end_bb
            };

            self.builder
                .build_conditional_branch(cond_value, then_bb, false_bb)
                .map_err(|e| e.to_string())?;

            self.builder.position_at_end(then_bb);
            self.local_scopes.push(HashMap::new());
            self.codegen_statements(&branch.body)?;
            self.local_scopes.pop();

            if let Some(insert_bb) = self.builder.get_insert_block() {
                if insert_bb.get_terminator().is_none() {
                    self.builder
                        .build_unconditional_branch(end_bb)
                        .map_err(|e| e.to_string())?;
                }
            }

            next_cond_bb = false_bb;
        }

        if let Some(else_stmts) = else_body {
            self.builder.position_at_end(next_cond_bb);
            self.local_scopes.push(HashMap::new());
            self.codegen_statements(else_stmts)?;
            self.local_scopes.pop();

            if let Some(insert_bb) = self.builder.get_insert_block() {
                if insert_bb.get_terminator().is_none() {
                    self.builder
                        .build_unconditional_branch(end_bb)
                        .map_err(|e| e.to_string())?;
                }
            }
        }

        self.builder.position_at_end(end_bb);
        Ok(())
    }

    fn codegen_expression(&mut self, expression: &Expression) -> Result<Binding<'ctx>, String> {
        match expression {
            Expression::Integer(value) => Ok(Binding::Integer {
                ty: Type::S32,
                value: self.context.i32_type().const_int(*value as u64, true),
            }),

            Expression::Boolean(value) => Ok(Binding::Boolean {
                value: self
                    .context
                    .bool_type()
                    .const_int(if *value { 1 } else { 0 }, false),
            }),

            Expression::String(value) => self.codegen_string_literal(value),

            Expression::Identifier(name) => {
                if let Some(local) = self.lookup_local(name) {
                    let loaded = self
                        .builder
                        .build_load(self.llvm_type(&local.ty)?, local.alloca, name)
                        .map_err(|e| e.to_string())?;
                    return self.binding_from_basic_value(&local.ty, loaded);
                }

                if let Some(function) = self.module.get_function(name) {
                    return Ok(Binding::Function { value: function });
                }

                if let Some(global) = self.module.get_global(name) {
                    return Ok(Binding::Pointer {
                        ty: Type::Pointer {
                            element: Box::new(Type::Void),
                        },
                        value: global.as_pointer_value(),
                    });
                }

                Err(format!("unknown identifier '{}'", name))
            }

            Expression::Signature(_) => Ok(Binding::Void),

            Expression::Block { signature, body } => {
                let anon_name = format!("__anon_block_{}", self.anon_counter);
                self.anon_counter += 1;

                let current_block = self.builder.get_insert_block();

                let fn_type = self.signature_to_function_type(signature.as_ref())?;
                let function = self.module.add_function(&anon_name, fn_type, None);

                self.codegen_named_function(&anon_name, signature.as_ref(), body)?;

                if let Some(bb) = current_block {
                    self.builder.position_at_end(bb);
                }

                Ok(Binding::Function { value: function })
            }

            Expression::Array(elements) => self.codegen_array_literal(elements),

            Expression::TypeOf(_) => {
                Err("type expression cannot be used as a runtime value".to_string())
            }

            Expression::Unary { operator, operand } => {
                let operand = self.codegen_expression(operand)?;
                self.codegen_unary(operator.clone(), &operand)
            }

            Expression::Binary {
                left,
                operator,
                right,
            } => {
                let left = self.codegen_expression(left)?;
                let right = self.codegen_expression(right)?;
                self.codegen_binary(&left, operator.clone(), &right)
            }

            Expression::Comparison {
                left,
                operator,
                right,
            } => {
                let left = self.codegen_expression(left)?;
                let right = self.codegen_expression(right)?;
                self.codegen_comparison(&left, operator.clone(), &right)
            }

            Expression::Cast { expression, ty } => {
                let value = self.codegen_expression(expression)?;
                let target_type = Checker::concrete_type(ty)?;
                self.codegen_cast(value, target_type)
            }

            Expression::Call { target, arguments } => self.codegen_call(target, arguments),

            Expression::Index { expression, index } => self.codegen_index(expression, index),

            Expression::Member { expression, field } => self.codegen_member(expression, field),

            Expression::AddressOf(expression) => self.codegen_address_of(expression),

            Expression::Decay(expression) => self.codegen_decay(expression),
        }
    }

    fn codegen_member(
        &mut self,
        expression: &Expression,
        field: &str,
    ) -> Result<Binding<'ctx>, String> {
        let binding = self.codegen_expression(expression)?;

        match binding {
            Binding::Slice {
                ty: Type::Slice { element },
                value,
                ..
            } => match field {
                "ptr" => {
                    let ptr_val = self
                        .builder
                        .build_extract_value(value, 0, "sliceptr")
                        .map_err(|e| e.to_string())?
                        .into_pointer_value();

                    Ok(Binding::Pointer {
                        ty: Type::Pointer { element },
                        value: ptr_val,
                    })
                }
                "len" => {
                    let len_val = self
                        .builder
                        .build_extract_value(value, 1, "slicelen")
                        .map_err(|e| e.to_string())?
                        .into_int_value();

                    Ok(Binding::Integer {
                        ty: Type::U64,
                        value: len_val,
                    })
                }
                _ => Err(format!("unknown slice member '.{}'", field)),
            },

            Binding::Array {
                ty: Type::Array { ref element, size },
                ..
            } => match field {
                "ptr" => {
                    let elem_clone = (*element).clone();
                    let storage = self.array_storage(binding, &elem_clone, size)?;
                    let zero = self.context.i32_type().const_zero();
                    let llvm_array_type = self
                        .llvm_type(&Type::Array {
                            element: elem_clone.clone(),
                            size,
                        })?
                        .into_array_type();

                    let pointer = unsafe {
                        self.builder
                            .build_gep(llvm_array_type, storage, &[zero, zero], "arrayptr")
                            .map_err(|e| e.to_string())?
                    };

                    Ok(Binding::Pointer {
                        ty: Type::Pointer {
                            element: elem_clone,
                        },
                        value: pointer,
                    })
                }
                "len" => Ok(Binding::Integer {
                    ty: Type::U64,
                    value: self.context.i64_type().const_int(size as u64, false),
                }),
                _ => Err(format!("unknown array member '.{}'", field)),
            },

            other => Err(format!(
                "member access '.{}' unsupported on {:?}",
                field, other
            )),
        }
    }

    fn codegen_decay(&mut self, expression: &Expression) -> Result<Binding<'ctx>, String> {
        let binding = self.codegen_expression(expression)?;

        match binding {
            Binding::Slice {
                ty: Type::Slice { element },
                value,
                ..
            } => {
                let ptr_val = self
                    .builder
                    .build_extract_value(value, 0, "decay_slice_ptr")
                    .map_err(|e| e.to_string())?
                    .into_pointer_value();

                Ok(Binding::Pointer {
                    ty: Type::Pointer { element },
                    value: ptr_val,
                })
            }

            Binding::Array {
                ty: Type::Array { ref element, size },
                ..
            } => {
                let elem_clone = (*element).clone();
                let storage = self.array_storage(binding, &elem_clone, size)?;
                let llvm_array_type = self
                    .llvm_type(&Type::Array {
                        element: elem_clone.clone(),
                        size,
                    })?
                    .into_array_type();

                let zero = self.context.i32_type().const_zero();
                let pointer = unsafe {
                    self.builder
                        .build_gep(llvm_array_type, storage, &[zero, zero], "decayptr")
                        .map_err(|error| error.to_string())?
                };

                Ok(Binding::Pointer {
                    ty: Type::Pointer {
                        element: elem_clone,
                    },
                    value: pointer,
                })
            }

            Binding::Pointer { .. } => Ok(binding),

            other => Err(format!("cannot decay non-array/slice binding: {:?}", other)),
        }
    }

    fn codegen_array_literal(&mut self, elements: &[Expression]) -> Result<Binding<'ctx>, String> {
        if elements.is_empty() {
            return Err("cannot generate an empty array literal".to_string());
        }

        let first = self.codegen_expression(&elements[0])?;
        let element_type = self.binding_type(&first)?;

        let mut bindings = Vec::with_capacity(elements.len());
        bindings.push(first);

        for element in &elements[1..] {
            let binding = self.codegen_expression(element)?;
            let ty = self.binding_type(&binding)?;

            if ty != element_type {
                return Err(format!(
                    "array literal elements have different types: expected {:?}, found {:?}",
                    element_type, ty
                ));
            }

            bindings.push(binding);
        }

        let array_value = match &element_type {
            Type::S16
            | Type::S32
            | Type::S64
            | Type::U8
            | Type::U16
            | Type::U32
            | Type::U64
            | Type::Bool => {
                let values = bindings
                    .iter()
                    .map(|b| self.binding_int_value(b))
                    .collect::<Result<Vec<_>, _>>()?;

                self.llvm_type(&element_type)?
                    .into_int_type()
                    .const_array(&values)
            }

            Type::Array { .. } => {
                let values = bindings
                    .iter()
                    .map(|b| match b {
                        Binding::Array { value, .. } => Ok(*value),
                        _ => Err("expected array element".to_string()),
                    })
                    .collect::<Result<Vec<_>, String>>()?;

                self.llvm_type(&element_type)?
                    .into_array_type()
                    .const_array(&values)
            }

            Type::Pointer { .. } | Type::Slice { .. } | Type::Function { .. } | Type::Void => {
                return Err(
                    "array literals of pointers, slices, functions, or void are not supported"
                        .to_string(),
                );
            }
        };

        Ok(Binding::Array {
            ty: Type::Array {
                element: Box::new(element_type),
                size: bindings.len(),
            },
            value: array_value,
            storage: None,
        })
    }

    fn codegen_unary(
        &mut self,
        operator: UnaryOperator,
        operand: &Binding<'ctx>,
    ) -> Result<Binding<'ctx>, String> {
        let ty = self.binding_type(operand)?;
        let value = self.binding_int_value(operand)?;

        match operator {
            UnaryOperator::Plus => Ok(Binding::Integer { ty, value }),

            UnaryOperator::Minus => {
                let value = self
                    .builder
                    .build_int_neg(value, "negtmp")
                    .map_err(|error| error.to_string())?;

                Ok(Binding::Integer { ty, value })
            }
        }
    }

    fn codegen_binary(
        &mut self,
        left: &Binding<'ctx>,
        operator: BinaryOperator,
        right: &Binding<'ctx>,
    ) -> Result<Binding<'ctx>, String> {
        let left_type = self.binding_type(left)?;
        let right_type = self.binding_type(right)?;

        if left_type != right_type {
            return Err(format!(
                "binary operands have different types: {:?} and {:?}",
                left_type, right_type
            ));
        }

        let left_value = self.binding_int_value(left)?;
        let right_value = self.binding_int_value(right)?;

        let value = match operator {
            BinaryOperator::Add => self
                .builder
                .build_int_add(left_value, right_value, "addtmp")
                .map_err(|error| error.to_string())?,

            BinaryOperator::Subtract => self
                .builder
                .build_int_sub(left_value, right_value, "subtmp")
                .map_err(|error| error.to_string())?,

            BinaryOperator::Multiply => self
                .builder
                .build_int_mul(left_value, right_value, "multmp")
                .map_err(|error| error.to_string())?,

            BinaryOperator::Divide => {
                if self.is_signed(&left_type) {
                    self.builder
                        .build_int_signed_div(left_value, right_value, "divtmp")
                        .map_err(|error| error.to_string())?
                } else {
                    self.builder
                        .build_int_unsigned_div(left_value, right_value, "divtmp")
                        .map_err(|error| error.to_string())?
                }
            }

            BinaryOperator::Modulo => {
                if self.is_signed(&left_type) {
                    self.builder
                        .build_int_signed_rem(left_value, right_value, "modtmp")
                        .map_err(|error| error.to_string())?
                } else {
                    self.builder
                        .build_int_unsigned_rem(left_value, right_value, "modtmp")
                        .map_err(|error| error.to_string())?
                }
            }
        };

        Ok(Binding::Integer {
            ty: left_type,
            value,
        })
    }

    fn codegen_comparison(
        &mut self,
        left: &Binding<'ctx>,
        operator: ComparisonOperator,
        right: &Binding<'ctx>,
    ) -> Result<Binding<'ctx>, String> {
        let left_type = self.binding_type(left)?;
        let right_type = self.binding_type(right)?;

        if left_type != right_type {
            return Err(format!(
                "comparison operands have different types: {:?} and {:?}",
                left_type, right_type
            ));
        }

        let left_value = self.binding_int_value(left)?;
        let right_value = self.binding_int_value(right)?;

        let predicate = match operator {
            ComparisonOperator::Equal => IntPredicate::EQ,
            ComparisonOperator::NotEqual => IntPredicate::NE,

            ComparisonOperator::Less => {
                if self.is_signed(&left_type) {
                    IntPredicate::SLT
                } else {
                    IntPredicate::ULT
                }
            }

            ComparisonOperator::LessEqual => {
                if self.is_signed(&left_type) {
                    IntPredicate::SLE
                } else {
                    IntPredicate::ULE
                }
            }

            ComparisonOperator::Greater => {
                if self.is_signed(&left_type) {
                    IntPredicate::SGT
                } else {
                    IntPredicate::UGT
                }
            }

            ComparisonOperator::GreaterEqual => {
                if self.is_signed(&left_type) {
                    IntPredicate::SGE
                } else {
                    IntPredicate::UGE
                }
            }
        };

        let value = self
            .builder
            .build_int_compare(predicate, left_value, right_value, "cmptmp")
            .map_err(|error| error.to_string())?;

        Ok(Binding::Boolean { value })
    }

    fn codegen_cast(
        &mut self,
        binding: Binding<'ctx>,
        target_type: Type,
    ) -> Result<Binding<'ctx>, String> {
        let source_type = self.binding_type(&binding)?;

        if source_type == target_type {
            return Ok(binding);
        }

        if !Checker::is_supported_cast(&source_type, &target_type) {
            return Err(format!(
                "unsupported cast from {:?} to {:?}",
                source_type, target_type
            ));
        }

        if Checker::is_integer_type(&source_type) && matches!(target_type, Type::Pointer { .. }) {
            let int_val = self.binding_int_value(&binding)?;
            let ptr_type = self.llvm_type(&target_type)?.into_pointer_type();
            let ptr_val = self
                .builder
                .build_int_to_ptr(int_val, ptr_type, "inttoptr")
                .map_err(|e| e.to_string())?;

            return Ok(Binding::Pointer {
                ty: target_type,
                value: ptr_val,
            });
        }

        if matches!(source_type, Type::Pointer { .. }) && Checker::is_integer_type(&target_type) {
            let ptr_val = match binding {
                Binding::Pointer { value, .. } => value,
                Binding::Function { value } => value.as_global_value().as_pointer_value(),
                _ => return Err("expected pointer binding".to_string()),
            };
            let int_type = self.llvm_type(&target_type)?.into_int_type();
            let int_val = self
                .builder
                .build_ptr_to_int(ptr_val, int_type, "ptrtoint")
                .map_err(|e| e.to_string())?;

            return Ok(Binding::Integer {
                ty: target_type,
                value: int_val,
            });
        }

        let value = self.binding_int_value(&binding)?;

        if target_type == Type::Bool {
            let zero = value.get_type().const_zero();
            let bool_val = self
                .builder
                .build_int_compare(IntPredicate::NE, value, zero, "tobool")
                .map_err(|e| e.to_string())?;

            return Ok(Binding::Boolean { value: bool_val });
        }

        let target_llvm_type = self.llvm_type(&target_type)?.into_int_type();
        let source_bits = value.get_type().get_bit_width();
        let target_bits = target_llvm_type.get_bit_width();

        let casted = if target_bits == source_bits {
            value
        } else if target_bits > source_bits {
            if source_type != Type::Bool && self.is_signed(&source_type) {
                self.builder
                    .build_int_s_extend(value, target_llvm_type, "sexttmp")
                    .map_err(|e| e.to_string())?
            } else {
                self.builder
                    .build_int_z_extend(value, target_llvm_type, "zexttmp")
                    .map_err(|e| e.to_string())?
            }
        } else {
            self.builder
                .build_int_truncate(value, target_llvm_type, "trunctmp")
                .map_err(|e| e.to_string())?
        };

        Ok(Binding::Integer {
            ty: target_type,
            value: casted,
        })
    }

    fn codegen_call(
        &mut self,
        target: &Expression,
        arguments: &[Expression],
    ) -> Result<Binding<'ctx>, String> {
        let (function_ptr, ret_ty, fn_type) = match target {
            Expression::Identifier(name) => {
                if let Some(func) = self.module.get_function(name) {
                    let return_type = if let Some(sig) = self.extern_functions.get(name) {
                        Checker::concrete_type(&sig.return_type)?
                    } else {
                        match func.get_type().get_return_type() {
                            Some(_) => Type::S32,
                            None => Type::Void,
                        }
                    };
                    Ok((
                        func.as_global_value().as_pointer_value(),
                        return_type,
                        func.get_type(),
                    ))
                } else if let Some(local) = self.lookup_local(name) {
                    let loaded = self
                        .builder
                        .build_load(self.llvm_type(&local.ty)?, local.alloca, "fn_ptr")
                        .map_err(|e| e.to_string())?
                        .into_pointer_value();

                    let ret_ty = match &local.ty {
                        Type::Pointer { element } => match element.as_ref() {
                            Type::Function { return_type, .. } => *return_type.clone(),
                            _ => Type::Void,
                        },
                        _ => Type::Void,
                    };

                    let fn_type = match self.llvm_type(&ret_ty) {
                        Ok(b) => b.fn_type(&[], true),
                        Err(_) => self.context.void_type().fn_type(&[], true),
                    };

                    Ok((loaded, ret_ty, fn_type))
                } else {
                    Err(format!("unknown function or callable variable '{}'", name))
                }
            }
            expr => {
                let target_binding = self.codegen_expression(expr)?;
                match target_binding {
                    Binding::Function { value } => {
                        let return_type = match value.get_type().get_return_type() {
                            Some(_) => Type::S32,
                            None => Type::Void,
                        };
                        Ok((
                            value.as_global_value().as_pointer_value(),
                            return_type,
                            value.get_type(),
                        ))
                    }
                    Binding::Pointer { value, ty } => {
                        let ret_ty = match ty {
                            Type::Pointer { element } => match *element {
                                Type::Function { return_type, .. } => *return_type,
                                _ => Type::Void,
                            },
                            _ => Type::Void,
                        };
                        let fn_type = match self.llvm_type(&ret_ty) {
                            Ok(b) => b.fn_type(&[], true),
                            Err(_) => self.context.void_type().fn_type(&[], true),
                        };
                        Ok((value, ret_ty, fn_type))
                    }
                    _ => Err("called target is not callable".to_string()),
                }
            }
        }?;

        let mut argument_values = Vec::with_capacity(arguments.len());
        for argument in arguments {
            let binding = self.codegen_expression(argument)?;
            argument_values.push(self.binding_basic_value(&binding)?.into());
        }

        let call = self
            .builder
            .build_indirect_call(fn_type, function_ptr, &argument_values, "calltmp")
            .map_err(|error| error.to_string())?;

        if ret_ty == Type::Void {
            Ok(Binding::Void)
        } else {
            let return_value = call
                .try_as_basic_value()
                .basic()
                .ok_or_else(|| "call did not return a value".to_string())?;

            self.binding_from_basic_value(&ret_ty, return_value)
        }
    }

    fn codegen_index(
        &mut self,
        expression: &Expression,
        index: &Expression,
    ) -> Result<Binding<'ctx>, String> {
        let binding = self.codegen_expression(expression)?;
        let index_binding = self.codegen_expression(index)?;
        let index_value = self.binding_int_value(&index_binding)?;

        match self.binding_type(&binding)? {
            Type::Array { element, size } => {
                if size == 0 {
                    return Err("cannot index empty array".to_string());
                }

                let storage = self.array_storage(binding, &element, size)?;
                let llvm_array_type = self
                    .llvm_type(&Type::Array {
                        element: Box::new((*element).clone()),
                        size,
                    })?
                    .into_array_type();

                let pointer = unsafe {
                    self.builder
                        .build_gep(
                            llvm_array_type,
                            storage,
                            &[self.context.i32_type().const_zero(), index_value],
                            "indexptr",
                        )
                        .map_err(|error| error.to_string())?
                };

                let loaded = self
                    .builder
                    .build_load(self.llvm_type(&element)?, pointer, "indextmp")
                    .map_err(|error| error.to_string())?;

                self.binding_from_basic_value(&element, loaded)
            }

            Type::Slice { element } => {
                let slice_struct = match binding {
                    Binding::Slice { value, .. } => value,
                    _ => return Err("expected slice binding".to_string()),
                };

                let ptr_val = self
                    .builder
                    .build_extract_value(slice_struct, 0, "sliceptr")
                    .map_err(|e| e.to_string())?
                    .into_pointer_value();

                let elem_llvm_type = self.llvm_type(&element)?;

                let gep = unsafe {
                    self.builder
                        .build_gep(elem_llvm_type, ptr_val, &[index_value], "sliceidx")
                        .map_err(|e| e.to_string())?
                };

                let loaded = self
                    .builder
                    .build_load(elem_llvm_type, gep, "sliceload")
                    .map_err(|e| e.to_string())?;

                self.binding_from_basic_value(&element, loaded)
            }

            other => Err(format!("cannot index value of type {:?}", other)),
        }
    }

    fn codegen_address_of(&mut self, expression: &Expression) -> Result<Binding<'ctx>, String> {
        match expression {
            Expression::Identifier(name) => {
                if let Some(local) = self.lookup_local(name) {
                    return Ok(Binding::Pointer {
                        ty: Type::Pointer {
                            element: Box::new(local.ty),
                        },
                        value: local.alloca,
                    });
                }

                Err(format!("unknown variable '{}'", name))
            }

            Expression::Index { expression, index } => {
                let binding = self.codegen_expression(expression)?;
                let index_binding = self.codegen_expression(index)?;
                let index_value = self.binding_int_value(&index_binding)?;

                match self.binding_type(&binding)? {
                    Type::Array { element, size } => {
                        if size == 0 {
                            return Err("cannot index empty array".to_string());
                        }

                        let storage = self.array_storage(binding, &element, size)?;
                        let llvm_array_type = self
                            .llvm_type(&Type::Array {
                                element: Box::new((*element).clone()),
                                size,
                            })?
                            .into_array_type();

                        let pointer = unsafe {
                            self.builder
                                .build_gep(
                                    llvm_array_type,
                                    storage,
                                    &[self.context.i32_type().const_zero(), index_value],
                                    "addressptr",
                                )
                                .map_err(|error| error.to_string())?
                        };

                        Ok(Binding::Pointer {
                            ty: Type::Pointer { element },
                            value: pointer,
                        })
                    }

                    Type::Slice { element } => {
                        let slice_struct = match binding {
                            Binding::Slice { value, .. } => value,
                            _ => return Err("expected slice binding".to_string()),
                        };

                        let ptr_val = self
                            .builder
                            .build_extract_value(slice_struct, 0, "sliceptr")
                            .map_err(|e| e.to_string())?
                            .into_pointer_value();

                        let elem_llvm_type = self.llvm_type(&element)?;

                        let pointer = unsafe {
                            self.builder
                                .build_gep(elem_llvm_type, ptr_val, &[index_value], "sliceaddr")
                                .map_err(|e| e.to_string())?
                        };

                        Ok(Binding::Pointer {
                            ty: Type::Pointer { element },
                            value: pointer,
                        })
                    }

                    other => Err(format!("cannot take address of value of type {:?}", other)),
                }
            }

            _ => Err("address-of requires a local variable or array index".to_string()),
        }
    }

    fn array_storage(
        &mut self,
        binding: Binding<'ctx>,
        element_type: &Type,
        size: usize,
    ) -> Result<PointerValue<'ctx>, String> {
        match binding {
            Binding::Array {
                storage: Some(storage),
                ..
            } => Ok(storage),

            Binding::Array {
                value,
                storage: None,
                ..
            } => {
                let llvm_array_type = self
                    .llvm_type(&Type::Array {
                        element: Box::new(element_type.clone()),
                        size,
                    })?
                    .into_array_type();

                let temporary = self
                    .builder
                    .build_alloca(llvm_array_type, "arraytmp")
                    .map_err(|error| error.to_string())?;

                self.builder
                    .build_store(temporary, value)
                    .map_err(|error| error.to_string())?;

                Ok(temporary)
            }

            _ => Err("array does not have addressable storage".to_string()),
        }
    }

    fn slice_type(&self) -> StructType<'ctx> {
        let ptr_type = self.context.ptr_type(AddressSpace::default());
        let len_type = self.context.i64_type();
        self.context
            .struct_type(&[ptr_type.into(), len_type.into()], false)
    }

    fn codegen_string_literal(&mut self, value: &[u8]) -> Result<Binding<'ctx>, String> {
        let element_type = self.context.i8_type();
        let array_type = element_type.array_type((value.len() + 1) as u32);

        let name = format!(".str.{}", self.string_counter);
        self.string_counter += 1;

        let global = self
            .module
            .add_global(array_type, Some(AddressSpace::default()), &name);

        let mut bytes: Vec<_> = value
            .iter()
            .map(|byte| element_type.const_int(*byte as u64, false))
            .collect();

        bytes.push(element_type.const_zero());

        let initializer = element_type.const_array(&bytes);
        global.set_initializer(&initializer);
        global.set_constant(true);

        let slice_llvm_type = self.slice_type();
        let ptr = global.as_pointer_value();
        let len = self.context.i64_type().const_int(value.len() as u64, false);

        let slice_val = slice_llvm_type.const_named_struct(&[ptr.into(), len.into()]);

        Ok(Binding::Slice {
            ty: Type::Slice {
                element: Box::new(Type::U8),
            },
            value: slice_val,
            storage: None,
        })
    }

    fn binding_from_basic_value(
        &self,
        ty: &Type,
        value: BasicValueEnum<'ctx>,
    ) -> Result<Binding<'ctx>, String> {
        match ty {
            Type::Void => Ok(Binding::Void),

            Type::Bool => Ok(Binding::Boolean {
                value: value.into_int_value(),
            }),

            Type::S16 | Type::S32 | Type::S64 | Type::U8 | Type::U16 | Type::U32 | Type::U64 => {
                Ok(Binding::Integer {
                    ty: ty.clone(),
                    value: value.into_int_value(),
                })
            }

            Type::Array { .. } => Ok(Binding::Array {
                ty: ty.clone(),
                value: value.into_array_value(),
                storage: None,
            }),

            Type::Pointer { .. } => Ok(Binding::Pointer {
                ty: ty.clone(),
                value: value.into_pointer_value(),
            }),

            Type::Slice { .. } => Ok(Binding::Slice {
                ty: ty.clone(),
                value: value.into_struct_value(),
                storage: None,
            }),

            Type::Function { .. } => Ok(Binding::Pointer {
                ty: ty.clone(),
                value: value.into_pointer_value(),
            }),
        }
    }

    fn binding_type(&self, binding: &Binding<'ctx>) -> Result<Type, String> {
        match binding {
            Binding::Void => Ok(Type::Void),
            Binding::Integer { ty, .. } => Ok(ty.clone()),
            Binding::Boolean { .. } => Ok(Type::Bool),
            Binding::Array { ty, .. } => Ok(ty.clone()),
            Binding::Pointer { ty, .. } => Ok(ty.clone()),
            Binding::Slice { ty, .. } => Ok(ty.clone()),
            Binding::Function { .. } => Ok(Type::Pointer {
                element: Box::new(Type::Function {
                    parameters: vec![],
                    return_type: Box::new(Type::Void),
                    is_variadic: false,
                }),
            }),
        }
    }

    fn binding_basic_value(&self, binding: &Binding<'ctx>) -> Result<BasicValueEnum<'ctx>, String> {
        match binding {
            Binding::Void => Err("cannot treat void as a basic value".to_string()),
            Binding::Integer { value, .. } => Ok((*value).into()),
            Binding::Boolean { value } => Ok((*value).into()),
            Binding::Array { value, .. } => Ok((*value).into()),
            Binding::Pointer { value, .. } => Ok((*value).into()),
            Binding::Slice { value, .. } => Ok((*value).into()),
            Binding::Function { value } => Ok(value.as_global_value().as_pointer_value().into()),
        }
    }

    fn binding_int_value(&self, binding: &Binding<'ctx>) -> Result<IntValue<'ctx>, String> {
        match binding {
            Binding::Integer { value, .. } => Ok(*value),
            Binding::Boolean { value } => Ok(*value),
            _ => Err("expected integer or boolean value".to_string()),
        }
    }

    fn build_return(&self, binding: &Binding<'ctx>) -> Result<(), String> {
        match binding {
            Binding::Void => {
                self.builder
                    .build_return(None)
                    .map_err(|error| error.to_string())?;
            }
            _ => {
                let value = self.binding_basic_value(binding)?;
                self.builder
                    .build_return(Some(&value))
                    .map_err(|error| error.to_string())?;
            }
        }
        Ok(())
    }

    fn llvm_type(&self, ty: &Type) -> Result<BasicTypeEnum<'ctx>, String> {
        match ty {
            Type::Void => Err("void type is not a basic type in LLVM".to_string()),
            Type::S16 | Type::U16 => Ok(self.context.i16_type().into()),
            Type::S32 | Type::U32 => Ok(self.context.i32_type().into()),
            Type::S64 | Type::U64 => Ok(self.context.i64_type().into()),
            Type::U8 => Ok(self.context.i8_type().into()),
            Type::Bool => Ok(self.context.bool_type().into()),

            Type::Array { element, size } => {
                Ok(self.llvm_type(element)?.array_type(*size as u32).into())
            }

            Type::Pointer { .. } | Type::Function { .. } => {
                Ok(self.context.ptr_type(AddressSpace::default()).into())
            }

            Type::Slice { .. } => Ok(self.slice_type().into()),
        }
    }

    fn zero_value(&self, ty: &Type) -> Result<BasicValueEnum<'ctx>, String> {
        match ty {
            Type::Void => Err("void has no zero value".to_string()),
            Type::S16 | Type::U16 => Ok(self.context.i16_type().const_zero().into()),
            Type::S32 | Type::U32 => Ok(self.context.i32_type().const_zero().into()),
            Type::S64 | Type::U64 => Ok(self.context.i64_type().const_zero().into()),
            Type::U8 => Ok(self.context.i8_type().const_zero().into()),
            Type::Bool => Ok(self.context.bool_type().const_zero().into()),

            Type::Array { element, size } => {
                let array_type = self
                    .llvm_type(&Type::Array {
                        element: element.clone(),
                        size: *size,
                    })?
                    .into_array_type();

                Ok(array_type.const_zero().into())
            }

            Type::Pointer { .. } | Type::Function { .. } => Ok(self
                .context
                .ptr_type(AddressSpace::default())
                .const_null()
                .into()),

            Type::Slice { .. } => Ok(self.slice_type().const_zero().into()),
        }
    }

    fn is_signed(&self, ty: &Type) -> bool {
        matches!(ty, Type::S16 | Type::S32 | Type::S64)
    }

    fn has_terminator(&self, function: FunctionValue<'ctx>) -> bool {
        function
            .get_last_basic_block()
            .and_then(|block| block.get_terminator())
            .is_some()
    }
}
