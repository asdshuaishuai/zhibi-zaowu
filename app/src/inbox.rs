//! 收件箱面板（右侧 Dock 第二页签）：待裁决提案就地采纳/拒收。

use std::rc::Rc;

use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::dock::{BasePanel, Panel as ComponentPanel, PanelEvent};
use gpui_kit::component::input::{Input, InputState};
use gpui_kit::component::Sizable;
use gpui_kit::{
    App, AppContext as _, Context, Entity, EventEmitter, FocusHandle, Focusable,
    FontWeight, InteractiveElement, IntoElement, ParentElement, Render, SharedString,
    StatefulInteractiveElement, Styled, Window, div, rgb,
};

use crate::store::{ProjectHandle, Proposal};

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
                    .flex()
                    .flex_col()
                    .gap_1()
                    .w_full()
                    .p_2()
                    .rounded_md()
                    .bg(rgb(0x24262d))
                    .border_l_2()
                    .border_color(rgb(0xc96f4a))
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap_2()
                            .child(
                                div()
                                    .text_xs()
                                    .font_weight(FontWeight::SEMIBOLD)
                                    .text_color(rgb(0xc96f4a))
                                    .child(SharedString::from(p.capability.clone())),
                            )
                            .child(
                                div()
                                    .text_xs()
                                    .text_color(rgb(0xd8d5cf))
                                    .child(SharedString::from(p.title.clone())),
                            ),
                    )
                    .child(
                        div()
                            .text_xs()
                            .text_color(rgb(0x8e8b85))
                            .child(SharedString::from(summary)),
                    )
                    .child(
                        div()
                            .flex()
                            .gap_2()
                            .items_center()
                            .child(
                                Button::new(format!("accept-{accept_id}"))
                                    .primary()
                                    .xsmall()
                                    .label("采纳")
                                    .on_click(cx.listener(move |_this, _, _, cx| {
                                        cx.emit(InboxEvent::Accept(accept_id.clone()));
                                    })),
                            )
                            .child(
                                Button::new(format!("reject-{reject_id}"))
                                    .xsmall()
                                    .label("拒收")
                                    .on_click(cx.listener(move |this, _, _, cx| {
                                        let reason = this.reason.read(cx).value().to_string();
                                        cx.emit(InboxEvent::Reject(
                                            reject_id.clone(),
                                            if reason.trim().is_empty() {
                                                "（未填理由）".into()
                                            } else {
                                                reason
                                            },
                                        ));
                                    })),
                            )
                            .child(
                                div()
                                    .text_xs()
                                    .text_color(rgb(0x5d5b57))
                                    .child(SharedString::from(format!("kind: {kind_label}"))),
                            ),
                    ),
            );
        }

        div()
            .id("inbox-scroll")
            .size_full()
            .overflow_y_scroll()
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap_2()
                    .p_2()
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap_2()
                            .child(
                                div()
                                    .text_sm()
                                    .font_weight(FontWeight::MEDIUM)
                                    .child(SharedString::from(format!(
                                        "待裁决 {} · 已处理 {}",
                                        pending.len(),
                                        decided_n
                                    ))),
                            )
                            .child(
                                Button::new("inbox-refresh")
                                    .ghost()
                                    .xsmall()
                                    .label("刷新")
                                    .on_click(cx.listener(|_this, _, _, cx| {
                                        cx.emit(InboxEvent::Refresh);
                                    })),
                            ),
                    )
                    .child(div().w_full().child(Input::new(&self.reason)))
                    .children(cards)
                    .children(if pending.is_empty() {
                        vec![div()
                            .pt_6()
                            .text_center()
                            .text_xs()
                            .text_color(rgb(0x5d5b57))
                            .child("没有待裁决提案——AI 产出登记后会出现在这里。")
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
        div().text_sm().font_weight(FontWeight::MEDIUM).child("提案收件箱")
    }
}
