; Scopes
(block) @local.scope
(function_definition) @local.scope
(if_arm) @local.scope
(switch_arm) @local.scope
(match_arm) @local.scope
(repeat_expression) @local.scope
(for_expression) @local.scope

; Definitions
(function_definition
  name: (function_identifier) @local.definition)

(extern_function_declaration
  name: (function_identifier) @local.definition)

(parameter
  name: (identifier) @local.definition)

(let_declaration
  name: (identifier) @local.definition)

(var_declaration
  name: (identifier) @local.definition)

(match_expression
  binder: (identifier) @local.definition)

(repeat_expression
  binder: (identifier) @local.definition)

(for_expression
  binder: (identifier) @local.definition)

; References
(identifier) @local.reference
