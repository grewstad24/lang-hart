#include "tree_sitter/parser.h"

enum TokenType {
  ARM_TERMINATOR,
};

void *tree_sitter_hart_external_scanner_create(void) { return NULL; }
void tree_sitter_hart_external_scanner_destroy(void *payload) {}
unsigned tree_sitter_hart_external_scanner_serialize(void *payload, char *buffer) { return 0; }
void tree_sitter_hart_external_scanner_deserialize(void *payload, const char *buffer, unsigned length) {}

bool tree_sitter_hart_external_scanner_scan(void *payload, TSLexer *lexer, const bool *valid_symbols) {
  if (!valid_symbols[ARM_TERMINATOR]) {
    return false;
  }

  lexer->mark_end(lexer);

  for (;;) {
    int32_t c = lexer->lookahead;
    if (c == ' ' || c == '\t' || c == '\r') {
      lexer->advance(lexer, true);
    } else if (c == '#') {
      lexer->advance(lexer, true);
      while (lexer->lookahead != '#' && lexer->lookahead != '\n' && lexer->lookahead != 0) {
        lexer->advance(lexer, true);
      }
      if (lexer->lookahead == '#') {
        lexer->advance(lexer, true);
      }
    } else if (c == '\n' || c == '}' || c == 0) {
      lexer->result_symbol = ARM_TERMINATOR;
      return true;
    } else {
      return false;
    }
  }
}

