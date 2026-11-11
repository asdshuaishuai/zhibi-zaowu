#!/usr/bin/env bash
# 执笔造物 开发启动：GPUI 前端 + DSH sdk 子进程
#
# 依赖：
# - Node >= 24.9（dsh CLI 的 import.meta.main / 引擎要求），本脚本自动走 nvm 24
# - npm 包 @deepseek-ai/dsh（npm i -g @deepseek-ai/dsh）
# - 模型端点（M1）：
#     DEEPSEEK_API_KEY   深度求索官方路由（deepseek-official）
#     或 ZHIBI_PROVIDER / ZHIBI_MODEL 覆盖（需要 profile 已挂对应适配器）
set -euo pipefail

source ~/.nvm/nvm.sh
nvm use 24 >/dev/null

export ZHIBI_DSH_BIN="${ZHIBI_DSH_BIN:-$(which dsh)}"
export ZHIBI_CWD="${ZHIBI_CWD:-$PWD}"

cd "$(dirname "$0")/../app"
cargo run
