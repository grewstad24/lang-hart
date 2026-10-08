default:
    @just run

run:
    cargo run -- tests/working
    ./main

install:
    cargo install --path .
