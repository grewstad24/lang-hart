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

; Types
(primitive_type) @type.builtin
(type_identifier) @type

; Functions
(function_definition
  name: (identifier) @function)

(extern_function_declaration
  name: (identifier) @function)

(call_expression
  function: (identifier) @function.call)

(method_call_expression
  method: (identifier) @function.method.call)

; Parameters & Variables
(parameter
  name: (identifier) @variable.parameter)

(let_declaration
  name: (identifier) @variable)

(var_declaration
  name: (identifier) @variable)

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

; Literals
(integer_literal) @number
(boolean_literal) @constant.builtin.boolean
(atom_literal) @symbol
(string_literal) @string
(escape_sequence) @string.escape

; Comments
(comment) @comment

; Builtins / Intrinsics
"@record" @attribute
"@sizeOf" @function.macro

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
