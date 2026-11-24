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
    BinaryOperator, ComparisonOperator, Definition, Expression, ExternalFunction, IfArm,
    IfCondition, MatchArm, Program, Signature, Span, StructFieldInit, StructFieldType, Type,
    format_error, unspanned_expression,
};

#[derive(Debug, Clone)]
enum Binding<'ctx> {
    Integer {
        ty: Type,
        value: IntValue<'ctx>,
    },
    Symbol {
        name: String,
        value: StructValue<'ctx>,
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

#[derive(Clone, Copy)]
struct LoopContext<'ctx> {
    continue_bb: inkwell::basic_block::BasicBlock<'ctx>,
    break_bb: inkwell::basic_block::BasicBlock<'ctx>,
    defer_depth: usize,
}

#[derive(Clone)]
struct BlockContext<'ctx> {
    merge_bb: inkwell::basic_block::BasicBlock<'ctx>,
    result_slot: Option<PointerValue<'ctx>>,
    result_ty: Option<Type>,
    defer_scope_idx: usize,
}

fn block_contains_yield(expr: &Expression) -> bool {
    match crate::syntax::unspanned_expression(expr) {
        Expression::Yield(_) => true,
        Expression::Block(_) => false,
        Expression::If { arms } => arms.iter().any(|arm| {
            if let crate::syntax::IfCondition::Expression(e) = &arm.condition {
                block_contains_yield(e) || block_contains_yield(&arm.body)
            } else {
                block_contains_yield(&arm.body)
            }
        }),
        Expression::Match { arms, .. } => arms.iter().any(|arm| block_contains_yield(&arm.body)),
        Expression::Repeat { body, .. }
        | Expression::RepeatWhile { body, .. }
        | Expression::RepeatUntil { body, .. }
        | Expression::For { body, .. } => block_contains_yield(body),
        Expression::Discard(e) | Expression::Defer(e) => block_contains_yield(e),
        _ => false,
    }
}

pub struct Codegen<'ctx> {
    context: &'ctx Context,
    module: Module<'ctx>,
    builder: Builder<'ctx>,
    target_data: TargetData,

    local_scopes: Vec<HashMap<String, LocalVar<'ctx>>>,
    scope_stack_ptrs: Vec<PointerValue<'ctx>>,
    global_vars: HashMap<String, GlobalVar<'ctx>>,
    pending_global_records: Vec<(String, Expression)>,
    extern_functions: HashMap<String, Signature>,
    function_signatures: HashMap<String, Signature>,
    type_aliases: HashMap<String, Type>,
    current_function_return_type: Option<Type>,
    loop_stack: Vec<LoopContext<'ctx>>,
    block_stack: Vec<BlockContext<'ctx>>,
    defer_stack: Vec<Vec<Expression>>,

    current_context_ptr: Option<PointerValue<'ctx>>,
    context_is_local: bool,
    specialized_def_names: HashSet<String>,
    current_span: Option<Span>,
    current_caller_line: Option<IntValue<'ctx>>,
    current_caller_file: Option<BasicValueEnum<'ctx>>,

    string_counter: usize,
    branch_counter: usize,
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
            current_function_return_type: None,
            loop_stack: Vec::new(),
            block_stack: Vec::new(),
            defer_stack: Vec::new(),
            current_context_ptr: None,
            context_is_local: false,
            specialized_def_names: HashSet::new(),
            current_span: None,
            current_caller_line: None,
            current_caller_file: None,
            string_counter: 0,
            branch_counter: 0,
        }
    }

    fn create_entry_block_alloca(
        &self,
        function: FunctionValue<'ctx>,
        llvm_ty: BasicTypeEnum<'ctx>,
        name: &str,
    ) -> Result<PointerValue<'ctx>, String> {
        let builder = self.context.create_builder();
        let entry = function
            .get_first_basic_block()
            .ok_or_else(|| "function has no entry block".to_string())?;
        match entry.get_first_instruction() {
            Some(first_instr) => builder.position_before(&first_instr),
            None => builder.position_at_end(entry),
        }
        builder.build_alloca(llvm_ty, name).map_err(|e| e.to_string())
    }

    fn ensure_context_is_local(&mut self) -> Result<PointerValue<'ctx>, String> {
        let current_ptr = self
            .current_context_ptr
            .ok_or_else(|| "missing @ctx".to_string())?;
        if self.context_is_local {
            return Ok(current_ptr);
        }

        let ctx_ty = crate::sema::context_type();
        let llvm_ctx_ty = self.llvm_type(&ctx_ty)?;
        let current_fn = self
            .builder
            .get_insert_block()
            .and_then(|bb| bb.get_parent())
            .ok_or_else(|| "cannot locate current function for COW context".to_string())?;
        let local_alloca =
            self.create_entry_block_alloca(current_fn, llvm_ctx_ty, "__cow_ctx_storage")?;

        let loaded = self
            .builder
            .build_load(llvm_ctx_ty, current_ptr, "cow_ctx_load")
            .map_err(|e| e.to_string())?;
        self.builder
            .build_store(local_alloca, loaded)
            .map_err(|e| e.to_string())?;

        self.current_context_ptr = Some(local_alloca);
        self.context_is_local = true;
        Ok(local_alloca)
    }

    fn ptr_sized_int_type(&self) -> IntType<'ctx> {
        self.context
            .ptr_sized_int_type(&self.target_data, Some(AddressSpace::default()))
    }

    fn ptr_byte_size(&self) -> u64 {
        self.target_data
            .get_pointer_byte_size(Some(AddressSpace::default())) as u64
    }

    fn symbol_binding(&self, name: &str) -> Binding<'ctx> {
        Binding::Symbol {
            name: name.to_string(),
            value: self.context.struct_type(&[], false).const_zero(),
        }
    }

    fn bool_type(&self) -> Type {
        Type::Union(vec![
            Type::Symbol("false".to_string()),
            Type::Symbol("true".to_string()),
        ])
    }

    fn bool_binding(&mut self, cond: IntValue<'ctx>) -> Result<Binding<'ctx>, String> {
        let bool_ty = self.bool_type();
        let llvm_bool_ty = self.llvm_type(&bool_ty)?.into_struct_type();
        let tag_i8 = self
            .builder
            .build_int_z_extend(cond, self.context.i8_type(), "bool_tag")
            .map_err(|e| e.to_string())?;
        let alloca = self
            .builder
            .build_alloca(llvm_bool_ty, "bool_union")
            .map_err(|e| e.to_string())?;
        self.builder
            .build_store(alloca, llvm_bool_ty.const_zero())
            .map_err(|e| e.to_string())?;
        let tag_ptr = self
            .builder
            .build_struct_gep(llvm_bool_ty, alloca, 0, "tag_ptr")
            .map_err(|e| e.to_string())?;
        self.builder
            .build_store(tag_ptr, tag_i8)
            .map_err(|e| e.to_string())?;
        let loaded = self
            .builder
            .build_load(llvm_bool_ty, alloca, "boxed_bool")
            .map_err(|e| e.to_string())?
            .into_struct_value();
        Ok(Binding::Union {
            ty: bool_ty,
            value: loaded,
            storage: Some(alloca),
        })
    }

    fn is_slice_type(&self, ty: &Type) -> Result<bool, String> {
        let resolved = self.resolve_alias_root(ty)?;
        if let Type::Struct(fields) = resolved {
            if fields.len() == 2 && fields[0].name == "ptr" && fields[1].name == "len" {
                let f0 = self.resolve_alias_root(&fields[0].ty)?;
                let f1 = self.resolve_alias_root(&fields[1].ty)?;
                if matches!(f0, Type::Pointer { .. }) && matches!(f1, Type::USize) {
                    return Ok(true);
                }
            }
        }
        Ok(false)
    }

    fn slice_element_type(&self, ty: &Type) -> Result<Type, String> {
        let resolved = self.resolve_alias_root(ty)?;
        if let Type::Struct(fields) = resolved {
            if fields.len() == 2 && fields[0].name == "ptr" {
                let f0 = self.resolve_alias_root(&fields[0].ty)?;
                if let Type::Pointer { element, .. } = f0 {
                    return Ok((**element).clone());
                }
            }
        }
        Err(format!("expected slice type, found {}", ty))
    }

    fn is_atom_type(&self, ty: &Type) -> Result<bool, String> {
        let resolved = self.resolve_alias_root(ty)?;
        if let Type::Struct(fields) = resolved {
            if fields.len() == 1 && fields[0].name == "str" {
                return self.is_slice_type(&fields[0].ty);
            }
        }
        Ok(false)
    }

    fn is_record_type(&self, ty: &Type) -> Result<bool, String> {
        Ok(Checker::is_record_type_with_aliases(ty, &self.type_aliases))
    }

    fn record_from_struct_binding(
        &mut self,
        fields: &[StructFieldType],
        struct_binding: Binding<'ctx>,
    ) -> Result<Binding<'ctx>, String> {
        let generic_alias = Checker::generic_type();
        let generic_type = self.resolve_alias_root(&generic_alias)?.clone();
        let key_type = Type::Alias("String".to_string());
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

        let source_value = self.binding_basic_value(&struct_binding)?.into_struct_value();

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

            let field_value = self
                .builder
                .build_extract_value(
                    source_value,
                    index as u32,
                    &format!("rec_fld_{}", field.name),
                )
                .map_err(|error| error.to_string())?;
            let field_binding = self.binding_from_basic_value(&field.ty, field_value)?;
            let boxed_value = self.codegen_cast(field_binding, generic_alias.clone())?;
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

        let (keys_pointer, values_pointer) = if count == 0 {
            (
                self.context.ptr_type(AddressSpace::default()).const_null(),
                self.context.ptr_type(AddressSpace::default()).const_null(),
            )
        } else {
            let kp = unsafe {
                self.builder
                    .build_gep(
                        keys_array_type,
                        keys_storage,
                        &[zero, zero],
                        "record.keys.ptr",
                    )
                    .map_err(|error| error.to_string())?
            };
            let vp = unsafe {
                self.builder
                    .build_gep(
                        values_array_type,
                        values_storage,
                        &[zero, zero],
                        "record.values.ptr",
                    )
                    .map_err(|error| error.to_string())?
            };
            (kp, vp)
        };

        let record_type = self.resolve_alias_root(&Checker::record_type())?.clone();
        let Type::Pointer {
            element: record_struct,
            ..
        } = &record_type
        else {
            return Err("expected Record to be a pointer type".to_string());
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

    fn slice_binding(&mut self, binding: Binding<'ctx>) -> Result<Binding<'ctx>, String> {
        let (elem_ty, size, storage_ptr) = match &binding {
            Binding::Array { ty, value, storage } => {
                let (elem, sz) = match ty {
                    Type::Array { element, size } => ((**element).clone(), *size),
                    _ => return Err("expected array type".to_string()),
                };
                let ptr = if let Some(s) = storage {
                    *s
                } else {
                    let llvm_arr_ty = value.get_type();
                    let tmp = self.builder.build_alloca(llvm_arr_ty, "slice_tmp").map_err(|e| e.to_string())?;
                    self.builder.build_store(tmp, *value).map_err(|e| e.to_string())?;
                    tmp
                };
                (elem, sz, ptr)
            }
            Binding::Struct { ty, value, storage } if matches!(self.resolve_alias_root(ty)?, Type::Tuple(_)) => {
                let elements = match self.resolve_alias_root(ty)? {
                    Type::Tuple(elems) => elems.clone(),
                    _ => unreachable!(),
                };
                if elements.is_empty() {
                    return Err("slicing on empty tuple unsupported".to_string());
                }
                let elem_ty = elements[0].clone();
                let size = elements.len();
                let llvm_elem_ty = self.llvm_type(&elem_ty)?;
                let llvm_arr_ty = llvm_elem_ty.array_type(size as u32);
                let arr_tmp = self.builder.build_alloca(llvm_arr_ty, "tuple_slice_arr").map_err(|e| e.to_string())?;
                let zero = self.ptr_sized_int_type().const_zero();
                for (i, elem_t) in elements.iter().enumerate() {
                    let idx = self.ptr_sized_int_type().const_int(i as u64, false);
                    let elem_dest_ptr = unsafe {
                        self.builder.build_gep(llvm_arr_ty, arr_tmp, &[zero, idx], "elem_dest").map_err(|e| e.to_string())?
                    };
                    let elem_val = if let Some(s) = storage {
                        let struct_ty = self.llvm_type(ty)?.into_struct_type();
                        let elem_src_ptr = self.builder.build_struct_gep(struct_ty, *s, i as u32, "tuple_elem_ptr").map_err(|e| e.to_string())?;
                        self.builder.build_load(self.llvm_type(elem_t)?, elem_src_ptr, "tuple_elem_val").map_err(|e| e.to_string())?
                    } else {
                        self.builder.build_extract_value(*value, i as u32, "tuple_elem_val").map_err(|e| e.to_string())?
                    };
                    let converted_val = if elem_t != &elem_ty {
                        let elem_binding = self.binding_from_basic_value(elem_t, elem_val)?;
                        let converted = self.codegen_cast(elem_binding, elem_ty.clone())?;
                        self.binding_basic_value(&converted)?
                    } else {
                        elem_val
                    };
                    self.builder.build_store(elem_dest_ptr, converted_val).map_err(|e| e.to_string())?;
                }
                (elem_ty, size, arr_tmp)
            }
            _ => return Err("slicing requires an array or tuple".to_string()),
        };

        let llvm_elem_ty = self.llvm_type(&elem_ty)?;
        let llvm_arr_ty = llvm_elem_ty.array_type(size as u32);
        let zero = self.ptr_sized_int_type().const_zero();
        let first_elem_ptr = unsafe {
            self.builder
                .build_gep(llvm_arr_ty, storage_ptr, &[zero, zero], "slice_first_elem")
                .map_err(|e| e.to_string())?
        };

        let slice_ty = Type::Struct(vec![
            StructFieldType {
                name: "ptr".to_string(),
                ty: Type::Pointer {
                    element: Box::new(elem_ty),
                    is_mut: false,
                },
            },
            StructFieldType {
                name: "len".to_string(),
                ty: Type::USize,
            },
        ]);
        let slice_llvm_ty = self.slice_type();
        let len_val = self.ptr_sized_int_type().const_int(size as u64, false);

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

        Ok(Binding::Struct {
            ty: slice_ty,
            value: slice_val,
            storage: None,
        })
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

    fn run_all_defers(&mut self) -> Result<(), String> {
        let mut defers_to_run = Vec::new();
        for scope in self.defer_stack.iter().rev() {
            for expr in scope.iter().rev() {
                defers_to_run.push(expr.clone());
            }
        }
        for expr in defers_to_run {
            self.codegen_expression(&expr)?;
        }
        Ok(())
    }

    fn run_defers_down_to(&mut self, min_depth: usize) -> Result<(), String> {
        let mut defers_to_run = Vec::new();
        let total = self.defer_stack.len();
        if total > min_depth {
            for scope_idx in (min_depth..total).rev() {
                for expr in self.defer_stack[scope_idx].iter().rev() {
                    defers_to_run.push(expr.clone());
                }
            }
        }
        for expr in defers_to_run {
            self.codegen_expression(&expr)?;
        }
        Ok(())
    }

    fn codegen_externals(&mut self, extern_functions: &[ExternalFunction]) -> Result<(), String> {
        for external in extern_functions {
            let fn_type = self.signature_to_function_type(Some(&external.signature))?;
            let func = self.module.add_function(&external.name, fn_type, None);
            if *external.signature.return_type == Type::Never {
                let attr_id = inkwell::attributes::Attribute::get_named_enum_kind_id("noreturn");
                let attr = self.context.create_enum_attribute(attr_id, 0);
                func.add_attribute(inkwell::attributes::AttributeLoc::Function, attr);
            }
        }

        Ok(())
    }

    fn codegen_definitions(&mut self, definitions: &[Definition]) -> Result<(), String> {
        for def in definitions {
            if def.name == "main" {
                continue;
            }
            if let Some(sig) = &def.signature {
                self.function_signatures
                    .insert(def.name.clone(), sig.clone());
                if def.is_specialized_def {
                    self.specialized_def_names.insert(def.name.clone());
                }
                let fn_type = match self.signature_to_function_type_with_def(Some(sig), def.is_specialized_def) {
                    Ok(ty) => ty,
                    Err(e) => return Err(format!("in definition '{}': {}", def.name, e)),
                };
                let f = self.module.add_function(&def.name, fn_type, None);
                if *sig.return_type == Type::Never {
                    let attr_id =
                        inkwell::attributes::Attribute::get_named_enum_kind_id("noreturn");
                    let attr = self.context.create_enum_attribute(attr_id, 0);
                    f.add_attribute(inkwell::attributes::AttributeLoc::Function, attr);
                }
            }
        }

        for def in definitions {
            if def.name == "main" {
                continue;
            }
            if def.signature.is_none() {
                let is_record = match unspanned_expression(&def.value) {
                    Expression::Cast { ty, .. } => self.is_record_type(ty).unwrap_or(false),
                    _ => false,
                };
                if is_record {
                    let ty = self.resolve_alias_root(&Checker::record_type())?.clone();
                    let llvm_type = self.llvm_type(&ty)?;
                    let global_name = format!("hart.record.{}", def.name);
                    let global =
                        self.module
                            .add_global(llvm_type, Some(AddressSpace::default()), &global_name);
                    global.set_linkage(inkwell::module::Linkage::Internal);
                    global.set_initializer(&self.zero_value(&ty)?);
                    self.global_vars.insert(
                        def.name.clone(),
                        GlobalVar {
                            ty,
                            pointer: global.as_pointer_value(),
                        },
                    );
                    self.pending_global_records
                        .push((def.name.clone(), def.value.clone()));
                }
            }
        }

        for def in definitions {
            if def.name == "main" {
                continue;
            }
            if self.pending_global_records.iter().any(|(name, _)| name == &def.name) {
                continue;
            }

            if def.signature.is_none() {
                let binding = self.codegen_expression(&def.value)?;
                let ty = self.binding_type(&binding)?;
                let llvm_ty = self.llvm_type(&ty)?;
                let global_name = format!("hart.const.{}", def.name);
                let global =
                    self.module
                        .add_global(llvm_ty, Some(AddressSpace::default()), &global_name);
                global.set_linkage(inkwell::module::Linkage::Internal);
                global.set_constant(true);

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

        for def in definitions {
            if def.name == "main" {
                continue;
            }
            if self.pending_global_records.iter().any(|(name, _)| name == &def.name) {
                continue;
            }

            if let Some(sig) = &def.signature {
                self.codegen_named_function(&def.name, Some(sig), &def.value)?;
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
        self.signature_to_function_type_with_def(signature, false)
    }

    fn signature_to_function_type_with_def(
        &self,
        signature: Option<&Signature>,
        is_specialized_def: bool,
    ) -> Result<FunctionType<'ctx>, String> {
        if let Some(sig) = signature {
            let mut param_types = Vec::new();
            if is_specialized_def {
                param_types.push(BasicMetadataTypeEnum::from(
                    self.context.ptr_type(AddressSpace::default()),
                ));
                param_types.push(BasicMetadataTypeEnum::from(
                    self.context.i64_type(),
                ));
                let string_alias = Type::Alias("String".to_string());
                let str_ty = self.resolve_alias_root(&string_alias)?;
                param_types.push(BasicMetadataTypeEnum::from(
                    self.llvm_type(str_ty)?,
                ));
            }
            for p in &sig.parameters {
                let llvm_ty = match self.llvm_type(&p.ty) {
                    Ok(ty) => ty,
                    Err(e) => {
                        return Err(format!("parameter '{}' (type {:?}): {}", p.name, p.ty, e));
                    }
                };
                param_types.push(BasicMetadataTypeEnum::from(llvm_ty));
            }

            let ret_type = &sig.return_type;
            if **ret_type == Type::Never {
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
        body: &Expression,
    ) -> Result<(), String> {
        let llvm_function = self
            .module
            .get_function(name)
            .ok_or_else(|| format!("missing declared function '{}'", name))?;

        let entry = self.context.append_basic_block(llvm_function, "entry");
        self.builder.position_at_end(entry);

        let previous_scopes = std::mem::take(&mut self.local_scopes);
        let previous_stack_ptrs = std::mem::take(&mut self.scope_stack_ptrs);
        let previous_defers = std::mem::take(&mut self.defer_stack);
        self.defer_stack.push(Vec::new());
        let previous_context_ptr = self.current_context_ptr;
        let previous_context_is_local = self.context_is_local;
        let previous_caller_line = self.current_caller_line;
        let previous_caller_file = self.current_caller_file;
        let previous_return_type = self.current_function_return_type.take();
        if let Some(sig) = signature {
            self.current_function_return_type = Some((*sig.return_type).clone());
        }
        let stack_ptr = self.enter_scope()?;

        let is_specialized = self.specialized_def_names.contains(name);
        if is_specialized {
            let ctx_param = llvm_function
                .get_nth_param(0)
                .ok_or_else(|| format!("missing context parameter in '{}'", name))?
                .into_pointer_value();
            self.current_context_ptr = Some(ctx_param);
            self.context_is_local = false;

            let line_param = llvm_function
                .get_nth_param(1)
                .ok_or_else(|| format!("missing caller line parameter in '{}'", name))?
                .into_int_value();
            self.current_caller_line = Some(line_param);

            let file_param = llvm_function
                .get_nth_param(2)
                .ok_or_else(|| format!("missing caller file parameter in '{}'", name))?;
            self.current_caller_file = Some(file_param);
        } else {
            let ctx_ty = crate::sema::context_type();
            let llvm_ctx_ty = self.llvm_type(&ctx_ty)?;
            let ctx_alloca = self
                .builder
                .build_alloca(llvm_ctx_ty, "__ctx_storage")
                .map_err(|e| e.to_string())?;
            self.current_context_ptr = Some(ctx_alloca);
            self.context_is_local = true;
            self.current_caller_line = None;
            self.current_caller_file = None;
        }

        if let Some(sig) = signature {
            let param_offset = if is_specialized { 3 } else { 0 };
            for (index, parameter) in sig.parameters.iter().enumerate() {
                let ty = &parameter.ty;
                let llvm_ty = self.llvm_type(ty)?;
                let param_val = llvm_function
                    .get_nth_param((index + param_offset) as u32)
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

        let body_binding = self.codegen_expression(body)?;

        let needs_terminator = self
            .builder
            .get_insert_block()
            .map_or(false, |bb| bb.get_terminator().is_none());

        if needs_terminator {
            let ret_ty = if let Some(sig) = signature {
                (*sig.return_type).clone()
            } else {
                Type::Symbol("none".to_string())
            };

            self.run_all_defers()?;
            self.exit_scope(stack_ptr)?;

            if ret_ty == Type::Never {
                self.builder
                    .build_unreachable()
                    .map_err(|e| e.to_string())?;
            } else {
                let return_val = self.binding_basic_value(&body_binding)?;
                self.builder
                    .build_return(Some(&return_val))
                    .map_err(|e| e.to_string())?;
            }
        } else {
            self.local_scopes.pop();
            self.scope_stack_ptrs.pop();
        }

        self.defer_stack = previous_defers;
        self.local_scopes = previous_scopes;
        self.scope_stack_ptrs = previous_stack_ptrs;
        self.current_function_return_type = previous_return_type;
        self.current_context_ptr = previous_context_ptr;
        self.context_is_local = previous_context_is_local;
        self.current_caller_line = previous_caller_line;
        self.current_caller_file = previous_caller_file;
        Ok(())
    }

    fn codegen_main(&mut self, main_def: &Definition) -> Result<(), String> {
        let i32_type = self.context.i32_type();
        let ptr_type = self.context.ptr_type(AddressSpace::default());
        let function_type = i32_type.fn_type(&[i32_type.into(), ptr_type.into()], false);

        let function = self.module.add_function("main", function_type, None);
        let entry = self.context.append_basic_block(function, "entry");

        self.builder.position_at_end(entry);
        let previous_scopes = std::mem::take(&mut self.local_scopes);
        let previous_stack_ptrs = std::mem::take(&mut self.scope_stack_ptrs);
        let previous_defers = std::mem::take(&mut self.defer_stack);
        self.defer_stack.push(Vec::new());
        let previous_context_ptr = self.current_context_ptr;
        let previous_context_is_local = self.context_is_local;
        let previous_caller_line = self.current_caller_line;
        let previous_caller_file = self.current_caller_file;
        self.current_caller_line = None;
        self.current_caller_file = None;
        let ctx_ty = crate::sema::context_type();
        let llvm_ctx_ty = self.llvm_type(&ctx_ty)?;
        let ctx_alloca = self
            .builder
            .build_alloca(llvm_ctx_ty, "__ctx_storage")
            .map_err(|e| e.to_string())?;
        self.current_context_ptr = Some(ctx_alloca);
        self.context_is_local = true;
        let previous_return_type = self.current_function_return_type.take();
        if let Some(sig) = &main_def.signature {
            self.current_function_return_type = Some((*sig.return_type).clone());
        } else {
            self.current_function_return_type = Some(Type::S32);
        }
        let stack_ptr = self.enter_scope()?;

        for (name, expr) in std::mem::take(&mut self.pending_global_records) {
            let record = self.codegen_expression(&expr)?;
            let value = self.binding_basic_value(&record)?;
            let global = self
                .global_vars
                .get(&name)
                .ok_or_else(|| format!("missing global record storage for '{}'", name))?;
            self.builder
                .build_store(global.pointer, value)
                .map_err(|error| error.to_string())?;
        }

        if let Some(sig) = &main_def.signature {
            if let Some(param) = sig.parameters.first() {
                let usize_type = self.ptr_sized_int_type();
                let string_struct_ty = self
                    .context
                    .struct_type(&[ptr_type.into(), usize_type.into()], false);
                let slice_struct_ty = self
                    .context
                    .struct_type(&[ptr_type.into(), usize_type.into()], false);

                let argc_val = function.get_nth_param(0).unwrap().into_int_value();
                let argv_val = function.get_nth_param(1).unwrap().into_pointer_value();

                let argc_usize = self
                    .builder
                    .build_int_z_extend(argc_val, usize_type, "argc_usize")
                    .map_err(|e| e.to_string())?;

                let malloc_fn = self.module.get_function("malloc").unwrap_or_else(|| {
                    let fn_type = ptr_type.fn_type(&[usize_type.into()], false);
                    self.module.add_function("malloc", fn_type, None)
                });
                let strlen_fn = self.module.get_function("strlen").unwrap_or_else(|| {
                    let fn_type = usize_type.fn_type(&[ptr_type.into()], false);
                    self.module.add_function("strlen", fn_type, None)
                });

                let sixteen = usize_type.const_int(16, false);
                let alloc_size = self
                    .builder
                    .build_int_mul(argc_usize, sixteen, "alloc_size")
                    .map_err(|e| e.to_string())?;
                let malloc_call = self
                    .builder
                    .build_call(malloc_fn, &[alloc_size.into()], "args_mem")
                    .map_err(|e| e.to_string())?;
                let args_buffer = malloc_call
                    .try_as_basic_value()
                    .basic()
                    .ok_or_else(|| "malloc must return a pointer".to_string())?
                    .into_pointer_value();

                let loop_cond = self.context.append_basic_block(function, "args.loop.cond");
                let loop_body = self.context.append_basic_block(function, "args.loop.body");
                let loop_end = self.context.append_basic_block(function, "args.loop.end");

                let i_alloca = self
                    .builder
                    .build_alloca(usize_type, "args.i")
                    .map_err(|e| e.to_string())?;
                self.builder
                    .build_store(i_alloca, usize_type.const_zero())
                    .map_err(|e| e.to_string())?;
                self.builder
                    .build_unconditional_branch(loop_cond)
                    .map_err(|e| e.to_string())?;

                self.builder.position_at_end(loop_cond);
                let cur_i = self
                    .builder
                    .build_load(usize_type, i_alloca, "cur_i")
                    .map_err(|e| e.to_string())?
                    .into_int_value();
                let has_more = self
                    .builder
                    .build_int_compare(inkwell::IntPredicate::ULT, cur_i, argc_usize, "has_more")
                    .map_err(|e| e.to_string())?;
                self.builder
                    .build_conditional_branch(has_more, loop_body, loop_end)
                    .map_err(|e| e.to_string())?;

                self.builder.position_at_end(loop_body);
                let arg_ptr_gep = unsafe {
                    self.builder
                        .build_gep(ptr_type, argv_val, &[cur_i], "arg_ptr_gep")
                }
                .map_err(|e| e.to_string())?;
                let arg_cstr = self
                    .builder
                    .build_load(ptr_type, arg_ptr_gep, "arg_cstr")
                    .map_err(|e| e.to_string())?
                    .into_pointer_value();

                let strlen_call = self
                    .builder
                    .build_call(strlen_fn, &[arg_cstr.into()], "arg_len")
                    .map_err(|e| e.to_string())?;
                let arg_len = strlen_call
                    .try_as_basic_value()
                    .basic()
                    .ok_or_else(|| "strlen must return a value".to_string())?
                    .into_int_value();

                let elem_gep = unsafe {
                    self.builder
                        .build_gep(string_struct_ty, args_buffer, &[cur_i], "elem_gep")
                }
                .map_err(|e| e.to_string())?;

                let field0_gep = self
                    .builder
                    .build_struct_gep(string_struct_ty, elem_gep, 0, "elem_ptr_gep")
                    .map_err(|e| e.to_string())?;
                self.builder
                    .build_store(field0_gep, arg_cstr)
                    .map_err(|e| e.to_string())?;

                let field1_gep = self
                    .builder
                    .build_struct_gep(string_struct_ty, elem_gep, 1, "elem_len_gep")
                    .map_err(|e| e.to_string())?;
                self.builder
                    .build_store(field1_gep, arg_len)
                    .map_err(|e| e.to_string())?;

                let next_i = self
                    .builder
                    .build_int_add(cur_i, usize_type.const_int(1, false), "next_i")
                    .map_err(|e| e.to_string())?;
                self.builder
                    .build_store(i_alloca, next_i)
                    .map_err(|e| e.to_string())?;
                self.builder
                    .build_unconditional_branch(loop_cond)
                    .map_err(|e| e.to_string())?;

                self.builder.position_at_end(loop_end);

                let slice_alloca = self
                    .builder
                    .build_alloca(slice_struct_ty, &param.name)
                    .map_err(|e| e.to_string())?;
                let slice_ptr_gep = self
                    .builder
                    .build_struct_gep(slice_struct_ty, slice_alloca, 0, "slice_ptr_gep")
                    .map_err(|e| e.to_string())?;
                self.builder
                    .build_store(slice_ptr_gep, args_buffer)
                    .map_err(|e| e.to_string())?;
                let slice_len_gep = self
                    .builder
                    .build_struct_gep(slice_struct_ty, slice_alloca, 1, "slice_len_gep")
                    .map_err(|e| e.to_string())?;
                self.builder
                    .build_store(slice_len_gep, argc_usize)
                    .map_err(|e| e.to_string())?;

                self.local_scopes.last_mut().unwrap().insert(
                    param.name.clone(),
                    LocalVar {
                        ty: param.ty.clone(),
                        alloca: slice_alloca,
                    },
                );
            }
        }

        let body_binding = self.codegen_expression(&main_def.value)?;

        let needs_terminator = self
            .builder
            .get_insert_block()
            .map_or(false, |bb| bb.get_terminator().is_none());

        if needs_terminator {
            self.run_all_defers()?;
            self.exit_scope(stack_ptr)?;
            let return_val: BasicValueEnum<'ctx> = match &body_binding {
                Binding::Integer { value, .. } => (*value).into(),
                _ => self.context.i32_type().const_zero().into(),
            };
            self.builder
                .build_return(Some(&return_val))
                .map_err(|e| e.to_string())?;
        } else {
            self.local_scopes.pop();
            self.scope_stack_ptrs.pop();
        }

        self.defer_stack = previous_defers;
        self.local_scopes = previous_scopes;
        self.scope_stack_ptrs = previous_stack_ptrs;
        self.current_function_return_type = previous_return_type;
        self.current_context_ptr = previous_context_ptr;
        self.context_is_local = previous_context_is_local;
        self.current_caller_line = previous_caller_line;
        self.current_caller_file = previous_caller_file;
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

    fn codegen_expression(&mut self, expression: &Expression) -> Result<Binding<'ctx>, String> {
        match expression {
            Expression::Spanned { expression, span } => {
                let old_span = self.current_span.replace(span.clone());
                let res = self
                    .codegen_expression(expression)
                    .map_err(|error| format_error(span, &error));
                self.current_span = old_span;
                res
            }
            Expression::Integer(value) => Ok(Binding::Integer {
                ty: Type::S32,
                value: self.context.i32_type().const_int(*value as u64, true),
            }),
            Expression::Symbol(name) => {
                let empty_struct = self.context.struct_type(&[], false).const_zero();
                Ok(Binding::Symbol {
                    name: name.clone(),
                    value: empty_struct,
                })
            }
            Expression::String(value) => self.codegen_string_literal(value),
            Expression::Struct(fields) => self.codegen_struct_literal(fields),
            Expression::Spread(inner) => self.codegen_expression(inner),
            Expression::Tuple(elements) => self.codegen_tuple_literal(elements),
            Expression::Context => {
                let ptr = self
                    .current_context_ptr
                    .ok_or_else(|| "missing @ctx in current function".to_string())?;
                let ctx_ty = crate::sema::context_type();
                let llvm_ty = self.llvm_type(&ctx_ty)?;
                let loaded = self
                    .builder
                    .build_load(llvm_ty, ptr, "ctx_val")
                    .map_err(|e| e.to_string())?
                    .into_struct_value();
                Ok(Binding::Struct {
                    ty: ctx_ty,
                    value: loaded,
                    storage: Some(ptr),
                })
            }
            Expression::CallerLineNumber => {
                let line_val = self
                    .current_caller_line
                    .ok_or_else(|| "@caller_line_number used outside of def function".to_string())?;
                Ok(Binding::Integer {
                    ty: Type::U64,
                    value: line_val,
                })
            }
            Expression::CallerFileName => {
                let file_val = self
                    .current_caller_file
                    .ok_or_else(|| "@caller_file_name used outside of def function".to_string())?;
                Ok(Binding::Struct {
                    ty: Type::Alias("String".to_string()),
                    value: file_val.into_struct_value(),
                    storage: None,
                })
            }
            Expression::ThisLineNumber(line) => {
                Ok(Binding::Integer {
                    ty: Type::U64,
                    value: self.context.i64_type().const_int(*line, false),
                })
            }
            Expression::ThisFileName(file) => {
                self.codegen_string_literal(file.as_bytes())
            }
            Expression::Identifier(name) => {
                if let Some(local) = self.lookup_local(name) {
                    let resolved_local_ty = self.resolve_alias_root(&local.ty)?;
                    if matches!(resolved_local_ty, Type::Struct(_) | Type::Tuple(_)) {
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

                    if let Type::Union(_) = resolved_local_ty {
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

                    if let Type::Array { .. } = resolved_local_ty {
                        let loaded = self
                            .builder
                            .build_load(self.llvm_type(&local.ty)?, local.alloca, name)
                            .map_err(|e| e.to_string())?
                            .into_array_value();

                        return Ok(Binding::Array {
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
                    let resolved_global_ty = self.resolve_alias_root(&global_var.ty)?;
                    if matches!(resolved_global_ty, Type::Struct(_) | Type::Tuple(_)) {
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

                    if let Type::Union(_) = resolved_global_ty {
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

                    if let Type::Array { .. } = resolved_global_ty {
                        let loaded = self
                            .builder
                            .build_load(self.llvm_type(&global_var.ty)?, global_var.pointer, name)
                            .map_err(|e| e.to_string())?
                            .into_array_value();

                        return Ok(Binding::Array {
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
                            return_type: Box::new(Type::Symbol("none".to_string())),
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
            Expression::Block(exprs) => {
                if exprs.iter().any(block_contains_yield) {
                    if let Some(current_bb) = self.builder.get_insert_block() {
                        if let Some(current_fn) = current_bb.get_parent() {
                            let id = self.branch_counter;
                            self.branch_counter += 1;
                            let merge_bb = self
                                .context
                                .append_basic_block(current_fn, &format!("block.merge.{}", id));

                            let defer_scope_idx = self.defer_stack.len();
                            self.defer_stack.push(Vec::new());

                            self.block_stack.push(BlockContext {
                                merge_bb,
                                result_slot: None,
                                result_ty: None,
                                defer_scope_idx,
                            });

                            self.local_scopes.push(HashMap::new());
                            let mut last_binding = self.symbol_binding("none");
                            for expr in exprs {
                                if let Some(bb) = self.builder.get_insert_block() {
                                    if bb.get_terminator().is_some() {
                                        break;
                                    }
                                }
                                last_binding = self.codegen_expression(expr)?;
                            }

                            let has_no_terminator = self
                                .builder
                                .get_insert_block()
                                .map_or(false, |bb| bb.get_terminator().is_none());

                            let block_defers = self.defer_stack.pop().unwrap_or_default();
                            if has_no_terminator {
                                for d in block_defers.into_iter().rev() {
                                    self.codegen_expression(&d)?;
                                }
                            }
                            self.local_scopes.pop();

                            let block_ctx = self.block_stack.pop().unwrap();

                            if let Some(bb) = self.builder.get_insert_block() {
                                if bb.get_terminator().is_none() {
                                    self.builder
                                        .build_unconditional_branch(merge_bb)
                                        .map_err(|e| e.to_string())?;
                                }
                            }

                            self.builder.position_at_end(merge_bb);

                            if let (Some(alloca), Some(ty)) = (block_ctx.result_slot, block_ctx.result_ty) {
                                let llvm_ty = self.llvm_type(&ty)?;
                                let loaded = self
                                    .builder
                                    .build_load(llvm_ty, alloca, "yield.val")
                                    .map_err(|e| e.to_string())?;
                                return self.binding_from_basic_value(&ty, loaded);
                            } else {
                                return Ok(last_binding);
                            }
                        }
                    }
                }

                self.defer_stack.push(Vec::new());
                self.local_scopes.push(HashMap::new());
                let mut last_binding = self.symbol_binding("none");
                for expr in exprs {
                    if let Some(bb) = self.builder.get_insert_block() {
                        if bb.get_terminator().is_some() {
                            break;
                        }
                    }
                    last_binding = self.codegen_expression(expr)?;
                }

                let has_no_terminator = self
                    .builder
                    .get_insert_block()
                    .map_or(false, |bb| bb.get_terminator().is_none());

                let block_defers = self.defer_stack.pop().unwrap_or_default();
                if has_no_terminator {
                    for d in block_defers.into_iter().rev() {
                        self.codegen_expression(&d)?;
                    }
                }
                self.local_scopes.pop();
                Ok(last_binding)
            }
            Expression::Discard(expr) => {
                self.codegen_expression(expr)?;
                Ok(self.symbol_binding("none"))
            }
            Expression::Defer(expr) => {
                if let Some(scope) = self.defer_stack.last_mut() {
                    scope.push(*expr.clone());
                }
                Ok(self.symbol_binding("none"))
            }
            Expression::Return(expr) => {
                let mut binding = self.codegen_expression(expr)?;
                if let Some(target_ty) = self.current_function_return_type.clone() {
                    if target_ty != Type::Never && target_ty != Type::Inferred {
                        let actual_ty = self.binding_type(&binding)?;
                        if actual_ty != target_ty {
                            if let Ok(cast_val) = self.codegen_cast(binding.clone(), target_ty) {
                                binding = cast_val;
                            }
                        }
                    }
                }
                self.run_all_defers()?;
                self.exit_current_scope()?;
                self.build_return(&binding)?;
                Ok(self.symbol_binding("none"))
            }
            Expression::Yield(expr) => {
                let mut binding = self.codegen_expression(expr)?;
                let ty = self.binding_type(&binding)?;

                if self.block_stack.is_empty() {
                    return Err("cannot yield outside of block".to_string());
                }

                let (merge_bb, existing_slot, target_ty_opt, defer_scope_idx) = {
                    let ctx = self.block_stack.last().unwrap();
                    (ctx.merge_bb, ctx.result_slot, ctx.result_ty.clone(), ctx.defer_scope_idx)
                };

                let alloca = match existing_slot {
                    Some(slot) => {
                        if let Some(target_ty) = target_ty_opt {
                            if target_ty != ty {
                                if let Ok(cast_val) = self.codegen_cast(binding.clone(), target_ty) {
                                    binding = cast_val;
                                }
                            }
                        }
                        slot
                    }
                    None => {
                        let current_fn = self.builder.get_insert_block().unwrap().get_parent().unwrap();
                        let llvm_ty = self.llvm_type(&ty)?;
                        let slot = self.create_entry_block_alloca(current_fn, llvm_ty, "yield.slot")?;
                        let ctx = self.block_stack.last_mut().unwrap();
                        ctx.result_slot = Some(slot);
                        ctx.result_ty = Some(ty);
                        slot
                    }
                };

                let val = self.binding_basic_value(&binding)?;
                self.builder.build_store(alloca, val).map_err(|e| e.to_string())?;
                self.run_defers_down_to(defer_scope_idx)?;
                self.builder.build_unconditional_branch(merge_bb).map_err(|e| e.to_string())?;

                Ok(self.symbol_binding("none"))
            }
            Expression::Break => {
                let loop_ctx = self
                    .loop_stack
                    .last()
                    .ok_or_else(|| "cannot break outside of loop".to_string())?
                    .clone();
                self.run_defers_down_to(loop_ctx.defer_depth)?;
                self.builder
                    .build_unconditional_branch(loop_ctx.break_bb)
                    .map_err(|e| e.to_string())?;
                Ok(self.symbol_binding("none"))
            }
            Expression::Continue => {
                let loop_ctx = self
                    .loop_stack
                    .last()
                    .ok_or_else(|| "cannot continue outside of loop".to_string())?
                    .clone();
                self.run_defers_down_to(loop_ctx.defer_depth)?;
                self.builder
                    .build_unconditional_branch(loop_ctx.continue_bb)
                    .map_err(|e| e.to_string())?;
                Ok(self.symbol_binding("none"))
            }
            Expression::Let { name, value } => {
                let current_binding = self.codegen_expression(value)?;
                let ty = self.binding_type(&current_binding)?;
                let llvm_ty = self.llvm_type(&ty)?;

                let alloca = self
                    .builder
                    .build_alloca(llvm_ty, name)
                    .map_err(|e| e.to_string())?;

                let val = self.binding_basic_value(&current_binding)?;
                self.builder
                    .build_store(alloca, val)
                    .map_err(|e| e.to_string())?;

                self.local_scopes
                    .last_mut()
                    .unwrap()
                    .insert(name.clone(), LocalVar { ty, alloca });
                Ok(self.symbol_binding("none"))
            }
            Expression::Var { name, ty, value } => {
                if let Some(val_expr) = value {
                    let current_binding = self.codegen_expression(val_expr)?;
                    let expr_ty = self.binding_type(&current_binding)?;
                    let final_ty = ty.as_ref().unwrap_or(&expr_ty);
                    let coerced_binding = self.coerce_to_type(current_binding, final_ty)?;
                    let llvm_ty = self.llvm_type(final_ty)?;

                    let alloca = self
                        .builder
                        .build_alloca(llvm_ty, name)
                        .map_err(|e| e.to_string())?;

                    let val = self.binding_basic_value(&coerced_binding)?;
                    self.builder
                        .build_store(alloca, val)
                        .map_err(|e| e.to_string())?;

                    self.local_scopes.last_mut().unwrap().insert(
                        name.clone(),
                        LocalVar {
                            ty: final_ty.clone(),
                            alloca,
                        },
                    );
                } else if let Some(decl_ty) = ty {
                    let llvm_ty = self.llvm_type(decl_ty)?;

                    let alloca = self
                        .builder
                        .build_alloca(llvm_ty, name)
                        .map_err(|e| e.to_string())?;

                    self.local_scopes.last_mut().unwrap().insert(
                        name.clone(),
                        LocalVar {
                            ty: decl_ty.clone(),
                            alloca,
                        },
                    );
                }
                Ok(self.symbol_binding("none"))
            }
            Expression::Assign { target, value } => {
                let current_binding = self.codegen_expression(value)?;
                match unspanned_expression(target) {
                    Expression::Context => {
                        let ptr = self.ensure_context_is_local()?;
                        let ctx_ty = crate::sema::context_type();
                        let coerced_binding = self.coerce_to_type(current_binding, &ctx_ty)?;
                        let val = self.binding_basic_value(&coerced_binding)?;
                        self.builder
                            .build_store(ptr, val)
                            .map_err(|e| e.to_string())?;

                        Ok(self.symbol_binding("none"))
                    }
                    Expression::Identifier(name) => {
                        let (local_alloca, local_ty) = if let Some(local) = self.lookup_local(name) {
                            (local.alloca, local.ty.clone())
                        } else {
                            let ty = self.binding_type(&current_binding)?;
                            let llvm_ty = self.llvm_type(&ty)?;
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
                            (alloca, ty)
                        };

                        let coerced_binding = self.coerce_to_type(current_binding, &local_ty)?;
                        let val = self.binding_basic_value(&coerced_binding)?;
                        self.builder
                            .build_store(local_alloca, val)
                            .map_err(|e| e.to_string())?;

                        Ok(self.symbol_binding("none"))
                    }
                    Expression::Dereference(ptr_expr) => {
                        let ptr_binding = self.codegen_expression(ptr_expr)?;
                        let ptr_type = self.binding_type(&ptr_binding)?;
                        let Type::Pointer { element, .. } = ptr_type else {
                            return Err(format!(
                                "expected pointer type for dereference assignment, found {}",
                                ptr_type
                            ));
                        };
                        if *element == Type::Opaque {
                            return Err(
                                "cannot assign through untyped pointer '*opaque'".to_string()
                            );
                        }
                        let dest_ptr = match ptr_binding {
                            Binding::Pointer { value, .. } => value,
                            _ => {
                                return Err("expected pointer binding for dereference assignment"
                                    .to_string());
                            }
                        };

                        let coerced_binding = self.coerce_to_type(current_binding, &element)?;
                        let val = self.binding_basic_value(&coerced_binding)?;
                        self.builder
                            .build_store(dest_ptr, val)
                            .map_err(|e| e.to_string())?;

                        Ok(self.symbol_binding("none"))
                    }
                    Expression::Member { .. } => {
                        if let Expression::Member { expression: base, .. } = unspanned_expression(target) {
                            if matches!(unspanned_expression(base), Expression::Context) {
                                self.ensure_context_is_local()?;
                            }
                        }
                        let dest_ptr_binding = self.codegen_address_of(target)?;
                        let Type::Pointer { element, .. } = self.binding_type(&dest_ptr_binding)? else {
                            return Err("expected pointer from address-of member in assignment".to_string());
                        };
                        let dest_ptr = match dest_ptr_binding {
                            Binding::Pointer { value, .. } => value,
                            _ => return Err("expected pointer binding for member assignment".to_string()),
                        };
                        let coerced_binding = self.coerce_to_type(current_binding, &element)?;
                        let val = self.binding_basic_value(&coerced_binding)?;
                        self.builder
                            .build_store(dest_ptr, val)
                            .map_err(|e| e.to_string())?;

                        Ok(self.symbol_binding("none"))
                    }
                    _ => Err("unsupported assignment target".to_string()),
                }
            }
            Expression::Repeat {
                count,
                binder,
                body,
            } => {
                let count_binding = self.codegen_expression(count)?;
                let count_val = self.binding_int_value(&count_binding)?;
                let count_ty = self.binding_type(&count_binding)?;
                let count_is_signed = self.is_signed(&count_ty);

                let limit = if count_is_signed {
                    self.builder.build_int_s_extend_or_bit_cast(
                        count_val,
                        self.ptr_sized_int_type(),
                        "repeat_limit",
                    )
                } else {
                    self.builder.build_int_z_extend_or_bit_cast(
                        count_val,
                        self.ptr_sized_int_type(),
                        "repeat_limit",
                    )
                }
                .map_err(|e| e.to_string())?;

                let current_block = self
                    .builder
                    .get_insert_block()
                    .ok_or_else(|| "cannot generate repeat outside basic block".to_string())?;
                let current_fn = current_block
                    .get_parent()
                    .ok_or_else(|| "current block has no parent function".to_string())?;

                let id = self.branch_counter;
                self.branch_counter += 1;

                let cond_bb = self
                    .context
                    .append_basic_block(current_fn, &format!("repeat.cond.{}", id));
                let body_bb = self
                    .context
                    .append_basic_block(current_fn, &format!("repeat.body.{}", id));
                let step_bb = self
                    .context
                    .append_basic_block(current_fn, &format!("repeat.step.{}", id));
                let merge_bb = self
                    .context
                    .append_basic_block(current_fn, &format!("repeat.merge.{}", id));

                let counter_alloca = self
                    .builder
                    .build_alloca(self.ptr_sized_int_type(), &format!("repeat.counter.{}", id))
                    .map_err(|e| e.to_string())?;

                self.builder
                    .build_store(counter_alloca, self.ptr_sized_int_type().const_zero())
                    .map_err(|e| e.to_string())?;

                let binder_alloca = if let Some(name) = binder {
                    let binder_llvm_ty = self.llvm_type(&count_ty)?;
                    let alloca = self
                        .builder
                        .build_alloca(binder_llvm_ty, &format!("repeat.binder.{}.{}", name, id))
                        .map_err(|e| e.to_string())?;
                    Some(alloca)
                } else {
                    None
                };

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
                        "repeat_cmp",
                    )
                    .map_err(|e| e.to_string())?;

                self.builder
                    .build_conditional_branch(has_next, body_bb, merge_bb)
                    .map_err(|e| e.to_string())?;

                // --- Body Block ---
                self.builder.position_at_end(body_bb);
                if let (Some(name), Some(alloca)) = (binder, binder_alloca) {
                    self.local_scopes.push(HashMap::new());
                    let idx_binding = Binding::Integer {
                        ty: Type::USize,
                        value: current_idx,
                    };
                    let casted_binding = self.codegen_cast(idx_binding, count_ty.clone())?;
                    let val = self.binding_basic_value(&casted_binding)?;
                    self.builder
                        .build_store(alloca, val)
                        .map_err(|e| e.to_string())?;
                    self.local_scopes.last_mut().unwrap().insert(
                        name.clone(),
                        LocalVar {
                            ty: count_ty.clone(),
                            alloca,
                        },
                    );
                }

                let loop_defer_depth = self.defer_stack.len();
                self.loop_stack.push(LoopContext {
                    continue_bb: step_bb,
                    break_bb: merge_bb,
                    defer_depth: loop_defer_depth,
                });
                self.codegen_expression(body)?;
                self.loop_stack.pop();

                if binder.is_some() {
                    self.local_scopes.pop();
                }

                if let Some(bb) = self.builder.get_insert_block() {
                    if bb.get_terminator().is_none() {
                        self.builder
                            .build_unconditional_branch(step_bb)
                            .map_err(|e| e.to_string())?;
                    }
                }

                // --- Step Block ---
                self.builder.position_at_end(step_bb);
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

                self.builder.position_at_end(merge_bb);
                Ok(self.symbol_binding("none"))
            }
            Expression::For {
                iterable,
                binder,
                body,
            } => {
                let iter_binding = self.codegen_expression(iterable)?;
                let iter_ty = self.binding_type(&iter_binding)?;

                let (element_ty, len_val, data_ptr) = match iter_ty {
                    Type::Array { ref element, size } => {
                        let len = self.ptr_sized_int_type().const_int(size as u64, false);
                        let elem_clone = (**element).clone();
                        let storage = self.array_storage(iter_binding, &elem_clone, size)?;
                        let llvm_array_type = self
                            .llvm_type(&Type::Array {
                                element: Box::new(elem_clone.clone()),
                                size,
                            })?
                            .into_array_type();
                        let zero = self.ptr_sized_int_type().const_zero();
                        let first_elem_ptr = unsafe {
                            self.builder
                                .build_gep(llvm_array_type, storage, &[zero, zero], "for_arr_data")
                                .map_err(|e| e.to_string())?
                        };
                        (elem_clone, len, first_elem_ptr)
                    }
                    ref other if self.is_slice_type(other)? => {
                        let elem_ty = self.slice_element_type(other)?;
                        let slice_struct = match iter_binding {
                            Binding::Struct { value, .. } => value,
                            _ => self.binding_basic_value(&iter_binding)?.into_struct_value(),
                        };
                        let ptr_val = self
                            .builder
                            .build_extract_value(slice_struct, 0, "sliceptr")
                            .map_err(|e| e.to_string())?
                            .into_pointer_value();
                        let len_val = self
                            .builder
                            .build_extract_value(slice_struct, 1, "slicelen")
                            .map_err(|e| e.to_string())?
                            .into_int_value();
                        (elem_ty, len_val, ptr_val)
                    }
                    other => return Err(format!("cannot iterate over type {}", other)),
                };

                let current_block = self
                    .builder
                    .get_insert_block()
                    .ok_or_else(|| "cannot generate for loop outside basic block".to_string())?;
                let current_fn = current_block
                    .get_parent()
                    .ok_or_else(|| "current block has no parent function".to_string())?;

                let id = self.branch_counter;
                self.branch_counter += 1;

                let cond_bb = self
                    .context
                    .append_basic_block(current_fn, &format!("for.cond.{}", id));
                let body_bb = self
                    .context
                    .append_basic_block(current_fn, &format!("for.body.{}", id));
                let step_bb = self
                    .context
                    .append_basic_block(current_fn, &format!("for.step.{}", id));
                let merge_bb = self
                    .context
                    .append_basic_block(current_fn, &format!("for.merge.{}", id));

                let counter_alloca = self
                    .builder
                    .build_alloca(self.ptr_sized_int_type(), &format!("for.counter.{}", id))
                    .map_err(|e| e.to_string())?;

                self.builder
                    .build_store(counter_alloca, self.ptr_sized_int_type().const_zero())
                    .map_err(|e| e.to_string())?;

                let binder_alloca = if let Some(name) = binder {
                    let binder_llvm_ty = self.llvm_type(&element_ty)?;
                    let alloca = self
                        .builder
                        .build_alloca(binder_llvm_ty, &format!("for.binder.{}.{}", name, id))
                        .map_err(|e| e.to_string())?;
                    Some(alloca)
                } else {
                    None
                };

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
                    .build_int_compare(IntPredicate::ULT, current_idx, len_val, "for_cmp")
                    .map_err(|e| e.to_string())?;

                self.builder
                    .build_conditional_branch(has_next, body_bb, merge_bb)
                    .map_err(|e| e.to_string())?;

                // --- Body Block ---
                self.builder.position_at_end(body_bb);

                if let (Some(name), Some(alloca)) = (binder, binder_alloca) {
                    let elem_llvm_ty = self.llvm_type(&element_ty)?;
                    let elem_ptr = unsafe {
                        self.builder
                            .build_gep(elem_llvm_ty, data_ptr, &[current_idx], "for_elem_ptr")
                            .map_err(|e| e.to_string())?
                    };
                    let loaded = self
                        .builder
                        .build_load(elem_llvm_ty, elem_ptr, "for_elem_val")
                        .map_err(|e| e.to_string())?;

                    self.builder
                        .build_store(alloca, loaded)
                        .map_err(|e| e.to_string())?;

                    self.local_scopes.push(HashMap::new());
                    self.local_scopes.last_mut().unwrap().insert(
                        name.clone(),
                        LocalVar {
                            ty: element_ty.clone(),
                            alloca,
                        },
                    );
                }

                let loop_defer_depth = self.defer_stack.len();
                self.loop_stack.push(LoopContext {
                    continue_bb: step_bb,
                    break_bb: merge_bb,
                    defer_depth: loop_defer_depth,
                });
                self.codegen_expression(body)?;
                self.loop_stack.pop();

                if binder.is_some() {
                    self.local_scopes.pop();
                }

                if let Some(bb) = self.builder.get_insert_block() {
                    if bb.get_terminator().is_none() {
                        self.builder
                            .build_unconditional_branch(step_bb)
                            .map_err(|e| e.to_string())?;
                    }
                }

                // --- Step Block ---
                self.builder.position_at_end(step_bb);
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

                self.builder.position_at_end(merge_bb);
                Ok(self.symbol_binding("none"))
            }
            Expression::RepeatWhile { condition, body } => {
                let current_block = self
                    .builder
                    .get_insert_block()
                    .ok_or_else(|| "cannot generate repeat while outside basic block".to_string())?;
                let current_fn = current_block
                    .get_parent()
                    .ok_or_else(|| "current block has no parent function".to_string())?;

                let id = self.branch_counter;
                self.branch_counter += 1;

                let cond_bb = self
                    .context
                    .append_basic_block(current_fn, &format!("while.cond.{}", id));
                let body_bb = self
                    .context
                    .append_basic_block(current_fn, &format!("while.body.{}", id));
                let merge_bb = self
                    .context
                    .append_basic_block(current_fn, &format!("while.merge.{}", id));

                self.builder
                    .build_unconditional_branch(cond_bb)
                    .map_err(|e| e.to_string())?;

                self.builder.position_at_end(cond_bb);
                let cond_binding = self.codegen_expression(condition)?;
                let cond_val = self.binding_to_bool_val(&cond_binding)?;
                self.builder
                    .build_conditional_branch(cond_val, body_bb, merge_bb)
                    .map_err(|e| e.to_string())?;

                self.builder.position_at_end(body_bb);
                let loop_defer_depth = self.defer_stack.len();
                self.loop_stack.push(LoopContext {
                    continue_bb: cond_bb,
                    break_bb: merge_bb,
                    defer_depth: loop_defer_depth,
                });
                self.codegen_expression(body)?;
                self.loop_stack.pop();

                if let Some(bb) = self.builder.get_insert_block() {
                    if bb.get_terminator().is_none() {
                        self.builder
                            .build_unconditional_branch(cond_bb)
                            .map_err(|e| e.to_string())?;
                    }
                }

                self.builder.position_at_end(merge_bb);
                Ok(self.symbol_binding("none"))
            }
            Expression::RepeatUntil { condition, body } => {
                let current_block = self
                    .builder
                    .get_insert_block()
                    .ok_or_else(|| "cannot generate repeat until outside basic block".to_string())?;
                let current_fn = current_block
                    .get_parent()
                    .ok_or_else(|| "current block has no parent function".to_string())?;

                let id = self.branch_counter;
                self.branch_counter += 1;

                let body_bb = self
                    .context
                    .append_basic_block(current_fn, &format!("until.body.{}", id));
                let cond_bb = self
                    .context
                    .append_basic_block(current_fn, &format!("until.cond.{}", id));
                let merge_bb = self
                    .context
                    .append_basic_block(current_fn, &format!("until.merge.{}", id));

                self.builder
                    .build_unconditional_branch(body_bb)
                    .map_err(|e| e.to_string())?;

                self.builder.position_at_end(body_bb);
                let loop_defer_depth = self.defer_stack.len();
                self.loop_stack.push(LoopContext {
                    continue_bb: cond_bb,
                    break_bb: merge_bb,
                    defer_depth: loop_defer_depth,
                });
                self.codegen_expression(body)?;
                self.loop_stack.pop();

                if let Some(bb) = self.builder.get_insert_block() {
                    if bb.get_terminator().is_none() {
                        self.builder
                            .build_unconditional_branch(cond_bb)
                            .map_err(|e| e.to_string())?;
                    }
                }

                self.builder.position_at_end(cond_bb);
                let cond_binding = self.codegen_expression(condition)?;
                let cond_val = self.binding_to_bool_val(&cond_binding)?;
                self.builder
                    .build_conditional_branch(cond_val, merge_bb, body_bb)
                    .map_err(|e| e.to_string())?;

                self.builder.position_at_end(merge_bb);
                Ok(self.symbol_binding("none"))
            }
            Expression::If { arms } => self.codegen_if_expression(arms),
            Expression::Match {
                target,
                binding_name,
                arms,
            } => self.codegen_match_expression(target, binding_name, arms),
            Expression::Array(elements) => self.codegen_array_literal(elements),
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
            Expression::AddressOf { expression, is_mut } => {
                if *is_mut {
                    if let Expression::Context = unspanned_expression(expression) {
                        self.ensure_context_is_local()?;
                    } else if let Expression::Member { expression: base, .. } = unspanned_expression(expression) {
                        if let Expression::Context = unspanned_expression(base) {
                            self.ensure_context_is_local()?;
                        }
                    }
                }
                let binding = self.codegen_address_of(expression)?;
                match binding {
                    Binding::Pointer {
                        ty: Type::Function { .. },
                        value,
                    } => Ok(Binding::Pointer {
                        ty: self.binding_type(&binding)?,
                        value,
                    }),
                    Binding::Pointer {
                        ty: Type::Pointer { element, .. },
                        value,
                    } => Ok(Binding::Pointer {
                        ty: Type::Pointer {
                            element,
                            is_mut: *is_mut,
                        },
                        value,
                    }),
                    other => Ok(other),
                }
            }
            Expression::Dereference(expression) => self.codegen_dereference(expression),
            Expression::SizeOf(inner) => {
                let binding = self.codegen_expression(inner)?;
                let ty = self.binding_type(&binding)?;
                let size = self.llvm_type_size(&ty)?;
                Ok(Binding::Integer {
                    ty: Type::USize,
                    value: self.ptr_sized_int_type().const_int(size, false),
                })
            }
            Expression::AsTypeOf { expression, target } => {
                let binding = self.codegen_expression(expression)?;
                let target_binding = self.codegen_expression(target)?;
                let target_ty = self.binding_type(&target_binding)?;
                self.codegen_cast(binding, target_ty)
            }
            Expression::Slice(expr) => {
                let binding = self.codegen_expression(expr)?;
                self.slice_binding(binding)
            }
        }
    }

    fn codegen_dereference(&mut self, expression: &Expression) -> Result<Binding<'ctx>, String> {
        let pointer_binding = self.codegen_expression(expression)?;
        let pointer_type = self.binding_type(&pointer_binding)?;
        if matches!(pointer_type, Type::Function { .. }) {
            return Ok(pointer_binding);
        }
        let Type::Pointer { element, .. } = pointer_type else {
            return Err(format!(
                "cannot dereference non-pointer type {}",
                pointer_type
            ));
        };
        if matches!(element.as_ref(), Type::Function { .. }) {
            return Ok(Binding::Pointer {
                ty: *element,
                value: match pointer_binding {
                    Binding::Pointer { value, .. } => value,
                    _ => return Err("expected pointer binding for dereference".to_string()),
                },
            });
        }
        if *element == Type::Opaque {
            return Err("cannot dereference untyped pointer '*opaque'".to_string());
        }
        let pointer_value = match pointer_binding {
            Binding::Pointer { value, .. } => value,
            _ => return Err("expected pointer binding for dereference".to_string()),
        };
        let element_llvm_type = self.llvm_type(&element)?;
        let loaded = self
            .builder
            .build_load(element_llvm_type, pointer_value, "deref_value")
            .map_err(|error| error.to_string())?;

        let resolved_elem = self.resolve_alias_root(&element)?;
        match resolved_elem {
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
            Type::Array { .. } => Ok(Binding::Array {
                ty: *element,
                value: loaded.into_array_value(),
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
                name: f.name.clone().unwrap_or_default(),
                ty,
            });
            field_values.push(val);
        }

        let struct_type = Type::Struct(field_types);
        let llvm_struct_type = self.llvm_type(&struct_type)?.into_struct_type();

        let struct_val = if self.builder.get_insert_block().is_none() {
            llvm_struct_type.const_named_struct(&field_values)
        } else {
            let mut struct_val = llvm_struct_type.get_undef();
            for (i, val) in field_values.into_iter().enumerate() {
                struct_val = self
                    .builder
                    .build_insert_value(struct_val, val, i as u32, &format!("struct_fld_{}", i))
                    .map_err(|e| e.to_string())?
                    .into_struct_value();
            }
            struct_val
        };

        Ok(Binding::Struct {
            ty: struct_type,
            value: struct_val,
            storage: None,
        })
    }

    fn codegen_tuple_literal(
        &mut self,
        elements: &[Expression],
    ) -> Result<Binding<'ctx>, String> {
        let mut element_types = Vec::with_capacity(elements.len());
        let mut element_values = Vec::with_capacity(elements.len());

        for elem in elements {
            let binding = self.codegen_expression(elem)?;
            let ty = self.binding_type(&binding)?;
            let val = self.binding_basic_value(&binding)?;

            element_types.push(ty);
            element_values.push(val);
        }

        let tuple_type = Type::Tuple(element_types);
        let llvm_struct_type = self.llvm_type(&tuple_type)?.into_struct_type();

        let struct_val = if self.builder.get_insert_block().is_none() {
            llvm_struct_type.const_named_struct(&element_values)
        } else {
            let mut struct_val = llvm_struct_type.get_undef();
            for (i, val) in element_values.into_iter().enumerate() {
                struct_val = self
                    .builder
                    .build_insert_value(struct_val, val, i as u32, &format!("tuple_fld_{}", i))
                    .map_err(|e| e.to_string())?
                    .into_struct_value();
            }
            struct_val
        };

        Ok(Binding::Struct {
            ty: tuple_type,
            value: struct_val,
            storage: None,
        })
    }

    fn binding_to_bool_val(
        &self,
        binding: &Binding<'ctx>,
    ) -> Result<inkwell::values::IntValue<'ctx>, String> {
        match binding {
            Binding::Union { value, .. } => {
                let tag = self
                    .builder
                    .build_extract_value(*value, 0, "cond_tag")
                    .map_err(|e| e.to_string())?
                    .into_int_value();
                self.builder
                    .build_int_compare(
                        IntPredicate::EQ,
                        tag,
                        self.context.i8_type().const_int(1, false),
                        "is_true",
                    )
                    .map_err(|e| e.to_string())
            }
            Binding::Symbol { name, .. } => {
                if name == "true" {
                    Ok(self.context.bool_type().const_int(1, false))
                } else {
                    Ok(self.context.bool_type().const_int(0, false))
                }
            }
            _ => Err("condition must evaluate to Bool (:true | :false)".to_string()),
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
                    let cond_val = self.binding_to_bool_val(&cond_binding)?;

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

                    let end_of_arm = self.builder.get_insert_block().unwrap();
                    if end_of_arm.get_terminator().is_none() {
                        if result_type.is_none() {
                            result_type = Some(self.binding_type(&body_binding)?);
                        }
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

                    let end_of_arm = self.builder.get_insert_block().unwrap();
                    if end_of_arm.get_terminator().is_none() {
                        if result_type.is_none() {
                            result_type = Some(self.binding_type(&body_binding)?);
                        }
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

        let ty = result_type.unwrap_or_else(|| Type::Symbol("none".to_string()));
        if incoming.is_empty() {
            self.builder
                .build_unreachable()
                .map_err(|e| e.to_string())?;
            Ok(self.symbol_binding("none"))
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
        let resolved_target_type = self.resolve_alias_root(&target_type)?;

        let target_variants = match resolved_target_type {
            Type::Union(vars) => vars.clone(),
            _ => return Err("match target must be a union type".to_string()),
        };

        let union_llvm_type = self.llvm_type(resolved_target_type)?.into_struct_type();

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

            let variant_type = arm
                .variant_type
                .as_ref()
                .ok_or_else(|| "unresolved match arm type in codegen".to_string())?;

            let covered = match variant_type {
                Type::Union(inner) => inner.clone(),
                single => vec![single.clone()],
            };

            for v in &covered {
                let tag_idx = target_variants
                    .iter()
                    .position(|tv| {
                        Checker::types_compatible_with_aliases(v, tv, &self.type_aliases)
                            && Checker::types_compatible_with_aliases(tv, v, &self.type_aliases)
                    })
                    .ok_or_else(|| format!("variant {} not in union {}", v, target_type))?
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

            let variant_type = arm.variant_type.as_ref().unwrap();
            let resolved_arm_variant_type = self.resolve_alias_root(variant_type)?.clone();
            let arm_llvm_ty = self.llvm_type(variant_type)?;
            let payload_ptr = self
                .builder
                .build_struct_gep(union_llvm_type, union_storage, 1, "payload_ptr")
                .map_err(|e| e.to_string())?;

            if let Type::Union(inner_vars) = &resolved_arm_variant_type {
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
                    let target_idx = target_variants
                        .iter()
                        .position(|tv| {
                            Checker::types_compatible_with_aliases(iv, tv, &self.type_aliases)
                                && Checker::types_compatible_with_aliases(
                                    tv,
                                    iv,
                                    &self.type_aliases,
                                )
                        })
                        .ok_or_else(|| {
                            format!("sub-variant {} not in union {}", iv, target_type)
                        })? as u8;
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
                if sub_payload_size > 0 {
                    let copy_size = self.ptr_sized_int_type().const_int(sub_payload_size, false);
                    self.builder
                        .build_memcpy(sub_payload_ptr, 1, payload_ptr, 1, copy_size)
                        .map_err(|e| e.to_string())?;
                }

                self.local_scopes.last_mut().unwrap().insert(
                    binding_name.to_string(),
                    LocalVar {
                        ty: variant_type.clone(),
                        alloca: sub_alloca,
                    },
                );
            } else {
                let loaded_variant = if self.llvm_type_size(variant_type)? == 0 {
                    self.zero_value(variant_type)?
                } else {
                    self.builder
                        .build_load(arm_llvm_ty, payload_ptr, binding_name)
                        .map_err(|e| e.to_string())?
                };

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
                        ty: variant_type.clone(),
                        alloca,
                    },
                );
            }

            let arm_binding = self.codegen_expression(&arm.body)?;
            self.local_scopes.pop();

            let end_bb = self.builder.get_insert_block().unwrap();
            if end_bb.get_terminator().is_none() {
                if result_type.is_none() {
                    result_type = Some(self.binding_type(&arm_binding)?);
                }
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

        let ty = result_type.unwrap_or_else(|| Type::Symbol("none".to_string()));
        if incoming.is_empty() {
            self.builder
                .build_unreachable()
                .map_err(|e| e.to_string())?;
            Ok(self.symbol_binding("none"))
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
                let resolved_ty = self.resolve_alias_root(ty)?;
                let (idx, field_type) = match resolved_ty {
                    Type::Struct(fields) => {
                        fields
                            .iter()
                            .enumerate()
                            .find(|(_, f)| f.name == field)
                            .map(|(i, f)| (i, f.ty.clone()))
                            .ok_or_else(|| format!("unknown struct field '{}'", field))?
                    }
                    Type::Tuple(elements) => {
                        let idx: usize = field.parse().map_err(|_| format!("unknown tuple field '{}'", field))?;
                        if idx >= elements.len() {
                            return Err(format!("tuple index {} out of bounds", idx));
                        }
                        (idx, elements[idx].clone())
                    }
                    _ => return Err("expected struct or tuple type".to_string()),
                };

                let resolved_field_type = self.resolve_alias_root(&field_type)?;
                if matches!(resolved_field_type, Type::Struct(_) | Type::Tuple(_)) {
                    if let Some(storage_ptr) = storage {
                        let llvm_struct_ty = self.llvm_type(resolved_ty)?.into_struct_type();
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

                if let Type::Array { .. } = resolved_field_type {
                    if let Some(storage_ptr) = storage {
                        let llvm_struct_ty = self.llvm_type(resolved_ty)?.into_struct_type();
                        let field_gep = self
                            .builder
                            .build_struct_gep(llvm_struct_ty, storage_ptr, idx as u32, field)
                            .map_err(|e| e.to_string())?;

                        let loaded = self
                            .builder
                            .build_load(self.llvm_type(&field_type)?, field_gep, field)
                            .map_err(|e| e.to_string())?
                            .into_array_value();

                        return Ok(Binding::Array {
                            ty: field_type,
                            value: loaded,
                            storage: Some(field_gep),
                        });
                    }
                }



                if let Some(storage_ptr) = storage {
                    let llvm_struct_ty = self.llvm_type(resolved_ty)?.into_struct_type();
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
                            is_mut: false,
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
            other => {
                let ty = self.binding_type(&other)?;
                Err(format!(
                    "member access '.{}' unsupported on type {}",
                    field, ty
                ))
            }
        }
    }

    fn codegen_array_literal(&mut self, elements: &[Expression]) -> Result<Binding<'ctx>, String> {
        if elements.is_empty() {
            let llvm_elem_ty = self.context.i8_type();
            let llvm_arr_ty = llvm_elem_ty.array_type(0);
            let backing_alloca = self
                .builder
                .build_alloca(llvm_arr_ty, "arr_empty_backing")
                .map_err(|e| e.to_string())?;
            let loaded = self
                .builder
                .build_load(llvm_arr_ty, backing_alloca, "arr_empty_val")
                .map_err(|e| e.to_string())?
                .into_array_value();
            return Ok(Binding::Array {
                ty: Type::Array {
                    element: Box::new(Type::U8),
                    size: 0,
                },
                value: loaded,
                storage: Some(backing_alloca),
            });
        }

        let mut bindings = Vec::with_capacity(elements.len());
        for el in elements {
            bindings.push(self.codegen_expression(el)?);
        }

        let b0_ty = self.binding_type(&bindings[0])?;
        let element_type = self.resolve_alias_root(&b0_ty)?.clone();
        for b in &bindings[1..] {
            let b_ty = self.binding_type(b)?;
            let ty = self.resolve_alias_root(&b_ty)?;
            if ty != &element_type {
                return Err(format!(
                    "array literal elements have mismatched types: expected {}, found {}",
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

        let loaded = self
            .builder
            .build_load(llvm_arr_ty, backing_alloca, "arr_literal_val")
            .map_err(|e| e.to_string())?
            .into_array_value();

        Ok(Binding::Array {
            ty: Type::Array {
                element: Box::new(element_type),
                size: count as usize,
            },
            value: loaded,
            storage: Some(backing_alloca),
        })
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
                "binary operands have different types: {} and {}",
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
                "comparison operands have different types: {} and {}",
                left_type, right_type
            ));
        }

        if let (Binding::Symbol { name: n1, .. }, Binding::Symbol { name: n2, .. }) = (left, right) {
            let is_equal = n1 == n2;
            let result = match operator {
                ComparisonOperator::Equal => is_equal,
                ComparisonOperator::NotEqual => !is_equal,
                _ => return Err("invalid comparison operator for symbols".to_string()),
            };
            let cond = self
                .context
                .bool_type()
                .const_int(if result { 1 } else { 0 }, false);
            return self.bool_binding(cond);
        }

        if let (Binding::Union { value: v1, .. }, Binding::Union { value: v2, .. }) = (left, right) {
            let tag1 = self
                .builder
                .build_extract_value(*v1, 0, "tag1")
                .map_err(|e| e.to_string())?
                .into_int_value();
            let tag2 = self
                .builder
                .build_extract_value(*v2, 0, "tag2")
                .map_err(|e| e.to_string())?
                .into_int_value();
            let predicate = match operator {
                ComparisonOperator::Equal => IntPredicate::EQ,
                ComparisonOperator::NotEqual => IntPredicate::NE,
                _ => return Err("invalid comparison operator for unions".to_string()),
            };
            let value = self
                .builder
                .build_int_compare(predicate, tag1, tag2, "cmptmp")
                .map_err(|e| e.to_string())?;
            return self.bool_binding(value);
        }

        let (left_value, right_value) = (
            self.binding_int_value(left)?,
            self.binding_int_value(right)?,
        );

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

        self.bool_binding(value)
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
            if self.is_slice_type(&resolved_target)? {
                let storage = match binding {
                    Binding::Struct { storage, .. } => storage,
                    _ => None,
                };
                let val = self.binding_basic_value(&binding)?;
                return Ok(Binding::Struct {
                    ty: target_type,
                    value: val.into_struct_value(),
                    storage,
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
        if !Checker::is_supported_cast_with_aliases(&source_type, &target_type, &self.type_aliases)
        {
            return Err(format!(
                "unsupported cast from {} to {}",
                source_type, target_type
            ));
        }



        if (matches!(resolved_source, Type::Array { .. }) || matches!(resolved_source, Type::Tuple(_)))
            && self.is_slice_type(&resolved_target)?
        {
            let slice_binding = self.slice_binding(binding)?;
            return self.codegen_cast_inner(slice_binding, target_type, visited);
        }

        if matches!(resolved_source, Type::Symbol(_)) && self.is_atom_type(&resolved_target)? {
            let sym_name = match &binding {
                Binding::Symbol { name, .. } => name.clone(),
                _ => return Err("expected symbol binding for symbol to atom cast".to_string()),
            };
            let str_binding = self.codegen_string_literal(sym_name.as_bytes())?;
            let str_val = self.binding_basic_value(&str_binding)?;
            let atom_llvm_ty = self.llvm_type(&resolved_target)?.into_struct_type();
            let mut atom_val = atom_llvm_ty.get_undef();
            atom_val = self
                .builder
                .build_insert_value(atom_val, str_val, 0, "atom_str")
                .map_err(|e| e.to_string())?
                .into_struct_value();
            return Ok(Binding::Struct {
                ty: target_type,
                value: atom_val,
                storage: None,
            });
        }

        if let Type::Struct(fields) = &resolved_source {
            if self.is_record_type(&resolved_target)? {
                let record_binding = self.record_from_struct_binding(fields, binding)?;
                return self.codegen_cast_inner(record_binding, target_type, visited);
            }
        }

        if self.is_slice_type(&resolved_source)? && self.is_slice_type(&resolved_target)? {
            let source_element = self.slice_element_type(&resolved_source)?;
            let target_element = self.slice_element_type(&resolved_target)?;
            let storage = match binding {
                Binding::Struct { storage, .. } => storage,
                _ => None,
            };
            let source_value = self.binding_basic_value(&binding)?.into_struct_value();
            if source_element == Type::Opaque || target_element == Type::Opaque {
                return Ok(Binding::Struct {
                    ty: target_type,
                    value: source_value,
                    storage,
                });
            }
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
            let target_element_type = self.llvm_type(&target_element)?;
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
            let source_element_type = self.llvm_type(&source_element)?;
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
                self.binding_from_basic_value(&source_element, source_element_value)?;
            let converted = self.codegen_cast_inner(
                source_element_binding,
                target_element,
                visited,
            )?;
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

            return Ok(Binding::Struct {
                ty: target_type,
                value: slice_value,
                storage,
            });
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

        if let (Type::Tuple(source_elements), Type::Tuple(target_elements)) =
            (&resolved_source, &resolved_target)
        {
            let source_value = self.binding_basic_value(&binding)?.into_struct_value();
            let target_llvm_type = self.llvm_type(&target_type)?.into_struct_type();
            let mut target_value = target_llvm_type.get_undef();

            for (index, (source_elem, target_elem)) in
                source_elements.iter().zip(target_elements).enumerate()
            {
                let elem_value = self
                    .builder
                    .build_extract_value(source_value, index as u32, &format!("cast_tuple_elem_{}", index))
                    .map_err(|error| error.to_string())?;
                let elem_binding = self.binding_from_basic_value(source_elem, elem_value)?;
                let converted =
                    self.codegen_cast_inner(elem_binding, target_elem.clone(), visited)?;
                target_value = self
                    .builder
                    .build_insert_value(
                        target_value,
                        self.binding_basic_value(&converted)?,
                        index as u32,
                        &format!("cast_tuple_elem_{}", index),
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



        if matches!(
            (&resolved_source, &resolved_target),
            (
                Type::Pointer { .. } | Type::Function { .. },
                Type::Pointer { .. } | Type::Function { .. }
            )
        ) {
            let ptr_val = match binding {
                Binding::Pointer { value, .. } => value,
                Binding::Function { value, .. } => value.as_global_value().as_pointer_value(),
                _ => return Err("expected pointer or function binding".to_string()),
            };
            return Ok(Binding::Pointer {
                ty: target_type,
                value: ptr_val,
            });
        }

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
                            "cannot uniquely promote union variant {} into {}",
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
                        "cannot uniquely promote {} into union {}",
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
                if self.llvm_type_size(&target_vars[target_index])? > 0 {
                    self.builder
                        .build_store(payload_ptr, val)
                        .map_err(|e| e.to_string())?;
                }
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
                    "no variant of {} can be cast to {}",
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
            let payload_ptr = self
                .builder
                .build_struct_gep(union_llvm_ty, storage, 1, "payload_ptr")
                .map_err(|e| e.to_string())?;

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
            self.builder
                .build_unreachable()
                .map_err(|e| e.to_string())?;

            let mut incoming =
                Vec::<(BasicValueEnum<'ctx>, inkwell::basic_block::BasicBlock<'ctx>)>::new();
            for (index, (_, case_bb)) in candidates.iter().zip(cases.iter()) {
                let source_variant = &variants[*index];
                self.builder.position_at_end(*case_bb);
                let source_value = if self.llvm_type_size(source_variant)? == 0 {
                    self.zero_value(source_variant)?
                } else {
                    self.builder
                        .build_load(
                            self.llvm_type(source_variant)?,
                            payload_ptr,
                            "unboxed_value",
                        )
                        .map_err(|e| e.to_string())?
                };
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
            && matches!(
                resolved_target,
                Type::Pointer { .. } | Type::Function { .. }
            )
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

        if matches!(
            resolved_source,
            Type::Pointer { .. } | Type::Function { .. }
        ) && Checker::is_integer_type(&resolved_target)
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

        let target_llvm_type = self.llvm_type(&target_type)?.into_int_type();
        let source_bits = value.get_type().get_bit_width();
        let target_bits = target_llvm_type.get_bit_width();

        let casted = if target_bits == source_bits {
            value
        } else if self.builder.get_insert_block().is_none() {
            let val_u64 = value.get_zero_extended_constant().unwrap_or(0);
            target_llvm_type.const_int(val_u64, self.is_signed(&resolved_source))
        } else if target_bits > source_bits {
            if self.is_signed(&resolved_source) {
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
                            let is_def = self.specialized_def_names.contains(name);
                            let fn_ty = self.signature_to_function_type_with_def(Some(sig), is_def)?;
                            let pts = sig.parameters.iter().map(|p| p.ty.clone()).collect();
                            ((*sig.return_type).clone(), fn_ty, pts)
                        } else {
                            let ret = match func.get_type().get_return_type() {
                                Some(_) => Type::S32,
                                None => Type::Never,
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

        if let Expression::Identifier(name) = unspanned_expression(target) {
            if self.specialized_def_names.contains(name) {
                let ctx_ptr = self
                    .current_context_ptr
                    .ok_or_else(|| format!("missing @ctx when calling '{}'", name))?;

                let (line, file) = if let Some(span) = &self.current_span {
                    (span.line as u64, span.file.clone())
                } else {
                    (0u64, String::new())
                };

                let line_val = self.context.i64_type().const_int(line, false);
                let file_binding = self.codegen_string_literal(file.as_bytes())?;
                let file_val = self.binding_basic_value(&file_binding)?;

                argument_values.insert(0, ctx_ptr.into());
                argument_values.insert(1, line_val.into());
                argument_values.insert(2, file_val.into());
            }
        }

        let call = self
            .builder
            .build_indirect_call(fn_type, function_ptr, &argument_values, "calltmp")
            .map_err(|error| error.to_string())?;

        if ret_ty == Type::Never {
            self.builder
                .build_unreachable()
                .map_err(|e| e.to_string())?;
            Ok(self.symbol_binding("none"))
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

                let fn_type = if **return_type == Type::Never {
                    self.context.void_type().fn_type(&param_types, *is_variadic)
                } else {
                    self.llvm_type(return_type)?
                        .fn_type(&param_types, *is_variadic)
                };

                Ok((*(return_type.clone()), fn_type))
            }
            Type::Pointer { element, .. } => self.function_type_info(element),
            Type::Alias(_) => self.function_type_info(self.resolve_alias_root(ty)?),
            _ => Err(format!(
                "type {} is not a function or function pointer",
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

        let ty = self.binding_type(&binding)?;
        if self.is_slice_type(&ty)? {
            let element = self.slice_element_type(&ty)?;
            let slice_struct = self.binding_basic_value(&binding)?.into_struct_value();
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

            let resolved_elem = self.resolve_alias_root(&element)?;
            if let Type::Struct(_) = resolved_elem {
                return Ok(Binding::Struct {
                    ty: element,
                    value: loaded.into_struct_value(),
                    storage: Some(gep),
                });
            }

            return self.binding_from_basic_value(&element, loaded);
        }

        match ty {
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

                let resolved_elem = self.resolve_alias_root(&element)?;
                if let Type::Struct(_) = resolved_elem {
                    return Ok(Binding::Struct {
                        ty: (*element).clone(),
                        value: loaded.into_struct_value(),
                        storage: Some(pointer),
                    });
                }

                self.binding_from_basic_value(&element, loaded)
            }
            Type::Pointer { element, .. } => {
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

                let resolved_elem = self.resolve_alias_root(&element)?;
                if let Type::Struct(_) = resolved_elem {
                    return Ok(Binding::Struct {
                        ty: (*element).clone(),
                        value: loaded.into_struct_value(),
                        storage: Some(element_pointer),
                    });
                }

                self.binding_from_basic_value(&element, loaded)
            }
            other => Err(format!("cannot index value of type {}", other)),
        }
    }

    fn codegen_address_of(&mut self, expression: &Expression) -> Result<Binding<'ctx>, String> {
        match unspanned_expression(expression) {
            Expression::Context => {
                let ptr = self
                    .current_context_ptr
                    .ok_or_else(|| "missing @ctx".to_string())?;
                Ok(Binding::Pointer {
                    ty: Type::Pointer {
                        element: Box::new(crate::sema::context_type()),
                        is_mut: true,
                    },
                    value: ptr,
                })
            }
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
                            is_mut: false,
                        },
                        value: local.alloca,
                    });
                }

                if let Some(global_var) = self.global_vars.get(name) {
                    return Ok(Binding::Pointer {
                        ty: Type::Pointer {
                            element: Box::new(global_var.ty.clone()),
                            is_mut: false,
                        },
                        value: global_var.pointer,
                    });
                }

                if let Some(func) = self.module.get_function(name) {
                    let sig = if let Some(sig) = self.extern_functions.get(name) {
                        Some(sig.clone())
                    } else if let Some(sig) = self.function_signatures.get(name) {
                        Some(sig.clone())
                    } else {
                        None
                    };
                    let fn_ty = if let Some(sig) = &sig {
                        Type::Function {
                            parameters: sig.parameters.iter().map(|p| p.ty.clone()).collect(),
                            return_type: sig.return_type.clone(),
                            is_variadic: sig.is_variadic,
                        }
                    } else {
                        Type::Function {
                            parameters: Vec::new(),
                            return_type: Box::new(Type::Never),
                            is_variadic: false,
                        }
                    };
                    return Ok(Binding::Pointer {
                        ty: fn_ty,
                        value: func.as_global_value().as_pointer_value(),
                    });
                }

                Err(format!("unknown variable or function '{}'", name))
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

                let ty = self.binding_type(&binding)?;
                if self.is_slice_type(&ty)? {
                    let element = self.slice_element_type(&ty)?;
                    let slice_struct = self.binding_basic_value(&binding)?.into_struct_value();
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

                    return Ok(Binding::Pointer {
                        ty: Type::Pointer {
                            element: Box::new(element),
                            is_mut: false,
                        },
                        value: pointer,
                    });
                }

                match ty {
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
                            ty: Type::Pointer {
                                element,
                                is_mut: false,
                            },
                            value: pointer,
                        })
                    }
                    Type::Pointer { element, is_mut } => {
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
                            ty: Type::Pointer { element, is_mut },
                            value: element_pointer,
                        })
                    }
                    other => Err(format!("cannot take address of value of type {}", other)),
                }
            }
            Expression::Member { expression, field } => {
                let binding = self.codegen_expression(expression)?;
                match binding {
                    Binding::Struct { ref ty, storage, .. } => {
                        let resolved_ty = self.resolve_alias_root(ty)?;
                        let fields = match resolved_ty {
                            Type::Struct(flds) => flds,
                            _ => return Err("expected struct type".to_string()),
                        };
                        let (idx, field_type) = fields
                            .iter()
                            .enumerate()
                            .find(|(_, f)| f.name == *field)
                            .map(|(i, f)| (i, f.ty.clone()))
                            .ok_or_else(|| format!("unknown struct field '{}'", field))?;
                        if let Some(storage_ptr) = storage {
                            let llvm_struct_ty = self.llvm_type(resolved_ty)?.into_struct_type();
                            let field_gep = self
                                .builder
                                .build_struct_gep(llvm_struct_ty, storage_ptr, idx as u32, field)
                                .map_err(|e| e.to_string())?;
                            Ok(Binding::Pointer {
                                ty: Type::Pointer {
                                    element: Box::new(field_type),
                                    is_mut: false,
                                },
                                value: field_gep,
                            })
                        } else {
                            Err("cannot take address of field of a temporary struct without storage".to_string())
                        }
                    }

                    _ => Err("cannot take address of member of non-struct".to_string()),
                }
            }
            _ => Err("address-of requires an lvalue (variable, struct member, array index, or dereference)".to_string()),
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

        Ok(Binding::Struct {
            ty: Type::Alias("String".to_string()),
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
            Type::Never | Type::Inferred => Err("never type has no basic value".to_string()),
            Type::GenericInstance { name, .. } => {
                Err(format!("unconcretized generic type '{}' reached codegen", name))
            }
            Type::Alias(_) => self.binding_from_basic_value(self.resolve_alias_root(ty)?, value),
            Type::Symbol(name) => Ok(Binding::Symbol {
                name: name.clone(),
                value: value.into_struct_value(),
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
            Type::Struct { .. } | Type::Tuple { .. } => Ok(Binding::Struct {
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

            Type::Function { .. } => Ok(Binding::Pointer {
                ty: ty.clone(),
                value: value.into_pointer_value(),
            }),
            Type::Opaque => {
                Err("opaque type has no runtime representation".to_string())
            }
        }
    }

    fn binding_type(&self, binding: &Binding<'ctx>) -> Result<Type, String> {
        match binding {
            Binding::Symbol { name, .. } => Ok(Type::Symbol(name.clone())),
            Binding::Integer { ty, .. } => Ok(ty.clone()),
            Binding::Array { ty, .. } => Ok(ty.clone()),
            Binding::Struct { ty, .. } => Ok(ty.clone()),
            Binding::Union { ty, .. } => Ok(ty.clone()),
            Binding::Pointer { ty, .. } => Ok(ty.clone()),
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
            Binding::Symbol { value, .. } => Ok((*value).into()),
            Binding::Integer { value, .. } => Ok((*value).into()),
            Binding::Array { value, .. } => Ok((*value).into()),
            Binding::Struct { value, .. } => Ok((*value).into()),
            Binding::Union { value, .. } => Ok((*value).into()),
            Binding::Pointer { value, .. } => Ok((*value).into()),
            Binding::Function { value, .. } => {
                Ok(value.as_global_value().as_pointer_value().into())
            }
        }
    }

    fn binding_int_value(&self, binding: &Binding<'ctx>) -> Result<IntValue<'ctx>, String> {
        match binding {
            Binding::Integer { value, .. } => Ok(*value),
            _ => Err("expected integer or boolean value".to_string()),
        }
    }

    fn build_return(&self, binding: &Binding<'ctx>) -> Result<(), String> {
        if let Some(bb) = self.builder.get_insert_block() {
            if let Some(f) = bb.get_parent() {
                if f.get_name().to_str() == Ok("main") {
                    let return_val: BasicValueEnum<'ctx> = match binding {
                        Binding::Integer { value, .. } => (*value).into(),
                        _ => self.context.i32_type().const_zero().into(),
                    };
                    self.builder
                        .build_return(Some(&return_val))
                        .map_err(|error| error.to_string())?;
                    return Ok(());
                }
            }
        }
        let value = self.binding_basic_value(binding)?;
        self.builder
            .build_return(Some(&value))
            .map_err(|error| error.to_string())?;
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
        if max_size == 0 {
            return Ok(0);
        }
        let padding = (max_alignment - max_size % max_alignment) % max_alignment;
        Ok(max_size + padding)
    }

    fn union_payload_storage_type(&self, variants: &[Type]) -> Result<BasicTypeEnum<'ctx>, String> {
        let payload_size = self.union_payload_size(variants)?;
        if payload_size == 0 {
            return Ok(self.context.i8_type().array_type(0).into());
        }

        let mut max_alignment = 1;
        for variant in variants {
            let alignment = self.llvm_type_align(variant)?;
            max_alignment = max_alignment.max(alignment);
        }

        let (storage_ty, elem_size) = match max_alignment {
            align if align >= 16 => (
                self.context
                    .custom_width_int_type(std::num::NonZeroU32::new(128).unwrap())
                    .unwrap(),
                16,
            ),
            8 => (self.context.i64_type(), 8),
            4 => (self.context.i32_type(), 4),
            2 => (self.context.i16_type(), 2),
            _ => (self.context.i8_type(), 1),
        };

        let element_count = payload_size.div_ceil(elem_size);
        Ok(storage_ty.array_type(element_count as u32).into())
    }

    fn llvm_type_align(&self, ty: &Type) -> Result<u64, String> {
        match ty {
            Type::Symbol(_) => Ok(1),
            _ => {
                let llvm_type = self.llvm_type(ty)?;
                Ok(self.target_data.get_abi_alignment(&llvm_type) as u64)
            }
        }
    }

    fn llvm_type_size(&self, ty: &Type) -> Result<u64, String> {
        match ty {
            Type::Symbol(_) => Ok(0),
            Type::USize | Type::SSize | Type::Pointer { .. } | Type::Function { .. } => {
                Ok(self.ptr_byte_size())
            }
            _ => {
                let llvm_type = self.llvm_type(ty)?;
                Ok(self.target_data.get_abi_size(&llvm_type))
            }
        }
    }

    fn llvm_type(&self, ty: &Type) -> Result<BasicTypeEnum<'ctx>, String> {
        match ty {
            Type::Never => Err("never type is not a basic type in LLVM".to_string()),
            Type::Inferred => Err("inferred type reached codegen".to_string()),
            Type::GenericInstance { name, .. } => {
                Err(format!("unconcretized generic type '{}' reached codegen", name))
            }
            Type::Alias(_) => self.llvm_type(self.resolve_alias_root(ty)?),
            Type::Symbol(_) => Ok(self.context.struct_type(&[], false).into()),
            Type::S16 | Type::U16 => Ok(self.context.i16_type().into()),
            Type::S32 | Type::U32 => Ok(self.context.i32_type().into()),
            Type::S64 | Type::U64 => Ok(self.context.i64_type().into()),
            Type::USize | Type::SSize => Ok(self.ptr_sized_int_type().into()),
            Type::U8 => Ok(self.context.i8_type().into()),
            Type::Struct(fields) => {
                let field_llvm_types: Vec<BasicTypeEnum<'ctx>> = fields
                    .iter()
                    .map(|f| self.llvm_type(&f.ty))
                    .collect::<Result<Vec<_>, String>>()?;
                Ok(self.context.struct_type(&field_llvm_types, false).into())
            }
            Type::Tuple(elements) => {
                let elem_llvm_types: Vec<BasicTypeEnum<'ctx>> = elements
                    .iter()
                    .map(|elem| self.llvm_type(elem))
                    .collect::<Result<Vec<_>, String>>()?;
                Ok(self.context.struct_type(&elem_llvm_types, false).into())
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
            Type::Opaque => {
                Err("opaque type has no runtime representation".to_string())
            }
        }
    }

    fn zero_value(&self, ty: &Type) -> Result<BasicValueEnum<'ctx>, String> {
        match ty {
            Type::Never | Type::Inferred => Err("never has no zero value".to_string()),
            Type::GenericInstance { name, .. } => {
                Err(format!("unconcretized generic type '{}' reached codegen", name))
            }
            Type::Alias(_) => self.zero_value(self.resolve_alias_root(ty)?),
            Type::Symbol(_) => Ok(self.context.struct_type(&[], false).const_zero().into()),
            Type::S16 | Type::U16 => Ok(self.context.i16_type().const_zero().into()),
            Type::S32 | Type::U32 => Ok(self.context.i32_type().const_zero().into()),
            Type::S64 | Type::U64 => Ok(self.context.i64_type().const_zero().into()),
            Type::USize | Type::SSize => Ok(self.ptr_sized_int_type().const_zero().into()),
            Type::U8 => Ok(self.context.i8_type().const_zero().into()),
            Type::Struct(fields) => {
                let struct_llvm_type = self.llvm_type(ty)?.into_struct_type();
                let field_zeroes = fields
                    .iter()
                    .map(|f| self.zero_value(&f.ty))
                    .collect::<Result<Vec<_>, String>>()?;
                Ok(struct_llvm_type.const_named_struct(&field_zeroes).into())
            }
            Type::Tuple(elements) => {
                let struct_llvm_type = self.llvm_type(ty)?.into_struct_type();
                let elem_zeroes = elements
                    .iter()
                    .map(|elem| self.zero_value(elem))
                    .collect::<Result<Vec<_>, String>>()?;
                Ok(struct_llvm_type.const_named_struct(&elem_zeroes).into())
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
            Type::Opaque => {
                Err("opaque type has no zero value".to_string())
            }
        }
    }

    fn is_signed(&self, ty: &Type) -> bool {
        matches!(ty, Type::S16 | Type::S32 | Type::S64 | Type::SSize)
    }
}
