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
    /// 当前选中
    selected: Option<String>,
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
        };
        panel.reload(window, cx);
        panel
    }

    /// 从磁盘重读，并把选中项回填到编辑框。
    pub fn reload(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.people = crate::characters::Roster::new(&self.handle.root).all();
        if self.selected.is_none() && !self.people.is_empty() {
            self.selected = Some(self.people[0].id.clone());
        }
        self.fill_editors(window, cx);
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

    /// 选中某人（并回填编辑框）。
    pub fn select(&mut self, id: String, window: &mut Window, cx: &mut Context<Self>) {
        self.selected = Some(id);
        self.fill_editors(window, cx);
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

        // —— 左：卡簿。每个角色一张卡位（含路人甲）——
        //
        // 排序：主要角色按出场/创建序在前，路人在后。不按字母排——
        // 作者找人是按"我记得有个演过路人的"，不是按拼音。
        let mut ordered: Vec<&crate::characters::Character> =
            self.people.iter().filter(|c| !c.minor).collect();
        let minors: Vec<&crate::characters::Character> =
            self.people.iter().filter(|c| c.minor).collect();
        let minor_count = minors.len();
        ordered.extend(minors);

        let mut book = div()
            .w(px(188.))
            .flex_shrink_0()
            .flex()
            .flex_col()
            .gap(px(2.));
        let mut shown_minor_head = false;
        for c in &ordered {
            if c.minor && !shown_minor_head {
                shown_minor_head = true;
                book = book.child(
                    div()
                        .pt(theme::PAD_SM)
                        .pb(px(2.))
                        .text_size(theme::FONT_MICRO)
                        .text_color(theme::FAINT())
                        .child(SharedString::from(format!("过场 / 路人 · {minor_count}"))),
                );
            }
            let on = selected.as_deref() == Some(c.id.as_str());
            let id = c.id.clone();
            // 卡位：顶部一条题签色带 + 名字，下面一行定位/别名。
            // 路人卡降权：题签变淡、不描边，但仍是卡（能点开、能写、能翻）。
            let accent = if c.minor { theme::LINE_SOFT() } else { theme::ACCENT() };
            let mut row = div()
                .id(SharedString::from(format!("char-card-{}", c.id)))
                .flex()
                .flex_col()
                .rounded(theme::RADIUS_BTN_SM)
                .overflow_hidden()
                .cursor_pointer()
                .border_1()
                .border_color(if on { theme::ACCENT_BORDER() } else { theme::PANEL() })
                .bg(if on { theme::ACCENT_SOFT() } else { theme::PANEL() })
                .hover(|d| d.border_color(theme::ACCENT_BORDER()))
                // 题签色带：卡牌的"头"
                .child(div().h(px(2.)).w_full().bg(accent))
                .child(
                    div()
                        .px(theme::PAD_SM)
                        .py(px(4.))
                        .flex()
                        .flex_col()
                        .child(
                            div()
                                .text_size(theme::FONT_LABEL)
                                .font_family(theme::FAMILY_SERIF)
                                .text_color(if on { theme::ACCENT() } else { theme::TEXT() })
                                .text_ellipsis()
                                .child(SharedString::from(c.name.clone())),
                        )
                        .children({
                            let sub = if !c.role.trim().is_empty() {
                                c.role.clone()
                            } else if !c.aliases.is_empty() {
                                c.aliases.join("、")
                            } else {
                                String::new()
                            };
                            if sub.trim().is_empty() {
                                None
                            } else {
                                Some(
                                    div()
                                        .text_size(theme::FONT_MICRO)
                                        .text_color(theme::FAINT())
                                        .text_ellipsis()
                                        .child(SharedString::from(sub)),
                                )
                            }
                        }),
                );
            // 信息完整度：路人的卡常常是空的，用小点提示"还缺多少"
            let filled = [
                !c.appearance.trim().is_empty(),
                !c.personality.trim().is_empty(),
                !c.motivation.trim().is_empty(),
                !c.voice.trim().is_empty(),
                !c.core_story.trim().is_empty(),
            ]
            .iter()
            .filter(|b| **b)
            .count();
            row = row.child(
                div()
                    .px(theme::PAD_SM)
                    .pb(px(3.))
                    .flex()
                    .gap(px(2.))
                    .children((0..5).map(move |i| {
                        div()
                            .w(px(9.))
                            .h(px(2.))
                            .rounded(px(1.))
                            .bg(if i < filled { accent } else { theme::LINE_SOFT() })
                    })),
            );
            book = book.child(row.on_click(cx.listener(move |this, _, window, cx| {
                this.select(id.clone(), window, cx);
            })));
        }

        // —— 右：卡面 ——
        let detail: gpui_kit::AnyElement = match self.current().cloned() {
            None => empty_hint(
                "还没有人物卡",
                "点左侧「新建」建一张；或从已有记忆迁移（别名 + 事实 → 人物卡）。\
                 路人也要建卡——信息可以简略，但别让他无名。",
            )
            .into_any_element(),
            Some(c) => {
                let cid = c.id.clone();
                let flipped = self.flipped;
                let aliases = c.aliases.clone();
                let arc = c.arc.clone();
                let locked = c.locked_fields.clone();
                let minor = c.minor;
                let status_ch = c.status_chapter;

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
                    .child(
                        div()
                            .flex()
                            .gap(theme::GAP_MD)
                            .child(div().flex_1().child(field("本名", &self.ed_name)))
                            .child(div().w(px(150.)).child(field("定位", &self.ed_role)))
                            .child(div().w(px(150.)).child(field("阵营", &self.ed_faction))),
                    )
                    .child(
                        div()
                            .flex()
                            .gap(theme::GAP_MD)
                            .child(div().flex_1().child(field("外貌", &self.ed_appearance)))
                            .child(div().w(px(150.)).child(field("年龄", &self.ed_age))),
                    )
                    .child(field("性格", &self.ed_personality))
                    .child(field("动机 · 想要什么", &self.ed_motivation))
                    .child(field("背景", &self.ed_background))
                    .child(field("口吻 · 写对白时查这条", &self.ed_voice))
                    .child(
                        div()
                            .flex()
                            .gap(theme::GAP_MD)
                            .child(div().flex_1().child(field("当前状态", &self.ed_status)))
                            .child(div().w(px(150.)).child(field("状态自第几章", &self.ed_status_ch))),
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
                    // 过场 / 路人：不影响能不能写，只影响视觉权重与"信息不全是正常的"
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
                                            match roster.upsert(c) {
                                                Ok(()) => {
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

                // 卡壳：外描边 + 顶部题签（名字）+ 翻面按钮
                let header = div()
                    .flex()
                    .items_center()
                    .gap(theme::GAP_MD)
                    .pb(theme::GAP_SM)
                    .border_b_1()
                    .border_color(theme::LINE())
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .gap(px(1.))
                            .child(
                                div()
                                    .text_size(theme::FONT_TITLE)
                                    .font_family(theme::FAMILY_SERIF)
                                    .text_color(if minor { theme::DIM() } else { theme::TEXT() })
                                    .child(SharedString::from(c.name.clone())),
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
                                            .child(SharedString::from(meta.join(" · "))),
                                    )
                                }
                            }),
                    )
                    .child(div().flex_1())
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
                    .child({
                        let cid = cid.clone();
                        cap_btn("char-delete", "删除", cx).on_click(cx.listener(
                            move |_, _, _, cx| cx.emit(CharacterEvent::Delete(cid.clone())),
                        ))
                    })
                    .child(
                        primary_btn("char-save", "保存", cx).on_click(cx.listener(
                            |this, _, _, cx| {
                                if let Some(c) = this.draft(cx) {
                                    cx.emit(CharacterEvent::Save(Box::new(c)));
                                }
                            },
                        )),
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

                div()
                    .flex()
                    .flex_col()
                    .gap(theme::GAP_MD)
                    .child(header)
                    .child(if flipped { back } else { front })
                    .when_true(status_ch > 0 && !flipped, |d| {
                        d.child(
                            div()
                                .text_size(theme::FONT_MICRO)
                                .text_color(theme::FAINT())
                                .child(SharedString::from(format!("状态自第{status_ch}章起"))),
                        )
                    })
                    .into_any_element()
            }
        };

        // —— 合并：选另一个人并进来 ——
        let others: Vec<(String, String)> = self
            .people
            .iter()
            .filter(|c| Some(c.id.as_str()) != selected.as_deref())
            .map(|c| (c.id.clone(), c.name.clone()))
            .collect();

        panel_root("characters-scroll").child(
            panel_col()
                .child(
                    div()
                        .flex()
                        .items_baseline()
                        .gap(theme::GAP_MD)
                        .child(sec_title("人物卡"))
                        .child(
                            div()
                                .flex_1()
                                .text_size(theme::FONT_MICRO)
                                .text_color(theme::FAINT())
                                .child(SharedString::from(format!(
                                    "{} 张卡（含 {} 位过场）· 正面写人，背面写他在故事里的份量",
                                    self.people.len(),
                                    minor_count
                                ))),
                        ),
                )
                .when_true(message.is_some(), |d| {
                    d.child(chip(
                        SharedString::from(message.clone().unwrap_or_default()),
                        Tone::Green,
                    ))
                })
                // 模块一键能力：由能力表驱动，不在这里写死按钮。
                // 加一个 Agent 能力只需改 capabilities.rs，页面自动出现。
                .children(crate::capabilities::agent("character").map(|a| {
                    let mut bar = div().flex().flex_wrap().gap(theme::GAP_SM).items_center();
                    bar = bar.child(
                        div()
                            .text_size(theme::FONT_MICRO)
                            .text_color(theme::FAINT())
                            .child(SharedString::from(a.duty)),
                    );
                    for cap in a.bulk {
                        bar = bar.child(
                            cap_btn(cap.key, cap.label, cx).on_click(cx.listener(
                                move |_this, _, _, cx| {
                                    cx.emit(CharacterEvent::Delegate {
                                        key: cap.key,
                                        target: None,
                                    });
                                },
                            )),
                        );
                    }
                    bar.into_any_element()
                }))
                .child(
                    div()
                        .flex()
                        .gap(theme::GAP_LG)
                        .items_start()
                        .child(
                            div()
                                .flex()
                                .flex_col()
                                .gap(theme::GAP_SM)
                                .child(book)
                                .child(
                                    div()
                                        .flex()
                                        .gap(theme::GAP_SM)
                                        .items_center()
                                        .child(div().flex_1().child(field("", &self.ed_new)))
                                        .child(cap_btn("char-create", "新建", cx).on_click(
                                            cx.listener(|this, _, _, cx| {
                                                let v = this.ed_new.read(cx).value().to_string();
                                                if !v.trim().is_empty() {
                                                    cx.emit(CharacterEvent::Create(v));
                                                }
                                            }),
                                        )),
                                )
                                .child(cap_btn("char-migrate", "从记忆迁移", cx).on_click(
                                    cx.listener(|_, _, _, cx| cx.emit(CharacterEvent::Migrate)),
                                )),
                        )
                        .child(
                            div()
                                .flex_1()
                                .min_w_0()
                                .p(theme::PAD_LG)
                                .bg(theme::PANEL())
                                .border_1()
                                .border_color(theme::LINE())
                                .rounded(theme::RADIUS_CARD)
                                .child(detail),
                        ),
                )
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
                    d.child(sec_title("合并人物"))
                        .child(
                            div()
                                .text_size(theme::FONT_MICRO)
                                .text_color(theme::FAINT())
                                .child(SharedString::from(format!(
                                    "把另一位并进「{into_name}」：别名并集、空格子补齐、已有的值保留。"
                                ))),
                        )
                        .child(rows)
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
            theme_sel: if cfg.theme.trim() == "xuan" { "xuan".into() } else { "ink".into() },
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
                                .child("保存后立即生效，全部窗口同步切换。"),
                        )
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
                            d.child(chip(SharedString::from(result.clone()), Tone::Green))
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
                            d.child(chip(SharedString::from(result.clone()), Tone::Green))
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
            .into_any_element()
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
