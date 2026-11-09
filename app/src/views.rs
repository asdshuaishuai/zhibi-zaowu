//! 台账页面板（M5 第一片）：设定 / 大纲·时间线 / 记忆 / 统计。
//! 全部只读渲染真实磁盘数据；写操作一律走提案（按钮上抛事件给 Workspace）。

use std::rc::Rc;

use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::dock::{BasePanel, Panel as ComponentPanel, PanelEvent};
use gpui_kit::component::Sizable;
use gpui_kit::{
    AnyElement, App, Context, EventEmitter, FocusHandle, Focusable, FontWeight,
    InteractiveElement, IntoElement, ParentElement, Render, SharedString,
    StatefulInteractiveElement, Styled, Window, div, px, rgb,
};

use crate::store::ProjectHandle;

#[derive(Debug, Clone)]
pub enum ViewEvent {
    /// 触发一次 AI 能力（capability 键）
    RunCapability(&'static str),
}

impl EventEmitter<PanelEvent> for CanonPanel {}
impl EventEmitter<ViewEvent> for CanonPanel {}
impl EventEmitter<PanelEvent> for OutlinePanel {}
impl EventEmitter<ViewEvent> for OutlinePanel {}
impl EventEmitter<PanelEvent> for MemoryPanel {}
impl EventEmitter<ViewEvent> for MemoryPanel {}
impl EventEmitter<PanelEvent> for StatsPanel {}

/* ───────────────────────── 设定 ───────────────────────── */

pub struct CanonPanel {
    focus_handle: FocusHandle,
    handle: Rc<ProjectHandle>,
    selected: Option<String>,
}

impl CanonPanel {
    pub fn new(cx: &mut Context<Self>, handle: Rc<ProjectHandle>) -> Self {
        let docs = handle.canon_docs();
        Self {
            focus_handle: cx.focus_handle(),
            handle,
            selected: docs.first().map(|d| d.0.clone()),
        }
    }
}

impl Focusable for CanonPanel {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus_handle.clone()
    }
}

impl Render for CanonPanel {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let docs = self.handle.canon_docs();
        let mut groups: Vec<(String, Vec<(String, String, String)>)> = Vec::new();
        for d in &docs {
            if let Some(g) = groups.iter_mut().find(|g| g.0 == d.1) {
                g.1.push(d.clone());
            } else {
                groups.push((d.1.clone(), vec![d.clone()]));
            }
        }
        let cur = docs.iter().find(|d| Some(&d.0) == self.selected.as_ref()).cloned().or_else(|| docs.first().cloned());

        let mut left: Vec<gpui_kit::AnyElement> = Vec::new();
        for (cat, items) in groups {
            left.push(
                div()
                    .text_xs()
                    .text_color(rgb(0x5d5b57))
                    .pt_2()
                    .child(SharedString::from(format!("{cat} · {}", items.len())))
                    .into_any_element(),
            );
            for (title, _, _) in items {
                let t = title.clone();
                left.push(
                    Button::new(format!("canon-{title}"))
                        .ghost()
                        .label(title)
                        .on_click(cx.listener(move |this, _, _, cx| {
                            this.selected = Some(t.clone());
                            cx.notify();
                        }))
                        .into_any_element(),
                );
            }
        }

        let body = if let Some((title, cat, content)) = cur {
            div()
                .flex_1()
                .min_w_0()
                .p_3()
                .rounded_md()
                .bg(rgb(0x24262d))
                .child(
                    div()
                        .flex()
                        .items_center()
                        .gap_2()
                        .pb_2()
                        .child(div().text_sm().font_weight(FontWeight::SEMIBOLD).child(SharedString::from(title.clone())))
                        .child(
                            div()
                                .text_xs()
                                .text_color(rgb(0x6f9bc4))
                                .child(SharedString::from(cat)),
                        ),
                )
                .child(
                    div()
                        .text_sm()
                        .text_color(rgb(0xd8d5cf))
                        .line_height(gpui_kit::relative(1.8))
                        .child(SharedString::from(content.chars().take(4000).collect::<String>())),
                )
        } else {
            div()
                .flex_1()
                .p_4()
                .text_sm()
                .text_color(rgb(0x5d5b57))
                .child("设定库为空——用「AI 归类设定」或导入后自动归类。")
        };

        div()
            .id("canon-scroll")
            .size_full()
            .overflow_y_scroll()
            .child(
                div()
                    .flex()
                    .flex_col()
                    .p_3()
                    .gap_2()
                    .child(
                        div().flex().items_center().gap_3().child(
                            div().text_sm().font_weight(FontWeight::MEDIUM).child("设定"),
                        ).child(
                            Button::new("canon-classify")
                                .xsmall()
                                .label("AI 归类（提案）")
                                .on_click(cx.listener(|_this, _, _, cx| {
                                    cx.emit(ViewEvent::RunCapability("classify"));
                                })),
                        ),
                    )
                    .child(
                        div()
                            .flex()
                            .gap_3()
                            .child(div().w(px(180.)).flex_col().gap_0p5().children(left))
                            .child(body),
                    ),
            )
    }
}

impl BasePanel for CanonPanel {
    fn panel_name(&self) -> &'static str {
        "canon-view"
    }
}
impl ComponentPanel for CanonPanel {
    fn title(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div().text_sm().font_weight(FontWeight::MEDIUM).child("设定")
    }
}

/* ───────────────────────── 大纲 · 时间线 ───────────────────────── */

pub struct OutlinePanel {
    focus_handle: FocusHandle,
    handle: Rc<ProjectHandle>,
}

impl OutlinePanel {
    pub fn new(cx: &mut Context<Self>, handle: Rc<ProjectHandle>) -> Self {
        Self { focus_handle: cx.focus_handle(), handle }
    }
}

impl Focusable for OutlinePanel {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus_handle.clone()
    }
}

fn row(cells: Vec<(String, Option<&str>)>) -> gpui_kit::AnyElement {
    let mut d = div().flex().gap_3().w_full().py_1().border_b_1().border_color(rgb(0x2e3138));
    for (text, color) in cells {
        d = d.child(
            div()
                .flex_1()
                .text_xs()
                .text_color(color.map(|c| c.parse_rgb()).unwrap_or_else(|| rgb(0xd8d5cf).into()))
                .child(SharedString::from(text)),
        );
    }
    d.into_any_element()
}

trait ParseRgb {
    fn parse_rgb(&self) -> gpui_kit::Hsla;
}
impl ParseRgb for str {
    fn parse_rgb(&self) -> gpui_kit::Hsla {
        let v: gpui_kit::Rgba = match self {
            "dim" => rgb(0x8e8b85),
            "faint" => rgb(0x5d5b57),
            "accent" => rgb(0xc96f4a),
            "blue" => rgb(0x6f9bc4),
            "green" => rgb(0x7fae72),
            _ => rgb(0xd8d5cf),
        };
        v.into()
    }
}

impl Render for OutlinePanel {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let lines = self.handle.storylines();
        let events = self.handle.events();

        let mut line_rows = Vec::new();
        for l in &lines {
            line_rows.push(row(vec![
                (l.get("id").and_then(|v| v.as_str()).unwrap_or("?").to_string(), Some("faint")),
                (l.get("name").and_then(|v| v.as_str()).unwrap_or("").to_string(), None),
                (l.get("status").and_then(|v| v.as_str()).unwrap_or("").to_string(), Some("blue")),
            ]));
        }
        let mut event_rows = Vec::new();
        for e in events.iter().take(40) {
            event_rows.push(row(vec![
                (e.get("id").and_then(|v| v.as_str()).unwrap_or("?").to_string(), Some("faint")),
                (format!("第{}", e.get("chapter").and_then(|c| c.as_u64()).unwrap_or(0)), Some("dim")),
                (e.get("objectiveFact").and_then(|v| v.as_str()).unwrap_or("").chars().take(60).collect(), None),
                (e.get("revealed").and_then(|v| v.as_bool()).map(|r| if r { "读者已知" } else { "暗线" }).unwrap_or("").to_string(), Some("accent")),
            ]));
        }

        div()
            .id("outline-scroll")
            .size_full()
            .overflow_y_scroll()
            .child(
                div()
                    .flex()
                    .flex_col()
                    .p_3()
                    .gap_2()
                    .child(
                        div().flex().items_center().gap_3()
                            .child(div().text_sm().font_weight(FontWeight::MEDIUM).child("故事线"))
                            .child(
                                Button::new("outline-sync")
                                    .xsmall()
                                    .label("同步大纲（提案）")
                                    .on_click(cx.listener(|_this, _, _, cx| {
                                        cx.emit(ViewEvent::RunCapability("outsync"));
                                    })),
                            )
                            .child(
                                Button::new("outline-audit")
                                    .xsmall()
                                    .label("全书连贯性审查")
                                    .on_click(cx.listener(|_this, _, _, cx| {
                                        cx.emit(ViewEvent::RunCapability("audit"));
                                    })),
                            ),
                    )
                    .children(line_rows)
                    .when_true(lines.is_empty(), |d| {
                        d.child(div().text_xs().text_color(rgb(0x5d5b57)).child("（还没有故事线——新书建框架后出现）"))
                    })
                    .child(div().text_sm().font_weight(FontWeight::MEDIUM).pt_3().child("事件时间线（作者真相｜读者已知）"))
                    .children(event_rows)
                    .when_true(events.is_empty(), |d| {
                        d.child(div().text_xs().text_color(rgb(0x5d5b57)).child("（还没有事件——用「AI 建时间线」提案）"))
                    }),
            )
    }
}

trait WhenTrue: Sized {
    fn when_true(self, cond: bool, f: impl FnOnce(Self) -> Self) -> Self;
}
impl WhenTrue for gpui_kit::Div {
    fn when_true(mut self, cond: bool, f: impl FnOnce(Self) -> Self) -> Self {
        if cond {
            self = f(self);
        }
        self
    }
}

impl BasePanel for OutlinePanel {
    fn panel_name(&self) -> &'static str {
        "outline-view"
    }
}
impl ComponentPanel for OutlinePanel {
    fn title(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div().text_sm().font_weight(FontWeight::MEDIUM).child("大纲 · 时间线")
    }
}

/* ───────────────────────── 记忆 ───────────────────────── */

pub struct MemoryPanel {
    focus_handle: FocusHandle,
    handle: Rc<ProjectHandle>,
}

impl MemoryPanel {
    pub fn new(cx: &mut Context<Self>, handle: Rc<ProjectHandle>) -> Self {
        Self { focus_handle: cx.focus_handle(), handle }
    }
}

impl Focusable for MemoryPanel {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus_handle.clone()
    }
}

impl Render for MemoryPanel {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let facts = self.handle.memory_facts();
        let mut rows = Vec::new();
        for f in facts.iter().take(60) {
            let public = f.get("publicToReader").and_then(|v| v.as_bool()).unwrap_or(true);
            rows.push(row(vec![
                (f.get("subject").and_then(|v| v.as_str()).unwrap_or("").to_string(), None),
                (f.get("predicate").and_then(|v| v.as_str()).unwrap_or("").to_string(), Some("accent")),
                (f.get("object").and_then(|v| v.as_str()).unwrap_or("").chars().take(50).collect(), None),
                (format!("第{}章", f.get("fromChapter").and_then(|v| v.as_u64()).unwrap_or(0)), Some("dim")),
                (if public { "读者可见" } else { "暗线" }.to_string(), Some("blue")),
            ]));
        }
        div()
            .id("memory-scroll")
            .size_full()
            .overflow_y_scroll()
            .child(
                div().flex().flex_col().p_3().gap_2()
                    .child(
                        div().flex().items_center().gap_3()
                            .child(div().text_sm().font_weight(FontWeight::MEDIUM).child("事实账本（双时态）"))
                            .child(
                                Button::new("memory-extract")
                                    .xsmall()
                                    .label("让AI记一笔")
                                    .on_click(cx.listener(|_this, _, _, cx| {
                                        cx.emit(ViewEvent::RunCapability("memory"));
                                    })),
                            ),
                    )
                    .child(row(vec![
                        ("主语".into(), Some("faint")),
                        ("谓语".into(), Some("faint")),
                        ("宾语".into(), Some("faint")),
                        ("生效".into(), Some("faint")),
                        ("读者".into(), Some("faint")),
                    ]))
                    .children(rows)
                    .when_true(facts.is_empty(), |d| {
                        d.child(div().text_xs().text_color(rgb(0x5d5b57)).child("（还没有事实——写完一章点「让AI记一笔」）"))
                    }),
            )
    }
}

impl BasePanel for MemoryPanel {
    fn panel_name(&self) -> &'static str {
        "memory-view"
    }
}
impl ComponentPanel for MemoryPanel {
    fn title(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div().text_sm().font_weight(FontWeight::MEDIUM).child("记忆中枢")
    }
}

/* ───────────────────────── 统计 ───────────────────────── */

pub struct StatsPanel {
    focus_handle: FocusHandle,
    handle: Rc<ProjectHandle>,
}

impl StatsPanel {
    pub fn new(cx: &mut Context<Self>, handle: Rc<ProjectHandle>) -> Self {
        Self { focus_handle: cx.focus_handle(), handle }
    }
}

impl Focusable for StatsPanel {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus_handle.clone()
    }
}

impl Render for StatsPanel {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        let daily = self.handle.daily_words();
        let recent: Vec<_> = daily.iter().rev().take(14).rev().cloned().collect();
        let max = recent.iter().map(|(_, w)| *w).max().unwrap_or(1).max(1);
        let proposals = self.handle.proposals();
        let accepted = proposals.iter().filter(|p| p.status == "已采纳").count();
        let rejected = proposals.iter().filter(|p| p.status == "已拒绝").count();
        let pending = proposals.iter().filter(|p| p.status == "待确认").count();

        let bars: Vec<gpui_kit::AnyElement> = recent
            .iter()
            .map(|(day, w)| {
                div()
                    .flex()
                    .flex_col()
                    .items_center()
                    .gap_1()
                    .w(px(38.))
                    .child(
                        div()
                            .w(px(22.))
                            .h(px((*w as f32 / max as f32 * 90.).max(4.)))
                            .rounded_t_md()
                            .bg(rgb(0xc96f4a)),
                    )
                    .child(div().text_xs().text_color(rgb(0x5d5b57)).child(SharedString::from(day[5..].to_string())))
                    .into_any_element()
            })
            .collect();

        div()
            .id("stats-scroll")
            .size_full()
            .overflow_y_scroll()
            .child(
                div().flex().flex_col().p_3().gap_3()
                    .child(div().text_sm().font_weight(FontWeight::MEDIUM).child("每日净增字数（近 14 天）"))
                    .child(div().flex().gap_2().items_end().children(bars))
                    .child(div().text_sm().font_weight(FontWeight::MEDIUM).pt_2().child("提案账本"))
                    .child(row(vec![
                        (format!("待裁决 {pending}"), Some("accent")),
                        (format!("已采纳 {accepted}"), Some("green")),
                        (format!("已拒收 {rejected}"), Some("faint")),
                        (format!("累计 {}", proposals.len()), Some("dim")),
                    ])),
            )
    }
}

impl BasePanel for StatsPanel {
    fn panel_name(&self) -> &'static str {
        "stats-view"
    }
}
impl ComponentPanel for StatsPanel {
    fn title(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div().text_sm().font_weight(FontWeight::MEDIUM).child("写作统计")
    }
}
