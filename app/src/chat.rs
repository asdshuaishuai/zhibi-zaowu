//! 对话页签（右侧 Dock 第三页）：自由问答，可读全书、仍只产提案。
//! 消息流与轨迹窗共享同一条 session——这里呈现的是 user/assistant 文本对。

use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::dock::{BasePanel, Panel as ComponentPanel, PanelEvent};
use gpui_kit::component::input::{Input, InputState};
use gpui_kit::component::Sizable;
use gpui_kit::{
    App, AppContext as _, Context, Entity, EventEmitter, FocusHandle, Focusable, FontWeight,
    InteractiveElement, IntoElement, ParentElement, Render, SharedString,
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
}

impl ChatPanel {
    pub fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        let input = cx.new(|cx| {
            InputState::new(window, cx).placeholder("问点什么（可读全书，产出仍只提案）…")
        });
        Self { focus_handle: cx.focus_handle(), log: Vec::new(), input, busy: false }
    }

    pub fn push(&mut self, msg: ChatMsg, cx: &mut Context<Self>) {
        self.log.push(msg);
        if self.log.len() > 120 {
            self.log.drain(..self.log.len() - 120);
        }
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

        let mut rows = Vec::new();
        for m in self.log.iter() {
            let user = m.role == "user";
            let mut row = div().flex().w_full();
            row = if user { row.justify_end() } else { row };
            rows.push(
                row
                    .child(
                        div()
                            .max_w(px(260.))
                            .px(theme::PAD_MD)
                            .py(theme::PAD_SM)
                            .rounded(theme::RADIUS_PANEL)
                            .bg(if user {
                                theme::ACCENT_SOFT()
                            } else {
                                theme::PANEL2()
                            })
                            .border_1()
                            .border_color(if user {
                                theme::ACCENT_BORDER()
                            } else {
                                theme::LINE()
                            })
                            .text_size(theme::FONT_SMALL)
                            .line_height(relative(theme::LINE_HEIGHT_BODY))
                            .text_color(theme::TEXT())
                            .child(SharedString::from(m.text.clone())),
                    ),
            );
        }
        if rows.is_empty() {
            rows.push(
                div()
                    .w_full()
                    .pt(theme::PAD_XL)
                    .text_center()
                    .text_size(theme::FONT_LABEL)
                    .text_color(theme::FAINT())
                    .child("问 AI 任何关于这本书的问题——它读得到全书，回答之外不改任何东西。"),
            );
        }

        let send = Button::new("chat-send")
            .primary()
            .xsmall()
            .label(if busy { "思考中…" } else { "发送" })
            .on_click(cx.listener(move |this, _, _, cx| {
                let text = this.input.read(cx).value().to_string();
                if text.trim().is_empty() {
                    return;
                }
                cx.emit(ChatEvent::Send(text));
            }));

        div()
            .id("chat-scroll")
            .size_full()
            .overflow_y_scroll()
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap(theme::GAP_MD)
                    .p(theme::PAD_MD)
                    .children(rows)
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap(theme::GAP_SM)
                            .pt(theme::PAD_SM)
                            .child(div().flex_1().min_w_0().child(Input::new(&input)))
                            .child(send),
                    ),
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
