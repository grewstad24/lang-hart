use std::collections::{HashMap, HashSet};

use inkwell::builder::Builder;
use inkwell::context::Context;
use inkwell::module::Module;
use inkwell::targets::{TargetData, TargetMachine};
use inkwell::types::{
    BasicMetadataTypeEnum, BasicType, BasicTypeEnum, FunctionType, IntType, StructType,
};
use inkwell::values::{
    ArrayValue, BasicValueEnum, FunctionValue, IntValue, PointerValue, StructValue,
};
use inkwell::{AddressSpace, IntPredicate};

use crate::sema::Checker;
use crate::syntax::{
    BinaryOperator, ComparisonOperator, Definition, DoIfArm, Expression, ExternalFunction, IfArm,
    IfCondition, MatchArm, Program, Signature, Statement, StructFieldInit, StructFieldType, Type,
    UnaryOperator, format_error, unspanned_expression,
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
    Struct {
        ty: Type,
        value: StructValue<'ctx>,
        storage: Option<PointerValue<'ctx>>,
    },
    Union {
        ty: Type,
        value: StructValue<'ctx>,
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
    target_data: TargetData,

    local_scopes: Vec<HashMap<String, LocalVar<'ctx>>>,
    scope_stack_ptrs: Vec<PointerValue<'ctx>>,
    global_vars: HashMap<String, GlobalVar<'ctx>>,
    pending_global_records: Vec<(String, Vec<StructFieldInit>)>,
    extern_functions: HashMap<String, Signature>,
    function_signatures: HashMap<String, Signature>,
    type_aliases: HashMap<String, Type>,
    atom_table: HashMap<String, u32>,

    string_counter: usize,
    branch_counter: usize,
    anon_counter: usize,
}

impl<'ctx> Codegen<'ctx> {
    pub fn new(context: &'ctx Context, module_name: &str, target_machine: &TargetMachine) -> Self {
        let module = context.create_module(module_name);
        let target_data = target_machine.get_target_data();
        module.set_triple(&target_machine.get_triple());
        module.set_data_layout(&target_data.get_data_layout());

        Self {
            context,
            module,
            builder: context.create_builder(),
            target_data,
            local_scopes: Vec::new(),
            scope_stack_ptrs: Vec::new(),
            global_vars: HashMap::new(),
            pending_global_records: Vec::new(),
            extern_functions: HashMap::new(),
            function_signatures: HashMap::new(),
            type_aliases: HashMap::new(),
            atom_table: HashMap::new(),
            string_counter: 0,
            branch_counter: 0,
            anon_counter: 0,
        }
    }

    fn ptr_sized_int_type(&self) -> IntType<'ctx> {
        self.context
            .ptr_sized_int_type(&self.target_data, Some(AddressSpace::default()))
    }

    fn ptr_byte_size(&self) -> u64 {
        self.target_data
            .get_pointer_byte_size(Some(AddressSpace::default())) as u64
    }

    fn resolve_alias_root<'a>(&'a self, mut ty: &'a Type) -> Result<&'a Type, String> {
        let mut visited = HashSet::new();
        while let Type::Alias(name) = ty {
            if !visited.insert(name.clone()) {
                return Err(format!("recursive type alias '{}' reached codegen", name));
            }
            ty = self
                .type_aliases
                .get(name)
                .ok_or_else(|| format!("unknown type alias '{}' in codegen", name))?;
        }
        Ok(ty)
    }

    pub fn generate(mut self, program: &Program) -> Result<Module<'ctx>, String> {
        self.type_aliases = program
            .type_aliases
            .iter()
            .map(|alias| (alias.name.clone(), alias.target.clone()))
            .collect();
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
            if let Expression::Block { signature, .. } = unspanned_expression(&def.value) {
                if let Some(sig) = signature {
                    self.function_signatures
                        .insert(def.name.clone(), sig.clone());
                }
                let fn_type = self.signature_to_function_type(signature.as_ref())?;
                self.module.add_function(&def.name, fn_type, None);
            }
        }

        for def in definitions {
            if def.name == "main" {
                continue;
            }
            if let Expression::Record(fields) = unspanned_expression(&def.value) {
                let ty = self.resolve_alias_root(&Checker::record_type())?.clone();
                let llvm_type = self.llvm_type(&ty)?;
                let global =
                    self.module
                        .add_global(llvm_type, Some(AddressSpace::default()), &def.name);
                global.set_initializer(&self.zero_value(&ty)?);
                self.global_vars.insert(
                    def.name.clone(),
                    GlobalVar {
                        ty,
                        pointer: global.as_pointer_value(),
                    },
                );
                self.pending_global_records
                    .push((def.name.clone(), fields.clone()));
            }
        }

        for def in definitions {
            if def.name == "main" {
                continue;
            }
            if matches!(unspanned_expression(&def.value), Expression::Record(_)) {
                continue;
            }

            if let Expression::Block { signature, body } = unspanned_expression(&def.value) {
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

        if let Some(main_def) = definitions.iter().find(|def| def.name == "main") {
            self.codegen_main(main_def)?;
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

        for (name, fields) in std::mem::take(&mut self.pending_global_records) {
            let record = self.codegen_record_literal(&fields)?;
            let value = self.binding_basic_value(&record)?;
            let global = self
                .global_vars
                .get(&name)
                .ok_or_else(|| format!("missing global record storage for '{}'", name))?;
            self.builder
                .build_store(global.pointer, value)
                .map_err(|error| error.to_string())?;
        }

        if let Expression::Block { body, .. } = unspanned_expression(&main_def.value) {
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

    fn coerce_to_type(
        &mut self,
        binding: Binding<'ctx>,
        target_type: &Type,
    ) -> Result<Binding<'ctx>, String> {
        let source_type = self.binding_type(&binding)?;
        if source_type == *target_type {
            return Ok(binding);
        }

        if let Type::Union(target_vars) = target_type {
            if let Type::Union(source_vars) = &source_type {
                if source_vars.iter().all(|sv| target_vars.contains(sv)) {
                    return self.codegen_cast(binding, target_type.clone());
                }
            } else if target_vars.contains(&source_type) {
                return self.codegen_cast(binding, target_type.clone());
            }
        }

        Ok(binding)
    }

    fn codegen_statement(&mut self, statement: &Statement) -> Result<(), String> {
        match statement {
            Statement::Spanned { statement, span } => self
                .codegen_statement(statement)
                .map_err(|error| format_error(span, &error)),
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
            Statement::DoTimes { count, body } => {
                let count_binding = self.codegen_expression(count)?;
                let count_val = self.binding_int_value(&count_binding)?;
                let count_is_signed = self.is_signed(&self.binding_type(&count_binding)?);

                let limit = if count_is_signed {
                    self.builder.build_int_s_extend_or_bit_cast(
                        count_val,
                        self.ptr_sized_int_type(),
                        "dotimes_limit",
                    )
                } else {
                    self.builder.build_int_z_extend_or_bit_cast(
                        count_val,
                        self.ptr_sized_int_type(),
                        "dotimes_limit",
                    )
                }
                .map_err(|e| e.to_string())?;

                let current_block = self
                    .builder
                    .get_insert_block()
                    .ok_or_else(|| "cannot generate dotimes outside basic block".to_string())?;
                let current_fn = current_block
                    .get_parent()
                    .ok_or_else(|| "current block has no parent function".to_string())?;

                let id = self.branch_counter;
                self.branch_counter += 1;

                let cond_bb = self
                    .context
                    .append_basic_block(current_fn, &format!("dotimes.cond.{}", id));
                let body_bb = self
                    .context
                    .append_basic_block(current_fn, &format!("dotimes.body.{}", id));
                let merge_bb = self
                    .context
                    .append_basic_block(current_fn, &format!("dotimes.merge.{}", id));

                let counter_alloca = self
                    .builder
                    .build_alloca(
                        self.ptr_sized_int_type(),
                        &format!("dotimes.counter.{}", id),
                    )
                    .map_err(|e| e.to_string())?;

                self.builder
                    .build_store(counter_alloca, self.ptr_sized_int_type().const_zero())
                    .map_err(|e| e.to_string())?;

                self.builder
                    .build_unconditional_branch(cond_bb)
                    .map_err(|e| e.to_string())?;

                // --- Condition Block ---
                self.builder.position_at_end(cond_bb);
                let current_idx = self
                    .builder
                    .build_load(self.ptr_sized_int_type(), counter_alloca, "i")
                    .map_err(|e| e.to_string())?
                    .into_int_value();

                let has_next = self
                    .builder
                    .build_int_compare(
                        if count_is_signed {
                            IntPredicate::SLT
                        } else {
                            IntPredicate::ULT
                        },
                        current_idx,
                        limit,
                        "dotimes_cmp",
                    )
                    .map_err(|e| e.to_string())?;

                self.builder
                    .build_conditional_branch(has_next, body_bb, merge_bb)
                    .map_err(|e| e.to_string())?;

                // --- Body Block ---
                self.builder.position_at_end(body_bb);
                self.codegen_statement(body)?;

                if let Some(bb) = self.builder.get_insert_block() {
                    if bb.get_terminator().is_none() {
                        let cur = self
                            .builder
                            .build_load(self.ptr_sized_int_type(), counter_alloca, "cur_i")
                            .map_err(|e| e.to_string())?
                            .into_int_value();
                        let next = self
                            .builder
                            .build_int_add(
                                cur,
                                self.ptr_sized_int_type().const_int(1, false),
                                "next_i",
                            )
                            .map_err(|e| e.to_string())?;
                        self.builder
                            .build_store(counter_alloca, next)
                            .map_err(|e| e.to_string())?;
                        self.builder
                            .build_unconditional_branch(cond_bb)
                            .map_err(|e| e.to_string())?;
                    }
                }

                self.builder.position_at_end(merge_bb);
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

                let coerced_binding = self.coerce_to_type(current_binding, &local.ty)?;
                let value = self.binding_basic_value(&coerced_binding)?;
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
            Expression::Spanned { expression, span } => self
                .codegen_expression(expression)
                .map_err(|error| format_error(span, &error)),
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
            Expression::Struct(fields) => self.codegen_struct_literal(fields),
            Expression::Record(fields) => self.codegen_record_literal(fields),
            Expression::Identifier(name) => {
                if let Some(local) = self.lookup_local(name) {
                    if let Type::Struct(_) = &local.ty {
                        let loaded = self
                            .builder
                            .build_load(self.llvm_type(&local.ty)?, local.alloca, name)
                            .map_err(|e| e.to_string())?
                            .into_struct_value();

                        return Ok(Binding::Struct {
                            ty: local.ty.clone(),
                            value: loaded,
                            storage: Some(local.alloca),
                        });
                    }

                    if let Type::Union(_) = &local.ty {
                        let loaded = self
                            .builder
                            .build_load(self.llvm_type(&local.ty)?, local.alloca, name)
                            .map_err(|e| e.to_string())?
                            .into_struct_value();

                        return Ok(Binding::Union {
                            ty: local.ty.clone(),
                            value: loaded,
                            storage: Some(local.alloca),
                        });
                    }

                    let loaded = self
                        .builder
                        .build_load(self.llvm_type(&local.ty)?, local.alloca, name)
                        .map_err(|e| e.to_string())?;
                    return self.binding_from_basic_value(&local.ty, loaded);
                }

                if let Some(global_var) = self.global_vars.get(name) {
                    if let Type::Struct(_) = &global_var.ty {
                        let loaded = self
                            .builder
                            .build_load(self.llvm_type(&global_var.ty)?, global_var.pointer, name)
                            .map_err(|e| e.to_string())?
                            .into_struct_value();

                        return Ok(Binding::Struct {
                            ty: global_var.ty.clone(),
                            value: loaded,
                            storage: Some(global_var.pointer),
                        });
                    }

                    if let Type::Union(_) = &global_var.ty {
                        let loaded = self
                            .builder
                            .build_load(self.llvm_type(&global_var.ty)?, global_var.pointer, name)
                            .map_err(|e| e.to_string())?
                            .into_struct_value();

                        return Ok(Binding::Union {
                            ty: global_var.ty.clone(),
                            value: loaded,
                            storage: Some(global_var.pointer),
                        });
                    }

                    let loaded = self
                        .builder
                        .build_load(self.llvm_type(&global_var.ty)?, global_var.pointer, name)
                        .map_err(|e| e.to_string())?;
                    return self.binding_from_basic_value(&global_var.ty, loaded);
                }

                if let Some(function) = self.module.get_function(name) {
                    let sig = if let Some(sig) = self.extern_functions.get(name) {
                        sig.clone()
                    } else if let Some(sig) = self.function_signatures.get(name) {
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
            Expression::Match {
                target,
                binding_name,
                arms,
            } => self.codegen_match_expression(target, binding_name, arms),
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
            Expression::Dereference(expression) => self.codegen_dereference(expression),
        }
    }

    fn codegen_dereference(&mut self, expression: &Expression) -> Result<Binding<'ctx>, String> {
        let pointer_binding = self.codegen_expression(expression)?;
        let pointer_type = self.binding_type(&pointer_binding)?;
        let Type::Pointer { element } = pointer_type else {
            return Err(format!(
                "cannot dereference non-pointer type {:?}",
                pointer_type
            ));
        };
        let pointer_value = match pointer_binding {
            Binding::Pointer { value, .. } => value,
            _ => return Err("expected pointer binding for dereference".to_string()),
        };
        let element_llvm_type = self.llvm_type(&element)?;
        let loaded = self
            .builder
            .build_load(element_llvm_type, pointer_value, "deref_value")
            .map_err(|error| error.to_string())?;

        match element.as_ref() {
            Type::Struct(_) => Ok(Binding::Struct {
                ty: *element,
                value: loaded.into_struct_value(),
                storage: Some(pointer_value),
            }),
            Type::Union(_) => Ok(Binding::Union {
                ty: *element,
                value: loaded.into_struct_value(),
                storage: Some(pointer_value),
            }),
            _ => self.binding_from_basic_value(&element, loaded),
        }
    }

    fn codegen_struct_literal(
        &mut self,
        fields: &[StructFieldInit],
    ) -> Result<Binding<'ctx>, String> {
        let mut field_types = Vec::with_capacity(fields.len());
        let mut field_values = Vec::with_capacity(fields.len());

        for f in fields {
            let binding = self.codegen_expression(&f.value)?;
            let ty = self.binding_type(&binding)?;
            let val = self.binding_basic_value(&binding)?;

            field_types.push(StructFieldType {
                name: f.name.clone(),
                ty,
            });
            field_values.push(val);
        }

        let struct_type = Type::Struct(field_types);
        let llvm_struct_type = self.llvm_type(&struct_type)?.into_struct_type();

        let mut struct_val = llvm_struct_type.get_undef();
        for (i, val) in field_values.into_iter().enumerate() {
            struct_val = self
                .builder
                .build_insert_value(struct_val, val, i as u32, &format!("struct_fld_{}", i))
                .map_err(|e| e.to_string())?
                .into_struct_value();
        }

        Ok(Binding::Struct {
            ty: struct_type,
            value: struct_val,
            storage: None,
        })
    }

    fn codegen_record_literal(
        &mut self,
        fields: &[StructFieldInit],
    ) -> Result<Binding<'ctx>, String> {
        let generic_type = self.resolve_alias_root(&Checker::generic_type())?.clone();
        let key_type = Type::Slice {
            element: Box::new(Type::U8),
        };
        let key_llvm_type = self.llvm_type(&key_type)?;
        let generic_llvm_type = self.llvm_type(&generic_type)?;
        let count = fields.len();
        let count_value = self.ptr_sized_int_type().const_int(count as u64, false);

        let keys_array_type = key_llvm_type.array_type(count as u32);
        let keys_storage = self
            .builder
            .build_alloca(keys_array_type, "record.keys")
            .map_err(|error| error.to_string())?;
        let values_array_type = generic_llvm_type.array_type(count as u32);
        let values_storage = self
            .builder
            .build_alloca(values_array_type, "record.values")
            .map_err(|error| error.to_string())?;
        let zero = self.ptr_sized_int_type().const_zero();

        for (index, field) in fields.iter().enumerate() {
            let key_binding = self.codegen_string_literal(field.name.as_bytes())?;
            let key_value = self.binding_basic_value(&key_binding)?;
            let index_value = self.ptr_sized_int_type().const_int(index as u64, false);
            let key_slot = unsafe {
                self.builder
                    .build_gep(
                        keys_array_type,
                        keys_storage,
                        &[zero, index_value],
                        "record.key.slot",
                    )
                    .map_err(|error| error.to_string())?
            };
            self.builder
                .build_store(key_slot, key_value)
                .map_err(|error| error.to_string())?;

            let value_binding = self.codegen_expression(&field.value)?;
            let boxed_value = self.codegen_cast(value_binding, generic_type.clone())?;
            let value = self.binding_basic_value(&boxed_value)?;
            let value_slot = unsafe {
                self.builder
                    .build_gep(
                        values_array_type,
                        values_storage,
                        &[zero, index_value],
                        "record.value.slot",
                    )
                    .map_err(|error| error.to_string())?
            };
            self.builder
                .build_store(value_slot, value)
                .map_err(|error| error.to_string())?;
        }

        let keys_pointer = unsafe {
            self.builder
                .build_gep(
                    keys_array_type,
                    keys_storage,
                    &[zero, zero],
                    "record.keys.ptr",
                )
                .map_err(|error| error.to_string())?
        };
        let values_pointer = unsafe {
            self.builder
                .build_gep(
                    values_array_type,
                    values_storage,
                    &[zero, zero],
                    "record.values.ptr",
                )
                .map_err(|error| error.to_string())?
        };

        let record_type = self.resolve_alias_root(&Checker::record_type())?.clone();
        let Type::Pointer {
            element: record_struct,
        } = &record_type
        else {
            unreachable!();
        };
        let record_llvm_type = self.llvm_type(record_struct)?.into_struct_type();
        let record_storage = self
            .builder
            .build_alloca(record_llvm_type, "record.header")
            .map_err(|error| error.to_string())?;
        let len_pointer = self
            .builder
            .build_struct_gep(record_llvm_type, record_storage, 0, "record.len")
            .map_err(|error| error.to_string())?;
        self.builder
            .build_store(len_pointer, count_value)
            .map_err(|error| error.to_string())?;
        let keys_field = self
            .builder
            .build_struct_gep(record_llvm_type, record_storage, 1, "record.keys.field")
            .map_err(|error| error.to_string())?;
        self.builder
            .build_store(keys_field, keys_pointer)
            .map_err(|error| error.to_string())?;
        let values_field = self
            .builder
            .build_struct_gep(record_llvm_type, record_storage, 2, "record.values.field")
            .map_err(|error| error.to_string())?;
        self.builder
            .build_store(values_field, values_pointer)
            .map_err(|error| error.to_string())?;

        Ok(Binding::Pointer {
            ty: record_type,
            value: record_storage,
        })
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

    fn codegen_match_expression(
        &mut self,
        target: &Expression,
        binding_name: &str,
        arms: &[MatchArm],
    ) -> Result<Binding<'ctx>, String> {
        let current_block = self
            .builder
            .get_insert_block()
            .ok_or_else(|| "cannot generate match outside basic block".to_string())?;

        let current_fn = current_block
            .get_parent()
            .ok_or_else(|| "current block has no parent function".to_string())?;

        let target_binding = self.codegen_expression(target)?;
        let target_type = self.binding_type(&target_binding)?;

        let target_variants = match &target_type {
            Type::Union(vars) => vars.clone(),
            _ => return Err("match target must be a union type".to_string()),
        };

        let union_llvm_type = self.llvm_type(&target_type)?.into_struct_type();

        let union_storage = match target_binding {
            Binding::Union {
                storage: Some(s), ..
            } => s,
            _ => {
                let val = self.binding_basic_value(&target_binding)?;
                let tmp = self
                    .builder
                    .build_alloca(union_llvm_type, "match_scrutinee")
                    .map_err(|e| e.to_string())?;
                self.builder
                    .build_store(tmp, val)
                    .map_err(|e| e.to_string())?;
                tmp
            }
        };

        let tag_ptr = self
            .builder
            .build_struct_gep(union_llvm_type, union_storage, 0, "tag_ptr")
            .map_err(|e| e.to_string())?;

        let tag_val = self
            .builder
            .build_load(self.context.i8_type(), tag_ptr, "tag")
            .map_err(|e| e.to_string())?
            .into_int_value();

        let id = self.branch_counter;
        self.branch_counter += 1;

        let merge_bb = self
            .context
            .append_basic_block(current_fn, &format!("match.merge.{}", id));
        let default_bb = self
            .context
            .append_basic_block(current_fn, &format!("match.default.{}", id));

        let mut incoming: Vec<(BasicValueEnum<'ctx>, inkwell::basic_block::BasicBlock<'ctx>)> =
            Vec::new();
        let mut result_type: Option<Type> = None;

        let mut switch_cases: Vec<(IntValue<'ctx>, inkwell::basic_block::BasicBlock<'ctx>)> =
            Vec::new();
        let mut arm_bbs: Vec<inkwell::basic_block::BasicBlock<'ctx>> = Vec::new();

        for (arm_idx, arm) in arms.iter().enumerate() {
            let case_bb = self
                .context
                .append_basic_block(current_fn, &format!("match.arm.{}.{}", arm_idx, id));
            arm_bbs.push(case_bb);

            let covered = match &arm.variant_type {
                Type::Union(inner) => inner.clone(),
                single => vec![single.clone()],
            };

            for v in &covered {
                let tag_idx = target_variants
                    .iter()
                    .position(|tv| tv == v)
                    .ok_or_else(|| format!("variant {:?} not in union {:?}", v, target_type))?
                    as u8;

                switch_cases.push((
                    self.context.i8_type().const_int(tag_idx as u64, false),
                    case_bb,
                ));
            }
        }

        self.builder
            .build_switch(tag_val, default_bb, &switch_cases)
            .map_err(|e| e.to_string())?;

        for (arm, case_bb) in arms.iter().zip(arm_bbs.iter()) {
            self.builder.position_at_end(*case_bb);
            self.local_scopes.push(HashMap::new());

            let arm_llvm_ty = self.llvm_type(&arm.variant_type)?;
            let payload_ptr = self
                .builder
                .build_struct_gep(union_llvm_type, union_storage, 1, "payload_ptr")
                .map_err(|e| e.to_string())?;

            if let Type::Union(inner_vars) = &arm.variant_type {
                let sub_union_llvm_ty = arm_llvm_ty.into_struct_type();
                let sub_alloca = self
                    .builder
                    .build_alloca(sub_union_llvm_ty, binding_name)
                    .map_err(|e| e.to_string())?;

                self.builder
                    .build_store(sub_alloca, sub_union_llvm_ty.const_zero())
                    .map_err(|e| e.to_string())?;

                let mut mapping_cases = Vec::new();
                for (inner_idx, iv) in inner_vars.iter().enumerate() {
                    let target_idx = target_variants.iter().position(|tv| tv == iv).unwrap() as u8;
                    mapping_cases.push((target_idx, inner_idx as u8));
                }

                let sub_tag_val = if mapping_cases.len() == 1 {
                    self.context
                        .i8_type()
                        .const_int(mapping_cases[0].1 as u64, false)
                } else {
                    let map_default_bb = self.context.append_basic_block(current_fn, "tag_map.def");
                    let map_merge_bb = self.context.append_basic_block(current_fn, "tag_map.merge");

                    let mut map_branches = Vec::new();
                    for (t_idx, _) in &mapping_cases {
                        let cbb = self.context.append_basic_block(current_fn, "tag_map.case");
                        map_branches
                            .push((self.context.i8_type().const_int(*t_idx as u64, false), cbb));
                    }

                    self.builder
                        .build_switch(tag_val, map_default_bb, &map_branches)
                        .map_err(|e| e.to_string())?;

                    let mut map_phi_incoming = Vec::new();
                    for ((_, in_idx), (_, cbb)) in mapping_cases.iter().zip(map_branches.iter()) {
                        self.builder.position_at_end(*cbb);
                        let val = self.context.i8_type().const_int(*in_idx as u64, false);
                        map_phi_incoming.push((BasicValueEnum::from(val), *cbb));
                        self.builder
                            .build_unconditional_branch(map_merge_bb)
                            .map_err(|e| e.to_string())?;
                    }

                    self.builder.position_at_end(map_default_bb);
                    self.builder
                        .build_unreachable()
                        .map_err(|e| e.to_string())?;

                    self.builder.position_at_end(map_merge_bb);
                    let phi = self
                        .builder
                        .build_phi(self.context.i8_type(), "translated_sub_tag")
                        .map_err(|e| e.to_string())?;
                    for (val, bb) in &map_phi_incoming {
                        phi.add_incoming(&[(val, *bb)]);
                    }
                    phi.as_basic_value().into_int_value()
                };

                let sub_tag_ptr = self
                    .builder
                    .build_struct_gep(sub_union_llvm_ty, sub_alloca, 0, "sub_tag_ptr")
                    .map_err(|e| e.to_string())?;
                self.builder
                    .build_store(sub_tag_ptr, sub_tag_val)
                    .map_err(|e| e.to_string())?;

                let sub_payload_ptr = self
                    .builder
                    .build_struct_gep(sub_union_llvm_ty, sub_alloca, 1, "sub_payload_ptr")
                    .map_err(|e| e.to_string())?;

                let sub_payload_size = self.union_payload_size(inner_vars)?;
                let copy_size = self.ptr_sized_int_type().const_int(sub_payload_size, false);

                self.builder
                    .build_memcpy(sub_payload_ptr, 1, payload_ptr, 1, copy_size)
                    .map_err(|e| e.to_string())?;

                self.local_scopes.last_mut().unwrap().insert(
                    binding_name.to_string(),
                    LocalVar {
                        ty: arm.variant_type.clone(),
                        alloca: sub_alloca,
                    },
                );
            } else {
                let loaded_variant = self
                    .builder
                    .build_load(arm_llvm_ty, payload_ptr, binding_name)
                    .map_err(|e| e.to_string())?;

                let alloca = self
                    .builder
                    .build_alloca(arm_llvm_ty, binding_name)
                    .map_err(|e| e.to_string())?;

                self.builder
                    .build_store(alloca, loaded_variant)
                    .map_err(|e| e.to_string())?;

                self.local_scopes.last_mut().unwrap().insert(
                    binding_name.to_string(),
                    LocalVar {
                        ty: arm.variant_type.clone(),
                        alloca,
                    },
                );
            }

            let arm_binding = self.codegen_expression(&arm.body)?;
            self.local_scopes.pop();

            if result_type.is_none() {
                result_type = Some(self.binding_type(&arm_binding)?);
            }

            let end_bb = self.builder.get_insert_block().unwrap();
            if end_bb.get_terminator().is_none() {
                if let Ok(val) = self.binding_basic_value(&arm_binding) {
                    incoming.push((val, end_bb));
                }
                self.builder
                    .build_unconditional_branch(merge_bb)
                    .map_err(|e| e.to_string())?;
            }
        }

        self.builder.position_at_end(default_bb);
        self.builder
            .build_unreachable()
            .map_err(|e| e.to_string())?;

        self.builder.position_at_end(merge_bb);

        let ty = result_type.unwrap_or(Type::Void);
        if ty == Type::Void {
            Ok(Binding::Void)
        } else {
            let llvm_ty = self.llvm_type(&ty)?;
            let phi = self
                .builder
                .build_phi(llvm_ty, &format!("match.res.{}", id))
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
            Binding::Struct {
                ref ty,
                value,
                storage,
            } => {
                let fields = match ty {
                    Type::Struct(flds) => flds,
                    _ => return Err("expected struct type".to_string()),
                };

                let (idx, field_type) = fields
                    .iter()
                    .enumerate()
                    .find(|(_, f)| f.name == field)
                    .map(|(i, f)| (i, f.ty.clone()))
                    .ok_or_else(|| format!("unknown struct field '{}'", field))?;

                if let Type::Struct(_) = &field_type {
                    if let Some(storage_ptr) = storage {
                        let llvm_struct_ty = self.llvm_type(ty)?.into_struct_type();
                        let field_gep = self
                            .builder
                            .build_struct_gep(llvm_struct_ty, storage_ptr, idx as u32, field)
                            .map_err(|e| e.to_string())?;

                        let loaded = self
                            .builder
                            .build_load(self.llvm_type(&field_type)?, field_gep, field)
                            .map_err(|e| e.to_string())?
                            .into_struct_value();

                        return Ok(Binding::Struct {
                            ty: field_type,
                            value: loaded,
                            storage: Some(field_gep),
                        });
                    }
                }

                if let Some(storage_ptr) = storage {
                    let llvm_struct_ty = self.llvm_type(ty)?.into_struct_type();
                    let field_gep = self
                        .builder
                        .build_struct_gep(llvm_struct_ty, storage_ptr, idx as u32, field)
                        .map_err(|e| e.to_string())?;
                    let loaded = self
                        .builder
                        .build_load(self.llvm_type(&field_type)?, field_gep, field)
                        .map_err(|e| e.to_string())?;
                    self.binding_from_basic_value(&field_type, loaded)
                } else {
                    let extracted = self
                        .builder
                        .build_extract_value(value, idx as u32, field)
                        .map_err(|e| e.to_string())?;
                    self.binding_from_basic_value(&field_type, extracted)
                }
            }
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
                        ty: Type::USize,
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
                    let zero = self.ptr_sized_int_type().const_zero();
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
                    ty: Type::USize,
                    value: self.ptr_sized_int_type().const_int(size as u64, false),
                }),
                _ => Err(format!("unknown array member '.{}'", field)),
            },
            other => Err(format!(
                "member access '.{}' unsupported on {:?}",
                field, other
            )),
        }
    }

    fn codegen_array_literal(&mut self, elements: &[Expression]) -> Result<Binding<'ctx>, String> {
        if elements.is_empty() {
            return Err("cannot generate an empty array literal".to_string());
        }

        let mut bindings = Vec::with_capacity(elements.len());
        for el in elements {
            bindings.push(self.codegen_expression(el)?);
        }

        let element_type = self.binding_type(&bindings[0])?;
        for b in &bindings[1..] {
            let ty = self.binding_type(b)?;
            if ty != element_type {
                return Err(format!(
                    "array literal elements have mismatched types: expected {:?}, found {:?}",
                    element_type, ty
                ));
            }
        }

        let llvm_elem_ty = self.llvm_type(&element_type)?;
        let count = elements.len() as u64;

        let llvm_arr_ty = llvm_elem_ty.array_type(count as u32);
        let backing_alloca = self
            .builder
            .build_alloca(llvm_arr_ty, "arr_literal_backing")
            .map_err(|e| e.to_string())?;

        let zero = self.ptr_sized_int_type().const_zero();

        for (i, b) in bindings.into_iter().enumerate() {
            let val = self.binding_basic_value(&b)?;
            let idx = self.ptr_sized_int_type().const_int(i as u64, false);

            let slot_ptr = unsafe {
                self.builder
                    .build_gep(
                        llvm_arr_ty,
                        backing_alloca,
                        &[zero, idx],
                        &format!("slot_{}", i),
                    )
                    .map_err(|e| e.to_string())?
            };
            self.builder
                .build_store(slot_ptr, val)
                .map_err(|e| e.to_string())?;
        }

        let first_elem_ptr = unsafe {
            self.builder
                .build_gep(
                    llvm_arr_ty,
                    backing_alloca,
                    &[zero, zero],
                    "slice_first_elem",
                )
                .map_err(|e| e.to_string())?
        };

        let slice_llvm_ty = self.slice_type();
        let len_val = self.ptr_sized_int_type().const_int(count, false);

        let mut slice_val = slice_llvm_ty.get_undef();
        slice_val = self
            .builder
            .build_insert_value(slice_val, first_elem_ptr, 0, "slice_ptr")
            .map_err(|e| e.to_string())?
            .into_struct_value();
        slice_val = self
            .builder
            .build_insert_value(slice_val, len_val, 1, "slice_len")
            .map_err(|e| e.to_string())?
            .into_struct_value();

        Ok(Binding::Slice {
            ty: Type::Slice {
                element: Box::new(element_type),
            },
            value: slice_val,
        })
    }

    fn codegen_unary(
        &mut self,
        operator: UnaryOperator,
        operand: &Binding<'ctx>,
    ) -> Result<Binding<'ctx>, String> {
        if operator == UnaryOperator::Not {
            let value = match operand {
                Binding::Boolean { value } => *value,
                _ => return Err("expected boolean value for 'not'".to_string()),
            };

            let zero = self.context.bool_type().const_zero();
            let inverted = self
                .builder
                .build_int_compare(IntPredicate::EQ, value, zero, "nottmp")
                .map_err(|error| error.to_string())?;

            return Ok(Binding::Boolean { value: inverted });
        }

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
            UnaryOperator::Not => unreachable!(),
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
        self.codegen_cast_inner(binding, target_type, &mut HashSet::new())
    }

    fn codegen_cast_inner(
        &mut self,
        binding: Binding<'ctx>,
        target_type: Type,
        visited: &mut HashSet<(String, String)>,
    ) -> Result<Binding<'ctx>, String> {
        let source_type = self.binding_type(&binding)?;

        if source_type == target_type {
            return Ok(binding);
        }

        let resolved_source = self.resolve_alias_root(&source_type)?.clone();
        let resolved_target = self.resolve_alias_root(&target_type)?.clone();

        if resolved_source == resolved_target {
            let val = self.binding_basic_value(&binding)?;
            return self.binding_from_basic_value(&target_type, val);
        }

        let pair = (
            Checker::canonical_type_key(&resolved_source),
            Checker::canonical_type_key(&resolved_target),
        );
        if visited.contains(&pair) {
            if matches!(resolved_target, Type::Slice { .. }) {
                let val = self.binding_basic_value(&binding)?;
                return Ok(Binding::Slice {
                    ty: target_type,
                    value: val.into_struct_value(),
                });
            }
            if matches!(resolved_target, Type::Pointer { .. }) {
                let val = self.binding_basic_value(&binding)?;
                return Ok(Binding::Pointer {
                    ty: target_type,
                    value: val.into_pointer_value(),
                });
            }
        }

        visited.insert(pair.clone());
        let result = self.do_codegen_cast(
            binding,
            target_type,
            source_type,
            resolved_source,
            resolved_target,
            visited,
        );
        visited.remove(&pair);
        result
    }

    fn do_codegen_cast(
        &mut self,
        binding: Binding<'ctx>,
        target_type: Type,
        source_type: Type,
        resolved_source: Type,
        resolved_target: Type,
        visited: &mut HashSet<(String, String)>,
    ) -> Result<Binding<'ctx>, String> {
        if !Checker::is_supported_cast_with_aliases(&source_type, &target_type, &self.type_aliases) {
            return Err(format!(
                "unsupported cast from {:?} to {:?}",
                source_type, target_type
            ));
        }

        if let (Type::Struct(source_fields), Type::Struct(target_fields)) =
            (&resolved_source, &resolved_target)
        {
            let source_value = self.binding_basic_value(&binding)?.into_struct_value();
            let target_llvm_type = self.llvm_type(&target_type)?.into_struct_type();
            let mut target_value = target_llvm_type.get_undef();

            for (index, (source_field, target_field)) in
                source_fields.iter().zip(target_fields).enumerate()
            {
                let field_value = self
                    .builder
                    .build_extract_value(source_value, index as u32, &source_field.name)
                    .map_err(|error| error.to_string())?;
                let field_binding = self.binding_from_basic_value(&source_field.ty, field_value)?;
                let converted =
                    self.codegen_cast_inner(field_binding, target_field.ty.clone(), visited)?;
                target_value = self
                    .builder
                    .build_insert_value(
                        target_value,
                        self.binding_basic_value(&converted)?,
                        index as u32,
                        &target_field.name,
                    )
                    .map_err(|error| error.to_string())?
                    .into_struct_value();
            }

            return Ok(Binding::Struct {
                ty: target_type,
                value: target_value,
                storage: None,
            });
        }

        if let (
            Type::Array {
                element: source_element,
                size,
            },
            Type::Array {
                element: target_element,
                ..
            },
        ) = (&resolved_source, &resolved_target)
        {
            let source_value = self.binding_basic_value(&binding)?.into_array_value();
            let target_llvm_type = self.llvm_type(&target_type)?.into_array_type();
            let mut target_value = target_llvm_type.get_undef();

            for index in 0..*size {
                let element_value = self
                    .builder
                    .build_extract_value(source_value, index as u32, "cast_array_element")
                    .map_err(|error| error.to_string())?;
                let element_binding =
                    self.binding_from_basic_value(source_element, element_value)?;
                let converted =
                    self.codegen_cast_inner(element_binding, (**target_element).clone(), visited)?;
                target_value = self
                    .builder
                    .build_insert_value(
                        target_value,
                        self.binding_basic_value(&converted)?,
                        index as u32,
                        "cast_array_element",
                    )
                    .map_err(|error| error.to_string())?
                    .into_array_value();
            }

            return Ok(Binding::Array {
                ty: target_type,
                value: target_value,
                storage: None,
            });
        }

        if let (
            Type::Slice {
                element: source_element,
            },
            Type::Slice {
                element: target_element,
            },
        ) = (&resolved_source, &resolved_target)
        {
            let source_value = self.binding_basic_value(&binding)?.into_struct_value();
            let source_ptr = self
                .builder
                .build_extract_value(source_value, 0, "cast_slice_ptr")
                .map_err(|error| error.to_string())?
                .into_pointer_value();
            let length = self
                .builder
                .build_extract_value(source_value, 1, "cast_slice_len")
                .map_err(|error| error.to_string())?
                .into_int_value();
            let target_element_type = self.llvm_type(target_element)?;
            let target_ptr = self
                .builder
                .build_array_alloca(target_element_type, length, "cast_slice_data")
                .map_err(|error| error.to_string())?;

            let current_block = self
                .builder
                .get_insert_block()
                .ok_or_else(|| "cannot cast slice outside basic block".to_string())?;
            let function = current_block
                .get_parent()
                .ok_or_else(|| "slice cast has no parent function".to_string())?;
            let condition_block = self.context.append_basic_block(function, "slice.cast.cond");
            let body_block = self.context.append_basic_block(function, "slice.cast.body");
            let end_block = self.context.append_basic_block(function, "slice.cast.end");
            self.builder
                .build_unconditional_branch(condition_block)
                .map_err(|error| error.to_string())?;

            self.builder.position_at_end(condition_block);
            let index = self
                .builder
                .build_phi(self.ptr_sized_int_type(), "slice.cast.index")
                .map_err(|error| error.to_string())?;
            index.add_incoming(&[(&self.ptr_sized_int_type().const_zero(), current_block)]);
            let index_value = index.as_basic_value().into_int_value();
            let has_next = self
                .builder
                .build_int_compare(IntPredicate::ULT, index_value, length, "slice.cast.more")
                .map_err(|error| error.to_string())?;
            self.builder
                .build_conditional_branch(has_next, body_block, end_block)
                .map_err(|error| error.to_string())?;

            self.builder.position_at_end(body_block);
            let source_element_type = self.llvm_type(source_element)?;
            let source_element_ptr = unsafe {
                self.builder
                    .build_gep(
                        source_element_type,
                        source_ptr,
                        &[index_value],
                        "slice.cast.src",
                    )
                    .map_err(|error| error.to_string())?
            };
            let source_element_value = self
                .builder
                .build_load(source_element_type, source_element_ptr, "slice.cast.value")
                .map_err(|error| error.to_string())?;
            let source_element_binding =
                self.binding_from_basic_value(source_element, source_element_value)?;
            let converted =
                self.codegen_cast_inner(source_element_binding, (**target_element).clone(), visited)?;
            let target_element_ptr = unsafe {
                self.builder
                    .build_gep(
                        target_element_type,
                        target_ptr,
                        &[index_value],
                        "slice.cast.dst",
                    )
                    .map_err(|error| error.to_string())?
            };
            self.builder
                .build_store(target_element_ptr, self.binding_basic_value(&converted)?)
                .map_err(|error| error.to_string())?;
            let body_end = self.builder.get_insert_block().unwrap();
            let next_index = self
                .builder
                .build_int_add(
                    index_value,
                    self.ptr_sized_int_type().const_int(1, false),
                    "slice.cast.next",
                )
                .map_err(|error| error.to_string())?;
            self.builder
                .build_unconditional_branch(condition_block)
                .map_err(|error| error.to_string())?;
            index.add_incoming(&[(&next_index, body_end)]);

            self.builder.position_at_end(end_block);
            let target_slice_type = self.llvm_type(&target_type)?.into_struct_type();
            let slice_value = target_slice_type.get_undef();
            let slice_value = self
                .builder
                .build_insert_value(slice_value, target_ptr, 0, "cast_slice_ptr")
                .map_err(|error| error.to_string())?
                .into_struct_value();
            let slice_value = self
                .builder
                .build_insert_value(slice_value, length, 1, "cast_slice_len")
                .map_err(|error| error.to_string())?
                .into_struct_value();

            return Ok(Binding::Slice {
                ty: target_type,
                value: slice_value,
            });
        }

        if let (Type::Pointer { .. }, Type::Pointer { .. }) = (&resolved_source, &resolved_target) {
            if let Binding::Pointer { value, .. } = binding {
                return Ok(Binding::Pointer {
                    ty: target_type,
                    value,
                });
            }
        }

        // 1. Variant or Sub-Union -> Union
        if let Type::Union(target_vars) = &resolved_target {
            let union_llvm_ty = self.llvm_type(&target_type)?.into_struct_type();
            let alloca = self
                .builder
                .build_alloca(union_llvm_ty, "union_box")
                .map_err(|e| e.to_string())?;

            self.builder
                .build_store(alloca, union_llvm_ty.const_zero())
                .map_err(|e| e.to_string())?;

            let tag_ptr = self
                .builder
                .build_struct_gep(union_llvm_ty, alloca, 0, "tag_ptr")
                .map_err(|e| e.to_string())?;

            let payload_ptr = self
                .builder
                .build_struct_gep(union_llvm_ty, alloca, 1, "payload_ptr")
                .map_err(|e| e.to_string())?;

            if let Type::Union(source_vars) = &resolved_source {
                let sub_union_llvm_ty = self.llvm_type(&source_type)?.into_struct_type();
                let sub_storage = if let Binding::Union {
                    storage: Some(storage),
                    ..
                } = &binding
                {
                    *storage
                } else {
                    let val = self.binding_basic_value(&binding)?;
                    let tmp = self
                        .builder
                        .build_alloca(sub_union_llvm_ty, "sub_tmp")
                        .map_err(|e| e.to_string())?;
                    self.builder
                        .build_store(tmp, val)
                        .map_err(|e| e.to_string())?;
                    tmp
                };

                let src_tag_ptr = self
                    .builder
                    .build_struct_gep(sub_union_llvm_ty, sub_storage, 0, "src_tag_ptr")
                    .map_err(|e| e.to_string())?;
                let src_tag_val = self
                    .builder
                    .build_load(self.context.i8_type(), src_tag_ptr, "src_tag")
                    .map_err(|e| e.to_string())?
                    .into_int_value();

                let current_bb = self.builder.get_insert_block().unwrap();
                let current_fn = current_bb.get_parent().unwrap();
                let src_payload_ptr = self
                    .builder
                    .build_struct_gep(sub_union_llvm_ty, sub_storage, 1, "src_payload_ptr")
                    .map_err(|e| e.to_string())?;
                let invalid_tag = self
                    .context
                    .append_basic_block(current_fn, "union.cast.invalid");
                let merge_block = self
                    .context
                    .append_basic_block(current_fn, "union.cast.merge");
                let mut cases = Vec::with_capacity(source_vars.len());
                for source_index in 0..source_vars.len() {
                    let case_block = self
                        .context
                        .append_basic_block(current_fn, "union.cast.case");
                    cases.push((
                        self.context.i8_type().const_int(source_index as u64, false),
                        case_block,
                    ));
                }
                self.builder
                    .build_switch(src_tag_val, invalid_tag, &cases)
                    .map_err(|e| e.to_string())?;

                for (source_index, source_variant) in source_vars.iter().enumerate() {
                    let case_block = cases[source_index].1;
                    self.builder.position_at_end(case_block);
                    let target_index = Checker::castable_union_variant_with_aliases(
                        source_variant,
                        target_vars,
                        &self.type_aliases,
                    )
                    .ok_or_else(|| {
                        format!(
                            "cannot uniquely promote union variant {:?} into {:?}",
                            source_variant, target_type
                        )
                    })?;
                    let source_variant_type = self.llvm_type(source_variant)?;
                    let source_payload = self
                        .builder
                        .build_load(source_variant_type, src_payload_ptr, "source_variant")
                        .map_err(|e| e.to_string())?;
                    let source_binding =
                        self.binding_from_basic_value(source_variant, source_payload)?;
                    let converted = self.codegen_cast_inner(
                        source_binding,
                        target_vars[target_index].clone(),
                        visited,
                    )?;
                    let target_tag = self.context.i8_type().const_int(target_index as u64, false);
                    self.builder
                        .build_store(tag_ptr, target_tag)
                        .map_err(|e| e.to_string())?;
                    self.builder
                        .build_store(payload_ptr, self.binding_basic_value(&converted)?)
                        .map_err(|e| e.to_string())?;
                    self.builder
                        .build_unconditional_branch(merge_block)
                        .map_err(|e| e.to_string())?;
                }

                self.builder.position_at_end(invalid_tag);
                self.builder
                    .build_unreachable()
                    .map_err(|e| e.to_string())?;
                self.builder.position_at_end(merge_block);
            } else {
                let target_index = Checker::castable_union_variant_with_aliases(
                    &source_type,
                    target_vars,
                    &self.type_aliases,
                )
                .ok_or_else(|| {
                    format!(
                        "cannot uniquely promote {:?} into union {:?}",
                        source_type, target_type
                    )
                })?;
                let converted =
                    self.codegen_cast_inner(binding, target_vars[target_index].clone(), visited)?;
                let tag_val = self.context.i8_type().const_int(target_index as u64, false);
                self.builder
                    .build_store(tag_ptr, tag_val)
                    .map_err(|e| e.to_string())?;

                let val = self.binding_basic_value(&converted)?;
                self.builder
                    .build_store(payload_ptr, val)
                    .map_err(|e| e.to_string())?;
            }

            let loaded = self
                .builder
                .build_load(union_llvm_ty, alloca, "boxed_union")
                .map_err(|e| e.to_string())?
                .into_struct_value();

            return Ok(Binding::Union {
                ty: target_type,
                value: loaded,
                storage: Some(alloca),
            });
        }

        // 2. Union -> Variant
        if let Type::Union(variants) = &resolved_source {
            let candidates: Vec<usize> = variants
                .iter()
                .enumerate()
                .filter(|(_, variant)| {
                    Checker::is_supported_cast_with_aliases(
                        variant,
                        &target_type,
                        &self.type_aliases,
                    )
                })
                .map(|(index, _)| index)
                .collect();
            if candidates.is_empty() {
                return Err(format!(
                    "no variant of {:?} can be cast to {:?}",
                    source_type, target_type
                ));
            }

            let union_llvm_ty = self.llvm_type(&resolved_source)?.into_struct_type();
            let storage = if let Binding::Union {
                storage: Some(storage),
                ..
            } = &binding
            {
                *storage
            } else {
                let val = self.binding_basic_value(&binding)?;
                let tmp = self
                    .builder
                    .build_alloca(union_llvm_ty, "unbox_tmp")
                    .map_err(|e| e.to_string())?;
                self.builder
                    .build_store(tmp, val)
                    .map_err(|e| e.to_string())?;
                tmp
            };

            let tag_ptr = self
                .builder
                .build_struct_gep(union_llvm_ty, storage, 0, "tag_ptr")
                .map_err(|e| e.to_string())?;
            let tag_val = self
                .builder
                .build_load(self.context.i8_type(), tag_ptr, "tag")
                .map_err(|e| e.to_string())?
                .into_int_value();
            let current_bb = self.builder.get_insert_block().unwrap();
            let current_fn = current_bb.get_parent().unwrap();
            let merge_bb = self.context.append_basic_block(current_fn, "unbox.merge");
            let fail_bb = self.context.append_basic_block(current_fn, "unbox.fail");
            let mut cases = Vec::with_capacity(candidates.len());
            for index in &candidates {
                cases.push((
                    self.context.i8_type().const_int(*index as u64, false),
                    self.context.append_basic_block(current_fn, "unbox.case"),
                ));
            }
            self.builder
                .build_switch(tag_val, fail_bb, &cases)
                .map_err(|e| e.to_string())?;

            self.builder.position_at_end(fail_bb);
            let trap_fn = if let Some(f) = self.module.get_function("llvm.trap") {
                f
            } else {
                let fn_ty = self.context.void_type().fn_type(&[], false);
                self.module.add_function("llvm.trap", fn_ty, None)
            };
            self.builder
                .build_call(trap_fn, &[], "")
                .map_err(|e| e.to_string())?;
            self.builder
                .build_unreachable()
                .map_err(|e| e.to_string())?;

            let payload_ptr = self
                .builder
                .build_struct_gep(union_llvm_ty, storage, 1, "payload_ptr")
                .map_err(|e| e.to_string())?;

            let mut incoming =
                Vec::<(BasicValueEnum<'ctx>, inkwell::basic_block::BasicBlock<'ctx>)>::new();
            for (index, (_, case_bb)) in candidates.iter().zip(cases.iter()) {
                let source_variant = &variants[*index];
                self.builder.position_at_end(*case_bb);
                let source_value = self
                    .builder
                    .build_load(
                        self.llvm_type(source_variant)?,
                        payload_ptr,
                        "unboxed_value",
                    )
                    .map_err(|e| e.to_string())?;
                let source_binding = self.binding_from_basic_value(source_variant, source_value)?;
                let converted =
                    self.codegen_cast_inner(source_binding, target_type.clone(), visited)?;
                let converted_value = self.binding_basic_value(&converted)?;
                let converted_end = self.builder.get_insert_block().unwrap();
                self.builder
                    .build_unconditional_branch(merge_bb)
                    .map_err(|e| e.to_string())?;
                incoming.push((converted_value, converted_end));
            }

            self.builder.position_at_end(merge_bb);
            let result_phi = self
                .builder
                .build_phi(self.llvm_type(&target_type)?, "unboxed_result")
                .map_err(|e| e.to_string())?;
            for (value, block) in &incoming {
                result_phi.add_incoming(&[(value, *block)]);
            }
            return self.binding_from_basic_value(&target_type, result_phi.as_basic_value());
        }

        if Checker::is_integer_type(&resolved_source)
            && matches!(resolved_target, Type::Pointer { .. } | Type::Function { .. })
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

        if matches!(resolved_source, Type::Pointer { .. } | Type::Function { .. })
            && Checker::is_integer_type(&resolved_target)
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

        if resolved_target == Type::Bool {
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
            if resolved_source != Type::Bool && self.is_signed(&resolved_source) {
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
        let (function_ptr, ret_ty, fn_type, expected_param_types) = match target {
            Expression::Identifier(name) => {
                if let Some(func) = self.module.get_function(name) {
                    let (return_type, fn_type, param_types) =
                        if let Some(sig) = self.extern_functions.get(name) {
                            let fn_ty = self.signature_to_function_type(Some(sig))?;
                            let pts = sig.parameters.iter().map(|p| p.ty.clone()).collect();
                            ((*sig.return_type).clone(), fn_ty, pts)
                        } else if let Some(sig) = self.function_signatures.get(name) {
                            let fn_ty = self.signature_to_function_type(Some(sig))?;
                            let pts = sig.parameters.iter().map(|p| p.ty.clone()).collect();
                            ((*sig.return_type).clone(), fn_ty, pts)
                        } else {
                            let ret = match func.get_type().get_return_type() {
                                Some(_) => Type::S32,
                                None => Type::Void,
                            };
                            (ret, func.get_type(), Vec::new())
                        };
                    Ok((
                        func.as_global_value().as_pointer_value(),
                        return_type,
                        fn_type,
                        param_types,
                    ))
                } else if let Some(local) = self.lookup_local(name) {
                    let loaded = self
                        .builder
                        .build_load(self.llvm_type(&local.ty)?, local.alloca, "fn_ptr")
                        .map_err(|e| e.to_string())?
                        .into_pointer_value();

                    let (ret_ty, fn_type) = self.function_type_info(&local.ty)?;
                    let pts = match &local.ty {
                        Type::Function { parameters, .. } => parameters.clone(),
                        _ => Vec::new(),
                    };
                    Ok((loaded, ret_ty, fn_type, pts))
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
                    let pts = match &global_var.ty {
                        Type::Function { parameters, .. } => parameters.clone(),
                        _ => Vec::new(),
                    };
                    Ok((loaded, ret_ty, fn_type, pts))
                } else {
                    Err(format!("unknown function or callable variable '{}'", name))
                }
            }
            expr => {
                let target_binding = self.codegen_expression(expr)?;
                match target_binding {
                    Binding::Function { signature, value } => {
                        let fn_type = self.signature_to_function_type(Some(&signature))?;
                        let pts = signature.parameters.iter().map(|p| p.ty.clone()).collect();
                        Ok((
                            value.as_global_value().as_pointer_value(),
                            *signature.return_type,
                            fn_type,
                            pts,
                        ))
                    }
                    Binding::Pointer { value, ty } => {
                        let (ret_ty, fn_type) = self.function_type_info(&ty)?;
                        let pts = match &ty {
                            Type::Function { parameters, .. } => parameters.clone(),
                            _ => Vec::new(),
                        };
                        Ok((value, ret_ty, fn_type, pts))
                    }
                    _ => Err("called target is not callable".to_string()),
                }
            }
        }?;

        let mut argument_values = Vec::with_capacity(arguments.len());
        for (i, argument) in arguments.iter().enumerate() {
            let binding = self.codegen_expression(argument)?;
            let coerced = if let Some(expected_ty) = expected_param_types.get(i) {
                self.coerce_to_type(binding, expected_ty)?
            } else {
                binding
            };
            argument_values.push(self.binding_basic_value(&coerced)?.into());
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
            Type::Alias(_) => self.function_type_info(self.resolve_alias_root(ty)?),
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
            .build_int_z_extend_or_bit_cast(index_value, self.ptr_sized_int_type(), "idxptr")
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
                            &[self.ptr_sized_int_type().const_zero(), index_i64],
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
            Type::Pointer { element } => {
                let pointer = match binding {
                    Binding::Pointer { value, .. } => value,
                    _ => return Err("expected pointer binding".to_string()),
                };
                let element_llvm_type = self.llvm_type(&element)?;
                let element_pointer = unsafe {
                    self.builder
                        .build_gep(element_llvm_type, pointer, &[index_i64], "ptr_idx")
                        .map_err(|error| error.to_string())?
                };
                let loaded = self
                    .builder
                    .build_load(element_llvm_type, element_pointer, "ptr_val")
                    .map_err(|error| error.to_string())?;
                self.binding_from_basic_value(&element, loaded)
            }
            other => Err(format!("cannot index value of type {:?}", other)),
        }
    }

    fn codegen_address_of(&mut self, expression: &Expression) -> Result<Binding<'ctx>, String> {
        match expression {
            Expression::Dereference(pointer_expression) => {
                let pointer = self.codegen_expression(pointer_expression)?;
                if matches!(self.binding_type(&pointer)?, Type::Pointer { .. }) {
                    Ok(pointer)
                } else {
                    Err("address-of dereference requires a pointer".to_string())
                }
            }
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
                    .build_int_z_extend_or_bit_cast(
                        index_value,
                        self.ptr_sized_int_type(),
                        "idxptr",
                    )
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
                                    &[self.ptr_sized_int_type().const_zero(), index_i64],
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
                    Type::Pointer { element } => {
                        let pointer = match binding {
                            Binding::Pointer { value, .. } => value,
                            _ => return Err("expected pointer binding".to_string()),
                        };
                        let element_llvm_type = self.llvm_type(&element)?;
                        let element_pointer = unsafe {
                            self.builder
                                .build_gep(
                                    element_llvm_type,
                                    pointer,
                                    &[index_i64],
                                    "ptr_index_addr",
                                )
                                .map_err(|error| error.to_string())?
                        };
                        Ok(Binding::Pointer {
                            ty: Type::Pointer { element },
                            value: element_pointer,
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
        let len_type = self.ptr_sized_int_type();
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
        let len = self
            .ptr_sized_int_type()
            .const_int(value.len() as u64, false);

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
            Type::Alias(_) => {
                self.binding_from_basic_value(self.resolve_alias_root(ty)?, value)
            }
            Type::Atom(name) => Ok(Binding::Atom {
                name: name.clone(),
                value: value.into_int_value(),
            }),
            Type::Bool => Ok(Binding::Boolean {
                value: value.into_int_value(),
            }),
            Type::S16
            | Type::S32
            | Type::S64
            | Type::SSize
            | Type::U8
            | Type::U16
            | Type::U32
            | Type::U64
            | Type::USize => Ok(Binding::Integer {
                ty: ty.clone(),
                value: value.into_int_value(),
            }),
            Type::Struct { .. } => Ok(Binding::Struct {
                ty: ty.clone(),
                value: value.into_struct_value(),
                storage: None,
            }),
            Type::Union { .. } => Ok(Binding::Union {
                ty: ty.clone(),
                value: value.into_struct_value(),
                storage: None,
            }),
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
            Binding::Struct { ty, .. } => Ok(ty.clone()),
            Binding::Union { ty, .. } => Ok(ty.clone()),
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
            Binding::Struct { value, .. } => Ok((*value).into()),
            Binding::Union { value, .. } => Ok((*value).into()),
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

    fn union_payload_size(&self, variants: &[Type]) -> Result<u64, String> {
        let mut max_size = 0;
        let mut max_alignment = 1;
        for v in variants {
            let size = self.llvm_type_size(v)?;
            if size > max_size {
                max_size = size;
            }
            max_alignment = max_alignment.max(self.llvm_type_align(v)?);
        }
        let padding = (max_alignment - max_size % max_alignment) % max_alignment;
        Ok(max_size + padding)
    }

    fn union_payload_storage_type(&self, variants: &[Type]) -> Result<BasicTypeEnum<'ctx>, String> {
        let mut storage_element = None;
        let mut storage_element_size = 0;
        let mut max_alignment = 0;

        for variant in variants {
            let llvm_type = self.llvm_type(variant)?;
            let alignment = self.target_data.get_abi_alignment(&llvm_type) as u64;
            let size = self.target_data.get_abi_size(&llvm_type);
            if alignment > max_alignment
                || (alignment == max_alignment && size > storage_element_size)
            {
                storage_element = Some(llvm_type);
                storage_element_size = size;
                max_alignment = alignment;
            }
        }

        let Some(storage_element) = storage_element else {
            return Ok(self.context.i8_type().array_type(0).into());
        };

        let payload_size = self.union_payload_size(variants)?;
        let element_count = payload_size.div_ceil(storage_element_size.max(1)).max(1);
        Ok(storage_element.array_type(element_count as u32).into())
    }

    fn llvm_type_align(&self, ty: &Type) -> Result<u64, String> {
        let llvm_type = self.llvm_type(ty)?;
        Ok(self.target_data.get_abi_alignment(&llvm_type) as u64)
    }

    fn llvm_type_size(&self, ty: &Type) -> Result<u64, String> {
        match ty {
            Type::USize | Type::SSize | Type::Pointer { .. } | Type::Function { .. } => {
                Ok(self.ptr_byte_size())
            }
            Type::Slice { .. } => Ok(2 * self.ptr_byte_size()),
            _ => {
                let llvm_type = self.llvm_type(ty)?;
                Ok(self.target_data.get_abi_size(&llvm_type))
            }
        }
    }

    fn llvm_type(&self, ty: &Type) -> Result<BasicTypeEnum<'ctx>, String> {
        match ty {
            Type::Void => Err("void type is not a basic type in LLVM".to_string()),
            Type::Alias(_) => self.llvm_type(self.resolve_alias_root(ty)?),
            Type::Atom(_) => Ok(self.context.i32_type().into()),
            Type::S16 | Type::U16 => Ok(self.context.i16_type().into()),
            Type::S32 | Type::U32 => Ok(self.context.i32_type().into()),
            Type::S64 | Type::U64 => Ok(self.context.i64_type().into()),
            Type::USize | Type::SSize => Ok(self.ptr_sized_int_type().into()),
            Type::U8 => Ok(self.context.i8_type().into()),
            Type::Bool => Ok(self.context.bool_type().into()),
            Type::Struct(fields) => {
                let field_llvm_types: Vec<BasicTypeEnum<'ctx>> = fields
                    .iter()
                    .map(|f| self.llvm_type(&f.ty))
                    .collect::<Result<Vec<_>, String>>()?;
                Ok(self.context.struct_type(&field_llvm_types, false).into())
            }
            Type::Union(variants) => {
                let tag_ty = self.context.i8_type();
                let payload_ty = self.union_payload_storage_type(variants)?;
                Ok(self
                    .context
                    .struct_type(&[tag_ty.into(), payload_ty.into()], false)
                    .into())
            }
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
            Type::Alias(_) => self.zero_value(self.resolve_alias_root(ty)?),
            Type::Atom(_) => Ok(self.context.i32_type().const_zero().into()),
            Type::S16 | Type::U16 => Ok(self.context.i16_type().const_zero().into()),
            Type::S32 | Type::U32 => Ok(self.context.i32_type().const_zero().into()),
            Type::S64 | Type::U64 => Ok(self.context.i64_type().const_zero().into()),
            Type::USize | Type::SSize => Ok(self.ptr_sized_int_type().const_zero().into()),
            Type::U8 => Ok(self.context.i8_type().const_zero().into()),
            Type::Bool => Ok(self.context.bool_type().const_zero().into()),
            Type::Struct(fields) => {
                let struct_llvm_type = self.llvm_type(ty)?.into_struct_type();
                let field_zeroes = fields
                    .iter()
                    .map(|f| self.zero_value(&f.ty))
                    .collect::<Result<Vec<_>, String>>()?;
                Ok(struct_llvm_type.const_named_struct(&field_zeroes).into())
            }
            Type::Union(_) => {
                let union_llvm_type = self.llvm_type(ty)?.into_struct_type();
                Ok(union_llvm_type.const_zero().into())
            }
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
        matches!(ty, Type::S16 | Type::S32 | Type::S64 | Type::SSize)
    }
}

#[cfg(test)]
mod cast_codegen_tests {
    use super::{Binding, Codegen, LocalVar};
    use crate::sema::Checker;
    use crate::syntax::{
        Definition, Expression, Program, Statement, StructFieldInit, StructFieldType, Type,
    };
    use inkwell::AddressSpace;
    use inkwell::OptimizationLevel;
    use inkwell::context::Context;
    use inkwell::targets::{
        CodeModel, InitializationConfig, RelocMode, Target, TargetMachine, TargetTriple,
    };
    use inkwell::types::BasicType;
    use std::collections::HashMap;

    fn test_target_machine() -> TargetMachine {
        Target::initialize_all(&InitializationConfig::default());
        let triple = TargetTriple::create("x86_64-unknown-linux-musl");
        Target::from_triple(&triple)
            .unwrap()
            .create_target_machine(
                &triple,
                "generic",
                "",
                OptimizationLevel::Default,
                RelocMode::Static,
                CodeModel::Default,
            )
            .unwrap()
    }

    #[test]
    fn pointer_sized_types_and_union_alignment_follow_target_data() {
        let context = Context::create();
        let target_machine = test_target_machine();
        let codegen = Codegen::new(&context, "target_layout", &target_machine);
        let pointer_bytes = codegen.ptr_byte_size();
        let usize_type = codegen.llvm_type(&Type::USize).unwrap().into_int_type();
        let ssize_type = codegen.llvm_type(&Type::SSize).unwrap().into_int_type();
        let expected_bits = (pointer_bytes * 8) as u32;

        assert_eq!(usize_type.get_bit_width(), expected_bits);
        assert_eq!(ssize_type.get_bit_width(), expected_bits);

        let union_type = Type::Union(vec![Type::U8, Type::S64]);
        let union_alignment = codegen.llvm_type_align(&union_type).unwrap();
        let wide_integer_alignment = codegen.llvm_type_align(&Type::S64).unwrap();
        assert!(union_alignment >= wide_integer_alignment);
    }

    #[test]
    fn record_literal_generates_valid_llvm() {
        let context = Context::create();
        let target_machine = test_target_machine();
        let mut codegen = Codegen::new(&context, "record_literal", &target_machine);
        codegen.type_aliases = Checker::standard_type_aliases()
            .into_iter()
            .map(|alias| (alias.name, alias.target))
            .collect();
        let record_type = Checker::record_type();
        let function_type = codegen.llvm_type(&record_type).unwrap().fn_type(&[], false);
        let function = codegen.module.add_function("run", function_type, None);
        let entry = context.append_basic_block(function, "entry");
        codegen.builder.position_at_end(entry);

        let result = codegen
            .codegen_record_literal(&[
                StructFieldInit {
                    name: "answer".to_string(),
                    value: Expression::Integer(42),
                },
                StructFieldInit {
                    name: "label".to_string(),
                    value: Expression::String(b"ready".to_vec()),
                },
            ])
            .unwrap();
        let result_value = codegen.binding_basic_value(&result).unwrap();
        codegen.builder.build_return(Some(&result_value)).unwrap();

        assert!(codegen.module.verify().is_ok());
    }

    #[test]
    fn main_can_reference_later_global_record() {
        let context = Context::create();
        let target_machine = test_target_machine();
        let codegen = Codegen::new(&context, "forward_global_record", &target_machine);
        let program = Program {
            variables: Vec::new(),
            extern_functions: Vec::new(),
            definitions: vec![
                Definition {
                    name: "main".to_string(),
                    value: Expression::Block {
                        signature: None,
                        body: vec![Statement::Discard(Expression::Identifier(
                            "my_data".to_string(),
                        ))],
                    },
                },
                Definition {
                    name: "my_data".to_string(),
                    value: Expression::Record(vec![StructFieldInit {
                        name: "answer".to_string(),
                        value: Expression::Integer(42),
                    }]),
                },
            ],
            type_aliases: Checker::standard_type_aliases(),
        };

        let module = codegen.generate(&program).unwrap();
        assert!(module.verify().is_ok());
    }

    #[test]
    fn pointer_index_load_and_address_generate_valid_llvm() {
        let context = Context::create();
        let target_machine = test_target_machine();
        let mut codegen = Codegen::new(&context, "pointer_index", &target_machine);
        let pointer_type = Type::Pointer {
            element: Box::new(Type::S32),
        };
        let llvm_pointer_type = codegen.llvm_type(&pointer_type).unwrap();

        let load_function_type = codegen
            .context
            .i32_type()
            .fn_type(&[llvm_pointer_type.into()], false);
        let load_function = codegen
            .module
            .add_function("load_index", load_function_type, None);
        let load_entry = context.append_basic_block(load_function, "entry");
        codegen.builder.position_at_end(load_entry);
        let load_param = load_function.get_nth_param(0).unwrap();
        load_param.set_name("p");
        let pointer_storage = codegen
            .builder
            .build_alloca(llvm_pointer_type, "p.addr")
            .unwrap();
        codegen
            .builder
            .build_store(pointer_storage, load_param)
            .unwrap();
        codegen.local_scopes.push(HashMap::from([(
            "p".to_string(),
            LocalVar {
                ty: pointer_type.clone(),
                alloca: pointer_storage,
            },
        )]));
        let loaded = codegen
            .codegen_index(
                &Expression::Identifier("p".to_string()),
                &Expression::Integer(0),
            )
            .unwrap();
        let loaded_value = codegen.binding_basic_value(&loaded).unwrap();
        codegen.builder.build_return(Some(&loaded_value)).unwrap();
        codegen.local_scopes.pop();

        let address_function_type = llvm_pointer_type.fn_type(&[llvm_pointer_type.into()], false);
        let address_function =
            codegen
                .module
                .add_function("address_index", address_function_type, None);
        let address_entry = context.append_basic_block(address_function, "entry");
        codegen.builder.position_at_end(address_entry);
        let address_param = address_function.get_nth_param(0).unwrap();
        address_param.set_name("p");
        let pointer_storage = codegen
            .builder
            .build_alloca(llvm_pointer_type, "p.addr")
            .unwrap();
        codegen
            .builder
            .build_store(pointer_storage, address_param)
            .unwrap();
        codegen.local_scopes.push(HashMap::from([(
            "p".to_string(),
            LocalVar {
                ty: pointer_type,
                alloca: pointer_storage,
            },
        )]));
        let address = codegen
            .codegen_address_of(&Expression::Index {
                expression: Box::new(Expression::Identifier("p".to_string())),
                index: Box::new(Expression::Integer(0)),
            })
            .unwrap();
        let address_value = codegen.binding_basic_value(&address).unwrap();
        codegen.builder.build_return(Some(&address_value)).unwrap();

        assert!(codegen.module.verify().is_ok());
    }

    #[test]
    fn pointer_dereference_and_address_generate_valid_llvm() {
        let context = Context::create();
        let target_machine = test_target_machine();
        let mut codegen = Codegen::new(&context, "pointer_dereference", &target_machine);
        let pointer_type = Type::Pointer {
            element: Box::new(Type::S32),
        };
        let llvm_pointer_type = codegen.llvm_type(&pointer_type).unwrap();
        let function_type = context
            .i32_type()
            .fn_type(&[llvm_pointer_type.into()], false);
        let function = codegen.module.add_function("load", function_type, None);
        let entry = context.append_basic_block(function, "entry");
        codegen.builder.position_at_end(entry);
        let parameter = function.get_nth_param(0).unwrap();
        let pointer_storage = codegen
            .builder
            .build_alloca(llvm_pointer_type, "pointer.addr")
            .unwrap();
        codegen
            .builder
            .build_store(pointer_storage, parameter)
            .unwrap();
        codegen.local_scopes.push(HashMap::from([(
            "pointer".to_string(),
            LocalVar {
                ty: pointer_type.clone(),
                alloca: pointer_storage,
            },
        )]));
        let dereferenced = codegen
            .codegen_dereference(&Expression::Identifier("pointer".to_string()))
            .unwrap();
        let dereferenced_value = codegen.binding_basic_value(&dereferenced).unwrap();
        codegen
            .builder
            .build_return(Some(&dereferenced_value))
            .unwrap();
        codegen.local_scopes.pop();

        let address_function_type = llvm_pointer_type.fn_type(&[llvm_pointer_type.into()], false);
        let address_function = codegen
            .module
            .add_function("address", address_function_type, None);
        let address_entry = context.append_basic_block(address_function, "entry");
        codegen.builder.position_at_end(address_entry);
        let parameter = address_function.get_nth_param(0).unwrap();
        let pointer_storage = codegen
            .builder
            .build_alloca(llvm_pointer_type, "pointer.addr")
            .unwrap();
        codegen
            .builder
            .build_store(pointer_storage, parameter)
            .unwrap();
        codegen.local_scopes.push(HashMap::from([(
            "pointer".to_string(),
            LocalVar {
                ty: pointer_type,
                alloca: pointer_storage,
            },
        )]));
        let address = codegen
            .codegen_address_of(&Expression::Dereference(Box::new(Expression::Identifier(
                "pointer".to_string(),
            ))))
            .unwrap();
        let address_value = codegen.binding_basic_value(&address).unwrap();
        codegen.builder.build_return(Some(&address_value)).unwrap();

        assert!(codegen.module.verify().is_ok());
    }

    #[test]
    fn nested_union_struct_cast_generates_valid_llvm() {
        let context = Context::create();
        let target_machine = test_target_machine();
        let mut codegen = Codegen::new(&context, "nested_struct_cast", &target_machine);
        let small_sum = Type::Union(vec![Type::S32, Type::S64]);
        let large_sum = Type::Union(vec![Type::S16, Type::S32, Type::S64]);
        let source_type = Type::Struct(vec![StructFieldType {
            name: "value".to_string(),
            ty: small_sum,
        }]);
        let target_type = Type::Struct(vec![StructFieldType {
            name: "value".to_string(),
            ty: large_sum,
        }]);
        let source_llvm_type = codegen.llvm_type(&source_type).unwrap().into_struct_type();
        let source_union = codegen
            .llvm_type(&Type::Union(vec![Type::S32, Type::S64]))
            .unwrap()
            .const_zero();
        let source_value = source_llvm_type.const_named_struct(&[source_union]);
        let function_type = codegen.llvm_type(&target_type).unwrap().fn_type(&[], false);
        let function = codegen.module.add_function("run", function_type, None);
        let entry = context.append_basic_block(function, "entry");
        codegen.builder.position_at_end(entry);

        let converted = codegen
            .codegen_cast(
                Binding::Struct {
                    ty: source_type,
                    value: source_value,
                    storage: None,
                },
                target_type,
            )
            .unwrap();
        let returned_value = codegen.binding_basic_value(&converted).unwrap();
        codegen.builder.build_return(Some(&returned_value)).unwrap();

        assert!(codegen.module.verify().is_ok());
    }

    #[test]
    fn slice_element_sum_cast_generates_valid_llvm() {
        let context = Context::create();
        let target_machine = test_target_machine();
        let mut codegen = Codegen::new(&context, "slice_element_cast", &target_machine);
        let source_type = Type::Slice {
            element: Box::new(Type::S32),
        };
        let target_type = Type::Slice {
            element: Box::new(Type::Union(vec![Type::S32, Type::S64])),
        };
        let slice_llvm_type = codegen.slice_type();
        let source_value = slice_llvm_type.const_named_struct(&[
            context
                .ptr_type(AddressSpace::default())
                .const_null()
                .into(),
            codegen.ptr_sized_int_type().const_zero().into(),
        ]);
        let function_type = codegen.llvm_type(&target_type).unwrap().fn_type(&[], false);
        let function = codegen.module.add_function("run", function_type, None);
        let entry = context.append_basic_block(function, "entry");
        codegen.builder.position_at_end(entry);

        let converted = codegen
            .codegen_cast(
                Binding::Slice {
                    ty: source_type,
                    value: source_value,
                },
                target_type,
            )
            .unwrap();
        let returned_value = codegen.binding_basic_value(&converted).unwrap();
        codegen.builder.build_return(Some(&returned_value)).unwrap();

        assert!(codegen.module.verify().is_ok());
    }
}
