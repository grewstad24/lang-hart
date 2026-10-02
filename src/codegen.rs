use std::collections::HashMap;

use inkwell::builder::Builder;
use inkwell::context::Context;
use inkwell::module::Module;
use inkwell::types::{BasicMetadataTypeEnum, BasicType, BasicTypeEnum, FunctionType};
use inkwell::values::{ArrayValue, BasicValueEnum, FunctionValue, IntValue, PointerValue};
use inkwell::{AddressSpace, IntPredicate};

use crate::ast::{
    BinaryOperator, ComparisonOperator, Expression, ExternalFunction, Function, FunctionSignature,
    Main, Program, Statement, Type, UnaryOperator, Variable,
};
use crate::checker::Checker;

#[derive(Debug, Clone)]
enum Binding<'ctx> {
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
}

pub struct Codegen<'ctx> {
    context: &'ctx Context,
    module: Module<'ctx>,
    builder: Builder<'ctx>,

    variables: HashMap<String, Type>,
    locals: HashMap<String, Binding<'ctx>>,

    functions: HashMap<String, FunctionSignature>,
    extern_functions: HashMap<String, FunctionSignature>,

    string_counter: usize,
}

impl<'ctx> Codegen<'ctx> {
    pub fn new(context: &'ctx Context, module_name: &str) -> Self {
        Self {
            context,
            module: context.create_module(module_name),
            builder: context.create_builder(),

            variables: HashMap::new(),
            locals: HashMap::new(),

            functions: HashMap::new(),
            extern_functions: HashMap::new(),

            string_counter: 0,
        }
    }

    pub fn generate(mut self, program: &Program) -> Result<Module<'ctx>, String> {
        let mut checker = Checker::new(program);
        let program = checker.check(program)?;

        self.collect_declarations(&program)?;

        self.codegen_globals(&program.variables)?;
        self.codegen_externals(&program.extern_functions)?;
        self.codegen_functions(&program.functions)?;
        self.codegen_main(&program.main)?;

        Ok(self.module)
    }

    fn collect_declarations(&mut self, program: &Program) -> Result<(), String> {
        for variable in &program.variables {
            self.variables
                .insert(variable.name.clone(), Checker::concrete_type(&variable.ty)?);
        }

        for external in &program.extern_functions {
            self.extern_functions
                .insert(external.name.clone(), external.signature.clone());
        }

        for function in &program.functions {
            let signature = FunctionSignature {
                parameters: function
                    .parameters
                    .iter()
                    .map(|parameter| parameter.ty.clone())
                    .collect(),
                return_type: function.return_type.clone(),
            };

            self.functions.insert(function.name.clone(), signature);
        }

        Ok(())
    }

    fn codegen_globals(&mut self, variables: &[Variable]) -> Result<(), String> {
        for variable in variables {
            let ty = Checker::concrete_type(&variable.ty)?;
            let llvm_type = self.llvm_type(&ty)?;

            let global =
                self.module
                    .add_global(llvm_type, Some(AddressSpace::default()), &variable.name);

            global.set_initializer(&self.zero_value(&ty)?);
        }

        Ok(())
    }

    fn codegen_externals(&mut self, extern_functions: &[ExternalFunction]) -> Result<(), String> {
        for external in extern_functions {
            let function_type = self.function_type(&external.signature)?;

            self.module
                .add_function(&external.name, function_type, None);
        }

        Ok(())
    }

    fn codegen_functions(&mut self, functions: &[Function]) -> Result<(), String> {
        for function in functions {
            self.codegen_function(function)?;
        }

        Ok(())
    }

    fn codegen_function(&mut self, function: &Function) -> Result<(), String> {
        let signature = FunctionSignature {
            parameters: function
                .parameters
                .iter()
                .map(|parameter| parameter.ty.clone())
                .collect(),
            return_type: function.return_type.clone(),
        };

        let function_type = self.function_type(&signature)?;

        let llvm_function = self
            .module
            .add_function(&function.name, function_type, None);

        let entry = self.context.append_basic_block(llvm_function, "entry");

        self.builder.position_at_end(entry);
        self.locals.clear();

        for (index, parameter) in function.parameters.iter().enumerate() {
            let ty = Checker::concrete_type(&parameter.ty)?;

            let value = llvm_function
                .get_nth_param(index as u32)
                .ok_or_else(|| format!("missing function parameter {}", index))?;

            let binding = self.binding_from_basic_value(&ty, value)?;

            self.locals.insert(parameter.name.clone(), binding);
        }

        self.codegen_statements(&function.body)?;

        if !self.has_terminator(llvm_function) {
            let return_type = Checker::concrete_type(&function.return_type)?;
            let zero = self.zero_value(&return_type)?;

            self.builder
                .build_return(Some(&zero))
                .map_err(|error| error.to_string())?;
        }

        Ok(())
    }

    fn codegen_main(&mut self, main: &Main) -> Result<(), String> {
        let i32_type = self.context.i32_type();
        let function_type = i32_type.fn_type(&[], false);

        let function = self.module.add_function("main", function_type, None);

        let entry = self.context.append_basic_block(function, "entry");

        self.builder.position_at_end(entry);
        self.locals.clear();

        self.codegen_statements(&main.body)?;

        if !self.has_terminator(function) {
            self.builder
                .build_return(Some(&i32_type.const_zero()))
                .map_err(|error| error.to_string())?;
        }

        Ok(())
    }

    fn codegen_statements(&mut self, statements: &[Statement]) -> Result<(), String> {
        for statement in statements {
            self.codegen_statement(statement)?;
        }

        Ok(())
    }

    fn codegen_statement(&mut self, statement: &Statement) -> Result<(), String> {
        match statement {
            Statement::Bind { expression, names } => {
                let binding = self.codegen_expression(expression)?;

                if names.len() != 1 {
                    return Err(
                        "multiple bindings are not supported by this code generator".to_string()
                    );
                }

                let name = &names[0];

                if self.variables.contains_key(name) {
                    self.store_global(name, &binding)?;
                } else {
                    self.locals.insert(name.clone(), binding);
                }

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

            Statement::Unsafe(statements) => self.codegen_statements(statements),
        }
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
                if let Some(binding) = self.locals.get(name) {
                    let binding = binding.clone();
                    return self.clone_binding(&binding);
                }

                if let Some(binding) = self.global_binding(name) {
                    return Ok(binding);
                }

                Err(format!("unknown identifier '{}'", name))
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

            Expression::Call { name, arguments } => self.codegen_call(name, arguments),

            Expression::Index { expression, index } => self.codegen_index(expression, index),

            Expression::AddressOf(expression) => self.codegen_address_of(expression),
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
                    .map(|binding| self.binding_int_value(binding))
                    .collect::<Result<Vec<_>, _>>()?;

                self.llvm_type(&element_type)?
                    .into_int_type()
                    .const_array(&values)
            }

            Type::Array { .. } => {
                let values = bindings
                    .iter()
                    .map(|binding| match binding {
                        Binding::Array { value, .. } => Ok(*value),

                        _ => Err("expected array element".to_string()),
                    })
                    .collect::<Result<Vec<_>, String>>()?;

                self.llvm_type(&element_type)?
                    .into_array_type()
                    .const_array(&values)
            }

            Type::Pointer { .. } => {
                return Err("array literals of pointers are not supported".to_string());
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

        let value = self.binding_int_value(&binding)?;
        let target_llvm_type = self.llvm_type(&target_type)?.into_int_type();

        let source_bits = value.get_type().get_bit_width();
        let target_bits = target_llvm_type.get_bit_width();

        let casted = if target_bits == source_bits {
            value
        } else if target_bits > source_bits {
            if self.is_signed(&source_type) {
                self.builder
                    .build_int_s_extend(value, target_llvm_type, "sexttmp")
                    .map_err(|error| error.to_string())?
            } else {
                self.builder
                    .build_int_z_extend(value, target_llvm_type, "zexttmp")
                    .map_err(|error| error.to_string())?
            }
        } else {
            self.builder
                .build_int_truncate(value, target_llvm_type, "trunctmp")
                .map_err(|error| error.to_string())?
        };

        if target_type == Type::Bool {
            Ok(Binding::Boolean { value: casted })
        } else {
            Ok(Binding::Integer {
                ty: target_type,
                value: casted,
            })
        }
    }

    fn codegen_call(
        &mut self,
        name: &str,
        arguments: &[Expression],
    ) -> Result<Binding<'ctx>, String> {
        let function = self
            .module
            .get_function(name)
            .ok_or_else(|| format!("unknown function '{}'", name))?;

        let signature = if let Some(signature) = self.extern_functions.get(name) {
            signature.clone()
        } else if let Some(signature) = self.functions.get(name) {
            signature.clone()
        } else {
            return Err(format!("unknown function '{}'", name));
        };

        if arguments.len() != signature.parameters.len() {
            return Err(format!(
                "function '{}' expects {} arguments, found {}",
                name,
                signature.parameters.len(),
                arguments.len()
            ));
        }

        let mut argument_values = Vec::with_capacity(arguments.len());

        for argument in arguments {
            let binding = self.codegen_expression(argument)?;
            argument_values.push(self.binding_basic_value(&binding)?.into());
        }

        let call = self
            .builder
            .build_call(function, &argument_values, "calltmp")
            .map_err(|error| error.to_string())?;

        let return_type = Checker::concrete_type(&signature.return_type)?;

        let return_value = call
            .try_as_basic_value()
            .basic()
            .ok_or_else(|| "function call did not return a value".to_string())?;

        self.binding_from_basic_value(&return_type, return_value)
    }

    fn codegen_index(
        &mut self,
        expression: &Expression,
        index: &Expression,
    ) -> Result<Binding<'ctx>, String> {
        let binding = self.codegen_expression(expression)?;
        let index_binding = self.codegen_expression(index)?;

        let index_value = self.binding_int_value(&index_binding)?;

        let (element_type, size) = match self.binding_type(&binding)? {
            Type::Array { element, size } => (*element, size),

            other => {
                return Err(format!("cannot index value of type {:?}", other));
            }
        };

        if size == 0 {
            return Err("cannot index empty array".to_string());
        }

        let storage = self.array_storage(binding, &element_type, size)?;

        let llvm_array_type = self
            .llvm_type(&Type::Array {
                element: Box::new(element_type.clone()),
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
            .build_load(self.llvm_type(&element_type)?, pointer, "indextmp")
            .map_err(|error| error.to_string())?;

        self.binding_from_basic_value(&element_type, loaded)
    }

    fn codegen_address_of(&mut self, expression: &Expression) -> Result<Binding<'ctx>, String> {
        match expression {
            Expression::Identifier(name) => {
                let global = self
                    .module
                    .get_global(name)
                    .ok_or_else(|| format!("unknown global '{}'", name))?;

                let ty = self
                    .variables
                    .get(name)
                    .cloned()
                    .ok_or_else(|| format!("unknown variable '{}'", name))?;

                Ok(Binding::Pointer {
                    ty: Type::Pointer {
                        element: Box::new(ty),
                    },
                    value: global.as_pointer_value(),
                })
            }

            Expression::Index { expression, index } => {
                let binding = self.codegen_expression(expression)?;
                let index_binding = self.codegen_expression(index)?;

                let index_value = self.binding_int_value(&index_binding)?;

                let (element_type, size) = match self.binding_type(&binding)? {
                    Type::Array { element, size } => (*element, size),

                    other => {
                        return Err(format!("cannot take address of value of type {:?}", other));
                    }
                };

                if size == 0 {
                    return Err("cannot index empty array".to_string());
                }

                let storage = self.array_storage(binding, &element_type, size)?;

                let llvm_array_type = self
                    .llvm_type(&Type::Array {
                        element: Box::new(element_type.clone()),
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
                    ty: Type::Pointer {
                        element: Box::new(element_type),
                    },
                    value: pointer,
                })
            }

            _ => Err("address-of requires a variable or array index".to_string()),
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

    fn codegen_string_literal(&mut self, value: &[u8]) -> Result<Binding<'ctx>, String> {
        let element_type = self.context.i8_type();
        let array_type = element_type.array_type(value.len() as u32);

        let name = format!(".str.{}", self.string_counter);
        self.string_counter += 1;

        let global = self
            .module
            .add_global(array_type, Some(AddressSpace::default()), &name);

        let bytes = value
            .iter()
            .map(|byte| element_type.const_int(*byte as u64, false))
            .collect::<Vec<_>>();

        let initializer = element_type.const_array(&bytes);

        global.set_initializer(&initializer);
        global.set_constant(true);

        Ok(Binding::Array {
            ty: Type::Array {
                element: Box::new(Type::U8),
                size: value.len(),
            },
            value: initializer,
            storage: Some(global.as_pointer_value()),
        })
    }

    fn binding_from_basic_value(
        &self,
        ty: &Type,
        value: BasicValueEnum<'ctx>,
    ) -> Result<Binding<'ctx>, String> {
        match ty {
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
        }
    }

    fn clone_binding(&self, binding: &Binding<'ctx>) -> Result<Binding<'ctx>, String> {
        Ok(binding.clone())
    }

    fn binding_type(&self, binding: &Binding<'ctx>) -> Result<Type, String> {
        match binding {
            Binding::Integer { ty, .. } => Ok(ty.clone()),

            Binding::Boolean { .. } => Ok(Type::Bool),

            Binding::Array { ty, .. } => Ok(ty.clone()),

            Binding::Pointer { ty, .. } => Ok(ty.clone()),
        }
    }

    fn binding_basic_value(&self, binding: &Binding<'ctx>) -> Result<BasicValueEnum<'ctx>, String> {
        match binding {
            Binding::Integer { value, .. } => Ok((*value).into()),

            Binding::Boolean { value } => Ok((*value).into()),

            Binding::Array { value, .. } => Ok((*value).into()),

            Binding::Pointer { value, .. } => Ok((*value).into()),
        }
    }

    fn binding_int_value(&self, binding: &Binding<'ctx>) -> Result<IntValue<'ctx>, String> {
        match binding {
            Binding::Integer { value, .. } => Ok(*value),

            Binding::Boolean { value } => Ok(*value),

            _ => Err("expected integer or boolean value".to_string()),
        }
    }

    fn global_binding(&self, name: &str) -> Option<Binding<'ctx>> {
        let ty = self.variables.get(name)?;
        let global = self.module.get_global(name)?;

        let loaded = self
            .builder
            .build_load(
                self.llvm_type(ty).ok()?,
                global.as_pointer_value(),
                "globaltmp",
            )
            .ok()?;

        match ty {
            Type::Array { .. } => Some(Binding::Array {
                ty: ty.clone(),
                value: loaded.into_array_value(),
                storage: Some(global.as_pointer_value()),
            }),

            Type::Bool => Some(Binding::Boolean {
                value: loaded.into_int_value(),
            }),

            Type::S16 | Type::S32 | Type::S64 | Type::U8 | Type::U16 | Type::U32 | Type::U64 => {
                Some(Binding::Integer {
                    ty: ty.clone(),
                    value: loaded.into_int_value(),
                })
            }

            Type::Pointer { .. } => Some(Binding::Pointer {
                ty: ty.clone(),
                value: loaded.into_pointer_value(),
            }),
        }
    }

    fn store_global(&self, name: &str, binding: &Binding<'ctx>) -> Result<(), String> {
        let global = self
            .module
            .get_global(name)
            .ok_or_else(|| format!("missing global '{}'", name))?;

        let value = self.binding_basic_value(binding)?;

        self.builder
            .build_store(global.as_pointer_value(), value)
            .map_err(|error| error.to_string())?;

        Ok(())
    }

    fn build_return(&self, binding: &Binding<'ctx>) -> Result<(), String> {
        let value = self.binding_basic_value(binding)?;

        self.builder
            .build_return(Some(&value))
            .map_err(|error| error.to_string())?;

        Ok(())
    }

    fn function_type(&self, signature: &FunctionSignature) -> Result<FunctionType<'ctx>, String> {
        let parameters = signature
            .parameters
            .iter()
            .map(Checker::concrete_type)
            .collect::<Result<Vec<_>, _>>()?;

        let parameter_types = parameters
            .iter()
            .map(|ty| self.llvm_type(ty).map(BasicMetadataTypeEnum::from))
            .collect::<Result<Vec<_>, _>>()?;

        let return_type = Checker::concrete_type(&signature.return_type)?;

        Ok(self
            .llvm_type(&return_type)?
            .fn_type(&parameter_types, false))
    }

    fn llvm_type(&self, ty: &Type) -> Result<BasicTypeEnum<'ctx>, String> {
        match ty {
            Type::S16 | Type::U16 => Ok(self.context.i16_type().into()),

            Type::S32 | Type::U32 => Ok(self.context.i32_type().into()),

            Type::S64 | Type::U64 => Ok(self.context.i64_type().into()),

            Type::U8 => Ok(self.context.i8_type().into()),

            Type::Bool => Ok(self.context.bool_type().into()),

            Type::Array { element, size } => {
                Ok(self.llvm_type(element)?.array_type(*size as u32).into())
            }

            Type::Pointer { .. } => Ok(self.context.ptr_type(AddressSpace::default()).into()),
        }
    }

    fn zero_value(&self, ty: &Type) -> Result<BasicValueEnum<'ctx>, String> {
        match ty {
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

            Type::Pointer { .. } => Ok(self
                .context
                .ptr_type(AddressSpace::default())
                .const_null()
                .into()),
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
