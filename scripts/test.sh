#!/usr/bin/env bash
# Tests rapides, sans Docker ni réseau : formatage, lint, tests unitaires et jeu d'exemples (mode démo).
set -euo pipefail
cd "$(dirname "$0")/../gateway"
cargo fmt --check
cargo clippy --all-targets --locked -- -D warnings
cargo test --locked
RULES_FILE=../config/rules.json SAMPLES_FILE=../samples/emails.json TRIAGE_MODE=demo cargo run --quiet --locked -- eval
