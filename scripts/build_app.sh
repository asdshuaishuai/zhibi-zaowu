#!/usr/bin/env bash
# 执笔 3.0 macOS 打包：release 构建 → .app bundle → ad-hoc 签名
# 跨端说明：Linux/Windows 用同一 cargo 工程直接构建（GPUI 原生支持），
# 打包格式各自不同（deb/AppImage/MSI），脚本留 TODO 桩。
set -euo pipefail
cd "$(dirname "$0")/.."

echo "==> cargo build --release"
cargo build --release

APP=target/release/执笔造物.app
BIN=target/release/zhibi

echo "==> 组装 bundle"
rm -rf "$APP"
mkdir -p "$APP/Contents/MacOS" "$APP/Contents/Resources"
cp "$BIN" "$APP/Contents/MacOS/执笔造物"

cat > "$APP/Contents/Info.plist" <<'PLIST'
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
  <key>CFBundleName</key><string>执笔造物</string>
  <key>CFBundleDisplayName</key><string>执笔造物</string>
  <key>CFBundleIdentifier</key><string>com.zaowu.zhibi</string>
  <key>CFBundleVersion</key><string>3.0.0-dev</string>
  <key>CFBundleShortVersionString</key><string>3.0.0-dev</string>
  <key>CFBundleExecutable</key><string>执笔造物</string>
  <key>CFBundleIconFile</key><string>AppIcon</string>
  <key>CFBundlePackageType</key><string>APPL</string>
  <key>LSMinimumSystemVersion</key><string>13.0</string>
  <key>NSHighResolutionCapable</key><true/>
</dict>
</plist>
PLIST

# 图标：优先用本仓 app/assets/AppIcon.icns（3.0 墨底断刀印），回退旧版母版
if [ -f app/assets/AppIcon.icns ]; then
  cp app/assets/AppIcon.icns "$APP/Contents/Resources/AppIcon.icns"
elif [ -f ../zhibi/ZhiBi/Sources/Assets.xcassets/AppIcon.appiconset/icon_512x512@2x.png ]; then
  sips -s format icns ../zhibi/ZhiBi/Sources/Assets.xcassets/AppIcon.appiconset/icon_512x512@2x.png     --out "$APP/Contents/Resources/AppIcon.icns" >/dev/null 2>&1 || true
fi

# 内嵌运行时：.vendor/{node,dsh}（scripts/vendor_runtime.sh 生成）+ novel-pack
if [ -x .vendor/node/bin/node ] && [ -f .vendor/dsh/lib/bin.js ]; then
  echo "==> 打包内嵌运行时（node + dsh）"
  mkdir -p "$APP/Contents/Resources/runtime"
  cp -R .vendor/node "$APP/Contents/Resources/runtime/node"
  cp -R .vendor/dsh "$APP/Contents/Resources/runtime/dsh"
  cp -R novel-pack "$APP/Contents/Resources/runtime/novel-pack"
  # patch 里的绝对路径换成包内相对（运行时由 ZHIBI_PACK_PATCH 覆盖，保留原样也可）
else
  echo "!! 未找到 .vendor（先跑 scripts/vendor_runtime.sh）；本次不内嵌 dsh，运行时回退全局命令"
fi

echo "==> ad-hoc 签名"
codesign -fs - --deep "$APP" 2>&1 | tail -1

echo "==> 完成：$APP"
echo "运行：open \"$APP\"（DSH 需 Node>=24.9 + npm i -g @deepseek-ai/dsh；"
echo "      可执行文件路径可用 ZHIBI_DSH_BIN 指定，能力包 patch 自动挂载）"
