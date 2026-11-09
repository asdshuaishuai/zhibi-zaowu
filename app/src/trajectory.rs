//! 右侧 Agent 轨迹面板（Dock 面板）。
//!
//! 数据源：DSH `session.event` 全量事件流，按「一次 prompt = 一个轮次」分节。
//! 展示为对话式时间线：分节头 + 工具调用/结果/文本产出气泡 + 完成/失败收尾。
//! 新事件进入时自动滚到底部。M1 用基础色板占位，M2 换执笔品牌色板。

use std::sync::atomic::{AtomicU64, Ordering};

use gpui_kit::component::button::Button;
use gpui_kit::component::Sizable;
use gpui_kit::component::dock::PanelEvent;
use gpui_kit::component::dock::Panel as ComponentPanel;
use gpui_kit::component::dock::BasePanel;
use gpui_kit::{
    App, Context, EventEmitter, FocusHandle, Focusable, FontWeight, InteractiveElement,
    IntoElement, ParentElement, Render, ScrollHandle, SharedString, StatefulInteractiveElement,
    Styled, Window, div, rgb, rgba,
};

#[derive(Debug, Clone)]
pub enum TrajEvent {
    /// 作者点「采纳到正文」：Workspace 快照后追加
    Adopt(String),
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
    /// UTC 时钟 HH:MM:SS（M1 不引本地时区依赖）
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
            time: utc_hhmmss(),
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

fn utc_hhmmss() -> String {
    let secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
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

impl Render for TrajectoryPanel {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let mut list = div().flex().flex_col().gap_1p5().size_full();

        if self.entries.is_empty() {
            list = list.child(
                div()
                    .pt_16()
                    .w_full()
                    .text_center()
                    .text_sm()
                    .text_color(rgb(0x8e8b85))
                    .child("还没有轨迹。\n跑一次任意 AI 能力，完整 agent 轨迹会像对话一样在这里展开。"),
            );
        } else {
            let entries = self.entries.clone();
            for e in entries {
                let adopt_btn: Option<gpui_kit::AnyElement> = if let TrajKind::Text(text) = &e.kind {
                    let text = text.clone();
                    Some(
                        Button::new(format!("adopt-{}", e.seq))
                            .xsmall()
                            .label("采纳到正文")
                            .on_click(cx.listener(move |_this, _, _, cx| {
                                cx.emit(TrajEvent::Adopt(text.clone()));
                            }))
                            .into_any_element(),
                    )
                } else {
                    None
                };
                list = list.child(bubble(e.clone(), adopt_btn));
            }
        }

        div()
            .id("trajectory-scroll")
            .size_full()
            .overflow_y_scroll()
            .track_scroll(&self.scroll_handle)
            .child(div().flex().flex_col().p_2().child(list))
    }
}

fn bubble(e: TrajEntry, adopt: Option<gpui_kit::AnyElement>) -> gpui_kit::AnyElement {
    // 品牌色板（对应原型稿）：墨底 · 朱砂强调 · 绿完成 · 红失败
    let (bg, fg): (gpui_kit::Hsla, gpui_kit::Hsla) = if e.kind.is_error() {
        (rgba(0xc0605c1f).into(), rgb(0xc0605c).into())
    } else if e.kind.is_done() {
        (rgba(0x7fae721e).into(), rgb(0x7fae72).into())
    } else if e.kind.is_turn() {
        (rgba(0xc96f4a1a).into(), rgb(0xc96f4a).into())
    } else {
        (rgba(0xffffff0d).into(), rgb(0xd8d5cf).into())
    };

    let mut row = div()
        .flex()
        .flex_col()
        .gap_0p5()
        .w_full()
        .rounded_md()
        .px_2()
        .py_1()
        .bg(bg);

    if e.kind.is_turn() {
        row = row.child(
            div()
                .w_full()
                .text_xs()
                .font_weight(FontWeight::SEMIBOLD)
                .text_color(fg)
                .child(SharedString::from(e.kind.label())),
        );
    } else {
        row = row.child(
            div()
                .w_full()
                .text_sm()
                .text_color(fg)
                .child(SharedString::from(e.kind.label())),
        );
    }

    if let Some(detail) = e.kind.detail() {
        row = row.child(
            div()
                .w_full()
                .text_xs()
                .text_color(rgb(0x8e8b85))
                .child(SharedString::from(detail)),
        );
    }

    row = row.child(
        div()
            .w_full()
            .text_xs()
            .text_color(rgb(0x5d5b57))
            .child(SharedString::from(e.time)),
    );

    if let Some(btn) = adopt {
        row = row.child(div().flex().justify_end().child(btn));
    }

    row.into_any_element()
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
            .text_sm()
            .font_weight(FontWeight::MEDIUM)
            .child("Agent 轨迹")
    }
}
