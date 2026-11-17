//! 台账页面板（M5 第一片）：设定 / 大纲·时间线 / 记忆 / 统计 / 伏笔 / 人物卡 / 设置 / 快照 / 导入 / 图谱。
//! 全部只读渲染真实磁盘数据；写操作一律走提案（按钮上抛事件给 Workspace）。
//!
//! 视觉一律取 `crate::theme` 令牌（唯一事实源，勿在此写裸色值）：
//! 页面底 BG、嵌套块 PANEL2+LINE 描边；状态语义一律 chip（FONT_TAG/RADIUS_CHIP/1px 语义描边）；
//! 台账码（线号/事件号/伏笔号/日期/快照文件名）一律 FAMILY_MONO+FONT_LABEL+FAINT；
//! 主按钮朱砂实心+白字、能力按钮朱砂软底+TEXT——经 `ButtonVariants::custom` 覆写库主题（规则 5/6/7）。

use std::cell::Cell as StdCell;
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
    /// 派活给某个专属 Agent。`target` = None 表示模块级一键能力；
    /// Some(desc) 表示针对某个指定对象（desc 是给人看的对象描述）。
    Delegate {
        agent: &'static str,
        key: &'static str,
        target: Option<String>,
    },
    /// 台账新增（种类, 字段）
    LedgerAdd(crate::store::Ledger, serde_json::Value),
    /// 台账改一条（种类, id, 字段补丁）
    LedgerUpdate(crate::store::Ledger, String, serde_json::Value),
    /// 台账删一条（种类, id）
    LedgerRemove(crate::store::Ledger, String),
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
/// 台账「逐条管理」的一行：编号 · 标题 · 一个就地微调按钮 · 删除。
///
/// 为什么不在 `kit_table` 里做：`Cell` 是纯数据（Text/Dim/Code/Chip），
/// 塞不进点击回调。表格负责"看全部"，这一栏负责"改单条"。
fn manage_row<C: EventEmitter<ViewEvent> + 'static>(
    cx: &mut Context<C>,
    ledger: crate::store::Ledger,
    id: &str,
    title: &str,
    meta: String,
    bump: Option<(String, serde_json::Value)>,
) -> gpui_kit::AnyElement {
    let mut line = div()
        .flex()
        .items_center()
        .gap(theme::GAP_SM)
        .py(px(1.))
        .child(
            div()
                .w(px(52.))
                .flex_shrink_0()
                .font_family(theme::FAMILY_MONO)
                .text_size(theme::FONT_MICRO)
                .text_color(theme::ACCENT())
                .child(SharedString::from(id.to_string())),
        )
        .child(
            div()
                .flex_1()
                .min_w_0()
                .text_size(theme::FONT_MICRO)
                .text_color(theme::DIM())
                .text_ellipsis()
                .child(SharedString::from(title.to_string())),
        );

    if let Some((label, patch)) = bump {
        let id_owned = id.to_string();
        line = line.child(
            div()
                .id(SharedString::from(format!("bump-{}-{id}", ledger.file())))
                .flex_shrink_0()
                .cursor_pointer()
                .text_size(theme::FONT_MICRO)
                .text_color(theme::FAINT())
                .child(SharedString::from(label))
                .on_click(cx.listener(move |_this, _, _, cx| {
                    cx.emit(ViewEvent::LedgerUpdate(ledger, id_owned.clone(), patch.clone()));
                })),
        );
    }

    let id_owned = id.to_string();
    line.child(
        div()
            .id(SharedString::from(format!("del-{}-{id}", ledger.file())))
            .flex_shrink_0()
            .cursor_pointer()
            .text_size(theme::FONT_MICRO)
            .text_color(theme::FAINT())
            .child("×")
            .on_click(cx.listener(move |_this, _, _, cx| {
                cx.emit(ViewEvent::LedgerRemove(ledger, id_owned.clone()));
            })),
    )
    .child(
        div()
            .flex_shrink_0()
            .text_size(theme::FONT_MICRO)
            .text_color(theme::FAINT())
            .child(SharedString::from(meta)),
    )
    .into_any_element()
}

/// 只读速览的一行（标签 + 灰字内容）；内容为空时整行不画，不留空标签。
fn summary_row(label: &'static str, text: &str) -> gpui_kit::AnyElement {
    if text.trim().is_empty() {
        return div().into_any_element();
    }
    div()
        .flex()
        .items_start()
        .gap(theme::GAP_SM)
        .child(
            div()
                .w(px(32.))
                .flex_shrink_0()
                .text_size(theme::FONT_MICRO)
                .text_color(theme::FAINT())
                .child(label),
        )
        .child(
            div()
                .flex_1()
                .min_w_0()
                .text_size(theme::FONT_MICRO)
                .line_height(relative(theme::LINE_HEIGHT_BODY))
                .text_color(theme::DIM())
                .overflow_hidden()
                .line_clamp(2)
                .child(SharedString::from(text.to_string())),
        )
        .into_any_element()
}

/// 一张人物卡的"信息完整度"：5 个关键字段填了几个（牌脚那排小点）。
fn filled_of(c: &crate::characters::Character) -> usize {
    [
        !c.appearance.trim().is_empty(),
        !c.personality.trim().is_empty(),
        !c.motivation.trim().is_empty(),
        !c.voice.trim().is_empty(),
        !c.core_story.trim().is_empty(),
    ]
    .iter()
    .filter(|b| **b)
    .count()
}

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

    pub fn show_document(&mut self, title: &str, cx: &mut Context<Self>) {
        if self.handle.canon_docs().iter().any(|d| d.0 == title) {
            self.selected = Some(title.to_string());
            cx.notify();
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
    search_event: Option<String>,
}

impl OutlinePanel {
    pub fn new(cx: &mut Context<Self>, handle: Rc<ProjectHandle>) -> Self {
        Self { focus_handle: cx.focus_handle(), handle, search_event: None }
    }

    pub fn show_search_event(&mut self, id: &str, cx: &mut Context<Self>) {
        self.search_event = Some(id.to_string());
        cx.notify();
    }

    pub fn clear_search_event(&mut self, cx: &mut Context<Self>) {
        if self.search_event.take().is_some() { cx.notify(); }
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
        let all_events = self.handle.events();
        let events: Vec<_> = if let Some(id) = &self.search_event {
            all_events.into_iter().filter(|e| e.get("id").and_then(|v| v.as_str()) == Some(id)).collect()
        } else {
            all_events
        };
        let stages = self.handle.stages();

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
        for e in events.iter() {
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
                        // 单行预览截断（不是条数截断）：表格一行放不下整段
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
                                // 作者亲笔新建：不经过 AI，直接落台账
                                .child(cap_btn("outline-add-event", "新建事件", cx).on_click(
                                    cx.listener(|_this, _, _, cx| {
                                        cx.emit(ViewEvent::LedgerAdd(
                                            crate::store::Ledger::Events,
                                            serde_json::json!({
                                                "chapter": 0,
                                                "objectiveFact": "新事件（待写）",
                                                "revealed": false,
                                                "happened": false,
                                                "storylineIDs": []
                                            }),
                                        ));
                                    }),
                                ))
                                .child(cap_btn("outline-add-line", "新建故事线", cx).on_click(
                                    cx.listener(|_this, _, _, cx| {
                                        cx.emit(ViewEvent::LedgerAdd(
                                            crate::store::Ledger::Storylines,
                                            serde_json::json!({
                                                "name": "新故事线",
                                                "kind": "",
                                                "isThroughLine": false
                                            }),
                                        ));
                                    }),
                                ))
                                // 一键能力由能力表驱动：大纲对账 / 伏笔盘点 / 设定归类
                                // 各归它的 Agent，页面不写死"这是谁的活"。
                                .children({
                                    let mut bar = div().flex().flex_wrap().gap(theme::GAP_SM);
                                    for aid in ["outline", "reviewer"] {
                                        let Some(spec) = crate::capabilities::agent(aid) else { continue };
                                        for cap in spec.bulk {
                                            bar = bar.child(
                                                cap_btn(cap.key, cap.label, cx).on_click(
                                                    cx.listener(move |_this, _, _, cx| {
                                                        cx.emit(ViewEvent::Delegate {
                                                            agent: aid,
                                                            key: cap.key,
                                                            target: None,
                                                        });
                                                    }),
                                                ),
                                            );
                                        }
                                    }
                                    vec![bar.into_any_element()]
                                }),
                        ),
                )
                .child(kit_table(&["线", "名称", "状态"], line_rows))
                .when_true(lines.is_empty(), |d| {
                    d.child(empty_hint("还没有故事线", "新书建框架提案采纳后，这里出现线号 · 名称 · 状态。"))
                })
                .when_true(!lines.is_empty(), |d| {
                    let mut rows = div().flex().flex_col().gap(px(1.));
                    for l in &lines {
                        let id = l.get("id").and_then(|v| v.as_str()).unwrap_or("?").to_string();
                        let name = l.get("name").and_then(|v| v.as_str()).unwrap_or("").to_string();
                        // 故事线没有"章节"可推，删掉仍要能走；meta 显示是否主线
                        let meta = if l.get("isThroughLine").and_then(|v| v.as_bool()).unwrap_or(false) {
                            "主线".to_string()
                        } else {
                            String::new()
                        };
                        rows = rows.child(manage_row(
                            cx,
                            crate::store::Ledger::Storylines,
                            &id,
                            &name,
                            meta,
                            None,
                        ));
                    }
                    d.child(sec_title("逐条管理"))
                        .child(
                            div()
                                .text_size(theme::FONT_MICRO)
                                .text_color(theme::FAINT())
                                .child("「×」删除故事线（可撤销）。名称与状态在事件列表里按线号对照改。"),
                        )
                        .child(rows)
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
                        .when_true(self.search_event.is_some(), |d| d.child(
                            cap_btn("outline-clear-search", "显示全部事件", cx).on_click(
                                cx.listener(|this, _, _, cx| {
                                    this.search_event = None;
                                    cx.notify();
                                }),
                            ),
                        ))
                        .child(kit_table(&["事件", "章", "作者真相", "读者已知"], event_rows))
                        .when_true(events.is_empty(), |d| {
                            d.child(empty_hint("还没有事件", "用「AI 建时间线」提案，从正文抽取作者真相事件。"))
                        })
                        .when_true(!events.is_empty(), |d| {
                            let total = events.len();
                            // 事件多，只列最近 12 条做就地管理；全量仍在上面表格里。
                            // 这是"便捷入口"的取舍，不是数据截断——表格不截。
                            let mut rows = div().flex().flex_col().gap(px(1.));
                            for e in events.iter().rev().take(12).rev() {
                                let id = e.get("id").and_then(|v| v.as_str()).unwrap_or("?").to_string();
                                let fact = e.get("objectiveFact").and_then(|v| v.as_str()).unwrap_or("");
                                let chapter = e.get("chapter").and_then(|c| c.as_u64()).unwrap_or(0);
                                let revealed = e.get("revealed").and_then(|v| v.as_bool()).unwrap_or(false);
                                let meta = if revealed { "读者已知".to_string() } else { "暗线".to_string() };
                                rows = rows.child(manage_row(
                                    cx,
                                    crate::store::Ledger::Events,
                                    &id,
                                    fact,
                                    meta,
                                    // 就地推章：把事件往后挪一章
                                    Some((
                                        format!("第{chapter}章 →"),
                                        serde_json::json!({ "chapter": chapter + 1 }),
                                    )),
                                ));
                            }
                            d.child(sec_title("逐条管理"))
                                .child(
                                    div()
                                        .text_size(theme::FONT_MICRO)
                                        .text_color(theme::FAINT())
                                        .child(format!(
                                            "最近 12 条（共 {total} 条）：「第N章 →」把事件往后挪一章；「×」删除（可撤销）。"
                                        )),
                                )
                                .child(rows)
                        })
                        .child(
                            div()
                                .flex()
                                .flex_col()
                                .gap(theme::GAP_MD)
                                .border_t_1()
                                .border_color(theme::LINE())
                                .pt(theme::GAP_GROUP)
                                .child(
                                    div()
                                        .flex()
                                        .items_center()
                                        .justify_between()
                                        .gap(theme::GAP_MD)
                                        .child(sec_title("幕（卷）"))
                                        .child(cap_btn("outline-add-stage", "新建幕", cx).on_click(
                                            cx.listener(|_this, _, _, cx| {
                                                // 新幕从第 1 章起、到第 1 章止；作者再改区间
                                                cx.emit(ViewEvent::LedgerAdd(
                                                    crate::store::Ledger::Stages,
                                                    serde_json::json!({
                                                        "name": "新幕",
                                                        "theme": "",
                                                        "chapterStart": 1,
                                                        "chapterEnd": 1
                                                    }),
                                                ));
                                            }),
                                        )),
                                )
                                .child({
                                    // 幕是少数几个有天然顺序的台账，按起章排，读起来才对。
                                    let mut ordered: Vec<&serde_json::Value> = stages.iter().collect();
                                    ordered.sort_by_key(|v| {
                                        v.get("chapterStart").and_then(|c| c.as_u64()).unwrap_or(0)
                                    });
                                    let rows: Vec<TableRow> = ordered
                                        .iter()
                                        .map(|st| {
                                            let a = st
                                                .get("chapterStart")
                                                .and_then(|v| v.as_u64())
                                                .unwrap_or(0);
                                            let b = st
                                                .get("chapterEnd")
                                                .and_then(|v| v.as_u64())
                                                .unwrap_or(0);
                                            row(vec![
                                                Cell::Code(
                                                    st.get("id")
                                                        .and_then(|v| v.as_str())
                                                        .unwrap_or("?")
                                                        .to_string(),
                                                ),
                                                Cell::Text(
                                                    st.get("name")
                                                        .and_then(|v| v.as_str())
                                                        .unwrap_or("")
                                                        .to_string(),
                                                ),
                                                Cell::Dim(format!("第{a}–{b}章")),
                                                Cell::Dim(
                                                    st.get("theme")
                                                        .and_then(|v| v.as_str())
                                                        .unwrap_or("")
                                                        .to_string(),
                                                ),
                                            ])
                                        })
                                        .collect();
                                    kit_table(&["幕", "卷名", "章节区间", "主题"], rows)
                                })
                                .when_true(stages.is_empty(), |d| {
                                    d.child(empty_hint(
                                        "还没有幕",
                                        "幕是卷的骨架：排期板的幕带、世界线的时间刻度都读它。",
                                    ))
                                })
                                .when_true(!stages.is_empty(), |d| {
                                    let mut rows = div().flex().flex_col().gap(px(1.));
                                    for st in stages.iter() {
                                        let id = st
                                            .get("id")
                                            .and_then(|v| v.as_str())
                                            .unwrap_or("?")
                                            .to_string();
                                        let name =
                                            st.get("name").and_then(|v| v.as_str()).unwrap_or("");
                                        let a = st
                                            .get("chapterStart")
                                            .and_then(|v| v.as_u64())
                                            .unwrap_or(0);
                                        let b =
                                            st.get("chapterEnd").and_then(|v| v.as_u64()).unwrap_or(0);
                                        rows = rows.child(manage_row(
                                            cx,
                                            crate::store::Ledger::Stages,
                                            &id,
                                            name,
                                            format!("{a}–{b}"),
                                            // 幕的粒度是卷，一次推一章没意义；推终点更常用
                                            Some((
                                                format!("止于{b} →"),
                                                serde_json::json!({ "chapterEnd": b + 1 }),
                                            )),
                                        ));
                                    }
                                    d.child(sec_title("逐条管理"))
                                        .child(
                                            div()
                                                .text_size(theme::FONT_MICRO)
                                                .text_color(theme::FAINT())
                                                .child("「止于N →」把幕的终点推后一章；「×」删除（可撤销）。"),
                                        )
                                        .child(rows)
                                }),
                        ),
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
    recall_terms: Entity<gpui_kit::component::input::InputState>,
    recall_chapter: Entity<gpui_kit::component::input::InputState>,
}

impl MemoryPanel {
    pub fn new(window: &mut Window, cx: &mut Context<Self>, handle: Rc<ProjectHandle>) -> Self {
        let recall_terms = cx.new(|cx| {
            gpui_kit::component::input::InputState::new(window, cx)
                .placeholder("人物 / 别名 / 关键词，空格分隔")
        });
        let recall_chapter = cx.new(|cx| {
            gpui_kit::component::input::InputState::new(window, cx).placeholder("章号（可空）")
        });
        // 召回是渲染期算的：输入一变就必须重渲染，否则敲完字表格还是上一批。
        // 与 editor.rs 的发话框同一做法（订阅 InputEvent::Change）。
        cx.subscribe(&recall_terms, |_, _, _: &gpui_kit::component::input::InputEvent, cx| {
            cx.notify()
        })
        .detach();
        cx.subscribe(&recall_chapter, |_, _, _: &gpui_kit::component::input::InputEvent, cx| {
            cx.notify()
        })
        .detach();
        Self { focus_handle: cx.focus_handle(), handle, recall_terms, recall_chapter }
    }
}

impl Focusable for MemoryPanel {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus_handle.clone()
    }
}

impl Render for MemoryPanel {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        // 一次读盘拿事实 + 别名（渲染期别读两遍，见 memory_ledger 的注释）
        let (facts, aliases) = self.handle.memory_ledger();

        // 召回：按相关度+章距+新近度排序去重，空查询=按新近度看最新一批。
        let terms: Vec<String> = self
            .recall_terms
            .read(cx)
            .value()
            .split_whitespace()
            .map(|s| s.to_string())
            .collect();
        let chapter = self.recall_chapter.read(cx).value().trim().parse::<u32>().ok();
        let hits = crate::memory::recall(
            &facts,
            &aliases,
            &crate::memory::RecallQuery { chapter, terms, limit: 12 },
        );
        let mut recall_rows = Vec::new();
        for h in &hits {
            recall_rows.push(row(vec![
                Cell::Hi(h.fact.subject.clone()),
                Cell::Text(h.fact.predicate.clone()),
                Cell::Text(h.fact.object.chars().take(40).collect()),
                Cell::Dim(format!("第{}章", h.fact.chapter)),
                Cell::Chip(format!("{}", h.score), Tone::Blue),
                Cell::Dim(if h.reasons.is_empty() { "新近".into() } else { h.reasons.join("/") }),
            ]));
        }

        let mut rows = Vec::new();
        for f in facts.iter() {
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
                .child(sec_title("召回（按需，不自动挂载）"))
                .child(
                    div()
                        .flex()
                        .items_center()
                        .gap(theme::GAP_MD)
                        .child(div().flex_1().min_w_0().child(
                            gpui_kit::component::input::Input::new(&self.recall_terms)
                                .bg(theme::PANEL2())
                                .border_1()
                                .border_color(theme::LINE())
                                .rounded(theme::RADIUS_BTN)
                                .text_size(theme::FONT_BODY)
                                .text_color(theme::TEXT()),
                        ))
                        .child(div().w(px(96.)).flex_shrink_0().child(
                            gpui_kit::component::input::Input::new(&self.recall_chapter)
                                .bg(theme::PANEL2())
                                .border_1()
                                .border_color(theme::LINE())
                                .rounded(theme::RADIUS_BTN)
                                .text_size(theme::FONT_BODY)
                                .text_color(theme::TEXT()),
                        )),
                )
                .child(kit_table(&["主语", "谓语", "宾语", "生效", "分", "理由"], recall_rows))
                .when_true(hits.is_empty(), |d| {
                    d.child(empty_hint("没有可召回的事实", "写完一章点「让AI记一笔」，或换个关键词 / 章号。"))
                })
                .child(
                    div()
                        .flex()
                        .items_center()
                        .justify_between()
                        .gap(theme::GAP_MD)
                        .pt(theme::GAP_GROUP)
                        .border_t_1()
                        .border_color(theme::LINE())
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
    search_target: Option<String>,
}

impl CluesPanel {
    pub fn new(cx: &mut Context<Self>, handle: Rc<ProjectHandle>) -> Self {
        Self { focus_handle: cx.focus_handle(), handle, search_target: None }
    }

    pub fn show_search_result(&mut self, id: &str, cx: &mut Context<Self>) {
        self.search_target = Some(id.to_string());
        cx.notify();
    }

    pub fn clear_search_result(&mut self, cx: &mut Context<Self>) {
        if self.search_target.take().is_some() { cx.notify(); }
    }
}

impl Focusable for CluesPanel {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus_handle.clone()
    }
}

impl Render for CluesPanel {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let all_clues = self.handle.clues_full();
        let clues: Vec<_> = if let Some(id) = &self.search_target {
            all_clues.into_iter().filter(|c| c.get("id").and_then(|v| v.as_str()) == Some(id)).collect()
        } else {
            all_clues
        };
        let mut rows = Vec::new();
        for c in clues.iter() {
            let status = c.get("status").and_then(|v| v.as_str()).unwrap_or("").to_string();
            let tone = status_tone(&status);
            let timing = c.get("timing").and_then(|v| v.as_str()).unwrap_or("").to_string();
            let mut cells = vec![
                Cell::CodeAccent(c.get("id").and_then(|v| v.as_str()).unwrap_or("?").to_string()),
                // 标题列单行预览截断，不隐藏条目
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
                        .child(
                            div()
                                .flex()
                                .gap(theme::GAP_SM)
                                .when_true(self.search_target.is_some(), |d| d.child(
                                    cap_btn("clues-clear-search", "显示全部", cx).on_click(
                                        cx.listener(|this, _, _, cx| {
                                            this.search_target = None;
                                            cx.notify();
                                        }),
                                    ),
                                ))
                                // 作者亲笔新建：不经过 AI，直接落台账
                                .child(cap_btn("clues-add", "新建伏笔", cx).on_click(
                                    cx.listener(|_this, _, _, cx| {
                                        cx.emit(ViewEvent::LedgerAdd(
                                            crate::store::Ledger::Clues,
                                            serde_json::json!({
                                                "title": "新伏笔",
                                                "detail": "",
                                                "status": "待埋",
                                                "plantedChapter": 0,
                                                "lastActionChapter": 0
                                            }),
                                        ));
                                    }),
                                ))
                                // 伏笔盘点属于大纲伏笔 Agent 的一键能力
                                .children({
                                    let mut bar = div().flex().gap(theme::GAP_SM);
                                    for aid in ["outline"] {
                                        let Some(spec) = crate::capabilities::agent(aid) else { continue };
                                        for cap in spec.bulk {
                                            if !cap.key.contains("clues") {
                                                continue;
                                            }
                                            bar = bar.child(
                                                cap_btn(cap.key, cap.label, cx).on_click(
                                                    cx.listener(move |_this, _, _, cx| {
                                                        cx.emit(ViewEvent::Delegate {
                                                            agent: aid,
                                                            key: cap.key,
                                                            target: None,
                                                        });
                                                    }),
                                                ),
                                            );
                                        }
                                    }
                                    vec![bar.into_any_element()]
                                }),
                        ),
                )
                .child(kit_table(&["编号", "标题 / 详情", "节奏", "埋于", "最近动作", "状态"], rows))
                .when_true(clues.is_empty(), |d| {
                    d.child(empty_hint("台账为空", "「AI 盘点伏笔」提案可从正文重建埋点、推进与节奏。"))
                })
                // 逐条管理：表格里的行是只读展示，删除/改章放在这里，
                // 避免在表格单元格里塞交互控件（kit 的 Table 不支持）。
                .when_true(!clues.is_empty(), |d| {
                    let mut lines = div().flex().flex_col().gap(px(1.));
                    for c in clues.iter() {
                        let id = c.get("id").and_then(|v| v.as_str()).unwrap_or("?").to_string();
                        let title = c.get("title").and_then(|v| v.as_str()).unwrap_or("").to_string();
                        let planted = c.get("plantedChapter").and_then(|v| v.as_u64()).unwrap_or(0);
                        let resolved = c.get("resolvedChapter").and_then(|v| v.as_u64()).unwrap_or(0);
                        let meta = if resolved > 0 { format!("收于 {resolved}") } else { String::new() };
                        lines = lines.child(manage_row(
                            cx,
                            crate::store::Ledger::Clues,
                            &id,
                            &title,
                            meta,
                            // 就地微调：埋设章 +1（改章节是最常做的动作）
                            Some((
                                format!("埋{planted} →"),
                                serde_json::json!({ "plantedChapter": planted + 1 }),
                            )),
                        ));
                    }
                    d.child(sec_title("逐条管理"))
                        .child(
                            div()
                                .text_size(theme::FONT_MICRO)
                                .text_color(theme::FAINT())
                                .child("点「埋N →」把埋设章推后一章；「×」删除（可撤销）。"),
                        )
                        .child(lines)
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

/* ───────────────────────── 人物档案（左列表 · 右档案 · 可编辑） ───────────────────────── */

/// 人物页的编辑事件。所有写操作都走宿主，页面本身不碰磁盘。
#[derive(Debug, Clone)]
pub enum CharacterEvent {
    /// 新建人物（名字）
    Create(String),
    /// 保存当前档案
    Save(Box<crate::characters::Character>),
    /// 删除某人
    Delete(String),
    /// 把 from 合并进 into
    Merge { into: String, from: String },
    /// 新建别名（追加到当前人物）
    AddAlias(String),
    /// 移除别名
    RemoveAlias(String),
    /// 从 memory.json 迁移
    Migrate,
    /// 锁定/解锁某字段
    ToggleLock(String),
    /// 派活给人物 Agent：一键能力（target=None）或针对某个人物（target=Some(描述)）
    Delegate {
        key: &'static str,
        target: Option<String>,
    },
}

pub struct CharactersPanel {
    focus_handle: FocusHandle,
    handle: Rc<ProjectHandle>,
    /// 列表
    people: Vec<crate::characters::Character>,
    /// 卡簿里"亮着"的那张卡。
    ///
    /// 与 `open` 分开：选中是**看**（卡面高亮、右侧小传预览），打开是**改**
    /// （右侧弹出完整人设卡）。三国杀里点牌是先拿起、再翻开——两步分开，
    /// 作者逛卡簿时不会一不小心把每张卡都推进编辑态。
    selected: Option<String>,
    /// 完整人设卡（右侧抽屉）打开的是谁；None = 抽屉收起。
    open: Option<String>,
    /// 档案字段编辑框（与 selected 同步）
    ed_name: Entity<gpui_kit::component::input::InputState>,
    ed_role: Entity<gpui_kit::component::input::InputState>,
    ed_faction: Entity<gpui_kit::component::input::InputState>,
    ed_appearance: Entity<gpui_kit::component::input::InputState>,
    ed_personality: Entity<gpui_kit::component::input::InputState>,
    ed_motivation: Entity<gpui_kit::component::input::InputState>,
    ed_background: Entity<gpui_kit::component::input::InputState>,
    ed_voice: Entity<gpui_kit::component::input::InputState>,
    ed_status: Entity<gpui_kit::component::input::InputState>,
    ed_age: Entity<gpui_kit::component::input::InputState>,
    /// 状态从第几章起（手填；派生值也能写，但作者常要手调）
    ed_status_ch: Entity<gpui_kit::component::input::InputState>,
    /// 卡背：核心剧情
    ed_core_story: Entity<gpui_kit::component::input::InputState>,
    /// 当前卡片是正面（档案）还是背面（核心剧情）
    flipped: bool,
    /// 新建 / 加别名的临时输入
    ed_new: Entity<gpui_kit::component::input::InputState>,
    /// 提示（迁移结果、错误）
    message: Option<String>,
    /// 卡簿容器的实测宽度（prepaint 写回，渲染时按它算列数）。
    /// 用 Rc<Cell> 而不是存字段：prepaint 闭包拿不到 &mut self。
    grid_w: Rc<StdCell<f32>>,
}

impl CharactersPanel {
    pub fn new(window: &mut Window, cx: &mut Context<Self>, handle: Rc<ProjectHandle>) -> Self {
        // 每个输入框都新建一个 InputState；window 借用不能跨闭包捕获，所以逐个写
        fn mk(
            window: &mut Window,
            cx: &mut Context<CharactersPanel>,
            placeholder: &str,
        ) -> Entity<gpui_kit::component::input::InputState> {
            cx.new(|c| {
                gpui_kit::component::input::InputState::new(window, c)
                    .placeholder(placeholder.to_string())
            })
        }
        let mut panel = Self {
            focus_handle: cx.focus_handle(),
            handle,
            people: Vec::new(),
            selected: None,
            open: None,
            ed_name: mk(window, cx, "本名"),
            ed_role: mk(window, cx, "角色定位：主角/反派/导师…"),
            ed_faction: mk(window, cx, "阵营 / 所属"),
            ed_appearance: mk(window, cx, "外貌"),
            ed_personality: mk(window, cx, "性格"),
            ed_motivation: mk(window, cx, "动机：他/她想要什么"),
            ed_background: mk(window, cx, "背景"),
            ed_voice: mk(window, cx, "口吻：说话方式、用词、口头禅"),
            ed_status: mk(window, cx, "当前状态：活着/受伤/失忆…"),
            ed_age: mk(window, cx, "年龄 / 年纪"),
            ed_status_ch: mk(window, cx, "第几章"),
            ed_core_story: mk(window, cx, "这个角色在故事里承担了什么"),
            flipped: false,
            ed_new: mk(window, cx, "名字"),
            message: None,
            grid_w: Rc::new(StdCell::new(0.)),
        };
        panel.reload(window, cx);
        // 开发预览：ZHIBI_CHAR_OPEN=1 时直接翻开第一张卡——
        // 截图核验"点开后的编修面"用，省掉模拟点击（合成鼠标事件在
        // 非活跃窗口里经常丢，靠它截图验证不靠谱）。
        if std::env::var("ZHIBI_CHAR_OPEN").is_ok() {
            if let Some(first) = panel.people.first().map(|c| c.id.clone()) {
                panel.open_card(first, window, cx);
            }
        }
        panel
    }

    /// 从磁盘重读，并把选中项回填到编辑框。
    pub fn reload(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.people = crate::characters::Roster::new(&self.handle.root).all();
        // 抽屉里那个人被删了/被并了：收起抽屉，别留一张空卡。
        if let Some(id) = &self.open {
            if !self.people.iter().any(|c| &c.id == id) {
                self.open = None;
            }
        }
        if self.selected.is_none() && !self.people.is_empty() {
            self.selected = Some(self.people[0].id.clone());
        }
        // 编辑框只跟抽屉走：抽屉收起时不用回填（没有可见的卡要填）。
        if self.open.is_some() {
            self.fill_editors(window, cx);
        }
        cx.notify();
    }

    fn current(&self) -> Option<&crate::characters::Character> {
        let id = self.selected.as_ref()?;
        self.people.iter().find(|c| &c.id == id)
    }

    /// 把选中人物的字段灌进编辑框。切人时调用，避免把上一个人的值带过去。
    fn fill_editors(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let c = self.current().cloned().unwrap_or_default();
        let status_ch = if c.status_chapter == 0 {
            String::new()
        } else {
            c.status_chapter.to_string()
        };
        let pairs: [(Entity<gpui_kit::component::input::InputState>, String); 12] = [
            (self.ed_name.clone(), c.name),
            (self.ed_role.clone(), c.role),
            (self.ed_faction.clone(), c.faction),
            (self.ed_appearance.clone(), c.appearance),
            (self.ed_personality.clone(), c.personality),
            (self.ed_motivation.clone(), c.motivation),
            (self.ed_background.clone(), c.background),
            (self.ed_voice.clone(), c.voice),
            (self.ed_status.clone(), c.status),
            (self.ed_age.clone(), c.age),
            (self.ed_status_ch.clone(), status_ch),
            (self.ed_core_story.clone(), c.core_story),
        ];
        for (entity, value) in pairs {
            entity.update(cx, |state, cx| state.set_value(value.as_str(), window, cx));
        }
    }

    /// 用编辑框的内容拼出一份完整档案（保留下拉/弧线/锁定等未在表单上的字段）。
    pub fn draft(&self, cx: &App) -> Option<crate::characters::Character> {
        let mut c = self.current()?.clone();
        c.name = self.ed_name.read(cx).value().to_string();
        c.role = self.ed_role.read(cx).value().to_string();
        c.faction = self.ed_faction.read(cx).value().to_string();
        c.appearance = self.ed_appearance.read(cx).value().to_string();
        c.personality = self.ed_personality.read(cx).value().to_string();
        c.motivation = self.ed_motivation.read(cx).value().to_string();
        c.background = self.ed_background.read(cx).value().to_string();
        c.voice = self.ed_voice.read(cx).value().to_string();
        c.status = self.ed_status.read(cx).value().to_string();
        c.age = self.ed_age.read(cx).value().to_string();
        c.status_chapter = self
            .ed_status_ch
            .read(cx)
            .value()
            .trim()
            .parse()
            .unwrap_or(0);
        c.core_story = self.ed_core_story.read(cx).value().to_string();
        Some(c)
    }

    pub fn set_message(&mut self, text: String, window: &mut Window, cx: &mut Context<Self>) {
        self.message = Some(text);
        self.reload(window, cx);
    }

    /// 选中某人（卡簿里亮起来；抽屉不动）。
    ///
    /// 外部跳转一律用 `open_card`；这个只留给"从别处回来还想让卡簿停在那个人身上"
    /// 的场景（比如 AI 提案里点到某人时先高亮、不抢占视线）。
    #[allow(dead_code)]
    pub fn select(&mut self, id: String, window: &mut Window, cx: &mut Context<Self>) {
        self.selected = Some(id);
        self.fill_editors(window, cx);
        cx.notify();
    }

    /// 翻开某人的人设卡：右侧抽屉弹出完整档案，字段回填可改。
    ///
    /// 外部跳进来（关系图点人物、AI 提案指向某人）时用这个而不是 `select`——
    /// 作者点「看这个人」就是要看人，不是要在一排卡里找亮点。
    pub fn open_card(&mut self, id: String, window: &mut Window, cx: &mut Context<Self>) {
        let id = self.people.iter().find(|p| p.id == id || p.name == id).map(|p| p.id.clone());
        let Some(id) = id else { return };
        self.selected = Some(id.clone());
        self.open = Some(id);
        self.fill_editors(window, cx);
        cx.notify();
    }

    fn close_card(&mut self, cx: &mut Context<Self>) {
        self.open = None;
        cx.notify();
    }
}

impl Focusable for CharactersPanel {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus_handle.clone()
    }
}

impl EventEmitter<PanelEvent> for CharactersPanel {}
impl EventEmitter<CharacterEvent> for CharactersPanel {}

impl Render for CharactersPanel {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let message = self.message.clone();
        let selected = self.selected.clone();

        // —— 卡簿：三国杀那种整张的大卡，不是头像 ——
        //
        // 卡面本身就是完整信息：姓名题签、基本信息、背景故事、相关剧情。
        // 一屏看两三张卡，但每张卡都是"这一个角色"的全貌——作者翻卡簿是在
        // **认人**，不是在一堆头像里找亮点。要看细节、要改，才点开右侧的编辑面。
        //
        // 卡面按 5:7（近似 63×88 的实体牌比例）固定成 200×280：
        // 太小塞不下背景故事与相关剧情，太大一屏连一张都放不开。
        // 列数按容器宽度算，卡尺寸不变——窗口窄了就少排一列，不是把卡压小。
        let width_cell = self.grid_w.clone();
        let card_w = 240.0f32;
        let gap = 12.0f32;
        let columns = {
            let w = width_cell.get() as f32;
            let c = if w <= 0. {
                3
            } else {
                ((w + gap) / (card_w + gap)).floor().clamp(1., 5.) as usize
            };
            c
        };

        let mut ordered: Vec<&crate::characters::Character> =
            self.people.iter().filter(|c| !c.minor).collect();
        let minors: Vec<&crate::characters::Character> =
            self.people.iter().filter(|c| c.minor).collect();
        let minor_count = minors.len();
        let main_count = ordered.len();
        ordered.extend(minors);

        // 卡面要用的两路关联数据（都是已有台账，不必新开数据通道）：
        // 1. 背景故事：canon/人物·X.md（作者写的设定文档）
        // 2. 相关剧情：events.json 里 objectiveFact 提到本名/别名的事件，按章号排
        // 名字与事件都是磁盘上确定的内容，不靠 AI 猜；查不到就留白并写明"还没记"。
        let canon_docs = self.handle.canon_docs();
        let events = self.handle.events();
        let card_data = |c: &crate::characters::Character| -> (String, Vec<(u32, String)>) {
            let names: Vec<&str> = std::iter::once(c.name.as_str())
                .chain(c.aliases.iter().map(|a| a.as_str()))
                .filter(|n| !n.trim().is_empty())
                .collect();
            // 背景：找标题里带本名的人物设定文档
            let mut bg = String::new();
            for (title, _cat, body) in &canon_docs {
                if names.iter().any(|n| title.contains(n)) {
                    bg = body
                        .lines()
                        .filter(|l| !l.trim_start().starts_with('#'))
                        .filter(|l| !l.trim().is_empty())
                        .collect::<Vec<_>>()
                        .join("\n")
                        .trim()
                        .to_string();
                    if !bg.is_empty() {
                        break;
                    }
                }
            }
            if bg.is_empty() {
                bg = c.background.trim().to_string();
            }
            // 剧情：事件事实里提到这个名字就算他的戏
            let mut plot: Vec<(u32, String)> = events
                .iter()
                .filter_map(|e| {
                    let fact = e.get("objectiveFact")?.as_str()?.to_string();
                    if !names.iter().any(|n| fact.contains(n)) {
                        return None;
                    }
                    let ch = e
                        .get("chapter")
                        .and_then(|v| v.as_u64())
                        .or_else(|| e.get("actualChapter").and_then(|v| v.as_u64()))
                        .unwrap_or(0) as u32;
                    Some((ch, fact))
                })
                .collect();
            plot.sort_by_key(|(ch, _)| *ch);
            (bg, plot)
        };

        // 卡面（纯展示，点击由外层壳挂上；Div 能带状态但 AnyElement 不行）
        let face = |c: &crate::characters::Character,
                         on: bool,
                         data: &(String, Vec<(u32, String)>)| {
            let (bg, plot) = data;
            let accent = if c.minor { theme::LINE_SOFT() } else { theme::ACCENT() };
            let seal: String = c.name.chars().next().map(|x| x.to_string()).unwrap_or_default();

            // 「基本信息」：定位/阵营/年龄/状态，能写多少写多少，空的不占行
            let mut info: Vec<(&str, String)> = Vec::new();
            if !c.role.trim().is_empty() {
                info.push(("定位", c.role.trim().to_string()));
            }
            if !c.faction.trim().is_empty() {
                info.push(("阵营", c.faction.trim().to_string()));
            }
            if !c.age.trim().is_empty() {
                info.push(("年纪", c.age.trim().to_string()));
            }
            if !c.status.trim().is_empty() {
                info.push((
                    "状态",
                    if c.status_chapter > 0 {
                        format!("{}（第{}章起）", c.status.trim(), c.status_chapter)
                    } else {
                        c.status.trim().to_string()
                    },
                ));
            }
            if !c.aliases.is_empty() {
                info.push(("别名", c.aliases.join("、")));
            }
            if info.is_empty() {
                info.push(("档案", "还没写定位/阵营——点开补齐".into()));
            }

            // 小字段块：标题 + 正文（正文缺省时给一句引导，不留空白）
            let block = |label: &'static str, body: String, hint: &'static str, lines: usize| {
                let empty = body.trim().is_empty();
                div()
                    .w_full()
                    .flex()
                    .flex_col()
                    .gap(px(2.))
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap(px(4.))
                            .child(
                                div()
                                    .w(px(2.))
                                    .h(px(9.))
                                    .rounded(px(1.))
                                    .bg(accent),
                            )
                            .child(
                                div()
                                    .text_size(px(9.5))
                                    .font_weight(FontWeight::MEDIUM)
                                    .text_color(theme::FAINT())
                                    .child(SharedString::from(label)),
                            ),
                    )
                    .child(
                        div()
                            .text_size(px(10.5))
                            .line_height(relative(1.5))
                            .text_color(if empty { theme::GHOST() } else { theme::DIM() })
                            .overflow_hidden()
                            .line_clamp(lines)
                            .child(SharedString::from(if empty {
                                hint.to_string()
                            } else {
                                body
                            })),
                    )
            };

            // 卡是固定尺寸的实体牌（5:7，近似 63×88mm 的牌面比例），
            // 不随窗口拉伸——拉伸会让"牌"变成"面板"，背景故事和剧情的
            // 行宽也跟着变，读起来就不再像一张卡了。
            let card = div()
                .w(px(240.))
                .h(px(336.))
                .flex_shrink_0()
                .flex()
                .flex_col()
                .rounded(px(10.))
                .overflow_hidden()
                .cursor_pointer()
                .border_1()
                .border_color(if on {
                    theme::ACCENT_BORDER()
                } else if c.minor {
                    theme::LINE_SOFT()
                } else {
                    theme::LINE()
                })
                .bg(if on { theme::ACCENT_SOFT() } else { theme::PANEL() })
                .hover(|d| d.border_color(theme::ACCENT_BORDER()).bg(theme::PANEL2()))
                // 牌头：姓名题签（篆意图省 + 定位色条）
                .child(
                    div()
                        .w_full()
                        .flex_shrink_0()
                        .px(px(10.))
                        .py(px(7.))
                        .flex()
                        .items_center()
                        .gap(px(7.))
                        .border_b_1()
                        .border_color(theme::LINE_SOFT())
                        .child(
                            div()
                                .w(px(26.))
                                .h(px(26.))
                                .flex_shrink_0()
                                .rounded(px(13.))
                                .border_1()
                                .border_color(accent)
                                .flex()
                                .items_center()
                                .justify_center()
                                .child(
                                    div()
                                        .text_size(px(13.))
                                        .font_family(theme::FAMILY_SERIF)
                                        .text_color(if c.minor { theme::FAINT() } else { accent })
                                        .child(SharedString::from(seal)),
                                ),
                        )
                        .child(
                            div()
                                .flex_1()
                                .min_w_0()
                                .flex()
                                .flex_col()
                                .child(
                                    div()
                                        .text_size(px(15.))
                                        .font_family(theme::FAMILY_SERIF)
                                        .font_weight(FontWeight::SEMIBOLD)
                                        .text_ellipsis()
                                        .text_color(if on {
                                            theme::ACCENT()
                                        } else if c.minor {
                                            theme::DIM()
                                        } else {
                                            theme::TEXT()
                                        })
                                        .child(SharedString::from(c.name.clone())),
                                )
                                .child(
                                    div()
                                        .text_size(px(9.))
                                        .text_color(theme::FAINT())
                                        .text_ellipsis()
                                        .child(SharedString::from(
                                            if c.minor {
                                                "过场 · 路人".to_string()
                                            } else {
                                                c.roles_line()
                                            },
                                        )),
                                ),
                        ),
                )
                // 基本信息表
                .child(
                    div()
                        .w_full()
                        .flex_shrink_0()
                        .px(px(10.))
                        .py(px(6.))
                        .flex()
                        .flex_col()
                        .gap(px(2.))
                        .border_b_1()
                        .border_color(theme::LINE_SOFT())
                        .children(info.into_iter().map(|(k, v)| {
                            div()
                                .flex()
                                .items_start()
                                .gap(px(6.))
                                .child(
                                    div()
                                        .w(px(26.))
                                        .flex_shrink_0()
                                        .text_size(px(9.))
                                        .text_color(theme::FAINT())
                                        .child(SharedString::from(k)),
                                )
                                .child(
                                    div()
                                        .flex_1()
                                        .min_w_0()
                                        .text_size(px(10.))
                                        .line_height(relative(1.35))
                                        .text_color(theme::DIM())
                                        .overflow_hidden()
                                        .line_clamp(2)
                                        .child(SharedString::from(v)),
                                )
                        })),
                )
                // 背景故事
                .child(
                    div()
                        .w_full()
                        .flex_shrink_0()
                        .px(px(10.))
                        .py(px(6.))
                        .child(block("背景故事", bg.clone(), "还没在设定里写这个人的来历", 2)),
                )
                .child(div().w_full().flex_shrink_0().px(px(10.)).child(
                    div().h(px(1.)).w_full().bg(theme::LINE_SOFT()),
                ))
                // 相关剧情：真事件，带章号，按章排
                .child(
                    div()
                        .w_full()
                        .flex_1()
                        .min_h_0()
                        .px(px(10.))
                        .py(px(6.))
                        .flex()
                        .flex_col()
                        .gap(px(3.))
                        .child(
                            div()
                                .flex_shrink_0()
                                .flex()
                                .items_center()
                                .gap(px(4.))
                                .child(
                                    div()
                                        .w(px(2.))
                                        .h(px(9.))
                                        .rounded(px(1.))
                                        .bg(accent),
                                )
                                .child(
                                    div()
                                        .text_size(px(9.5))
                                        .font_weight(FontWeight::MEDIUM)
                                        .text_color(theme::FAINT())
                                        .child(SharedString::from(format!(
                                            "相关剧情 · {}",
                                            plot.len()
                                        ))),
                                ),
                        )
                        .child(if plot.is_empty() {
                            div()
                                .text_size(px(10.5))
                                .line_height(relative(1.5))
                                .text_color(theme::GHOST())
                                .child("还没有写到这个人的事")
                                .into_any_element()
                        } else {
                            div()
                                .flex_1()
                                .min_h_0()
                                .w_full()
                                .overflow_hidden()
                                .flex()
                                .flex_col()
                                .gap(px(2.))
                                .children(plot.iter().take(5).map(|(ch, fact)| {
                                    div()
                                        .flex()
                                        .items_start()
                                        .gap(px(5.))
                                        .child(
                                            div()
                                                .flex_shrink_0()
                                                .font_family(theme::FAMILY_MONO)
                                                .text_size(px(8.5))
                                                .text_color(accent)
                                                .child(SharedString::from(format!("{ch:02}"))),
                                        )
                                        .child(
                                            div()
                                                .flex_1()
                                                .min_w_0()
                                                .text_size(px(10.))
                                                .line_height(relative(1.35))
                                                .text_color(theme::DIM())
                                                .overflow_hidden()
                                                .text_ellipsis()
                                                .child(SharedString::from(fact.clone())),
                                        )
                                }))
                                .when(plot.len() > 5, |d| {
                                    d.child(
                                        div()
                                            .text_size(px(9.))
                                            .text_color(theme::FAINT())
                                            .child(SharedString::from(format!(
                                                "…另 {} 条点开看",
                                                plot.len() - 5
                                            ))),
                                    )
                                })
                                .into_any_element()
                        }),
                )
                // 牌脚：完整度 + 点开提示
                .child(
                    div()
                        .w_full()
                        .flex_shrink_0()
                        .px(px(10.))
                        .py(px(5.))
                        .flex()
                        .items_center()
                        .gap(px(5.))
                        .border_t_1()
                        .border_color(theme::LINE_SOFT())
                        .child(
                            div()
                                .flex()
                                .gap(px(1.5))
                                .children((0..5).map(move |i| {
                                    div()
                                        .w(px(4.))
                                        .h(px(4.))
                                        .rounded(px(2.))
                                        .bg(if i < filled_of(c) { accent } else { theme::LINE_SOFT() })
                                })),
                        )
                        .child(
                            div()
                                .flex_1()
                                .text_size(px(8.5))
                                .text_color(theme::FAINT())
                                .child(SharedString::from("点开 · 编修 / 立绘位留空")),
                        ),
                );
            card
        };

        // 牌墙：按列数分行排开。卡是固定尺寸的实体牌，行内等高，牌墙就该是齐的。
        //
        // 列数按"可用宽度"算，而可用宽度随窗口 / 侧栏 / 抽屉变化。
        // 量法：在牌墙里放一条 **w_full 的零高测量条**，读它 prepaint 后的宽度——
        // 它永远等于容器宽，与排了几列无关。
        // 为什么不能量第一行再反推：行宽 = 张数 × 卡宽，用行宽算列数会越算越少，
        // 最后稳定在只剩一列（这个 bug 真的出过一次，界面看着像"卡排不下"）。
        let probe = div()
            .w_full()
            .h(px(1.))
            .flex_shrink_0()
            .opacity(0.)
            .on_children_prepainted(move |_, _, _| {});
        let wall_probe_cell = width_cell.clone();
        let mut wall = div()
            .w_full()
            .flex()
            .flex_col()
            .gap(px(gap))
            .child(
                div()
                    .w_full()
                    .flex()
                    .gap(px(gap))
                    .on_children_prepainted(move |bounds, _, _| {
                        let w = bounds.first().map(|b| b.size.width.as_f32()).unwrap_or(0.);
                        if w > 1. && (wall_probe_cell.get() as f32 - w).abs() > 0.5 {
                            wall_probe_cell.set(w);
                        }
                    })
                    .child(probe),
            );
        let mut row: Vec<gpui_kit::AnyElement> = Vec::new();
        let mut shown_minor_head = false;
        for c in &ordered {
            if c.minor && !shown_minor_head {
                if !row.is_empty() {
                    wall = wall.child(div().flex().gap(px(gap)).children(std::mem::take(&mut row)));
                }
                shown_minor_head = true;
                wall = wall.child(
                    div()
                        .w_full()
                        .pt(theme::PAD_SM)
                        .text_size(theme::FONT_MICRO)
                        .text_color(theme::FAINT())
                        .child(SharedString::from(format!("过场 / 路人 · {minor_count}"))),
                );
            }
            let on = selected.as_deref() == Some(c.id.as_str());
            let id = c.id.clone();
            let data = card_data(c);
            // 点击挂在普通 div 壳上：AnyElement 本身不能挂 handler，
            // 卡面保持纯展示，壳只负责"点这张牌 = 翻开这一张"。
            row.push(
                div()
                    .id(SharedString::from(format!("char-card-{}", c.id)))
                    .flex_shrink_0()
                    .on_click(cx.listener(move |this, _, window, cx| {
                        this.open_card(id.clone(), window, cx);
                    }))
                    .child(face(c, on, &data))
                    .into_any_element(),
            );
            if row.len() >= columns {
                wall = wall.child(
                    div()
                        .flex()
                        .items_start()
                        .gap(px(gap))
                        .children(std::mem::take(&mut row)),
                );
            }
        }
        if !row.is_empty() {
            wall = wall.child(div().flex().items_start().gap(px(gap)).children(row));
        }

        // —— 右侧：编修面（点开某张卡才出现）——
        //
        // 卡面已经把"这个人是谁"讲完了；这里只做卡面做不了的事：
        // 改字段、锁字段、加别名、翻到核心剧情、派活给人物 Agent。
        // 没点开时右侧只留一张小传预览——作者逛卡簿时不该先被一张空表单挡住。
        // 空书架：没有卡就没有抽屉可言，只留一张空态引导。
        let detail_empty = empty_hint(
            "还没有人物卡",
            "在下面的输入框里写个名字点「新建」；或从已有记忆迁移（别名 + 事实 → 人物卡）。\
             路人也要建卡——信息可以简略，但别让他无名。",
        );
        // 抽屉内容（展开态才渲染）：完整人设卡。
        let drawer: Option<gpui_kit::Div> = match self.current().cloned() {
            None => None,
            Some(c) => {
                let cid = c.id.clone();
                let flipped = self.flipped;
                let clet = c.clone();
                let aliases = c.aliases.clone();
                let arc = c.arc.clone();
                let locked = c.locked_fields.clone();
                let minor = c.minor;
                let status_ch = c.status_chapter;
                let name = c.name.clone();
                let accent = if minor { theme::LINE_SOFT() } else { theme::ACCENT() };
                let _ = &clet;

                // 别名条
                let mut alias_chips = div().flex().flex_wrap().gap(theme::GAP_XS);
                for a in &aliases {
                    let a2 = a.clone();
                    alias_chips = alias_chips.child(
                        div()
                            .flex()
                            .items_center()
                            .gap(px(3.))
                            .px(px(6.))
                            .py(px(1.))
                            .rounded(px(4.))
                            .bg(theme::PANEL2())
                            .child(
                                div()
                                    .text_size(theme::FONT_MICRO)
                                    .text_color(theme::DIM())
                                    .child(SharedString::from(a.clone())),
                            )
                            .child(
                                div()
                                    .id(SharedString::from(format!("rm-alias-{a2}")))
                                    .cursor_pointer()
                                    .text_size(theme::FONT_MICRO)
                                    .text_color(theme::FAINT())
                                    .child("×")
                                    .on_click(cx.listener(move |_, _, _, cx| {
                                        cx.emit(CharacterEvent::RemoveAlias(a2.clone()));
                                    })),
                            ),
                    );
                }
                if aliases.is_empty() {
                    alias_chips = alias_chips.child(
                        div()
                            .text_size(theme::FONT_MICRO)
                            .text_color(theme::FAINT())
                            .child("（无别名）"),
                    );
                }

                // 锁定开关
                let mut locks = div().flex().flex_wrap().gap(theme::GAP_XS);
                for fname in ["voice", "motivation", "background", "personality", "coreStory"] {
                    let on = locked.iter().any(|x| x == fname);
                    let f2 = fname.to_string();
                    locks = locks.child(
                        div()
                            .id(SharedString::from(format!("lock-{fname}")))
                            .px(px(6.))
                            .py(px(1.))
                            .rounded(px(4.))
                            .border_1()
                            .border_color(if on { theme::AMBER() } else { theme::LINE() })
                            .cursor_pointer()
                            .child(
                                div()
                                    .text_size(theme::FONT_MICRO)
                                    .text_color(if on { theme::AMBER() } else { theme::FAINT() })
                                    .child(if on {
                                        format!("🔒 {fname}")
                                    } else {
                                        format!("锁 {fname}")
                                    }),
                            )
                            .on_click(cx.listener(move |_, _, _, cx| {
                                cx.emit(CharacterEvent::ToggleLock(f2.clone()));
                            })),
                    );
                }

                // 正面：完整人物信息（分区，不是一坨表单）
                let front: gpui_kit::AnyElement = div()
                    .flex()
                    .flex_col()
                    .gap(theme::GAP_SM)
                    // 一行三列在 440 宽的面板里会把标签挤成「角色定」这种半截词，
                    // 改成每行两列：标签都完整、输入框也够宽。
                    .child(field("本名", &self.ed_name))
                    .child(
                        div()
                            .flex()
                            .gap(theme::GAP_MD)
                            .child(div().flex_1().min_w_0().child(field("定位", &self.ed_role)))
                            .child(div().flex_1().min_w_0().child(field("阵营", &self.ed_faction))),
                    )
                    .child(
                        div()
                            .flex()
                            .gap(theme::GAP_MD)
                            .child(div().flex_1().min_w_0().child(field("年龄", &self.ed_age)))
                            .child(div().flex_1().min_w_0().child(field("外貌", &self.ed_appearance))),
                    )
                    .child(field("性格", &self.ed_personality))
                    .child(field("动机 · 想要什么", &self.ed_motivation))
                    .child(field("背景", &self.ed_background))
                    .child(field("口吻 · 写对白时查这条", &self.ed_voice))
                    .child(
                        div()
                            .flex()
                            .gap(theme::GAP_MD)
                            .child(div().flex_1().min_w_0().child(field("当前状态", &self.ed_status)))
                            .child(div().w(px(120.)).flex_shrink_0().child(field("自第几章", &self.ed_status_ch))),
                    )
                    .child(sec_title("别名"))
                    .child(alias_chips)
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap(theme::GAP_SM)
                            .child(div().flex_1().child(field("", &self.ed_new)))
                            .child(cap_btn("char-add-alias", "加别名", cx).on_click(
                                cx.listener(|this, _, _, cx| {
                                    let v = this.ed_new.read(cx).value().to_string();
                                    if !v.trim().is_empty() {
                                        cx.emit(CharacterEvent::AddAlias(v));
                                    }
                                }),
                            )),
                    )
                    // 只读速览：卡面上已有、但面板上缺的那几项收进来，
                    // 免得"完整人设卡"点开后下半截是空的。
                    // 只有真的写了东西才画这一块：目标/缺陷/弧线/备注全空时
                    // 留一个空标题框，比不画更难看（作者会以为数据丢了）。
                    .when(
                        !clet.goals.is_empty()
                            || !clet.flaws.is_empty()
                            || !arc.is_empty()
                            || !clet.notes.trim().is_empty(),
                        |d| {
                            d.child(
                                div()
                            .mt(theme::GAP_SM)
                            .p(theme::PAD_MD)
                            .rounded(theme::RADIUS_PANEL)
                            .bg(theme::PANEL2())
                            .border_1()
                            .border_color(theme::LINE_SOFT())
                            .flex()
                            .flex_col()
                            .gap(px(4.))
                            .child(
                                div()
                                    .text_size(theme::FONT_LABEL)
                                    .font_weight(FontWeight::SEMIBOLD)
                                    .text_color(theme::TEXT())
                                    .child("档案速览"),
                            )
                            .child(summary_row("目标", &clet.goals.join("、")))
                            .child(summary_row("缺陷", &clet.flaws.join("、")))
                            .child(summary_row(
                                "弧线",
                                &if arc.is_empty() {
                                    String::new()
                                } else {
                                    arc.iter()
                                        .map(|p| format!("第{}章 {}", p.chapter, p.state))
                                        .collect::<Vec<_>>()
                                        .join(" → ")
                                },
                            ))
                            .child(summary_row("备注", &clet.notes)),
                            )
                        },
                    )
                    .into_any_element();

                // 背面：核心剧情（翻卡时的那一面）
                let arc_line = if arc.is_empty() {
                    "（还没有弧线记录）".to_string()
                } else {
                    arc.iter()
                        .map(|p| format!("第{}章 {}", p.chapter, p.state))
                        .collect::<Vec<_>>()
                        .join(" → ")
                };
                let back: gpui_kit::AnyElement = div()
                    .flex()
                    .flex_col()
                    .gap(theme::GAP_MD)
                    .child(
                        div()
                            .text_size(theme::FONT_MICRO)
                            .text_color(theme::FAINT())
                            .child("这个角色在故事里承担了什么——不是时间表，是他在本书里的份量。"),
                    )
                    .child(div().flex_1().min_h(px(180.)).child(field(
                        "核心剧情",
                        &self.ed_core_story,
                    )))
                    .child(sec_title("弧线（按章）"))
                    .child(
                        div()
                            .text_size(theme::FONT_MICRO)
                            .line_height(relative(theme::LINE_HEIGHT_BODY))
                            .text_color(theme::DIM())
                            .child(SharedString::from(arc_line)),
                    )
                    .child(sec_title("AI 不可改的字段"))
                    .child(locks)
                    .child({
                        // 标记按钮：卡属性，直接落盘（不等"保存"）
                        div()
                            .flex()
                            .items_center()
                            .gap(theme::GAP_SM)
                            .pt(theme::GAP_SM)
                            .border_t_1()
                            .border_color(theme::LINE())
                            .child(
                                div()
                                    .text_size(theme::FONT_MICRO)
                                    .text_color(theme::DIM())
                                    .child("这是过场 / 路人角色"),
                            )
                            .child(div().flex_1())
                            .child(
                                div()
                                    .id("char-minor")
                                    .px(px(8.))
                                    .py(px(2.))
                                    .rounded(px(5.))
                                    .border_1()
                                    .border_color(if minor {
                                        theme::ACCENT_BORDER()
                                    } else {
                                        theme::LINE()
                                    })
                                    .bg(if minor { theme::ACCENT_SOFT() } else { theme::PANEL2() })
                                    .cursor_pointer()
                                    .text_size(theme::FONT_MICRO)
                                    .text_color(if minor { theme::ACCENT() } else { theme::DIM() })
                                    .child(if minor { "已标记" } else { "标记" })
                                    .on_click(cx.listener(|this, _, window, cx| {
                                        if let Some(mut c) = this.draft(cx) {
                                            c.minor = !c.minor;
                                            let now_minor = c.minor;
                                            let roster =
                                                crate::characters::Roster::new(&this.handle.root);
                                            // 这个开关也是落库：不记 journal 的话，
                                            // 作者点错了在「改动记录」里撤不回来。
                                            let path = this.handle.root.join("characters.json");
                                            let before = std::fs::read_to_string(&path).unwrap_or_default();
                                            match roster.upsert(c) {
                                                Ok(()) => {
                                                    let after = std::fs::read_to_string(&path).unwrap_or_default();
                                                    if before != after {
                                                        let _ = crate::journal::Journal::new(&this.handle.root).record(
                                                            &this.handle.root,
                                                            "characters.json",
                                                            &before,
                                                            &after,
                                                            &crate::journal::Actor::Author,
                                                            "edit",
                                                            &format!("b{}", crate::journal::now_millis()),
                                                            if now_minor { "标记为过场/路人" } else { "移出过场" },
                                                        );
                                                    }
                                                    this.message = Some(if now_minor {
                                                        "已标记为过场 / 路人".into()
                                                    } else {
                                                        "已移出过场".into()
                                                    });
                                                }
                                                Err(e) => {
                                                    this.message = Some(format!("保存失败：{e}"))
                                                }
                                            }
                                            this.reload(window, cx);
                                        }
                                    })),
                            )
                    })
                    .into_any_element();

                // —— 展开态：右侧抽屉（完整人设卡）——
                let header = div()
                    .flex()
                    .flex_wrap()
                    .items_center()
                    .gap(theme::GAP_SM)
                    .pb(theme::GAP_SM)
                    .border_b_1()
                    .border_color(theme::LINE())
                    .child(
                        div()
                            .w(px(34.))
                            .h(px(34.))
                            .flex_shrink_0()
                            .rounded(px(17.))
                            .border_1()
                            .border_color(accent)
                            .flex()
                            .items_center()
                            .justify_center()
                            .child(
                                div()
                                    .text_size(px(15.))
                                    .font_family(theme::FAMILY_SERIF)
                                    .text_color(accent)
                                    .child(SharedString::from(
                                        c.name.chars().next().map(|x| x.to_string()).unwrap_or_default(),
                                    )),
                            ),
                    )
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .gap(px(1.))
                            .min_w_0()
                            .child(
                                div()
                                    .text_size(theme::FONT_LABEL)
                                    .font_family(theme::FAMILY_SERIF)
                                    .font_weight(FontWeight::SEMIBOLD)
                                    .text_color(if minor { theme::DIM() } else { theme::TEXT() })
                                    .child(SharedString::from(name.clone())),
                            )
                            .children({
                                let mut meta: Vec<String> = Vec::new();
                                if !c.role.trim().is_empty() {
                                    meta.push(c.role.clone());
                                }
                                if !c.faction.trim().is_empty() {
                                    meta.push(c.faction.clone());
                                }
                                if minor {
                                    meta.push("过场 / 路人".into());
                                }
                                if meta.is_empty() {
                                    None
                                } else {
                                    Some(
                                        div()
                                            .text_size(theme::FONT_MICRO)
                                            .text_color(theme::FAINT())
                                            .text_ellipsis()
                                            .child(SharedString::from(meta.join(" · "))),
                                    )
                                }
                            }),
                    )
                    .child(div().flex_1().min_w(px(8.)))
                    // 翻面：卡牌的核心交互。正面写档案，背面看剧情。
                    .child(
                        div()
                            .id("char-flip")
                            .px(px(9.))
                            .py(px(3.))
                            .rounded(px(6.))
                            .border_1()
                            .border_color(if flipped {
                                theme::ACCENT_BORDER()
                            } else {
                                theme::LINE()
                            })
                            .bg(if flipped { theme::ACCENT_SOFT() } else { theme::PANEL2() })
                            .cursor_pointer()
                            .text_size(theme::FONT_MICRO)
                            .text_color(if flipped { theme::ACCENT() } else { theme::DIM() })
                            .child(if flipped { "翻回正面" } else { "翻到背面 · 核心剧情" })
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.flipped = !this.flipped;
                                cx.notify();
                            })),
                    )
                    .child(
                        primary_btn("char-save", "保存", cx).on_click(cx.listener(
                            |this, _, _, cx| {
                                if let Some(c) = this.draft(cx) {
                                    cx.emit(CharacterEvent::Save(Box::new(c)));
                                }
                            },
                        )),
                    )
                    .child(
                        cap_btn("char-delete", "删除", cx).on_click(cx.listener(
                            move |_, _, _, cx| cx.emit(CharacterEvent::Delete(cid.clone())),
                        )),
                    )
                    // 收起：抽屉要能退回卡簿，不能"关不掉"
                    .child(
                        div()
                            .id("char-close")
                            .px(px(7.))
                            .py(px(3.))
                            .rounded(px(6.))
                            .cursor_pointer()
                            .text_size(theme::FONT_MICRO)
                            .text_color(theme::FAINT())
                            .hover(|d| d.text_color(theme::TEXT()))
                            .child("收起 ✕")
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.close_card(cx);
                            })),
                    )
                    // 针对这个人的操作：选中沈砚 → 只补沈砚，不顺带动别人。
                    .children(crate::capabilities::agent("character").map(|a| {
                        let mut bar = div().flex().gap(theme::GAP_SM);
                        for op in a.targeted {
                            if op.target_kind != crate::capabilities::TargetKind::Character {
                                continue;
                            }
                            let desc = crate::prompts::describe_character(&c);
                            bar = bar.child(
                                cap_btn(op.key, op.label, cx).on_click(cx.listener(
                                    move |_this, _, _, cx| {
                                        cx.emit(CharacterEvent::Delegate {
                                            key: op.key,
                                            target: Some(desc.clone()),
                                        });
                                    },
                                )),
                            );
                        }
                        bar.into_any_element()
                    }));

                Some(
                    div()
                        .flex()
                        .flex_col()
                        .gap(theme::GAP_MD)
                        .child(header)
                        .child(div().flex_1().min_h_0().child(if flipped { back } else { front }))
                        .when_true(status_ch > 0 && !flipped, |d| {
                            d.child(
                                div()
                                    .text_size(theme::FONT_MICRO)
                                    .text_color(theme::FAINT())
                                    .child(SharedString::from(format!("状态自第{status_ch}章起"))),
                            )
                        }),
                )
            }
        };

        // —— 合并：选另一个人并进来 ——
        let others: Vec<(String, String)> = self
            .people
            .iter()
            .filter(|c| Some(c.id.as_str()) != selected.as_deref())
            .map(|c| (c.id.clone(), c.name.clone()))
            .collect();

        // —— 页面骨架：页头（一行：标题 + 统计 + 工具）→ 牌墙 → 可选编修面板 ——
        //
        // 上一版的问题是"左侧牌墙 + 右上角永远开着的预览卡"：预览卡只有
        // 三行字，却占走 240px 宽和一片空白，还让牌墙在最常见的一屏里
        // 只能排两列、右边还是空的。卡面本身已经把"这个人是谁"讲完了，
        // 预览是重复的——删掉它，牌墙居中铺满，要改就点开编修面板。
        //
        // 牌墙居中而不是左贴：书只有几张卡时靠左会留下半屏空白，
        // 居中后空白平均落在两边，视觉上是"一叠牌摆在桌上"。
        let show_drawer = drawer.is_some();

        // 页头工具：新建 / 迁徙 / 合并 / 能力，全部收到一行里，
        // 不再把「新建」压到牌墙下面（那里每加一张卡就得往下滚才点得到）。
        let mut tools = div().flex().items_center().gap(theme::GAP_MD).flex_wrap();
        let _ = &mut tools;
        let mut header = div()
            .flex()
            .items_center()
            .gap(theme::GAP_MD)
            .flex_wrap()
            .child(
                div()
                    .flex()
                    .items_baseline()
                    .gap(theme::GAP_MD)
                    .child(sec_title("人物卡"))
                    .child(
                        div()
                            .text_size(theme::FONT_MICRO)
                            .text_color(theme::FAINT())
                            .child(SharedString::from(format!(
                                "{} 张（主要 {} · 过场 {}）· 点一张翻开看完整人设",
                                self.people.len(),
                                main_count,
                                minor_count
                            ))),
                    ),
            )
            .child(div().flex_1())
            .child(cap_btn("char-migrate", "从记忆迁移", cx).on_click(
                cx.listener(|_, _, _, cx| cx.emit(CharacterEvent::Migrate)),
            ))
            .child(div().w(px(150.)).child(field("", &self.ed_new)))
            .child(cap_btn("char-create", "新建", cx).on_click(cx.listener(
                |this, _, _, cx| {
                    let v = this.ed_new.read(cx).value().to_string();
                    if !v.trim().is_empty() {
                        cx.emit(CharacterEvent::Create(v));
                    }
                },
            )));
        if let Some(a) = crate::capabilities::agent("character") {
            for cap in a.bulk {
                header = header.child(cap_btn(cap.key, cap.label, cx).on_click(
                    cx.listener(move |_this, _, _, cx| {
                        cx.emit(CharacterEvent::Delegate { key: cap.key, target: None });
                    }),
                ));
            }
        }
        header = header.when(message.is_some(), |d| {
            let text = message.clone().unwrap_or_default();
            let tone = if theme::feedback_is_error(&text) {
                Tone::Amber
            } else {
                Tone::Green
            };
            d.child(chip(SharedString::from(text), tone))
        });

        // 牌墙区：抽屉开着时让出右侧（520 + 间距），牌墙自动少一列；
        // 没开时占满整行并居中。
        let wall_area = div()
            .w_full()
            .flex()
            .justify_center()
            .child(wall);

        let body = div().flex().gap(theme::GAP_GROUP).items_start().child(
            div()
                .flex_1()
                .min_w_0()
                .flex()
                .flex_col()
                .gap(theme::GAP_MD)
                .child(wall_area)
                // 变并：只在明明有两张卡以上时才出现，别让它在只有一个人时占一行
                .when_true(others.len() > 0 && self.current().is_some(), |d| {
                    let into = self.current().map(|c| c.id.clone()).unwrap_or_default();
                    let into_name = self.current().map(|c| c.name.clone()).unwrap_or_default();
                    let mut rows = div().flex().flex_wrap().gap(theme::GAP_XS);
                    for (oid, oname) in others.clone() {
                        let i2 = into.clone();
                        rows = rows.child(
                            cap_btn(
                                Box::leak(format!("merge-{oid}").into_boxed_str()),
                                Box::leak(format!("并入「{oname}」").into_boxed_str()),
                                cx,
                            )
                            .on_click(cx.listener(move |_, _, _, cx| {
                                cx.emit(CharacterEvent::Merge {
                                    into: i2.clone(),
                                    from: oid.clone(),
                                });
                            })),
                        );
                    }
                    d.child(
                        div()
                            .flex()
                            .items_center()
                            .gap(theme::GAP_SM)
                            .flex_wrap()
                            .pt(theme::GAP_SM)
                            .border_t_1()
                            .border_color(theme::LINE_SOFT())
                            .child(
                                div()
                                    .flex_shrink_0()
                                    .text_size(theme::FONT_MICRO)
                                    .text_color(theme::FAINT())
                                    .child(SharedString::from(format!(
                                        "并入「{into_name}」：别名并集、空格子补齐"
                                    ))),
                            )
                            .child(rows),
                    )
                }),
        );
        // 编修面板**浮在右侧**，不占牌墙的列宽。
        //
        // 上一版把它当 flex 兄弟：一翻开就把牌墙压到 366px，卡立刻从三列掉成一列，
        // 作者每翻一个人整面牌墙重排一次——眼睛跟不上。做成浮层后牌墙列数恒定，
        // 只有最右侧那一列会被盖住，关掉面板立刻还原。
        // 浮层不挡"看"的路径：点卡、滚动、页头工具都在左侧与左上方。
        let drawer_overlay: gpui_kit::AnyElement = match drawer {
            Some(d) => div()
                .absolute()
                .top(px(64.))
                .right(px(0.))
                .w(px(440.))
                .p(theme::PAD_LG)
                .bg(theme::PANEL())
                .border_1()
                .border_color(theme::ACCENT_BORDER())
                .rounded(theme::RADIUS_CARD)
                .shadow_lg()
                .child(d)
                .into_any_element(),
            None => div().into_any_element(),
        };
        let _ = show_drawer;

        div()
            .relative()
            .size_full()
            .child(
                panel_root("characters-scroll").child(
                    panel_col()
                        .child(header)
                        .when(self.people.is_empty(), |d| d.child(detail_empty))
                        .when(!self.people.is_empty(), |d| d.child(body)),
                ),
            )
            .child(drawer_overlay)
    }
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
            .child("人物档案")
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
    /// 保存本书专属 Agent（空 persona = 删除，恢复默认）
    SaveBookPreset { name: String, persona: String, takeover: bool },
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
    /// 闲置多少分钟回收本书的 DSH 进程（0 = 不回收）
    ed_idle: Entity<gpui_kit::component::input::InputState>,
    /// 同时活着的 DSH 进程上限
    ed_max_procs: Entity<gpui_kit::component::input::InputState>,
    /// 内置插件开关的界面镜像（保存时整体写回 settings）
    plugin_overrides: std::collections::BTreeMap<String, bool>,
    /// 主题形态镜像（ink / xuan）
    theme_sel: String,
    /// 外观操作回执（主题切换成功/失败），渲染在外观卡片里
    theme_msg: Option<SharedString>,
    /// 账号快照（余额、登录状态）；None = 还没查过
    account: Option<crate::account::AccountSnapshot>,
    /// 余额正在查
    account_busy: bool,
    status: String,
    testing: bool,
    /// 本书专属 Agent：名字与人格（空 = 还没有 / 已删除）
    ed_book_name: Entity<gpui_kit::component::input::InputState>,
    ed_book_persona: Entity<gpui_kit::component::input::InputState>,
    book_takeover: bool,
}

impl SettingsPanel {
    pub fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        let cfg = crate::store::load_settings();
        // 本书专属 Agent 存在 DSH_HOME（按书隔离），设置页打开时读一遍回填。
        // 面板不持有 root，所以由宿主在构造后调用 load_book_preset 注入。
        let book_name = cx.new(|cx| gpui_kit::component::input::InputState::new(window, cx).placeholder("这本书的助手叫什么（如：沈砚助理）"));
        let book_persona = cx.new(|cx| {
            gpui_kit::component::input::InputState::new(window, cx)
                .placeholder("它是什么样的人、熟悉什么、该怎么说话（留空 = 删除，恢复默认）")
        });
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
        let ed_idle = cx.new(|cx| {
            gpui_kit::component::input::InputState::new(window, cx)
                .placeholder("30")
                .default_value(cfg.idle_recycle_min.to_string())
        });
        let ed_max_procs = cx.new(|cx| {
            gpui_kit::component::input::InputState::new(window, cx)
                .placeholder("2")
                .default_value(cfg.max_runtime_procs.to_string())
        });
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
            ed_idle,
            ed_max_procs,
            plugin_overrides: cfg.plugin_overrides.clone(),
            // 以**内存里的真实主题**为准，不是以 settings.json 为准：
            // 跑起来之后主题可能是标题栏按钮切的、也可能是启动参数覆盖的，
            // 这时读盘会显示成旧值——作者看到的就是"选了宣纸但框还在墨底"。
            theme_sel: crate::theme::theme_kind().persist_key().to_string(),
            theme_msg: None,
            account: None,
            account_busy: false,
            status: String::new(),
            testing: false,
            ed_book_name: book_name,
            ed_book_persona: book_persona,
            book_takeover: false,
        }
    }

    /// 回填本书专属 Agent（宿主在构造后调用）。
    ///
    /// 做成独立方法而不是构造参数：设置页先于书打开也可能被构造，
    /// 那时还不知道是哪本书；等宿主确定了书再来注入。
    pub fn load_book_preset(
        &mut self,
        preset: Option<crate::dsh::BookPreset>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let (name, persona, takeover) = match preset {
            Some(b) => (b.name, b.persona, b.takeover),
            None => (String::new(), String::new(), false),
        };
        self.ed_book_name.update(cx, |st, cx| {
            st.set_value(&name, window, cx);
        });
        self.ed_book_persona.update(cx, |st, cx| {
            st.set_value(&persona, window, cx);
        });
        self.book_takeover = takeover;
        cx.notify();
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

    /// 内置插件清单：按组列出，每行说明"它是干什么的"和"关掉会失去什么"。
    ///
    /// 单独一个方法而不是内联在 render 里：这段有嵌套循环 + 两种行形态
    /// （必需项是锁定标签，可选项是开关），写在 render 的链式调用中间会
    /// 完全读不出来。
    fn plugin_rows(&self, cx: &mut Context<Self>) -> Vec<gpui_kit::AnyElement> {
        let mut out = Vec::new();
        for (g, items) in crate::plugins::grouped() {
            out.push(
                div()
                    .pt(theme::GAP_SM)
                    .text_size(theme::FONT_MICRO)
                    .font_weight(FontWeight::SEMIBOLD)
                    .text_color(theme::FAINT())
                    .child(g.label())
                    .into_any_element(),
            );
            for p in items {
                let row = p.row;
                let on = crate::plugins::enabled(&self.plugin_overrides, row);
                let right = if p.required {
                    div()
                        .flex_shrink_0()
                        .px(px(6.))
                        .py(px(1.))
                        .rounded(theme::RADIUS_TAG)
                        .bg(theme::CHIP_NEUTRAL())
                        .text_size(theme::FONT_MICRO)
                        .text_color(theme::FAINT())
                        .child("必需")
                        .into_any_element()
                } else {
                    switch_row(SharedString::from(format!("plugin-{row}")), on, cx, move |this, cx| {
                        let cur = crate::plugins::enabled(&this.plugin_overrides, row);
                        this.plugin_overrides.insert(row.to_string(), !cur);
                        cx.notify();
                    })
                };
                out.push(
                    div()
                        .flex()
                        .items_start()
                        .gap(theme::GAP_MD)
                        .child(
                            div()
                                .flex_1()
                                .min_w_0()
                                .flex()
                                .flex_col()
                                .child(
                                    div()
                                        .text_size(theme::FONT_LABEL)
                                        .text_color(if p.required { theme::DIM() } else { theme::TEXT() })
                                        .child(SharedString::from(p.label)),
                                )
                                .child(
                                    div()
                                        .text_size(theme::FONT_MICRO)
                                        .line_height(relative(theme::LINE_HEIGHT_BODY))
                                        .text_color(theme::FAINT())
                                        .child(SharedString::from(p.what)),
                                )
                                // 关掉的代价：这段比"它是干什么的"更重要
                                .when_true(!on, |d| {
                                    d.child(
                                        div()
                                            .text_size(theme::FONT_MICRO)
                                            .line_height(relative(theme::LINE_HEIGHT_BODY))
                                            .text_color(theme::AMBER())
                                            .child(SharedString::from(format!("已关闭 · {}", p.without))),
                                    )
                                }),
                        )
                        .child(right)
                        .into_any_element(),
                );
            }
        }
        out
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
        // 空串 = 保留原值：作者清空输入框不该把功能悄悄变成"不回收 / 上限 0"
        let parse_u32 = |s: &str, cur: u32| s.trim().parse::<u32>().unwrap_or(cur);
        let cur = crate::store::load_settings();
        cfg.idle_recycle_min =
            parse_u32(&self.ed_idle.read(cx).value(), cur.idle_recycle_min);
        // 上限至少 1：0 会让开书本身失败
        cfg.max_runtime_procs = parse_u32(&self.ed_max_procs.read(cx).value(), cur.max_runtime_procs)
            .max(1);
        cfg.plugin_overrides = self.plugin_overrides.clone();
        cfg.theme = self.theme_sel.clone();
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
/// 只有开关、没有标签的行（用于标签已经写在左边的场景）。
///
/// `toggle_row` 把"开/关 + 标签"绑成一个整体点击区；插件页每一行左边是
/// 名字 + 说明 + 代价，右边只要一个开关，所以不能复用。
fn switch_row<F>(
    id: SharedString,
    on: bool,
    cx: &mut Context<SettingsPanel>,
    toggle: F,
) -> gpui_kit::AnyElement
where
    F: Fn(&mut SettingsPanel, &mut Context<SettingsPanel>) + 'static,
{
    let toggle = std::rc::Rc::new(toggle);
    let _ = cx;
    div()
        .id(id)
        .flex_shrink_0()
        .w(px(34.))
        .py(px(2.))
        .rounded(theme::RADIUS_TAG)
        .border_1()
        .border_color(if on {
            theme::GREEN_BORDER()
        } else {
            theme::LINE()
        })
        .bg(if on {
            theme::GREEN_SOFT()
        } else {
            theme::CHIP_NEUTRAL()
        })
        .flex()
        .justify_center()
        .cursor_pointer()
        .text_size(theme::FONT_MICRO)
        .text_color(if on { theme::GREEN() } else { theme::FAINT() })
        .child(if on { "开" } else { "关" })
        .on_click(cx.listener(move |this, _, _, cx| {
            toggle(this, cx);
            cx.notify();
        }))
        .into_any_element()
}

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
                .w(px(68.))
                .flex_shrink_0()
                .text_size(theme::FONT_LABEL)
                .text_color(theme::DIM())
                .text_ellipsis()
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
                .child(sec_title("外观"))
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
                        .child(
                            div()
                                .text_size(theme::FONT_MICRO)
                                .text_color(theme::FAINT())
                                .child("点选即切、立即生效并保存；工作区标题栏的「◐ 主题」按钮也可以一键来回切。"),
                        )
                        .when(self.theme_msg.is_some(), |d| {
                            d.child(
                                div()
                                    .text_size(theme::FONT_MICRO)
                                    .text_color(theme::GREEN())
                                    .child(self.theme_msg.clone().unwrap_or_default()),
                            )
                        })
                        .child(
                            div().flex().gap(theme::GAP_SM).children([
                                ("ink", "墨底 · 深色", "工作台夜间写作的墨色界面"),
                                ("xuan", "宣纸 · 浅色", "米色纸面 + 墨字 + 朱砂印，白天校稿"),
                            ]
                            .into_iter()
                            .map(|(key, label, hint)| {
                                let on = self.theme_sel == key;
                                div()
                                    .id(SharedString::from(format!("theme-{key}")))
                                    .flex_1()
                                    .min_w_0()
                                    .flex()
                                    .flex_col()
                                    .gap(px(2.))
                                    .p(theme::PAD_MD)
                                    .rounded(theme::RADIUS_CARD)
                                    .border_1()
                                    .cursor_pointer()
                                    .border_color(if on { theme::ACCENT_BORDER() } else { theme::LINE() })
                                    .bg(if on { theme::ACCENT_SOFT() } else { theme::PANEL2() })
                                    .child(
                                        div()
                                            .text_size(theme::FONT_LABEL)
                                            .font_weight(FontWeight::SEMIBOLD)
                                            .text_color(if on { theme::ACCENT() } else { theme::TEXT() })
                                            .child(label),
                                    )
                                    .child(
                                        div()
                                            .text_size(theme::FONT_MICRO)
                                            .text_color(theme::FAINT())
                                            .child(hint),
                                    )
                                    .on_click(cx.listener(move |this, _, _, cx| {
                                        this.theme_sel = key.to_string();
                                        // 点选即切并落盘：作者要的是「立刻看到」，
                                        // 而且重启后还得是这个主题。只切不存的话，
                                        // 下次启动又回到 settings.json 的旧值——
                                        // 那正是"切了却像没切"的一半原因。
                                        let kind = crate::theme::ThemeKind::from_setting(key);
                                        match crate::switch_theme(kind, cx) {
                                            Ok(()) => this.theme_msg = Some(SharedString::from(format!(
                                                "已切到{}主题（已保存，重启后保持）",
                                                kind.label()
                                            ))),
                                            Err(e) => this.theme_msg = Some(SharedString::from(format!(
                                                "已切到{}主题，但保存失败：{e}",
                                                kind.label()
                                            ))),
                                        }
                                        cx.notify();
                                    }))
                                    .into_any_element()
                            })
                            .collect::<Vec<_>>()),
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
                        ))
                        // 进程额度：每本书一个 DSH 进程，切书时旧的可能还没释放。
                        // 这两个数字直接决定内存，所以要露出来而不是写死在代码里。
                        .child(field("闲置回收", &self.ed_idle))
                        .child(
                            div()
                                .text_size(theme::FONT_MICRO)
                                .text_color(theme::FAINT())
                                .child("闲置多少分钟后关掉本书的 AI 进程（0 = 一直留着；默认 30）"),
                        )
                        .child(field("进程上限", &self.ed_max_procs))
                        .child(
                            div()
                                .text_size(theme::FONT_MICRO)
                                .text_color(theme::FAINT())
                                .child("同时活着的 AI 进程最多几个（默认 2：当前这本 + 刚切走那本）"),
                        ),
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
                // —— 内置插件配置 ——
                //
                // 计划里的"五类能力"之四。它不是包管理器的复刻：
                // 这里只列**作者可以安全关掉**的行，每一条都写清代价。
                // 关掉写进 settings.json 的 pluginOverrides，由 mode_patch
                // 翻译成 `- id: X / disabled: true`，下次启动运行时生效。
                .child(sec_title("内置插件"))
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
                        .child(
                            div()
                                .text_size(theme::FONT_MICRO)
                                .line_height(relative(theme::LINE_HEIGHT_BODY))
                                .text_color(theme::FAINT())
                                .child("关掉任何一项都会改变 AI 能做什么。这里的开关下次启动 AI 运行时生效。"),
                        )
                        .children(self.plugin_rows(cx)),
                )
                // —— Agent 预设 ——
                //
                // 计划里"五类能力"之五。这里列的不是"可对话的角色"，而是
                // **能力清单**：每个 Agent 负责哪类活、页面上的哪个按钮会叫它、
                // 选中什么对象时能对它做什么。作者看完这张表就知道
                // "我想干这件事该点哪儿"，不用猜。
                .child(sec_title("Agent 预设"))
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
                        .child(
                            div()
                                .text_size(theme::FONT_MICRO)
                                .line_height(relative(theme::LINE_HEIGHT_BODY))
                                .text_color(theme::FAINT())
                                .child("专属 Agent 是能力，不是聊天窗口：它们在各自模块页被按钮唤起，                                        也可以由主 Agent 自己判断该派给谁。"),
                        )
                        .children(crate::capabilities::AGENTS.iter().map(|a| {
                            let bulk: Vec<String> = a.bulk.iter().map(|c| c.label.to_string()).collect();
                            let targeted: Vec<String> = a
                                .targeted
                                .iter()
                                .map(|op| format!("{}（{}）", op.label, op.target_kind.label()))
                                .collect();
                            div()
                                .flex()
                                .flex_col()
                                .gap(px(1.))
                                .pt(theme::GAP_SM)
                                .child(
                                    div()
                                        .flex()
                                        .items_baseline()
                                        .gap(theme::GAP_SM)
                                        .child(
                                            div()
                                                .text_size(theme::FONT_LABEL)
                                                .font_weight(FontWeight::SEMIBOLD)
                                                .text_color(theme::TEXT())
                                                .child(SharedString::from(a.label)),
                                        )
                                        .child(
                                            div()
                                                .font_family(theme::FAMILY_MONO)
                                                .text_size(theme::FONT_MICRO)
                                                .text_color(theme::ACCENT())
                                                .child(SharedString::from(a.id)),
                                        ),
                                )
                                .child(
                                    div()
                                        .text_size(theme::FONT_MICRO)
                                        .line_height(relative(theme::LINE_HEIGHT_BODY))
                                        .text_color(theme::DIM())
                                        .child(SharedString::from(a.duty)),
                                )
                                .when_true(!bulk.is_empty(), |d| {
                                    d.child(
                                        div()
                                            .text_size(theme::FONT_MICRO)
                                            .text_color(theme::FAINT())
                                            .child(SharedString::from(format!(
                                                "模块一键：{}",
                                                bulk.join(" / ")
                                            ))),
                                    )
                                })
                                .when_true(!targeted.is_empty(), |d| {
                                    d.child(
                                        div()
                                            .text_size(theme::FONT_MICRO)
                                            .text_color(theme::FAINT())
                                            .child(SharedString::from(format!(
                                                "选定对象：{}",
                                                targeted.join(" / ")
                                            ))),
                                    )
                                })
                                .into_any_element()
                        })),
                )
                // —— 本书专属 Agent ——
                //
                // 只有一本书一个，存在本书 DSH_HOME 里；删除它就是"恢复默认"。
                // 放在模式卡片下面而不是单独一页：它是模式的一部分（创造模式的产物）。
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
                        .child(
                            div()
                                .flex()
                                .items_baseline()
                                .gap(theme::GAP_SM)
                                .child(
                                    div()
                                        .text_size(theme::FONT_LABEL)
                                        .font_weight(FontWeight::SEMIBOLD)
                                        .text_color(theme::TEXT())
                                        .child("本书专属 Agent"),
                                )
                                .child(
                                    div()
                                        .text_size(theme::FONT_MICRO)
                                        .text_color(theme::FAINT())
                                        .child("在「创造」模式里和 AI 一起定；清空人格即删除并恢复默认"),
                                ),
                        )
                        .child(field("名字", &self.ed_book_name))
                        .child(field("人格与规则", &self.ed_book_persona))
                        .child(toggle_row(
                            "让它接管默认派活",
                            "打开后主 Agent 把写作任务优先派给它；关闭时它只是第 7 个可选 Agent",
                            self.book_takeover,
                            cx,
                            |this, cx| {
                                this.book_takeover = !this.book_takeover;
                                cx.notify();
                            },
                        ))
                        .child(
                            div()
                                .flex()
                                .justify_end()
                                .child(
                                    Button::new("save-book-preset")
                                        .primary()
                                        .xsmall()
                                        .label("保存并重启运行时")
                                        .on_click(cx.listener(|this, _, _, cx| {
                                            let name = this.ed_book_name.read(cx).value().trim().to_string();
                                            let persona =
                                                this.ed_book_persona.read(cx).value().trim().to_string();
                                            cx.emit(SettingsEvent::SaveBookPreset {
                                                name: if name.is_empty() {
                                                    "本书专属".into()
                                                } else {
                                                    name
                                                },
                                                persona,
                                                takeover: this.book_takeover,
                                            });
                                        })),
                                ),
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
        // 快照列表只显示最近 40 个（不是台账，是文件快照；更早的靠目录查看）
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
    /// 导入设定文档（目录路径）
    ImportSettings(String),
    /// 让总 Agent 提取 imports/ 里的材料并分派各专属 Agent
    Extract,
}

pub struct ImportPanel {
    focus_handle: FocusHandle,
    /// 预留给面板内直接扫描（当前由 Workspace 后台执行）
    #[allow(dead_code)]
    handle: Rc<ProjectHandle>,
    path: Entity<gpui_kit::component::input::InputState>,
    /// 设定文档目录（导入后交总 Agent 提取）
    set_path: Entity<gpui_kit::component::input::InputState>,
    last_result: String,
}

impl ImportPanel {
    pub fn new(window: &mut Window, cx: &mut Context<Self>, handle: Rc<ProjectHandle>) -> Self {
        let path = cx.new(|cx| {
            gpui_kit::component::input::InputState::new(window, cx)
                .placeholder("要导入的目录（.md/.txt，按「第N章」启发式分类）")
        });
        let set_path = cx.new(|cx| {
            gpui_kit::component::input::InputState::new(window, cx)
                .placeholder("设定文档目录（.md/.txt：世界观/人物/大纲等非章节材料）")
        });
        Self {
            focus_handle: cx.focus_handle(),
            handle,
            path,
            set_path,
            last_result: String::new(),
        }
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
                            let tone = if theme::feedback_is_error(&result) {
                                Tone::Amber
                            } else {
                                Tone::Green
                            };
                            d.child(chip(SharedString::from(result.clone()), tone))
                        }),
                )
                .child(sec_title("导入设定文档（先归档，再让总 Agent 提取分派）"))
                .child(
                    div()
                        .text_size(theme::FONT_META)
                        .line_height(relative(theme::LINE_HEIGHT_BODY))
                        .text_color(theme::DIM())
                        .child("流程：文档原样存进书的 imports/ 暂存区 → 总 Agent 通读后提取
                            人物/关系/设定/伏笔/大纲 → 分派给对应专属 Agent → 全部落成提案，
                            去收件箱逐条裁决。材料原样保留，不被覆盖。"),
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
                        .child(field("目录", &self.set_path))
                        .child(
                            div().flex().items_center().gap(theme::GAP_MD).child(
                                primary_btn("import-set", "归档到书", cx).on_click(
                                    cx.listener(|this, _, _, cx| {
                                        let p = this.set_path.read(cx).value().to_string();
                                        cx.emit(ImportEvent::ImportSettings(p));
                                    }),
                                ),
                            ),
                        )
                        // 第二步：归档后才有东西可提取。按钮常在，没材料时点了
                        // 会得到明确提示而不是静默无动作。
                        .child(
                            div().flex().items_center().gap(theme::GAP_MD).child(
                                primary_btn("import-extract", "让总 Agent 提取并分派", cx)
                                    .on_click(cx.listener(|_this, _, _, cx| {
                                        cx.emit(ImportEvent::Extract);
                                    })),
                            ),
                        )
                        .when_true(!result.is_empty(), |d| {
                            let tone = if theme::feedback_is_error(&result) {
                                Tone::Amber
                            } else {
                                Tone::Green
                            };
                            d.child(chip(SharedString::from(result.clone()), tone))
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
impl EventEmitter<ViewEvent> for GraphPanel {}

impl Focusable for GraphPanel {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus_handle.clone()
    }
}

impl Render for GraphPanel {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let lines = self.handle.storylines();
        let events = self.handle.events();
        // 一次读盘拿事实 + 别名（渲染期别读两遍，见 store::memory_ledger）
        let (facts, aliases) = self.handle.memory_ledger();
        let cur = self.handle.chapters().iter().map(|c| c.n).max().unwrap_or(0);
        // 书级推断规则：memory.json 的 inferRules 块可覆盖默认（修仙题材加 师徒/道侣…）
        let rules = crate::memory::InferRules::from_memory_json(
            &self.handle.memory_rules_blob(),
        );
        let inf = crate::memory::infer_with_rules(&facts, &aliases, cur, &rules);

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
                for r in related.iter() {
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
        // 推断区：同师/对称推导、单值冲突、陈旧待复核。只读报告，供作者裁决。
        if !inf.implied.is_empty() || !inf.contradictions.is_empty() || !inf.stale.is_empty() {
            body.push(
                div()
                    .border_t_1()
                    .border_color(theme::LINE())
                    .pt(theme::GAP_GROUP)
                    .child(sec_title("记忆推断"))
                    .into_any_element(),
            );
            for m in &inf.implied {
                body.push(
                    div()
                        .flex()
                        .items_center()
                        .gap(theme::GAP_MD)
                        .pl(theme::PAD_MD)
                        .py(theme::GAP_XS)
                        .child(div().flex_shrink_0().text_size(theme::FONT_LABEL).text_color(theme::GREEN()).child("推导"))
                        .child(
                            div()
                                .flex_1()
                                .min_w_0()
                                .text_size(theme::FONT_META)
                                .text_color(theme::DIM())
                                .child(SharedString::from(format!("{} ·{}· {}（{}）", m.subject, m.predicate, m.object, m.rule))),
                        )
                        .into_any_element(),
                );
            }
            for c in &inf.contradictions {
                // 冲突不是只能"看"——给作者一条把它送进 AI 裁决的通路：
                // 让 AI 读上下文后给一条修正提案（作者采纳才落库，铁律不变）。
                // 事件沿 Delegate 走（与人物卡「补全这张卡」同一条路径），
                // 主 Agent 按 capabilities::routing_hint 派给合适的专属 Agent。
                let (s2, p2, objs2) = (c.subject.clone(), c.predicate.clone(), c.objects.clone());
                let target_desc = format!(
                    "记忆冲突：{} ·{}· 同时指向 [{}]。请读 memory.json 与相关章节正文，判断哪一条对、其余是否为别名/旧版本/录入错误，产出一条 propose_memory 修正提案（删除或更正错误的宾语），引用涉及的事实编号。不要直写台账。",
                    s2, p2, objs2.join(" / ")
                );
                body.push(
                    div()
                        .flex()
                        .items_center()
                        .gap(theme::GAP_MD)
                        .pl(theme::PAD_MD)
                        .py(theme::GAP_XS)
                        .child(div().flex_shrink_0().text_size(theme::FONT_LABEL).text_color(theme::AMBER()).child("冲突"))
                        .child(
                            div()
                                .flex_1()
                                .min_w_0()
                                .text_size(theme::FONT_META)
                                .text_color(theme::AMBER())
                                .child(SharedString::from(format!("{} ·{}· 同时指向 {}（第{}章）", c.subject, c.predicate, c.objects.join(" / "), c.chapters.iter().map(|n| n.to_string()).collect::<Vec<_>>().join(",")))),
                        )
                        .child(
                            Button::new(SharedString::from(format!("mem-fix-{}-{}", c.subject, c.predicate)))
                                .label("裁决")
                                .xsmall()
                                .on_click(cx.listener(move |_, _, _, cx| {
                                    cx.emit(ViewEvent::Delegate {
                                        agent: "reviewer",
                                        key: "review-chapter",
                                        target: Some(target_desc.clone()),
                                    });
                                })),
                        )
                        .into_any_element(),
                );
            }
            for id in &inf.stale {
                body.push(
                    div()
                        .flex()
                        .items_center()
                        .gap(theme::GAP_MD)
                        .pl(theme::PAD_MD)
                        .py(theme::GAP_XS)
                        .child(div().flex_shrink_0().text_size(theme::FONT_LABEL).text_color(theme::FAINT()).child("待复核"))
                        .child(
                            div()
                                .flex_1()
                                .min_w_0()
                                .text_size(theme::FONT_META)
                                .text_color(theme::FAINT())
                                .child(SharedString::from(format!("事实 {} 生效章远落后于当前第{}章", id, cur))),
                        )
                        .into_any_element(),
                );
            }
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

/* ───────────────────────── 改动记录（谁改了什么，能不能退回去） ───────────────────────── */

#[derive(Debug, Clone)]
pub enum JournalEvent {
    /// 撤销一条改动
    Undo(String),
    /// 撤销一整批（一个回合 / 一次采纳）
    UndoBatch(String),
    /// 重新读日志
    Refresh,
    /// 撤回所有越界改动（AI 改了没有代笔授权的章）
    RevertBreaches,
}

pub struct JournalPanel {
    focus_handle: FocusHandle,
    handle: std::rc::Rc<crate::store::ProjectHandle>,
    /// 日志按批次分组展示：一个回合一组，比平铺几十条更容易看懂
    groups: Vec<(String, Vec<crate::journal::Change>)>,
    /// 最近一次操作的结果，显示在顶部
    message: Option<(String, Tone)>,
    /// 当前的越界改动（AI 改了没有代笔授权的章）。空 = 没越界。
    breaches: Vec<crate::journal::Breach>,
}

impl JournalPanel {
    pub fn new(_window: &mut Window, cx: &mut Context<Self>, handle: std::rc::Rc<crate::store::ProjectHandle>) -> Self {
        let mut panel = Self {
            focus_handle: cx.focus_handle(),
            handle,
            groups: Vec::new(),
            message: None,
            breaches: Vec::new(),
        };
        panel.reload(cx);
        panel
    }

    pub fn reload(&mut self, cx: &mut Context<Self>) {
        let journal = crate::journal::Journal::new(&self.handle.root);
        // 批次号是毫秒时间戳，字符串排序即时间序；BTreeMap 反向取最近的
        let groups: Vec<(String, Vec<crate::journal::Change>)> = journal
            .groups(40)
            .into_iter()
            .rev()
            .map(|(k, v)| (k, v))
            .collect();
        self.groups = groups;
        // 越界检查：这是"代笔授权"承诺的另一半——事前 write-policy 拦工具，
        // 事后在这里兜住绕过工具的直写。
        let auth = crate::journal::BookAuth::load(&self.handle.root);
        self.breaches = crate::journal::find_breaches(&self.handle.root, &auth, 200);
        cx.notify();
    }

    /// `tone` 用语义等级而不是内部枚举：调用方在别的模块里，不该依赖视图的颜色类型。
    /// 0 = 正常（绿）/ 1 = 需要注意（琥珀）/ 2 = 失败（红）
    pub fn set_message(&mut self, text: String, tone: u8, cx: &mut Context<Self>) {
        let tone = match tone {
            0 => Tone::Green,
            1 => Tone::Amber,
            _ => Tone::Red,
        };
        self.message = Some((text, tone));
        self.reload(cx);
    }

    /// 批次号 → 人话时间（本地时区的时分）。
    fn batch_label(batch: &str) -> String {
        let ms: u64 = batch.trim_start_matches('b').parse().unwrap_or(0);
        if ms == 0 {
            return batch.to_string();
        }
        let secs = (ms / 1000) as i64;
        // 只显示时分秒，日期由分组顺序体现
        format!(
            "{:02}:{:02}",
            (secs / 3600) % 24,
            (secs / 60) % 60
        )
    }

    fn change_row(&self, c: &crate::journal::Change, cx: &mut Context<Self>) -> gpui_kit::AnyElement {
        let id = c.id.clone();
        // AI 改的用琥珀色标出来：这是作者最需要警觉的一类
        let (tone, who) = match &c.actor {
            crate::journal::Actor::Author => (Tone::Neutral, "作者".to_string()),
            other => (Tone::Amber, other.label()),
        };
        div()
            .flex()
            .items_center()
            .gap(theme::GAP_MD)
            .py(px(3.))
            .child(div().w(px(56.)).flex_shrink_0().child(chip(SharedString::from(who), tone)))
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .text_size(theme::FONT_MICRO)
                    .text_color(theme::TEXT())
                    .text_ellipsis()
                    .child(SharedString::from(c.path.clone())),
            )
            .when_true(!c.note.is_empty(), |d| {
                d.child(
                    div()
                        .max_w(px(200.))
                        .text_size(theme::FONT_MICRO)
                        .text_color(theme::FAINT())
                        .text_ellipsis()
                        .child(SharedString::from(c.note.clone())),
                )
            })
            .child(
                cap_btn("journal-undo", "撤销", cx).on_click(cx.listener(move |this, _, _, cx| {
                    cx.emit(JournalEvent::Undo(id.clone()));
                    let _ = this;
                })),
            )
            .into_any_element()
    }
}

impl EventEmitter<PanelEvent> for JournalPanel {}
impl EventEmitter<JournalEvent> for JournalPanel {}

impl Focusable for JournalPanel {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus_handle.clone()
    }
}

impl Render for JournalPanel {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let mut body: Vec<gpui_kit::AnyElement> = Vec::new();

        if let Some((msg, tone)) = &self.message {
            body.push(chip(SharedString::from(msg.clone()), *tone));
        }

        // 越界改动：AI 改了没有代笔授权的章。这是"代笔授权"承诺的另一半——
        // 事前 write-policy 拦工具，这里是绕过工具直写时的兜底。
        if !self.breaches.is_empty() {
            let chapters: Vec<String> =
                self.breaches.iter().map(|b| format!("第{}章", b.chapter)).collect();
            let n = self.breaches.len();
            body.push(
                div()
                    .flex()
                    .items_center()
                    .gap(theme::GAP_SM)
                    .p(theme::PAD_SM)
                    .rounded(theme::RADIUS_PANEL)
                    .border_1()
                    .border_color(theme::AMBER())
                    .bg(theme::AMBER_SOFT())
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .gap(px(1.))
                            .child(
                                div()
                                    .text_size(theme::FONT_LABEL)
                                    .font_weight(FontWeight::SEMIBOLD)
                                    .text_color(theme::AMBER())
                                    .child(SharedString::from(format!(
                                        "⚠ AI 越界改了 {n} 处正文"
                                    ))),
                            )
                            .child(
                                div()
                                    .text_size(theme::FONT_MICRO)
                                    .text_color(theme::DIM())
                                    .child(SharedString::from(format!(
                                        "{} 没有代笔授权。可以撤回，或到设置里授权这些章。",
                                        chapters.join("、")
                                    ))),
                            ),
                    )
                    .child(div().flex_1())
                    .child(
                        cap_btn("journal-revert-breaches", "撤回越界改动", cx).on_click(
                            cx.listener(|_this, _, _, cx| {
                                cx.emit(JournalEvent::RevertBreaches);
                            }),
                        ),
                    )
                    .into_any_element(),
            );
        }

        if self.groups.is_empty() {
            body.push(
                div()
                    .text_size(theme::FONT_LABEL)
                    .text_color(theme::FAINT())
                    .child("还没有改动记录。作者保存、AI 写入、采纳提案都会在这里留痕。")
                    .into_any_element(),
            );
        }

        for (batch, changes) in &self.groups {
            let batch_id = batch.clone();
            let has_ai = changes
                .iter()
                .any(|c| !matches!(c.actor, crate::journal::Actor::Author));
            let mut rows: Vec<gpui_kit::AnyElement> = Vec::new();
            for c in changes {
                rows.push(self.change_row(c, cx));
            }
            body.push(
                div()
                    .flex()
                    .flex_col()
                    .gap(px(2.))
                    .p(theme::PAD_MD)
                    .bg(theme::PANEL())
                    .border_1()
                    .border_color(if has_ai {
                        theme::AMBER_SOFT()
                    } else {
                        theme::LINE()
                    })
                    .rounded(theme::RADIUS_CARD)
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap(theme::GAP_MD)
                            .child(chip(
                                SharedString::from(Self::batch_label(batch)),
                                Tone::Blue,
                            ))
                            .child(
                                div()
                                    .text_size(theme::FONT_MICRO)
                                    .text_color(theme::FAINT())
                                    .child(SharedString::from(format!(
                                        "{} 处改动{}",
                                        changes.len(),
                                        if has_ai { " · 含 AI 写入" } else { "" }
                                    ))),
                            )
                            .child(div().ml_auto().child(
                                cap_btn("journal-undo-batch", "撤销整批", cx).on_click(
                                    cx.listener(move |_, _, _, cx: &mut Context<JournalPanel>| {
                                        cx.emit(JournalEvent::UndoBatch(batch_id.clone()));
                                    }),
                                ),
                            )),
                    )
                    .children(rows)
                    .into_any_element(),
            );
        }

        panel_root("journal-scroll").child(
            panel_col()
                .child(sec_title("改动记录"))
                .child(
                    div()
                        .text_size(theme::FONT_MICRO)
                        .line_height(relative(theme::LINE_HEIGHT_BODY))
                        .text_color(theme::FAINT())
                        .child("每一处落盘都留了前像。撤销 = 写回那一刻的原内容；如果之后又被改过，会提示冲突而不是覆盖。"),
                )
                .child(
                    div()
                        .flex()
                        .gap(theme::GAP_MD)
                        .child(cap_btn("journal-refresh", "刷新", cx).on_click(
                            cx.listener(|_, _, _, cx: &mut Context<JournalPanel>| {
                                cx.emit(JournalEvent::Refresh)
                            }),
                        )),
                )
                .children(body),
        )
    }
}

impl BasePanel for JournalPanel {
    fn panel_name(&self) -> &'static str {
        "journal-view"
    }
}
impl ComponentPanel for JournalPanel {
    fn title(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div()
            .text_size(theme::FONT_BODY)
            .font_weight(FontWeight::MEDIUM)
            .text_color(theme::TEXT())
            .child("改动记录")
    }
}


/* ───────────────────────── 知识库（导入材料存放 + AI 拆分） ───────────────────────── */

/// 知识库事件。导入材料存放在书的 imports/，这里只做"管理 + 派 AI"。
#[derive(Debug, Clone)]
pub enum KnowledgeEvent {
    /// 导入一个目录的设定文档（原样归档进 imports/）
    ImportSettings(String),
    /// 全量提取：总 Agent 通读 imports/ 并分派各专属 Agent
    Extract,
    /// 单篇拆分：让总 Agent 只处理这一篇
    Split(String),
    /// 删除一篇（作者动作，journal 留痕）
    Delete(String),
}

pub struct KnowledgePanel {
    focus_handle: FocusHandle,
    handle: Rc<ProjectHandle>,
    /// 导入目录输入
    path: Entity<gpui_kit::component::input::InputState>,
    docs: Vec<String>,
    selected: Option<String>,
    preview: String,
    result: String,
}

impl KnowledgePanel {
    pub fn new(window: &mut Window, cx: &mut Context<Self>, handle: Rc<ProjectHandle>) -> Self {
        let path = cx.new(|cx| {
            gpui_kit::component::input::InputState::new(window, cx)
                .placeholder("设定文档目录（.md/.txt）")
        });
        let mut this = Self {
            focus_handle: cx.focus_handle(),
            handle,
            path,
            docs: Vec::new(),
            selected: None,
            preview: String::new(),
            result: String::new(),
        };
        this.refresh();
        this
    }

    pub fn refresh(&mut self) {
        self.docs = self.handle.imported_docs();
        // 选中的被删了就清掉预览
        if let Some(sel) = &self.selected {
            if !self.docs.contains(sel) {
                self.selected = None;
                self.preview = String::new();
            }
        }
        if let Some(sel) = &self.selected {
            self.preview = self
                .handle
                .read_imported_doc(sel)
                .unwrap_or_else(|e| format!("（读不出来：{e}）"));
        }
    }

    pub fn set_result(&mut self, msg: String, cx: &mut Context<Self>) {
        self.result = msg;
        self.refresh();
        cx.notify();
    }

    fn set_selected(&mut self, name: String, cx: &mut Context<Self>) {
        self.selected = Some(name.clone());
        self.preview = self
            .handle
            .read_imported_doc(&name)
            .unwrap_or_else(|e| format!("（读不出来：{e}）"));
        cx.notify();
    }
}

impl EventEmitter<PanelEvent> for KnowledgePanel {}
impl EventEmitter<KnowledgeEvent> for KnowledgePanel {}

impl Focusable for KnowledgePanel {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus_handle.clone()
    }
}

impl Render for KnowledgePanel {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let result = self.result.clone();
        let preview_head: String = self.preview.chars().take(2400).collect();
        let truncated = self.preview.chars().count() > 2400;
        let sel = self.selected.clone();

        // 右栏（选中才出现）。先在 render 顶层构造：when_some 在这个文件里
        // 没进作用域，且闭包里要 emit 事件，早构造早省心。
        let right_pane: Option<gpui_kit::AnyElement> = sel.clone().map(|name| {
            let name2 = name.clone();
            let name3 = name.clone();
            div()
                .flex()
                .items_center()
                .gap(theme::GAP_MD)
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .text_ellipsis()
                        .text_size(theme::FONT_LABEL)
                        .font_weight(FontWeight::SEMIBOLD)
                        .text_color(theme::TEXT())
                        .child(SharedString::from(name.clone())),
                )
                .child(
                    primary_btn("kb-split", "AI 拆分此文档", cx).on_click(cx.listener({
                        let name = name2.clone();
                        move |_this, _, _, cx| {
                            cx.emit(KnowledgeEvent::Split(name.clone()));
                        }
                    })),
                )
                .child(
                    danger_btn(name3.clone(), "删除", cx).on_click(cx.listener(move |_this, _, _, cx| {
                        cx.emit(KnowledgeEvent::Delete(name3.clone()));
                    })),
                )
                .into_any_element()
        });
        let preview_pane: gpui_kit::AnyElement = div()
            .id("kb-preview")
            .flex_1()
            .min_h_0()
            .overflow_y_scroll()
            .p(theme::PAD_MD)
            .rounded(theme::RADIUS_PANEL)
            .bg(theme::PROSE_BG())
            .border_1()
            .border_color(theme::LINE())
            .text_size(theme::FONT_LABEL)
            .line_height(relative(theme::LINE_HEIGHT_BODY))
            .text_color(theme::DIM())
            .child(SharedString::from(if preview_head.is_empty() {
                "（选中一篇查看内容）".to_string()
            } else if truncated {
                format!("{}\n\n……（预览截断，全文 {} 字）", preview_head, self.preview.chars().count())
            } else {
                preview_head.clone()
            }))
            .into_any_element();

        panel_root("knowledge-scroll").child(
            panel_col()
                .child(sec_title("知识库 —— 导入材料的存放与 AI 拆分"))
                .child(
                    div()
                        .text_size(theme::FONT_META)
                        .line_height(relative(theme::LINE_HEIGHT_BODY))
                        .text_color(theme::DIM())
                        .child("这里存放作者导入的原始设定材料。总 Agent 可以逐篇拆分：
                            人物/关系/设定/伏笔/大纲各归各类，全部走提案，材料本身不会被改写。"),
                )
                // 导入 + 全量提取
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
                        .child(field("导入目录", &self.path))
                        .child(
                            div()
                                .flex()
                                .items_center()
                                .gap(theme::GAP_MD)
                                .child(
                                    primary_btn("kb-import", "归档到书", cx).on_click(
                                        cx.listener(|this, _, _, cx| {
                                            let p = this.path.read(cx).value().to_string();
                                            cx.emit(KnowledgeEvent::ImportSettings(p));
                                        }),
                                    ),
                                )
                                .child(
                                    primary_btn("kb-extract", "总 Agent 全量提取", cx).on_click(
                                        cx.listener(|_this, _, _, cx| {
                                            cx.emit(KnowledgeEvent::Extract);
                                        }),
                                    ),
                                ),
                        )
                        .when_true(!result.is_empty(), |d| {
                            let tone = if theme::feedback_is_error(&result) {
                                Tone::Amber
                            } else {
                                Tone::Green
                            };
                            d.child(chip(SharedString::from(result.clone()), tone))
                        }),
                )
                // 主体：左列表 + 右预览
                .child(
                    div()
                        .flex_1()
                        .min_h_0()
                        .flex()
                        .gap(theme::GAP_GROUP)
                        .child(
                            div()
                                .id("kb-doc-list")
                                .w(px(220.))
                                .flex_shrink_0()
                                .min_h_0()
                                .overflow_y_scroll()
                                .flex()
                                .flex_col()
                                .gap(px(2.))
                                .children(self.docs.is_empty().then(|| {
                                    div()
                                        .text_size(theme::FONT_META)
                                        .text_color(theme::FAINT())
                                        .child("还没有导入的材料——上方填目录归档")
                                        .into_any_element()
                                }))
                                .children(self.docs.iter().map(|name| {
                                    let on = sel.as_deref() == Some(name.as_str());
                                    div()
                                        .id(SharedString::from(format!("kb-doc-{name}")))
                                        .px(theme::PAD_SM)
                                        .py(px(4.))
                                        .rounded(px(5.))
                                        .cursor_pointer()
                                        .text_size(theme::FONT_LABEL)
                                        .text_ellipsis()
                                        .overflow_hidden()
                                        .whitespace_nowrap()
                                        .text_color(if on { theme::ACCENT() } else { theme::DIM() })
                                        .bg(if on { theme::ACCENT_SOFT() } else { theme::PANEL2() })
                                        .child(SharedString::from(name.clone()))
                                        .on_click(cx.listener({
                                            let name = name.clone();
                                            move |this, _, _, cx| {
                                                this.set_selected(name.clone(), cx);
                                            }
                                        }))
                                        .into_any_element()
                                })),
                        )
                        .child(
                            div()
                                .flex_1()
                                .min_w_0()
                                .min_h_0()
                                .flex()
                                .flex_col()
                                .gap(theme::GAP_SM)
                                .children(right_pane.into_iter())
                                .child(preview_pane),
                        ),
                ),
        )
    }
}

impl BasePanel for KnowledgePanel {
    fn panel_name(&self) -> &'static str {
        "knowledge-view"
    }
}
impl ComponentPanel for KnowledgePanel {
    fn title(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div()
            .text_size(theme::FONT_BODY)
            .font_weight(FontWeight::MEDIUM)
            .text_color(theme::TEXT())
            .child("知识库")
    }
}

/* ───────────────────────── 章节（综合栏目） ───────────────────────── */

/// 章节页的出口：点卡片进稿纸 / 新建章。页面本身只读磁盘，不改任何东西。
#[derive(Debug, Clone)]
pub enum ChaptersEvent {
    Open(u32),
    NewChapter,
    /// 驾驶舱格子点了：跳到对应页（伏笔/故事线/世界线/排期…）
    Goto(crate::workspace::Mode),
    /// 章节卡「排期板」：切到排期板并定位该章
    JumpDirector(u32),
    /// 章节卡「世界线」：切到世界线并聚焦该章
    JumpWorldline(u32),
}

/// 全书章节总览：目录 + 进度 + 快速进入。
///
/// 为什么从侧栏搬出来：侧栏平铺几百章，既挤掉导航，也看不出"写到哪、还差多少"。
/// 章节本来就是全书的骨架，值得一个自己的页面。
pub struct ChaptersPanel {
    focus_handle: FocusHandle,
    handle: Rc<ProjectHandle>,
    current: u32,
}

impl ChaptersPanel {
    pub fn new(cx: &mut Context<Self>, handle: Rc<ProjectHandle>, current: u32) -> Self {
        Self {
            focus_handle: cx.focus_handle(),
            handle,
            current,
        }
    }

    /// 进入页面时同步当前章（高亮"正在写"那张卡）。
    pub fn refresh(&mut self, current: u32, cx: &mut Context<Self>) {
        self.current = current;
        cx.notify();
    }

    fn card(&self, info: &crate::store::ChapterInfo, cx: &mut Context<Self>) -> gpui_kit::AnyElement {
        let n = info.n;
        let active = n == self.current;
        let written = info.has_prose;
        let title = if info.title.trim().is_empty() {
            "未命名".to_string()
        } else {
            info.title.clone()
        };
        div()
            .id(SharedString::from(format!("ch-card-{n}")))
            .flex()
            .flex_col()
            .gap(theme::GAP_MD)
            .w(px(236.))
            .p(theme::PAD_LG)
            .rounded(theme::RADIUS_CARD)
            .border_1()
            .border_color(if active {
                theme::ACCENT_BORDER()
            } else {
                theme::LINE()
            })
            .bg(if active {
                theme::ACCENT_SOFT()
            } else {
                theme::PANEL()
            })
            .cursor_pointer()
            .hover(|s| s.border_color(theme::ACCENT_BORDER()).bg(theme::PANEL2()))
            .on_click(cx.listener(move |_this, _, _, cx| {
                cx.emit(ChaptersEvent::Open(n));
            }))
            .child(
                div()
                    .flex()
                    .items_center()
                    .justify_between()
                    .gap(theme::GAP_SM)
                    .child(
                        div()
                            .font_family(theme::FAMILY_MONO)
                            .text_size(theme::FONT_LABEL)
                            .text_color(if active {
                                theme::ACCENT()
                            } else {
                                theme::FAINT()
                            })
                            .child(SharedString::from(format!("第 {n:02} 章"))),
                    )
                    .child(chip(
                        if written { "成稿" } else { "未动笔" },
                        if written { Tone::Green } else { Tone::Neutral },
                    )),
            )
            .child(
                div()
                    .min_w_0()
                    .text_ellipsis()
                    .whitespace_nowrap()
                    .text_size(theme::FONT_BODY)
                    .font_weight(FontWeight::MEDIUM)
                    .text_color(if written { theme::TEXT() } else { theme::DIM() })
                    .child(SharedString::from(title)),
            )
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(theme::GAP_SM)
                    .child(
                        div()
                            .text_size(theme::FONT_TAG)
                            .text_color(theme::FAINT())
                            .child(SharedString::from(if written {
                                format!("{} 字", info.words)
                            } else {
                                "还没落笔".to_string()
                            })),
                    )
                    .child(div().flex_1())
                    .child(
                        div()
                            .text_size(theme::FONT_TAG)
                            .text_color(if active { theme::ACCENT() } else { theme::FAINT() })
                            .child(SharedString::from(if active { "正在写" } else { "打开 →" })),
                    ),
            )
            // 卡片尾部：跳排期板 / 跳世界线（纯导航，不改任何台账）。
            // 这两格和驾驶舱格形成三角——章节页 ↔ 排期板 ↔ 世界线双向可达。
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(theme::GAP_XS)
                    .pt(theme::GAP_XS)
                    .border_t_1()
                    .border_color(theme::LINE())
                    .child(
                        Button::new(SharedString::from(format!("ch-dir-{n}")))
                            .label("排期板")
                            .xsmall()
                            .on_click(cx.listener(move |_, _, _, cx| {
                                cx.emit(ChaptersEvent::JumpDirector(n));
                            })),
                    )
                    .child(
                        Button::new(SharedString::from(format!("ch-wld-{n}")))
                            .label("世界线")
                            .xsmall()
                            .on_click(cx.listener(move |_, _, _, cx| {
                                cx.emit(ChaptersEvent::JumpWorldline(n));
                            })),
                    ),
            )
            .into_any_element()
    }

    /// 「现在」驾驶舱：一进书就看到写这一章要接什么、有什么该收了。
    /// 六宫格，全部确定性计算（不调 AI），每一项都是"作者现在就能做的事"。
    fn cockpit(&self, cx: &mut Context<Self>) -> gpui_kit::AnyElement {
        let handle = &self.handle;
        let chapters = handle.chapters();
        let written_max = chapters.iter().filter(|c| c.has_prose).map(|c| c.n).max().unwrap_or(0);
        let next_ch = handle.next_chapter_number();

        // 1. 固化边界 + 2. forecast 末梢（一次读 worldlines）
        let store = crate::worldline::Store::new(&handle.root);
        let lines = store.all().unwrap_or_default();
        let boundary = store.fixed_boundary(&lines);
        let forecast_horizon = crate::worldline::layout::fan(&lines, boundary).horizon;

        // 3. 当前章（写稿目标章）
        let cur = self.current.max(1);

        // 4. 今日字数
        let today = handle.daily_words().last().map(|(_, w)| *w).unwrap_or(0);

        // 5. 静默伏笔（≥8 章没养的）
        let clues = handle.clues_full();
        let mut silent_clues: Vec<(String, u32)> = clues
            .iter()
            .filter(|c| matches!(c.get("status").and_then(|v| v.as_str()), Some("埋下" | "推进中")))
            .filter_map(|c| {
                let planted = c.get("plantedChapter").and_then(|v| v.as_u64())? as u32;
                let last = c
                    .get("lastActionChapter")
                    .and_then(|v| v.as_u64())
                    .map(|n| n as u32)
                    .unwrap_or(planted);
                let silent = next_ch.saturating_sub(last.max(planted));
                if silent >= 8 {
                    Some((c.get("id").and_then(|v| v.as_str()).unwrap_or("").to_string(), silent))
                } else {
                    None
                }
            })
            .collect();
        silent_clues.sort_by_key(|(_, s)| std::cmp::Reverse(*s));

        // 6. 断线故事线
        let broken_lines: usize = handle
            .storylines()
            .iter()
            .filter(|l| l.get("status").and_then(|v| v.as_str()) == Some("断线"))
            .count();

        // 7. 开书体检信号（确定性，与预警条同源不同形——预警条在排期板底部，
        //    体检是进书时的"现在就要注意"提醒，三条封顶不刷屏）
        let mut checkup: Vec<(String, Tone)> = Vec::new();
        let (facts, aliases) = handle.memory_ledger();
        let inf = crate::memory::infer_with_rules(
            &facts,
            &aliases,
            written_max,
            &crate::memory::InferRules::from_memory_json(&handle.memory_rules_blob()),
        );
        if !inf.contradictions.is_empty() {
            checkup.push((
                format!("记忆有 {} 条冲突（图谱里可裁决）", inf.contradictions.len()),
                Tone::Amber,
            ));
        }
        if let Some((id, silent)) = silent_clues.first() {
            checkup.push((
                format!("伏笔 {} 已静默 {} 章（最久）", id, silent),
                Tone::Amber,
            ));
        }
        if broken_lines > 0 {
            checkup.push((
                format!("{broken_lines} 条故事线断线"),
                Tone::Amber,
            ));
        }
        // 事实账本只能证明「最近何时记过该人物的事实」，不能证明他何时出场。
        let alias_map = crate::memory::alias_map(&aliases);
        let mut last_fact: std::collections::HashMap<String, u32> = std::collections::HashMap::new();
        for f in &facts {
            let s = crate::memory::normalize(
                f.get("subject").and_then(|v| v.as_str()).unwrap_or(""),
                &alias_map,
            );
            let ch = f.get("fromChapter").and_then(|v| v.as_u64()).unwrap_or(0) as u32;
            if !s.is_empty() && ch > 0 && ch <= written_max {
                last_fact.entry(s).and_modify(|m| *m = (*m).max(ch)).or_insert(ch);
            }
        }
        let mut dormant: Vec<&str> = last_fact
            .iter()
            .filter(|(_, ch)| written_max.saturating_sub(**ch) >= 20)
            .map(|(name, _)| name.as_str())
            .collect();
        dormant.sort();
        if let Some(name) = dormant.first() {
            checkup.push((
                format!("{} 已 {} 章没有新记忆事实（不代表未出场）", name, written_max.saturating_sub(*last_fact.get(*name).unwrap())),
                Tone::Neutral,
            ));
        }
        checkup.truncate(3);

        // — B4 前瞻：按日均字数推算完本日 —
        let daily = handle.daily_words();
        let recent_avg = if daily.is_empty() {
            0
        } else {
            let recent = daily.iter().rev().take(14).map(|(_, w)| *w).sum::<usize>();
            recent / daily.len().min(14)
        };
        let target_chapters = handle.project_meta().target_chapters;
        let word_target = handle.project_word_target() as u64;
        let total_target = target_chapters as u64 * word_target;
        let total_now: u64 = chapters.iter().map(|c| c.words as u64).sum();
        let eta_label = if recent_avg == 0 || total_target == 0 || total_now >= total_target {
            None
        } else {
            let remain = total_target - total_now;
            let days = (remain as f64 / recent_avg as f64).ceil() as u64;
            Some((days, recent_avg))
        };

        // 七格数据（标签, 主值, 副行, tone, 点击跳哪）
        let mut cells: Vec<gpui_kit::AnyElement> = Vec::new();

        // — 固化边界 —
        cells.push(cockpit_cell(
            "固化到",
            format!("第 {boundary} 章"),
            format!("if 空间从 {} 起", (boundary + 1).max(1)),
            Tone::Blue,
            Some(crate::workspace::Mode::Worldline),
            cx,
        ));

        // — 预测推到 —
        let forecast_label = if forecast_horizon > boundary {
            format!("第 {forecast_horizon} 章")
        } else {
            "还没推".to_string()
        };
        cells.push(cockpit_cell(
            "预测到",
            forecast_label,
            if forecast_horizon > boundary {
                format!("领先正文 {} 章", forecast_horizon - written_max)
            } else {
                "在预言书推一条".to_string()
            },
            if forecast_horizon > boundary { Tone::Green } else { Tone::Neutral },
            Some(crate::workspace::Mode::Oracle),
            cx,
        ));

        // — 正在写 —
        cells.push(cockpit_cell(
            "正在写",
            format!("第 {cur} 章"),
            if cur <= boundary { "补写已固化段".to_string() } else { format!("开放区 · 下一 {next_ch}") },
            Tone::Accent,
            Some(crate::workspace::Mode::Editor),
            cx,
        ));

        // — 今日字数 —
        cells.push(cockpit_cell(
            "今日",
            format!("+{today} 字"),
            if today > 0 { "继续".to_string() } else { "还没动笔".to_string() },
            if today > 0 { Tone::Green } else { Tone::Neutral },
            Some(crate::workspace::Mode::Stats),
            cx,
        ));

        // — 静默伏笔 —
        let clue_main = if silent_clues.is_empty() {
            "都养着".to_string()
        } else {
            format!("{} 条静默", silent_clues.len())
        };
        let clue_sub = if let Some((id, silent)) = silent_clues.first() {
            format!("最久 {} · {} 章没动", id, silent)
        } else {
            "8 章内都养过".to_string()
        };
        cells.push(cockpit_cell(
            "伏笔",
            clue_main,
            clue_sub,
            if silent_clues.is_empty() { Tone::Green } else { Tone::Amber },
            Some(crate::workspace::Mode::Clues),
            cx,
        ));

        // — 断线故事线 —
        cells.push(cockpit_cell(
            "故事线",
            if broken_lines == 0 { "都连着".to_string() } else { format!("{broken_lines} 条断线") },
            if broken_lines == 0 { "健康".to_string() } else { "排期板里看看".to_string() },
            if broken_lines == 0 { Tone::Green } else { Tone::Amber },
            Some(crate::workspace::Mode::Outline),
            cx,
        ));

        // — 预计完本（B4 前瞻指标） —
        cells.push(cockpit_cell(
            "预计完本",
            eta_label.map(|(d, _)| format!("{d} 天")).unwrap_or_else(|| "—".to_string()),
            match eta_label {
                Some((_, avg)) => format!("日均 {avg} 字"),
                None if total_target == 0 => "没设目标".to_string(),
                None if recent_avg == 0 => "还没写作记录".to_string(),
                None => "已达标".to_string(),
            },
            if eta_label.is_some() { Tone::Accent } else { Tone::Neutral },
            Some(crate::workspace::Mode::Stats),
            cx,
        ));

        let mut root = div()
            .flex()
            .flex_col()
            .gap(theme::GAP_SM)
            .w_full();

        // 开书体检条：有信号才显示，三条封顶。琥珀底，比七格更靠上。
        if !checkup.is_empty() {
            root = root.child(
                div()
                    .flex()
                    .flex_wrap()
                    .gap(theme::GAP_MD)
                    .w_full()
                    .px(theme::PAD_LG)
                    .py(theme::GAP_SM)
                    .rounded(theme::RADIUS_BTN)
                    .bg(theme::AMBER_SOFT())
                    .child(
                        div()
                            .text_size(theme::FONT_TAG)
                            .text_color(theme::AMBER())
                            .child("开书体检"),
                    )
                    .children(checkup.into_iter().map(|(msg, tone)| {
                        div()
                            .text_size(theme::FONT_TAG)
                            .text_color(tone.text())
                            .child(SharedString::from(format!("· {msg}")))
                            .into_any_element()
                    })),
            );
        }

        root.child(
            div()
                .flex()
                .flex_wrap()
                .gap(theme::GAP_MD)
                .w_full()
                .p(theme::PAD_LG)
                .rounded(theme::RADIUS_CARD)
                .border_1()
                .border_color(theme::LINE())
                .bg(theme::PANEL2())
                .child(
                    div()
                        .flex()
                        .items_center()
                        .gap(theme::GAP_SM)
                        .w_full()
                        .child(
                            div()
                                .text_size(theme::FONT_TAG)
                                .text_color(theme::FAINT())
                                .child("现在"),
                        ),
                )
                .children(cells),
        )
        .into_any_element()
    }
}

/// 驾驶舱一格：标签 + 主值 + 副行。`Some(mode)` 时可点——点一下切到对应页。
fn cockpit_cell(
    label: &'static str,
    main: String,
    sub: String,
    tone: Tone,
    goto: Option<crate::workspace::Mode>,
    cx: &mut Context<ChaptersPanel>,
) -> gpui_kit::AnyElement {
    let body = || {
        div()
            .flex()
            .flex_col()
            .gap(theme::GAP_XS)
            .child(
                div()
                    .text_size(theme::FONT_LABEL)
                    .text_color(theme::FAINT())
                    .child(label),
            )
            .child(
                div()
                    .text_size(theme::FONT_BODY)
                    .font_weight(FontWeight::MEDIUM)
                    .text_color(tone.text())
                    .child(SharedString::from(main.clone())),
            )
            .child(
                div()
                    .text_size(theme::FONT_TAG)
                    .text_color(theme::DIM())
                    .child(SharedString::from(sub.clone())),
            )
    };
    match goto {
        Some(mode) => div()
            .id(SharedString::from(format!("cockpit-{label}")))
            .flex_1()
            .min_w(px(140.))
            .p(theme::PAD_MD)
            .rounded(theme::RADIUS_BTN)
            .bg(theme::PANEL())
            .cursor_pointer()
            .hover(|s| s.bg(theme::ACCENT_SOFT()))
            .on_click(cx.listener(move |_this, _, _, cx| {
                cx.emit(ChaptersEvent::Goto(mode));
            }))
            .child(body())
            .into_any_element(),
        None => div()
            .flex_1()
            .min_w(px(140.))
            .p(theme::PAD_MD)
            .rounded(theme::RADIUS_BTN)
            .bg(theme::PANEL())
            .child(body())
            .into_any_element(),
    }
}

impl EventEmitter<PanelEvent> for ChaptersPanel {}
impl EventEmitter<ChaptersEvent> for ChaptersPanel {}

impl Focusable for ChaptersPanel {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus_handle.clone()
    }
}

impl Render for ChaptersPanel {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let chapters = self.handle.chapters();
        let written = chapters.iter().filter(|c| c.has_prose).count();
        let total_words: usize = chapters.iter().map(|c| c.words).sum();
        let target_chapters = self.handle.project_meta().target_chapters;
        let word_target = self.handle.project_word_target() as u64;
        let target_words = if target_chapters > 0 && word_target > 0 {
            target_chapters as u64 * word_target
        } else {
            0
        };
        let today = self
            .handle
            .daily_words()
            .last()
            .map(|(_, w)| *w)
            .unwrap_or(0);

        let mut cards: Vec<gpui_kit::AnyElement> = Vec::new();
        for info in &chapters {
            cards.push(self.card(info, cx));
        }

        let progress = |label: String, now: u64, goal: u64, tone: Tone| -> gpui_kit::AnyElement {
            let frac = if goal == 0 {
                0.
            } else {
                (now as f32 / goal as f32).clamp(0., 1.)
            };
            div()
                .flex()
                .flex_col()
                .gap(theme::GAP_SM)
                .flex_1()
                .min_w(px(220.))
                .child(
                    div()
                        .flex()
                        .items_center()
                        .justify_between()
                        .gap(theme::GAP_SM)
                        .child(
                            div()
                                .text_size(theme::FONT_TAG)
                                .text_color(theme::FAINT())
                                .child(SharedString::from(label)),
                        )
                        .child(
                            div()
                                .font_family(theme::FAMILY_MONO)
                                .text_size(theme::FONT_TAG)
                                .text_color(theme::DIM())
                                .child(SharedString::from(if goal == 0 {
                                    format!("{now}")
                                } else {
                                    format!("{now} / {goal}")
                                })),
                        ),
                )
                .child(
                    div()
                        .h(px(6.))
                        .w_full()
                        .rounded(theme::RADIUS_TAG)
                        .bg(theme::BAR_TRACK())
                        .overflow_hidden()
                        .child(
                            div()
                                .h_full()
                                .w(relative(frac))
                                .rounded(theme::RADIUS_TAG)
                                .bg(tone.text()),
                        ),
                )
                .into_any_element()
        };

        let mut stats: Vec<gpui_kit::AnyElement> = vec![
            chip(format!("共 {} 章", chapters.len()), Tone::Neutral),
            chip(format!("成稿 {written}"), Tone::Green),
            chip(format!("总字数 {total_words}"), Tone::Accent),
        ];
        if today > 0 {
            stats.push(chip(format!("今日 +{today}"), Tone::Blue));
        }
        if target_chapters > 0 {
            stats.push(chip(
                format!("目标 {target_chapters} 章"),
                if written >= target_chapters as usize {
                    Tone::Green
                } else {
                    Tone::Amber
                },
            ));
        }

        let mut body = panel_col()
            .child(
                div()
                    .flex()
                    .items_end()
                    .justify_between()
                    .gap(theme::GAP_GROUP)
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .gap(theme::GAP_XS)
                            .child(sec_title("章节"))
                            .child(
                                div()
                                    .text_size(theme::FONT_META)
                                    .text_color(theme::DIM())
                                    .child("全书目录与进度；点任意一章进入稿纸"),
                            ),
                    )
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap(theme::GAP_SM)
                            .child(
                                cap_btn("chapters-new", "新建章", cx).on_click(cx.listener(
                                    |_this, _, _, cx| cx.emit(ChaptersEvent::NewChapter),
                                )),
                            )
                            .child(
                                cap_btn("chapters-current", "继续写当前章", cx).on_click(
                                    cx.listener(|this, _, _, cx| {
                                        cx.emit(ChaptersEvent::Open(this.current.max(1)));
                                    }),
                                ),
                            ),
                    ),
            )
            .child(
                div()
                    .flex()
                    .flex_wrap()
                    .items_center()
                    .gap(theme::GAP_MD)
                    .children(stats),
            );

        // 两条进度：章数与字数。目标没设就只显示当前值，不画假进度。
        let mut bars: Vec<gpui_kit::AnyElement> = Vec::new();
        if target_chapters > 0 {
            bars.push(progress(
                "章数进度".to_string(),
                written as u64,
                target_chapters as u64,
                Tone::Green,
            ));
        }
        if target_words > 0 {
            bars.push(progress(
                "字数进度".to_string(),
                total_words as u64,
                target_words,
                Tone::Accent,
            ));
        }
        if !bars.is_empty() {
            body = body.child(
                div()
                    .flex()
                    .flex_wrap()
                    .gap(theme::GAP_GROUP_LG)
                    .w_full()
                    .children(bars),
            );
        }

        // —— 「现在」驾驶舱：一进书就看到这章接什么、有什么该收了 ——
        // 全是确定性计算，不调 AI；每一项都是"作者现在就能做的事"。
        body = body.child(self.cockpit(cx));

        if chapters.is_empty() {
            body = body.child(
                div()
                    .flex()
                    .flex_col()
                    .items_center()
                    .gap(theme::GAP_MD)
                    .w_full()
                    .py(px(48.))
                    .rounded(theme::RADIUS_CARD)
                    .border_1()
                    .border_color(theme::LINE())
                    .bg(theme::PANEL())
                    .child(
                        div()
                            .text_size(theme::FONT_BODY)
                            .text_color(theme::DIM())
                            .child("还没有章节"),
                    )
                    .child(
                        div()
                            .text_size(theme::FONT_TAG)
                            .text_color(theme::FAINT())
                            .child("点右上「新建章」开第一章——建好就能直接落笔"),
                    ),
            );
        } else {
            body = body.child(
                div()
                    .flex()
                    .flex_wrap()
                    .gap(theme::GAP_GROUP)
                    .w_full()
                    .children(cards),
            );
        }

        panel_root("chapters-scroll").child(body)
    }
}

impl BasePanel for ChaptersPanel {
    fn panel_name(&self) -> &'static str {
        "chapters-view"
    }
}
impl ComponentPanel for ChaptersPanel {
    fn title(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div()
            .text_size(theme::FONT_BODY)
            .font_weight(FontWeight::MEDIUM)
            .text_color(theme::TEXT())
            .child("章节")
    }
}
