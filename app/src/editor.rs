//! 中央编辑器面板（M1 占位稿）：章节头 + 正文占位 + 对 DSH 发话的输入条。
//!
//! 发送以事件上抛给 Workspace（订阅制，不直接持有 client）。

use gpui_kit::component::dock::PanelEvent;
use gpui_kit::component::dock::Panel as ComponentPanel;
use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::input::{Input, InputState};
use gpui_kit::component::dock::BasePanel;
use gpui_kit::{
    AnyElement, App, AppContext as _, Context, Entity, EventEmitter, FocusHandle, Focusable,
    FontWeight, InteractiveElement, StatefulInteractiveElement, IntoElement, ParentElement, Render, SharedString, Styled,
    Window, div, rgb, relative,
};

#[derive(Debug, Clone)]
pub enum EditorEvent {
    SendRequested(String),
}

pub struct EditorPanel {
    focus_handle: FocusHandle,
    input: Entity<InputState>,
}

impl EditorPanel {
    pub fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        let input = cx.new(|cx| {
            InputState::new(window, cx)
                .placeholder("对 AI 说点什么（M1 冒烟：原话发给 DSH，轨迹在右侧滚动）…")
        });
        Self {
            focus_handle: cx.focus_handle(),
            input,
        }
    }
}

impl EventEmitter<PanelEvent> for EditorPanel {}
impl EventEmitter<EditorEvent> for EditorPanel {}

impl BasePanel for EditorPanel {
    fn panel_name(&self) -> &'static str {
        "editor"
    }
}

impl ComponentPanel for EditorPanel {}

impl Focusable for EditorPanel {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus_handle.clone()
    }
}

impl Render for EditorPanel {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let input = self.input.clone();
        let send = Button::new("send-prompt")
            .primary()
            .label("发送")
            .on_click(cx.listener(move |this, _, window, cx| {
                let text = this.input.read(cx).value().to_string();
                if text.trim().is_empty() {
                    return;
                }
                cx.emit(EditorEvent::SendRequested(text));
                this.input.update(cx, |state, cx| {
                    state.set_value("", window, cx);
                });
            }));

        div()
            .flex()
            .flex_col()
            .size_full()
            .bg(rgb(0x17181c))
            .child(
                div()
                    .id("editor-scroll")
                    .flex()
                    .flex_col()
                    .flex_1()
                    .min_h_0()
                    .overflow_y_scroll()
                    .px_8()
                    .py_4()
                    .gap_3()
                    .child(
                        div()
                            .flex()
                            .items_baseline()
                            .gap_2()
                            .child(
                                div()
                                    .text_xl()
                                    .font_weight(FontWeight::MEDIUM)
                                    .text_color(rgb(0xd8d5cf))
                                    .child("第12章 · 断刀"),
                            )
                            .child(
                                div()
                                    .text_xs()
                                    .text_color(rgb(0x5d5b57))
                                    .child("第 3/40 章 · 3,214 字 · 写作中（M1 占位稿）"),
                            ),
                    )
                    .child(prose_para("沈砚按着刀柄走进雨里。巷口的灯笼在风里晃，把他的影子拉长又压短，像一根随时会断的线。三个月了，那把断刀始终没有离开过他的腰——刀是师父留下的，缺口处还沾着北境的铁锈。"))
                    .child(prose_para("酒肆里坐着的人比他想的要多。靠窗那桌在赌钱，押的是明日漕帮开仓的价；角落里两个短打汉子压着嗓子说话，说的却是「赤星军」三个字。沈砚要了一角酒，背对着他们坐下，耳朵却支棱着。"))
                    .child(prose_para("「听说了吗，雁回山那边的老观又燃了灯。」"))
                    .child(prose_para("「嘘——那灯三十年前就该灭了。」"))
                    .child(prose_para("沈砚的手指在刀柄上收紧了一分。老观燃灯，是师父当年留下的暗号：灯亮，就是有人回来了。他没有回头，只是把酒一口饮尽。")),
            )
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap_2()
                    .border_t_1()
                    .border_color(rgb(0x2e3138))
                    .px_3()
                    .py_2()
                    .bg(rgb(0x1e2026))
                    .child(div().flex_1().child(Input::new(&input)))
                    .child(send),
            )
    }
}

fn prose_para(text: &str) -> AnyElement {
    div()
        .w_full()
        .text_sm()
        .text_color(rgb(0xd8d5cf))
        .line_height(relative(1.9))
        .child(SharedString::from(text.to_string()))
        .into_any_element()
}
