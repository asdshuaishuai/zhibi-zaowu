//! 执笔 3.0 设计令牌 —— 全站唯一颜色/间距/圆角/排版事实源。
//!
//! 色板对齐 `design/design.css:3-10`（--bg/--panel/--panel2/--line/--text/--dim/--faint/
//! --accent/--green/--blue/--purple/--red 及各 soft）与 `design/workspace.html:18-33`
//! （--ym-* 变量表）；数值档对齐原型间距/圆角/排版规范：
//! 栏内边距 12-22px · 元素间 6-10px · 分组间 14-18px；
//! 卡片 10-12px · 按钮/输入 6-8px · 小徽标 4-5px；
//! 正文 13px/1.55 · 稿纸 14-14.5px/1.9 · 标签 10.5-11px · 标题 15-18px。
//!
//! 用法约定：
//! - 颜色是**函数**（gpui 的 `rgb()`/`rgba()` 不是 const fn，无法做 const 项）：
//!   `.bg(theme::BG())`、`.text_color(theme::TEXT())`、`.border_color(theme::LINE())`、
//!   `.bg(theme::ACCENT_SOFT())`。Rgba 自带 Into<Hsla>，直接传即可。
//!   半透明 token 以 8 位 hex 0xRRGGBBAA 表达，alpha 换算：.14→0x24 .12→0x1F .06→0x0F。
//! - 间距/圆角/字号是 `Pixels` const（px() 为 const fn）：
//!   `.p(theme::PAD_XL)`、`.gap(theme::GAP_MD)`、`rounded(theme::RADIUS_CARD)`、
//!   `.text_size(theme::FONT_BODY)`。
//! - 行高是比例 f32：`.line_height(relative(theme::LINE_HEIGHT_PROSE))`。
//! - 禁止再写裸 rgb(0x…) 字面量；需要新色先加 token。
//! - hover 语言 = 提亮 + 描边转朱砂（`ACCENT_BORDER()`）；选中 = `ACCENT_SOFT()` 底 + `TEXT()` 字；
//!   破坏性 = `RED()`/`RED_SOFT()`；成功 = `GREEN()`/`GREEN_SOFT()`；运行中 = `BLUE()`/`BLUE_SOFT()`。

#![allow(non_snake_case)]

use gpui_kit::{Pixels, Rgba, px, rgb, rgba};

// ---------------------------------------------------------------------------
// 表面与描边（design.css:4 --bg/--panel/--panel2/--line）
// ---------------------------------------------------------------------------

/// 墨底 #17181c —— 页面/画布背景。
pub fn BG() -> Rgba {
    rgb(0x17181C)
}
/// 主面板 #1e2026 —— 卡片、标题栏、侧栏、底栏（比墨底**亮**一档）。
pub fn PANEL() -> Rgba {
    rgb(0x1E2026)
}
/// 次级面板 #24262d —— 输入框底、内嵌块、图标按钮底。
pub fn PANEL2() -> Rgba {
    rgb(0x24262D)
}
/// 稿纸卡 #1b1d22 —— 正文容器专用（介于墨底与面板之间，editor.rs 现值）。
pub fn PROSE_BG() -> Rgba {
    rgb(0x1B1D22)
}
/// 分隔线 #2e3138 —— 1px 描边/分节线。
pub fn LINE() -> Rgba {
    rgb(0x2E3138)
}
/// 弱化分隔线 rgba(46,49,56,.55) —— 表格行间线（outline.html:28）。
pub fn LINE_SOFT() -> Rgba {
    rgba(0x2E31388C)
}

// ---------------------------------------------------------------------------
// 文字（design.css:5 --text/--dim/--faint；workspace.html:33 --ym-ghost）
// ---------------------------------------------------------------------------

/// 正文/宣纸字 #d8d5cf。
pub fn TEXT() -> Rgba {
    rgb(0xD8D5CF)
}
/// 次级文字 #8e8b85 —— 计数、说明。
pub fn DIM() -> Rgba {
    rgb(0x8E8B85)
}
/// 弱文字 #5d5b57 —— 标签、meta、时间戳。
pub fn FAINT() -> Rgba {
    rgb(0x5D5B57)
}
/// 幽灵/禁用文字 #6e6a63（workspace.html:33）。
pub fn GHOST() -> Rgba {
    rgb(0x6E6A63)
}
/// 朱砂实心主按钮上的文字：白。
pub fn ON_ACCENT() -> Rgba {
    rgb(0xFFFFFF)
}

// ---------------------------------------------------------------------------
// 强调朱砂（design.css:5-6 --accent/--accent-soft；描边档位取原型 .4/.45/.5 中值）
// ---------------------------------------------------------------------------

/// 强调朱砂 #c96f4a —— 主按钮实心、选中编号、关键 <b>。
pub fn ACCENT() -> Rgba {
    rgb(0xC96F4A)
}
/// 朱砂软底 rgba(201,111,74,.14) —— 选中/hover/主能力按钮底。
pub fn ACCENT_SOFT() -> Rgba {
    rgba(0xC96F4A24)
}
/// 朱砂描边 rgba(201,111,74,.45) —— hover 提亮后的边框色。
pub fn ACCENT_BORDER() -> Rgba {
    rgba(0xC96F4A73)
}

// ---------------------------------------------------------------------------
// 状态语义色（design.css:7-9；outline.html:23 .g/.o/.r/.p；workspace.html:80 绿描边）
// ---------------------------------------------------------------------------

/// 成功/完成 #7fae72。
pub fn GREEN() -> Rgba {
    rgb(0x7FAE72)
}
/// 成功软底 rgba(127,174,114,.12) —— ok 气泡、绿徽标底。
pub fn GREEN_SOFT() -> Rgba {
    rgba(0x7FAE721F)
}
/// 成功描边 rgba(127,174,114,.35) —— .za-chip.g 边框（design.css:41）。
pub fn GREEN_BORDER() -> Rgba {
    rgba(0x7FAE7259)
}
/// 运行中蓝 #6f9bc4 —— spinner、live 状态。
pub fn BLUE() -> Rgba {
    rgb(0x6F9BC4)
}
/// 蓝软底 rgba(111,155,196,.12) —— livebox/运行活动条底。
pub fn BLUE_SOFT() -> Rgba {
    rgba(0x6F9BC41F)
}
/// 蓝描边 rgba(111,155,196,.4)。
pub fn BLUE_BORDER() -> Rgba {
    rgba(0x6F9BC466)
}
/// 危险/失败 #c0605c —— 破坏性动作、红告警。
pub fn RED() -> Rgba {
    rgb(0xC0605C)
}
/// 危险软底 rgba(192,96,92,.12)。
pub fn RED_SOFT() -> Rgba {
    rgba(0xC0605C1F)
}
/// 放缓/提醒琥珀 #d9a05b（outline.html:23 .o、p2 到期提醒）。
pub fn AMBER() -> Rgba {
    rgb(0xD9A05B)
}
/// 琥珀软底 rgba(217,160,91,.15)。
pub fn AMBER_SOFT() -> Rgba {
    rgba(0xD9A05B26)
}
/// 紫 #a08cc0 —— 次要分类/进度条填充（仅实色，原型未定义紫软底）。
pub fn PURPLE() -> Rgba {
    rgb(0xA08CC0)
}

// ---------------------------------------------------------------------------
// 中性半透明面
// ---------------------------------------------------------------------------

/// 进度条轨道 rgba(255,255,255,.06)（outline.html:20 .bar）。
pub fn BAR_TRACK() -> Rgba {
    rgba(0xFFFFFF0F)
}
/// 中性徽标底 rgba(255,255,255,.07)（outline.html:53 .p5 长线档）。
pub fn CHIP_NEUTRAL() -> Rgba {
    rgba(0xFFFFFF12)
}
/// 封面题字宣纸色 #e8ddca（M8 封面改按 Swift 五式重绘后暂未使用，留作备用）。
#[allow(dead_code)]
pub fn COVER_TEXT() -> Rgba {
    rgb(0xE8DDCA)
}

// ---------------------------------------------------------------------------
// 间距（栏内 12-22 · 元素间 6-10 · 分组间 14-18）
// ---------------------------------------------------------------------------

/// 4px —— 仅用于同一行内紧耦合元素（图标与文字等）。
pub const PAD_XS: Pixels = px(4.);
/// 8px —— 按钮内边距、小元素纵向呼吸。
pub const PAD_SM: Pixels = px(8.);
/// 12px —— 栏内边距下限 / 卡片内边距下限。
pub const PAD_MD: Pixels = px(12.);
/// 16px —— 面板内边距标准值。
pub const PAD_LG: Pixels = px(16.);
/// 20px —— 统一左右栏内边距（原型全部栏 …20px）。
pub const PAD_XL: Pixels = px(20.);
/// 22px —— 页面级外边距上限（shelf .shelf padding 22px 28px）。
pub const PAD_2XL: Pixels = px(22.);

/// 元素间 gap 下限 4px（例外档，默认不用）。
pub const GAP_XS: Pixels = px(4.);
/// 6px —— 列表行距、图标列紧凑档。
pub const GAP_SM: Pixels = px(6.);
/// 8px —— 元素间标准档。
pub const GAP_MD: Pixels = px(8.);
/// 10px —— 元素间上限档（标题栏簇 gap 10px）。
pub const GAP_LG: Pixels = px(10.);
/// 14px —— 分组间下限。
pub const GAP_GROUP_SM: Pixels = px(14.);
/// 16px —— 分组间标准（网格 gap 16px）。
pub const GAP_GROUP: Pixels = px(16.);
/// 18px —— 分组间上限。
pub const GAP_GROUP_LG: Pixels = px(18.);

// ---------------------------------------------------------------------------
// 圆角（卡片 10-12 · 按钮/输入 6-8 · 小徽标 4-5）
// ---------------------------------------------------------------------------

/// 4px —— pace 节奏 chip 等最小徽标。
pub const RADIUS_TAG: Pixels = px(4.);
/// 5px —— 类别小标 .cm。
pub const RADIUS_BADGE: Pixels = px(5.);
/// 6px —— 紧凑按钮/图标按钮 .iconbtn。
pub const RADIUS_BTN_SM: Pixels = px(6.);
/// 8px —— 按钮/输入框标准档。
pub const RADIUS_BTN: Pixels = px(8.);
/// 9px —— 状态 chip / 轨迹气泡 .bub。
pub const RADIUS_CHIP: Pixels = px(9.);
/// 10px —— 面板级浮起块（窗口、cbody、audit 汇总条）。
pub const RADIUS_PANEL: Pixels = px(10.);
/// 12px —— 卡片标准（pcard 12px）。
pub const RADIUS_CARD: Pixels = px(12.);

// ---------------------------------------------------------------------------
// 排版（正文 13/1.55 · 稿纸 14-14.5/1.9 · 标签 10.5-11 · 标题 15-18）
// ---------------------------------------------------------------------------

/// 9.5px —— 档案卡锚点、柱状图两行标签等微字。
pub const FONT_MICRO: Pixels = px(9.5);
/// 10.5px —— chip/pace 标签、表头 th、pmeta。
pub const FONT_TAG: Pixels = px(10.5);
/// 11px —— 等宽码、节标题、次级 meta。
pub const FONT_LABEL: Pixels = px(11.);
/// 11.5px —— 轨迹气泡正文、卡片副 meta。
pub const FONT_META: Pixels = px(11.5);
/// 12.5px —— 卡片正文 pbody、侧栏导航行。
pub const FONT_SMALL: Pixels = px(12.5);
/// 13px —— 全局正文基准。
pub const FONT_BODY: Pixels = px(13.);
/// 14px —— 稿纸正文（配合 serif 字体 + LINE_HEIGHT_PROSE）。
pub const FONT_PROSE: Pixels = px(14.);
/// 15px —— 区块/章标题下档。
pub const FONT_TITLE_SM: Pixels = px(15.);
/// 17px —— 统计大数字。
/// 预留：统计大数字档（复审后未用到，保留给统计页改版）
#[allow(dead_code)]
pub const FONT_STAT: Pixels = px(17.);
/// 18px —— 页标题 h1。
/// 预留：标题档（书架封面改用 30px 大字题签后未用）
#[allow(dead_code)]
pub const FONT_TITLE: Pixels = px(18.);
/// 20px —— 书架大标题。
pub const FONT_PAGE_TITLE: Pixels = px(20.);

/// 正文行高 1.55 —— `.line_height(relative(LINE_HEIGHT_BODY))`。
pub const LINE_HEIGHT_BODY: f32 = 1.55;
/// 稿纸行高 1.9 —— `.line_height(relative(LINE_HEIGHT_PROSE))`。
pub const LINE_HEIGHT_PROSE: f32 = 1.9;

/// UI 字体：原型 -apple-system/PingFang SC（design.css:13）。
pub const FAMILY_UI: &str = "PingFang SC";
/// 衬线字体：章标题/稿纸（原型 "Noto Serif SC","Songti SC"，design.css:16）。macOS 取宋体。
pub const FAMILY_SERIF: &str = "Songti SC";
/// 等宽字体：线号/事件号/日期等台账码（原型 ui-monospace）。
pub const FAMILY_MONO: &str = "Menlo";

// ---------------------------------------------------------------------------
// 布局与组件尺寸
// ---------------------------------------------------------------------------

/// 进度条轨道高 8px（原型 .za-bar，填充同高同圆角 RADIUS_TAG）。
/// 预留：细进度条档（Progress 组件接管后未用）
#[allow(dead_code)]
pub const BAR_H: Pixels = px(8.);
/// 左栏台账宽 196px（workspace.html:65）。
pub const SIDEBAR_W: Pixels = px(196.);
/// 右侧 Dock 宽 302px（workspace.html:120，现实现 340 需回收）。
pub const DOCK_W: Pixels = px(302.);
