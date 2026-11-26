; Keywords
[
  "pub"
  "mod"
  "use"
  "func"
  "def"
  "extern"
  "type"
  "const"
  "if"
  "else"
  "switch"
  "match"
  "using"
  "repeat"
  "for"
  "while"
  "until"
  "defer"
  "yield"
  "return"
  "break"
  "continue"
  "as"
  "mut"
] @keyword

; Types
(primitive_type) @type.builtin
(type_identifier) @type
(qualified_type_identifier) @type

; Variables & Identifiers (default fallback first)
(identifier) @variable

(mut_declaration
  name: (identifier) @variable)

(const_declaration
  name: (identifier) @variable)

; Parameters
(parameter
  name: (identifier) @variable.parameter)

(def_parameter
  name: (identifier) @variable.parameter)

(match_expression
  binder: (identifier) @variable.parameter)

(repeat_expression
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
"@sizeOf" @function.macro
"@asTypeOf" @function.macro
(context_expression) @variable.builtin
(caller_line_expression) @variable.builtin
(caller_file_expression) @variable.builtin
(this_line_expression) @variable.builtin
(this_file_expression) @variable.builtin

; Operators & Delimiters
[
  ":="
  ":"
  "->"
  "=>"
  "."
  "*"
  "&"
  "&mut"
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
  "::"
  "::*"
] @punctuation.delimiter

; Module paths
(mod_declaration name: (module_path) @module)
(use_directive path: (module_path) @module)
(use_directive alias: (identifier) @module)
