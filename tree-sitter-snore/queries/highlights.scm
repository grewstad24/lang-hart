; Keywords
[
  "decl"
  "mod"
  "export"
  "import"
  "to"
  "from"
  "fn"
  "meta"
  "extern"
  "type"
  "val"
  "let"
  "var"
  "if"
  "else"
  "switch"
  "match"
  "using"
  "with"
  "repeat"
  "for"
  "return"
  "as"
  "mut"
] @keyword

"not" @keyword.operator

; Types
(primitive_type) @type.builtin
(type_identifier) @type

; Variables & Identifiers (default fallback first)
(identifier) @variable

(let_declaration
  name: (identifier) @variable)

(var_declaration
  name: (identifier) @variable)

(val_declaration
  name: (identifier) @variable)

; Parameters
(parameter
  name: (identifier) @variable.parameter)

(match_expression
  binder: (identifier) @variable.parameter)

(repeat_expression
  binder: (identifier) @variable.parameter)

(for_expression
  binder: (identifier) @variable.parameter)

; Fields
(struct_field_type
  name: (identifier) @property)

(struct_field_init
  name: (identifier) @property)

(field_expression
  field: (identifier) @property)

; Functions (declarations and calls)
(function_identifier) @function

; Literals
(integer_literal) @number
(symbol_literal) @constant
(string_literal) @string
(escape_sequence) @string.escape

; Comments
(comment) @comment

; Builtins / Intrinsics
"@record" @attribute
"@struct" @attribute
"@sizeOf" @function.macro
"@slice" @function.macro
"@atom" @function.macro
"@generalise" @function.macro

; Operators & Delimiters
[
  ":="
  ":"
  "->"
  "=>"
  ".."
  "."
  "*"
  "&"
  "|"
  "+"
  "-"
  "/"
  "%"
  "="
  "!="
  "<"
  "<="
  ">"
  ">="
] @operator

(variadic_parameter) @punctuation.delimiter

[
  "("
  ")"
  "["
  "]"
  "{"
  "}"
] @punctuation.bracket

[
  ","
  ";"
] @punctuation.delimiter

; Namespace paths
(namespace_path) @module
(decl_module name: (identifier) @module)
(mod_declaration name: (identifier) @module)
