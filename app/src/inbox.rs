//! 收件箱面板（右侧 Dock 第二页签）：待裁决提案就地采纳/拒收。
//!
//! 视觉全部取自 `crate::theme` 令牌（无裸 `rgb(0x…)`，间距/圆角/字号不混档）：
//! - 层次（规则 4）：滚动底 = `BG()` 墨底，卡片/空态块 = `PANEL()` 亮一档 + `LINE()` 1px；
//!   理由输入框 = `PANEL2()` + `LINE()` 描边的次级面（库主题近白，故 `appearance(false)` 后自绘）。
//! - 状态语言（规则 6）：待裁决 = `ACCENT_SOFT` + `ACCENT_BORDER` chip；已处理 = `CHIP_NEUTRAL`
//!   + `LINE` chip；拒收 = `RED_SOFT` + `RED`；一律 `FONT_TAG`/`RADIUS_CHIP`/1px 语义描边。
//! - 按钮（规则 5、7）：采纳 = `ACCENT()` 实心 + `ON_ACCENT()` 白字 + `RADIUS_BTN`；
//!   刷新 = 中性次级（`PANEL2` + `LINE` + `DIM`），hover 底色提亮一档并把描边转成
//!   `ACCENT_BORDER()`，active 再压暗一档 —— 提亮/压暗由 `lifted`/`pressed` 用现有
//!   token（`CHIP_NEUTRAL` 7% 白、`BG` 墨底）叠出，不新增色值。
//! - 文字三档（规则 8）：标题 `TEXT()`、摘要 `DIM()`、`kind` 台账码 `FAINT()`。
//! - 台账码（规则 10）：`kind: xxx` 用 `FAMILY_MONO` + `FONT_LABEL`，等宽右列。
//! - 分节（规则 12）：栏水平内边距 `PAD_XL`(20)，列表内卡片之间 `GAP_MD`(8)，分节用
//!   `border-top`（`LINE`）+ `PAD_LG`(16) 内边距；废除 `p_2` / `rounded_md` / `text_xs` 混档。
//! - 空态（规则 15）：TEXT 主句 + DIM 引导句 + 指向「刷新」动作的中性 chip。

use std::rc::Rc;

use gpui_kit::component::dock::{BasePanel, Panel as ComponentPanel, PanelEvent};
use gpui_kit::component::input::{Input, InputState};
use gpui_kit::{
    App, AppContext as _, ClickEvent, Context, Entity, EventEmitter, FocusHandle, Focusable,
    FontWeight, Hsla, InteractiveElement, IntoElement, ParentElement, Pixels, Render, Rgba,
    SharedString, StatefulInteractiveElement, Styled, Window, div, relative,
};

use crate::store::{ProjectHandle, Proposal};
use crate::theme;

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
}

impl InboxPanel {
    pub fn new(window: &mut Window, cx: &mut Context<Self>, handle: Rc<ProjectHandle>) -> Self {
        let reason = cx.new(|cx| InputState::new(window, cx).placeholder("拒收理由（回灌给 AI）…"));
        let proposals = handle.proposals();
        Self { focus_handle: cx.focus_handle(), handle, proposals, reason }
    }

    pub fn refresh(&mut self, cx: &mut Context<Self>) {
        self.proposals = self.handle.proposals();
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
    /// PANEL2 底 + LINE 描边 + DIM 字（刷新）。
    Neutral,
}

/// 语义 chip 语气（规则 6）：可行动 = 朱砂，纯计数 = 中性。
#[derive(Clone, Copy)]
enum ChipTone {
    Action,
    Neutral,
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

/// 计数 chip：标签 + 数值（数值转 `TEXT()`/半粗，规则 8 先分级再上色）。
fn stat_chip(label: &'static str, value: usize, tone: ChipTone) -> impl IntoElement {
    let (bg, border, fg): (Hsla, Hsla, Hsla) = match tone {
        ChipTone::Action => (
            theme::ACCENT_SOFT().into(),
            theme::ACCENT_BORDER().into(),
            theme::ACCENT().into(),
        ),
        ChipTone::Neutral => (
            theme::CHIP_NEUTRAL().into(),
            theme::LINE().into(),
            theme::DIM().into(),
        ),
    };

    div()
        .flex()
        .items_center()
        .gap(theme::GAP_SM)
        .flex_shrink_0()
        .px(theme::PAD_SM)
        .py(theme::PAD_XS)
        .rounded(theme::RADIUS_CHIP)
        .bg(bg)
        .border_1()
        .border_color(border)
        .font_family(theme::FAMILY_UI)
        .text_size(theme::FONT_TAG)
        .child(div().text_color(fg).child(label))
        .child(
            div()
                .font_weight(FontWeight::SEMIBOLD)
                .text_color(theme::TEXT())
                .child(value.to_string()),
        )
}

impl Render for InboxPanel {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let pending: Vec<&Proposal> = self.proposals.iter().filter(|p| p.status == "待确认").collect();
        let decided_n = self.proposals.len() - pending.len();

        let mut cards = Vec::new();
        for p in &pending {
            let id = p.id.clone();
            let accept_id = id.clone();
            let reject_id = id.clone();
            let kind_label = p
                .payload
                .get("kind")
                .and_then(|k| k.as_str())
                .unwrap_or("?")
                .to_string();
            let summary = summarize_payload(&p.payload);
            cards.push(
                div()
                    .w_full()
                    .flex()
                    .flex_col()
                    .gap(theme::GAP_MD)
                    .p(theme::PAD_LG)
                    .rounded(theme::RADIUS_CARD)
                    .bg(theme::PANEL())
                    .border_1()
                    .border_color(theme::LINE())
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
                    // 动作行：采纳（主能力）· 拒收（破坏性）· 右列等宽 kind 台账码
                    .child(
                        div()
                            .w_full()
                            .flex()
                            .items_center()
                            .gap(theme::GAP_MD)
                            .child(
                                action_button(
                                    format!("accept-{accept_id}"),
                                    "采纳",
                                    Tone::Primary,
                                    cx.listener(move |_this, _, _, cx| {
                                        cx.emit(InboxEvent::Accept(accept_id.clone()));
                                    }),
                                ),
                            )
                            .child(
                                action_button(
                                    format!("reject-{reject_id}"),
                                    "拒收",
                                    Tone::Destructive,
                                    cx.listener(move |this, _, _, cx| {
                                        let reason = this.reason.read(cx).value().to_string();
                                        cx.emit(InboxEvent::Reject(
                                            reject_id.clone(),
                                            if reason.trim().is_empty() {
                                                "（未填理由）".into()
                                            } else {
                                                reason
                                            },
                                        ));
                                    }),
                                ),
                            )
                            .child(div().flex_1())
                            .child(
                                div()
                                    .flex_shrink_0()
                                    .whitespace_nowrap()
                                    .font_family(theme::FAMILY_MONO)
                                    .text_size(theme::FONT_LABEL)
                                    .text_color(theme::FAINT())
                                    .child(SharedString::from(format!("kind: {kind_label}"))),
                            ),
                    ),
            );
        }

        div()
            .id("inbox-scroll")
            .size_full()
            .overflow_y_scroll()
            .bg(theme::BG())
            .child(
                div()
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
                            .child(
                                action_button(
                                    "inbox-refresh",
                                    "刷新",
                                    Tone::Neutral,
                                    cx.listener(|_this, _, _, cx| {
                                        cx.emit(InboxEvent::Refresh);
                                    }),
                                ),
                            ),
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
                                    .child(
                                        Input::new(&self.reason)
                                            .appearance(false)
                                            .text_color(theme::TEXT()),
                                    ),
                            ),
                    )
                    .children(cards)
                    // —— 空态：必须有引导语并指向动作（规则 15）
                    .children(if pending.is_empty() {
                        vec![div()
                            .w_full()
                            .flex()
                            .flex_col()
                            .items_center()
                            .justify_center()
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
                                    .child("没有待裁决提案"),
                            )
                            .child(
                                div()
                                    .text_size(theme::FONT_META)
                                    .text_color(theme::DIM())
                                    .child(
                                        "AI 每完成一轮设定、骨架、草稿或验证报告，成果都会登记到这里，等你采纳或拒收。",
                                    ),
                            )
                            .child(
                                div()
                                    .flex()
                                    .items_center()
                                    .gap(theme::GAP_SM)
                                    .px(theme::PAD_SM)
                                    .py(theme::PAD_XS)
                                    .rounded(theme::RADIUS_CHIP)
                                    .bg(theme::CHIP_NEUTRAL())
                                    .border_1()
                                    .border_color(theme::LINE())
                                    .font_family(theme::FAMILY_UI)
                                    .text_size(theme::FONT_TAG)
                                    .text_color(theme::DIM())
                                    .child("没出现？点上方「刷新」重新拉取登记"),
                            )
                            .into_any_element()]
                    } else {
                        Vec::new()
                    }),
            )
    }
}

fn summarize_payload(payload: &serde_json::Value) -> String {
    let kind = payload.get("kind").and_then(|k| k.as_str()).unwrap_or("");
    let arr = |key: &str| payload.get(key).and_then(|v| v.as_array()).map(|a| a.len()).unwrap_or(0);
    match kind {
        "canon" => format!("{} 篇设定文档（同题不覆盖）", arr("docs")),
        "clues" => format!("{} 条伏笔（同 id 合并）", arr("clues")),
        "canonClassify" => format!("{} 篇归类/改名", arr("items")),
        "skeleton" => format!("{} 节拍骨架（批准后解锁草稿）", arr("beats")),
        "draft" => format!(
            "第{}章草稿（采纳前自动快照）",
            payload.get("chapter").and_then(|c| c.as_u64()).unwrap_or(0)
        ),
        "report" => format!("验证报告 {} 处发现（只报告不代改）", arr("findings")),
        "deslop" => format!("去AI味 {} 处建议（逐条采纳）", arr("suggestions")),
        "memoryPack" => format!("记忆 {} 条事实（双时态）", arr("facts")),
        "dossier" => format!(
            "档案卡：{} 场景 · {} 锚点",
            arr("scenes"),
            arr("relics")
        ),
        "clueFixes" => format!("埋点修复 {} 条（只改台账）", arr("fixes")),
        "outlineUpdates" => format!("大纲更新 {} 条建议", arr("updates")),
        "memo" => "备忘".into(),
        other => other.into(),
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
