//! 中央编辑器面板：真实正文（TextareaState，来自当前章节）+ 发话输入条。
//! 正文保存由 Workspace 在切章/回书架时读 state.value() 完成。

use gpui_kit::component::dock::PanelEvent;
use gpui_kit::component::dock::{BasePanel, Panel as ComponentPanel};
use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::input::{Input, InputState, Textarea, TextareaState};
use gpui_kit::component::Sizable;
use gpui_kit::{
    App, AppContext as _, Context, Entity, EventEmitter, FocusHandle, Focusable, InteractiveElement,
    IntoElement, ParentElement, Render, StatefulInteractiveElement, Styled, Window, div, rgb,
};

#[derive(Debug, Clone)]
pub enum EditorEvent {
    SendRequested(String),
    /// 显式保存（按钮）
    SaveRequested,
    /// AI 能力按钮（capability 键，Workspace 映射为任务书）
    RunCapability(String),
}

pub struct EditorPanel {
    focus_handle: FocusHandle,
    /// 当前章正文（Workspace 持有同一 Entity，用于读写与保存）
    pub prose: Entity<TextareaState>,
    prompt: Entity<InputState>,
}

impl EditorPanel {
    pub fn new(window: &mut Window, cx: &mut Context<Self>, prose: Entity<TextareaState>) -> Self {
        let prompt = cx.new(|cx| {
            InputState::new(window, cx)
                .placeholder("对 AI 说点什么（原话发给 DSH，轨迹在右侧滚动）…")
        });
        Self { focus_handle: cx.focus_handle(), prose, prompt }
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
        let prose = self.prose.clone();
        let prompt = self.prompt.clone();

        let save = Button::new("save-prose")
            .label("保存")
            .on_click(cx.listener(|_this, _, _, cx| {
                cx.emit(EditorEvent::SaveRequested);
            }));
        let send = Button::new("send-prompt")
            .primary()
            .label("发送")
            .on_click(cx.listener(move |this, _, window, cx| {
                let text = this.prompt.read(cx).value().to_string();
                if text.trim().is_empty() {
                    return;
                }
                cx.emit(EditorEvent::SendRequested(text));
                this.prompt.update(cx, |state, cx| {
                    state.set_value("", window, cx);
                });
            }));

        let ai_buttons = crate::prompts::EDITOR_AI_BUTTONS
            .iter()
            .map(|b| {
                let cap = b.capability.to_string();
                Button::new(format!("ai-{cap}"))
                    .xsmall()
                    .label(b.label)
                    .on_click(cx.listener(move |_this, _, _, cx| {
                        cx.emit(EditorEvent::RunCapability(cap.clone()));
                    }))
            })
            .collect::<Vec<_>>();

        div()
            .flex()
            .flex_col()
            .size_full()
            .bg(rgb(0x17181c))
            .child(
                div()
                    .flex()
                    .gap_2()
                    .items_center()
                    .px_8()
                    .pt_2()
                    .flex_wrap()
                    .children(ai_buttons),
            )
            .child(
                div()
                    .id("editor-scroll")
                    .flex()
                    .flex_col()
                    .flex_1()
                    .min_h_0()
                    .overflow_y_scroll()
                    .px_8()
                    .py_3()
                    .gap_2()
                    .child(div().flex_1().min_h_24().child(Textarea::new(&prose))),
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
                    .child(div().flex_1().child(Input::new(&prompt)))
                    .child(save)
                    .child(send),
            )
    }
}
