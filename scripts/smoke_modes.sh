#!/usr/bin/env bash
# 逐页冒烟：用真实长篇 fixture 启动应用，每页存活 N 秒后退出。
# 判据不是截图（需要屏幕录制授权），而是「进程活着 且 日志无 panic」——
# 渲染期 panic 会让进程当场死掉，这是最硬的一条。
set -uo pipefail
cd "$(dirname "$0")/.."

BOOK="${1:-.smoke/冒烟长卷.zhibi}"
SECS="${2:-4}"
BIN="./target/release/zhibi"

if [ ! -x "$BIN" ]; then echo "先构建：cargo build --release"; exit 1; fi
if [ ! -d "$BOOK" ]; then echo "缺 fixture：python3 scripts/make_smoke_fixture.py"; exit 1; fi

MODES=(editor chapters outline canon memory stats clues characters graph canvas director worldline oracle bookinfo search)
fail=0

for m in "${MODES[@]}"; do
  log=".smoke/log-$m.log"
  : > "$log"
  ZHIBI_OPEN="$BOOK" ZHIBI_MODE="$m" "$BIN" >"$log" 2>&1 &
  pid=$!
  sleep "$SECS"

  if ! kill -0 "$pid" 2>/dev/null; then
    wait "$pid"; code=$?
    echo "✗ $m —— 进程已退出（code=$code）"
    grep -iE "panic|error|fatal" "$log" | head -3
    fail=$((fail+1))
    continue
  fi

  if grep -qiE "panicked at|RUST_BACKTRACE" "$log"; then
    echo "✗ $m —— 日志有 panic"
    grep -A3 "panicked at" "$log" | head -6
    fail=$((fail+1))
  else
    # 抓一帧 CPU/内存，顺带看有没有卡死在渲染循环里（持续 100% CPU 是可疑信号）
    stats=$(ps -o %cpu=,rss= -p "$pid" 2>/dev/null | tr -s ' ')
    echo "✓ $m —— 存活 ${SECS}s（cpu/rss:${stats:-n/a}）"
  fi

  kill -TERM "$pid" 2>/dev/null
  wait "$pid" 2>/dev/null
done

echo "---"
if [ "$fail" -eq 0 ]; then echo "全部 ${#MODES[@]} 页冒烟通过"; else echo "$fail 页失败"; fi
exit "$fail"
