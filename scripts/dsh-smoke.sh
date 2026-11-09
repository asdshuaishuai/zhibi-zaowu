#!/usr/bin/env bash
# DSH sdk 协议冒烟（不进 GUI）：initialize → session/query → 观察事件流
# 期望：先回 initialize result，再滚 session.event / session.status 通知，
#       无 API Key 时以 turn/end{kind:"error", error.message=...} 收尾也算通过。
set -euo pipefail
source ~/.nvm/nvm.sh
nvm use 24 >/dev/null

command -v dsh >/dev/null || { echo "先：npm i -g @deepseek-ai/dsh"; exit 1; }

{ printf '{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"cwd":"/tmp","provider":"deepseek-official","model":"deepseek-chat"}}\n'
  sleep 4
  printf '{"jsonrpc":"2.0","id":2,"method":"session/query","params":{"sessionId":"zhibi-smoke","contentBlocks":[{"type":"text","text":"用一句话说你好"}]}}\n'
  sleep 20
} | timeout 40 dsh --profile sdk 2>/dev/null | while IFS= read -r line; do
  type=$(printf '%s' "$line" | sed -E 's/.*"method":"([^"]+)".*/\1/;t;s/.*"result.*/RESPONSE/')
  echo "· $type"
done
echo "冒烟结束（上行只打印方法名，避免刷屏）"
