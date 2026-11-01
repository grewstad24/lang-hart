use crate::syntax::{Expression, Lexer, Parser, Token, format_error};

#[derive(Debug, Clone, PartialEq)]
pub struct ExportRule {
    pub symbols: Vec<String>,
    pub to: Vec<String>,
    pub span: crate::syntax::Span,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ModConfig {
    pub exports: Vec<ExportRule>,
    pub imports: Vec<Vec<String>>,
}

impl ModConfig {
    pub fn parse_and_validate(source: &str, file_path: &str) -> Result<Self, String> {
        let (tokens, spans) =
            Lexer::new_with_file(source.to_string(), file_path).tokenize_with_spans()?;

        let mut parser = Parser::new_with_spans(tokens, spans);
        let parsed = parser.parse_struct_fields(Token::Eof);
        let fields = parsed.map_err(|error| format_error(parser.current_span(), &error))?;

        let mut exports = Vec::new();
        let mut imports = Vec::new();

        for field in fields {
            Self::assert_constant_data_expr(&field.value, file_path)?;

            match field.name.as_str() {
                "export" => {
                    exports = Self::parse_export_directive(&field.value, file_path)?;
                }
                "import" => {
                    imports = Self::parse_import_directive(&field.value, file_path)?;
                }
                other => {
                    let error = format!("unknown directive in mod.yawn: '@{}'", other);
                    if let Expression::Spanned { span, .. } = &field.value {
                        return Err(format_error(span, &error));
                    }
                    return Err(format!("{}: error: {}", file_path, error));
                }
            }
        }

        Ok(Self { exports, imports })
    }

    fn assert_constant_data_expr(expr: &Expression, file_path: &str) -> Result<(), String> {
        if let Expression::Spanned { expression, span } = expr {
            return Self::assert_constant_data_expr(expression, file_path)
                .map_err(|error| format_error(span, &error));
        }
        match expr {
            Expression::Integer(_)
            | Expression::String(_)
            | Expression::Boolean(_)
            | Expression::Atom(_)
            | Expression::Identifier(_) => Ok(()),
            Expression::Array(elements) => {
                for el in elements {
                    Self::assert_constant_data_expr(el, file_path)?;
                }
                Ok(())
            }
            Expression::Struct(fields) => {
                for f in fields {
                    Self::assert_constant_data_expr(&f.value, file_path)?;
                }
                Ok(())
            }
            Expression::Block { .. } => Err(format!(
                "in '{}': functions ('do...done') are prohibited in mod.yawn",
                file_path
            )),
            Expression::Cast { .. } => Err(format!(
                "in '{}': type casts ('as') are prohibited in mod.yawn",
                file_path
            )),
            Expression::Call { .. } => Err(format!(
                "in '{}': function and method calls are prohibited in mod.yawn",
                file_path
            )),
            _ => Err(format!(
                "in '{}': dynamic expressions are prohibited in mod.yawn",
                file_path
            )),
        }
    }

    /// Enforces: @export must be strictly an Array of Structs:
    /// `[] ( @symbol []String, @to []String )`
    fn parse_export_directive(
        expr: &Expression,
        file_path: &str,
    ) -> Result<Vec<ExportRule>, String> {
        if let Expression::Spanned { expression, span } = expr {
            return Self::parse_export_directive(expression, file_path)
                .map_err(|error| format_error(span, &error));
        }
        match expr {
            Expression::Array(elements) => {
                let mut rules = Vec::with_capacity(elements.len());
                for el in elements {
                    let rule_span = match el {
                        Expression::Spanned { span, .. } => span.clone(),
                        _ => match expr {
                            Expression::Spanned { span, .. } => span.clone(),
                            _ => unreachable!(),
                        },
                    };
                    match crate::syntax::unspanned_expression(el) {
                        Expression::Struct(fields) => {
                            let mut symbols = None;
                            let mut to = None;

                            for field in fields {
                                match field.name.as_str() {
                                    "symbols" | "symbol" => {
                                        symbols = Some(Self::parse_string_array(
                                            &field.value,
                                            file_path,
                                            "@symbol",
                                        )?);
                                    }
                                    "to" => {
                                        to = Some(Self::parse_string_array(
                                            &field.value,
                                            file_path,
                                            "@to",
                                        )?);
                                    }
                                    other => {
                                        return Err(format!(
                                            "in '{}': unknown export field '@{}', expected '@symbol' or '@to'",
                                            file_path, other
                                        ));
                                    }
                                }
                            }

                            let symbols = symbols.ok_or_else(|| {
                                format!(
                                    "in '{}': export struct missing required field '@symbol'",
                                    file_path
                                )
                            })?;
                            let to = to.ok_or_else(|| {
                                format!(
                                    "in '{}': export struct missing required field '@to'",
                                    file_path
                                )
                            })?;

                            rules.push(ExportRule {
                                symbols,
                                to,
                                span: rule_span,
                            });
                        }
                        _ => {
                            return Err(format!(
                                "in '{}': type mismatch for @export: expected array of structs '[]( @symbol []String, @to []String )'",
                                file_path
                            ));
                        }
                    }
                }
                Ok(rules)
            }
            _ => Err(format!(
                "in '{}': type mismatch for @export: expected array of structs '[]( @symbol []String, @to []String )'",
                file_path
            )),
        }
    }

    /// Enforces: @import must be strictly an Array of Arrays of Strings:
    /// `[][]String`
    fn parse_import_directive(
        expr: &Expression,
        file_path: &str,
    ) -> Result<Vec<Vec<String>>, String> {
        if let Expression::Spanned { expression, span } = expr {
            return Self::parse_import_directive(expression, file_path)
                .map_err(|error| format_error(span, &error));
        }
        match expr {
            Expression::Array(elements) => {
                let mut imports = Vec::with_capacity(elements.len());
                for el in elements {
                    let path = Self::parse_string_array(el, file_path, "import entry")?;
                    imports.push(path);
                }
                Ok(imports)
            }
            _ => Err(format!(
                "in '{}': type mismatch for @import: expected array of string arrays '[][]String'",
                file_path
            )),
        }
    }

    /// Enforces: value must strictly be `[]String` (an array where every element is an identifier, atom, or string)
    fn parse_string_array(
        expr: &Expression,
        file_path: &str,
        context: &str,
    ) -> Result<Vec<String>, String> {
        if let Expression::Spanned { expression, span } = expr {
            return Self::parse_string_array(expression, file_path, context)
                .map_err(|error| format_error(span, &error));
        }
        match expr {
            Expression::Array(elements) => elements
                .iter()
                .map(|e| Self::extract_string(e, file_path, context))
                .collect(),
            _ => Err(format!(
                "in '{}': expected {} to be an array of strings '[]String'",
                file_path, context
            )),
        }
    }

    fn extract_string(expr: &Expression, file_path: &str, context: &str) -> Result<String, String> {
        if let Expression::Spanned { expression, span } = expr {
            return Self::extract_string(expression, file_path, context)
                .map_err(|error| format_error(span, &error));
        }
        match expr {
            Expression::Identifier(name) | Expression::Atom(name) => Ok(name.clone()),
            Expression::String(bytes) => String::from_utf8(bytes.clone())
                .map_err(|_| format!("in '{}': invalid UTF-8 in {}", file_path, context)),
            _ => Err(format!(
                "in '{}': expected string or identifier in {}, found {:?}",
                file_path, context, expr
            )),
        }
    }
}
