use std::collections::{HashMap, HashSet};

use crate::sema::ModuleScope;
use crate::syntax::{
    Definition, Expression, LogicalModule, StructFieldInit, StructFieldType, Type, TypeAlias,
};

pub fn mangle_type(ty: &Type) -> String {
    match ty {
        Type::Never => "never".to_string(),
        Type::Inferred => "inferred".to_string(),
        Type::Opaque => "opaque".to_string(),
        Type::U8 => "u8".to_string(),
        Type::U16 => "u16".to_string(),
        Type::U32 => "u32".to_string(),
        Type::U64 => "u64".to_string(),
        Type::USize => "usize".to_string(),
        Type::S16 => "s16".to_string(),
        Type::S32 => "s32".to_string(),
        Type::S64 => "s64".to_string(),
        Type::SSize => "ssize".to_string(),
        Type::Symbol(name) => format!("symbol_{}", name),
        Type::Alias(name) => name.clone(),
        Type::GenericInstance {
            name,
            type_arguments,
        } => {
            let inner = type_arguments
                .iter()
                .map(mangle_type)
                .collect::<Vec<_>>()
                .join("__");
            format!("{}__{}", name, inner)
        }
        Type::Pointer { element, is_mut } => {
            if *is_mut {
                format!("ptr_mut_{}", mangle_type(element))
            } else {
                format!("ptr_{}", mangle_type(element))
            }
        }
        Type::Array { element, size } => format!("arr_{}_{}", size, mangle_type(element)),
        Type::Struct(fields) => {
            let inner = fields
                .iter()
                .map(|f| format!("{}_{}", f.name, mangle_type(&f.ty)))
                .collect::<Vec<_>>()
                .join("_");
            format!("struct_{}", inner)
        }
        Type::Tuple(elements) => {
            let inner = elements
                .iter()
                .map(mangle_type)
                .collect::<Vec<_>>()
                .join("_");
            format!("tuple_{}", inner)
        }
        Type::Union(variants) => {
            let inner = variants
                .iter()
                .map(mangle_type)
                .collect::<Vec<_>>()
                .join("_or_");
            format!("union_{}", inner)
        }
        Type::Function {
            parameters,
            return_type,
            ..
        } => {
            let params = parameters
                .iter()
                .map(mangle_type)
                .collect::<Vec<_>>()
                .join("_");
            format!("fn_{}_ret_{}", params, mangle_type(return_type))
        }
    }
}

pub fn sanitize_identifier(s: &str) -> String {
    s.chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '_' {
                c
            } else {
                '_'
            }
        })
        .collect()
}

pub fn substitute_type(ty: &Type, substitutions: &HashMap<String, Type>) -> Type {
    match ty {
        Type::Alias(name) => {
            if let Some(concrete) = substitutions.get(name) {
                concrete.clone()
            } else {
                Type::Alias(name.clone())
            }
        }
        Type::GenericInstance {
            name,
            type_arguments,
        } => {
            let new_args = type_arguments
                .iter()
                .map(|arg| substitute_type(arg, substitutions))
                .collect();
            Type::GenericInstance {
                name: name.clone(),
                type_arguments: new_args,
            }
        }
        Type::Pointer { element, is_mut } => Type::Pointer {
            element: Box::new(substitute_type(element, substitutions)),
            is_mut: *is_mut,
        },
        Type::Array { element, size } => Type::Array {
            element: Box::new(substitute_type(element, substitutions)),
            size: *size,
        },
        Type::Struct(fields) => {
            let new_fields = fields
                .iter()
                .map(|f| StructFieldType {
                    name: f.name.clone(),
                    ty: substitute_type(&f.ty, substitutions),
                })
                .collect();
            Type::Struct(new_fields)
        }
        Type::Tuple(elements) => {
            let new_elements = elements
                .iter()
                .map(|e| substitute_type(e, substitutions))
                .collect();
            Type::Tuple(new_elements)
        }
        Type::Union(variants) => {
            let new_variants = variants
                .iter()
                .map(|v| substitute_type(v, substitutions))
                .collect();
            Type::Union(new_variants)
        }
        Type::Function {
            parameters,
            return_type,
            is_variadic,
        } => {
            let new_params = parameters
                .iter()
                .map(|p| substitute_type(p, substitutions))
                .collect();
            let new_ret = substitute_type(return_type, substitutions);
            Type::Function {
                parameters: new_params,
                return_type: Box::new(new_ret),
                is_variadic: *is_variadic,
            }
        }
        other => other.clone(),
    }
}

pub fn substitute_expression(expr: &mut Expression, substitutions: &HashMap<String, Type>) {
    match expr {
        Expression::Spanned { expression, .. } => {
            substitute_expression(expression, substitutions);
        }
        Expression::SizeOf(inner) => {
            substitute_expression(inner, substitutions);
        }
        Expression::AsTypeOf { expression, target } => {
            substitute_expression(expression, substitutions);
            substitute_expression(target, substitutions);
        }
        Expression::Slice(expr) => {
            substitute_expression(expr, substitutions);
        }
        Expression::Cast { expression, ty } => {
            substitute_expression(expression, substitutions);
            *ty = substitute_type(ty, substitutions);
        }
        Expression::Var { ty, value, .. } => {
            if let Some(t) = ty {
                *t = substitute_type(t, substitutions);
            }
            if let Some(v) = value {
                substitute_expression(v, substitutions);
            }
        }
        Expression::Let { value, .. } => {
            substitute_expression(value, substitutions);
        }
        Expression::Assign { target, value } => {
            substitute_expression(target, substitutions);
            substitute_expression(value, substitutions);
        }
        Expression::Binary { left, right, .. } => {
            substitute_expression(left, substitutions);
            substitute_expression(right, substitutions);
        }
        Expression::Comparison { left, right, .. } => {
            substitute_expression(left, substitutions);
            substitute_expression(right, substitutions);
        }
        Expression::Call { target, arguments } => {
            substitute_expression(target, substitutions);
            for arg in arguments {
                substitute_expression(arg, substitutions);
            }
        }
        Expression::Index { expression, index } => {
            substitute_expression(expression, substitutions);
            substitute_expression(index, substitutions);
        }
        Expression::Member { expression, .. } => {
            substitute_expression(expression, substitutions);
        }
        Expression::AddressOf { expression, .. } => {
            substitute_expression(expression, substitutions);
        }
        Expression::Dereference(operand) => {
            substitute_expression(operand, substitutions);
        }
        Expression::Block(exprs) => {
            for e in exprs {
                substitute_expression(e, substitutions);
            }
        }
        Expression::Defer(operand) => {
            substitute_expression(operand, substitutions);
        }
        Expression::Discard(operand) => {
            substitute_expression(operand, substitutions);
        }
        Expression::Return(operand) => {
            substitute_expression(operand, substitutions);
        }
        Expression::Yield(operand) => {
            substitute_expression(operand, substitutions);
        }
        Expression::Repeat { count, body, .. } => {
            substitute_expression(count, substitutions);
            substitute_expression(body, substitutions);
        }
        Expression::RepeatWhile { condition, body } => {
            substitute_expression(condition, substitutions);
            substitute_expression(body, substitutions);
        }
        Expression::RepeatUntil { condition, body } => {
            substitute_expression(condition, substitutions);
            substitute_expression(body, substitutions);
        }
        Expression::For { iterable, body, .. } => {
            substitute_expression(iterable, substitutions);
            substitute_expression(body, substitutions);
        }
        Expression::If { arms } => {
            for arm in arms {
                if let crate::syntax::IfCondition::Expression(e) = &mut arm.condition {
                    substitute_expression(e, substitutions);
                }
                substitute_expression(&mut arm.body, substitutions);
            }
        }
        Expression::Match { target, arms, .. } => {
            substitute_expression(target, substitutions);
            for arm in arms {
                if let Some(vt) = &mut arm.variant_type {
                    *vt = substitute_type(vt, substitutions);
                }
                substitute_expression(&mut arm.body, substitutions);
            }
        }
        Expression::Array(elements) => {
            for el in elements {
                substitute_expression(el, substitutions);
            }
        }
        Expression::Struct(fields) => {
            for f in fields {
                substitute_expression(&mut f.value, substitutions);
            }
        }
        Expression::Tuple(elements) => {
            for el in elements {
                substitute_expression(el, substitutions);
            }
        }
        Expression::Spread(inner) => {
            substitute_expression(inner, substitutions);
        }
        Expression::Identifier(_)
        | Expression::Integer(_)
        | Expression::String(_)
        | Expression::Symbol(_)
        | Expression::Break
        | Expression::Continue
        | Expression::Context
        | Expression::CallerLineNumber
        | Expression::CallerFileName
        | Expression::ThisLineNumber(_)
        | Expression::ThisFileName(_) => {}
    }
}

fn unwrap_identifier_name(expr: &Expression) -> Option<String> {
    match expr {
        Expression::Spanned { expression, .. } => unwrap_identifier_name(expression),
        Expression::Identifier(name) => Some(name.clone()),
        _ => None,
    }
}

fn rewrite_target_identifier(expr: &mut Expression, new_name: String) {
    match expr {
        Expression::Spanned { expression, .. } => {
            rewrite_target_identifier(expression, new_name);
        }
        Expression::Identifier(name) => {
            *name = new_name;
        }
        _ => {}
    }
}

pub(crate) struct GeneratedSpecialization {
    base_name: String,
    originating_file: String,
    specialized_def: Definition,
}

fn receiver_has_field(
    receiver: &Expression,
    field: &str,
    local_types: &HashMap<String, Type>,
    scope: &ModuleScope,
    generated: &[GeneratedSpecialization],
) -> bool {
    let unspanned = crate::syntax::unspanned_expression(receiver);
    if let Expression::Member { .. } = unspanned {
        if let Some(ty) = infer_expr_type_with_generated(receiver, local_types, scope, generated) {
            let resolved = resolve_type_alias_deep(&ty, scope);
            return match resolved {
                Type::Struct(fields) => fields.iter().any(|f| f.name == field),
                Type::Pointer { element, .. } => {
                    let elem_res = resolve_type_alias_deep(&element, scope);
                    if let Type::Struct(fields) = elem_res {
                        fields.iter().any(|f| f.name == field)
                    } else {
                        false
                    }
                }
                _ => false,
            };
        }
    }
    if let Some(ty) = infer_expr_type_with_generated(receiver, local_types, scope, generated) {
        let resolved = resolve_type_alias_deep(&ty, scope);
        match resolved {
            Type::Struct(fields) => fields.iter().any(|f| f.name == field),
            Type::Pointer { element, .. } => {
                let elem_res = resolve_type_alias_deep(&element, scope);
                if let Type::Struct(fields) = elem_res {
                    fields.iter().any(|f| f.name == field)
                } else {
                    false
                }
            }
            Type::Tuple(elements) => {
                if let Ok(idx) = field.parse::<usize>() {
                    idx < elements.len()
                } else {
                    false
                }
            }
            _ => false,
        }
    } else {
        false
    }
}

fn is_function_in_scope<V>(
    name: &str,
    scope: &ModuleScope,
    def_templates: &HashMap<String, V>,
) -> bool {
    crate::syntax::is_intrinsic_function(name)
        || scope.definitions.contains_key(name)
        || scope.lookup_extern(name).is_some()
        || def_templates.contains_key(name)
}

fn scan_and_rewrite_expression(
    expr: &mut Expression,
    scope: &ModuleScope,
    generated: &mut Vec<GeneratedSpecialization>,
    changed: &mut bool,
) -> Result<(), String> {
    match expr {
        Expression::Spanned { expression, .. } => {
            scan_and_rewrite_expression(expression, scope, generated, changed)?;
        }
        Expression::Call { target, arguments } => {
            if let Expression::Member { expression: receiver, field } = target.as_mut() {
                if is_function_in_scope(field, scope, &HashMap::<String, ()>::new())
                    && !receiver_has_field(receiver, field, &HashMap::new(), scope, generated)
                {
                    let field_name = field.clone();
                    let recv = *receiver.clone();
                    let mut new_args = vec![recv];
                    new_args.append(arguments);
                    *expr = Expression::Call {
                        target: Box::new(Expression::Identifier(field_name)),
                        arguments: new_args,
                    };
                    *changed = true;
                    return scan_and_rewrite_expression(expr, scope, generated, changed);
                }
            }

            // First rewrite inner arguments and target
            for arg in arguments.iter_mut() {
                scan_and_rewrite_expression(arg, scope, generated, changed)?;
            }
            scan_and_rewrite_expression(target, scope, generated, changed)?;

            if let Expression::Identifier(name) = crate::syntax::unspanned_expression(target) {
                if name == "@asTypeOf" && arguments.len() == 2 {
                    let target_expr = arguments.pop().unwrap();
                    let src_expr = arguments.pop().unwrap();
                    *expr = Expression::AsTypeOf {
                        expression: Box::new(src_expr),
                        target: Box::new(target_expr),
                    };
                    *changed = true;
                    return scan_and_rewrite_expression(expr, scope, generated, changed);
                }
                if name == "@sizeOf" && arguments.len() == 1 {
                    let src_expr = arguments.pop().unwrap();
                    *expr = Expression::SizeOf(Box::new(src_expr));
                    *changed = true;
                    return scan_and_rewrite_expression(expr, scope, generated, changed);
                }
            }
        }
        Expression::Index { expression, index } => {
            scan_and_rewrite_expression(expression, scope, generated, changed)?;
            scan_and_rewrite_expression(index, scope, generated, changed)?;
        }
        Expression::Member { expression, .. } => {
            scan_and_rewrite_expression(expression, scope, generated, changed)?;
        }
        Expression::Binary { left, right, .. } => {
            scan_and_rewrite_expression(left, scope, generated, changed)?;
            scan_and_rewrite_expression(right, scope, generated, changed)?;
        }
        Expression::Comparison { left, right, .. } => {
            scan_and_rewrite_expression(left, scope, generated, changed)?;
            scan_and_rewrite_expression(right, scope, generated, changed)?;
        }
        Expression::Cast { expression, .. } => {
            scan_and_rewrite_expression(expression, scope, generated, changed)?;
        }
        Expression::Slice(operand) => {
            scan_and_rewrite_expression(operand, scope, generated, changed)?;
        }
        Expression::AddressOf { expression, .. } => {
            scan_and_rewrite_expression(expression, scope, generated, changed)?;
        }
        Expression::Dereference(operand) => {
            scan_and_rewrite_expression(operand, scope, generated, changed)?;
        }
        Expression::Block(exprs) => {
            for e in exprs {
                scan_and_rewrite_expression(e, scope, generated, changed)?;
            }
        }
        Expression::Defer(operand) => {
            scan_and_rewrite_expression(operand, scope, generated, changed)?;
        }
        Expression::Discard(operand) => {
            scan_and_rewrite_expression(operand, scope, generated, changed)?;
        }
        Expression::Return(operand) => {
            scan_and_rewrite_expression(operand, scope, generated, changed)?;
        }
        Expression::Yield(operand) => {
            scan_and_rewrite_expression(operand, scope, generated, changed)?;
        }
        Expression::Let { value, .. } => {
            scan_and_rewrite_expression(value, scope, generated, changed)?;
        }
        Expression::Var { value, .. } => {
            if let Some(v) = value {
                scan_and_rewrite_expression(v, scope, generated, changed)?;
            }
        }
        Expression::Assign { target, value } => {
            scan_and_rewrite_expression(target, scope, generated, changed)?;
            scan_and_rewrite_expression(value, scope, generated, changed)?;
        }
        Expression::Repeat { count, body, .. } => {
            scan_and_rewrite_expression(count, scope, generated, changed)?;
            scan_and_rewrite_expression(body, scope, generated, changed)?;
        }
        Expression::RepeatWhile { condition, body } => {
            scan_and_rewrite_expression(condition, scope, generated, changed)?;
            scan_and_rewrite_expression(body, scope, generated, changed)?;
        }
        Expression::RepeatUntil { condition, body } => {
            scan_and_rewrite_expression(condition, scope, generated, changed)?;
            scan_and_rewrite_expression(body, scope, generated, changed)?;
        }
        Expression::For { iterable, body, .. } => {
            scan_and_rewrite_expression(iterable, scope, generated, changed)?;
            scan_and_rewrite_expression(body, scope, generated, changed)?;
        }
        Expression::If { arms } => {
            for arm in arms {
                if let crate::syntax::IfCondition::Expression(e) = &mut arm.condition {
                    scan_and_rewrite_expression(e, scope, generated, changed)?;
                }
                scan_and_rewrite_expression(&mut arm.body, scope, generated, changed)?;
            }
        }
        Expression::Match { target, arms, .. } => {
            scan_and_rewrite_expression(target, scope, generated, changed)?;
            for arm in arms {
                scan_and_rewrite_expression(&mut arm.body, scope, generated, changed)?;
            }
        }
        Expression::Array(elements) => {
            for el in elements {
                scan_and_rewrite_expression(el, scope, generated, changed)?;
            }
        }
        Expression::Struct(fields) => {
            for f in fields {
                scan_and_rewrite_expression(&mut f.value, scope, generated, changed)?;
            }
        }
        Expression::Tuple(elements) => {
            for el in elements {
                scan_and_rewrite_expression(el, scope, generated, changed)?;
            }
        }
        Expression::SizeOf(inner) => {
            scan_and_rewrite_expression(inner, scope, generated, changed)?;
        }
        Expression::AsTypeOf { expression, target } => {
            scan_and_rewrite_expression(expression, scope, generated, changed)?;
            scan_and_rewrite_expression(target, scope, generated, changed)?;
        }
        Expression::Spread(inner) => {
            scan_and_rewrite_expression(inner, scope, generated, changed)?;
        }
        Expression::Identifier(_)
        | Expression::Integer(_)
        | Expression::String(_)
        | Expression::Symbol(_)
        | Expression::Break
        | Expression::Continue
        | Expression::Context
        | Expression::CallerLineNumber
        | Expression::CallerFileName
        | Expression::ThisLineNumber(_)
        | Expression::ThisFileName(_) => {}
    }
    Ok(())
}

pub fn monomorphize_modules(
    modules: &mut [LogicalModule],
    module_scopes: &mut [ModuleScope],
) -> Result<(), String> {
    let mut changed = true;
    let mut iterations = 0;

    while changed {
        changed = false;
        iterations += 1;
        if iterations > 128 {
            return Err(
                "monomorphization depth limit exceeded (possible cyclic function recursion)"
                    .to_string(),
            );
        }

        let mut all_generated = Vec::new();

        for (module, scope) in modules.iter_mut().zip(module_scopes.iter()) {
            for file in &mut module.source_files {
                for def in &mut file.definitions {
                    if def.is_def {
                        continue;
                    }
                    scan_and_rewrite_expression(
                        &mut def.value,
                        scope,
                        &mut all_generated,
                        &mut changed,
                    )?;
                }
            }
        }

        for item in all_generated {
            let spec_name = item.specialized_def.name.clone();

            // Insert into the originating source file if not already present
            let mut inserted_file = false;
            for module in modules.iter_mut() {
                for file in &mut module.source_files {
                    if file.file_path == item.originating_file {
                        if !file.definitions.iter().any(|d| d.name == spec_name) {
                            file.definitions.push(item.specialized_def.clone());
                        }
                        inserted_file = true;
                        break;
                    }
                }
                if inserted_file {
                    break;
                }
            }

            // Insert provenance into every module scope that knows about the base function
            for scope in module_scopes.iter_mut() {
                if scope.definitions.contains_key(&item.base_name)
                    && !scope.definitions.contains_key(&spec_name)
                {
                    scope.definitions.insert(
                        spec_name.clone(),
                        crate::sema::SymbolProvenance {
                            item: item.specialized_def.clone(),
                            originating_file: item.originating_file.clone(),
                        },
                    );
                }
            }
        }

        for (module, scope) in modules.iter().zip(module_scopes.iter_mut()) {
            for file in &module.source_files {
                for def in &file.definitions {
                    if let Some(prov) = scope.definitions.get_mut(&def.name) {
                        prov.item = def.clone();
                    }
                }
            }
        }
    }

    Ok(())
}

fn concretize_type(
    ty: &mut Type,
    templates: &HashMap<String, (String, TypeAlias)>,
    generated: &mut Vec<(String, TypeAlias)>,
    changed: &mut bool,
) -> Result<(), String> {
    match ty {
        Type::Pointer { element, .. } => concretize_type(element, templates, generated, changed)?,
        Type::Array { element, .. } => concretize_type(element, templates, generated, changed)?,
        Type::Struct(fields) => {
            for f in fields {
                concretize_type(&mut f.ty, templates, generated, changed)?;
            }
        }
        Type::Tuple(elements) => {
            for e in elements {
                concretize_type(e, templates, generated, changed)?;
            }
        }
        Type::Union(variants) => {
            for v in variants {
                concretize_type(v, templates, generated, changed)?;
            }
        }
        Type::Function {
            parameters,
            return_type,
            ..
        } => {
            for p in parameters {
                concretize_type(p, templates, generated, changed)?;
            }
            concretize_type(return_type, templates, generated, changed)?;
        }
        Type::GenericInstance {
            name,
            type_arguments,
        } => {
            for arg in type_arguments.iter_mut() {
                concretize_type(arg, templates, generated, changed)?;
            }

            let (originating_file, template) = templates
                .get(name)
                .cloned()
                .ok_or_else(|| format!("unknown meta type '{}'", name))?;

            if type_arguments.len() != template.type_parameters.len() {
                return Err(format!(
                    "meta type '{}' expects {} type arguments, found {}",
                    name,
                    template.type_parameters.len(),
                    type_arguments.len()
                ));
            }

            let mut substitutions = HashMap::new();
            for (param_name, arg_ty) in template.type_parameters.iter().zip(type_arguments.iter()) {
                substitutions.insert(param_name.clone(), arg_ty.clone());
            }

            let mut concrete_target = substitute_type(&template.target, &substitutions);
            concretize_type(&mut concrete_target, templates, generated, changed)?;

            let mangled_suffix = type_arguments
                .iter()
                .map(mangle_type)
                .collect::<Vec<_>>()
                .join("__");
            let concrete_name = sanitize_identifier(&format!("{}__{}", name, mangled_suffix));

            let concrete_alias = TypeAlias {
                name: concrete_name.clone(),
                target: concrete_target,
                span: template.span.clone(),
                is_meta: false,
                type_parameters: Vec::new(),
            };

            generated.push((originating_file, concrete_alias));

            *ty = Type::Alias(concrete_name);
            *changed = true;
        }
        _ => {}
    }
    Ok(())
}

fn concretize_expression(
    expr: &mut Expression,
    templates: &HashMap<String, (String, TypeAlias)>,
    generated: &mut Vec<(String, TypeAlias)>,
    changed: &mut bool,
) -> Result<(), String> {
    match expr {
        Expression::Spanned { expression, .. } => {
            concretize_expression(expression, templates, generated, changed)?;
        }
        Expression::SizeOf(operand) => {
            concretize_expression(operand, templates, generated, changed)?;
        }
        Expression::AsTypeOf { expression, target } => {
            concretize_expression(expression, templates, generated, changed)?;
            concretize_expression(target, templates, generated, changed)?;
        }
        Expression::Slice(operand) => {
            concretize_expression(operand, templates, generated, changed)?;
        }
        Expression::Cast { expression, ty } => {
            concretize_expression(expression, templates, generated, changed)?;
            concretize_type(ty, templates, generated, changed)?;
        }
        Expression::Var { ty, value, .. } => {
            if let Some(t) = ty {
                concretize_type(t, templates, generated, changed)?;
            }
            if let Some(v) = value {
                concretize_expression(v, templates, generated, changed)?;
            }
        }
        Expression::Let { value, .. } => {
            concretize_expression(value, templates, generated, changed)?;
        }
        Expression::Assign { target, value } => {
            concretize_expression(target, templates, generated, changed)?;
            concretize_expression(value, templates, generated, changed)?;
        }
        Expression::Binary { left, right, .. } => {
            concretize_expression(left, templates, generated, changed)?;
            concretize_expression(right, templates, generated, changed)?;
        }
        Expression::Comparison { left, right, .. } => {
            concretize_expression(left, templates, generated, changed)?;
            concretize_expression(right, templates, generated, changed)?;
        }
        Expression::Call { target, arguments } => {
            concretize_expression(target, templates, generated, changed)?;
            for arg in arguments {
                concretize_expression(arg, templates, generated, changed)?;
            }
        }
        Expression::Index { expression, index } => {
            concretize_expression(expression, templates, generated, changed)?;
            concretize_expression(index, templates, generated, changed)?;
        }
        Expression::Member { expression, .. } => {
            concretize_expression(expression, templates, generated, changed)?;
        }
        Expression::AddressOf { expression, .. } => {
            concretize_expression(expression, templates, generated, changed)?;
        }
        Expression::Dereference(e) => {
            concretize_expression(e, templates, generated, changed)?;
        }
        Expression::Block(exprs) => {
            for e in exprs {
                concretize_expression(e, templates, generated, changed)?;
            }
        }
        Expression::Defer(e) => {
            concretize_expression(e, templates, generated, changed)?;
        }
        Expression::Discard(e) => {
            concretize_expression(e, templates, generated, changed)?;
        }
        Expression::Return(e) => {
            concretize_expression(e, templates, generated, changed)?;
        }
        Expression::Yield(e) => {
            concretize_expression(e, templates, generated, changed)?;
        }
        Expression::If { arms } => {
            for arm in arms {
                if let crate::syntax::IfCondition::Expression(e) = &mut arm.condition {
                    concretize_expression(e, templates, generated, changed)?;
                }
                concretize_expression(&mut arm.body, templates, generated, changed)?;
            }
        }
        Expression::Match { target, arms, .. } => {
            concretize_expression(target, templates, generated, changed)?;
            for arm in arms {
                if let Some(vt) = &mut arm.variant_type {
                    concretize_type(vt, templates, generated, changed)?;
                }
                concretize_expression(&mut arm.body, templates, generated, changed)?;
            }
        }
        Expression::Repeat { count, body, .. } => {
            concretize_expression(count, templates, generated, changed)?;
            concretize_expression(body, templates, generated, changed)?;
        }
        Expression::RepeatWhile { condition, body } => {
            concretize_expression(condition, templates, generated, changed)?;
            concretize_expression(body, templates, generated, changed)?;
        }
        Expression::RepeatUntil { condition, body } => {
            concretize_expression(condition, templates, generated, changed)?;
            concretize_expression(body, templates, generated, changed)?;
        }
        Expression::For { iterable, body, .. } => {
            concretize_expression(iterable, templates, generated, changed)?;
            concretize_expression(body, templates, generated, changed)?;
        }
        Expression::Array(elements) => {
            for el in elements {
                concretize_expression(el, templates, generated, changed)?;
            }
        }
        Expression::Struct(fields) => {
            for f in fields {
                concretize_expression(&mut f.value, templates, generated, changed)?;
            }
        }
        Expression::Tuple(elements) => {
            for el in elements {
                concretize_expression(el, templates, generated, changed)?;
            }
        }
        Expression::Spread(inner) => {
            concretize_expression(inner, templates, generated, changed)?;
        }
        Expression::Integer(_)
        | Expression::String(_)
        | Expression::Symbol(_)
        | Expression::Identifier(_)
        | Expression::Break
        | Expression::Continue
        | Expression::Context
        | Expression::CallerLineNumber
        | Expression::CallerFileName
        | Expression::ThisLineNumber(_)
        | Expression::ThisFileName(_) => {}
    }
    Ok(())
}

pub fn concretize_meta_types(
    modules: &mut [LogicalModule],
    module_scopes: &mut [ModuleScope],
) -> Result<(), String> {
    let mut changed = true;
    let mut iterations = 0;

    while changed {
        changed = false;
        iterations += 1;
        if iterations > 128 {
            return Err(
                "meta type concretization depth limit exceeded (possible cyclic meta type recursion)"
                    .to_string(),
            );
        }

        let mut templates: HashMap<String, (String, TypeAlias)> = HashMap::new();
        for module in modules.iter() {
            for file in &module.source_files {
                for alias in &file.type_aliases {
                    if alias.is_meta {
                        templates.insert(alias.name.clone(), (file.file_path.clone(), alias.clone()));
                    }
                }
            }
        }
        for scope in module_scopes.iter() {
            for (name, prov) in &scope.type_aliases {
                if prov.item.is_meta {
                    templates.entry(name.clone()).or_insert_with(|| (prov.originating_file.clone(), prov.item.clone()));
                }
            }
        }

        let mut generated_aliases: Vec<(String, TypeAlias)> = Vec::new();

        for module in modules.iter_mut() {
            for file in &mut module.source_files {
                for def in &mut file.definitions {
                    if let Some(sig) = &mut def.signature {
                        for p in &mut sig.parameters {
                            concretize_type(&mut p.ty, &templates, &mut generated_aliases, &mut changed)?;
                        }
                        concretize_type(&mut sig.return_type, &templates, &mut generated_aliases, &mut changed)?;
                    }
                    concretize_expression(&mut def.value, &templates, &mut generated_aliases, &mut changed)?;
                }
                for ext in &mut file.extern_functions {
                    for p in &mut ext.signature.parameters {
                        concretize_type(&mut p.ty, &templates, &mut generated_aliases, &mut changed)?;
                    }
                    concretize_type(&mut ext.signature.return_type, &templates, &mut generated_aliases, &mut changed)?;
                }
                for alias in &mut file.type_aliases {
                    if !alias.is_meta {
                        concretize_type(&mut alias.target, &templates, &mut generated_aliases, &mut changed)?;
                    }
                }
            }
        }

        for scope in module_scopes.iter_mut() {
            for def_prov in scope.definitions.values_mut() {
                if let Some(sig) = &mut def_prov.item.signature {
                    for p in &mut sig.parameters {
                        concretize_type(&mut p.ty, &templates, &mut generated_aliases, &mut changed)?;
                    }
                    concretize_type(&mut sig.return_type, &templates, &mut generated_aliases, &mut changed)?;
                }
                concretize_expression(&mut def_prov.item.value, &templates, &mut generated_aliases, &mut changed)?;
            }
            for ext_prov in scope.extern_functions.values_mut() {
                for p in &mut ext_prov.item.signature.parameters {
                    concretize_type(&mut p.ty, &templates, &mut generated_aliases, &mut changed)?;
                }
                concretize_type(&mut ext_prov.item.signature.return_type, &templates, &mut generated_aliases, &mut changed)?;
            }
            for alias_prov in scope.type_aliases.values_mut() {
                if !alias_prov.item.is_meta {
                    concretize_type(&mut alias_prov.item.target, &templates, &mut generated_aliases, &mut changed)?;
                }
            }
        }

        for (originating_file, concrete_alias) in generated_aliases {
            let spec_name = concrete_alias.name.clone();

            for module in modules.iter_mut() {
                for file in &mut module.source_files {
                    if file.file_path == originating_file {
                        if !file.type_aliases.iter().any(|a| a.name == spec_name) {
                            file.type_aliases.push(concrete_alias.clone());
                        }
                        break;
                    }
                }
            }

            for scope in module_scopes.iter_mut() {
                if !scope.type_aliases.contains_key(&spec_name) {
                    scope.type_aliases.insert(
                        spec_name.clone(),
                        crate::sema::SymbolProvenance {
                            item: concrete_alias.clone(),
                            originating_file: originating_file.clone(),
                        },
                    );
                }
            }
        }
    }

    Ok(())
}

pub fn unwrap_returns(expr: &mut Expression) {
    unwrap_def_body_returns(expr);
}

pub fn unwrap_def_body_returns(expr: &mut Expression) {
    match expr {
        Expression::Spanned { expression, .. } => unwrap_def_body_returns(expression),
        Expression::Return(inner) => {
            unwrap_def_body_returns(inner);
            *expr = *inner.clone();
        }
        Expression::Discard(inner) => {
            unwrap_def_body_returns(inner);
            *expr = *inner.clone();
        }
        Expression::Block(exprs) => {
            for e in exprs.iter_mut() {
                unwrap_all_returns_in_expr(e);
            }
            if let Some(last) = exprs.last_mut() {
                unwrap_discard_and_return(last);
            }
        }
        _ => {
            unwrap_all_returns_in_expr(expr);
        }
    }
}

fn unwrap_discard_and_return(expr: &mut Expression) {
    match expr {
        Expression::Spanned { expression, .. } => unwrap_discard_and_return(expression),
        Expression::Discard(inner) => {
            unwrap_discard_and_return(inner);
            *expr = *inner.clone();
        }
        Expression::Return(inner) => {
            unwrap_discard_and_return(inner);
            *expr = *inner.clone();
        }
        _ => {}
    }
}

fn unwrap_all_returns_in_expr(expr: &mut Expression) {
    match expr {
        Expression::Spanned { expression, .. } => unwrap_all_returns_in_expr(expression),
        Expression::Return(inner) => {
            unwrap_all_returns_in_expr(inner);
            *expr = *inner.clone();
        }
        Expression::Block(exprs) => {
            for e in exprs {
                unwrap_all_returns_in_expr(e);
            }
        }
        Expression::If { arms } => {
            for arm in arms {
                unwrap_all_returns_in_expr(&mut arm.body);
            }
        }
        Expression::Match { arms, .. } => {
            for arm in arms {
                unwrap_all_returns_in_expr(&mut arm.body);
            }
        }
        Expression::Discard(inner) => {
            unwrap_all_returns_in_expr(inner);
            if matches!(crate::syntax::unspanned_expression(inner), Expression::Return(_)) {
                unwrap_discard_and_return(expr);
            }
        }
        _ => {}
    }
}

pub fn resolve_type_alias_deep(ty: &Type, scope: &ModuleScope) -> Type {
    let mut current = ty.clone();
    let mut visited = std::collections::HashSet::new();
    while let Type::Alias(ref name) = current {
        if !visited.insert(name.clone()) {
            break;
        }
        if let Some(alias) = scope.lookup_type_alias(name) {
            current = alias.target.clone();
        } else if let Some(prov) = scope.home_type_aliases.get(name) {
            current = prov.item.target.clone();
        } else {
            break;
        }
    }
    current
}

pub fn unify_types_to_sum_type(types: &[Type], scope: &ModuleScope) -> Type {
    if types.is_empty() {
        return Type::Symbol("none".to_string());
    }

    let mut flattened = Vec::new();
    for t in types {
        let resolved = resolve_type_alias_deep(t, scope);
        match resolved {
            Type::Union(variants) => {
                for v in variants {
                    flattened.push(resolve_type_alias_deep(&v, scope));
                }
            }
            other => flattened.push(other),
        }
    }

    let mut unique: Vec<Type> = Vec::new();
    for t in flattened {
        if !unique.iter().any(|u| u == &t) {
            unique.push(t);
        }
    }

    let mut non_never: Vec<Type> = unique
        .into_iter()
        .filter(|t| *t != Type::Never && *t != Type::Inferred)
        .collect();
    if non_never.is_empty() {
        return Type::Never;
    }
    if non_never.len() == 1 {
        return non_never.remove(0);
    }

    non_never.sort_by_key(crate::sema::Checker::canonical_type_key);
    Type::Union(non_never)
}

fn collect_returns(
    expr: &Expression,
    local_types: &HashMap<String, Type>,
    scope: &ModuleScope,
    generated: &[GeneratedSpecialization],
    returns: &mut Vec<Type>,
    visiting: &mut HashSet<String>,
    depth: usize,
) {
    if depth > 20 {
        return;
    }
    let unspanned = crate::syntax::unspanned_expression(expr);
    match unspanned {
        Expression::Return(r) | Expression::Yield(r) => {
            if let Some(t) = infer_expr_type_internal(r, local_types, scope, generated, visiting, depth + 1) {
                if t != Type::Never && t != Type::Inferred && !returns.contains(&t) {
                    returns.push(t);
                }
            }
        }
        Expression::Block(exprs) => {
            let mut block_locals = local_types.clone();
            for stmt in exprs {
                let mut unspanned_stmt = crate::syntax::unspanned_expression(stmt);
                while let Expression::Discard(inner) = unspanned_stmt {
                    unspanned_stmt = crate::syntax::unspanned_expression(inner);
                }
                match unspanned_stmt {
                    Expression::Let { name, value } => {
                        if let Some(t) = infer_expr_type_internal(value, &block_locals, scope, generated, visiting, depth + 1) {
                            block_locals.insert(name.clone(), t);
                        }
                    }
                    Expression::Var { name, ty, value } => {
                        if let Some(t) = ty {
                            if *t != Type::Inferred {
                                block_locals.insert(name.clone(), t.clone());
                            } else if let Some(v) = value {
                                if let Some(t) = infer_expr_type_internal(v, &block_locals, scope, generated, visiting, depth + 1) {
                                    block_locals.insert(name.clone(), t);
                                }
                            }
                        } else if let Some(v) = value {
                            if let Some(t) = infer_expr_type_internal(v, &block_locals, scope, generated, visiting, depth + 1) {
                                block_locals.insert(name.clone(), t);
                            }
                        }
                    }
                    Expression::Assign { target, value } => {
                        if let Expression::Identifier(name) = crate::syntax::unspanned_expression(target) {
                            if let Some(t) = infer_expr_type_internal(value, &block_locals, scope, generated, visiting, depth + 1) {
                                block_locals.insert(name.clone(), t);
                            }
                        }
                    }
                    _ => {}
                }
                collect_returns(stmt, &block_locals, scope, generated, returns, visiting, depth + 1);
            }
        }
        Expression::If { arms } => {
            for arm in arms {
                collect_returns(&arm.body, local_types, scope, generated, returns, visiting, depth + 1);
            }
        }
        Expression::Match { target, binding_name, arms } => {
            let target_ty_opt = infer_expr_type_internal(target, local_types, scope, generated, visiting, depth + 1);
            if let Some(ref target_ty) = target_ty_opt {
                let resolved_target = resolve_type_alias_deep(target_ty, scope);
                if !matches!(resolved_target, Type::Union(_)) {
                    let mut matched_arm_idx = None;
                    for (idx, arm) in arms.iter().enumerate() {
                        if let Some(vt) = &arm.variant_type {
                            if type_matches_pattern(target_ty, vt, scope) {
                                matched_arm_idx = Some(idx);
                                break;
                            }
                        } else {
                            matched_arm_idx = Some(idx);
                            break;
                        }
                    }
                    if let Some(idx) = matched_arm_idx {
                        let arm = &arms[idx];
                        let mut arm_locals = local_types.clone();
                        if !binding_name.is_empty() {
                            arm_locals.insert(binding_name.clone(), target_ty.clone());
                        }
                        collect_returns(&arm.body, &arm_locals, scope, generated, returns, visiting, depth + 1);
                        return;
                    }
                }
            }

            let unhandled_variants: Vec<Type> = if let Some(target_ty) = infer_expr_type_with_generated(target, local_types, scope, generated) {
                let resolved = resolve_type_alias_deep(&target_ty, scope);
                if let Type::Union(variants) = resolved {
                    variants
                        .into_iter()
                        .filter(|v| {
                            !arms.iter().any(|a| {
                                if let Some(vt) = &a.variant_type {
                                    type_matches_pattern(v, vt, scope)
                                } else {
                                    false
                                }
                            })
                        })
                        .collect()
                } else {
                    Vec::new()
                }
            } else {
                Vec::new()
            };

            for arm in arms {
                let mut arm_locals = local_types.clone();
                if !binding_name.is_empty() {
                    if let Some(vt) = &arm.variant_type {
                        arm_locals.insert(binding_name.clone(), vt.clone());
                    } else if !unhandled_variants.is_empty() {
                        let narrowed = if unhandled_variants.len() == 1 {
                            unhandled_variants[0].clone()
                        } else {
                            unify_types_to_sum_type(&unhandled_variants, scope)
                        };
                        arm_locals.insert(binding_name.clone(), narrowed);
                    } else if let Some(target_ty) = infer_expr_type_internal(target, local_types, scope, generated, visiting, depth + 1) {
                        arm_locals.insert(binding_name.clone(), target_ty);
                    }
                }
                collect_returns(&arm.body, &arm_locals, scope, generated, returns, visiting, depth + 1);
            }
        }
        Expression::Repeat { count: _, body, binder } => {
            let mut loop_locals = local_types.clone();
            if let Some(b) = binder {
                loop_locals.insert(b.clone(), Type::USize);
            }
            collect_returns(body, &loop_locals, scope, generated, returns, visiting, depth + 1);
        }
        Expression::RepeatWhile { condition: _, body } => {
            collect_returns(body, local_types, scope, generated, returns, visiting, depth + 1);
        }
        Expression::RepeatUntil { condition: _, body } => {
            collect_returns(body, local_types, scope, generated, returns, visiting, depth + 1);
        }
        Expression::For { iterable, binder, body } => {
            let mut loop_locals = local_types.clone();
            if let Some(iter_ty) = infer_expr_type_internal(iterable, local_types, scope, generated, visiting, depth + 1) {
                let resolved = resolve_type_alias_deep(&iter_ty, scope);
                match resolved {
                    Type::Array { element, .. } => {
                        if let Some(b) = binder {
                            loop_locals.insert(b.clone(), *element);
                        }
                    }
                    ref other if crate::sema::Checker::new(scope).is_slice_type(other).unwrap_or(false) => {
                        if let Ok(elem) = crate::sema::Checker::new(scope).slice_element_type(other) {
                            if let Some(b) = binder {
                                loop_locals.insert(b.clone(), elem);
                            }
                        }
                    }
                    _ => {}
                }
            }
            collect_returns(body, &loop_locals, scope, generated, returns, visiting, depth + 1);
        }
        Expression::Discard(inner) => {
            collect_returns(inner, local_types, scope, generated, returns, visiting, depth + 1);
        }
        _ => {}
    }
}

pub fn infer_expr_type(
    expr: &Expression,
    local_types: &HashMap<String, Type>,
    scope: &ModuleScope,
) -> Option<Type> {
    infer_expr_type_with_generated(expr, local_types, scope, &[])
}

fn collect_block_yield_types(
    expr: &Expression,
    local_types: &HashMap<String, Type>,
    scope: &ModuleScope,
    generated: &[GeneratedSpecialization],
    yields: &mut Vec<Type>,
    visiting: &mut HashSet<String>,
    depth: usize,
) {
    if depth > 20 {
        return;
    }
    match crate::syntax::unspanned_expression(expr) {
        Expression::Yield(inner) => {
            if let Some(t) = infer_expr_type_internal(inner, local_types, scope, generated, visiting, depth + 1) {
                if t != Type::Never && t != Type::Inferred && !yields.contains(&t) {
                    yields.push(t);
                }
            }
        }
        Expression::Block(_) => {}
        Expression::If { arms } => {
            for arm in arms {
                collect_block_yield_types(&arm.body, local_types, scope, generated, yields, visiting, depth + 1);
            }
        }
        Expression::Match { arms, .. } => {
            for arm in arms {
                collect_block_yield_types(&arm.body, local_types, scope, generated, yields, visiting, depth + 1);
            }
        }
        Expression::Discard(inner) => {
            collect_block_yield_types(inner, local_types, scope, generated, yields, visiting, depth + 1);
        }
        _ => {}
    }
}

fn infer_function_return_type(
    def_value: &Expression,
    local_types: &HashMap<String, Type>,
    scope: &ModuleScope,
    generated: &[GeneratedSpecialization],
    visiting: &mut HashSet<String>,
    depth: usize,
) -> Option<Type> {
    let mut returns = Vec::new();
    collect_returns(def_value, local_types, scope, generated, &mut returns, visiting, depth + 1);
    let fallthrough = infer_expr_type_internal(def_value, local_types, scope, generated, visiting, depth + 1);
    if let Some(ref ft) = fallthrough {
        if *ft != Type::Never && *ft != Type::Inferred && !returns.contains(ft) {
            returns.push(ft.clone());
        }
    }
    if returns.is_empty() {
        fallthrough
    } else if returns.len() == 1 {
        Some(returns.remove(0))
    } else {
        Some(unify_types_to_sum_type(&returns, scope))
    }
}

pub(crate) fn infer_expr_type_with_generated(
    expr: &Expression,
    local_types: &HashMap<String, Type>,
    scope: &ModuleScope,
    generated: &[GeneratedSpecialization],
) -> Option<Type> {
    let mut visiting = HashSet::new();
    infer_expr_type_internal(expr, local_types, scope, generated, &mut visiting, 0)
}

fn infer_expr_type_internal(
    expr: &Expression,
    local_types: &HashMap<String, Type>,
    scope: &ModuleScope,
    generated: &[GeneratedSpecialization],
    visiting: &mut HashSet<String>,
    depth: usize,
) -> Option<Type> {
    if depth > 20 {
        return None;
    }
    match expr {
        Expression::Spanned { expression, .. } => {
            infer_expr_type_internal(expression, local_types, scope, generated, visiting, depth + 1)
        }
        Expression::Integer(_) => Some(Type::S32),
        Expression::String(_) => Some(Type::Alias("String".to_string())),
        Expression::Symbol(name) => {
            if name == "true" || name == "false" {
                Some(Type::Alias("Bool".to_string()))
            } else {
                Some(Type::Symbol(name.clone()))
            }
        }
        Expression::SizeOf(_) => Some(Type::USize),
        Expression::AsTypeOf { target, .. } => {
            infer_expr_type_internal(target, local_types, scope, generated, visiting, depth + 1)
        }
        Expression::Identifier(name) => {
            if let Some(ty) = local_types.get(name) {
                return Some(ty.clone());
            }
            if name == "T" && local_types.len() == 1 {
                return Some(local_types.values().next().unwrap().clone());
            }
            match name.as_str() {
                "s16" => return Some(Type::S16),
                "s32" => return Some(Type::S32),
                "s64" => return Some(Type::S64),
                "u8" => return Some(Type::U8),
                "u16" => return Some(Type::U16),
                "u32" => return Some(Type::U32),
                "u64" => return Some(Type::U64),
                "usize" => return Some(Type::USize),
                "ssize" => return Some(Type::SSize),
                _ => {}
            }
            if let Some(prov) = scope.definitions.get(name) {
                if let Some(sig) = &prov.item.signature {
                    return Some(Type::Function {
                        parameters: sig.parameters.iter().map(|p| p.ty.clone()).collect(),
                        return_type: sig.return_type.clone(),
                        is_variadic: sig.is_variadic,
                    });
                } else if !visiting.contains(name) {
                    visiting.insert(name.clone());
                    let res = infer_expr_type_internal(&prov.item.value, &HashMap::new(), scope, generated, visiting, depth + 1);
                    visiting.remove(name);
                    return res;
                }
            }
            if let Some(ext) = scope.lookup_extern(name) {
                return Some(Type::Function {
                    parameters: ext.signature.parameters.iter().map(|p| p.ty.clone()).collect(),
                    return_type: ext.signature.return_type.clone(),
                    is_variadic: ext.signature.is_variadic,
                });
            }
            if let Some(ext) = scope.extern_functions.get(name) {
                return Some(Type::Function {
                    parameters: ext.item.signature.parameters.iter().map(|p| p.ty.clone()).collect(),
                    return_type: ext.item.signature.return_type.clone(),
                    is_variadic: ext.item.signature.is_variadic,
                });
            }
            if let Some(alias) = scope.lookup_type_alias(name) {
                return Some(Type::Alias(alias.name.clone()));
            }
            None
        }
        Expression::Cast { ty, .. } => Some(ty.clone()),
        Expression::AddressOf { expression, is_mut } => {
            infer_expr_type_internal(expression, local_types, scope, generated, visiting, depth + 1).map(|el| Type::Pointer {
                element: Box::new(el),
                is_mut: *is_mut,
            })
        }
        Expression::Dereference(expression) => {
            infer_expr_type_internal(expression, local_types, scope, generated, visiting, depth + 1).and_then(|t| {
                match resolve_type_alias_deep(&t, scope) {
                    Type::Pointer { element, .. } => Some(*element),
                    _ => None,
                }
            })
        }
        Expression::Array(elements) => {
            if elements.is_empty() {
                Some(Type::Array {
                    element: Box::new(Type::U8),
                    size: 0,
                })
            } else {
                let mut elem_types = Vec::new();
                for el in elements {
                    let unspanned = crate::syntax::unspanned_expression(el);
                    if let Expression::Spread(inner) = unspanned {
                        if let Some(t) = infer_expr_type_internal(inner, local_types, scope, generated, visiting, depth + 1) {
                            let resolved = resolve_type_alias_deep(&t, scope);
                            match resolved {
                                Type::Array { element, .. } => {
                                    elem_types.push(*element);
                                }
                                Type::Tuple(tys) => {
                                    for ty in tys {
                                        elem_types.push(ty);
                                    }
                                }
                                _ => elem_types.push(t),
                            }
                        } else {
                            return None;
                        }
                    } else if let Some(t) = infer_expr_type_internal(el, local_types, scope, generated, visiting, depth + 1) {
                        elem_types.push(t);
                    } else {
                        return None;
                    }
                }
                let unified = unify_types_to_sum_type(&elem_types, scope);
                Some(Type::Array {
                    element: Box::new(unified),
                    size: elements.len(),
                })
            }
        }
        Expression::Tuple(elements) => {
            let mut elem_types = Vec::with_capacity(elements.len());
            for elem in elements {
                if let Some(t) = infer_expr_type_internal(elem, local_types, scope, generated, visiting, depth + 1) {
                    elem_types.push(t);
                } else {
                    return None;
                }
            }
            Some(Type::Tuple(elem_types))
        }
        Expression::Struct(fields) => {
            let mut struct_fields = Vec::with_capacity(fields.len());
            for f in fields {
                if let Some(name) = &f.name {
                    if let Some(t) = infer_expr_type_internal(&f.value, local_types, scope, generated, visiting, depth + 1) {
                        if let Some(existing) = struct_fields.iter_mut().find(|sf: &&mut StructFieldType| sf.name == *name) {
                            existing.ty = t;
                        } else {
                            struct_fields.push(StructFieldType {
                                name: name.clone(),
                                ty: t,
                            });
                        }
                    } else {
                        return None;
                    }
                } else {
                    if let Some(t) = infer_expr_type_internal(&f.value, local_types, scope, generated, visiting, depth + 1) {
                        let resolved = resolve_type_alias_deep(&t, scope);
                        if let Type::Struct(s_fields) = resolved {
                            for sf in s_fields {
                                if let Some(existing) = struct_fields.iter_mut().find(|existing: &&mut StructFieldType| existing.name == sf.name) {
                                    existing.ty = sf.ty;
                                } else {
                                    struct_fields.push(sf);
                                }
                            }
                        }
                    } else {
                        return None;
                    }
                }
            }
            Some(Type::Struct(struct_fields))
        }
        Expression::Slice(inner) => {
            if let Some(inner_ty) = infer_expr_type_internal(inner, local_types, scope, generated, visiting, depth + 1) {
                let resolved = resolve_type_alias_deep(&inner_ty, scope);
                match resolved {
                    Type::Array { element, .. } => Some(Type::GenericInstance {
                        name: "Slice".to_string(),
                        type_arguments: vec![*element],
                    }),
                    Type::Tuple(elements) => elements.first().cloned().map(|first| {
                        Type::GenericInstance {
                            name: "Slice".to_string(),
                            type_arguments: vec![first],
                        }
                    }),
                    _ => None,
                }
            } else {
                None
            }
        }
        Expression::Member { expression, field } => {
            if field == "@sizeOf" && !receiver_has_field(expression, field, local_types, scope, generated) {
                return Some(Type::USize);
            }
            if let Some(expr_ty) = infer_expr_type_internal(expression, local_types, scope, generated, visiting, depth + 1) {
                let resolved = resolve_type_alias_deep(&expr_ty, scope);
                match resolved {
                    Type::Struct(fields) => {
                        fields.iter().find(|f| f.name == *field).map(|f| f.ty.clone())
                    }
                    Type::Tuple(elements) => {
                        field.parse::<usize>().ok().and_then(|idx| elements.get(idx).cloned())
                    }
                    Type::GenericInstance { name, type_arguments } => {
                        if let Some(alias) = scope.lookup_type_alias(&name) {
                            if alias.is_meta && alias.type_parameters.len() == type_arguments.len() {
                                let mut substitutions = HashMap::new();
                                for (param, arg) in alias.type_parameters.iter().zip(type_arguments.iter()) {
                                    substitutions.insert(param.clone(), arg.clone());
                                }
                                let target_concrete = substitute_type(&alias.target, &substitutions);
                                let resolved_target = resolve_type_alias_deep(&target_concrete, scope);
                                match resolved_target {
                                    Type::Struct(fields) => {
                                        fields.iter().find(|f| f.name == *field).map(|f| f.ty.clone())
                                    }
                                    Type::Tuple(elements) => {
                                        field.parse::<usize>().ok().and_then(|idx| elements.get(idx).cloned())
                                    }
                                    _ => None,
                                }
                            } else {
                                None
                            }
                        } else if name == "Slice" {
                            if field == "len" {
                                Some(Type::USize)
                            } else if field == "ptr" {
                                type_arguments.first().map(|elem| Type::Pointer {
                                    element: Box::new(elem.clone()),
                                    is_mut: false,
                                })
                            } else {
                                None
                            }
                        } else {
                            None
                        }
                    }
                    Type::Array { .. } if field == "len" => Some(Type::USize),
                    Type::Pointer { element, .. } => {
                        let resolved_elem = resolve_type_alias_deep(&element, scope);
                        if let Type::Struct(fields) = resolved_elem {
                            fields.iter().find(|f| f.name == *field).map(|f| f.ty.clone())
                        } else {
                            None
                        }
                    }
                    _ => None,
                }
            } else {
                None
            }
        }
        Expression::Index { expression, index } => {
            if let Some(expr_ty) = infer_expr_type_internal(expression, local_types, scope, generated, visiting, depth + 1) {
                let resolved = resolve_type_alias_deep(&expr_ty, scope);
                match resolved {
                    Type::Array { element, .. } => Some(*element),
                    Type::Pointer { element, .. } => Some(*element),
                    Type::GenericInstance { name, type_arguments } if name == "Slice" => {
                        type_arguments.first().cloned()
                    }
                    Type::Tuple(elements) => {
                        if let Expression::Integer(i) = crate::syntax::unspanned_expression(index) {
                            elements.get(*i as usize).cloned()
                        } else {
                            None
                        }
                    }
                    _ => None,
                }
            } else {
                None
            }
        }
        Expression::Binary { left, .. } => infer_expr_type_internal(left, local_types, scope, generated, visiting, depth + 1),
        Expression::Comparison { .. } => Some(Type::Alias("Bool".to_string())),
        Expression::Block(exprs) => {
            let mut block_locals = local_types.clone();
            for stmt in exprs {
                let mut unspanned = crate::syntax::unspanned_expression(stmt);
                while let Expression::Discard(inner) = unspanned {
                    unspanned = crate::syntax::unspanned_expression(inner);
                }
                match unspanned {
                    Expression::Let { name, value } => {
                        if let Some(t) = infer_expr_type_internal(value, &block_locals, scope, generated, visiting, depth + 1) {
                            block_locals.insert(name.clone(), t);
                        }
                    }
                    Expression::Var { name, ty, value } => {
                        if let Some(t) = ty {
                            if *t != Type::Inferred {
                                block_locals.insert(name.clone(), t.clone());
                            } else if let Some(v) = value {
                                if let Some(t) = infer_expr_type_internal(v, &block_locals, scope, generated, visiting, depth + 1) {
                                    block_locals.insert(name.clone(), t);
                                }
                            }
                        } else if let Some(v) = value {
                            if let Some(t) = infer_expr_type_internal(v, &block_locals, scope, generated, visiting, depth + 1) {
                                block_locals.insert(name.clone(), t);
                            }
                        }
                    }
                    Expression::Assign { target, value } => {
                        if let Expression::Identifier(name) = crate::syntax::unspanned_expression(target) {
                            if let Some(t) = infer_expr_type_internal(value, &block_locals, scope, generated, visiting, depth + 1) {
                                block_locals.insert(name.clone(), t);
                            }
                        }
                    }
                    _ => {}
                }
            }

            let mut yields = Vec::new();
            for stmt in exprs {
                collect_block_yield_types(stmt, &block_locals, scope, generated, &mut yields, visiting, depth + 1);
            }
            if !yields.is_empty() {
                if yields.len() == 1 {
                    Some(yields.remove(0))
                } else {
                    Some(unify_types_to_sum_type(&yields, scope))
                }
            } else {
                exprs.last().and_then(|e| infer_expr_type_internal(e, &block_locals, scope, generated, visiting, depth + 1))
            }
        }
        Expression::If { arms } => {
            let mut arm_types = Vec::new();
            for arm in arms {
                if let Some(t) = infer_expr_type_internal(&arm.body, local_types, scope, generated, visiting, depth + 1) {
                    if t != Type::Never && t != Type::Inferred && !arm_types.contains(&t) {
                        arm_types.push(t);
                    }
                }
            }
            if arm_types.is_empty() {
                Some(Type::Never)
            } else if arm_types.len() == 1 {
                Some(arm_types.remove(0))
            } else {
                Some(unify_types_to_sum_type(&arm_types, scope))
            }
        }
        Expression::Match { target, binding_name, arms } => {
            let target_ty_opt = infer_expr_type_internal(target, local_types, scope, generated, visiting, depth + 1);
            if let Some(ref target_ty) = target_ty_opt {
                let resolved_target = resolve_type_alias_deep(target_ty, scope);
                if !matches!(resolved_target, Type::Union(_)) {
                    let mut matched_arm_idx = None;
                    for (idx, arm) in arms.iter().enumerate() {
                        if let Some(vt) = &arm.variant_type {
                            if type_matches_pattern(target_ty, vt, scope) {
                                matched_arm_idx = Some(idx);
                                break;
                            }
                        } else {
                            matched_arm_idx = Some(idx);
                            break;
                        }
                    }
                    if let Some(idx) = matched_arm_idx {
                        let arm = &arms[idx];
                        let mut arm_locals = local_types.clone();
                        if !binding_name.is_empty() {
                            arm_locals.insert(binding_name.clone(), target_ty.clone());
                        }
                        return infer_expr_type_internal(&arm.body, &arm_locals, scope, generated, visiting, depth + 1);
                    }
                }
            }

            let unhandled_variants: Vec<Type> = if let Some(target_ty) = infer_expr_type_with_generated(target, local_types, scope, generated) {
                let resolved = resolve_type_alias_deep(&target_ty, scope);
                if let Type::Union(variants) = resolved {
                    variants
                        .into_iter()
                        .filter(|v| {
                            !arms.iter().any(|a| {
                                if let Some(vt) = &a.variant_type {
                                    type_matches_pattern(v, vt, scope)
                                } else {
                                    false
                                }
                            })
                        })
                        .collect()
                } else {
                    Vec::new()
                }
            } else {
                Vec::new()
            };

            let mut arm_types = Vec::new();
            for arm in arms {
                let mut arm_locals = local_types.clone();
                if !binding_name.is_empty() {
                    if let Some(vt) = &arm.variant_type {
                        arm_locals.insert(binding_name.clone(), vt.clone());
                    } else if !unhandled_variants.is_empty() {
                        let narrowed = if unhandled_variants.len() == 1 {
                            unhandled_variants[0].clone()
                        } else {
                            unify_types_to_sum_type(&unhandled_variants, scope)
                        };
                        arm_locals.insert(binding_name.clone(), narrowed);
                    } else if let Some(target_ty) = infer_expr_type_internal(target, local_types, scope, generated, visiting, depth + 1) {
                        arm_locals.insert(binding_name.clone(), target_ty);
                    }
                }
                if let Some(t) = infer_expr_type_internal(&arm.body, &arm_locals, scope, generated, visiting, depth + 1) {
                    if t != Type::Never && t != Type::Inferred && !arm_types.contains(&t) {
                        arm_types.push(t);
                    }
                }
            }
            if arm_types.is_empty() {
                Some(Type::Never)
            } else if arm_types.len() == 1 {
                Some(arm_types.remove(0))
            } else {
                Some(unify_types_to_sum_type(&arm_types, scope))
            }
        }
        Expression::Return(_) => Some(Type::Never),
        Expression::Yield(inner) => infer_expr_type_internal(inner, local_types, scope, generated, visiting, depth + 1),
        Expression::Defer(_) => Some(Type::Symbol("none".to_string())),
        Expression::Break | Expression::Continue => Some(Type::Never),
        Expression::Context => Some(crate::sema::context_type()),
        Expression::CallerLineNumber => Some(Type::U64),
        Expression::CallerFileName => Some(Type::Alias("String".to_string())),
        Expression::ThisLineNumber(_) => Some(Type::U64),
        Expression::ThisFileName(_) => Some(Type::Alias("String".to_string())),
        Expression::Discard(inner) => {
            let mut unspanned = crate::syntax::unspanned_expression(inner);
            while let Expression::Discard(d) = unspanned {
                unspanned = crate::syntax::unspanned_expression(d);
            }
            if let Expression::Return(_) = unspanned {
                Some(Type::Never)
            } else {
                let inner_ty = infer_expr_type_internal(unspanned, local_types, scope, generated, visiting, depth + 1);
                if let Some(Type::Never) = inner_ty {
                    Some(Type::Never)
                } else {
                    Some(Type::Symbol("none".to_string()))
                }
            }
        }
        Expression::Call { target, arguments } => {
            let fn_name_opt = unwrap_identifier_name(target).or_else(|| {
                if let Expression::Member { field, .. } = crate::syntax::unspanned_expression(target) {
                    Some(field.clone())
                } else {
                    None
                }
            });
            if let Some(fn_name) = fn_name_opt {
                if fn_name == "@asTypeOf" && !arguments.is_empty() {
                    return infer_expr_type_internal(arguments.last().unwrap(), local_types, scope, generated, visiting, depth + 1);
                }
                if fn_name == "@sizeOf" {
                    return Some(Type::USize);
                }
                if let Some(g) = generated.iter().rev().find(|g| g.specialized_def.name == fn_name) {
                    if let Some(sig) = &g.specialized_def.signature {
                        if *sig.return_type != Type::Inferred {
                            return Some(*sig.return_type.clone());
                        } else if !visiting.contains(&fn_name) {
                            visiting.insert(fn_name.clone());
                            let res = infer_function_return_type(&g.specialized_def.value, &HashMap::new(), scope, generated, visiting, depth + 1);
                            visiting.remove(&fn_name);
                            return res;
                        }
                    }
                }
                if let Some(prov) = scope.definitions.get(&fn_name) {
                    if let Some(sig) = &prov.item.signature {
                        if *sig.return_type != Type::Inferred {
                            return Some(*sig.return_type.clone());
                        } else if !visiting.contains(&fn_name) {
                            visiting.insert(fn_name.clone());
                            let res = infer_function_return_type(&prov.item.value, &HashMap::new(), scope, generated, visiting, depth + 1);
                            visiting.remove(&fn_name);
                            return res;
                        }
                    } else if !visiting.contains(&fn_name) {
                        visiting.insert(fn_name.clone());
                        let res = infer_function_return_type(&prov.item.value, &HashMap::new(), scope, generated, visiting, depth + 1);
                        visiting.remove(&fn_name);
                        return res;
                    }
                }
                if let Some(ext) = scope.lookup_extern(&fn_name) {
                    return Some(*ext.signature.return_type.clone());
                }
                if let Some(ext) = scope.extern_functions.get(&fn_name) {
                    return Some(*ext.item.signature.return_type.clone());
                }
            }
            if let Some(target_ty) = infer_expr_type_internal(target, local_types, scope, generated, visiting, depth + 1) {
                let resolved = resolve_type_alias_deep(&target_ty, scope);
                if let Type::Function { return_type, .. } = resolved {
                    return Some(*return_type);
                }
            }
            None
        }
        _ => None,
    }
}



pub fn type_matches_pattern(target: &Type, pattern: &Type, scope: &ModuleScope) -> bool {
    type_matches_pattern_depth(target, pattern, scope, 0)
}

fn type_matches_pattern_depth(target: &Type, pattern: &Type, scope: &ModuleScope, depth: usize) -> bool {
    if depth > 16 {
        return false;
    }
    let resolved_target = resolve_type_alias_deep(target, scope);
    let resolved_pattern = resolve_type_alias_deep(pattern, scope);

    if resolved_target == resolved_pattern {
        return true;
    }

    if matches!(resolved_target, Type::Symbol(_)) {
        if let Type::Struct(p_fields) = &resolved_pattern {
            if p_fields.len() == 2 && p_fields[0].name == "ptr" && p_fields[1].name == "len" {
                if let Type::Pointer { element, .. } = &p_fields[0].ty {
                    if resolve_type_alias_deep(element, scope) == Type::U8 {
                        return true;
                    }
                }
            }
        }
    }

    if let (Type::Struct(t_fields), Type::Struct(p_fields)) = (&resolved_target, &resolved_pattern) {
        if t_fields.len() == p_fields.len() {
            if t_fields.len() == 2
                && t_fields[0].name == "ptr"
                && t_fields[1].name == "len"
                && p_fields[0].name == "ptr"
                && p_fields[1].name == "len"
            {
                if let (
                    Type::Pointer { element: t_elem, .. },
                    Type::Pointer { element: p_elem, .. },
                ) = (&t_fields[0].ty, &p_fields[0].ty)
                {
                    let p_res = resolve_type_alias_deep(p_elem, scope);
                    if let Type::Alias(name) = &p_res {
                        if name == "Generic" || name == "Printable" {
                            return true;
                        }
                    }
                    if let Type::Union(_) = &p_res {
                        return type_matches_pattern_depth(t_elem, &p_res, scope, depth + 1);
                    }
                    return type_matches_pattern_depth(t_elem, p_elem, scope, depth + 1);
                }
            }
        }
    }

    if let Type::Union(t_vars) = &resolved_target {
        return t_vars.iter().all(|tv| type_matches_pattern_depth(tv, pattern, scope, depth + 1));
    }

    match &resolved_pattern {
        Type::Union(variants) => {
            variants.iter().any(|v| type_matches_pattern_depth(target, v, scope, depth + 1))
        }
        Type::Tuple(p_elems) => {
            if let Type::Tuple(t_elems) = &resolved_target {
                if p_elems.len() == t_elems.len() {
                    return p_elems.iter().zip(t_elems.iter()).all(|(p, t)| {
                        let p_res = resolve_type_alias_deep(p, scope);
                        if let Type::Alias(name) = &p_res {
                            if name == "Generic" || name == "Printable" {
                                return true;
                            }
                        }
                        if let Type::Union(_) = &p_res {
                            return type_matches_pattern_depth(t, &p_res, scope, depth + 1);
                        }
                        type_matches_pattern_depth(t, p, scope, depth + 1)
                    });
                }
            }
            false
        }
        Type::GenericInstance { name: p_name, type_arguments: p_args } => {
            if let Type::GenericInstance { name: t_name, type_arguments: t_args } = &resolved_target {
                if p_name == t_name && p_args.len() == t_args.len() {
                    return p_args.iter().zip(t_args.iter()).all(|(p, t)| {
                        let p_res = resolve_type_alias_deep(p, scope);
                        if let Type::Alias(name) = &p_res {
                            if name == "Generic" || name == "Printable" {
                                return true;
                            }
                        }
                        if let Type::Union(_) = &p_res {
                            return type_matches_pattern_depth(t, &p_res, scope, depth + 1);
                        }
                        type_matches_pattern_depth(t, p, scope, depth + 1)
                    });
                }
            }
            if let Some(template) = scope.lookup_type_alias(p_name) {
                if template.is_meta && template.type_parameters.len() == p_args.len() {
                    let mut subs = HashMap::new();
                    for (param, arg) in template.type_parameters.iter().zip(p_args.iter()) {
                        subs.insert(param.clone(), arg.clone());
                    }
                    let substituted = substitute_type(&template.target, &subs);
                    return type_matches_pattern_depth(target, &substituted, scope, depth + 1);
                }
            }
            false
        }
        _ => false,
    }
}

fn scan_and_monomorphize_def(
    expr: &mut Expression,
    scope: &ModuleScope,
    def_templates: &HashMap<String, (String, Definition)>,
    local_types: &mut HashMap<String, Type>,
    generated: &mut Vec<GeneratedSpecialization>,
    changed: &mut bool,
) -> Result<(), String> {
    match expr {
        Expression::Spanned { expression, .. } => {
            scan_and_monomorphize_def(
                expression,
                scope,
                def_templates,
                local_types,
                generated,
                changed,
            )?;
        }
        Expression::Block(exprs) => {
            let orig_locals = local_types.clone();
            for stmt in exprs.iter_mut() {
                scan_and_monomorphize_def(
                    stmt,
                    scope,
                    def_templates,
                    local_types,
                    generated,
                    changed,
                )?;
                let mut unspanned = crate::syntax::unspanned_expression(stmt);
                while let Expression::Discard(inner) = unspanned {
                    unspanned = crate::syntax::unspanned_expression(inner);
                }
                match unspanned {
                    Expression::Let { name, value } => {
                        if let Some(t) = infer_expr_type_with_generated(value, local_types, scope, generated) {
                            local_types.insert(name.clone(), t);
                        }
                    }
                    Expression::Var { name, ty, value } => {
                        if let Some(t) = ty {
                            if *t != Type::Inferred {
                                local_types.insert(name.clone(), t.clone());
                            } else if let Some(v) = value {
                                if let Some(t) = infer_expr_type_with_generated(v, local_types, scope, generated) {
                                    local_types.insert(name.clone(), t);
                                }
                            }
                        } else if let Some(v) = value {
                            if let Some(t) = infer_expr_type_with_generated(v, local_types, scope, generated) {
                                local_types.insert(name.clone(), t);
                            }
                        }
                    }
                    Expression::Assign { target, value } => {
                        if let Expression::Identifier(name) = crate::syntax::unspanned_expression(target) {
                            if !local_types.contains_key(name) {
                                if let Some(t) = infer_expr_type_with_generated(value, local_types, scope, generated) {
                                    local_types.insert(name.clone(), t);
                                }
                            }
                        }
                    }
                    _ => {}
                }
            }
            *local_types = orig_locals;
        }
        Expression::Call { target, arguments } => {
            if let Expression::Member { expression: receiver, field } = target.as_mut() {
                if is_function_in_scope(field, scope, def_templates)
                    && !receiver_has_field(receiver, field, local_types, scope, generated)
                {
                    let field_name = field.clone();
                    let recv = *receiver.clone();
                    let mut new_args = vec![recv];
                    new_args.append(arguments);
                    *expr = Expression::Call {
                        target: Box::new(Expression::Identifier(field_name)),
                        arguments: new_args,
                    };
                    *changed = true;
                    return scan_and_monomorphize_def(expr, scope, def_templates, local_types, generated, changed);
                }
            }

            for arg in arguments.iter_mut() {
                scan_and_monomorphize_def(arg, scope, def_templates, local_types, generated, changed)?;
            }
            scan_and_monomorphize_def(target, scope, def_templates, local_types, generated, changed)?;

            if let Expression::Identifier(name) = crate::syntax::unspanned_expression(target) {
                if name == "@asTypeOf" && arguments.len() == 2 {
                    let target_expr = arguments.pop().unwrap();
                    let src_expr = arguments.pop().unwrap();
                    *expr = Expression::AsTypeOf {
                        expression: Box::new(src_expr),
                        target: Box::new(target_expr),
                    };
                    *changed = true;
                    return scan_and_monomorphize_def(expr, scope, def_templates, local_types, generated, changed);
                }
                if name == "@sizeOf" && arguments.len() == 1 {
                    let src_expr = arguments.pop().unwrap();
                    *expr = Expression::SizeOf(Box::new(src_expr));
                    *changed = true;
                    return scan_and_monomorphize_def(expr, scope, def_templates, local_types, generated, changed);
                }
            }

            if arguments.iter().any(|e| matches!(crate::syntax::unspanned_expression(e), Expression::Spread(_))) {
                let mut expanded = Vec::new();
                let mut all_known = true;
                for el in arguments.iter() {
                    if let Expression::Spread(inner) = crate::syntax::unspanned_expression(el) {
                        if let Some(t) = infer_expr_type_with_generated(inner, local_types, scope, generated) {
                            let resolved = resolve_type_alias_deep(&t, scope);
                            match resolved {
                                Type::Array { size, .. } => {
                                    for i in 0..size {
                                        expanded.push(Expression::Index {
                                            expression: inner.clone(),
                                            index: Box::new(Expression::Integer(i as i64)),
                                        });
                                    }
                                }
                                Type::Tuple(tys) => {
                                    for i in 0..tys.len() {
                                        expanded.push(Expression::Member {
                                            expression: inner.clone(),
                                            field: i.to_string(),
                                        });
                                    }
                                }
                                _ => {
                                    all_known = false;
                                    break;
                                }
                            }
                        } else {
                            all_known = false;
                            break;
                        }
                    } else {
                        expanded.push(el.clone());
                    }
                }
                if all_known {
                    *arguments = expanded;
                    *changed = true;
                }
            }

            if let Some(fn_name) = unwrap_identifier_name(target) {
                let template_opt = if let Some(prov) = scope.definitions.get(&fn_name) {
                    if prov.item.is_def {
                        Some((prov.originating_file.clone(), prov.item.clone()))
                    } else {
                        None
                    }
                } else {
                    def_templates.get(&fn_name).cloned()
                };
                if let Some((originating_file, template)) = template_opt {
                    let sig = template
                        .signature
                        .as_ref()
                        .ok_or_else(|| format!("def function '{}' has no signature", fn_name))?;

                    if arguments.len() != sig.parameters.len() {
                        return Err(format!(
                            "def function '{}' expects {} arguments, found {}",
                            fn_name,
                            sig.parameters.len(),
                            arguments.len()
                        ));
                    }

                    let mut arg_types = Vec::with_capacity(arguments.len());
                    for (param, arg) in sig.parameters.iter().zip(arguments.iter()) {
                        if param.ty != Type::Inferred {
                            arg_types.push(param.ty.clone());
                        } else {
                            let ty = infer_expr_type_with_generated(arg, local_types, scope, generated).unwrap_or(Type::Inferred);
                            arg_types.push(ty.clone());
                        }
                    }

                    let mangled_suffix = arg_types
                        .iter()
                        .map(mangle_type)
                        .collect::<Vec<_>>()
                        .join("__");
                    let mangled_name =
                        sanitize_identifier(&format!("{}__{}", fn_name, mangled_suffix));

                    let already_exists = generated.iter().any(|g| g.specialized_def.name == mangled_name)
                        || scope.definitions.contains_key(&mangled_name);

                    if !already_exists {
                        let mut specialized_def = template.clone();
                        specialized_def.name = mangled_name.clone();
                        specialized_def.is_def = false;
                        specialized_def.is_specialized_def = true;

                        let mut param_types_map = HashMap::new();
                        if let Some(spec_sig) = &mut specialized_def.signature {
                            for (p, arg_ty) in spec_sig.parameters.iter_mut().zip(arg_types.iter()) {
                                if p.ty == Type::Inferred && *arg_ty != Type::Inferred {
                                    p.ty = arg_ty.clone();
                                }
                                param_types_map.insert(p.name.clone(), p.ty.clone());
                            }
                        }

                        if let Some(spec_sig) = &mut specialized_def.signature {
                            if *spec_sig.return_type == Type::Inferred {
                                let ret_ty = infer_function_return_type(&specialized_def.value, &param_types_map, scope, generated, &mut HashSet::new(), 0)
                                    .unwrap_or(Type::Symbol("none".to_string()));
                                if ret_ty != Type::Never {
                                    spec_sig.return_type = Box::new(ret_ty);
                                }
                            }
                        }

                        generated.push(GeneratedSpecialization {
                            base_name: fn_name.clone(),
                            originating_file: originating_file.clone(),
                            specialized_def,
                        });
                    }

                    rewrite_target_identifier(target, mangled_name);
                    *changed = true;
                }
            }
        }
        Expression::For { iterable, binder, body } => {
            scan_and_monomorphize_def(iterable, scope, def_templates, local_types, generated, changed)?;
            let orig_locals = local_types.clone();
            if let Some(iter_ty) = infer_expr_type_with_generated(iterable, local_types, scope, generated) {
                let resolved = resolve_type_alias_deep(&iter_ty, scope);
                match resolved {
                    Type::Array { element, .. } => {
                        if let Some(b) = binder {
                            local_types.insert(b.clone(), *element);
                        }
                    }
                    Type::GenericInstance { name, type_arguments } if name == "Slice" => {
                        if let Some(elem) = type_arguments.first() {
                            if let Some(b) = binder {
                                local_types.insert(b.clone(), elem.clone());
                            }
                        }
                    }
                    _ => {}
                }
            }
            scan_and_monomorphize_def(body, scope, def_templates, local_types, generated, changed)?;
            *local_types = orig_locals;
        }
        Expression::Repeat { count, body, binder } => {
            scan_and_monomorphize_def(count, scope, def_templates, local_types, generated, changed)?;
            let orig_locals = local_types.clone();
            if let Some(b) = binder {
                local_types.insert(b.clone(), Type::USize);
            }
            scan_and_monomorphize_def(body, scope, def_templates, local_types, generated, changed)?;
            *local_types = orig_locals;
        }
        Expression::RepeatWhile { condition, body } => {
            scan_and_monomorphize_def(condition, scope, def_templates, local_types, generated, changed)?;
            let orig_locals = local_types.clone();
            scan_and_monomorphize_def(body, scope, def_templates, local_types, generated, changed)?;
            *local_types = orig_locals;
        }
        Expression::RepeatUntil { condition, body } => {
            let orig_locals = local_types.clone();
            scan_and_monomorphize_def(body, scope, def_templates, local_types, generated, changed)?;
            scan_and_monomorphize_def(condition, scope, def_templates, local_types, generated, changed)?;
            *local_types = orig_locals;
        }
        Expression::If { arms } => {
            for arm in arms {
                let orig_locals = local_types.clone();
                if let crate::syntax::IfCondition::Expression(cond) = &mut arm.condition {
                    scan_and_monomorphize_def(cond, scope, def_templates, local_types, generated, changed)?;
                }
                scan_and_monomorphize_def(&mut arm.body, scope, def_templates, local_types, generated, changed)?;
                *local_types = orig_locals;
            }
        }
        Expression::Match { target, arms, binding_name } => {
            scan_and_monomorphize_def(target, scope, def_templates, local_types, generated, changed)?;
            if let Some(target_ty) = infer_expr_type_with_generated(target, local_types, scope, generated) {
                let resolved_target = resolve_type_alias_deep(&target_ty, scope);
                if !matches!(resolved_target, Type::Union(_)) {
                    let mut matched_arm_idx = None;
                    for (idx, arm) in arms.iter().enumerate() {
                        if let Some(vt) = &arm.variant_type {
                            if type_matches_pattern(&target_ty, vt, scope) {
                                matched_arm_idx = Some(idx);
                                break;
                            }
                        } else {
                            matched_arm_idx = Some(idx);
                            break;
                        }
                    }

                    if let Some(matched_idx) = matched_arm_idx {
                        let matched_arm = arms[matched_idx].clone();
                        let target_expr = (**target).clone();
                        let b_name = binding_name.clone();

                        let mut rewritten = if b_name.is_empty() {
                            Expression::Block(vec![
                                Expression::Discard(Box::new(target_expr)),
                                *matched_arm.body,
                            ])
                        } else {
                            Expression::Block(vec![
                                Expression::Let {
                                    name: b_name,
                                    value: Box::new(target_expr),
                                },
                                *matched_arm.body,
                            ])
                        };

                        scan_and_monomorphize_def(&mut rewritten, scope, def_templates, local_types, generated, changed)?;
                        *expr = rewritten;
                        *changed = true;
                        return Ok(());
                    }
                }
            }

            let unhandled_variants: Vec<Type> = if let Some(target_ty) = infer_expr_type_with_generated(target, local_types, scope, generated) {
                let resolved = resolve_type_alias_deep(&target_ty, scope);
                if let Type::Union(variants) = resolved {
                    variants
                        .into_iter()
                        .filter(|v| {
                            !arms.iter().any(|a| {
                                if let Some(vt) = &a.variant_type {
                                    type_matches_pattern(v, vt, scope)
                                } else {
                                    false
                                }
                            })
                        })
                        .collect()
                } else {
                    Vec::new()
                }
            } else {
                Vec::new()
            };

            for arm in arms {
                let orig_locals = local_types.clone();
                if let Some(vt) = &arm.variant_type {
                    local_types.insert(binding_name.clone(), vt.clone());
                } else if !unhandled_variants.is_empty() {
                    let narrowed = if unhandled_variants.len() == 1 {
                        unhandled_variants[0].clone()
                    } else {
                        unify_types_to_sum_type(&unhandled_variants, scope)
                    };
                    local_types.insert(binding_name.clone(), narrowed);
                } else if !binding_name.is_empty() {
                    if let Some(target_ty) = infer_expr_type_with_generated(target, local_types, scope, generated) {
                        local_types.insert(binding_name.clone(), target_ty);
                    }
                }
                scan_and_monomorphize_def(&mut arm.body, scope, def_templates, local_types, generated, changed)?;
                *local_types = orig_locals;
            }
        }
        Expression::Binary { left, right, .. } | Expression::Comparison { left, right, .. } => {
            scan_and_monomorphize_def(left, scope, def_templates, local_types, generated, changed)?;
            scan_and_monomorphize_def(right, scope, def_templates, local_types, generated, changed)?;
        }
        Expression::Cast { expression, .. }
        | Expression::Slice(expression)
        | Expression::SizeOf(expression)
        | Expression::AddressOf { expression, .. }
        | Expression::Dereference(expression)
        | Expression::Defer(expression)
        | Expression::Discard(expression)
        | Expression::Return(expression)
        | Expression::Yield(expression) => {
            scan_and_monomorphize_def(expression, scope, def_templates, local_types, generated, changed)?;
        }
        Expression::AsTypeOf { expression, target } => {
            scan_and_monomorphize_def(expression, scope, def_templates, local_types, generated, changed)?;
            scan_and_monomorphize_def(target, scope, def_templates, local_types, generated, changed)?;
        }
        Expression::Index { expression, index } => {
            scan_and_monomorphize_def(expression, scope, def_templates, local_types, generated, changed)?;
            scan_and_monomorphize_def(index, scope, def_templates, local_types, generated, changed)?;
        }
        Expression::Member { expression, .. } => {
            scan_and_monomorphize_def(expression, scope, def_templates, local_types, generated, changed)?;
        }
        Expression::Array(elements) | Expression::Tuple(elements) => {
            for el in elements.iter_mut() {
                scan_and_monomorphize_def(el, scope, def_templates, local_types, generated, changed)?;
            }
            if elements.iter().any(|e| matches!(crate::syntax::unspanned_expression(e), Expression::Spread(_))) {
                let mut expanded = Vec::new();
                let mut all_known = true;
                for el in elements.iter() {
                    if let Expression::Spread(inner) = crate::syntax::unspanned_expression(el) {
                        if let Some(t) = infer_expr_type_with_generated(inner, local_types, scope, generated) {
                            let resolved = resolve_type_alias_deep(&t, scope);
                            match resolved {
                                Type::Array { size, .. } => {
                                    for i in 0..size {
                                        expanded.push(Expression::Index {
                                            expression: inner.clone(),
                                            index: Box::new(Expression::Integer(i as i64)),
                                        });
                                    }
                                }
                                Type::Tuple(tys) => {
                                    for i in 0..tys.len() {
                                        expanded.push(Expression::Member {
                                            expression: inner.clone(),
                                            field: i.to_string(),
                                        });
                                    }
                                }
                                _ => {
                                    all_known = false;
                                    break;
                                }
                            }
                        } else {
                            all_known = false;
                            break;
                        }
                    } else {
                        expanded.push(el.clone());
                    }
                }
                if all_known {
                    *elements = expanded;
                    *changed = true;
                }
            }
        }
        Expression::Struct(fields) => {
            for f in fields.iter_mut() {
                scan_and_monomorphize_def(&mut f.value, scope, def_templates, local_types, generated, changed)?;
            }
            if fields.iter().any(|f| f.name.is_none()) {
                let mut expanded_fields: Vec<StructFieldInit> = Vec::new();
                let mut all_known = true;
                for f in fields.iter() {
                    if f.name.is_none() {
                        if let Some(t) = infer_expr_type_with_generated(&f.value, local_types, scope, generated) {
                            let resolved = resolve_type_alias_deep(&t, scope);
                            match resolved {
                                Type::Struct(s_fields) => {
                                    for sf in s_fields {
                                        let member_expr = Expression::Member {
                                            expression: Box::new(f.value.clone()),
                                            field: sf.name.clone(),
                                        };
                                        if let Some(existing) = expanded_fields.iter_mut().find(|ex| ex.name.as_ref() == Some(&sf.name)) {
                                            existing.value = member_expr;
                                        } else {
                                            expanded_fields.push(StructFieldInit {
                                                name: Some(sf.name),
                                                value: member_expr,
                                            });
                                        }
                                    }
                                }
                                _ => {
                                    all_known = false;
                                    break;
                                }
                            }
                        } else {
                            all_known = false;
                            break;
                        }
                    } else {
                        let name = f.name.as_ref().unwrap();
                        if let Some(existing) = expanded_fields.iter_mut().find(|ex| ex.name.as_ref() == Some(name)) {
                            existing.value = f.value.clone();
                        } else {
                            expanded_fields.push(f.clone());
                        }
                    }
                }
                if all_known {
                    *fields = expanded_fields;
                    *changed = true;
                }
            }
        }
        Expression::Let { value, .. } => {
            scan_and_monomorphize_def(value, scope, def_templates, local_types, generated, changed)?;
        }
        Expression::Var { value, .. } => {
            if let Some(v) = value {
                scan_and_monomorphize_def(v, scope, def_templates, local_types, generated, changed)?;
            }
        }
        Expression::Assign { target, value } => {
            scan_and_monomorphize_def(target, scope, def_templates, local_types, generated, changed)?;
            scan_and_monomorphize_def(value, scope, def_templates, local_types, generated, changed)?;
        }
        Expression::Spread(inner) => {
            scan_and_monomorphize_def(inner, scope, def_templates, local_types, generated, changed)?;
        }
        Expression::Identifier(_)
        | Expression::Integer(_)
        | Expression::String(_)
        | Expression::Symbol(_)
        | Expression::Break
        | Expression::Continue
        | Expression::Context
        | Expression::CallerLineNumber
        | Expression::CallerFileName
        | Expression::ThisLineNumber(_)
        | Expression::ThisFileName(_) => {}
    }
    Ok(())
}

pub fn specialise_def_functions(
    modules: &mut [LogicalModule],
    module_scopes: &mut [ModuleScope],
) -> Result<(), String> {
    let mut changed = true;
    let mut iterations = 0;

    while changed {
        changed = false;
        iterations += 1;
        if iterations > 128 {
            return Err(
                "def function specialisation depth limit exceeded (possible cyclic recursion)"
                    .to_string(),
            );
        }

        let mut def_templates: HashMap<String, (String, Definition)> = HashMap::new();
        for scope in module_scopes.iter() {
            for (name, prov) in &scope.definitions {
                if prov.item.is_def {
                    def_templates.insert(name.clone(), (prov.originating_file.clone(), prov.item.clone()));
                }
            }
        }
        for module in modules.iter() {
            for file in &module.source_files {
                for def in &file.definitions {
                    if def.is_def {
                        def_templates.insert(def.name.clone(), (file.file_path.clone(), def.clone()));
                    }
                }
            }
        }

        if def_templates.is_empty() {
            break;
        }

        let mut all_generated = Vec::new();

        for (module, scope) in modules.iter_mut().zip(module_scopes.iter()) {
            for file in &mut module.source_files {
                for def in &mut file.definitions {
                    if def.is_def {
                        continue;
                    }
                    let mut local_types = HashMap::new();
                    if let Some(sig) = &def.signature {
                        for p in &sig.parameters {
                            local_types.insert(p.name.clone(), p.ty.clone());
                        }
                    }
                    scan_and_monomorphize_def(
                        &mut def.value,
                        scope,
                        &def_templates,
                        &mut local_types,
                        &mut all_generated,
                        &mut changed,
                    )?;
                }
            }
        }

        for item in all_generated {
            let spec_name = item.specialized_def.name.clone();

            for module in modules.iter_mut() {
                for file in &mut module.source_files {
                    if file.file_path == item.originating_file {
                        if !file.definitions.iter().any(|d| d.name == spec_name) {
                            file.definitions.push(item.specialized_def.clone());
                        }
                        break;
                    }
                }
            }

            for scope in module_scopes.iter_mut() {
                if scope.definitions.contains_key(&item.base_name)
                    && !scope.definitions.contains_key(&spec_name)
                {
                    scope.definitions.insert(
                        spec_name.clone(),
                        crate::sema::SymbolProvenance {
                            item: item.specialized_def.clone(),
                            originating_file: item.originating_file.clone(),
                        },
                    );
                }
            }
        }

        for (module, scope) in modules.iter().zip(module_scopes.iter_mut()) {
            for file in &module.source_files {
                for def in &file.definitions {
                    if let Some(prov) = scope.definitions.get_mut(&def.name) {
                        prov.item = def.clone();
                    }
                }
            }
        }
    }

    Ok(())
}

