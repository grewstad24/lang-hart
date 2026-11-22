default:
    @just run

run:
    cargo run -- build tests/working
    ./main

install:
    cargo install --path .

zed-grammar:
    cd tree-sitter-hart && npx tree-sitter generate
    ~/.local/share/zed/extensions/build/wasi-sdk/bin/clang \
      --target=wasm32-wasip1 \
      -O3 \
      -fPIC \
      -shared \
      -I tree-sitter-hart/src \
      tree-sitter-hart/src/parser.c \
      -o editors/zed/grammars/hart.wasm

