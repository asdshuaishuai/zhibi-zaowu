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
    ParentElement, Render, SharedString, StatefulInteractiveElement, Styled, Window, div, px,
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
            drag: None,
            hover: None,
            message: None,
        };
        this.refresh();
        this
    }

    /// 从台账重建排期卡；窗口 [current, current+COLS)。
    pub fn refresh(&mut self) {
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
            };
            if ch >= self.current && ch < end {
                cards.push(card);
            } else {
                pool.push(card);
            }
        }
        self.cards = cards;
        self.pool = pool;
        self.drag = None;
        self.hover = None;
    }

    fn drop_to(&mut self, col: usize, cx: &mut Context<Self>) {
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
        for (i, _) in (0..COLS).enumerate() {
            let chap = self.current + i as u32;
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
        .border_color(if dragging { theme::ACCENT_BORDER() } else { theme::LINE() })
        .bg(theme::PANEL2())
        .cursor_grab()
        .on_mouse_down(
            MouseButton::Left,
            cx.listener(move |this, _: &MouseDownEvent, _, cx| {
                this.drag = Some(c.clone());
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
                .font_family(theme::FAMILY_MONO)
                .text_size(theme::FONT_MICRO)
                .text_color(theme::FAINT())
                .child(SharedString::from(card.id.clone())),
        )
        .into_any_element()
}

impl Render for DirectorPanel {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let warnings = self.warnings();
        let msg = self.message.clone();
        let drag_active = self.drag.is_some();

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
                        .id(SharedString::from(format!("dbody-{i}")))
                        .flex()
                        .flex_col()
                        .gap(theme::GAP_SM)
                        .p(theme::PAD_SM)
                        .min_h(px(120.))
                        .children(col_cards.iter().map(|c| card_view(c, drag_active && self.drag.as_ref().map(|d| d.id.clone()) == Some(c.id.clone()), cx)))
                        .on_mouse_move(cx.listener(move |this, _, _, cx| {
                            if this.drag.is_some() && this.hover != Some(i) {
                                this.hover = Some(i);
                                cx.notify();
                            }
                        }))
                        .on_mouse_up(MouseButton::Left, cx.listener(move |this, _, _, cx| {
                            if this.drag.is_some() {
                                this.drop_to(i, cx);
                            }
                        })),
                );
            board.push(col.into_any_element());
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
            .size_full()
            .flex()
            .flex_col()
            .bg(theme::BG())
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
                                this.current += 1;
                                this.refresh();
                                cx.notify();
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
