//! 收件箱面板（右侧 Dock 第二页签）：待裁决提案就地过目、采纳/拒收。
//!
//! 作者主权结构（2026-11-10）——每张提案卡固定四块，信息层次不靠文案修辞：
//! 1. **依据**：优先读工具侧 `sourceRefs`（条目级字符串数组，见 novel-pack 工具 schema），
//!    无则从既有引用字段推导（报告 location/evidence、去AI味 original/reason、埋点 clueID…）；
//!    两者都没有就显式标「未提供依据」——自述（note）单列，不当依据。
//! 2. **内容预览**：逐条列出 payload 里真正会被写入的东西（beats/findings/original→replacement/
//!    草稿正文…），默认折叠、可展开；不再只给一个数量。
//! 3. **落点 · 回滚**：这一单采纳会改哪里、采纳前有没有快照/备份，直接写在卡上。
//! 4. **动作行**：采纳 / 拒收（按钮名不变），右列 `kind` 台账码。
//!
//! 底部常驻「决策账本」：等作者拍板 N 项 / 已决 M 项 / 采纳率；滚动区内保留已决历史
//! （状态、时间、拒收理由、payload 预览），已处理提案不再只剩一个计数。
//!
//! 视觉全部取自 `crate::theme` 令牌（无裸 `rgb(0x…)`，间距/圆角/字号不混档）：
//! - 层次（规则 4）：滚动底 = `BG()` 墨底，卡片 = `PANEL()` + `LINE()` 1px；
//!   卡内三块（依据/预览/落点）= `PANEL2()` + `LINE()` 的次级面。
//! - 状态语言（规则 6）：待裁决 = `ACCENT_SOFT` + `ACCENT_BORDER` chip；已采纳 = `GREEN_SOFT`；
//!   已拒绝 = `RED_SOFT`；「未提供依据」= `AMBER_SOFT` + `AMBER()` 警示（不夸大、不静默）。
//! - 文字三档（规则 8）：标题/引用条目 `TEXT()`、摘要/说明 `DIM()`、标签/时间 `FAINT()`。
//! - 台账码（规则 10）：`kind: xxx`、快照时间、提案时间用 `FAMILY_MONO` + `FONT_LABEL`。

use std::collections::BTreeSet;
use std::rc::Rc;

use gpui_kit::component::dock::{BasePanel, Panel as ComponentPanel, PanelEvent};
use gpui_kit::component::empty::{Empty, EmptyContent, EmptyDescription, EmptyHeader, EmptyTitle};
use gpui_kit::component::input::{Input, InputState};
use gpui_kit::component::tag::Tag;
use gpui_kit::{
    AnyElement, App, AppContext as _, ClickEvent, Context, Entity, EventEmitter, FocusHandle,
    Focusable, FontWeight, Hsla, InteractiveElement, IntoElement, ParentElement, Pixels, Render,
    Rgba, SharedString, StatefulInteractiveElement, Styled, Window, div, px, relative,
};
use serde_json::Value;

use crate::store::{chapter_from_detail, chapter_value, ProjectHandle, Proposal};
use crate::theme;

/// 折叠时每卡显示的最大条目数。
const PREVIEW_ITEMS_COLLAPSED: usize = 2;
/// 展开时每卡显示的最大条目数（防 100 条伏笔把 Dock 撑爆）。
const PREVIEW_ITEMS_EXPANDED: usize = 40;
/// 折叠时单条文本的最大字数。
const PREVIEW_CHARS_COLLAPSED: usize = 160;
/// 展开时单条文本的最大字数。
const PREVIEW_CHARS_EXPANDED: usize = 600;
/// 草稿/设定修订：折叠时的正文字数。
const LONG_TEXT_COLLAPSED: usize = 240;
/// 草稿/设定修订：展开时的正文字数（仍留上限，避免异常 payload）。
const LONG_TEXT_EXPANDED: usize = 4000;
/// 依据区一次最多列出的引用行数。
const BASIS_ROWS_SHOWN: usize = 6;
/// 已决账本一次最多列出的历史条数。
const HISTORY_ROWS: usize = 12;

#[derive(Debug, Clone)]
pub enum InboxEvent {
    Accept(String),
    Reject(String, String),
    Refresh,
}

pub struct InboxPanel {
    focus_handle: FocusHandle,
    handle: Rc<ProjectHandle>,
    proposals: Vec<Proposal>,
    reason: Entity<InputState>,
    /// 已展开的提案 id（待裁决卡与已决历史共用，默认全折叠）。
    expanded: BTreeSet<String>,
}

impl InboxPanel {
    pub fn new(window: &mut Window, cx: &mut Context<Self>, handle: Rc<ProjectHandle>) -> Self {
        let reason = cx.new(|cx| InputState::new(window, cx).placeholder("拒收理由（回灌给 AI）…"));
        let proposals = handle.proposals();
        Self {
            focus_handle: cx.focus_handle(),
            handle,
            proposals,
            reason,
            expanded: BTreeSet::new(),
        }
    }

    pub fn refresh(&mut self, cx: &mut Context<Self>) {
        self.proposals = self.handle.proposals();
        cx.notify();
    }

    fn toggle_expanded(&mut self, id: String, cx: &mut Context<Self>) {
        if !self.expanded.remove(&id) {
            self.expanded.insert(id);
        }
        cx.notify();
    }
}

impl EventEmitter<PanelEvent> for InboxPanel {}
impl EventEmitter<InboxEvent> for InboxPanel {}

impl Focusable for InboxPanel {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus_handle.clone()
    }
}

/// 按钮语气（统一视觉规则 5、6）：主能力 / 破坏性 / 中性次级。
#[derive(Clone, Copy)]
enum Tone {
    /// 朱砂实心 + 白字（采纳）。
    Primary,
    /// 红软底 + 红字（拒收，破坏性）。
    Destructive,
    /// PANEL2 底 + LINE 描边 + DIM 字（刷新/展开）。
    Neutral,
}

/// 语义 chip 语气（规则 6）：可行动 = 朱砂，纯计数 = 中性，缺依据 = 琥珀警示。
#[derive(Clone, Copy)]
enum ChipTone {
    Action,
    Neutral,
    Warn,
}

/// hover「底色提亮一档」：在原色上叠 7% 白（`CHIP_NEUTRAL`），避免新增裸色值。
fn lifted(base: Rgba) -> Hsla {
    let base: Hsla = base.into();
    let veil: Hsla = theme::CHIP_NEUTRAL().into();
    base.blend(veil)
}

/// 按下「加深一档」：在原色上叠 22% 墨底（`BG`），给按压可感知反馈（规则 7）。
fn pressed(base: Rgba) -> Hsla {
    let base: Hsla = base.into();
    let ink: Hsla = theme::BG().into();
    base.blend(ink.opacity(0.22))
}

/// 面板内的紧凑动作钮：全部用 token 自绘，绕开 gpui-component 库主题（近白/近黑）。
fn action_button(
    id: impl Into<SharedString>,
    label: &'static str,
    tone: Tone,
    on_click: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
) -> impl IntoElement {
    // `ElementId` 只接受 `SharedString`/`&'static str`（无 `From<String>`），先落地成 SharedString。
    let id: SharedString = id.into();
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
        Tone::Destructive => (
            // 静止：红软底 + 中性 LINE 描边；hover 把描边转成语义红（破坏性动作不套朱砂强调），
            // 底色提亮一档，按压再加深一档（规则 6、7）。
            theme::RED_SOFT().into(),
            theme::LINE().into(),
            theme::RED().into(),
            lifted(theme::RED_SOFT()),
            theme::RED().into(),
            theme::RED().into(),
            pressed(theme::RED_SOFT()),
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
            style.bg(hover_bg).border_color(hover_border).text_color(hover_fg)
        })
        .active(move |style| style.bg(active_bg))
        .on_click(on_click)
        .child(label)
}

/// 计数 chip = kit Tag（primary=朱砂可行动，secondary=中性计数）。
fn stat_chip(label: &'static str, value: usize, tone: ChipTone) -> impl IntoElement {
    let t = match tone {
        ChipTone::Action => Tag::primary(),
        ChipTone::Neutral | ChipTone::Warn => Tag::secondary(),
    };
    t.outline()
        .child(SharedString::from(label.to_string()))
        .child(div().font_weight(FontWeight::SEMIBOLD).child(value.to_string()))
}

/// 带完整文案的语义 chip（计数行 / 缺依据警示用）。
fn text_chip(text: String, tone: ChipTone) -> impl IntoElement {
    let (fg, bg, border) = match tone {
        ChipTone::Action => (theme::ACCENT(), theme::ACCENT_SOFT(), theme::ACCENT_BORDER()),
        ChipTone::Warn => (theme::AMBER(), theme::AMBER_SOFT(), theme::LINE()),
        ChipTone::Neutral => (theme::DIM(), theme::CHIP_NEUTRAL(), theme::LINE()),
    };
    div()
        .flex_shrink_0()
        .px(theme::PAD_SM)
        .py(theme::PAD_XS)
        .rounded(theme::RADIUS_CHIP)
        .bg(bg)
        .border_1()
        .border_color(border)
        .font_family(theme::FAMILY_UI)
        .text_size(theme::FONT_TAG)
        .font_weight(FontWeight::SEMIBOLD)
        .text_color(fg)
        .child(SharedString::from(text))
}

/// 块标题（依据 / 内容预览 / 落点 · 回滚）。
fn section_label(text: &'static str) -> impl IntoElement {
    div()
        .font_family(theme::FAMILY_UI)
        .text_size(theme::FONT_TAG)
        .font_weight(FontWeight::SEMIBOLD)
        .text_color(theme::FAINT())
        .child(text)
}

/// 左标签 + 右内容的单行（依据/落点/时间都走这一套，避免每处自排）。
fn field_line(label: &'static str, value: String, color: Rgba) -> AnyElement {
    div()
        .w_full()
        .flex()
        .items_start()
        .gap(theme::GAP_SM)
        .child(
            div()
                .w(px(30.))
                .flex_shrink_0()
                .font_family(theme::FAMILY_UI)
                .text_size(theme::FONT_TAG)
                .text_color(theme::FAINT())
                .child(label),
        )
        .child(
            div()
                .flex_1()
                .min_w_0()
                .text_size(theme::FONT_TAG)
                .line_height(relative(theme::LINE_HEIGHT_BODY))
                .text_color(color)
                .child(SharedString::from(value)),
        )
        .into_any_element()
}

/// 多行文本：按 `\n` 拆成独立行（草稿预览不能挤成一段）。
fn text_lines(text: &str, size: Pixels, color: Rgba) -> AnyElement {
    let mut col = div()
        .w_full()
        .flex()
        .flex_col()
        .gap(theme::GAP_XS)
        .text_size(size)
        .line_height(relative(theme::LINE_HEIGHT_BODY))
        .text_color(color);
    for line in text.split('\n') {
        let line = if line.trim().is_empty() { " ".to_string() } else { line.to_string() };
        col = col.child(div().w_full().child(SharedString::from(line)));
    }
    col.into_any_element()
}

/// 收入依据区的一行：来源类别 + 编号/标题 + 可核对摘录。
#[derive(Debug, Clone)]
struct BasisItem {
    kind: &'static str,
    label: String,
    excerpt: String,
}

/// 一条 payload 条目的可预览抽象（已截断，直接可渲染）。
#[derive(Debug, Clone)]
struct PayloadPreview {
    lines: Vec<String>,
    total: usize,
    unit: &'static str,
    clipped: bool,
    total_chars: Option<usize>,
}

impl PayloadPreview {
    fn new(unit: &'static str) -> Self {
        Self { lines: Vec::new(), total: 0, unit, clipped: false, total_chars: None }
    }

    fn push(&mut self, line: String) {
        self.lines.push(line);
    }

    fn has_content(&self) -> bool {
        !self.lines.is_empty()
    }

    /// 折叠态是否还有没显示出来的内容。
    fn expandable(&self) -> bool {
        self.clipped || self.total > self.lines.len()
    }

    fn footnote(&self, expanded: bool) -> String {
        if let Some(n) = self.total_chars {
            let shown: usize = self.lines.iter().map(|l| l.chars().count()).sum();
            return if self.clipped {
                format!("共 {n} 字 · 已截断（显示前 {shown} 字）")
            } else {
                format!("共 {n} 字")
            };
        }
        if self.total == 0 {
            return "（无可预览条目）".to_string();
        }
        let hidden = self.total.saturating_sub(self.lines.len());
        let clipped_note = if self.clipped { " · 已截断" } else { "" };
        if hidden == 0 {
            format!("共 {} {}{clipped_note}", self.total, self.unit)
        } else if expanded {
            format!(
                "共 {} {} · 仅显示前 {} {}{clipped_note}",
                self.total,
                self.unit,
                self.lines.len(),
                self.unit
            )
        } else {
            format!(
                "共 {} {} · 折叠显示前 {} {}{clipped_note}",
                self.total,
                self.unit,
                self.lines.len(),
                self.unit
            )
        }
    }
}

// ───────────────────────── payload 读取小工具 ─────────────────────────

fn field<'a>(v: &'a Value, key: &str) -> &'a str {
    v.get(key).and_then(|x| x.as_str()).unwrap_or("")
}

fn obj_field<'a>(o: &'a serde_json::Map<String, Value>, key: &str) -> &'a str {
    o.get(key).and_then(|x| x.as_str()).unwrap_or("")
}

/// 取数组：支持直接数组与旧版（Swift 时代）`{"_0":[…],"_1":[…]}` 索引包裹。
fn arr<'a>(v: &'a Value, key: &str) -> Vec<&'a Value> {
    match v.get(key) {
        Some(Value::Array(a)) => a.iter().collect(),
        Some(Value::Object(o)) => {
            let mut keys: Vec<&String> = o.keys().collect();
            keys.sort_by_key(|k| k.trim_start_matches('_').parse::<usize>().unwrap_or(usize::MAX));
            let mut out = Vec::new();
            for k in keys {
                if let Some(inner) = o.get(k).and_then(|x| x.as_array()) {
                    out.extend(inner.iter());
                }
            }
            out
        }
        _ => Vec::new(),
    }
}

/// 旧版（Swift）payload 的数组键不同：fixes→clueFixes、updates→outlineUpdates。
/// 按优先顺序取第一个非空数组，兼容包裹解码。
fn arr_any<'a>(v: &'a Value, keys: &[&str]) -> Vec<&'a Value> {
    for k in keys {
        let a = arr(v, k);
        if !a.is_empty() {
            return a;
        }
    }
    Vec::new()
}

/// 数组条目可能是字符串，也可能是带 text/summary/title 的对象（dossier 的 advanced/turns）。
fn value_text(v: &Value) -> Option<&str> {
    v.as_str()
        .or_else(|| v.get("text").and_then(|x| x.as_str()))
        .or_else(|| v.get("summary").and_then(|x| x.as_str()))
        .or_else(|| v.get("title").and_then(|x| x.as_str()))
}

fn chapter_of(v: &Value) -> u64 {
    v.get("chapter").and_then(|x| x.as_u64()).unwrap_or(0)
}

/// 埋点修复的目标章：显式字段优先，其次与 store::apply_payload 同规则解析
/// 「最后一个第N章」。卡片要显示真正会写进 plantedChapter 的章号。
fn fix_target_chapter(fx: &Value) -> Option<u32> {
    let explicit = ["plantedChapter", "chapter", "toChapter"]
        .iter()
        .find_map(|key| fx.get(*key).and_then(chapter_value));
    explicit.or_else(|| {
        if matches!(field(fx, "kind"), "补埋" | "改期") {
            chapter_from_detail(field(fx, "detail"))
        } else {
            None
        }
    })
}

/// 单行化：多行/连续空白压成一行（预览条目用；草稿正文另有 `text_lines`）。
fn oneline(s: &str) -> String {
    s.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// 按字符截断（中文按字，不按字节）。
fn clip_chars(s: &str, max: usize) -> String {
    let mut out = String::new();
    for (i, ch) in s.chars().enumerate() {
        if i >= max {
            out.push('…');
            break;
        }
        out.push(ch);
    }
    out
}

fn one_line_clip(s: &str, max: usize) -> String {
    clip_chars(&oneline(s), max)
}

/// 正文预览：先截断，再按行拆（保留作者能看懂的分段）。
fn text_chunks(text: &str, cap: usize) -> Vec<String> {
    let clipped = clip_chars(text.trim(), cap);
    let mut out: Vec<String> = clipped
        .split('\n')
        .map(|line| if line.trim().is_empty() { " ".to_string() } else { line.to_string() })
        .collect();
    if out.is_empty() {
        out.push(" ".to_string());
    }
    out
}

/// 台账码/时间：ISO 串截到分钟，`T` 换空格。
fn short_time(s: &str) -> String {
    if s.is_empty() {
        return "—".to_string();
    }
    s.replace('T', " ").chars().take(16).collect()
}

// ───────────────────────── 依据 ─────────────────────────

/// 显式引用来源的键：工具侧约定 `sourceRefs`，同时兼容 `refs`/`basis`/`citations`。
const REF_KEYS: [&str; 4] = ["sourceRefs", "refs", "basis", "citations"];

fn is_id(s: &str, prefix: char) -> bool {
    match s.strip_prefix(prefix) {
        Some(rest) => !rest.is_empty() && rest.chars().all(|c| c.is_ascii_digit()),
        None => false,
    }
}

fn clean_ref_label(s: &str) -> String {
    let s = s
        .trim()
        .trim_start_matches(|c: char| c == ':' || c == '：' || c.is_whitespace())
        .trim();
    let s = s.trim_start_matches('《').trim_end_matches('》');
    if s.is_empty() {
        "（未标注）".to_string()
    } else {
        s.to_string()
    }
}

/// 把模型给的引用串归类成「事实 / 设定 / 伏笔 / 事件 / 故事线 / 来源」。
/// 只认可判定的形态（F03、E02、L01、三元组、带前缀）；判不出就落「来源」，不硬猜。
fn classify_ref(raw: &str, context: &'static str) -> (&'static str, String) {
    let s = raw.trim();
    if is_id(s, 'F') {
        return ("伏笔", s.to_string());
    }
    if is_id(s, 'E') {
        return ("事件", s.to_string());
    }
    if is_id(s, 'L') {
        return ("故事线", s.to_string());
    }
    if s.matches('·').count() >= 2 {
        return ("事实", s.to_string());
    }
    for (prefix, kind) in [("设定", "设定"), ("伏笔", "伏笔"), ("事实", "事实")] {
        if let Some(rest) = s.strip_prefix(prefix) {
            return (kind, clean_ref_label(rest));
        }
    }
    if context != "来源" {
        return (context, s.to_string());
    }
    ("来源", s.to_string())
}

/// 条目数组 → 归类上下文（只给「肯定是这类」的数组）。
fn context_kind(key: &str) -> &'static str {
    match key {
        "docs" | "items" => "设定",
        "updates" => "故事线",
        "facts" => "事实",
        _ => "来源",
    }
}

fn push_ref_value(v: &Value, context: &'static str, out: &mut Vec<BasisItem>) {
    match v {
        Value::Array(items) => {
            for it in items {
                match it {
                    Value::String(s) => {
                        if !s.trim().is_empty() {
                            let (kind, label) = classify_ref(s, context);
                            out.push(BasisItem { kind, label, excerpt: String::new() });
                        }
                    }
                    Value::Object(o) => {
                        let raw = obj_field(o, "id");
                        let raw = if raw.is_empty() {
                            let alt = obj_field(o, "ref");
                            if alt.is_empty() {
                                let t = obj_field(o, "title");
                                if t.is_empty() { obj_field(o, "label") } else { t }
                            } else {
                                alt
                            }
                        } else {
                            raw
                        };
                        if raw.is_empty() {
                            continue;
                        }
                        let explicit = obj_field(o, "kind");
                        let explicit = if explicit.is_empty() { obj_field(o, "source") } else { explicit };
                        let (kind, label) = if explicit.is_empty() {
                            classify_ref(raw, context)
                        } else {
                            (kind_label(explicit), raw.to_string())
                        };
                        let excerpt = {
                            let q = obj_field(o, "quote");
                            if q.is_empty() {
                                let e = obj_field(o, "excerpt");
                                if e.is_empty() { obj_field(o, "note") } else { e }
                            } else {
                                q
                            }
                        };
                        out.push(BasisItem { kind, label, excerpt: excerpt.to_string() });
                    }
                    _ => {}
                }
            }
        }
        Value::String(s) => {
            if !s.trim().is_empty() {
                let (kind, label) = classify_ref(s, context);
                out.push(BasisItem { kind, label, excerpt: String::new() });
            }
        }
        _ => {}
    }
}

fn kind_label(kind: &str) -> &'static str {
    match kind {
        "canon" | "setting" | "设定" => "设定",
        "clue" | "foreshadow" | "伏笔" => "伏笔",
        "fact" | "事实" => "事实",
        "event" | "事件" => "事件",
        "storyline" | "故事线" => "故事线",
        "text" | "prose" | "正文" => "正文",
        _ => "来源",
    }
}

/// 依据清单：显式 sourceRefs 优先；没有显式条目时，从既有字段推导可核对的位置。
fn payload_basis(payload: &Value) -> Vec<BasisItem> {
    let mut out: Vec<BasisItem> = Vec::new();

    for key in REF_KEYS {
        if let Some(v) = payload.get(key) {
            push_ref_value(v, "来源", &mut out);
        }
    }
    for key in ["docs", "clues", "items", "beats", "suggestions", "findings", "facts", "fixes", "clueFixes", "updates", "outlineUpdates"] {
        for item in arr(payload, key) {
            for ref_key in REF_KEYS {
                if let Some(v) = item.get(ref_key) {
                    push_ref_value(v, context_kind(key), &mut out);
                }
            }
        }
    }

    if out.is_empty() {
        derived_basis(payload, &mut out);
    }

    // 去重（同 kind + 同 label 只留一次），保留首次出现顺序。
    let mut seen: BTreeSet<(&'static str, String)> = BTreeSet::new();
    out.retain(|b| seen.insert((b.kind, b.label.clone())));
    out
}

/// 既有字段推导：这些字段本身就是「指向既有材料」的坐标或原文。
fn derived_basis(payload: &Value, out: &mut Vec<BasisItem>) {
    match crate::store::payload_kind(payload).as_str() {
        "report" => {
            for f in arr(payload, "findings") {
                let loc = field(f, "location");
                out.push(BasisItem {
                    kind: "正文",
                    label: if loc.is_empty() { "位置未标注".to_string() } else { loc.to_string() },
                    excerpt: field(f, "evidence").to_string(),
                });
            }
        }
        "deslop" => {
            for s in arr(payload, "suggestions") {
                let original = field(s, "original");
                if original.is_empty() {
                    continue;
                }
                out.push(BasisItem {
                    kind: "正文",
                    label: format!("「{}」", clip_chars(&oneline(original), 24)),
                    excerpt: field(s, "reason").to_string(),
                });
            }
        }
        "clueFixes" => {
            for fx in arr_any(payload, &["fixes", "clueFixes"]) {
                let id = field(fx, "clueID");
                if id.is_empty() {
                    continue;
                }
                out.push(BasisItem {
                    kind: "伏笔",
                    label: id.to_string(),
                    excerpt: field(fx, "detail").to_string(),
                });
            }
        }
        "outlineUpdates" => {
            for u in arr_any(payload, &["updates", "outlineUpdates"]) {
                let id = field(u, "storylineID");
                if id.is_empty() {
                    continue;
                }
                out.push(BasisItem {
                    kind: "故事线",
                    label: id.to_string(),
                    excerpt: field(u, "note").to_string(),
                });
            }
        }
        "plannedEvent" => {
            let sid = field(payload, "storylineID");
            if !sid.is_empty() {
                out.push(BasisItem {
                    kind: "故事线",
                    label: sid.to_string(),
                    excerpt: field(payload, "from").to_string(),
                });
            }
        }
        "canonRevise" | "canonClassify" => {
            for it in arr(payload, "items") {
                let title = field(it, "title");
                if title.is_empty() {
                    continue;
                }
                out.push(BasisItem { kind: "设定", label: title.to_string(), excerpt: String::new() });
            }
        }
        "canon" => {
            for d in arr(payload, "docs") {
                let title = field(d, "title");
                if title.is_empty() {
                    continue;
                }
                out.push(BasisItem { kind: "设定", label: title.to_string(), excerpt: String::new() });
            }
        }
        "clues" => {
            for c in arr(payload, "clues") {
                let title = field(c, "title");
                if title.is_empty() {
                    continue;
                }
                let id = field(c, "id");
                let label = if id.is_empty() { title.to_string() } else { format!("{id} {title}") };
                out.push(BasisItem { kind: "伏笔", label, excerpt: String::new() });
            }
        }
        "memoryPack" | "dossier" => {
            let ch = chapter_of(payload);
            if ch > 0 {
                out.push(BasisItem {
                    kind: "正文",
                    label: format!("第{ch}章（提取来源）"),
                    excerpt: String::new(),
                });
            }
        }
        _ => {}
    }
}

// ───────────────────────── 内容预览 ─────────────────────────

fn default_unit(kind: &str) -> &'static str {
    match kind {
        "canon" | "canonRevise" | "canonClassify" => "篇",
        "skeleton" => "拍",
        "report" | "deslop" => "处",
        "dossier" => "类",
        _ => "条",
    }
}

/// payload → 可渲染的条目预览。未知类型退化为原始 payload（不猜结构）。
fn payload_preview(payload: &Value, expanded: bool) -> PayloadPreview {
    let kind = crate::store::payload_kind(payload);
    let limit = if expanded { PREVIEW_ITEMS_EXPANDED } else { PREVIEW_ITEMS_COLLAPSED };
    let max = if expanded { PREVIEW_CHARS_EXPANDED } else { PREVIEW_CHARS_COLLAPSED };
    let mut out = PayloadPreview::new(default_unit(&kind));

    match kind.as_str() {
        "canon" => {
            let docs = arr(payload, "docs");
            out.total = docs.len();
            for d in docs.iter().take(limit) {
                let content = field(d, "content");
                out.clipped |= content.chars().count() > max;
                let body = one_line_clip(content, max);
                out.push(format!("《{}》  {body}", field(d, "title")));
            }
        }
        "character" => {
            let list = arr_any(payload, &["characters", "items"]);
            out.total = list.len();
            for c in list.iter().take(limit) {
                let name = field(c, "name");
                let mut parts: Vec<String> = Vec::new();
                for (key, label) in [
                    ("role", "定位"),
                    ("appearance", "外貌"),
                    ("personality", "性格"),
                    ("motivation", "动机"),
                    ("voice", "口吻"),
                ] {
                    let v = field(c, key);
                    if !v.is_empty() {
                        parts.push(format!("{label}：{}", one_line_clip(v, 24)));
                    }
                }
                if parts.is_empty() {
                    parts.push("（只建卡，字段暂空）".to_string());
                }
                out.push(format!("{name}  {}", parts.join(" · ")));
            }
        }
        "clues" => {
            let clues = arr(payload, "clues");
            out.total = clues.len();
            for c in clues.iter().take(limit) {
                let id = field(c, "id");
                let id = if id.is_empty() { "待分配" } else { id };
                out.clipped |= field(c, "detail").chars().count() > max;
                let detail = one_line_clip(field(c, "detail"), max);
                out.push(format!("{id} · {}  {detail}", field(c, "title")));
            }
        }
        "canonClassify" => {
            let items = arr(payload, "items");
            out.total = items.len();
            for it in items.iter().take(limit) {
                let mut line = format!("《{}》 → {}", field(it, "title"), field(it, "category"));
                let rename = field(it, "rename");
                if !rename.is_empty() {
                    line.push_str(&format!("（改名：{rename}）"));
                }
                if expanded {
                    let note = field(it, "note");
                    if !note.is_empty() {
                        line.push_str(&format!("\n   理由：{}", one_line_clip(note, max)));
                    }
                }
                out.push(line);
            }
        }
        "skeleton" => {
            let beats = arr(payload, "beats");
            out.total = beats.len();
            for (i, b) in beats.iter().take(limit).enumerate() {
                let mut line =
                    format!("{}. {} · {}", i + 1, field(b, "title"), one_line_clip(field(b, "summary"), max));
                let hook = field(b, "hook");
                if !hook.is_empty() {
                    line.push_str(&format!("（钩子：{}）", one_line_clip(hook, 40)));
                }
                let words = b.get("suggestedWords").and_then(|v| v.as_u64()).unwrap_or(0);
                if words > 0 {
                    line.push_str(&format!(" · 约{words}字"));
                }
                out.push(line);
            }
        }
        "draft" => {
            let text = field(payload, "text");
            out.total = 1;
            out.total_chars = Some(text.chars().count());
            let cap = if expanded { LONG_TEXT_EXPANDED } else { LONG_TEXT_COLLAPSED };
            out.clipped = text.chars().count() > cap;
            out.lines = text_chunks(text, cap);
        }
        "canonRevise" => {
            let items = arr(payload, "items");
            out.total = items.len();
            let single = items.len() == 1;
            let cap = if expanded { LONG_TEXT_EXPANDED } else { LONG_TEXT_COLLAPSED };
            for it in items.iter().take(limit) {
                let title = field(it, "title");
                let content = field(it, "content");
                if single {
                    // 单篇修订：正文是主体，折叠态也要能看出「还有更多」，才给得出展开。
                    out.total_chars = Some(content.chars().count());
                    out.clipped = content.chars().count() > cap;
                }
                if expanded && single {
                    out.push(format!("《{title}》"));
                    out.lines.extend(text_chunks(content, cap));
                } else {
                    if !single {
                        out.clipped |= content.chars().count() > max;
                    }
                    let excerpt = one_line_clip(content, max);
                    out.push(format!("《{title}》  {excerpt}"));
                }
            }
        }
        "report" => {
            let findings = arr(payload, "findings");
            out.total = findings.len();
            for (i, f) in findings.iter().take(limit).enumerate() {
                let mut line = format!("{}[{}] {}", i + 1, field(f, "kind"), field(f, "location"));
                let evidence = field(f, "evidence");
                if !evidence.is_empty() {
                    line.push_str(&format!(" · {}", one_line_clip(evidence, max)));
                }
                if expanded {
                    let suggestion = field(f, "suggestion");
                    if !suggestion.is_empty() {
                        line.push_str(&format!("\n   → 建议：{}", one_line_clip(suggestion, max)));
                    }
                }
                out.push(line);
            }
        }
        "deslop" => {
            let suggestions = arr(payload, "suggestions");
            out.total = suggestions.len();
            for (i, s) in suggestions.iter().take(limit).enumerate() {
                let mut line = format!(
                    "{}「{}」→「{}」",
                    i + 1,
                    one_line_clip(field(s, "original"), 60),
                    one_line_clip(field(s, "replacement"), 60)
                );
                if expanded {
                    let reason = field(s, "reason");
                    if !reason.is_empty() {
                        line.push_str(&format!("\n   理由：{}", one_line_clip(reason, max)));
                    }
                }
                out.push(line);
            }
        }
        "relations" => {
            let rels = arr(payload, "relations");
            out.total = rels.len();
            for r in rels.iter().take(limit) {
                let mut line = format!(
                    "{} —{}→ {}",
                    field(r, "from"),
                    field(r, "predicate"),
                    field(r, "to")
                );
                if let Some(public) = r.get("publicToReader").and_then(|v| v.as_bool()) {
                    line.push_str(if public { " · 读者已知" } else { " · 暗线" });
                }
                let ev = field(r, "evidence");
                if !ev.is_empty() {
                    line.push_str(&format!("\n   依据：{}", one_line_clip(ev, max)));
                }
                out.push(line);
            }
        }
        "memoryPack" => {
            let facts = arr(payload, "facts");
            out.total = facts.len();
            for f in facts.iter().take(limit) {
                let mut line = format!(
                    "{} {} {}",
                    field(f, "subject"),
                    field(f, "predicate"),
                    field(f, "object")
                );
                if let Some(public) = f.get("publicToReader").and_then(|v| v.as_bool()) {
                    line.push_str(if public { " · 读者已知" } else { " · 读者未知" });
                }
                out.push(line);
            }
        }
        "dossier" => {
            let headline = field(payload, "headline");
            if !headline.is_empty() {
                out.push(format!("本章：{headline}"));
            }
            for (label, key) in [
                ("场景", "scenes"),
                ("出场", "cast"),
                ("悬念", "hooks"),
                ("锚点", "relics"),
                ("推进", "advanced"),
                ("拐点", "turns"),
            ] {
                let items = arr(payload, key);
                if items.is_empty() {
                    continue;
                }
                let shown = if expanded { 12 } else { 4 };
                out.clipped |= items.len() > shown;
                let joined = items
                    .iter()
                    .copied()
                    .take(shown)
                    .filter_map(value_text)
                    .collect::<Vec<_>>()
                    .join(" / ");
                let more = if items.len() > shown { " …" } else { "" };
                out.push(format!("{label}：{joined}{more}"));
            }
            out.total = out.lines.len().max(1);
        }
        "clueFixes" => {
            let fixes = arr_any(payload, &["fixes", "clueFixes"]);
            out.total = fixes.len();
            for fx in fixes.iter().take(limit) {
                let mut line = format!("{} · {}", field(fx, "clueID"), field(fx, "kind"));
                // 补埋/改期真正会写的埋设章必须上卡（解析不到才是「缺目标章号」）。
                if let Some(ch) = fix_target_chapter(fx) {
                    line.push_str(&format!(" → 第{ch}章"));
                }
                let detail = field(fx, "detail");
                if !detail.is_empty() {
                    line.push_str(&format!(" · {}", one_line_clip(detail, max)));
                }
                out.push(line);
            }
        }
        "outlineUpdates" => {
            let updates = arr_any(payload, &["updates", "outlineUpdates"]);
            out.total = updates.len();
            for u in updates.iter().take(limit) {
                let status = field(u, "status");
                let status = if status.is_empty() { "状态不变" } else { status };
                out.push(format!(
                    "[{}] {status} · {}",
                    field(u, "storylineID"),
                    one_line_clip(field(u, "note"), max)
                ));
            }
        }
        "plannedEvent" => {
            out.total = 1;
            let mut line = format!("第{}章 · {}", chapter_of(payload), field(payload, "title"));
            let sid = field(payload, "storylineID");
            if !sid.is_empty() {
                line.push_str(&format!(" · 线 {sid}"));
            }
            let from = field(payload, "from");
            if !from.is_empty() {
                line.push_str(&format!(" · 自 {from}"));
            }
            out.push(line);
        }
        _ => {
            // 未知 kind / 历史兼容名：原样展示 payload，不替作者猜结构。
            out.total = 1;
            let raw = serde_json::to_string(payload).unwrap_or_default();
            let cap = if expanded { PREVIEW_CHARS_EXPANDED } else { LONG_TEXT_COLLAPSED };
            out.clipped = raw.chars().count() > cap;
            out.lines = text_chunks(&raw, cap);
        }
    }

    out
}

// ───────────────────────── 落点 · 回滚 ─────────────────────────

/// 这一单采纳会改哪里、采纳前有没有快照/备份 —— 撤销粒度与 AI 动作一一对应。
/// 第三项：条目多于 1 时写明「整单采纳」——采纳粒度不藏在实现里。
fn payload_effect(payload: &Value) -> (String, String, Option<String>) {
    let (place, rollback) = effect_of(payload);
    (place, rollback, payload_unit(payload))
}

/// 多条目提案的实际采纳粒度（apply_payload 以提案为单位，不拆条目）。
fn payload_unit(payload: &Value) -> Option<String> {
    let n = match crate::store::payload_kind(payload).as_str() {
        "canon" => arr(payload, "docs").len(),
        "clues" => arr(payload, "clues").len(),
        "canonClassify" | "canonRevise" => arr(payload, "items").len(),
        "skeleton" => arr(payload, "beats").len(),
        "report" => arr(payload, "findings").len(),
        "deslop" => arr(payload, "suggestions").len(),
        "memoryPack" => arr(payload, "facts").len(),
        "relations" | "aliases" => arr(payload, "relations").len().max(arr(payload, "aliases").len()),
        "character" => arr_any(payload, &["characters", "items"]).len(),
        "clueFixes" => arr_any(payload, &["fixes", "clueFixes"]).len(),
        "outlineUpdates" => arr_any(payload, &["updates", "outlineUpdates"]).len(),
        _ => 1,
    };
    if n > 1 {
        Some(format!("整单处理 {n} 条（按提案粒度）"))
    } else {
        None
    }
}

fn effect_of(payload: &Value) -> (String, String) {
    let ch = chapter_of(payload);
    match crate::store::payload_kind(payload).as_str() {
        "canon" => (
            "设定库（同题不覆盖）".into(),
            "同题跳过；新增篇可直接删除".into(),
        ),
        "clues" => (
            "伏笔台账（同 id 合并，作者字段不覆盖）".into(),
            "采纳前 clues.json 备份到 snapshots/ledger/；不触正文".into(),
        ),
        "canonClassify" => (
            "设定类别头（可选改名）".into(),
            "采纳前每篇原文档备份到 snapshots/ledger/；设定正文保留".into(),
        ),
        "skeleton" => (
            format!("第{ch}章 meta.beats（解锁写草稿）"),
            "采纳前 meta.json 备份到 snapshots/ledger/；不触正文".into(),
        ),
        "draft" => (
            format!("第{ch}章正文（整章覆盖）"),
            "采纳前自动快照；「快照」页可恢复".into(),
        ),
        "report" => (
            format!("第{ch}章 meta.report（只读归档）"),
            "采纳前 meta.json 备份到 snapshots/ledger/；不触正文".into(),
        ),
        "deslop" => (
            format!("第{ch}章正文（命中处逐条替换）"),
            "首次替换前自动快照；未命中项跳过".into(),
        ),
        "relations" => (
            "relations.json 区间台账 + memory.json facts".into(),
            "采纳前双份备份到 snapshots/ledger/；关系维按章显示，同一对人物可多段演变".into(),
        ),
        "character" => (
            "characters.json（只补空、尊重 lockedFields）+ memory.aliases".into(),
            "采纳前 characters.json / memory.json 备份到 snapshots/ledger/；新建人物同步到画布".into(),
        ),
        "memoryPack" => (
            "memory.json facts（去重入账）".into(),
            "采纳前 memory.json 备份到 snapshots/ledger/；不触正文".into(),
        ),
        "dossier" => (
            "dossiers.json（同章覆盖）+ memory.aliases".into(),
            "采纳前 dossiers.json / memory.json 备份到 snapshots/ledger/；不触正文".into(),
        ),
        "clueFixes" => (
            "伏笔台账 status / plantedChapter / actionLog".into(),
            "采纳前 clues.json 备份到 snapshots/ledger/；不触正文".into(),
        ),
        "outlineUpdates" => (
            "storylines.json status / syncNote".into(),
            "采纳前 storylines.json 备份到 snapshots/ledger/；缺 note 不清旧备注".into(),
        ),
        "canonRevise" => {
            let title = arr(payload, "items")
                .first()
                .map(|it| field(it, "title").to_string())
                .filter(|t| !t.is_empty())
                .unwrap_or_else(|| "（未标注）".to_string());
            (
                format!("设定《{title}》正文"),
                "采纳前原文备份到 canon/backups/".into(),
            )
        }
        "plannedEvent" => (
            "events.json 新增事件（happened=false）".into(),
            "采纳前 events.json 备份到 snapshots/ledger/；不触正文".into(),
        ),
        _ => (
            "无落库语义（采纳会被拒，提案保持待审）".into(),
            "—".into(),
        ),
    }
}

// ───────────────────────── 卡片 / 历史 ─────────────────────────

/// 依据块：自述单列，引用条目逐行；一条可核对的引用都没有时显式标「未提供依据」。
fn basis_block(note: &str, refs: &[BasisItem]) -> AnyElement {
    let mut block = div()
        .w_full()
        .flex()
        .flex_col()
        .gap(theme::GAP_SM)
        .p(theme::PAD_MD)
        .rounded(theme::RADIUS_CHIP)
        .bg(theme::PANEL2())
        .border_1()
        .border_color(theme::LINE())
        .child(section_label("依据"));

    if note.trim().is_empty() {
        block = block.child(field_line("自述", "（未提供）".into(), theme::FAINT()));
    } else {
        block = block.child(field_line("自述", note.trim().to_string(), theme::DIM()));
    }

    if refs.is_empty() {
        block = block.child(
            div()
                .w_full()
                .flex()
                .items_start()
                .gap(theme::GAP_SM)
                .child(text_chip("未提供依据".to_string(), ChipTone::Warn))
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .text_size(theme::FONT_TAG)
                        .line_height(relative(theme::LINE_HEIGHT_BODY))
                        .text_color(theme::DIM())
                        .child("没有可核对的引用条目；自述不算依据，采纳前先看内容预览。"),
                ),
        );
    } else {
        for b in refs.iter().take(BASIS_ROWS_SHOWN) {
            let mut row = div()
                .w_full()
                .flex()
                .flex_col()
                .gap(theme::GAP_XS)
                .child(
                    div()
                        .w_full()
                        .text_size(theme::FONT_META)
                        .text_color(theme::TEXT())
                        .child(SharedString::from(format!("引自：{} {}", b.kind, b.label))),
                );
            if !b.excerpt.trim().is_empty() {
                row = row.child(
                    div()
                        .w_full()
                        .text_size(theme::FONT_TAG)
                        .line_height(relative(theme::LINE_HEIGHT_BODY))
                        .text_color(theme::DIM())
                        .child(SharedString::from(one_line_clip(&b.excerpt, 90))),
                );
            }
            block = block.child(row);
        }
        if refs.len() > BASIS_ROWS_SHOWN {
            block = block.child(
                div()
                    .text_size(theme::FONT_TAG)
                    .text_color(theme::FAINT())
                    .child(SharedString::from(format!("另有 {} 条引用", refs.len() - BASIS_ROWS_SHOWN))),
            );
        }
    }

    block.into_any_element()
}

fn preview_block(preview: &PayloadPreview, expanded: bool, toggle: Option<AnyElement>) -> AnyElement {
    let mut block = div()
        .w_full()
        .flex()
        .flex_col()
        .gap(theme::GAP_SM)
        .p(theme::PAD_MD)
        .rounded(theme::RADIUS_CHIP)
        .bg(theme::PANEL2())
        .border_1()
        .border_color(theme::LINE())
        .child(section_label("内容预览"));

    for line in &preview.lines {
        block = block.child(text_lines(line, theme::FONT_META, theme::TEXT()));
    }

    let mut foot = div()
        .w_full()
        .flex()
        .items_center()
        .gap(theme::GAP_MD)
        .child(
            div()
                .flex_1()
                .min_w_0()
                .text_size(theme::FONT_TAG)
                .text_color(theme::FAINT())
                .child(SharedString::from(preview.footnote(expanded))),
        );
    if let Some(t) = toggle {
        foot = foot.child(t);
    }
    block = block.child(foot);

    block.into_any_element()
}

fn effect_block(effect: &(String, String, Option<String>)) -> AnyElement {
    let mut block = div()
        .w_full()
        .flex()
        .flex_col()
        .gap(theme::GAP_SM)
        .p(theme::PAD_MD)
        .rounded(theme::RADIUS_CHIP)
        .bg(theme::PANEL2())
        .border_1()
        .border_color(theme::LINE())
        .child(section_label("落点 · 回滚"))
        .child(field_line("落点", effect.0.clone(), theme::TEXT()))
        .child(field_line("回滚", effect.1.clone(), theme::DIM()));
    if let Some(unit) = &effect.2 {
        block = block.child(field_line("粒度", unit.clone(), theme::DIM()));
    }
    block.into_any_element()
}

fn pending_card(
    p: &Proposal,
    expanded: bool,
    collapsed: PayloadPreview,
    actions: AnyElement,
    toggle: Option<AnyElement>,
) -> AnyElement {
    let preview = if expanded { payload_preview(&p.payload, true) } else { collapsed };
    let basis = payload_basis(&p.payload);
    let effect = payload_effect(&p.payload);
    let kind = crate::store::payload_kind(&p.payload);
    let kind = if kind.is_empty() { "?".to_string() } else { kind };
    let summary = summarize_payload(&p.payload);

    let mut card = div()
        .w_full()
        .flex()
        .flex_col()
        .gap(theme::GAP_MD)
        .p(theme::PAD_LG)
        .rounded(theme::RADIUS_CARD)
        .bg(theme::PANEL())
        .border_1()
        .border_color(theme::LINE())
        .shadow_sm()
        .hover(|d| d.border_color(theme::ACCENT_BORDER()))
        .font_family(theme::FAMILY_UI)
        .text_size(theme::FONT_BODY)
        .line_height(relative(theme::LINE_HEIGHT_BODY))
        // 类别小标（朱砂软底）+ 提案标题
        .child(
            div()
                .w_full()
                .flex()
                .items_center()
                .gap(theme::GAP_SM)
                .child(
                    div()
                        .flex_shrink_0()
                        .px(theme::PAD_SM)
                        .py(theme::PAD_XS)
                        .rounded(theme::RADIUS_BADGE)
                        .bg(theme::ACCENT_SOFT())
                        .border_1()
                        .border_color(theme::ACCENT_BORDER())
                        .text_size(theme::FONT_TAG)
                        .font_weight(FontWeight::SEMIBOLD)
                        .text_color(theme::ACCENT())
                        .child(SharedString::from(p.capability.clone())),
                )
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .overflow_hidden()
                        .font_weight(FontWeight::MEDIUM)
                        .text_color(theme::TEXT())
                        .child(SharedString::from(p.title.clone())),
                ),
        )
        // 摘要 = 次级说明档
        .child(
            div()
                .w_full()
                .text_size(theme::FONT_META)
                .text_color(theme::DIM())
                .child(SharedString::from(summary)),
        )
        // 台账码：kind + 创建时间（等宽右列，规则 10）
        .child(
            div()
                .w_full()
                .flex()
                .items_center()
                .gap(theme::GAP_SM)
                .child(
                    div()
                        .font_family(theme::FAMILY_MONO)
                        .text_size(theme::FONT_LABEL)
                        .text_color(theme::FAINT())
                        .child(SharedString::from(format!("kind: {kind}"))),
                )
                .child(div().flex_1())
                .child(
                    div()
                        .font_family(theme::FAMILY_MONO)
                        .text_size(theme::FONT_LABEL)
                        .text_color(theme::FAINT())
                        .child(SharedString::from(short_time(&p.created_at))),
                ),
        )
        // 依据先于内容：先看「凭什么」，再看「写什么」
        .child(basis_block(&p.note, &basis));

    // 展开态即使暂时没有可预览内容，也要保留「收起」，否则卡会卡在展开态。
    if preview.has_content() || toggle.is_some() {
        card = card.child(preview_block(&preview, expanded, toggle));
    }

    card.child(effect_block(&effect)).child(actions).into_any_element()
}

fn decision_chip(status: &str) -> AnyElement {
    let (fg, bg, border) = match status {
        "已采纳" => (theme::GREEN(), theme::GREEN_SOFT(), theme::GREEN_BORDER()),
        "已拒绝" => (theme::RED(), theme::RED_SOFT(), theme::LINE()),
        // 历史兼容状态：不冒充采纳/拒收语义，走中性档。
        _ => (theme::DIM(), theme::CHIP_NEUTRAL(), theme::LINE()),
    };
    div()
        .flex_shrink_0()
        .px(theme::PAD_SM)
        .py(theme::PAD_XS)
        .rounded(theme::RADIUS_CHIP)
        .bg(bg)
        .border_1()
        .border_color(border)
        .font_family(theme::FAMILY_UI)
        .text_size(theme::FONT_TAG)
        .font_weight(FontWeight::SEMIBOLD)
        .text_color(fg)
        .child(SharedString::from(status.to_string()))
        .into_any_element()
}

/// 已决历史行：标题/状态/时间常显，展开后回看 payload 预览、依据、落点与拒收理由。
fn history_row(p: &Proposal, expanded: bool, toggle: AnyElement) -> AnyElement {
    let preview = payload_preview(&p.payload, expanded);
    let basis = payload_basis(&p.payload);
    let effect = payload_effect(&p.payload);
    let summary = summarize_payload(&p.payload);
    let partial = p
        .apply_result
        .as_ref()
        .is_some_and(|r| !r.skipped.is_empty());

    let mut row = div()
        .w_full()
        .flex()
        .flex_col()
        .gap(theme::GAP_SM)
        .py(theme::PAD_SM)
        .border_b_1()
        .border_color(theme::LINE_SOFT())
        .child(
            div()
                .w_full()
                .flex()
                .items_center()
                .gap(theme::GAP_SM)
                .child(decision_chip(&p.status))
                .children(if partial {
                    vec![text_chip("部分入库".to_string(), ChipTone::Warn).into_any_element()]
                } else {
                    Vec::new()
                })
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .overflow_hidden()
                        .text_size(theme::FONT_META)
                        .text_color(theme::DIM())
                        .child(SharedString::from(p.title.clone())),
                )
                .child(
                    div()
                        .flex_shrink_0()
                        .font_family(theme::FAMILY_MONO)
                        .text_size(theme::FONT_LABEL)
                        .text_color(theme::FAINT())
                        .child(SharedString::from(short_time(p.decided_at.as_deref().unwrap_or("")))),
                )
                .child(toggle),
        )
        .child(
            div()
                .w_full()
                .text_size(theme::FONT_TAG)
                .text_color(theme::FAINT())
                .child(SharedString::from(summary)),
        );

    // 采纳结果来自 inbox.json 持久化的 applyResult（重启后仍可回放实际入库了什么），
    // 不是只存在于内存轨迹里的提示。
    if let Some(res) = &p.apply_result {
        let mut text = if res.message.is_empty() {
            format!("{} 项", res.applied)
        } else {
            res.message.clone()
        };
        if !res.skipped.is_empty() && !text.contains("跳过") {
            text.push_str(&format!("；跳过：{}", res.skipped.join("、")));
        }
        if !res.at.is_empty() {
            text.push_str(&format!(" · {}", short_time(&res.at)));
        }
        row = row.child(field_line("入库", text, theme::DIM()));
    }

    if expanded {
        if preview.has_content() {
            row = row.child(preview_block(&preview, true, None));
        }
        row = row.child(basis_block(&p.note, &basis));
        row = row.child(effect_block(&effect));
        if let Some(reason) = &p.reject_reason {
            if !reason.trim().is_empty() {
                row = row.child(field_line("拒收", reason.clone(), theme::RED()));
            }
        }
    }

    row.into_any_element()
}

/// 底部常驻决策账本：拍板压力与已决数量随时可见，不随滚动跑掉。
fn ledger_footer(pending: usize, decided: usize, accepted: usize, rejected: usize) -> AnyElement {
    let rate = if decided > 0 { accepted * 100 / decided } else { 0 };
    let rate_text = if decided > 0 {
        format!("采纳率 {rate}%（{accepted} / {decided}） · 拒收 {rejected}")
    } else {
        "采纳率 —（还没有已决提案）".to_string()
    };

    div()
        .w_full()
        .flex_shrink_0()
        .flex()
        .flex_col()
        .gap(theme::GAP_SM)
        .px(theme::PAD_XL)
        .py(theme::PAD_MD)
        .bg(theme::PANEL())
        .border_t_1()
        .border_color(theme::LINE())
        .child(
            div()
                .w_full()
                .flex()
                .flex_wrap()
                .items_center()
                .gap(theme::GAP_SM)
                .child(section_label("决策账本"))
                .child(text_chip(format!("等作者拍板 {pending} 项"), ChipTone::Action))
                .child(text_chip(format!("已决 {decided} 项"), ChipTone::Neutral)),
        )
        .child(
            div()
                .font_family(theme::FAMILY_MONO)
                .text_size(theme::FONT_TAG)
                .text_color(theme::FAINT())
                .child(SharedString::from(rate_text)),
        )
        .into_any_element()
}

impl Render for InboxPanel {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let pending: Vec<&Proposal> = self.proposals.iter().filter(|p| p.status == "待确认").collect();
        let decided: Vec<&Proposal> = self.proposals.iter().filter(|p| p.status != "待确认").collect();
        let decided_n = decided.len();
        let accepted = decided.iter().filter(|p| p.status == "已采纳").count();
        let rejected = decided.iter().filter(|p| p.status == "已拒绝").count();

        let mut cards: Vec<AnyElement> = Vec::new();
        for p in &pending {
            let id = p.id.clone();
            let accept_id = id.clone();
            let reject_id = id.clone();
            let expanded = self.expanded.contains(&p.id);

            let actions = div()
                .w_full()
                .flex()
                .items_center()
                .gap(theme::GAP_MD)
                .child(action_button(
                    format!("accept-{accept_id}"),
                    "采纳",
                    Tone::Primary,
                    cx.listener(move |_this, _, _, cx| {
                        cx.emit(InboxEvent::Accept(accept_id.clone()));
                    }),
                ))
                .child(action_button(
                    format!("reject-{reject_id}"),
                    "拒收",
                    Tone::Destructive,
                    // 提交后清空：下一条拒收不会静默复用上一条理由。
                    cx.listener(move |this, _, window, cx| {
                        let reason = this.reason.read(cx).value().to_string();
                        cx.emit(InboxEvent::Reject(
                            reject_id.clone(),
                            if reason.trim().is_empty() {
                                "（未填理由）".into()
                            } else {
                                reason
                            },
                        ));
                        this.reason
                            .update(cx, |state, cx| state.set_value("", window, cx));
                    }),
                ))
                .child(div().flex_1())
                .into_any_element();

            let collapsed = payload_preview(&p.payload, false);
            let toggle = if expanded || collapsed.expandable() {
                let tid = id.clone();
                Some(
                    action_button(
                        format!("toggle-{tid}"),
                        if expanded { "收起" } else { "展开全部" },
                        Tone::Neutral,
                        cx.listener(move |this, _, _, cx| {
                            this.toggle_expanded(tid.clone(), cx);
                        }),
                    )
                    .into_any_element(),
                )
            } else {
                None
            };

            cards.push(pending_card(*p, expanded, collapsed, actions, toggle));
        }

        let mut history_rows: Vec<AnyElement> = Vec::new();
        let show_all_history = self.expanded.contains("#history-all");
        let history_limit = if show_all_history { decided.len() } else { HISTORY_ROWS };
        for p in decided.iter().take(history_limit) {
            let id = p.id.clone();
            let expanded = self.expanded.contains(&p.id);
            let tid = id.clone();
            let toggle = action_button(
                format!("toggle-{tid}"),
                if expanded { "收起" } else { "展开" },
                Tone::Neutral,
                cx.listener(move |this, _, _, cx| {
                    this.toggle_expanded(tid.clone(), cx);
                }),
            )
            .into_any_element();
            history_rows.push(history_row(*p, expanded, toggle));
        }

        let history_note = if decided.len() > HISTORY_ROWS && !show_all_history {
            format!("已决账本 · 共 {decided_n} 项（显示最近 {HISTORY_ROWS} 项）")
        } else {
            format!("已决账本 · 共 {decided_n} 项")
        };

        let mut scroll_body = div()
            .w_full()
            .flex()
            .flex_col()
            .gap(theme::GAP_MD)
            .px(theme::PAD_XL)
            .pt(theme::PAD_MD)
            .pb(theme::PAD_XL)
            // —— 汇总条：语义 chip + 次级刷新钮（规则 6、7、12）
            .child(
                div()
                    .w_full()
                    .flex()
                    .items_center()
                    .gap(theme::GAP_SM)
                    .child(stat_chip("待裁决", pending.len(), ChipTone::Action))
                    .child(stat_chip("已处理", decided_n, ChipTone::Neutral))
                    .child(div().flex_1())
                    .child(action_button(
                        "inbox-refresh",
                        "刷新",
                        Tone::Neutral,
                        cx.listener(|_this, _, _, cx| {
                            cx.emit(InboxEvent::Refresh);
                        }),
                    )),
            )
            // —— 拒收理由：分节用 border-top + PANEL2 次级面（规则 4、12）
            .child(
                div()
                    .w_full()
                    .flex()
                    .flex_col()
                    .gap(theme::GAP_SM)
                    .border_t_1()
                    .border_color(theme::LINE())
                    .pt(theme::PAD_LG)
                    .child(
                        div()
                            .font_family(theme::FAMILY_UI)
                            .text_size(theme::FONT_TAG)
                            .font_weight(FontWeight::SEMIBOLD)
                            .text_color(theme::FAINT())
                            .child("拒收理由（回灌给 AI）"),
                    )
                    .child(
                        div()
                            .id("inbox-reason-box")
                            .w_full()
                            .rounded(theme::RADIUS_BTN)
                            .bg(theme::PANEL2())
                            .border_1()
                            .border_color(theme::LINE())
                            .hover(|style| style.border_color(theme::ACCENT_BORDER()))
                            .child(Input::new(&self.reason).appearance(false).text_color(theme::TEXT())),
                    )
                    .child(
                        div()
                            .text_size(theme::FONT_TAG)
                            .text_color(theme::FAINT())
                            .child("只作用于下一次「拒收」；提交后清空。"),
                    ),
            )
            .children(cards);

        // —— 空态：必须有引导语并指向动作（规则 15），文案保持原样
        if pending.is_empty() {
            scroll_body = scroll_body.child(
                Empty::new()
                    .w_full()
                    .child(
                        EmptyHeader::new()
                            .title(EmptyTitle::new().child("没有待裁决提案"))
                            .description(EmptyDescription::new().child(
                                "AI 每完成一轮设定、骨架、草稿或验证报告，成果都会登记到这里，等你采纳或拒收。",
                            )),
                    )
                    .child(EmptyContent::new().child(
                        Tag::secondary().outline().child("没出现？点上方「刷新」重新拉取登记"),
                    )),
            );
        }

        // —— 已决历史：状态、时间、拒收理由、payload 预览都能回看
        if !decided.is_empty() {
            scroll_body = scroll_body.child(
                div()
                    .w_full()
                    .flex()
                    .flex_col()
                    .gap(theme::GAP_XS)
                    .border_t_1()
                    .border_color(theme::LINE())
                    .pt(theme::PAD_LG)
                    .child(
                        div()
                            .w_full()
                            .flex()
                            .items_center()
                            .gap(theme::GAP_SM)
                            .child(section_label("已决账本"))
                            .child(
                                div()
                                    .flex_1()
                                    .text_size(theme::FONT_TAG)
                                    .text_color(theme::FAINT())
                                    .child(SharedString::from(history_note)),
                            )
                            .children(if decided.len() > HISTORY_ROWS {
                                vec![action_button(
                                    "history-toggle",
                                    if show_all_history { "收起历史" } else { "显示全部" },
                                    Tone::Neutral,
                                    cx.listener(|this, _, _, cx| {
                                        this.toggle_expanded("#history-all".into(), cx);
                                    }),
                                )
                                .into_any_element()]
                            } else {
                                Vec::new()
                            }),
                    )
                    .children(history_rows),
            );
        }

        div()
            .size_full()
            .flex()
            .flex_col()
            .bg(theme::BG())
            .child(
                div()
                    .id("inbox-scroll")
                    .flex_1()
                    .min_h_0()
                    .overflow_y_scroll()
                    .child(scroll_body),
            )
            .child(ledger_footer(pending.len(), decided_n, accepted, rejected))
    }
}

fn summarize_payload(payload: &Value) -> String {
    let kind = crate::store::payload_kind(payload);
    let n = |key: &str| {
        // 旧版键名兼容：fixes→clueFixes、updates→outlineUpdates
        let legacy = match key {
            "fixes" => "clueFixes",
            "updates" => "outlineUpdates",
            _ => "",
        };
        if legacy.is_empty() {
            arr(payload, key).len()
        } else {
            arr_any(payload, &[key, legacy]).len()
        }
    };
    match kind.as_str() {
        "canon" => format!("{} 篇设定文档（同题不覆盖）", n("docs")),
        "clues" => format!("{} 条伏笔（同 id 合并）", n("clues")),
        "canonClassify" => format!("{} 篇归类/改名", n("items")),
        "canonRevise" => format!("修订设定 {} 篇（采纳前备份原文）", n("items")),
        "skeleton" => format!("{} 节拍骨架（批准后解锁草稿）", n("beats")),
        "draft" => format!("第{}章草稿（采纳前自动快照）", chapter_of(payload)),
        "plannedEvent" => format!(
            "第{}章 计划事件（事件台账 happened=false）",
            chapter_of(payload)
        ),
        "report" => format!("验证报告 {} 处发现（只报告不代改）", n("findings")),
        "deslop" => format!("去AI味 {} 处建议（一次采纳全部命中项）", n("suggestions")),
        "memoryPack" => format!("记忆 {} 条事实（双时态）", n("facts")),
        "relations" => format!("人物关系 {} 条（入区间台账）", n("relations")),
        "character" => format!(
            "人物档案 {} 人（只补空，新建进画布）",
            arr_any(payload, &["characters", "items"]).len()
        ),
        "aliases" => format!("别名档案 {} 人", n("aliases")),
        "dossier" => format!("档案卡：{} 场景 · {} 锚点", n("scenes"), n("relics")),
        "clueFixes" => format!("埋点修复 {} 条（只改台账）", n("fixes")),
        "outlineUpdates" => format!("大纲更新 {} 条建议", n("updates")),
        "memo" => "备忘（无落库语义）".into(),
        other => format!("未登记类型「{other}」（预览为原始 payload）"),
    }
}

impl BasePanel for InboxPanel {
    fn panel_name(&self) -> &'static str {
        "proposal-inbox"
    }
}

impl ComponentPanel for InboxPanel {
    fn tab_name(&self, _: &App) -> Option<SharedString> {
        Some("提案".into())
    }

    fn title(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        let pending_n = self.proposals.iter().filter(|p| p.status == "待确认").count();
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
                    .child("提案收件箱"),
            )
            // 待裁决数量徽标：类别小标段（RADIUS_BADGE + FONT_TAG，规则 6、11）
            .children(if pending_n > 0 {
                vec![div()
                    .flex()
                    .items_center()
                    .px(theme::PAD_SM)
                    .py(theme::PAD_XS)
                    .rounded(theme::RADIUS_BADGE)
                    .bg(theme::ACCENT_SOFT())
                    .border_1()
                    .border_color(theme::ACCENT_BORDER())
                    .font_family(theme::FAMILY_UI)
                    .text_size(theme::FONT_TAG)
                    .font_weight(FontWeight::SEMIBOLD)
                    .text_color(theme::ACCENT())
                    .child(pending_n.to_string())
                    .into_any_element()]
            } else {
                Vec::new()
            })
    }
}
#[cfg(test)]
mod tests {
    use super::payload_basis;
    use serde_json::json;

    /// 世界线晋升的 canon/clues 提案即使没有条目级 sourceRefs，也必须从 title 推导出依据，
    /// 不能落到琥珀「未提供依据」。
    #[test]
    fn promoted_payloads_have_basis() {
        let canon = json!({
            "kind": "canon",
            "docs": [{ "title": "灯约", "content": "灯灭即恩断" }],
            "sourceRefs": ["线β", "设定:灯约"]
        });
        let basis = payload_basis(&canon);
        assert!(basis.iter().any(|b| b.kind == "设定" && b.label == "灯约"));

        let clues = json!({
            "kind": "clues",
            "clues": [{ "title": "虎符", "detail": "埋在第三章", "plantedChapter": 3 }]
        });
        let basis = payload_basis(&clues);
        assert!(basis.iter().any(|b| b.kind == "伏笔" && b.label == "虎符"));
    }

    /// 埋点卡显示的目标章必须与 apply_payload 的解析一致（取最后一个第N章）。
    #[test]
    fn clue_fix_preview_targets_last_chapter() {
        let fx = json!({ "clueID": "F01", "kind": "改期", "detail": "由第3章改到第9章" });
        assert_eq!(super::fix_target_chapter(&fx), Some(9));
        let explicit = json!({ "clueID": "F01", "kind": "改期", "detail": "第8章", "plantedChapter": 11 });
        assert_eq!(super::fix_target_chapter(&explicit), Some(11));
    }
}
