#!/usr/bin/env bash
# 一次性 vendor 运行时：把 dsh（含全部依赖）与 Node 二进制复制到 .vendor/，
# 之后构建直接把两者打进 .app，不再联网、不再依赖全局 npm。
#
# 用法：scripts/vendor_runtime.sh [--force]
# 产物（.vendor/ 已 gitignore，体积约 1GB，不适合入库）：
#   .vendor/node/bin/node    与 dsh 匹配的 Node（>=24.9）
#   .vendor/dsh/             @deepseek-ai/dsh 完整安装（lib + node_modules）
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
VENDOR="$ROOT/.vendor"
NODE_VERSION="24.22.2"
FORCE=0
[ "${1:-}" = "--force" ] && FORCE=1
mkdir -p "$VENDOR"

# ── 1. Node 运行时（优先本机 nvm，其次 nodejs.org 下载一次）──
if [ "$FORCE" = 1 ] || [ ! -x "$VENDOR/node/bin/node" ]; then
  echo "==> vendor Node $NODE_VERSION"
  CAND="$HOME/.nvm/versions/node/v$NODE_VERSION/bin/node"
  mkdir -p "$VENDOR/node/bin"
  if [ -x "$CAND" ]; then
    cp "$CAND" "$VENDOR/node/bin/node"
  else
    TMP=$(mktemp -d)
    ARCH=$(uname -m); case "$ARCH" in arm64) A=darwin-arm64 ;; *) A=darwin-x64 ;; esac
    curl -fsSL "https://nodejs.org/dist/v$NODE_VERSION/node-v$NODE_VERSION-$A.tar.gz" -o "$TMP/node.tgz"
    tar -xzf "$TMP/node.tgz" -C "$TMP"
    cp "$TMP/node-v$NODE_VERSION-$A/bin/node" "$VENDOR/node/bin/node"
    rm -rf "$TMP"
  fi
  chmod +x "$VENDOR/node/bin/node"
fi

# ── 2. dsh：从本机全局安装复制（无网络）；找不到才提示先装 ──
if [ "$FORCE" = 1 ] || [ ! -f "$VENDOR/dsh/lib/bin.js" ]; then
  SRC=""
  for d in "$HOME"/.nvm/versions/node/*/lib/node_modules/@deepseek-ai/dsh \
           /usr/local/lib/node_modules/@deepseek-ai/dsh \
           /opt/homebrew/lib/node_modules/@deepseek-ai/dsh; do
    [ -f "$d/lib/bin.js" ] && SRC="$d" && break
  done
  if [ -z "$SRC" ]; then
    echo "本机没有全局安装 @deepseek-ai/dsh。先跑一次：" >&2
    echo "  npm i -g @deepseek-ai/dsh@0.2.2-rc.1" >&2
    exit 1
  fi
  echo "==> vendor dsh：$SRC"
  rm -rf "$VENDOR/dsh"
  # dsh 包内自带 node_modules（生产依赖已扁平安装），整包复制即可
  cp -R "$SRC" "$VENDOR/dsh"
fi

if [ ! -f "$VENDOR/dsh/lib/bin.js" ]; then
  echo "vendor 失败：.vendor/dsh/lib/bin.js 不存在" >&2; exit 1
fi

"$VENDOR/node/bin/node" "$VENDOR/dsh/lib/bin.js" --version 2>&1 | head -1
du -sh "$VENDOR" 2>/dev/null
echo "==> vendor 完成：构建将直接打包进 .app，不再联网"
