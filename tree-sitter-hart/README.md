# tree-sitter-hart

Tree-sitter grammar for the [Hart](https://github.com/grewstad24/lang-hart) programming language (`.hart` files).

## Features

- Complete grammar covering all language syntax:
  - Modules, declarations, imports, exports, and namespaces
  - Function declarations, meta functions, and extern functions (with variadic support)
  - Type definitions, primitive types, pointer types, slice types, array types, struct types, and union types
  - Control flow: `if`, `switch`, `match` (with `using`/`with`), `repeat`, and `for`
  - Expressions: assignments, binary operators, unary operators, postfix calls, pipeline method calls (`..`), member access (`.`), dereference (`.*`), address-of (`.&`), index (`[]`), and casts (`as`)
  - Literals: integers, strings with escape sequences, booleans, atoms (`:none`), arrays, structs, records (`@record`), and `@sizeOf`
- Syntax highlighting queries (`queries/highlights.scm`)
- Local scope and definition queries (`queries/locals.scm`)
- Comprehensive test corpus (`test/corpus/`)
- Pre-generated C parser (`src/parser.c`) ready to compile into any editor or language binding

## Building & Testing

To generate the parser and run tests:

```bash
npm install
npm run build
npm test
```

To parse a file:

```bash
npx tree-sitter parse path/to/file.hart
```

