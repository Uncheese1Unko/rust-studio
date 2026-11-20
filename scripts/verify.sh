#!/usr/bin/env bash
# ローカル検証を一括で実行する。CI（.github/workflows/ci.yml、追加は
# PBI #6 で対応中）と揃える検証内容を、CI が無い環境でも同じ順序・
# 同じ条件で回すための入口。
#
# 使い方: ./scripts/verify.sh
# 終了コード: どれか1つでも失敗したら非 0。
set -euo pipefail
cd "$(dirname "$0")/.."

echo "== cargo fmt --check =="
cargo fmt --check

echo "== cargo clippy --workspace --all-targets -- -D warnings =="
cargo clippy --workspace --all-targets -- -D warnings

echo "== cargo test --workspace =="
cargo test --workspace

echo "verify: OK"
