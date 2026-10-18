#!/bin/sh
set -eu
cd "$(dirname "$0")/.."
cargo run -p transtools -- check
cargo test --workspace
cargo test --release -p transtools validation::tests
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo check --features flatpak
cargo check --features appimage
cargo run -p transtools -- packfiles
