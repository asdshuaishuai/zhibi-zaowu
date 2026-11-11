# 执笔造物 Windows 打包：在 Windows 主机（或 CI windows runner）原生构建
# 用法（PowerShell）：
#   .\scripts\build_app_windows.ps1            # 出 target\release\执笔-portable
#   .\scripts\build_app_windows.ps1 -Installer # 另用 NSIS（makensis）出安装包
# 说明：GPUI 的 Windows 图形栈必须在本机/原生 runner 编译（无法从 macOS 交叉）。
param([switch]$Installer)

$ErrorActionPreference = "Stop"
Set-Location (Split-Path -Parent $PSScriptRoot)

Write-Host "==> cargo build --release (windows)" -ForegroundColor Cyan
cargo build --release

$bin  = "target\release\zhibi.exe"
$out  = "target\release\执笔-portable"
if (-not (Test-Path $bin)) { Write-Error "缺 $bin（确认 workspace target 输出名）"; exit 1 }

Write-Host "==> 组装可移植目录" -ForegroundColor Cyan
if (Test-Path $out) { Remove-Item -Recurse -Force $out }
New-Item -ItemType Directory -Path $out | Out-Null
Copy-Item $bin "$out\zhibi.exe"
# dsh 依赖说明随包（Node>=24.9 + 全局 dsh；或 ZHIBI_DSH_BIN 指向 dsh.cmd 路径）
@"
执笔造物（Windows）
依赖：Node >= 24.9，npm i -g @deepseek-ai/dsh
可选：环境变量 ZHIBI_DSH_BIN 指向 dsh.cmd/dsh.exe
"@ | Set-Content "$out\README.txt" -Encoding UTF8

if ($Installer) {
    $nsis = Get-Command makensis -ErrorAction SilentlyContinue
    if (-not $nsis) {
        Write-Warning "未装 NSIS（makensis）：仅产出可移植目录。安装 NSIS 后重跑 -Installer。"
    } else {
        $nsi = "target\release\zhibi.nsi"
        @"
!include "MUI2.nsh"
Name "执笔"
OutFile "target\release\zhibi-setup.exe"
InstallDir "`$PROGRAMFILES64\执笔"
!insertmacro MUI_PAGE_DIRECTORY
!insertmacro MUI_PAGE_INSTANCES
!insertmacro MUI_UNPAGE_CONFIRM
!insertmacro MUI_UNPAGE_INSTANCES
Section
  RMDir /r "`$INSTDIR"
  File /r "target\release\执笔-portable\*.*" "`$INSTDIR"
  WriteUninstaller "`$INSTDIR\uninst.exe"
  CreateShortCut "`$DESKTOP\执笔.lnk" "`$INSTDIR\zhibi.exe"
SectionEnd
"@ | Set-Content $nsi -Encoding UTF8
        & makensis $nsi
    }
}

Write-Host "==> 完成：$out" -ForegroundColor Green
Write-Host "提示：项目数据仍读写 %USERPROFILE%\Documents\执笔\*.zhibi（与 macOS 双向兼容）。"
