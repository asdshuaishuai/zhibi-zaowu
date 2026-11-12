#!/usr/bin/env bash
# 双端测试：Rust（客户端）+ novel-pack（能力包）。CI 与本地都跑这一个入口。
#
# 两端读同一份夹具 tests/conformance/cases.json——任一侧改了规则而没改夹具，
# 两侧会同时失败。这正是它存在的意义。
set -euo pipefail
cd "$(dirname "$0")/.."

echo "==> Rust（cargo test）"
cargo test --manifest-path app/Cargo.toml

# node >= 24：优先 .vendor 里的（构建用的那个），否则 PATH
NODE=".vendor/node/bin/node"
[ -x "$NODE" ] || NODE="node"

echo "==> novel-pack（node --test）"
cd novel-pack
"../$NODE" --experimental-strip-types --test "test/*.test.mjs"
