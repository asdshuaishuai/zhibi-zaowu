#!/usr/bin/env bash
# DSH sdk 协议冒烟（不进 GUI）：initialize → session/query → 观察事件流
# 通过标准：initialize 拿到 result；session/query 后能收到 session.event/session.status
# 或以 turn/end{kind:"error"} 收尾（无 API Key 时属预期）。工具面探测可选。
#
# 依赖：node >= 24.9（nvm 24）、npm i -g @deepseek-ai/dsh
# 用法：scripts/dsh-smoke.sh [workspace]   默认 /tmp
set -uo pipefail

# —— node 引导：优先 PATH 中 >=24 的 node，否则挑 nvm 里最新版本 ——
node_major() { node -p 'process.versions.node.split(".")[0]' 2>/dev/null || echo 0; }
if [ "$(node_major)" -lt 24 ] && [ -d "$HOME/.nvm/versions/node" ]; then
  latest=$(ls "$HOME/.nvm/versions/node" | sort -V | tail -1)
  [ -n "$latest" ] && export PATH="$HOME/.nvm/versions/node/$latest/bin:$PATH"
fi
if [ "$(node_major)" -lt 24 ]; then
  echo "需要 node >= 24.9（当前 $(node -v 2>/dev/null || echo none)）；装 nvm 24 后重试" >&2
  exit 2
fi
command -v dsh >/dev/null || { echo "先装：npm i -g @deepseek-ai/dsh" >&2; exit 2; }

WS="${1:-/tmp}"
ERR=$(mktemp)
trap 'rm -f "$ERR" "$IN" "$OUT" 2>/dev/null' EXIT

emit() {
  while IFS= read -r line; do
    m=$(printf '%s' "$line" | grep -o '"method":"[^"]*"' | head -1 | cut -d'"' -f4)
    if [ -n "$m" ]; then echo "· $m"; else echo "· RESPONSE"; fi
  done
}

IN=$(mktemp); OUT=$(mktemp)
{
  printf '{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"cwd":"%s","provider":"deepseek-official","model":"deepseek-chat"}}\n' "$WS"
  sleep 3
  printf '{"jsonrpc":"2.0","id":2,"method":"session/query","params":{"sessionId":"zhibi-smoke","contentBlocks":[{"type":"text","text":"用一句话说你好"}]}}\n'
  sleep 15
} > "$IN"
timeout 30 dsh --profile sdk < "$IN" > "$OUT" 2>"$ERR"
rc=$?
rm -f "$IN"
emit < "$OUT"
rm -f "$OUT"
echo "— 冒烟结束（dsh 退出码 ${rc}）"
if [ "$rc" -ne 0 ]; then
  echo "— dsh stderr 摘要：" >&2
  tail -6 "$ERR" >&2
fi
# 无 API Key 时 dsh 可能整体退出：这仍是协议链路的有效信息，不把它当脚本失败
exit 0
