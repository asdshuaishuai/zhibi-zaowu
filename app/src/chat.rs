//! 对话页签（右侧 Dock 第三页）：自由问答，可读全书、仍只产提案。
//! 消息流与轨迹窗共享同一条 session——这里呈现的是 user/assistant 文本对。

use gpui_kit::component::bubble::{Bubble, BubbleVariant};
use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::message::MessageAlignment;
use gpui_kit::component::dock::{BasePanel, Panel as ComponentPanel, PanelEvent};
use gpui_kit::component::input::{Input, InputState};
use gpui_kit::prelude::FluentBuilder;
use gpui_kit::component::Sizable;
use gpui_kit::{
    App, AppContext as _, Context, Entity, EventEmitter, FocusHandle, Focusable, FontWeight,
    InteractiveElement, IntoElement, ParentElement, Render, ScrollHandle, SharedString,
    StatefulInteractiveElement, Styled, Window, div, px, relative,
};

use crate::theme;

#[derive(Debug, Clone)]
pub enum ChatEvent {
    Send(String),
}

#[derive(Clone)]
pub struct ChatMsg {
    /// "user" | "assistant"
    pub role: String,
    pub text: String,
}

pub struct ChatPanel {
    focus_handle: FocusHandle,
    log: Vec<ChatMsg>,
    input: Entity<InputState>,
    busy: bool,
    scroll: ScrollHandle,
}

impl ChatPanel {
    pub fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        let input = cx.new(|cx| {
            InputState::new(window, cx).placeholder("问点什么（可读全书，产出仍只提案）…")
        });
        Self {
            focus_handle: cx.focus_handle(),
            log: Vec::new(),
            input,
            busy: false,
            scroll: ScrollHandle::new(),
        }
    }

    pub fn push(&mut self, msg: ChatMsg, cx: &mut Context<Self>) {
        self.log.push(msg);
        if self.log.len() > 120 {
            self.log.drain(..self.log.len() - 120);
        }
        self.scroll.scroll_to_bottom();
        cx.notify();
    }

    pub fn set_busy(&mut self, busy: bool, cx: &mut Context<Self>) {
        self.busy = busy;
        cx.notify();
    }
}

impl EventEmitter<PanelEvent> for ChatPanel {}
impl EventEmitter<ChatEvent> for ChatPanel {}

impl Focusable for ChatPanel {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus_handle.clone()
    }
}

impl Render for ChatPanel {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let input = self.input.clone();
        let busy = self.busy;

        let mut rows: Vec<gpui_kit::AnyElement> = Vec::new();
        if self.log.is_empty() {
            rows.push(
                div()
                    .w_full()
                    .flex()
                    .flex_col()
                    .items_center()
                    .gap(theme::GAP_MD)
                    .pt(px(56.))
                    .px(theme::PAD_LG)
                    .child(
                        div()
                            .w(px(44.))
                            .h(px(44.))
                            .flex()
                            .items_center()
                            .justify_center()
                            .rounded(theme::RADIUS_CHIP)
                            .bg(theme::ACCENT_SOFT())
                            .border_1()
                            .border_color(theme::ACCENT_BORDER())
                            .text_size(theme::FONT_TITLE_SM)
                            .text_color(theme::ACCENT())
                            .child("问"),
                    )
                    .child(
                        div()
                            .text_size(theme::FONT_BODY)
                            .font_weight(FontWeight::MEDIUM)
                            .text_color(theme::TEXT())
                            .child("和 AI 聊这本书"),
                    )
                    .child(
                        div()
                            .max_w(px(280.))
                            .text_center()
                            .text_size(theme::FONT_LABEL)
                            .line_height(relative(theme::LINE_HEIGHT_BODY))
                            .text_color(theme::DIM())
                            .child("它读得到全书设定/大纲/伏笔与成稿；问答之外不改任何东西——要改，它只会提提案。"),
                    )
                    .child(
                        div()
                            .font_family(theme::FAMILY_MONO)
                            .text_size(theme::FONT_TAG)
                            .text_color(theme::FAINT())
                            .child("例：老周的动机目前立得住吗？"),
                    )
                    .into_any_element(),
            );
        } else {
            for m in self.log.iter() {
                let user = m.role == "user";
                rows.push(
                    div()
                        .w_full()
                        .flex()
                        .flex_col()
                        .items_start()
                        .gap(px(3.))
                        .when(user, |d| d.items_end())
                        .child(
                            div()
                                .px(px(2.))
                                .text_size(theme::FONT_MICRO)
                                .font_family(theme::FAMILY_MONO)
                                .text_color(if user { theme::ACCENT() } else { theme::FAINT() })
                                .child(if user { "你" } else { "AI · 只提案" }),
                        )
                        .child(
                            Bubble::new()
                                .with_variant(if user {
                                    BubbleVariant::Tinted
                                } else {
                                    BubbleVariant::Secondary
                                })
                                .alignment(if user {
                                    MessageAlignment::End
                                } else {
                                    MessageAlignment::Start
                                })
                                .bg(if user { theme::ACCENT_SOFT() } else { theme::PANEL2() })
                                .border_1()
                                .border_color(if user { theme::ACCENT_BORDER() } else { theme::LINE() })
                                .rounded(theme::RADIUS_CARD)
                                .max_w(px(280.))
                                .child(
                                    div()
                                        .px(px(2.))
                                        .py(px(2.))
                                        .text_size(theme::FONT_SMALL)
                                        .line_height(relative(theme::LINE_HEIGHT_BODY))
                                        .text_color(theme::TEXT())
                                        .child(SharedString::from(m.text.clone())),
                                ),
                        )
                        .into_any_element(),
                );
            }
        }

        let send = Button::new("chat-send")
            .primary()
            .xsmall()
            .label(if busy { "思考中…" } else { "发送" })
            .on_click(cx.listener(|this, _, _, cx| {
                let text = this.input.read(cx).value().to_string();
                if text.trim().is_empty() {
                    return;
                }
                cx.emit(ChatEvent::Send(text));
            }));

        div()
            .size_full()
            .flex()
            .flex_col()
            .bg(theme::BG())
            .child(
                div()
                    .id("chat-scroll")
                    .flex_1()
                    .min_h_0()
                    .overflow_y_scroll()
                    .track_scroll(&self.scroll)
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .gap(theme::GAP_LG)
                            .p(theme::PAD_MD)
                            .children(rows),
                    ),
            )
            // —— 底部固定输入条（与滚动区分离）
            .child(
                div()
                    .flex_shrink_0()
                    .flex()
                    .items_center()
                    .gap(theme::GAP_SM)
                    .px(theme::PAD_MD)
                    .py(theme::GAP_SM)
                    .border_t_1()
                    .border_color(theme::LINE())
                    .bg(theme::PANEL())
                    .child(div().flex_1().min_w_0().child(Input::new(&input)))
                    .child(send),
            )
    }
}

impl BasePanel for ChatPanel {
    fn panel_name(&self) -> &'static str {
        "chat-view"
    }
}

impl ComponentPanel for ChatPanel {
    fn tab_name(&self, _: &App) -> Option<SharedString> {
        Some("对话".into())
    }

    fn title(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div()
            .text_size(theme::FONT_SMALL)
            .font_weight(FontWeight::MEDIUM)
            .child("对话")
    }
}
