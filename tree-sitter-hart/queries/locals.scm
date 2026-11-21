; Scopes
(block) @local.scope
(function_definition) @local.scope
(def_definition) @local.scope
(if_arm) @local.scope
(switch_arm) @local.scope
(match_arm) @local.scope
(repeat_expression) @local.scope

; Definitions
(function_definition
  name: (function_identifier) @local.definition.function)

(def_definition
  name: (function_identifier) @local.definition.function)

(extern_function_declaration
  name: (function_identifier) @local.definition.function)

(parameter
  name: (identifier) @local.definition.parameter)

(def_parameter
  name: (identifier) @local.definition.parameter)

(mut_declaration
  name: (identifier) @local.definition.variable)

(const_declaration
  name: (identifier) @local.definition.variable)

(match_expression
  binder: (identifier) @local.definition.variable)

(repeat_expression
  binder: (identifier) @local.definition.variable)

; References
(identifier) @local.reference

