use std::collections::HashMap;

use crate::sema::ModuleScope;
use crate::syntax::{
    Definition, Expression, LogicalModule, Parameter, StructFieldType, Type, TypeAlias,
};

pub fn mangle_type(ty: &Type) -> String {
    match ty {
        Type::Never => "never".to_string(),
        Type::Opaque => "opaque".to_string(),
        Type::MetaType => "metatype".to_string(),
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
        Expression::Identifier(name) => {
            if let Some(concrete) = substitutions.get(name) {
                *expr = Expression::Type(concrete.clone());
            }
        }
        Expression::SizeOf(ty) => {
            *ty = substitute_type(ty, substitutions);
        }
        Expression::Slice(expr) => {
            substitute_expression(expr, substitutions);
        }
        Expression::Type(ty) => {
            *ty = substitute_type(ty, substitutions);
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
        Expression::Unary { operand, .. } => {
            substitute_expression(operand, substitutions);
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
        Expression::Discard(operand) => {
            substitute_expression(operand, substitutions);
        }
        Expression::Return(operand) => {
            substitute_expression(operand, substitutions);
        }
        Expression::Repeat { count, body, .. } => {
            substitute_expression(count, substitutions);
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
        Expression::Record(fields) => {
            for f in fields {
                substitute_expression(&mut f.value, substitutions);
            }
        }
        Expression::Tuple(elements) => {
            for el in elements {
                substitute_expression(el, substitutions);
            }
        }
        Expression::Integer(_)
        | Expression::String(_)
        | Expression::Symbol(_) => {}
    }
}

fn extract_type_argument(arg: &Expression, _scope: &ModuleScope) -> Result<Type, String> {
    match arg {
        Expression::Spanned { expression, .. } => extract_type_argument(expression, _scope),
        Expression::Type(ty) => Ok(ty.clone()),
        Expression::Identifier(name) => Err(format!(
            "expected explicit 'type' keyword before type argument, found '{}' (use 'type {}')",
            name, name
        )),
        _ => {
            Err("expected explicit 'type' keyword for type argument (e.g. 'type s32')".to_string())
        }
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

struct GeneratedSpecialization {
    base_name: String,
    originating_file: String,
    specialized_def: Definition,
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
            // First rewrite inner arguments and target
            for arg in arguments.iter_mut() {
                scan_and_rewrite_expression(arg, scope, generated, changed)?;
            }
            scan_and_rewrite_expression(target, scope, generated, changed)?;

            if let Some(fn_name) = unwrap_identifier_name(target) {
                if let Some(def_prov) = scope.definitions.get(&fn_name) {
                    if def_prov.item.is_meta {
                        let template = &def_prov.item;
                        let sig = template
                            .signature
                            .as_ref()
                            .ok_or_else(|| format!("meta fn '{}' has no signature", fn_name))?;

                        if arguments.len() != sig.parameters.len() {
                            return Err(format!(
                                "meta fn '{}' expects {} arguments, found {}",
                                fn_name,
                                sig.parameters.len(),
                                arguments.len()
                            ));
                        }

                        let mut substitutions = HashMap::new();
                        let mut type_arg_mangles = Vec::new();

                        for (param, arg) in sig.parameters.iter().zip(arguments.iter()) {
                            if param.ty == Type::MetaType {
                                let type_arg = extract_type_argument(arg, scope)?;
                                type_arg_mangles.push(mangle_type(&type_arg));
                                substitutions.insert(param.name.clone(), type_arg);
                            }
                        }

                        let mangled_suffix = type_arg_mangles.join("__");
                        let mangled_name =
                            sanitize_identifier(&format!("{}__{}", fn_name, mangled_suffix));

                        // Instantiate specialization
                        let mut specialized_def = template.clone();
                        specialized_def.name = mangled_name.clone();
                        specialized_def.is_meta = false;

                        if let Some(spec_sig) = &mut specialized_def.signature {
                            let mut new_params = Vec::new();
                            for p in &spec_sig.parameters {
                                if p.ty != Type::MetaType {
                                    new_params.push(Parameter {
                                        name: p.name.clone(),
                                        ty: substitute_type(&p.ty, &substitutions),
                                    });
                                }
                            }
                            spec_sig.parameters = new_params;
                            spec_sig.return_type =
                                Box::new(substitute_type(&spec_sig.return_type, &substitutions));
                        }

                        substitute_expression(&mut specialized_def.value, &substitutions);

                        generated.push(GeneratedSpecialization {
                            base_name: fn_name.clone(),
                            originating_file: def_prov.originating_file.clone(),
                            specialized_def,
                        });

                        // Rewrite call site
                        rewrite_target_identifier(target, mangled_name);

                        // Remove meta arguments
                        let old_args = std::mem::take(arguments);
                        let mut new_args = Vec::new();
                        for (arg, param) in old_args.into_iter().zip(sig.parameters.iter()) {
                            if param.ty != Type::MetaType {
                                new_args.push(arg);
                            }
                        }
                        *arguments = new_args;

                        *changed = true;
                    }
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
        Expression::Unary { operand, .. } => {
            scan_and_rewrite_expression(operand, scope, generated, changed)?;
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
        Expression::Discard(operand) => {
            scan_and_rewrite_expression(operand, scope, generated, changed)?;
        }
        Expression::Return(operand) => {
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
        Expression::Record(fields) => {
            for f in fields {
                scan_and_rewrite_expression(&mut f.value, scope, generated, changed)?;
            }
        }
        Expression::Tuple(elements) => {
            for el in elements {
                scan_and_rewrite_expression(el, scope, generated, changed)?;
            }
        }
        Expression::SizeOf(_)
        | Expression::Type(_)
        | Expression::Identifier(_)
        | Expression::Integer(_)
        | Expression::String(_)
        | Expression::Symbol(_) => {}
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
                "monomorphization depth limit exceeded (possible cyclic meta function recursion)"
                    .to_string(),
            );
        }

        let mut all_generated = Vec::new();

        for (module, scope) in modules.iter_mut().zip(module_scopes.iter()) {
            for file in &mut module.source_files {
                for def in &mut file.definitions {
                    if def.is_meta {
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

            // Insert provenance into every module scope that knows about the base meta function
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
        Expression::SizeOf(ty) => {
            concretize_type(ty, templates, generated, changed)?;
        }
        Expression::Slice(operand) => {
            concretize_expression(operand, templates, generated, changed)?;
        }
        Expression::Type(ty) => {
            concretize_type(ty, templates, generated, changed)?;
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
        Expression::Unary { operand, .. } => {
            concretize_expression(operand, templates, generated, changed)?;
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
        Expression::Discard(e) => {
            concretize_expression(e, templates, generated, changed)?;
        }
        Expression::Return(e) => {
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
        Expression::Record(fields) => {
            for f in fields {
                concretize_expression(&mut f.value, templates, generated, changed)?;
            }
        }
        Expression::Tuple(elements) => {
            for el in elements {
                concretize_expression(el, templates, generated, changed)?;
            }
        }
        Expression::Integer(_)
        | Expression::String(_)
        | Expression::Symbol(_)
        | Expression::Identifier(_) => {}
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::syntax::StructFieldType;

    #[test]
    fn test_mangle_types() {
        assert_eq!(mangle_type(&Type::S32), "s32");
        assert_eq!(mangle_type(&Type::USize), "usize");
        assert_eq!(
            mangle_type(&Type::Pointer {
                element: Box::new(Type::U8),
                is_mut: false,
            }),
            "ptr_u8"
        );
        assert_eq!(
            mangle_type(&Type::GenericInstance {
                name: "Slice".to_string(),
                type_arguments: vec![Type::U8],
            }),
            "Slice__u8"
        );

        let struct_type = Type::Struct(vec![
            StructFieldType {
                name: "id".to_string(),
                ty: Type::S32,
            },
            StructFieldType {
                name: "name".to_string(),
                ty: Type::Alias("String".to_string()),
            },
        ]);
        assert_eq!(mangle_type(&struct_type), "struct_id_s32_name_String");
    }

    #[test]
    fn test_substitute_type() {
        let mut substitutions = HashMap::new();
        substitutions.insert("T".to_string(), Type::S32);

        let generic_ptr = Type::Pointer {
            element: Box::new(Type::Alias("T".to_string())),
            is_mut: false,
        };
        let concrete = substitute_type(&generic_ptr, &substitutions);
        assert_eq!(
            concrete,
            Type::Pointer {
                element: Box::new(Type::S32),
                is_mut: false,
            }
        );
    }

    #[test]
    fn test_explicit_type_arguments_required() {
        let mut scope = ModuleScope::new("test".to_string());
        scope.definitions.insert(
            "heap_new".to_string(),
            crate::sema::SymbolProvenance {
                originating_file: "test.zzz".to_string(),
                item: Definition {
                    name: "heap_new".to_string(),
                    signature: Some(crate::syntax::Signature {
                        parameters: vec![
                            Parameter {
                                name: "T".to_string(),
                                ty: Type::MetaType,
                            },
                            Parameter {
                                name: "val".to_string(),
                                ty: Type::Alias("T".to_string()),
                            },
                        ],
                        return_type: Box::new(Type::Pointer {
                            element: Box::new(Type::Alias("T".to_string())),
                            is_mut: false,
                        }),
                        is_variadic: false,
                    }),
                    value: Expression::Symbol("none".to_string()),
                    is_meta: true,
                },
            },
        );

        let mut generated = Vec::new();
        let mut changed = false;

        // Calling heap_new(5) with 1 argument when 2 are expected must fail
        let mut call_missing_type = Expression::Call {
            target: Box::new(Expression::Identifier("heap_new".to_string())),
            arguments: vec![Expression::Integer(5)],
        };
        assert!(
            scan_and_rewrite_expression(
                &mut call_missing_type,
                &scope,
                &mut generated,
                &mut changed
            )
            .is_err()
        );

        // Calling heap_new(s32, 5) with both arguments succeeds
        let mut call_explicit = Expression::Call {
            target: Box::new(Expression::Identifier("heap_new".to_string())),
            arguments: vec![Expression::Type(Type::S32), Expression::Integer(5)],
        };
        assert!(
            scan_and_rewrite_expression(&mut call_explicit, &scope, &mut generated, &mut changed)
                .is_ok()
        );
    }
}
