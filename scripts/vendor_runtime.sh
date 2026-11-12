#!/usr/bin/env bash
# 准备执笔的 DSH 基础运行环境：Node 二进制 + zhibi-runtime（只装 dsh sdk 闭包）。
#
# 用法：scripts/vendor_runtime.sh [--force]
# 产物（均 gitignore，不入库）：
#   .vendor/node/bin/node          与 DSH 匹配的 Node（>=24.9）
#   .vendor/dsh-runtime/           zhibi-runtime 的安装产物（node_modules + 官方 lib）
#
# 与旧版区别：不再复制全局 @deepseek-ai/dsh 整包（758MB，含 web/桌面端 bundle），
# 改为安装 runtime/zhibi-runtime 声明的依赖（约 274MB，不含 web-frontend / client-ui-* /
# acp / experimental-*），并把官方 lib/*.js 原样拷入。DSH 版本号只在
# runtime/zhibi-runtime/package.json 里维护一处，升级时只改那里。
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
VENDOR="$ROOT/.vendor"
RUNTIME_SRC="$ROOT/runtime/zhibi-runtime"
RUNTIME_OUT="$VENDOR/dsh-runtime"
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

# ── 2. 运行环境依赖（npm install，不带 devDependencies）──
echo "==> 安装 zhibi-runtime 依赖"
if [ "$FORCE" = 1 ] || [ ! -d "$RUNTIME_OUT/node_modules" ]; then
  rm -rf "$RUNTIME_OUT"
  mkdir -p "$RUNTIME_OUT"
  cp "$RUNTIME_SRC/package.json" "$RUNTIME_OUT/package.json"
  ( cd "$RUNTIME_OUT" && npm install --omit=dev --no-audit --no-fund --loglevel=error )
fi

# ── 3. 官方 lib/*.js 原样拷入（bin.js 会动态 import 带 hash 的兄弟块，必须整目录）──
UPSTREAM_LIB="$RUNTIME_SRC/lib"
if [ ! -d "$UPSTREAM_LIB" ]; then
  echo "缺少 $UPSTREAM_LIB —— 应从官方 @deepseek-ai/dsh 原样拷贝 lib/ 目录（不含 node_modules）" >&2
  exit 1
fi
rm -rf "$RUNTIME_OUT/lib"
cp -R "$UPSTREAM_LIB" "$RUNTIME_OUT/lib"

# dsh 自身的 package.json 决定 INSTALL_ANCHOR 解析到的版本号与安装范围；
# 用官方那份，而不是我们私有的 package.json。
if [ -f "$RUNTIME_SRC/dsh-upstream-package.json" ]; then
  cp "$RUNTIME_SRC/dsh-upstream-package.json" "$RUNTIME_OUT/package.json.dsh"
fi

if [ ! -f "$RUNTIME_OUT/lib/bin.js" ]; then
  echo "vendor 失败：$RUNTIME_OUT/lib/bin.js 不存在" >&2; exit 1
fi

# ── 4. 冒烟：--help 与 --dump-config 必须都能过 ──
echo "==> 冒烟"
DSH_HOME_PROBE=$(mktemp -d)
DSH_HOME="$DSH_HOME_PROBE" "$VENDOR/node/bin/node" "$RUNTIME_OUT/lib/bin.js" --profile sdk --help >/dev/null
DSH_HOME="$DSH_HOME_PROBE" "$VENDOR/node/bin/node" "$RUNTIME_OUT/lib/bin.js" --profile sdk --dump-config >/dev/null
rm -rf "$DSH_HOME_PROBE"

du -sh "$VENDOR/node" "$RUNTIME_OUT" 2>/dev/null
echo "==> vendor 完成：构建将直接打包进 .app，不再联网"
