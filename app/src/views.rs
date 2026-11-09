//! 台账页面板（M5 第一片）：设定 / 大纲·时间线 / 记忆 / 统计。
//! 全部只读渲染真实磁盘数据；写操作一律走提案（按钮上抛事件给 Workspace）。

use std::rc::Rc;

use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::dock::{BasePanel, Panel as ComponentPanel, PanelEvent};
use gpui_kit::component::Sizable;
use gpui_kit::{
    App, AppContext as _, Context, Entity, EventEmitter, FocusHandle, Focusable, FontWeight,
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
impl EventEmitter<ViewEvent> for StatsPanel {}
impl EventEmitter<PanelEvent> for CluesPanel {}
impl EventEmitter<ViewEvent> for CluesPanel {}
impl EventEmitter<PanelEvent> for CharactersPanel {}

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


/* ───────────────────────── 伏笔台账 ───────────────────────── */

pub struct CluesPanel {
    focus_handle: FocusHandle,
    handle: Rc<ProjectHandle>,
}

impl CluesPanel {
    pub fn new(cx: &mut Context<Self>, handle: Rc<ProjectHandle>) -> Self {
        Self { focus_handle: cx.focus_handle(), handle }
    }
}

impl Focusable for CluesPanel {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus_handle.clone()
    }
}

impl Render for CluesPanel {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let clues = self.handle.clues_full();
        let mut rows = Vec::new();
        for c in clues.iter().take(80) {
            rows.push(row(vec![
                (c.get("id").and_then(|v| v.as_str()).unwrap_or("?").to_string(), Some("accent")),
                (c.get("title").and_then(|v| v.as_str()).unwrap_or("").chars().take(40).collect(), None),
                (c.get("status").and_then(|v| v.as_str()).unwrap_or("").to_string(), Some("blue")),
                (format!("埋{}", c.get("plantedChapter").and_then(|v| v.as_u64()).unwrap_or(0)), Some("dim")),
                (format!("动{}", c.get("lastActionChapter").and_then(|v| v.as_u64()).unwrap_or(0)), Some("dim")),
                (c.get("timing").and_then(|v| v.as_str()).unwrap_or("").to_string(), Some("faint")),
            ]));
        }
        div()
            .id("clues-scroll")
            .size_full()
            .overflow_y_scroll()
            .child(
                div().flex().flex_col().p_3().gap_2()
                    .child(
                        div().flex().items_center().gap_3()
                            .child(div().text_sm().font_weight(FontWeight::MEDIUM).child("伏笔台账"))
                            .child(
                                Button::new("clues-inventory")
                                    .xsmall()
                                    .label("AI 盘点（提案）")
                                    .on_click(cx.listener(|_this, _, _, cx| {
                                        cx.emit(ViewEvent::RunCapability("inventory"));
                                    })),
                            )
                            .child(div().text_xs().text_color(rgb(0x5d5b57)).child(SharedString::from(format!("{} 条", clues.len())))),
                    )
                    .children(rows)
                    .when_true(clues.is_empty(), |d| {
                        d.child(div().text_xs().text_color(rgb(0x5d5b57)).child("（台账为空——「AI 盘点伏笔」可从正文重建）"))
                    }),
            )
    }
}

impl BasePanel for CluesPanel {
    fn panel_name(&self) -> &'static str {
        "clues-view"
    }
}
impl ComponentPanel for CluesPanel {
    fn title(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div().text_sm().font_weight(FontWeight::MEDIUM).child("伏笔台账")
    }
}

/* ───────────────────────── 人物卡（别名归一） ───────────────────────── */

pub struct CharactersPanel {
    focus_handle: FocusHandle,
    handle: Rc<ProjectHandle>,
}

impl CharactersPanel {
    pub fn new(cx: &mut Context<Self>, handle: Rc<ProjectHandle>) -> Self {
        Self { focus_handle: cx.focus_handle(), handle }
    }
}

impl Focusable for CharactersPanel {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus_handle.clone()
    }
}

impl Render for CharactersPanel {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        let aliases = self.handle.aliases();
        let facts = self.handle.memory_facts();
        let mut rows = Vec::new();
        for a in aliases.iter().take(60) {
            let canon = a.get("canonicalName").and_then(|v| v.as_str()).unwrap_or("?").to_string();
            let subs: Vec<String> = a
                .get("aliases")
                .and_then(|v| v.as_array())
                .map(|arr| arr.iter().filter_map(|x| x.as_str().map(String::from)).collect())
                .unwrap_or_default();
            let appears = facts
                .iter()
                .filter(|f| f.get("subject").and_then(|s| s.as_str()) == Some(canon.as_str()))
                .count();
            rows.push(row(vec![
                (canon, None),
                (subs.join(" / "), Some("dim")),
                (format!("{appears} 条事实"), Some("blue")),
            ]));
        }
        div()
            .id("characters-scroll")
            .size_full()
            .overflow_y_scroll()
            .child(
                div().flex().flex_col().p_3().gap_2()
                    .child(div().text_sm().font_weight(FontWeight::MEDIUM).child("人物卡（别名归一）"))
                    .children(rows)
                    .when_true(aliases.is_empty(), |d| {
                        d.child(div().text_xs().text_color(rgb(0x5d5b57)).child("（还没有别名档案——记忆提取/人物归一提案采纳后出现）"))
                    }),
            )
    }
}

impl BasePanel for CharactersPanel {
    fn panel_name(&self) -> &'static str {
        "characters-view"
    }
}
impl ComponentPanel for CharactersPanel {
    fn title(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div().text_sm().font_weight(FontWeight::MEDIUM).child("人物卡")
    }
}


/* ───────────────────────── 设置（端点 + 连通测试） ───────────────────────── */

#[derive(Debug, Clone)]
pub enum SettingsEvent {
    Save { base_url: String, model: String, provider: String },
    Test(String),
}

pub struct SettingsPanel {
    focus_handle: FocusHandle,
    base_url: Entity<gpui_kit::component::input::InputState>,
    model: Entity<gpui_kit::component::input::InputState>,
    provider: Entity<gpui_kit::component::input::InputState>,
    status: String,
    testing: bool,
}

impl SettingsPanel {
    pub fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        let cfg = crate::store::load_settings();
        let base_url = cx.new(|cx| {
            gpui_kit::component::input::InputState::new(window, cx)
                .placeholder("http://192.168.x.x:port/v1")
                .default_value(cfg.base_url)
        });
        let model = cx.new(|cx| {
            gpui_kit::component::input::InputState::new(window, cx)
                .placeholder("模型 ID")
                .default_value(cfg.model)
        });
        let provider = cx.new(|cx| {
            gpui_kit::component::input::InputState::new(window, cx)
                .placeholder("provider 路由（如 deepseek-official）")
                .default_value(if cfg.provider.is_empty() { "deepseek-official".into() } else { cfg.provider })
        });
        Self { focus_handle: cx.focus_handle(), base_url, model, provider, status: String::new(), testing: false }
    }

    pub fn set_result(&mut self, ok: bool, msg: String, cx: &mut Context<Self>) {
        self.testing = false;
        self.status = if ok { format!("✓ {msg}") } else { format!("✕ {msg}") };
        cx.notify();
    }
}

impl EventEmitter<PanelEvent> for SettingsPanel {}
impl EventEmitter<SettingsEvent> for SettingsPanel {}

impl Focusable for SettingsPanel {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus_handle.clone()
    }
}

fn field(label: &'static str, state: &Entity<gpui_kit::component::input::InputState>) -> gpui_kit::AnyElement {
    div()
        .flex()
        .items_center()
        .gap_2()
        .w_full()
        .child(div().w(px(90.)).text_xs().text_color(rgb(0x8e8b85)).child(label))
        .child(div().flex_1().child(gpui_kit::component::input::Input::new(state)))
        .into_any_element()
}

impl Render for SettingsPanel {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let status = self.status.clone();
        let testing = self.testing;
        div()
            .id("settings-scroll")
            .size_full()
            .overflow_y_scroll()
            .child(
                div()
                    .flex()
                    .flex_col()
                    .p_4()
                    .gap_3()
                    .child(div().text_sm().font_weight(FontWeight::MEDIUM).child("模型服务（OpenAI 兼容端点）"))
                    .child(field("Base URL", &self.base_url))
                    .child(field("模型 ID", &self.model))
                    .child(field("Provider", &self.provider))
                    .child(
                        div()
                            .flex()
                            .gap_2()
                            .items_center()
                            .child(
                                Button::new("settings-save")
                                    .primary()
                                    .xsmall()
                                    .label("保存")
                                    .on_click(cx.listener(|this, _, _, cx| {
                                        let base_url = this.base_url.read(cx).value().to_string();
                                        let model = this.model.read(cx).value().to_string();
                                        let provider = this.provider.read(cx).value().to_string();
                                        cx.emit(SettingsEvent::Save { base_url, model, provider });
                                    })),
                            )
                            .child(
                                Button::new("settings-test")
                                    .xsmall()
                                    .label(if testing { "测试中…" } else { "测试连接" })
                                    .on_click(cx.listener(|this, _, _, cx| {
                                        let base_url = this.base_url.read(cx).value().to_string();
                                        if base_url.trim().is_empty() {
                                            this.status = "先填 Base URL".into();
                                            cx.notify();
                                            return;
                                        }
                                        this.testing = true;
                                        this.status = String::new();
                                        cx.notify();
                                        cx.emit(SettingsEvent::Test(base_url));
                                    })),
                            ),
                    )
                    .when_true(!status.is_empty(), |d| {
                        d.child(
                            div()
                                .text_xs()
                                .text_color(if status.starts_with('✓') { rgb(0x7fae72) } else { rgb(0xc0605c) })
                                .child(SharedString::from(status)),
                        )
                    })
                    .child(
                        div()
                            .text_xs()
                            .text_color(rgb(0x5d5b57))
                            .child("局域网免 Key 端点直接可用；保存后下次 AI 调用生效。DSH 运行时（sdk profile）由应用自动带 --patch 挂载执笔能力包。"),
                    ),
            )
    }
}

impl BasePanel for SettingsPanel {
    fn panel_name(&self) -> &'static str {
        "settings-view"
    }
}
impl ComponentPanel for SettingsPanel {
    fn title(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div().text_sm().font_weight(FontWeight::MEDIUM).child("设置")
    }
}

/* ───────────────────────── 快照（浏览与恢复） ───────────────────────── */

#[derive(Debug, Clone)]
pub enum SnapEvent {
    Restore(String),
}

pub struct SnapshotsPanel {
    focus_handle: FocusHandle,
    handle: Rc<ProjectHandle>,
    chapter: u32,
    names: Vec<String>,
}

impl SnapshotsPanel {
    pub fn new(cx: &mut Context<Self>, handle: Rc<ProjectHandle>, chapter: u32) -> Self {
        let names = handle.snapshots(chapter);
        Self { focus_handle: cx.focus_handle(), handle, chapter, names }
    }
    pub fn reload(&mut self, chapter: u32, cx: &mut Context<Self>) {
        self.chapter = chapter;
        self.names = self.handle.snapshots(chapter);
        cx.notify();
    }
}

impl EventEmitter<PanelEvent> for SnapshotsPanel {}
impl EventEmitter<SnapEvent> for SnapshotsPanel {}

impl Focusable for SnapshotsPanel {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus_handle.clone()
    }
}

impl Render for SnapshotsPanel {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let mut rows: Vec<gpui_kit::AnyElement> = Vec::new();
        for name in self.names.iter().take(40) {
            let n = name.clone();
            rows.push(
                div()
                    .flex()
                    .items_center()
                    .gap_2()
                    .py_1()
                    .border_b_1()
                    .border_color(rgb(0x2e3138))
                    .child(div().flex_1().text_xs().child(SharedString::from(n.clone())))
                    .child(
                        Button::new(format!("restore-{n}"))
                            .xsmall()
                            .label("恢复")
                            .on_click(cx.listener(move |_this, _, _, cx| {
                                cx.emit(SnapEvent::Restore(n.clone()));
                            })),
                    )
                    .into_any_element(),
            );
        }
        div()
            .id("snap-scroll")
            .size_full()
            .overflow_y_scroll()
            .child(
                div()
                    .flex()
                    .flex_col()
                    .p_3()
                    .gap_2()
                    .child(
                        div()
                            .text_sm()
                            .font_weight(FontWeight::MEDIUM)
                            .child(SharedString::from(format!("第{}章快照（{} 份）", self.chapter, self.names.len()))),
                    )
                    .children(rows)
                    .when_true(self.names.is_empty(), |d| {
                        d.child(
                            div()
                                .text_xs()
                                .text_color(rgb(0x5d5b57))
                                .child("（还没有快照——采纳草稿/去AI味/恢复选区时自动创建）"),
                        )
                    }),
            )
    }
}

impl BasePanel for SnapshotsPanel {
    fn panel_name(&self) -> &'static str {
        "snapshots-view"
    }
}
impl ComponentPanel for SnapshotsPanel {
    fn title(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div().text_sm().font_weight(FontWeight::MEDIUM).child("快照")
    }
}

/* ───────────────────────── 导入目录 → 提案 ───────────────────────── */

#[derive(Debug, Clone)]
pub enum ImportEvent {
    Run(String),
}

pub struct ImportPanel {
    focus_handle: FocusHandle,
    /// 预留给面板内直接扫描（当前由 Workspace 后台执行）
    #[allow(dead_code)]
    handle: Rc<ProjectHandle>,
    path: Entity<gpui_kit::component::input::InputState>,
    last_result: String,
}

impl ImportPanel {
    pub fn new(window: &mut Window, cx: &mut Context<Self>, handle: Rc<ProjectHandle>) -> Self {
        let path = cx.new(|cx| {
            gpui_kit::component::input::InputState::new(window, cx)
                .placeholder("要导入的目录（.md/.txt，按「第N章」启发式分类）")
        });
        Self { focus_handle: cx.focus_handle(), handle, path, last_result: String::new() }
    }

    pub fn set_result(&mut self, msg: String, cx: &mut Context<Self>) {
        self.last_result = msg;
        cx.notify();
    }
}

impl EventEmitter<PanelEvent> for ImportPanel {}
impl EventEmitter<ImportEvent> for ImportPanel {}

impl Focusable for ImportPanel {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus_handle.clone()
    }
}

impl Render for ImportPanel {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let result = self.last_result.clone();
        div()
            .id("import-scroll")
            .size_full()
            .overflow_y_scroll()
            .child(
                div()
                    .flex()
                    .flex_col()
                    .p_4()
                    .gap_3()
                    .child(div().text_sm().font_weight(FontWeight::MEDIUM).child("导入旧稿（全部走提案，采纳才落库）"))
                    .child(
                        div()
                            .text_xs()
                            .text_color(rgb(0x8e8b85))
                            .child("规则：文件名/内容匹配「第N章」→ 草稿提案；「设定/大纲/人物」目录 → 设定提案；其余忽略。"),
                    )
                    .child(field("目录", &self.path))
                    .child(
                        Button::new("import-run")
                            .primary()
                            .xsmall()
                            .label("扫描并生成提案")
                            .on_click(cx.listener(|this, _, _, cx| {
                                let p = this.path.read(cx).value().to_string();
                                cx.emit(ImportEvent::Run(p));
                            })),
                    )
                    .when_true(!result.is_empty(), |d| {
                        d.child(
                            div()
                                .text_xs()
                                .text_color(rgb(0x7fae72))
                                .child(SharedString::from(result)),
                        )
                    }),
            )
    }
}

impl BasePanel for ImportPanel {
    fn panel_name(&self) -> &'static str {
        "import-view"
    }
}
impl ComponentPanel for ImportPanel {
    fn title(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div().text_sm().font_weight(FontWeight::MEDIUM).child("导入旧稿")
    }
}

/* ───────────────────────── 记忆图谱（列表视图） ───────────────────────── */

pub struct GraphPanel {
    focus_handle: FocusHandle,
    handle: Rc<ProjectHandle>,
}

impl GraphPanel {
    pub fn new(cx: &mut Context<Self>, handle: Rc<ProjectHandle>) -> Self {
        Self { focus_handle: cx.focus_handle(), handle }
    }
}

impl EventEmitter<PanelEvent> for GraphPanel {}

impl Focusable for GraphPanel {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus_handle.clone()
    }
}

impl Render for GraphPanel {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        let lines = self.handle.storylines();
        let events = self.handle.events();
        let facts = self.handle.memory_facts();

        let mut body: Vec<gpui_kit::AnyElement> = Vec::new();
        for l in &lines {
            let lid = l.get("id").and_then(|v| v.as_str()).unwrap_or("?").to_string();
            let name = l.get("name").and_then(|v| v.as_str()).unwrap_or("").to_string();
            body.push(
                div()
                    .flex()
                    .items_center()
                    .gap_2()
                    .pt_2()
                    .child(div().text_sm().font_weight(FontWeight::SEMIBOLD).text_color(rgb(0xc96f4a)).child(SharedString::from(format!("● {name}"))))
                    .child(div().text_xs().text_color(rgb(0x5d5b57)).child(SharedString::from(lid.clone())))
                    .into_any_element(),
            );
            let related: Vec<String> = events
                .iter()
                .filter(|e| e.get("storylineIDs").and_then(|s| s.as_array()).map(|a| a.iter().any(|x| x.as_str() == Some(lid.as_str()))).unwrap_or(false))
                .map(|e| format!("第{}章 {}", e.get("chapter").and_then(|c| c.as_u64()).unwrap_or(0), e.get("objectiveFact").and_then(|f| f.as_str()).unwrap_or("").chars().take(30).collect::<String>()))
                .collect();
            if related.is_empty() {
                body.push(div().pl_4().text_xs().text_color(rgb(0x5d5b57)).child("（暂无挂线事件）").into_any_element());
            } else {
                for r in related.iter().take(12) {
                    body.push(div().pl_4().text_xs().text_color(rgb(0x8e8b85)).child(SharedString::from(r.clone())).into_any_element());
                }
            }
        }
        if lines.is_empty() {
            body.push(
                div()
                    .text_xs()
                    .text_color(rgb(0x5d5b57))
                    .child("（还没有故事线/事件——建框架提案采纳后，这里按线展开事件与事实锚点）")
                    .into_any_element(),
            );
        }
        body.push(
            div()
                .pt_3()
                .text_xs()
                .text_color(rgb(0x5d5b57))
                .child(SharedString::from(format!("事实 {} 条 · 事件 {} 个", facts.len(), events.len())))
                .into_any_element(),
        );

        div()
            .id("graph-scroll")
            .size_full()
            .overflow_y_scroll()
            .child(div().flex().flex_col().p_3().gap_1().children(body))
    }
}

impl BasePanel for GraphPanel {
    fn panel_name(&self) -> &'static str {
        "graph-view"
    }
}
impl ComponentPanel for GraphPanel {
    fn title(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div().text_sm().font_weight(FontWeight::MEDIUM).child("记忆图谱")
    }
}
