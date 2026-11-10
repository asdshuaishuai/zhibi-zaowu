#!/usr/bin/env bash
# 执笔 3.0 Linux 打包：在 Linux 主机上原生构建 → AppImage（回退 tar.gz）
# 说明：GPUI 的 Linux 图形栈（wayland/x11 + 相关 -dev 包）必须在目标 Linux 本机编译，
#       无法从 macOS 交叉出可用的 GUI 产物。本脚本在 Linux runner 上直接可用。
set -euo pipefail
cd "$(dirname "$0")/.."

echo "==> cargo build --release (linux)"
cargo build --release

APP=target/release/执笔.appimage-src
BIN=target/release/zhibi
if [ ! -f "$BIN" ]; then
  # 某些环境二进制带 .debug 扩展或名字不同，保底找 release 目录
  echo "找不到 $BIN；请确认 cargo 输出名（zhibi）" >&2; exit 1
fi

echo "==> 组装 AppDir"
rm -rf "$APP"
APPDIR="${APP}/zhibi.AppDir"
mkdir -p "$APPDIR/usr/bin"
cp "$BIN" "$APPDIR/usr/bin/zhibi"

cat > "$APPDIR/zhibi.desktop" <<'DESKTOP'
[Desktop Entry]
Type=Application
Name=执笔
Name[zh_CN]=执笔
Comment=跨端 AI 小说写作工作台（GPUI + DSH）
Exec=zhibi
Icon=zhibi
Terminal=false
Categories=Office;TextEditor;
DESKTOP

# 图标：沿用旧版母版（存在则转 png）
ICON=../zhibi/ZhiBi/Sources/Assets.xcassets/AppIcon.appiconset/icon_512x512@2x.png
if [ -f "$ICON" ]; then
  cp "$ICON" "$APPDIR/zhibi.png"
fi

echo "==> AppImage 合成（优先 linuxdeploy/appimagetool，缺则 tar.gz 回退）"
if command -v appimagetool >/dev/null 2>&1; then
  APPIMAGE_ARCH=x86_64 appimagetool "$APPDIR" "target/release/执笔-x86_64.AppImage"
  echo "==> 完成：target/release/执笔-x86_64.AppImage"
elif command -v linuxdeploy >/dev/null 2>&1; then
  linuxdeploy --appdir "$APPDIR" --output appimage -d "$APPDIR/zhibi.desktop"
  mv 执笔*.AppImage target/release/ 2>/dev/null || true
  echo "==> 完成（linuxdeploy）"
else
  tar -C "$APP" -czf target/release/zhibi-linux-x86_64.tar.gz .
  echo "==> 未装 appimagetool/linuxdeploy：产物为 tar.gz（解包后运行 usr/bin/zhibi）"
  echo "    建议：apt install libwayland-dev libx11-dev 等 GPUI 依赖后，"
  echo "    从 https://github.com/AppImage/appimagetool 取 appimagetool 重跑本脚本。"
fi

echo "运行提示：dsh 需 Node>=24.9 + npm i -g @deepseek-ai/dsh；ZHIBI_DSH_BIN 可指定路径。"
