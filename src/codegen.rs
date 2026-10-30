use std::collections::HashMap;

use inkwell::builder::Builder;
use inkwell::context::Context;
use inkwell::module::Module;
use inkwell::types::{BasicMetadataTypeEnum, BasicType, BasicTypeEnum, FunctionType, StructType};
use inkwell::values::{
    ArrayValue, BasicValueEnum, FunctionValue, IntValue, PointerValue, StructValue,
};
use inkwell::{AddressSpace, IntPredicate};

use crate::sema::Checker;
use crate::syntax::{
    BinaryOperator, ComparisonOperator, Definition, DoIfArm, Expression, ExternalFunction, IfArm,
    IfCondition, Program, Signature, Statement, Type, UnaryOperator,
};

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
    Atom {
        name: String,
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
    },
    Function {
        signature: Signature,
        value: FunctionValue<'ctx>,
    },
}

#[derive(Debug, Clone)]
struct LocalVar<'ctx> {
    ty: Type,
    alloca: PointerValue<'ctx>,
}

#[derive(Debug, Clone)]
struct GlobalVar<'ctx> {
    ty: Type,
    pointer: PointerValue<'ctx>,
}

pub struct Codegen<'ctx> {
    context: &'ctx Context,
    module: Module<'ctx>,
    builder: Builder<'ctx>,

    local_scopes: Vec<HashMap<String, LocalVar<'ctx>>>,
    scope_stack_ptrs: Vec<PointerValue<'ctx>>,
    global_vars: HashMap<String, GlobalVar<'ctx>>,
    extern_functions: HashMap<String, Signature>,
    atom_table: HashMap<String, u32>,

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
            scope_stack_ptrs: Vec::new(),
            global_vars: HashMap::new(),
            extern_functions: HashMap::new(),
            atom_table: HashMap::new(),
            string_counter: 0,
            branch_counter: 0,
            anon_counter: 0,
        }
    }

    pub fn generate(mut self, program: &Program) -> Result<Module<'ctx>, String> {
        self.collect_declarations(program)?;
        self.codegen_externals(&program.extern_functions)?;
        self.codegen_definitions(&program.definitions)?;
        Ok(self.module)
    }

    fn collect_declarations(&mut self, program: &Program) -> Result<(), String> {
        for external in &program.extern_functions {
            self.extern_functions
                .insert(external.name.clone(), external.signature.clone());
        }
        Ok(())
    }

    fn get_atom_id(&mut self, name: &str) -> u32 {
        if let Some(&id) = self.atom_table.get(name) {
            id
        } else {
            let id = self.atom_table.len() as u32;
            self.atom_table.insert(name.to_string(), id);
            id
        }
    }

    fn get_stacksave_intrinsic(&self) -> FunctionValue<'ctx> {
        if let Some(func) = self.module.get_function("llvm.stacksave.p0") {
            func
        } else {
            let ptr_type = self.context.ptr_type(AddressSpace::default());
            let fn_type = ptr_type.fn_type(&[], false);
            self.module.add_function("llvm.stacksave.p0", fn_type, None)
        }
    }

    fn get_stackrestore_intrinsic(&self) -> FunctionValue<'ctx> {
        if let Some(func) = self.module.get_function("llvm.stackrestore.p0") {
            func
        } else {
            let ptr_type = self.context.ptr_type(AddressSpace::default());
            let fn_type = self.context.void_type().fn_type(&[ptr_type.into()], false);
            self.module
                .add_function("llvm.stackrestore.p0", fn_type, None)
        }
    }

    fn enter_scope(&mut self) -> Result<PointerValue<'ctx>, String> {
        let stacksave = self.get_stacksave_intrinsic();
        let stack_ptr = self
            .builder
            .build_call(stacksave, &[], "saved_stack")
            .map_err(|e| e.to_string())?
            .try_as_basic_value()
            .basic()
            .ok_or_else(|| "llvm.stacksave.p0 must yield a basic value".to_string())?
            .into_pointer_value();

        self.local_scopes.push(HashMap::new());
        self.scope_stack_ptrs.push(stack_ptr);
        Ok(stack_ptr)
    }

    fn exit_scope(&mut self, stack_ptr: PointerValue<'ctx>) -> Result<(), String> {
        self.local_scopes.pop();
        self.scope_stack_ptrs.pop();

        if let Some(bb) = self.builder.get_insert_block() {
            if bb.get_terminator().is_none() {
                let stackrestore = self.get_stackrestore_intrinsic();
                self.builder
                    .build_call(stackrestore, &[stack_ptr.into()], "")
                    .map_err(|e| e.to_string())?;
            }
        }
        Ok(())
    }

    fn exit_current_scope(&mut self) -> Result<(), String> {
        if let Some(&stack_ptr) = self.scope_stack_ptrs.last() {
            if let Some(bb) = self.builder.get_insert_block() {
                if bb.get_terminator().is_none() {
                    let stackrestore = self.get_stackrestore_intrinsic();
                    self.builder
                        .build_call(stackrestore, &[stack_ptr.into()], "")
                        .map_err(|e| e.to_string())?;
                }
            }
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
                    let llvm_ty = self.llvm_type(&param.ty)?;
                    Ok(llvm_ty.into())
                })
                .collect::<Result<Vec<_>, String>>()?;

            let ret_type = &external.signature.return_type;

            let fn_type = match self.llvm_type(ret_type) {
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

                let init_val = match self.binding_basic_value(&binding) {
                    Ok(val) => val,
                    Err(_) => self.zero_value(&ty)?,
                };
                global.set_initializer(&init_val);

                self.global_vars.insert(
                    def.name.clone(),
                    GlobalVar {
                        ty,
                        pointer: global.as_pointer_value(),
                    },
                );
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
                    let llvm_ty = self.llvm_type(&p.ty)?;
                    Ok(BasicMetadataTypeEnum::from(llvm_ty))
                })
                .collect::<Result<Vec<_>, String>>()?;

            let ret_type = &sig.return_type;
            if **ret_type == Type::Void {
                Ok(self
                    .context
                    .void_type()
                    .fn_type(&param_types, sig.is_variadic))
            } else {
                Ok(self
                    .llvm_type(ret_type)?
                    .fn_type(&param_types, sig.is_variadic))
            }
        } else {
            Ok(self.context.void_type().fn_type(&[], false))
        }
    }

    fn lookup_local(&self, name: &str) -> Option<LocalVar<'ctx>> {
        self.local_scopes
            .iter()
            .rev()
            .find_map(|scope| scope.get(name).cloned())
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

        let previous_scopes = std::mem::take(&mut self.local_scopes);
        let previous_stack_ptrs = std::mem::take(&mut self.scope_stack_ptrs);
        let stack_ptr = self.enter_scope()?;

        if let Some(sig) = signature {
            for (index, parameter) in sig.parameters.iter().enumerate() {
                let ty = &parameter.ty;
                let llvm_ty = self.llvm_type(ty)?;
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

                self.local_scopes.last_mut().unwrap().insert(
                    parameter.name.clone(),
                    LocalVar {
                        ty: ty.clone(),
                        alloca,
                    },
                );
            }
        }

        self.codegen_statements(body)?;

        let needs_terminator = self
            .builder
            .get_insert_block()
            .map_or(false, |bb| bb.get_terminator().is_none());

        if needs_terminator {
            let ret_ty = if let Some(sig) = signature {
                (*sig.return_type).clone()
            } else {
                Type::Void
            };

            self.exit_scope(stack_ptr)?;

            if ret_ty == Type::Void {
                self.builder.build_return(None).map_err(|e| e.to_string())?;
            } else {
                let unreachable_bb = self
                    .context
                    .append_basic_block(llvm_function, "unreachable");
                self.builder
                    .build_unconditional_branch(unreachable_bb)
                    .map_err(|e| e.to_string())?;
                self.builder.position_at_end(unreachable_bb);
                self.builder
                    .build_unreachable()
                    .map_err(|e| e.to_string())?;
            }
        } else {
            self.local_scopes.pop();
            self.scope_stack_ptrs.pop();
        }

        self.local_scopes = previous_scopes;
        self.scope_stack_ptrs = previous_stack_ptrs;
        Ok(())
    }

    fn codegen_main(&mut self, main_def: &Definition) -> Result<(), String> {
        let i32_type = self.context.i32_type();
        let function_type = i32_type.fn_type(&[], false);

        let function = self.module.add_function("main", function_type, None);
        let entry = self.context.append_basic_block(function, "entry");

        self.builder.position_at_end(entry);
        let previous_scopes = std::mem::take(&mut self.local_scopes);
        let previous_stack_ptrs = std::mem::take(&mut self.scope_stack_ptrs);
        let stack_ptr = self.enter_scope()?;

        if let Expression::Block { body, .. } = &main_def.value {
            self.codegen_statements(body)?;
        } else {
            return Err("entry point '@main' must evaluate to a block".to_string());
        }

        let needs_terminator = self
            .builder
            .get_insert_block()
            .map_or(false, |bb| bb.get_terminator().is_none());

        if needs_terminator {
            self.exit_scope(stack_ptr)?;
            let unreachable_bb = self.context.append_basic_block(function, "unreachable");
            self.builder
                .build_unconditional_branch(unreachable_bb)
                .map_err(|e| e.to_string())?;
            self.builder.position_at_end(unreachable_bb);
            self.builder
                .build_unreachable()
                .map_err(|e| e.to_string())?;
        } else {
            self.local_scopes.pop();
            self.scope_stack_ptrs.pop();
        }

        self.local_scopes = previous_scopes;
        self.scope_stack_ptrs = previous_stack_ptrs;
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
            Statement::Compound(statements) => {
                self.codegen_statements(statements)?;
                Ok(())
            }
            Statement::Declare { name, ty } => {
                let llvm_ty = self.llvm_type(ty)?;

                let alloca = self
                    .builder
                    .build_alloca(llvm_ty, name)
                    .map_err(|e| e.to_string())?;

                self.local_scopes.last_mut().unwrap().insert(
                    name.clone(),
                    LocalVar {
                        ty: ty.clone(),
                        alloca,
                    },
                );
                Ok(())
            }
            Statement::Assign { expression, target } => {
                let current_binding = self.codegen_expression(expression)?;

                let local = if let Some(existing) = self.lookup_local(target) {
                    existing
                } else {
                    let ty = self.binding_type(&current_binding)?;
                    let llvm_ty = self.llvm_type(&ty)?;
                    let alloca = self
                        .builder
                        .build_alloca(llvm_ty, target)
                        .map_err(|e| e.to_string())?;

                    let new_local = LocalVar {
                        ty: ty.clone(),
                        alloca,
                    };
                    self.local_scopes
                        .last_mut()
                        .unwrap()
                        .insert(target.clone(), new_local.clone());
                    new_local
                };

                let value = self.binding_basic_value(&current_binding)?;
                self.builder
                    .build_store(local.alloca, value)
                    .map_err(|e| e.to_string())?;

                Ok(())
            }
            Statement::Discard(expression) => {
                self.codegen_expression(expression)?;
                Ok(())
            }
            Statement::Return(expression) => {
                let binding = self.codegen_expression(expression)?;
                self.exit_current_scope()?;
                self.build_return(&binding)
            }
            Statement::If { arms } => self.codegen_doif_statement(arms),
        }
    }

    fn codegen_doif_statement(&mut self, arms: &[DoIfArm]) -> Result<(), String> {
        let current_block = self
            .builder
            .get_insert_block()
            .ok_or_else(|| "cannot generate doif outside basic block".to_string())?;

        let current_fn = current_block
            .get_parent()
            .ok_or_else(|| "current block has no parent function".to_string())?;

        let id = self.branch_counter;
        self.branch_counter += 1;

        let merge_bb = self
            .context
            .append_basic_block(current_fn, &format!("doif.merge.{}", id));

        let first_test_bb = self
            .context
            .append_basic_block(current_fn, &format!("doif.arm.0.{}", id));

        self.builder
            .build_unconditional_branch(first_test_bb)
            .map_err(|e| e.to_string())?;

        let mut next_test_bb = first_test_bb;

        for (i, arm) in arms.iter().enumerate() {
            self.builder.position_at_end(next_test_bb);

            match &arm.condition {
                IfCondition::Expression(cond_expr) => {
                    let cond_binding = self.codegen_expression(cond_expr)?;
                    let cond_val = match cond_binding {
                        Binding::Boolean { value } => value,
                        _ => return Err("doif condition must evaluate to a boolean".to_string()),
                    };

                    let then_bb = self
                        .context
                        .append_basic_block(current_fn, &format!("doif.then.{}.{}", i, id));
                    let false_bb = self
                        .context
                        .append_basic_block(current_fn, &format!("doif.arm.{}.{}", i + 1, id));

                    self.builder
                        .build_conditional_branch(cond_val, then_bb, false_bb)
                        .map_err(|e| e.to_string())?;

                    self.builder.position_at_end(then_bb);
                    self.codegen_statement(&arm.body)?;

                    if let Some(bb) = self.builder.get_insert_block() {
                        if bb.get_terminator().is_none() {
                            self.builder
                                .build_unconditional_branch(merge_bb)
                                .map_err(|e| e.to_string())?;
                        }
                    }

                    next_test_bb = false_bb;
                }
                IfCondition::Else => {
                    self.codegen_statement(&arm.body)?;

                    if let Some(bb) = self.builder.get_insert_block() {
                        if bb.get_terminator().is_none() {
                            self.builder
                                .build_unconditional_branch(merge_bb)
                                .map_err(|e| e.to_string())?;
                        }
                    }
                }
            }
        }

        if merge_bb.get_first_use().is_none() {
            unsafe {
                merge_bb
                    .delete()
                    .map_err(|_| "failed to delete basic block".to_string())?;
            }
        } else {
            self.builder.position_at_end(merge_bb);
        }

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
            Expression::Atom(name) => {
                let id = self.get_atom_id(name);
                let value = self.context.i32_type().const_int(id as u64, false);
                Ok(Binding::Atom {
                    name: name.clone(),
                    value,
                })
            }
            Expression::String(value) => self.codegen_string_literal(value),
            Expression::Identifier(name) => {
                if let Some(local) = self.lookup_local(name) {
                    let loaded = self
                        .builder
                        .build_load(self.llvm_type(&local.ty)?, local.alloca, name)
                        .map_err(|e| e.to_string())?;
                    return self.binding_from_basic_value(&local.ty, loaded);
                }

                if let Some(global_var) = self.global_vars.get(name) {
                    let loaded = self
                        .builder
                        .build_load(self.llvm_type(&global_var.ty)?, global_var.pointer, name)
                        .map_err(|e| e.to_string())?;
                    return self.binding_from_basic_value(&global_var.ty, loaded);
                }

                if let Some(function) = self.module.get_function(name) {
                    let sig = if let Some(sig) = self.extern_functions.get(name) {
                        sig.clone()
                    } else {
                        Signature {
                            parameters: vec![],
                            return_type: Box::new(Type::Void),
                            is_variadic: false,
                        }
                    };
                    return Ok(Binding::Function {
                        signature: sig,
                        value: function,
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

                let sig = signature.clone().unwrap_or(Signature {
                    parameters: vec![],
                    return_type: Box::new(Type::Void),
                    is_variadic: false,
                });

                Ok(Binding::Function {
                    signature: sig,
                    value: function,
                })
            }
            Expression::If { arms } => self.codegen_if_expression(arms),
            Expression::Array(elements) => self.codegen_array_literal(elements),
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
                self.codegen_cast(value, ty.clone())
            }
            Expression::Call { target, arguments } => self.codegen_call(target, arguments),
            Expression::Index { expression, index } => self.codegen_index(expression, index),
            Expression::Member { expression, field } => self.codegen_member(expression, field),
            Expression::AddressOf(expression) => self.codegen_address_of(expression),
            Expression::Decay(expression) => self.codegen_decay(expression),
        }
    }

    fn codegen_if_expression(&mut self, arms: &[IfArm]) -> Result<Binding<'ctx>, String> {
        let current_block = self
            .builder
            .get_insert_block()
            .ok_or_else(|| "cannot generate if outside basic block".to_string())?;

        let current_fn = current_block
            .get_parent()
            .ok_or_else(|| "current block has no parent function".to_string())?;

        let id = self.branch_counter;
        self.branch_counter += 1;

        let merge_bb = self
            .context
            .append_basic_block(current_fn, &format!("if.merge.{}", id));

        let first_test_bb = self
            .context
            .append_basic_block(current_fn, &format!("if.arm.0.{}", id));

        self.builder
            .build_unconditional_branch(first_test_bb)
            .map_err(|e| e.to_string())?;

        let mut next_test_bb = first_test_bb;
        let mut incoming: Vec<(BasicValueEnum<'ctx>, inkwell::basic_block::BasicBlock<'ctx>)> =
            Vec::new();
        let mut result_type: Option<Type> = None;

        for (i, arm) in arms.iter().enumerate() {
            self.builder.position_at_end(next_test_bb);

            match &arm.condition {
                IfCondition::Expression(cond_expr) => {
                    let cond_binding = self.codegen_expression(cond_expr)?;
                    let cond_val = match cond_binding {
                        Binding::Boolean { value } => value,
                        _ => return Err("if condition must evaluate to a boolean".to_string()),
                    };

                    let then_bb = self
                        .context
                        .append_basic_block(current_fn, &format!("if.then.{}.{}", i, id));
                    let false_bb = self
                        .context
                        .append_basic_block(current_fn, &format!("if.arm.{}.{}", i + 1, id));

                    self.builder
                        .build_conditional_branch(cond_val, then_bb, false_bb)
                        .map_err(|e| e.to_string())?;

                    self.builder.position_at_end(then_bb);
                    let body_binding = self.codegen_expression(&arm.body)?;

                    if result_type.is_none() {
                        result_type = Some(self.binding_type(&body_binding)?);
                    }

                    let end_of_arm = self.builder.get_insert_block().unwrap();
                    if end_of_arm.get_terminator().is_none() {
                        if let Ok(val) = self.binding_basic_value(&body_binding) {
                            incoming.push((val, end_of_arm));
                        }
                        self.builder
                            .build_unconditional_branch(merge_bb)
                            .map_err(|e| e.to_string())?;
                    }

                    next_test_bb = false_bb;
                }
                IfCondition::Else => {
                    let body_binding = self.codegen_expression(&arm.body)?;

                    if result_type.is_none() {
                        result_type = Some(self.binding_type(&body_binding)?);
                    }

                    let end_of_arm = self.builder.get_insert_block().unwrap();
                    if end_of_arm.get_terminator().is_none() {
                        if let Ok(val) = self.binding_basic_value(&body_binding) {
                            incoming.push((val, end_of_arm));
                        }
                        self.builder
                            .build_unconditional_branch(merge_bb)
                            .map_err(|e| e.to_string())?;
                    }
                }
            }
        }

        self.builder.position_at_end(merge_bb);

        let ty = result_type.unwrap_or(Type::Void);
        if ty == Type::Void {
            Ok(Binding::Void)
        } else {
            let llvm_ty = self.llvm_type(&ty)?;
            let phi = self
                .builder
                .build_phi(llvm_ty, &format!("if.res.{}", id))
                .map_err(|e| e.to_string())?;

            for (val, bb) in &incoming {
                phi.add_incoming(&[(val, *bb)]);
            }

            self.binding_from_basic_value(&ty, phi.as_basic_value())
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
                    let zero = self.context.i64_type().const_zero();
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

                let zero = self.context.i64_type().const_zero();
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
            Type::Atom(_) => {
                let values = bindings
                    .iter()
                    .map(|b| match b {
                        Binding::Atom { value, .. } => Ok(*value),
                        _ => Err("expected atom element".to_string()),
                    })
                    .collect::<Result<Vec<_>, String>>()?;

                self.context.i32_type().const_array(&values)
            }
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

        let (left_value, right_value) = match (left, right) {
            (Binding::Atom { value: v1, .. }, Binding::Atom { value: v2, .. }) => (*v1, *v2),
            _ => (
                self.binding_int_value(left)?,
                self.binding_int_value(right)?,
            ),
        };

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

        if Checker::is_integer_type(&source_type)
            && matches!(target_type, Type::Pointer { .. } | Type::Function { .. })
        {
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

        if matches!(source_type, Type::Pointer { .. } | Type::Function { .. })
            && Checker::is_integer_type(&target_type)
        {
            let ptr_val = match binding {
                Binding::Pointer { value, .. } => value,
                Binding::Function { value, .. } => value.as_global_value().as_pointer_value(),
                _ => return Err("expected pointer or function binding".to_string()),
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
                    let (return_type, fn_type) = if let Some(sig) = self.extern_functions.get(name)
                    {
                        let fn_ty = self.signature_to_function_type(Some(sig))?;
                        ((*sig.return_type).clone(), fn_ty)
                    } else {
                        let ret = match func.get_type().get_return_type() {
                            Some(_) => Type::S32,
                            None => Type::Void,
                        };
                        (ret, func.get_type())
                    };
                    Ok((
                        func.as_global_value().as_pointer_value(),
                        return_type,
                        fn_type,
                    ))
                } else if let Some(local) = self.lookup_local(name) {
                    let loaded = self
                        .builder
                        .build_load(self.llvm_type(&local.ty)?, local.alloca, "fn_ptr")
                        .map_err(|e| e.to_string())?
                        .into_pointer_value();

                    let (ret_ty, fn_type) = self.function_type_info(&local.ty)?;
                    Ok((loaded, ret_ty, fn_type))
                } else if let Some(global_var) = self.global_vars.get(name) {
                    let loaded = self
                        .builder
                        .build_load(
                            self.llvm_type(&global_var.ty)?,
                            global_var.pointer,
                            "fn_ptr",
                        )
                        .map_err(|e| e.to_string())?
                        .into_pointer_value();

                    let (ret_ty, fn_type) = self.function_type_info(&global_var.ty)?;
                    Ok((loaded, ret_ty, fn_type))
                } else {
                    Err(format!("unknown function or callable variable '{}'", name))
                }
            }
            expr => {
                let target_binding = self.codegen_expression(expr)?;
                match target_binding {
                    Binding::Function { signature, value } => {
                        let fn_type = self.signature_to_function_type(Some(&signature))?;
                        Ok((
                            value.as_global_value().as_pointer_value(),
                            *signature.return_type,
                            fn_type,
                        ))
                    }
                    Binding::Pointer { value, ty } => {
                        let (ret_ty, fn_type) = self.function_type_info(&ty)?;
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

    fn function_type_info(&self, ty: &Type) -> Result<(Type, FunctionType<'ctx>), String> {
        match ty {
            Type::Function {
                parameters,
                return_type,
                is_variadic,
            } => {
                let param_types = parameters
                    .iter()
                    .map(|p| {
                        let llvm_ty = self.llvm_type(p)?;
                        Ok(BasicMetadataTypeEnum::from(llvm_ty))
                    })
                    .collect::<Result<Vec<_>, String>>()?;

                let fn_type = if **return_type == Type::Void {
                    self.context.void_type().fn_type(&param_types, *is_variadic)
                } else {
                    self.llvm_type(return_type)?
                        .fn_type(&param_types, *is_variadic)
                };

                Ok((*(return_type.clone()), fn_type))
            }
            Type::Pointer { element } => self.function_type_info(element),
            _ => Err(format!(
                "type {:?} is not a function or function pointer",
                ty
            )),
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
        let index_i64 = self
            .builder
            .build_int_z_extend_or_bit_cast(index_value, self.context.i64_type(), "idx64")
            .map_err(|e| e.to_string())?;

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
                            &[self.context.i64_type().const_zero(), index_i64],
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
                        .build_gep(elem_llvm_type, ptr_val, &[index_i64], "sliceidx")
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

                if let Some(global_var) = self.global_vars.get(name) {
                    return Ok(Binding::Pointer {
                        ty: Type::Pointer {
                            element: Box::new(global_var.ty.clone()),
                        },
                        value: global_var.pointer,
                    });
                }

                Err(format!("unknown variable '{}'", name))
            }
            Expression::Index { expression, index } => {
                let binding = self.codegen_expression(expression)?;
                let index_binding = self.codegen_expression(index)?;
                let index_value = self.binding_int_value(&index_binding)?;
                let index_i64 = self
                    .builder
                    .build_int_z_extend_or_bit_cast(index_value, self.context.i64_type(), "idx64")
                    .map_err(|e| e.to_string())?;

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
                                    &[self.context.i64_type().const_zero(), index_i64],
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
                                .build_gep(elem_llvm_type, ptr_val, &[index_i64], "sliceaddr")
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
        })
    }

    fn binding_from_basic_value(
        &self,
        ty: &Type,
        value: BasicValueEnum<'ctx>,
    ) -> Result<Binding<'ctx>, String> {
        match ty {
            Type::Void => Ok(Binding::Void),
            Type::Atom(name) => Ok(Binding::Atom {
                name: name.clone(),
                value: value.into_int_value(),
            }),
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
            Binding::Atom { name, .. } => Ok(Type::Atom(name.clone())),
            Binding::Integer { ty, .. } => Ok(ty.clone()),
            Binding::Boolean { .. } => Ok(Type::Bool),
            Binding::Array { ty, .. } => Ok(ty.clone()),
            Binding::Pointer { ty, .. } => Ok(ty.clone()),
            Binding::Slice { ty, .. } => Ok(ty.clone()),
            Binding::Function { signature, .. } => {
                let parameters = signature.parameters.iter().map(|p| p.ty.clone()).collect();
                Ok(Type::Function {
                    parameters,
                    return_type: signature.return_type.clone(),
                    is_variadic: signature.is_variadic,
                })
            }
        }
    }

    fn binding_basic_value(&self, binding: &Binding<'ctx>) -> Result<BasicValueEnum<'ctx>, String> {
        match binding {
            Binding::Void => Err("cannot treat void as a basic value".to_string()),
            Binding::Atom { value, .. } => Ok((*value).into()),
            Binding::Integer { value, .. } => Ok((*value).into()),
            Binding::Boolean { value } => Ok((*value).into()),
            Binding::Array { value, .. } => Ok((*value).into()),
            Binding::Pointer { value, .. } => Ok((*value).into()),
            Binding::Slice { value, .. } => Ok((*value).into()),
            Binding::Function { value, .. } => {
                Ok(value.as_global_value().as_pointer_value().into())
            }
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
            Type::Atom(_) => Ok(self.context.i32_type().into()),
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
            Type::Atom(_) => Ok(self.context.i32_type().const_zero().into()),
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
}
