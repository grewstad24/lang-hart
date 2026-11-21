default:
    @just run

run:
    cargo run -- build tests/working
    ./main

install:
    cargo install --path .
