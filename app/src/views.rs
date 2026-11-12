//! 台账页面板（M5 第一片）：设定 / 大纲·时间线 / 记忆 / 统计 / 伏笔 / 人物卡 / 设置 / 快照 / 导入 / 图谱。
//! 全部只读渲染真实磁盘数据；写操作一律走提案（按钮上抛事件给 Workspace）。
//!
//! 视觉一律取 `crate::theme` 令牌（唯一事实源，勿在此写裸色值）：
//! 页面底 BG、嵌套块 PANEL2+LINE 描边；状态语义一律 chip（FONT_TAG/RADIUS_CHIP/1px 语义描边）；
//! 台账码（线号/事件号/伏笔号/日期/快照文件名）一律 FAMILY_MONO+FONT_LABEL+FAINT；
//! 主按钮朱砂实心+白字、能力按钮朱砂软底+TEXT——经 `ButtonVariants::custom` 覆写库主题（规则 5/6/7）。

use std::rc::Rc;

use gpui_kit::component::button::{Button, ButtonCustomVariant, ButtonRounded, ButtonVariants as _};
use gpui_kit::component::empty::{
    Empty, EmptyDescription, EmptyHeader, EmptyMedia, EmptyMediaVariant, EmptyTitle,
};
use gpui_kit::component::table::{
    Table, TableBody, TableCell, TableHead, TableHeader, TableRow,
};
use gpui_kit::component::tag::Tag;
use gpui_kit::component::dock::{BasePanel, Panel as ComponentPanel, PanelEvent};
use gpui_kit::component::Sizable;
use gpui_kit::prelude::FluentBuilder;
use gpui_kit::{
    App, AppContext as _, Context, Entity, EventEmitter, FocusHandle, Focusable, FontWeight,
    InteractiveElement, IntoElement, ParentElement, Render, SharedString,
    StatefulInteractiveElement, Styled, Window, div, px, relative,
};

use crate::store::ProjectHandle;
use crate::theme;

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

/* ───────────────────────── 视觉公共件（chip / 表格 / 按钮令牌配方） ───────────────────────── */

/// 状态语言四色 + 中性 chip 档（规则 6）。
#[derive(Clone, Copy)]
#[allow(dead_code)]
enum Tone {
    Accent,
    Green,
    Blue,
    Amber,
    Red,
    Purple,
    Neutral,
}

impl Tone {
    fn text(self) -> gpui_kit::Hsla {
        let c = match self {
            Tone::Accent => theme::ACCENT(),
            Tone::Green => theme::GREEN(),
            Tone::Blue => theme::BLUE(),
            Tone::Amber => theme::AMBER(),
            Tone::Red => theme::RED(),
            Tone::Purple => theme::PURPLE(),
            Tone::Neutral => theme::DIM(),
        };
        c.into()
    }
    fn soft(self) -> gpui_kit::Rgba {
        match self {
            Tone::Accent => theme::ACCENT_SOFT(),
            Tone::Green => theme::GREEN_SOFT(),
            Tone::Blue => theme::BLUE_SOFT(),
            Tone::Amber => theme::AMBER_SOFT(),
            Tone::Red => theme::RED_SOFT(),
            Tone::Purple => theme::PURPLE().alpha(0.14),
            Tone::Neutral => theme::CHIP_NEUTRAL(),
        }
    }
    fn border(self) -> gpui_kit::Rgba {
        match self {
            Tone::Accent => theme::ACCENT_BORDER(),
            Tone::Green => theme::GREEN_BORDER(),
            Tone::Blue => theme::BLUE_BORDER(),
            // AMBER/PURPLE 无独立描边 token：取本色低透明档，禁止裸十六进制。
            Tone::Amber => theme::AMBER().alpha(0.4),
            Tone::Red => theme::RED().alpha(0.35),
            Tone::Purple => theme::PURPLE().alpha(0.4),
            Tone::Neutral => theme::LINE(),
        }
    }
}

/// 状态/节奏文案 → 语义色（纯展示映射，不动数据）。
fn status_tone(s: &str) -> Tone {
    if ["回收", "闭合", "完结", "完成", "收束", "揭开", "已解"].iter().any(|k| s.contains(k)) {
        Tone::Green
    } else if ["断线", "遗忘", "失联", "冲突", "危险", "逾期"]
        .iter()
        .any(|k| s.contains(k))
    {
        Tone::Red
    } else if ["放缓", "停滞", "搁置", "待续", "到期", "短期", "近期"]
        .iter()
        .any(|k| s.contains(k))
    {
        Tone::Amber
    } else if ["推进", "进行", "发展", "活跃", "展开", "埋"].iter().any(|k| s.contains(k)) {
        Tone::Blue
    } else {
        Tone::Neutral
    }
}

/// 语义状态 chip（规则 6/11：FONT_TAG + RADIUS_CHIP + soft 底 + 1px 语义描边）。
fn chip(text: impl Into<SharedString>, tone: Tone) -> gpui_kit::AnyElement {
    div()
        .flex()
        .items_center()
        .flex_shrink_0()
        .whitespace_nowrap()
        .px(theme::PAD_SM)
        .py(theme::PAD_XS)
        .rounded(theme::RADIUS_CHIP)
        .bg(tone.soft())
        .border_1()
        .border_color(tone.border())
        .line_height(relative(1.5))
        .text_size(theme::FONT_TAG)
        .text_color(tone.text())
        .child(text.into())
        .into_any_element()
}


/// 表格单元格语义：正文 / 次级 / 等宽台账码 / 强调等宽 / 强调文本 / 状态 chip / 节奏 chip。
enum Cell {
    Text(String),
    Dim(String),
    Code(String),
    CodeAccent(String),
    Hi(String),
    Chip(String, Tone),
    Pace(String, Tone),
}



/// 数据行：行距 py=GAP_SM、分隔线 LINE_SOFT（outline.html 表行档）。
/// kit Table：表头 + 数据行（替代手搓 div 行）。
fn kit_table(headers: &[&str], rows: Vec<TableRow>) -> gpui_kit::AnyElement {
    let mut body = TableBody::new();
    for r in rows {
        body = body.child(r);
    }
    let head = TableHeader::new().child(
        TableRow::new().children(
            headers
                .iter()
                .map(|h| TableHead::new().child(SharedString::from(h.to_string()))),
        ),
    );
    Table::new().w_full().child(head).child(body).into_any_element()
}

/// Tone → kit Tag（语义色徽标，规则 6/10）。
fn tag_for(text: &str, tone: &Tone) -> Tag {
    let t = match tone {
        Tone::Green => Tag::success(),
        Tone::Blue => Tag::info(),
        Tone::Red => Tag::danger(),
        Tone::Amber => Tag::warning(),
        Tone::Accent => Tag::primary(),
        _ => Tag::secondary(),
    };
    t.outline().child(SharedString::from(text.to_string()))
}

/// 数据行 = kit TableRow；单元格按 Cell 语义映射（chip/pace → Tag，码 → 等宽文本）。
fn row(cells: Vec<Cell>) -> TableRow {
    let mut tr = TableRow::new();
    for c in cells {
        let cell = match c {
            Cell::Text(t) => TableCell::new().child(SharedString::from(t)),
            Cell::Dim(t) => TableCell::new()
                .text_color(theme::FAINT())
                .child(SharedString::from(t)),
            Cell::Code(t) => TableCell::new()
                .font_family(theme::FAMILY_MONO)
                .text_color(theme::FAINT())
                .child(SharedString::from(t)),
            Cell::CodeAccent(t) => TableCell::new()
                .font_family(theme::FAMILY_MONO)
                .text_color(theme::ACCENT())
                .child(SharedString::from(t)),
            Cell::Hi(t) => TableCell::new()
                .font_weight(FontWeight::SEMIBOLD)
                .child(SharedString::from(t)),
            Cell::Chip(t, tone) | Cell::Pace(t, tone) => {
                TableCell::new().child(tag_for(&t, &tone))
            }
        };
        tr = tr.child(cell);
    }
    tr
}

/// kit Empty：统一空态（图标 + 标题 + 说明）。
fn kit_empty(title: &str, desc: &str) -> gpui_kit::AnyElement {
    Empty::new()
        .header(
            EmptyHeader::new()
                .media(EmptyMedia::new().with_variant(EmptyMediaVariant::Icon))
                .title(EmptyTitle::new().child(SharedString::from(title.to_string())))
                .description(EmptyDescription::new().child(SharedString::from(desc.to_string()))),
        )
        .into_any_element()
}



/// 区块标题（面板内 h2 档：15px SEMIBOLD 正文色）。
fn sec_title(text: impl Into<SharedString>) -> gpui_kit::Div {
    div()
        .text_size(theme::FONT_TITLE_SM)
        .font_weight(FontWeight::SEMIBOLD)
        .text_color(theme::TEXT())
        .child(text.into())
}

/// 空态引导（规则 15）：PANEL 卡 + 引导语指向动作，禁止静默空白。
fn empty_hint(title: &str, desc: &str) -> gpui_kit::AnyElement {
    kit_empty(title, desc)
}

/// 面板内容列容器：页面底 BG + 统一栏边距（规则 4/12）。
fn panel_col<'a>() -> gpui_kit::Div {
    div()
        .flex()
        .flex_col()
        .gap(theme::GAP_GROUP_SM)
        .px(theme::PAD_XL)
        .pt(theme::PAD_LG)
        .pb(theme::PAD_XL)
}

/// 面板根（滚动容器）：墨底 BG。
fn panel_root(id: &'static str) -> gpui_kit::Stateful<gpui_kit::Div> {
    div().id(id).size_full().overflow_y_scroll().bg(theme::BG())
}

/// 主能力按钮（规则 5）：朱砂软底 + TEXT 文字 + hover 提亮一档；custom 变体覆写库主题。
fn cap_btn(id: &'static str, label: &'static str, cx: &App) -> Button {
    Button::new(id)
        .label(label)
        .xsmall()
        .custom(
            ButtonCustomVariant::new(cx)
                .color(theme::ACCENT_SOFT().into())
                .foreground(theme::TEXT().into())
                .hover(theme::ACCENT().alpha(0.24).into())
                .active(theme::ACCENT().alpha(0.34).into()),
        )
        .rounded(ButtonRounded::Size(theme::RADIUS_BTN_SM))
        .text_size(theme::FONT_TAG)
}

/// 主按钮（规则 5）：ACCENT 实心 + ON_ACCENT 白字 + RADIUS_BTN；hover 提亮、按下压暗。
fn primary_btn(id: &'static str, label: &'static str, cx: &App) -> Button {
    Button::new(id)
        .label(label)
        .custom(
            ButtonCustomVariant::new(cx)
                .color(theme::ACCENT().into())
                .foreground(theme::ON_ACCENT().into())
                .hover(theme::ACCENT().blend(theme::ON_ACCENT().alpha(0.12)).into())
                .active(theme::ACCENT().blend(theme::BG().alpha(0.16)).into()),
        )
        .rounded(ButtonRounded::Size(theme::RADIUS_BTN))
        .text_size(theme::FONT_BODY)
}

/// 破坏性动作按钮（规则 6/7）：RED_SOFT 底 + RED 字，hover 转深红底。
fn danger_btn(id: String, label: &'static str, cx: &App) -> Button {
    Button::new(id)
        .label(label)
        .xsmall()
        .custom(
            ButtonCustomVariant::new(cx)
                .color(theme::RED_SOFT().into())
                .foreground(theme::RED().into())
                .hover(theme::RED().alpha(0.3).into())
                .active(theme::RED().alpha(0.42).into()),
        )
        .rounded(ButtonRounded::Size(theme::RADIUS_BTN_SM))
        .text_size(theme::FONT_TAG)
}

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

        // 左侧条目列：选中行 ACCENT_SOFT 底 + TEXT 字；未选中 DIM 字 + hover 提亮（规则 5/7）。
        let mut left: Vec<gpui_kit::AnyElement> = Vec::new();
        for (gi, (cat, items)) in groups.into_iter().enumerate() {
            let mut g = div()
                .flex()
                .items_center()
                .gap(theme::GAP_SM)
                .pt(if gi == 0 { px(0.) } else { theme::GAP_GROUP_SM })
                .text_size(theme::FONT_LABEL)
                .font_weight(FontWeight::MEDIUM)
                .text_color(theme::FAINT())
                .child(SharedString::from(format!("{cat} · {}", items.len())));
            if gi > 0 {
                g = g.border_t_1().border_color(theme::LINE()).mt(theme::GAP_SM);
            }
            left.push(g.into_any_element());
            for (title, _, _) in items {
                let t = title.clone();
                let sel = self.selected.as_deref() == Some(title.as_str());
                let mut item = div()
                    .id(format!("canon-{title}"))
                    .flex()
                    .items_center()
                    .w_full()
                    .px(theme::PAD_SM)
                    .py(theme::PAD_XS)
                    .rounded(theme::RADIUS_BTN_SM)
                    .text_size(theme::FONT_SMALL)
                    .line_height(relative(theme::LINE_HEIGHT_BODY))
                    .text_color(if sel { theme::TEXT() } else { theme::DIM() })
                    .hover(|s| s.bg(theme::ACCENT().alpha(0.1)).text_color(theme::TEXT()))
                    .active(|s| s.bg(theme::ACCENT().alpha(0.2)))
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.selected = Some(t.clone());
                        cx.notify();
                    }));
                if sel {
                    item = item
                        .bg(theme::ACCENT_SOFT())
                        .border_1()
                        .border_color(theme::ACCENT().alpha(0.28));
                }
                left.push(item.child(SharedString::from(title)).into_any_element());
            }
        }

        // 右侧文档卡：PANEL2 次级面 + LINE 描边 + 卡片圆角（规则 4/11）。
        let body = if let Some((title, cat, content)) = cur {
            div()
                .flex_1()
                .min_w_0()
                .p(theme::PAD_LG)
                .rounded(theme::RADIUS_CARD)
                .bg(theme::PANEL2())
                .border_1()
                .border_color(theme::LINE())
                .child(
                    div()
                        .flex()
                        .items_center()
                        .gap(theme::GAP_MD)
                        .pb(theme::GAP_MD)
                        .mb(theme::GAP_MD)
                        .border_b_1()
                        .border_color(theme::LINE_SOFT())
                        .child(
                            div()
                                .text_size(theme::FONT_TITLE_SM)
                                .font_family(theme::FAMILY_SERIF)
                                .font_weight(FontWeight::SEMIBOLD)
                                .text_color(theme::TEXT())
                                .child(SharedString::from(title.clone())),
                        )
                        .child(chip(cat, Tone::Neutral)),
                )
                .child(
                    div()
                        .text_size(theme::FONT_BODY)
                        .line_height(relative(theme::LINE_HEIGHT_BODY))
                        .text_color(theme::TEXT())
                        .child(SharedString::from(content.chars().take(4000).collect::<String>())),
                )
                .into_any_element()
        } else {
            empty_hint("设定库为空", "用「AI 归类设定」或导入后自动归类，条目会按世界观分类收进来。")
        };

        panel_root("canon-scroll").child(
            panel_col()
                .child(
                    div()
                        .flex()
                        .items_center()
                        .justify_between()
                        .gap(theme::GAP_MD)
                        .child(sec_title("设定"))
                        .child(cap_btn("canon-classify", "AI 归类（提案）", cx).on_click(
                            cx.listener(|_this, _, _, cx| {
                                cx.emit(ViewEvent::RunCapability("classify"));
                            }),
                        )),
                )
                .child(
                    div()
                        .flex()
                        .items_start()
                        .gap(theme::GAP_GROUP_LG)
                        .w_full()
                        .child(
                            div()
                                .w(theme::SIDEBAR_W)
                                .flex_shrink_0()
                                .flex()
                                .flex_col()
                                .gap(theme::GAP_XS)
                                .children(left),
                        )
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
        div()
            .text_size(theme::FONT_BODY)
            .font_weight(FontWeight::MEDIUM)
            .text_color(theme::TEXT())
            .child("设定")
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

impl Render for OutlinePanel {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let lines = self.handle.storylines();
        let events = self.handle.events();

        let mut line_rows = Vec::new();
        for l in &lines {
            let status = l.get("status").and_then(|v| v.as_str()).unwrap_or("").to_string();
            let tone = status_tone(&status);
            let mut cells = vec![
                (Cell::Code(l.get("id").and_then(|v| v.as_str()).unwrap_or("?").to_string())),
                Cell::Text(l.get("name").and_then(|v| v.as_str()).unwrap_or("").to_string()),
            ];
            if !status.is_empty() {
                cells.push(Cell::Chip(status, tone));
            }
            line_rows.push(row(cells));
        }
        let mut event_rows = Vec::new();
        for e in events.iter().take(40) {
            let revealed = e
                .get("revealed")
                .and_then(|v| v.as_bool())
                .map(|r| if r { "读者已知" } else { "暗线" })
                .unwrap_or("");
            let mut cells = vec![
                Cell::Code(e.get("id").and_then(|v| v.as_str()).unwrap_or("?").to_string()),
                Cell::Dim(format!("第{}", e.get("chapter").and_then(|c| c.as_u64()).unwrap_or(0))),
                Cell::Text(
                    e.get("objectiveFact")
                        .and_then(|v| v.as_str())
                        .unwrap_or("")
                        .chars()
                        .take(60)
                        .collect(),
                ),
            ];
            if !revealed.is_empty() {
                cells.push(Cell::Chip(
                    revealed.to_string(),
                    if revealed == "暗线" { Tone::Neutral } else { Tone::Blue },
                ));
            }
            event_rows.push(row(cells));
        }

        panel_root("outline-scroll").child(
            panel_col()
                .child(
                    div()
                        .flex()
                        .items_center()
                        .justify_between()
                        .gap(theme::GAP_MD)
                        .child(sec_title("故事线"))
                        .child(
                            div()
                                .flex()
                                .items_center()
                                .gap(theme::GAP_SM)
                                .child(cap_btn("outline-sync", "同步大纲（提案）", cx).on_click(
                                    cx.listener(|_this, _, _, cx| {
                                        cx.emit(ViewEvent::RunCapability("outsync"));
                                    }),
                                ))
                                .child(cap_btn("outline-audit", "全书连贯性审查", cx).on_click(
                                    cx.listener(|_this, _, _, cx| {
                                        cx.emit(ViewEvent::RunCapability("audit"));
                                    }),
                                )),
                        ),
                )
                .child(kit_table(&["线", "名称", "状态"], line_rows))
                .when_true(lines.is_empty(), |d| {
                    d.child(empty_hint("还没有故事线", "新书建框架提案采纳后，这里出现线号 · 名称 · 状态。"))
                })
                .child(
                    div()
                        .flex()
                        .flex_col()
                        .gap(theme::GAP_MD)
                        .border_t_1()
                        .border_color(theme::LINE())
                        .pt(theme::GAP_GROUP)
                        .child(sec_title("事件时间线（作者真相｜读者已知）"))
                        .child(kit_table(&["事件", "章", "作者真相", "读者已知"], event_rows))
                        .when_true(events.is_empty(), |d| {
                            d.child(empty_hint("还没有事件", "用「AI 建时间线」提案，从正文抽取作者真相事件。"))
                        }),
                ),
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
        div()
            .text_size(theme::FONT_BODY)
            .font_weight(FontWeight::MEDIUM)
            .text_color(theme::TEXT())
            .child("大纲 · 时间线")
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
                Cell::Text(f.get("subject").and_then(|v| v.as_str()).unwrap_or("").to_string()),
                Cell::Hi(f.get("predicate").and_then(|v| v.as_str()).unwrap_or("").to_string()),
                Cell::Text(
                    f.get("object").and_then(|v| v.as_str()).unwrap_or("").chars().take(50).collect(),
                ),
                Cell::Dim(format!("第{}章", f.get("fromChapter").and_then(|v| v.as_u64()).unwrap_or(0))),
                Cell::Chip(
                    if public { "读者可见" } else { "暗线" }.to_string(),
                    if public { Tone::Blue } else { Tone::Neutral },
                ),
            ]));
        }
        panel_root("memory-scroll").child(
            panel_col()
                .child(
                    div()
                        .flex()
                        .items_center()
                        .justify_between()
                        .gap(theme::GAP_MD)
                        .child(sec_title("事实账本（双时态）"))
                        .child(cap_btn("memory-extract", "让AI记一笔", cx).on_click(
                            cx.listener(|_this, _, _, cx| {
                                cx.emit(ViewEvent::RunCapability("memory"));
                            }),
                        )),
                )
                .child(kit_table(&["主语", "谓语", "宾语", "生效", "读者"], rows))
                .when_true(facts.is_empty(), |d| {
                    d.child(empty_hint("还没有事实", "写完一章点「让AI记一笔」，谓宾结构与生效章节会收进这本账。"))
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
        div()
            .text_size(theme::FONT_BODY)
            .font_weight(FontWeight::MEDIUM)
            .text_color(theme::TEXT())
            .child("记忆中枢")
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

        // 柱状图：BAR_TRACK 轨道 + 朱砂进行中填充 + RADIUS_TAG 圆角，日期码等宽 FAINT（规则 10/13）。
        let bars: Vec<gpui_kit::AnyElement> = recent
            .iter()
            .map(|(day, w)| {
                div()
                    .flex()
                    .flex_col()
                    .items_center()
                    .gap(theme::GAP_SM)
                    .w(px(38.))
                    .child(
                        div()
                            .w(px(22.))
                            .h(px(94.))
                            .flex()
                            .flex_col()
                            .justify_end()
                            .overflow_hidden()
                            .rounded(theme::RADIUS_TAG)
                            .bg(theme::BAR_TRACK())
                            .child(
                                div()
                                    .w_full()
                                    .h(px((*w as f32 / max as f32 * 90.).max(4.)))
                                    .bg(theme::ACCENT()),
                            ),
                    )
                    .child(
                        div()
                            .font_family(theme::FAMILY_MONO)
                            .text_size(theme::FONT_MICRO)
                            .text_color(theme::FAINT())
                            .child(SharedString::from(day[5..].to_string())),
                    )
                    .into_any_element()
            })
            .collect();

        panel_root("stats-scroll").child(
            panel_col()
                .child(sec_title("每日净增字数（近 14 天）"))
                .child(div().flex().flex_wrap().gap(theme::GAP_MD).items_end().children(bars))
                .child(
                    div()
                        .flex()
                        .flex_col()
                        .gap(theme::GAP_MD)
                        .border_t_1()
                        .border_color(theme::LINE())
                        .pt(theme::GAP_GROUP)
                        .child(sec_title("提案账本"))
                        .child(
                            div()
                                .flex()
                                .flex_wrap()
                                .gap(theme::GAP_MD)
                                .items_center()
                                .child(chip(format!("待裁决 {pending}"), Tone::Accent))
                                .child(chip(format!("已采纳 {accepted}"), Tone::Green))
                                .child(chip(format!("已拒收 {rejected}"), Tone::Red))
                                .child(chip(format!("累计 {}", proposals.len()), Tone::Neutral)),
                        ),
                ),
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
        div()
            .text_size(theme::FONT_BODY)
            .font_weight(FontWeight::MEDIUM)
            .text_color(theme::TEXT())
            .child("写作统计")
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
            let status = c.get("status").and_then(|v| v.as_str()).unwrap_or("").to_string();
            let tone = status_tone(&status);
            let timing = c.get("timing").and_then(|v| v.as_str()).unwrap_or("").to_string();
            let mut cells = vec![
                Cell::CodeAccent(c.get("id").and_then(|v| v.as_str()).unwrap_or("?").to_string()),
                Cell::Text(c.get("title").and_then(|v| v.as_str()).unwrap_or("").chars().take(40).collect()),
            ];
            if !status.is_empty() {
                cells.push(Cell::Chip(status, tone));
            }
            cells.push(Cell::Code(format!("埋{}", c.get("plantedChapter").and_then(|v| v.as_u64()).unwrap_or(0))));
            cells.push(Cell::Code(format!("动{}", c.get("lastActionChapter").and_then(|v| v.as_u64()).unwrap_or(0))));
            if !timing.is_empty() {
                cells.push(Cell::Pace(timing, status_tone(c.get("timing").and_then(|v| v.as_str()).unwrap_or(""))));
            }
            rows.push(row(cells));
        }
        panel_root("clues-scroll").child(
            panel_col()
                .child(
                    div()
                        .flex()
                        .items_center()
                        .justify_between()
                        .gap(theme::GAP_MD)
                        .child(
                            div()
                                .flex()
                                .items_center()
                                .gap(theme::GAP_MD)
                                .child(sec_title("伏笔台账"))
                                .child(
                                    div()
                                        .text_size(theme::FONT_LABEL)
                                        .text_color(theme::FAINT())
                                        .child(SharedString::from(format!("{} 条", clues.len()))),
                                ),
                        )
                        .child(cap_btn("clues-inventory", "AI 盘点（提案）", cx).on_click(
                            cx.listener(|_this, _, _, cx| {
                                cx.emit(ViewEvent::RunCapability("inventory"));
                            }),
                        )),
                )
                .child(kit_table(&["编号", "标题 / 详情", "节奏", "埋于", "最近动作", "状态"], rows))
                .when_true(clues.is_empty(), |d| {
                    d.child(empty_hint("台账为空", "「AI 盘点伏笔」提案可从正文重建埋点、推进与节奏。"))
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
        div()
            .text_size(theme::FONT_BODY)
            .font_weight(FontWeight::MEDIUM)
            .text_color(theme::TEXT())
            .child("伏笔台账")
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
    fn render(&mut self, _: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
        let aliases = self.handle.aliases();
        let facts = self.handle.memory_facts();
        let mut cards = Vec::new();
        for a in aliases.iter().take(60) {
            let canon = a.get("canonicalName").and_then(|v| v.as_str()).unwrap_or("?").to_string();
            let subs: Vec<String> = a
                .get("aliases")
                .and_then(|v| v.as_array())
                .map(|arr| arr.iter().filter_map(|x| x.as_str().map(String::from)).collect())
                .unwrap_or_default();
            let mine: Vec<&serde_json::Value> = facts
                .iter()
                .filter(|f| f.get("subject").and_then(|s| s.as_str()) == Some(canon.as_str()))
                .collect();
            let first_ch = mine
                .iter()
                .filter_map(|f| f.get("fromChapter").and_then(|c| c.as_u64()))
                .min();
            cards.push(character_card(&canon, &subs, mine.len(), first_ch));
        }
        panel_root("characters-scroll").child(
            panel_col()
                .child(sec_title("人物卡"))
                .child(
                    div()
                        .text_size(theme::FONT_LABEL)
                        .text_color(theme::FAINT())
                        .child("按别名归一聚合；事实来自记忆提取提案。AI 可用 propose_aliases / propose_memory 增补，采纳后自动入卡。"),
                )
                .child(div().flex().flex_wrap().gap(theme::GAP_GROUP).children(cards))
                .when_true(aliases.is_empty(), |d| {
                    d.child(empty_hint("还没有别名档案", "记忆提取 / 人物归一提案采纳后，本名与别名会在这里归一。"))
                }),
        )
    }
}

/// 人物卡（游戏卡牌式）：上部立绘位（首字巨印 + 渐变色带 + 朱砂角印），
/// 下部称谓 / 别名 / 事实账。色带按人名哈希在四语义色相间轮换（与书封同一套语言）。
fn character_card(
    canon: &str,
    aliases: &[String],
    fact_count: usize,
    first_chapter: Option<u64>,
) -> gpui_kit::AnyElement {
    let h = canon
        .chars()
        .fold(7u32, |acc, c| acc.wrapping_mul(31).wrapping_add(c as u32));
    let hue = match h % 4 {
        0 => theme::ACCENT(),
        1 => theme::BLUE(),
        2 => theme::GREEN(),
        _ => theme::PURPLE(),
    };
    let glyph: String = canon.chars().take(1).collect();
    div()
        .w(px(224.))
        .flex()
        .flex_col()
        .rounded(theme::RADIUS_CARD)
        .bg(theme::PANEL())
        .border_1()
        .border_color(theme::LINE())
        .overflow_hidden()
        .shadow_sm()
        .hover(|d| d.border_color(theme::ACCENT_BORDER()))
        .child(
            div()
                .relative()
                .w_full()
                .h(px(132.))
                .flex()
                .items_center()
                .justify_center()
                .bg(theme::BG().blend(hue.alpha(0.20)))
                .child(
                    div()
                        .font_family(theme::FAMILY_SERIF)
                        .text_size(px(56.))
                        .font_weight(FontWeight::BOLD)
                        .text_color(theme::COVER_TEXT())
                        .child(SharedString::from(glyph)),
                )
                .child(
                    div()
                        .absolute()
                        .right(px(10.))
                        .bottom(px(10.))
                        .w(px(22.))
                        .h(px(22.))
                        .flex()
                        .items_center()
                        .justify_center()
                        .rounded(px(4.))
                        .bg(theme::ACCENT().alpha(0.9))
                        .child(
                            div()
                                .text_size(px(12.))
                                .font_family(theme::FAMILY_SERIF)
                                .text_color(theme::ON_ACCENT())
                                .child("执"),
                        ),
                )
                .child(
                    div()
                        .absolute()
                        .left(px(0.))
                        .top(px(0.))
                        .right(px(0.))
                        .h(px(3.))
                        .bg(hue.alpha(0.75)),
                ),
        )
        .child(
            div()
                .flex()
                .flex_col()
                .gap(theme::GAP_SM)
                .px(theme::PAD_MD)
                .py(theme::PAD_SM)
                .child(
                    div()
                        .font_family(theme::FAMILY_SERIF)
                        .text_size(theme::FONT_TITLE_SM)
                        .font_weight(FontWeight::SEMIBOLD)
                        .text_color(theme::TEXT())
                        .child(SharedString::from(canon.to_string())),
                )
                .child(
                    div()
                        .text_size(theme::FONT_LABEL)
                        .text_color(theme::DIM())
                        .child(SharedString::from(if aliases.is_empty() {
                            "（无别名）".to_string()
                        } else {
                            format!("又作：{}", aliases.join("、"))
                        })),
                )
                .child(
                    div()
                        .flex()
                        .items_center()
                        .gap(theme::GAP_SM)
                        .child(
                            Tag::secondary()
                                .outline()
                                .child(SharedString::from(format!("{fact_count} 条事实"))),
                        )
                        .when(first_chapter.is_some(), |d| {
                            d.child(
                                Tag::secondary()
                                    .outline()
                                    .child(SharedString::from(format!(
                                        "首见 第{}章",
                                        first_chapter.unwrap_or(0)
                                    ))),
                            )
                        }),
                ),
        )
        .into_any_element()
}

impl BasePanel for CharactersPanel {
    fn panel_name(&self) -> &'static str {
        "characters-view"
    }
}
impl ComponentPanel for CharactersPanel {
    fn title(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div()
            .text_size(theme::FONT_BODY)
            .font_weight(FontWeight::MEDIUM)
            .text_color(theme::TEXT())
            .child("人物卡")
    }
}


/* ───────────────────────── 设置（端点 + 连通测试） ───────────────────────── */

#[derive(Debug, Clone)]
pub enum SettingsEvent {
    /// 保存整份设置
    Persist(crate::store::AppSettings),
    Test(String),
    /// 刷新账号余额
    RefreshAccount,
}

pub struct SettingsPanel {
    focus_handle: FocusHandle,
    base_url: Entity<gpui_kit::component::input::InputState>,
    model: Entity<gpui_kit::component::input::InputState>,
    provider: Entity<gpui_kit::component::input::InputState>,
    /// 推理强度：low / medium / high（空 = 交给 DSH 默认）
    effort: Entity<gpui_kit::component::input::InputState>,
    /// 默认写作档位：assist / cowrite / ghostwrite
    writing_mode: Entity<gpui_kit::component::input::InputState>,
    /// 每日字数目标（0 = 不设）
    daily_goal: Entity<gpui_kit::component::input::InputState>,
    /// 允许联网 / 允许子 agent
    web_enabled: bool,
    subagent_enabled: bool,
    /// 账号快照（余额、登录状态）；None = 还没查过
    account: Option<crate::account::AccountSnapshot>,
    /// 余额正在查
    account_busy: bool,
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
        let effort = cx.new(|cx| {
            gpui_kit::component::input::InputState::new(window, cx)
                .placeholder("推理强度：low / medium / high（留空用默认）")
                .default_value(cfg.reasoning_effort)
        });
        let writing_mode = cx.new(|cx| {
            gpui_kit::component::input::InputState::new(window, cx)
                .placeholder("assist / cowrite / ghostwrite")
                .default_value(if cfg.writing_mode.is_empty() {
                    "assist".into()
                } else {
                    cfg.writing_mode
                })
        });
        let daily_goal = cx.new(|cx| {
            gpui_kit::component::input::InputState::new(window, cx)
                .placeholder("每日字数目标（0 = 不设）")
                .default_value(if cfg.daily_goal == 0 {
                    String::new()
                } else {
                    cfg.daily_goal.to_string()
                })
        });
        let web_enabled = cfg.web_enabled;
        let subagent_enabled = cfg.subagent_enabled;
        Self {
            focus_handle: cx.focus_handle(),
            base_url,
            model,
            provider,
            effort,
            writing_mode,
            daily_goal,
            web_enabled,
            subagent_enabled,
            account: None,
            account_busy: false,
            status: String::new(),
            testing: false,
        }
    }

    pub fn set_result(&mut self, ok: bool, msg: String, cx: &mut Context<Self>) {
        self.testing = false;
        self.status = if ok { format!("✓ {msg}") } else { format!("✕ {msg}") };
        cx.notify();
    }

    /// 余额查询回来（后台线程做完后回填）。
    pub fn set_account(&mut self, snap: crate::account::AccountSnapshot, cx: &mut Context<Self>) {
        self.account_busy = false;
        self.account = Some(snap);
        cx.notify();
    }

    /// 保存时把手上的全部设置一起交给宿主。
    fn snapshot_values(&self, cx: &App) -> crate::store::AppSettings {
        let mut cfg = crate::store::load_settings();
        cfg.base_url = self.base_url.read(cx).value().to_string();
        cfg.model = self.model.read(cx).value().to_string();
        cfg.provider = self.provider.read(cx).value().to_string();
        cfg.reasoning_effort = self.effort.read(cx).value().to_string();
        cfg.writing_mode = self.writing_mode.read(cx).value().to_string();
        cfg.daily_goal = self
            .daily_goal
            .read(cx)
            .value()
            .trim()
            .parse()
            .unwrap_or(0);
        cfg.web_enabled = self.web_enabled;
        cfg.subagent_enabled = self.subagent_enabled;
        cfg
    }
}

impl EventEmitter<PanelEvent> for SettingsPanel {}
impl EventEmitter<SettingsEvent> for SettingsPanel {}

impl Focusable for SettingsPanel {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus_handle.clone()
    }
}

/// 开关行：点击整行切换。用文字「开 / 关」而不是画一个假的 switch，
/// 因为库里没有现成的 switch 组件，画一个像但不能拖的控件比文字更误导。
fn toggle_row<F>(
    id: &'static str,
    label: &'static str,
    on: bool,
    cx: &mut Context<SettingsPanel>,
    toggle: F,
) -> gpui_kit::AnyElement
where
    F: Fn(&mut SettingsPanel, &mut Context<SettingsPanel>) + 'static,
{
    let toggle = std::rc::Rc::new(toggle);
    div()
        .id(id)
        .flex()
        .items_center()
        .gap(theme::GAP_MD)
        .py(px(4.))
        .cursor_pointer()
        .child(
            div()
                .w(px(28.))
                .text_size(theme::FONT_MICRO)
                .text_color(if on { theme::GREEN() } else { theme::FAINT() })
                .child(if on { "开" } else { "关" }),
        )
        .child(
            div()
                .flex_1()
                .text_size(theme::FONT_LABEL)
                .text_color(theme::TEXT())
                .child(label),
        )
        .on_click(cx.listener(move |this, _, _, cx| toggle(this, cx)))
        .into_any_element()
}

/// 模式说明行：左侧模式名，右侧一句人话。
fn mode_line(name: &'static str, detail: &'static str) -> gpui_kit::AnyElement {
    div()
        .flex()
        .items_start()
        .gap(theme::GAP_MD)
        .child(
            div()
                .w(px(40.))
                .flex_shrink_0()
                .text_size(theme::FONT_LABEL)
                .text_color(theme::ACCENT())
                .child(name),
        )
        .child(
            div()
                .flex_1()
                .text_size(theme::FONT_MICRO)
                .line_height(relative(theme::LINE_HEIGHT_BODY))
                .text_color(theme::DIM())
                .child(detail),
        )
        .into_any_element()
}

/// 表单行：标签 DIM；输入框覆写库主题为 PANEL2 底 + LINE 1px 描边 + 按钮圆角（规则 4/5）。
fn field(label: &'static str, state: &Entity<gpui_kit::component::input::InputState>) -> gpui_kit::AnyElement {
    div()
        .flex()
        .items_center()
        .gap(theme::GAP_MD)
        .w_full()
        .child(
            div()
                .w(px(90.))
                .flex_shrink_0()
                .text_size(theme::FONT_LABEL)
                .text_color(theme::DIM())
                .child(label),
        )
        .child(div().flex_1().min_w_0().child(
            gpui_kit::component::input::Input::new(state)
                .bg(theme::PANEL2())
                .border_1()
                .border_color(theme::LINE())
                .rounded(theme::RADIUS_BTN)
                .text_size(theme::FONT_BODY)
                .text_color(theme::TEXT()),
        ))
        .into_any_element()
}

impl Render for SettingsPanel {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let status = self.status.clone();
        let testing = self.testing;
        // 账号卡：余额是作者最关心的“还能用多久”
        let account_card = {
            let (line, tone) = match &self.account {
                None => ("未查询".to_string(), Tone::Neutral),
                Some(a) => {
                    if let Some(err) = &a.error {
                        (err.clone(), Tone::Amber)
                    } else if a.signed_in {
                        let bonus = if a.bonus.is_empty() || a.bonus == "0" {
                            String::new()
                        } else {
                            format!("（赠金 {}）", a.bonus)
                        };
                        (
                            format!("余额 ¥{}{bonus} · 累计消费 ¥{}", a.balance, a.spent),
                            Tone::Green,
                        )
                    } else {
                        ("未登录".to_string(), Tone::Amber)
                    }
                }
            };
            let busy = self.account_busy;
            div()
                .flex()
                .flex_col()
                .gap(theme::GAP_MD)
                .p(theme::PAD_LG)
                .bg(theme::PANEL())
                .border_1()
                .border_color(theme::LINE())
                .rounded(theme::RADIUS_CARD)
                .child(chip(SharedString::from(line), tone))
                .child(
                    div()
                        .flex()
                        .items_center()
                        .gap(theme::GAP_MD)
                        .child(
                            cap_btn(
                                "account-refresh",
                                if busy { "查询中…" } else { "刷新余额" },
                                cx,
                            )
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.account_busy = true;
                                cx.notify();
                                cx.emit(SettingsEvent::RefreshAccount);
                            })),
                        )
                        .when_true(busy, |d| d.child(chip("正在查询", Tone::Blue))),
                )
                .child(
                    div()
                        .text_size(theme::FONT_MICRO)
                        .line_height(relative(theme::LINE_HEIGHT_BODY))
                        .text_color(theme::FAINT())
                        .child("账号凭据与 DSH 共用一份（在 DSH 里登录一次即可）。只读余额，不代替登录。"),
                )
        };

        panel_root("settings-scroll").child(
            panel_col()
                .child(sec_title("账号与余额"))
                .child(account_card)
                .child(sec_title("模型服务（OpenAI 兼容端点）"))
                .child(
                    div()
                        .flex()
                        .flex_col()
                        .gap(theme::GAP_MD)
                        .p(theme::PAD_LG)
                        .bg(theme::PANEL())
                        .border_1()
                        .border_color(theme::LINE())
                        .rounded(theme::RADIUS_CARD)
                        .child(field("Base URL", &self.base_url))
                        .child(field("模型 ID", &self.model))
                        .child(field("Provider", &self.provider))
                        .child(field("推理强度", &self.effort)),
                )
                .child(
                    div()
                        .flex()
                        .gap(theme::GAP_MD)
                        .items_center()
                        .child(primary_btn("settings-save", "保存", cx).on_click(cx.listener(|this, _, _, cx| {
                            // 整份交出去：模型、推理强度、写作档位、日目标、开关一次写盘
                            cx.emit(SettingsEvent::Persist(this.snapshot_values(cx)));
                        })))
                        .child(
                            cap_btn("settings-test", if testing { "测试中…" } else { "测试连接" }, cx)
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
                        )
                        // 运行中 = BLUE 语义 chip（规则 6）。
                        .when_true(testing, |d| d.child(chip("连通测试进行中", Tone::Blue))),
                )
                .when_true(!status.is_empty(), |d| {
                    d.child(
                        div()
                            .flex()
                            .items_center()
                            .gap(theme::GAP_MD)
                            .child(chip(
                                SharedString::from(status.clone()),
                                if status.starts_with('✓') {
                                    Tone::Green
                                } else if status.starts_with('✕') {
                                    Tone::Red
                                } else {
                                    Tone::Amber
                                },
                            )),
                    )
                })
                .child(sec_title("写作"))
                .child(
                    div()
                        .flex()
                        .flex_col()
                        .gap(theme::GAP_MD)
                        .p(theme::PAD_LG)
                        .bg(theme::PANEL())
                        .border_1()
                        .border_color(theme::LINE())
                        .rounded(theme::RADIUS_CARD)
                        .child(field("默认写作档位", &self.writing_mode))
                        .child(field("每日字数目标", &self.daily_goal))
                        .child(
                            div()
                                .text_size(theme::FONT_MICRO)
                                .text_color(theme::FAINT())
                                .child("assist 只提案 · cowrite 可写台账 · ghostwrite 还可写授权章节的正文"),
                        ),
                )
                .child(sec_title("AI 权限"))
                .child(
                    div()
                        .flex()
                        .flex_col()
                        .gap(theme::GAP_SM)
                        .p(theme::PAD_LG)
                        .bg(theme::PANEL())
                        .border_1()
                        .border_color(theme::LINE())
                        .rounded(theme::RADIUS_CARD)
                        .child(toggle_row(
                            "settings-web",
                            "允许联网检索",
                            self.web_enabled,
                            cx,
                            |this, cx| {
                                this.web_enabled = !this.web_enabled;
                                cx.notify();
                            },
                        ))
                        .child(toggle_row(
                            "settings-subagent",
                            "允许派子 agent（人物/关系/世界线等专属 Agent）",
                            self.subagent_enabled,
                            cx,
                            |this, cx| {
                                this.subagent_enabled = !this.subagent_enabled;
                                cx.notify();
                            },
                        )),
                )
                .child(sec_title("Agent 模式与插件"))
                .child(
                    div()
                        .flex()
                        .flex_col()
                        .gap(theme::GAP_SM)
                        .p(theme::PAD_LG)
                        .bg(theme::PANEL())
                        .border_1()
                        .border_color(theme::LINE())
                        .rounded(theme::RADIUS_CARD)
                        .child(mode_line("托管", "PTC 执行器，主 Agent 可指挥 6 个专属 Agent；长任务用 goal/plan/todo"))
                        .child(mode_line("极简", "只读 + 提案工具，不派子 agent；纯粹的人机共创"))
                        .child(mode_line("创造", "官方 Cordis 工具，可为本书造专属 Agent（装进本书的 DSH_HOME）"))
                        .child(
                            div()
                                .mt(theme::PAD_SM)
                                .text_size(theme::FONT_MICRO)
                                .line_height(relative(theme::LINE_HEIGHT_BODY))
                                .text_color(theme::FAINT())
                                .child("模式切换在标题栏右侧。切换会重启本书的 AI 运行时，会话历史保留。"),
                        ),
                )
                .child(
                    div()
                        .text_size(theme::FONT_LABEL)
                        .line_height(relative(theme::LINE_HEIGHT_BODY))
                        .text_color(theme::FAINT())
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
        div()
            .text_size(theme::FONT_BODY)
            .font_weight(FontWeight::MEDIUM)
            .text_color(theme::TEXT())
            .child("设置")
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
        let mut snap_rows: Vec<TableRow> = Vec::new();
        for name in self.names.iter().take(40) {
            let n = name.clone();
            // 快照文件名 = 台账码（等宽）；恢复 = 破坏性红（规则 6/10）。
            snap_rows.push(
                TableRow::new()
                    .child(
                        TableCell::new()
                            .font_family(theme::FAMILY_MONO)
                            .text_color(theme::FAINT())
                            .child(SharedString::from(n.clone())),
                    )
                    .child(TableCell::new().child(
                        danger_btn(format!("restore-{n}"), "恢复", cx).on_click(
                            cx.listener(move |_this, _, _, cx| {
                                cx.emit(SnapEvent::Restore(n.clone()));
                            }),
                        ),
                    )),
            );
        }
        panel_root("snap-scroll").child(
            panel_col()
                .child(
                    div()
                        .flex()
                        .items_center()
                        .gap(theme::GAP_MD)
                        .child(sec_title(SharedString::from(format!("第{}章快照", self.chapter))))
                        .child(
                            div()
                                .text_size(theme::FONT_LABEL)
                                .text_color(theme::FAINT())
                                .child(SharedString::from(format!("{} 份", self.names.len()))),
                        ),
                )
                .child(kit_table(&["快照", "操作"], snap_rows))
                .when_true(self.names.is_empty(), |d| {
                    d.child(empty_hint("还没有快照", "采纳草稿 / 去AI味 / 恢复选区时会自动创建章节快照。"))
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
        div()
            .text_size(theme::FONT_BODY)
            .font_weight(FontWeight::MEDIUM)
            .text_color(theme::TEXT())
            .child("快照")
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
        panel_root("import-scroll").child(
            panel_col()
                .child(sec_title("导入旧稿（全部走提案，采纳才落库）"))
                .child(
                    div()
                        .text_size(theme::FONT_META)
                        .line_height(relative(theme::LINE_HEIGHT_BODY))
                        .text_color(theme::DIM())
                        .child("规则：文件名/内容匹配「第N章」→ 草稿提案；「设定/大纲/人物」目录 → 设定提案；其余忽略。"),
                )
                .child(
                    div()
                        .flex()
                        .flex_col()
                        .gap(theme::GAP_MD)
                        .p(theme::PAD_LG)
                        .bg(theme::PANEL())
                        .border_1()
                        .border_color(theme::LINE())
                        .rounded(theme::RADIUS_CARD)
                        .child(field("目录", &self.path))
                        .child(
                            div().flex().items_center().gap(theme::GAP_MD).child(
                                primary_btn("import-run", "扫描并生成提案", cx).on_click(
                                    cx.listener(|this, _, _, cx| {
                                        let p = this.path.read(cx).value().to_string();
                                        cx.emit(ImportEvent::Run(p));
                                    }),
                                ),
                            ),
                        )
                        // 成功回执 = GREEN chip（规则 6）。
                        .when_true(!result.is_empty(), |d| {
                            d.child(chip(SharedString::from(result.clone()), Tone::Green))
                        }),
                ),
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
        div()
            .text_size(theme::FONT_BODY)
            .font_weight(FontWeight::MEDIUM)
            .text_color(theme::TEXT())
            .child("导入旧稿")
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
        for (gi, l) in lines.iter().enumerate() {
            let lid = l.get("id").and_then(|v| v.as_str()).unwrap_or("?").to_string();
            let name = l.get("name").and_then(|v| v.as_str()).unwrap_or("").to_string();
            // 故事线名 = 叙事性标题（衬线），线号 = 台账码（等宽 FAINT）（规则 10）。
            let mut head = div()
                .flex()
                .items_center()
                .gap(theme::GAP_MD)
                .pt(if gi == 0 { px(0.) } else { theme::GAP_GROUP });
            if gi > 0 {
                head = head.border_t_1().border_color(theme::LINE());
            }
            body.push(
                head.child(
                    div()
                        .text_size(theme::FONT_TAG)
                        .text_color(theme::ACCENT())
                        .child("●"),
                )
                .child(
                    div()
                        .text_size(theme::FONT_TITLE_SM)
                        .font_family(theme::FAMILY_SERIF)
                        .font_weight(FontWeight::SEMIBOLD)
                        .text_color(theme::TEXT())
                        .child(SharedString::from(name)),
                )
                .child(
                    div()
                        .flex_shrink_0()
                        .font_family(theme::FAMILY_MONO)
                        .text_size(theme::FONT_LABEL)
                        .text_color(theme::FAINT())
                        .child(SharedString::from(lid.clone())),
                )
                .into_any_element(),
            );
            let related: Vec<String> = events
                .iter()
                .filter(|e| e.get("storylineIDs").and_then(|s| s.as_array()).map(|a| a.iter().any(|x| x.as_str() == Some(lid.as_str()))).unwrap_or(false))
                .map(|e| format!("第{}章 {}", e.get("chapter").and_then(|c| c.as_u64()).unwrap_or(0), e.get("objectiveFact").and_then(|f| f.as_str()).unwrap_or("").chars().take(30).collect::<String>()))
                .collect();
            if related.is_empty() {
                body.push(
                    div()
                        .pl(theme::PAD_MD)
                        .text_size(theme::FONT_META)
                        .text_color(theme::FAINT())
                        .child("（暂无挂线事件）")
                        .into_any_element(),
                );
            } else {
                for r in related.iter().take(12) {
                    // 「第N章」前缀按台账码等宽处理，事实正文走次级档（规则 8/10）。
                    let (chap, rest) = match r.split_once(' ') {
                        Some((c, o)) => (c.to_string(), o.to_string()),
                        None => (String::new(), r.clone()),
                    };
                    body.push(
                        div()
                            .flex()
                            .items_center()
                            .gap(theme::GAP_MD)
                            .pl(theme::PAD_MD)
                            .py(theme::GAP_XS)
                            .child(
                                div()
                                    .flex_shrink_0()
                                    .font_family(theme::FAMILY_MONO)
                                    .text_size(theme::FONT_LABEL)
                                    .text_color(theme::FAINT())
                                    .child(SharedString::from(chap)),
                            )
                            .child(
                                div()
                                    .flex_1()
                                    .min_w_0()
                                    .overflow_hidden()
                                    .whitespace_nowrap()
                                    .text_size(theme::FONT_META)
                                    .text_color(theme::DIM())
                                    .child(SharedString::from(rest)),
                            )
                            .into_any_element(),
                    );
                }
            }
        }
        if lines.is_empty() {
            body.push(
                empty_hint("还没有故事线 / 事件", "建框架提案采纳后，这里按线展开事件与事实锚点。").into_any_element(),
            );
        }
        body.push(
            div()
                .border_t_1()
                .border_color(theme::LINE())
                .pt(theme::GAP_GROUP)
                .text_size(theme::FONT_LABEL)
                .text_color(theme::FAINT())
                .child(SharedString::from(format!("事实 {} 条 · 事件 {} 个", facts.len(), events.len())))
                .into_any_element(),
        );

        panel_root("graph-scroll").child(panel_col().gap(theme::GAP_MD).children(body))
    }
}

impl BasePanel for GraphPanel {
    fn panel_name(&self) -> &'static str {
        "graph-view"
    }
}
impl ComponentPanel for GraphPanel {
    fn title(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div()
            .text_size(theme::FONT_BODY)
            .font_weight(FontWeight::MEDIUM)
            .text_color(theme::TEXT())
            .child("记忆图谱")
    }
}
