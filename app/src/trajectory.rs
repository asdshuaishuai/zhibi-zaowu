//! 右侧 Agent 轨迹面板（Dock 面板）。
//!
//! 数据源：DSH `session.event` 全量事件流，按「一次 prompt = 一个轮次」分节。
//! 展示为对话式时间线：轮次分隔头（`.runhead`）+ 工具调用/结果/文本产出气泡 + 完成/失败收尾。
//! 新事件进入时自动滚到底部。
//!
//! 视觉全部取自 `crate::theme` 令牌（无裸 `rgb(0x…)`，间距/圆角/字号不混档）：
//! - 层次（规则 4）：滚动底 = `BG()` 墨底；中性气泡 = `PANEL()` 亮一档 + `LINE()` 1px；
//!   语义气泡 = `ACCENT_SOFT`/`GREEN_SOFT`/`RED_SOFT` 底 + 对应语义描边（原型 trajectory.html `.bub`）。
//! - 状态语言（规则 6）：完成/结果 = `GREEN_SOFT` + `GREEN_BORDER` + `GREEN()` 字；
//!   失败/报错 = `RED_SOFT` + `RED()` 字；轮次头 = `ACCENT()`；气泡一律 `RADIUS_CHIP`(9)。
//! - 按钮（规则 5、7）：采纳/替换 = `ACCENT()` 实心 + `ON_ACCENT()` 白字 + `RADIUS_BTN`；
//!   追加 = 中性次级（`PANEL2` + `LINE` + `DIM`），hover 底色提亮一档（`CHIP_NEUTRAL` 7% 白叠出）、
//!   描边转 `ACCENT_BORDER()`、active 叠 22% 墨底压暗——全部由现有 token 派生，库主题（近白/近黑）
//!   一概绕开（gpui-component 的 primary Button 走库色，故这里用 token 自绘动作钮）。
//! - 文字三档（规则 8）：气泡正文 `TEXT()`/语义色、参数摘要 `DIM()`、时间/弱注 `FAINT()`。
//! - 台账码（规则 10）：时间戳 `FAMILY_MONO` + `FONT_LABEL` + `FAINT()`，右对齐成列。
//! - 排版（规则 9）：气泡正文 `FONT_META`(11.5) + `LINE_HEIGHT_BODY`；轮次头 `FONT_LABEL`(11) 半粗。
//! - 间距（规则 12）：栏水平内边距 `PAD_MD`(12)，气泡间 `GAP_SM`(6)，组内 `GAP_XS`，
//!   按钮间 `GAP_MD`；废除 `p_2`(8px 贴边) / `rounded_md` / `text_xs` 混档。
//! - 空态（规则 15）：TEXT 主句 + DIM 引导句 + FAINT 指向动作的提示句；
//!   时间显示改本地时钟（`localtime_r`，修复旧 `utc_hhmmss` 的 UTC 缺陷）。

use std::sync::atomic::{AtomicU64, Ordering};

use gpui_kit::component::bubble::{Bubble, BubbleVariant};
use gpui_kit::component::dock::BasePanel;
use gpui_kit::component::dock::Panel as ComponentPanel;
use gpui_kit::component::dock::PanelEvent;
use gpui_kit::{
    App, ClickEvent, Context, EventEmitter, FocusHandle, Focusable, FontWeight, Hsla,
    InteractiveElement, IntoElement, ParentElement, Pixels, Render, Rgba, ScrollHandle,
    SharedString, StatefulInteractiveElement, Styled, Window, div, relative,
};

use crate::theme;

#[derive(Debug, Clone)]
pub enum TrajEvent {
    /// 作者点「采纳到正文」：Workspace 快照后追加
    Adopt(String),
    /// 选区创作：替换当前选区（快照先行）
    AdoptSelection(String),
}

/// 一条轨迹气泡。
#[derive(Debug, Clone)]
pub enum TrajKind {
    /// 轮次分节头（第 N 轮）
    Turn(u64),
    /// 普通信息
    Info(String),
    /// 工具调用（名称 + 参数摘要）
    Tool { name: String, args: String },
    /// 工具成功
    ToolOk(String),
    /// 工具失败
    ToolErr(String),
    /// 模型文本产出
    Text(String),
    /// 选区创作产出：可替换选区或追加
    SelectionDraft { original: String, text: String },
    /// 用户输入
    User(String),
    /// 完成
    Done(String),
    /// 失败
    Fail(String),
    /// 连接层错误
    Conn(String),
}

#[derive(Debug, Clone)]
pub struct TrajEntry {
    #[allow(dead_code)]
    pub seq: u64,
    pub kind: TrajKind,
    /// 本地时钟 HH:MM:SS（`localtime_r`，规则 15）
    pub time: String,
}

impl TrajKind {
    fn label(&self) -> String {
        match self {
            TrajKind::Turn(n) => format!("▶ 第 {n} 轮"),
            TrajKind::Info(t) => t.clone(),
            TrajKind::Tool { name, .. } => format!("🛠 调用 {name}"),
            TrajKind::ToolOk(t) => format!("↳ {t}"),
            TrajKind::ToolErr(t) => format!("✕ 工具报错：{t}"),
            TrajKind::Text(t) => t.clone(),
            TrajKind::SelectionDraft { text, original } => {
                format!("选区创作（原文 {} 字）→ {text}", original.chars().count())
            }
            TrajKind::User(t) => format!("你：{t}"),
            TrajKind::Done(t) => format!("✓ {t}"),
            TrajKind::Fail(t) => format!("✕ {t}"),
            TrajKind::Conn(t) => format!("⚠ {t}"),
        }
    }

    fn detail(&self) -> Option<String> {
        match self {
            TrajKind::Tool { args, .. } if !args.is_empty() => Some(args.clone()),
            _ => None,
        }
    }

    fn is_error(&self) -> bool {
        matches!(self, TrajKind::ToolErr(_) | TrajKind::Fail(_) | TrajKind::Conn(_))
    }

    fn is_done(&self) -> bool {
        matches!(self, TrajKind::Done(_))
    }

    fn is_turn(&self) -> bool {
        matches!(self, TrajKind::Turn(_))
    }

    fn is_tool(&self) -> bool {
        matches!(self, TrajKind::Tool { .. })
    }

    fn is_user(&self) -> bool {
        matches!(self, TrajKind::User(_))
    }
}

pub struct TrajectoryPanel {
    focus_handle: FocusHandle,
    entries: Vec<TrajEntry>,
    next_seq: AtomicU64,
    scroll_handle: ScrollHandle,
}

impl TrajectoryPanel {
    pub fn new(cx: &mut Context<Self>) -> Self {
        Self {
            focus_handle: cx.focus_handle(),
            entries: Vec::new(),
            next_seq: AtomicU64::new(1),
            scroll_handle: ScrollHandle::new(),
        }
    }

    /// 追加一条轨迹并滚到底部。
    pub fn push(&mut self, kind: TrajKind, cx: &mut Context<Self>) {
        let seq = self.next_seq.fetch_add(1, Ordering::SeqCst);
        self.entries.push(TrajEntry {
            seq,
            kind,
            time: local_hhmmss(),
        });
        if self.entries.len() > 400 {
            self.entries.drain(..self.entries.len() - 400);
        }
        cx.notify();
        // 布局完成后再滚到底
        let handle = self.scroll_handle.clone();
        cx.spawn(async move |_, _| {
            handle.scroll_to_bottom();
            anyhow::Ok(())
        })
        .detach();
    }

    #[allow(dead_code)]
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }
}

/// 本地时钟 HH:MM:SS。旧实现按 UTC epoch 硬算（已知缺陷，规则 15），
/// 改为直读系统本地时区。
fn local_hhmmss() -> String {
    let secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0);
    format_clock(secs)
}

/// unix：`localtime_r` 直读本地时区。`struct tm` 的前三个 i32 字段
/// （sec/min/hour）在 macOS/Linux 布局一致，尾部用足量填充承接内核写入。
#[cfg(unix)]
fn format_clock(secs: i64) -> String {
    #[repr(C)]
    struct CTm {
        tm_sec: i32,
        tm_min: i32,
        tm_hour: i32,
        _pad: usize,
        _rest: [std::ffi::c_char; 96],
    }
    unsafe extern "C" {
        fn localtime_r(t: *const i64, tm: *mut CTm) -> *mut CTm;
    }
    let mut tm: CTm = unsafe { std::mem::zeroed() };
    let tm = unsafe { &*localtime_r(&secs, &mut tm) };
    format!("{:02}:{:02}:{:02}", tm.tm_hour, tm.tm_min, tm.tm_sec)
}

/// 非 unix 回退：按 UTC 组钟面（不引入额外依赖，保持零依赖）。
#[cfg(not(unix))]
fn format_clock(secs: i64) -> String {
    format!(
        "{:02}:{:02}:{:02}",
        (secs / 3600) % 24,
        (secs / 60) % 60,
        secs % 60
    )
}

impl EventEmitter<PanelEvent> for TrajectoryPanel {}
impl EventEmitter<TrajEvent> for TrajectoryPanel {}

impl Focusable for TrajectoryPanel {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus_handle.clone()
    }
}

/// 动作钮语气档（与收件箱面板同一套 token 语言，规则 5/6/7）。
#[derive(Clone, Copy)]
enum Tone {
    /// 主能力动作：朱砂实心 + 白字。
    Primary,
    /// 次级动作：面板2 底 + 分隔线描边，hover 描边转朱砂。
    Neutral,
}

/// hover「提亮一档」：在原色上叠 7% 白纱（`CHIP_NEUTRAL`），全部取自现有 token。
fn lifted(base: Rgba) -> Hsla {
    let base: Hsla = base.into();
    let veil: Hsla = theme::CHIP_NEUTRAL().into();
    base.blend(veil)
}

/// active「压暗一档」：在原色上叠 22% 墨底（`BG`），给按压可感知反馈（规则 7）。
fn pressed(base: Rgba) -> Hsla {
    let base: Hsla = base.into();
    let ink: Hsla = theme::BG().into();
    base.blend(ink.opacity(0.22))
}

/// 面板内的紧凑动作钮：全部用 token 自绘，绕开 gpui-component 库主题（近白/近黑）。
/// 事件语义与旧实现一致：`on_click` 仍原样转发到 `cx.listener` 闭包。
fn action_button(
    id: String,
    label: &'static str,
    tone: Tone,
    on_click: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
) -> impl IntoElement {
    let (bg, border, fg, hover_bg, hover_border, hover_fg, active_bg, radius): (
        Hsla,
        Hsla,
        Hsla,
        Hsla,
        Hsla,
        Hsla,
        Hsla,
        Pixels,
    ) = match tone {
        Tone::Primary => (
            theme::ACCENT().into(),
            theme::ACCENT().into(),
            theme::ON_ACCENT().into(),
            lifted(theme::ACCENT()),
            theme::ACCENT().into(),
            theme::ON_ACCENT().into(),
            pressed(theme::ACCENT()),
            theme::RADIUS_BTN,
        ),
        Tone::Neutral => (
            theme::PANEL2().into(),
            theme::LINE().into(),
            theme::DIM().into(),
            lifted(theme::PANEL2()),
            theme::ACCENT_BORDER().into(),
            theme::TEXT().into(),
            pressed(theme::PANEL2()),
            theme::RADIUS_BTN_SM,
        ),
    };

    div()
        .id(id)
        .flex()
        .items_center()
        .justify_center()
        .flex_shrink_0()
        .px(theme::PAD_SM)
        .py(theme::PAD_XS)
        .cursor_pointer()
        .rounded(radius)
        .bg(bg)
        .border_1()
        .border_color(border)
        .font_family(theme::FAMILY_UI)
        .text_size(theme::FONT_LABEL)
        .font_weight(FontWeight::MEDIUM)
        .text_color(fg)
        .hover(move |style| {
            style
                .bg(hover_bg)
                .border_color(hover_border)
                .text_color(hover_fg)
        })
        .active(move |style| style.bg(active_bg))
        .on_click(on_click)
        .child(label)
}

impl Render for TrajectoryPanel {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let mut list = div()
            .flex()
            .flex_col()
            .gap(theme::GAP_SM)
            .size_full();

        if self.entries.is_empty() {
            // —— 空态：主句 TEXT + 引导 DIM + 指向动作 FAINT（规则 15），不许静默吞掉
            list = list.child(
                div()
                    .w_full()
                    .flex()
                    .flex_col()
                    .items_center()
                    .gap(theme::GAP_MD)
                    .p(theme::PAD_2XL)
                    .rounded(theme::RADIUS_CARD)
                    .bg(theme::PANEL())
                    .border_1()
                    .border_color(theme::LINE())
                    .font_family(theme::FAMILY_UI)
                    .text_center()
                    .child(
                        div()
                            .text_size(theme::FONT_BODY)
                            .font_weight(FontWeight::MEDIUM)
                            .text_color(theme::TEXT())
                            .child("还没有轨迹"),
                    )
                    .child(
                        div()
                            .text_size(theme::FONT_META)
                            .line_height(relative(theme::LINE_HEIGHT_BODY))
                            .text_color(theme::DIM())
                            .child(
                                "跑一次任意 AI 能力——续写、搭骨架、归类设定、验证报告——完整 agent 轨迹会像对话一样在这里逐条展开。",
                            ),
                    )
                    .child(
                        div()
                            .font_family(theme::FAMILY_MONO)
                            .text_size(theme::FONT_TAG)
                            .text_color(theme::FAINT())
                            .child("▸ 发起后新事件自动滚动到此处；⌘⇧J 可随时收起本面板"),
                    ),
            );
        } else {
            let entries = self.entries.clone();
            for e in entries {
                let adopt_btn: Option<gpui_kit::AnyElement> = match &e.kind {
                    TrajKind::Text(text) => {
                        let text = text.clone();
                        Some(
                            action_button(
                                format!("adopt-{}", e.seq),
                                "采纳到正文",
                                Tone::Primary,
                                cx.listener(move |_this, _, _, cx| {
                                    cx.emit(TrajEvent::Adopt(text.clone()));
                                }),
                            )
                            .into_any_element(),
                        )
                    }
                    TrajKind::SelectionDraft { text, .. } => {
                        let text = text.clone();
                        let replace = text.clone();
                        Some(
                            div()
                                .flex()
                                .gap(theme::GAP_MD)
                                .justify_end()
                                .child(action_button(
                                    format!("repl-{}", e.seq),
                                    "替换选区",
                                    Tone::Primary,
                                    cx.listener(move |_this, _, _, cx| {
                                        cx.emit(TrajEvent::AdoptSelection(replace.clone()));
                                    }),
                                ))
                                .child(action_button(
                                    format!("appnd-{}", e.seq),
                                    "追加到正文",
                                    Tone::Neutral,
                                    cx.listener(move |_this, _, _, cx| {
                                        cx.emit(TrajEvent::Adopt(text.clone()));
                                    }),
                                ))
                                .into_any_element(),
                        )
                    }
                    _ => None,
                };
                list = list.child(entry_block(e, adopt_btn));
            }
        }

        div()
            .id("trajectory-scroll")
            .size_full()
            .overflow_y_scroll()
            .track_scroll(&self.scroll_handle)
            .bg(theme::BG())
            .child(
                div()
                    .flex()
                    .flex_col()
                    .px(theme::PAD_MD)
                    .py(theme::PAD_MD)
                    .child(list),
            )
    }
}

/// 一条轨迹的渲染：轮次走 `.runhead` 分隔行（朱砂标签 + 等宽时间 + 1px 长线），
/// 其余走语义气泡。
fn entry_block(e: TrajEntry, adopt: Option<gpui_kit::AnyElement>) -> gpui_kit::AnyElement {
    if e.kind.is_turn() {
        return div()
            .flex()
            .items_center()
            .gap(theme::GAP_MD)
            .pt(theme::GAP_MD)
            .pb(theme::GAP_XS)
            .child(
                div()
                    .font_family(theme::FAMILY_UI)
                    .text_size(theme::FONT_LABEL)
                    .font_weight(FontWeight::SEMIBOLD)
                    .text_color(theme::ACCENT())
                    .child(SharedString::from(e.kind.label())),
            )
            .child(
                div()
                    .font_family(theme::FAMILY_MONO)
                    .text_size(theme::FONT_LABEL)
                    .text_color(theme::FAINT())
                    .child(SharedString::from(e.time)),
            )
            .child(div().flex_1().border_t_1().border_color(theme::LINE()))
            .into_any_element();
    }
    bubble(e, adopt)
}

/// 语义气泡 = kit Bubble（variant 承载语义面），底色/描边按原型四色语言覆写（规则 6）。
fn bubble(e: TrajEntry, adopt: Option<gpui_kit::AnyElement>) -> gpui_kit::AnyElement {
    let (variant, bg, border, fg): (BubbleVariant, Rgba, Rgba, Rgba) = if e.kind.is_error() {
        (BubbleVariant::Destructive, theme::RED_SOFT(), theme::LINE(), theme::RED())
    } else if e.kind.is_done() || matches!(e.kind, TrajKind::ToolOk(_)) {
        (
            BubbleVariant::Secondary,
            theme::GREEN_SOFT(),
            theme::GREEN_BORDER(),
            theme::GREEN(),
        )
    } else if e.kind.is_tool() || e.kind.is_user() {
        (
            BubbleVariant::Tinted,
            theme::ACCENT_SOFT(),
            theme::ACCENT_BORDER(),
            theme::TEXT(),
        )
    } else {
        (BubbleVariant::Muted, theme::PANEL(), theme::LINE(), theme::TEXT())
    };

    let mut b = Bubble::new()
        .with_variant(variant)
        .w_full()
        .bg(bg)
        .border_1()
        .border_color(border)
        .child(
            div()
                .w_full()
                .font_family(theme::FAMILY_UI)
                .text_size(theme::FONT_META)
                .line_height(relative(theme::LINE_HEIGHT_BODY))
                .text_color(fg)
                .child(SharedString::from(e.kind.label())),
        );

    if let Some(detail) = e.kind.detail() {
        b = b.child(
            div()
                .w_full()
                .font_family(theme::FAMILY_UI)
                .text_size(theme::FONT_TAG)
                .line_height(relative(theme::LINE_HEIGHT_BODY))
                .text_color(theme::DIM())
                .child(SharedString::from(detail)),
        );
    }

    // 页脚：动作钮靠右，时间戳等宽右列（规则 10）。
    let mut footer = div()
        .flex()
        .w_full()
        .items_center()
        .justify_end()
        .gap(theme::GAP_MD);
    if let Some(btn) = adopt {
        footer = footer.child(btn);
    }
    footer = footer.child(
        div()
            .font_family(theme::FAMILY_MONO)
            .text_size(theme::FONT_LABEL)
            .text_color(theme::FAINT())
            .child(SharedString::from(e.time)),
    );
    b = b.child(footer);

    b.into_any_element()
}
// —— Dock 面板契约 ——

impl BasePanel for TrajectoryPanel {
    fn panel_name(&self) -> &'static str {
        "agent-trajectory"
    }

    fn closable(&self, _: &App) -> bool {
        // 轨迹是门户级特性：只许收起，不许关掉
        false
    }
}

impl ComponentPanel for TrajectoryPanel {
    fn tab_name(&self, _: &App) -> Option<SharedString> {
        Some("轨迹".into())
    }

    fn title(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div()
            .flex()
            .items_center()
            .gap(theme::GAP_MD)
            .child(
                div()
                    .font_family(theme::FAMILY_UI)
                    .text_size(theme::FONT_TITLE_SM)
                    .font_weight(FontWeight::SEMIBOLD)
                    .text_color(theme::TEXT())
                    .child("Agent 轨迹"),
            )
    }
}
