hart_home := env_var_or_default("HART_HOME", env_var("HOME") / ".hart")

default:
    @just run

run:
    cargo run -- build tests/working
    ./main

install:
    cargo install --path .

setup-home:
    #!/usr/bin/env bash
    set -euo pipefail
    HART_DIR="{{hart_home}}"
    echo "Resetting $HART_DIR ... "
    rm -rf "$HART_DIR"
    mkdir -p "$HART_DIR"
    echo "Installing standard library ... "
    cp -r std "$HART_DIR/std"
    echo "Installing Cosmopolitan toolchain ... "
    if [ -d "vendor/cosmocc" ]; then
        cp -r vendor/cosmocc "$HART_DIR/cosmocc"
    elif [ -d "$HOME/.local/opt/cosmocc" ]; then
        cp -r "$HOME/.local/opt/cosmocc" "$HART_DIR/cosmocc"
    else
        echo "error: cosmocc not found in 'vendor/cosmocc' or '$HOME/.local/opt/cosmocc'" >&2
        exit 1
    fi
    echo "Hart home setup complete at $HART_DIR"

reload-std:
    #!/usr/bin/env bash
    set -euo pipefail
    HART_DIR="{{hart_home}}"
    mkdir -p "$HART_DIR"
    rm -rf "$HART_DIR/std"
    cp -r std "$HART_DIR/std"
    if [ ! -d "$HART_DIR/cosmocc" ]; then
        if [ -d "vendor/cosmocc" ]; then
            cp -r vendor/cosmocc "$HART_DIR/cosmocc"
        elif [ -d "$HOME/.local/opt/cosmocc" ]; then
            cp -r "$HOME/.local/opt/cosmocc" "$HART_DIR/cosmocc"
        fi
    fi
    echo "Reloaded standard library into $HART_DIR"

zed-grammar:
    (cd tree-sitter-hart && npx tree-sitter generate)
    mkdir -p editors/zed/grammars
    rm -rf editors/zed/grammars/hart
    zig cc \
      -target wasm32-wasi \
      -mexec-model=reactor \
      -O3 \
      -fPIC \
      -shared \
      -I tree-sitter-hart/src \
      tree-sitter-hart/src/parser.c \
      tree-sitter-hart/src/scanner.c \
      -o editors/zed/grammars/hart.wasm
    cp tree-sitter-hart/queries/highlights.scm editors/zed/languages/hart/highlights.scm
    @echo "Zed grammar compiled successfully to editors/zed/grammars/hart.wasm"

zed-ext:
    cd editors/zed && cargo build --target wasm32-wasip2 --release
    cp editors/zed/target/wasm32-wasip2/release/hart_zed.wasm editors/zed/extension.wasm
    @echo "Zed extension compiled successfully to editors/zed/extension.wasm"

test-grammar:
    cd tree-sitter-hart && npx tree-sitter test

stage: install reload-std zed-grammar zed-ext
    @echo "Staging complete!"

stage-all: install setup-home zed-grammar zed-ext
    @echo "Full staging complete!"
