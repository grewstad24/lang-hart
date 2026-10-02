default:
    @just --list

test:
    cargo test

run *ARGS="":
    cargo run {{ ARGS }}

build:
    cargo build --release

clean:
    cargo clean

install:
    cargo install --path .
