; ============================================================
; Keywords
; ============================================================

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
] @keyword

"not" @keyword.operator


; ============================================================
; Types
; ============================================================

(primitive_type) @type.builtin
(type_identifier) @type


; ============================================================
; Variables & Identifiers (default fallback first)
; ============================================================

(identifier) @variable

(let_declaration
  name: (identifier) @variable)

(var_declaration
  name: (identifier) @variable)

(parameter
  name: (identifier) @variable.parameter)

(match_expression
  binder: (identifier) @variable.parameter)

(repeat_expression
  binder: (identifier) @variable.parameter)

(for_expression
  binder: (identifier) @variable.parameter)


; ============================================================
; Struct Fields and Properties
; ============================================================

(struct_field_type
  name: (identifier) @property)

(struct_field_init
  name: (identifier) @property)

(field_expression
  field: (identifier) @property)


; ============================================================
; Functions (declarations and calls)
; ============================================================

(function_identifier) @function


; ============================================================
; Literals
; ============================================================

(integer_literal) @number

(boolean_literal) @constant.builtin.boolean

(atom_literal) @constant

(string_literal) @string

(escape_sequence) @string.escape


; ============================================================
; Comments
; ============================================================

(comment) @comment


; ============================================================
; Compile-time Intrinsics
; ============================================================

"@record" @attribute
"@sizeOf" @attribute


; ============================================================
; Operators
; ============================================================

[
  ":="
  "->"
  "=>"
  ".."
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


; ============================================================
; Punctuation
; ============================================================

[
  ":"
  "."
] @punctuation.delimiter

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


; ============================================================
; Modules and Namespaces
; ============================================================

(namespace_path) @module

(decl_module
  name: (identifier) @module)

(mod_declaration
  name: (identifier) @module)
