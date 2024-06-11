#!/bin/bash

# Dependencies:
# - grcov (https://github.com/mozilla/grcov): `cargo install grcov`.
# - llvm-profdata: `rustup component add llvm-tools-preview`.

rm ./target/coverage/data/*.profraw
export CARGO_INCREMENTAL=0
export RUSTFLAGS='-Cinstrument-coverage' 
export LLVM_PROFILE_FILE='./target/coverage/data/cargo-test-%p-%m.profraw'
cargo clean
cargo build
cargo test
grcov ./target/coverage/data/ --binary-path ./target/debug/deps/ -s . -t html --branch --ignore-not-existing --ignore '../*' --ignore "/*" -o target/coverage/html
xdg-open target/coverage/html/index.html
