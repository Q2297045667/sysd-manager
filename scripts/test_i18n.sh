#!/bin/sh
set -eu
cd "$(dirname "$0")/.."
python3 -m unittest discover -s tests -p 'test_*.py'
python3 scripts/check_i18n.py
cargo test --workspace
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo check --features flatpak
cargo check --features appimage
cargo run -p transtools -- packfiles
