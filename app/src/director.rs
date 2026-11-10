//! M7-3 导演台：章 × 事件/伏笔排期板（design/canvas.html 的「导演台」视图实现）。
//!
//! 主权与铁律的分工：拖卡改期 = 作者亲笔（直写 events.json/clues.json，
//! 这是台账的"手改"通道，与编辑正文同级，AI 无此权力）；「AI 核对本章」=
//! 走 prompts::director_task 进 DSH，AI 只能报告与提案。
//! 底部预警条全部由确定性规则算出（无排期章/伏笔静默逾期/断线线/窗口外存量）。

use std::rc::Rc;

use gpui_kit::component::button::Button;
use gpui_kit::component::dock::{BasePanel, Panel as ComponentPanel, PanelEvent};
use gpui_kit::component::tag::Tag;
use gpui_kit::component::Sizable;
use gpui_kit::prelude::FluentBuilder;
use gpui_kit::{
    AnyElement, App, Context, EventEmitter, FocusHandle, Focusable,
    FontWeight, InteractiveElement as _, IntoElement, MouseButton, MouseDownEvent,
    ParentElement, Render, SharedString, StatefulInteractiveElement, Styled, Window, div, px, relative,
};

use crate::store::ProjectHandle;
use crate::theme;

fn hsla(c: gpui_kit::Rgba) -> gpui_kit::Hsla {
    c.into()
}

fn s(v: &serde_json::Value, key: &str) -> String {
    v.get(key).and_then(|x| x.as_str()).unwrap_or("").to_string()
}

const COLS: usize = 8;
const COL_W: f32 = 196.;

#[derive(Clone, Copy, PartialEq)]
enum CardKind {
    Event,
    CluePlant,
}

#[derive(Clone)]
struct DCard {
    kind: CardKind,
    id: String,
    label: String,
    chapter: u32,
    /// 暗线事件（未揭示）：卡上加锁标
    hidden: bool,
    /// 计划事件（happened=false，预测排期晋升后的待写成）：虚线卡 + 计划标
    planned: bool,
}

#[derive(Clone)]
struct PredCard {
    line: String,
    idx: usize,
    label: String,
}

#[derive(Debug, Clone)]
pub enum DirectorEvent {
    /// 让 AI 按骨架合同核对第 N 章（0=从本板窗口起点）
    Review(u32),
}

pub struct DirectorPanel {
    focus_handle: FocusHandle,
    handle: Rc<ProjectHandle>,
    current: u32,
    cards: Vec<DCard>,
    pool: Vec<DCard>,
    preds: Vec<PredCard>,
    pred_drag: Option<usize>,
    drag: Option<DCard>,
    hover: Option<usize>,
    message: Option<SharedString>,
}

impl DirectorPanel {
    pub fn new(cx: &mut Context<Self>, handle: Rc<ProjectHandle>, current: u32) -> Self {
        let mut this = Self {
            focus_handle: cx.focus_handle(),
            handle,
            current,
            cards: Vec::new(),
            pool: Vec::new(),
            preds: Vec::new(),
            pred_drag: None,
            drag: None,
            hover: None,
            message: None,
        };
        this.refresh();
        this
    }

    /// 从台账重建排期卡；窗口 [current, current+COLS)。
    pub fn refresh(&mut self) {
        // 右界=下一待写章：章节被删/书变短时把窗口拉回，避免只剩空列
        let max = self.handle.next_chapter_number().max(1);
        if self.current > max {
            self.current = max;
        }
        let end = self.current + COLS as u32;
        let mut cards = Vec::new();
        let mut pool = Vec::new();
        for e in self.handle.events() {
            let ch = e.get("chapter").and_then(|v| v.as_u64()).unwrap_or(0) as u32;
            let card = DCard {
                kind: CardKind::Event,
                id: s(&e, "id"),
                label: s(&e, "objectiveFact").chars().take(18).collect(),
                chapter: ch,
                hidden: !e.get("revealed").and_then(|r| r.as_bool()).unwrap_or(true),
                planned: !e.get("happened").and_then(|v| v.as_bool()).unwrap_or(true),
            };
            if ch >= self.current && ch < end {
                cards.push(card);
            } else {
                pool.push(card);
            }
        }
        for c in self.handle.clues_full() {
            let status = s(&c, "status");
            if !matches!(status.as_str(), "埋下" | "推进中") {
                continue;
            }
            let ch = c.get("plantedChapter").and_then(|v| v.as_u64()).unwrap_or(0) as u32;
            let card = DCard {
                kind: CardKind::CluePlant,
                id: s(&c, "id"),
                label: format!("埋·{}", s(&c, "title").chars().take(14).collect::<String>()),
                chapter: ch,
                hidden: false,
                planned: false,
            };
            if ch >= self.current && ch < end {
                cards.push(card);
            } else {
                pool.push(card);
            }
        }
        self.cards = cards;
        self.pool = pool;
        // M8-3：预测线 add 型 diff 进候选池（拖入列=晋升为 plannedEvent 提案，非直写）
        let mut preds = Vec::new();
        for w in self.handle.worldlines() {
            if s(&w, "kind") != "forecast" {
                continue;
            }
            let line = s(&w, "id");
            for (idx, d) in w.get("diffs").and_then(|x| x.as_array()).cloned().unwrap_or_default().into_iter().enumerate() {
                if d.get("promoted").and_then(|p| p.as_bool()).unwrap_or(false) {
                    continue;
                }
                let target = s(&d, "target");
                if s(&d, "kind") == "add" && (target == "事件" || target == "故事线") {
                    preds.push(PredCard {
                        line: line.clone(),
                        idx,
                        label: s(&d, "after").chars().take(18).collect(),
                    });
                }
            }
        }
        self.preds = preds;
        self.drag = None;
        self.pred_drag = None;
        self.hover = None;
    }

    fn drop_to(&mut self, col: usize, cx: &mut Context<Self>) {
        // M8-3 预测卡：落列=带章号晋升（plannedEvent 提案，采纳才入 events.json）
        if let Some(pi) = self.pred_drag.take() {
            let chap = self.current + col as u32;
            if let Some(p) = self.preds.get(pi) {
                let (line, idx) = (p.line.clone(), p.idx);
                match self.handle.promote_diff_chapter(&line, idx, chap) {
                    Ok(pid) => self.message = Some(format!("预测线{line} 计划 → 第{chap}章 提案 {pid}（采纳才入账）").into()),
                    Err(e) => self.message = Some(format!("排期晋升失败：{e}").into()),
                }
            }
            self.refresh();
            self.hover = None;
            cx.notify();
            return;
        }
        let Some(card) = self.drag.take() else { return };
        let chap = self.current + col as u32;
        let res = match card.kind {
            CardKind::Event => self.handle.set_event_chapter(&card.id, chap).map(|l| format!("事件「{l}」→ 第{chap}章")),
            CardKind::CluePlant => self
                .handle
                .set_clue_planted(&card.id, chap)
                .map(|l| format!("伏笔「{l}」埋点 → 第{chap}章")),
        };
        match res {
            Ok(msg) => {
                self.message = Some(msg.into());
                self.refresh();
            }
            Err(e) => self.message = Some(format!("改期失败：{e}").into()),
        }
        self.hover = None;
        cx.notify();
    }

    /// 确定性预警（只读规则，无 AI）。
    fn warnings(&self) -> Vec<AnyElement> {
        let mut out: Vec<AnyElement> = Vec::new();
        // 未写到的未来章没有事件是常态，不再刷「无事件覆盖」噪音；只提示已有章节
        let last_existing = self.handle.next_chapter_number().saturating_sub(1).max(1);
        for (i, _) in (0..COLS).enumerate() {
            let chap = self.current + i as u32;
            if chap > last_existing {
                continue;
            }
            let n = self.cards.iter().filter(|c| c.chapter == chap).count();
            if n == 0 {
                out.push(
                    Tag::custom(hsla(theme::RED_SOFT()), hsla(theme::RED()), hsla(theme::LINE()))
                        .outline()
                        .child(SharedString::from(format!("第{chap}章无事件覆盖")))
                        .into_any_element(),
                );
            }
        }
        for c in self.handle.clues_full() {
            let status = s(&c, "status");
            if !matches!(status.as_str(), "埋下" | "推进中") {
                continue;
            }
            let planted = c.get("plantedChapter").and_then(|v| v.as_u64()).unwrap_or(0) as u32;
            let last = c.get("lastActionChapter").and_then(|v| v.as_u64()).unwrap_or(planted as u64) as u32;
            let silent = self.current.saturating_sub(last.max(planted));
            if silent >= 8 {
                out.push(
                    Tag::custom(hsla(theme::AMBER_SOFT()), hsla(theme::AMBER()), hsla(theme::LINE()))
                        .outline()
                        .child(SharedString::from(format!("{} 已静默 {silent} 章", s(&c, "id"))))
                        .into_any_element(),
                );
            }
        }
        for l in self.handle.storylines() {
            if s(&l, "status") == "断线" {
                out.push(
                    Tag::custom(hsla(theme::RED_SOFT()), hsla(theme::RED()), hsla(theme::LINE()))
                        .outline()
                        .child(SharedString::from(format!("{}「{}」断线", s(&l, "id"), s(&l, "name"))))
                        .into_any_element(),
                );
            }
        }
        let unhidden_future: usize = self
            .handle
            .events()
            .iter()
            .filter(|e| {
                let ch = e.get("chapter").and_then(|v| v.as_u64()).unwrap_or(0) as u32;
                !e.get("revealed").and_then(|r| r.as_bool()).unwrap_or(true) && ch >= self.current && ch < self.current + COLS as u32
            })
            .count();
        if unhidden_future > 0 {
            out.push(
                Tag::custom(hsla(theme::BLUE_SOFT()), hsla(theme::BLUE()), hsla(theme::LINE()))
                    .outline()
                    .child(SharedString::from(format!("窗口内 {unhidden_future} 条未揭示暗线事件")))
                    .into_any_element(),
            );
        }
        out
    }
}

impl EventEmitter<PanelEvent> for DirectorPanel {}
impl EventEmitter<DirectorEvent> for DirectorPanel {}

impl Focusable for DirectorPanel {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus_handle.clone()
    }
}

fn card_view(card: &DCard, dragging: bool, cx: &gpui_kit::Context<DirectorPanel>) -> AnyElement {
    let c = card.clone();
    let (dot, dot_color) = match card.kind {
        CardKind::Event => ("●", if card.hidden { theme::PURPLE() } else { theme::BLUE() }),
        CardKind::CluePlant => ("◆", theme::AMBER()),
    };
    div()
        .id(SharedString::from(format!("card-{}", card.id)))
        .p(theme::PAD_SM)
        .rounded(theme::RADIUS_PANEL)
        .border_1()
        .border_color(if dragging {
            theme::ACCENT_BORDER()
        } else if card.planned {
            theme::GREEN_BORDER()
        } else {
            theme::LINE()
        })
        .when(card.planned, |d| d.border_dashed())
        .bg(theme::PANEL2())
        .cursor_grab()
        .on_mouse_down(
            MouseButton::Left,
            cx.listener(move |this, _: &MouseDownEvent, _, cx| {
                this.drag = Some(c.clone());
                this.pred_drag = None;
                cx.notify();
            }),
        )
        .child(
            div()
                .flex()
                .items_center()
                .gap(theme::GAP_XS)
                .child(div().text_size(theme::FONT_TAG).text_color(dot_color).child(dot))
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .overflow_hidden()
                        .whitespace_nowrap()
                        .text_size(theme::FONT_TAG)
                        .text_color(theme::TEXT())
                        .child(SharedString::from(card.label.clone())),
                ),
        )
        .child(
            div()
                .flex()
                .items_center()
                .gap(theme::GAP_SM)
                .child(
                    div()
                        .font_family(theme::FAMILY_MONO)
                        .text_size(theme::FONT_MICRO)
                        .text_color(theme::FAINT())
                        .child(SharedString::from(card.id.clone())),
                )
                .when(card.planned, |d| {
                    d.child(
                        div()
                            .text_size(theme::FONT_MICRO)
                            .text_color(theme::GREEN())
                            .child("计划·待写成"),
                    )
                }),
        )
        .into_any_element()
}

impl Render for DirectorPanel {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let warnings = self.warnings();
        let msg = self.message.clone();
        let drag_active = self.drag.is_some() || self.pred_drag.is_some();
        let stages = self.handle.stages();
        let stage_of = |chap: u32| -> Option<(String, String)> {
            for st in &stages {
                let a = st.get("chapterStart").and_then(|v| v.as_u64()).unwrap_or(0) as u32;
                let b = st.get("chapterEnd").and_then(|v| v.as_u64()).unwrap_or(0) as u32;
                if chap >= a && chap <= b {
                    return Some((s(st, "name"), s(st, "theme")));
                }
            }
            None
        };

        let mut board: Vec<AnyElement> = Vec::new();
        for i in 0..COLS {
            let chap = self.current + i as u32;
            let col_cards: Vec<DCard> = self.cards.iter().filter(|c| c.chapter == chap).cloned().collect();
            let hovered = self.hover == Some(i) && drag_active;
            let col = div()
                .id(SharedString::from(format!("dcol-{i}")))
                .w(px(COL_W))
                .flex_shrink_0()
                .flex()
                .flex_col()
                .rounded(theme::RADIUS_CARD)
                .border_1()
                .border_color(if hovered { theme::ACCENT_BORDER() } else { theme::LINE() })
                .bg(if hovered { theme::ACCENT_SOFT() } else { theme::PANEL() })
                .child(
                    div()
                        .flex()
                        .items_center()
                        .gap(theme::GAP_SM)
                        .px(theme::PAD_SM)
                        .py(theme::GAP_SM)
                        .border_b_1()
                        .border_color(theme::LINE())
                        .child(
                            div()
                                .text_size(theme::FONT_LABEL)
                                .font_weight(FontWeight::SEMIBOLD)
                                .text_color(theme::TEXT())
                                .child(SharedString::from(format!("第{chap}章"))),
                        )
                        .child(
                            div().flex_1().child(
                                Tag::custom(
                                    hsla(theme::PANEL2()),
                                    hsla(if col_cards.is_empty() { theme::RED() } else { theme::GREEN() }),
                                    hsla(theme::LINE()),
                                )
                                .outline()
                                .child(SharedString::from(if col_cards.is_empty() { "空" } else { "有排期" })),
                            ),
                        )
                        .child(
                            Button::new(SharedString::from(format!("dreview-{chap}")))
                                .xsmall()
                                .label("AI 核对")
                                .on_click(cx.listener(move |_, _, _, cx| {
                                    cx.emit(DirectorEvent::Review(chap));
                                })),
                        ),
                )
                .child(
                    div()
                        .px(theme::PAD_SM)
                        .py(theme::GAP_XS)
                        .text_size(theme::FONT_MICRO)
                        .line_height(relative(1.35))
                        .child(match stage_of(chap) {
                            Some((name, theme_name)) => div()
                .flex()
                .flex_col()
                .child(
                    div()
                        .font_family(theme::FAMILY_SERIF)
                        .font_weight(FontWeight::SEMIBOLD)
                        .text_color(theme::PURPLE())
                        .child(SharedString::from(name)),
                )
                .when(!theme_name.is_empty(), |d| {
                    d.child(
                        div().text_color(theme::FAINT()).child(SharedString::from(theme_name)),
                    )
                }),
                            None => div().text_color(theme::LINE()).child("—"),
                        }),
                )
                .child(
                    div()
                        .id(SharedString::from(format!("dbody-{i}")))
                        .flex()
                        .flex_col()
                        .gap(theme::GAP_SM)
                        .p(theme::PAD_SM)
                        .min_h(px(120.))
                        .children(col_cards.iter().map(|c| card_view(c, drag_active && self.drag.as_ref().map(|d| d.id.clone()) == Some(c.id.clone()), cx)))
                        .on_mouse_move(cx.listener(move |this, _, _, cx| {
                            if (this.drag.is_some() || this.pred_drag.is_some()) && this.hover != Some(i) {
                                this.hover = Some(i);
                                cx.notify();
                            }
                        }))
                        .on_mouse_up(MouseButton::Left, cx.listener(move |this, _, _, cx| {
                            if this.drag.is_some() || this.pred_drag.is_some() {
                                this.drop_to(i, cx);
                            }
                        })),
                );
            board.push(col.into_any_element());
        }

        // M8-3 预测候选列：虚线卡，拖入章列=晋升为排期提案（不直写台账）
        let mut pred_cards: Vec<AnyElement> = Vec::new();
        for (pi, pc) in self.preds.clone().into_iter().enumerate() {
            let dragging = self.pred_drag == Some(pi);
            pred_cards.push(
                div()
                    .id(SharedString::from(format!("pred-{}-{}", pc.line, pc.idx)))
                    .p(theme::PAD_SM)
                    .rounded(theme::RADIUS_PANEL)
                    .border_1()
                    .border_dashed()
                    .border_color(if dragging { theme::ACCENT_BORDER() } else { theme::GREEN_BORDER() })
                    .bg(theme::PANEL2())
                    .cursor_grab()
                    .on_mouse_down(
                        MouseButton::Left,
                        cx.listener(move |this, _, _, cx| {
                            this.pred_drag = Some(pi);
                            this.drag = None;
                            cx.notify();
                        }),
                    )
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap(theme::GAP_XS)
                            .child(div().text_size(theme::FONT_TAG).text_color(theme::GREEN()).child("◇"))
                            .child(
                                div()
                                    .flex_1()
                                    .min_w_0()
                                    .overflow_hidden()
                                    .whitespace_nowrap()
                                    .text_size(theme::FONT_TAG)
                                    .text_color(theme::TEXT())
                                    .child(SharedString::from(pc.label.clone())),
                            ),
                    )
                    .child(
                        div()
                            .font_family(theme::FAMILY_MONO)
                            .text_size(theme::FONT_MICRO)
                            .text_color(theme::FAINT())
                            .child(SharedString::from(format!("线{} · 采纳后入账", pc.line))),
                    )
                    .into_any_element(),
            );
        }
        if !pred_cards.is_empty() {
            board.push(
                div()
                    .w(px(COL_W))
                    .flex_shrink_0()
                    .flex()
                    .flex_col()
                    .rounded(theme::RADIUS_CARD)
                    .border_1()
                    .border_dashed()
                    .border_color(theme::GREEN_BORDER())
                    .bg(theme::PANEL())
                    .child(
                        div()
                            .px(theme::PAD_SM)
                            .py(theme::GAP_SM)
                            .text_size(theme::FONT_LABEL)
                            .font_weight(FontWeight::SEMIBOLD)
                            .text_color(theme::GREEN())
                            .child("预测候选（拖入=排期提案）"),
                    )
                    .child(
                        div().flex().flex_col().gap(theme::GAP_SM).p(theme::PAD_SM).children(pred_cards),
                    )
                    .into_any_element(),
            );
        }

        // 待排期池（窗口外存量；拖入即改期）
        let pool_cards: Vec<DCard> = self.pool.clone();
        let mut pool_body: Vec<AnyElement> = Vec::new();
        pool_body.push(
            div()
                .text_size(theme::FONT_MICRO)
                .text_color(theme::FAINT())
                .child(SharedString::from(format!("窗口外存量 {} 张", pool_cards.len())))
                .into_any_element(),
        );
        for c in pool_cards.iter().take(24) {
            pool_body.push(card_view(c, false, cx));
        }
        board.push(
            div()
                .w(px(COL_W))
                .flex_shrink_0()
                .flex()
                .flex_col()
                .rounded(theme::RADIUS_CARD)
                .border_1()
                .border_color(theme::LINE())
                .bg(theme::PANEL())
                .child(
                    div()
                        .px(theme::PAD_SM)
                        .py(theme::GAP_SM)
                        .text_size(theme::FONT_LABEL)
                        .font_weight(FontWeight::SEMIBOLD)
                        .text_color(theme::DIM())
                        .child("待排期（拖入窗口即改期）"),
                )
                .child(
                    div()
                        .flex()
                        .flex_col()
                        .gap(theme::GAP_SM)
                        .p(theme::PAD_SM)
                        .children(pool_body),
                )
                .into_any_element(),
        );

        div()
            .id("director-root")
            .size_full()
            .flex()
            .flex_col()
            .bg(theme::BG())
            .on_mouse_down_out(cx.listener(|this, _, _, cx| {
                // 在板外按下：清掉残留拖拽态（防止下一次点击章列误晋升旧卡）
                if this.drag.is_some() || this.pred_drag.is_some() || this.hover.is_some() {
                    this.drag = None;
                    this.pred_drag = None;
                    this.hover = None;
                    cx.notify();
                }
            }))
            .on_mouse_up_out(MouseButton::Left, cx.listener(|this, _, _, cx| {
                // 在板外松手：丢弃本次拖拽，不能留下 pred_drag/drag
                if this.drag.is_some() || this.pred_drag.is_some() {
                    this.drag = None;
                    this.pred_drag = None;
                    this.hover = None;
                    cx.notify();
                }
            }))
            .on_mouse_up(MouseButton::Left, cx.listener(|this, _, _, cx| {
                // 章列 body 的 on_mouse_up 先冒泡执行 drop_to；走到这里仍非空说明
                // 松手位置不在任何章列（表头/空白/预测候选列），直接丢弃。
                if this.drag.is_some() || this.pred_drag.is_some() {
                    this.drag = None;
                    this.pred_drag = None;
                    this.hover = None;
                    cx.notify();
                }
            }))
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(theme::GAP_MD)
                    .px(theme::PAD_MD)
                    .py(theme::GAP_SM)
                    .border_b_1()
                    .border_color(theme::LINE())
                    .bg(theme::PANEL())
                    .child(
                        div()
                            .text_size(theme::FONT_META)
                            .font_weight(FontWeight::SEMIBOLD)
                            .text_color(theme::TEXT())
                            .child("导演台 · 排期板"),
                    )
                    .child(
                        div().flex_1().text_size(theme::FONT_LABEL).text_color(theme::FAINT()).child(
                            "拖卡改期=作者亲笔直写台账；AI 只核对与提案",
                        ),
                    )
                    .when_some(msg, |d, m| {
                        d.child(
                            div()
                                .text_size(theme::FONT_LABEL)
                                .text_color(theme::GREEN())
                                .child(m),
                        )
                    })
                    .child(
                        Button::new("d-prev")
                            .xsmall()
                            .label("←")
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.current = this.current.saturating_sub(1);
                                if this.current == 0 {
                                    this.current = 1;
                                }
                                this.refresh();
                                cx.notify();
                            })),
                    )
                    .child(
                        Button::new("d-next")
                            .xsmall()
                            .label("→")
                            .on_click(cx.listener(|this, _, _, cx| {
                                // 右界 = 下一待写章：再往后只有空列与「无事件覆盖」噪音
                                let max = this.handle.next_chapter_number();
                                if this.current < max {
                                    this.current += 1;
                                    this.refresh();
                                    cx.notify();
                                }
                            })),
                    ),
            )
            .child(
                div()
                    .id("director-scroll")
                    .flex_1()
                    .min_h_0()
                    .overflow_x_scroll()
                    .child(
                        div()
                            .flex()
                            .items_start()
                            .gap(theme::GAP_GROUP)
                            .p(theme::PAD_MD)
                            .children(board),
                    ),
            )
            .child(
                div()
                    .flex()
                    .flex_wrap()
                    .items_center()
                    .gap(theme::GAP_SM)
                    .px(theme::PAD_MD)
                    .py(theme::GAP_SM)
                    .border_t_1()
                    .border_color(theme::LINE())
                    .bg(theme::PANEL())
                    .child(
                        div()
                            .text_size(theme::FONT_LABEL)
                            .text_color(theme::FAINT())
                            .child("合同预警"),
                    )
                    .when(warnings.is_empty(), |d| {
                        d.child(
                            div()
                                .text_size(theme::FONT_LABEL)
                                .text_color(theme::GREEN())
                                .child("✓ 窗口内无确定性预警"),
                        )
                    })
                    .children(warnings),
            )
    }
}

impl BasePanel for DirectorPanel {
    fn panel_name(&self) -> &'static str {
        "director-view"
    }
}

impl ComponentPanel for DirectorPanel {
    fn tab_name(&self, _: &App) -> Option<SharedString> {
        Some("导演台".into())
    }

    fn title(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div()
            .text_size(theme::FONT_BODY)
            .font_weight(FontWeight::MEDIUM)
            .text_color(theme::TEXT())
            .child("导演台")
    }
}
