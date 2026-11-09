//! 中央编辑器面板：真实正文（TextareaState，来自当前章节）+ 发话输入条。
//! 正文保存由 Workspace 在切章/回书架时读 state.value() 完成。

use gpui_kit::component::dock::PanelEvent;
use gpui_kit::component::dock::{BasePanel, Panel as ComponentPanel};
use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::input::{Input, InputEvent, InputState, Textarea, TextareaState};
use gpui_kit::component::Sizable;
use gpui_kit::{
    App, AppContext as _, Context, Entity, EventEmitter, FocusHandle, Focusable, InteractiveElement,
    IntoElement, ParentElement, Render, SharedString, StatefulInteractiveElement, Styled, Window,
    div, px, rgb,
};

#[derive(Debug, Clone)]
pub enum EditorEvent {
    SendRequested(String),
    /// 显式保存（按钮）
    SaveRequested,
    /// AI 能力按钮（capability 键，Workspace 映射为任务书）
    RunCapability(String),
    /// 选区创作（kind 键；Workspace 读选区文本拼任务书）
    SelectionAction(&'static str),
}

pub struct EditorPanel {
    focus_handle: FocusHandle,
    /// 当前章正文（Workspace 持有同一 Entity，用于读写与保存）
    pub prose: Entity<TextareaState>,
    prompt: Entity<InputState>,
    pub word_target: u32,
}

impl EditorPanel {
    pub fn new(window: &mut Window, cx: &mut Context<Self>, prose: Entity<TextareaState>, word_target: u32) -> Self {
        let prompt = cx.new(|cx| {
            InputState::new(window, cx)
                .placeholder("对 AI 说点什么（原话发给 DSH，轨迹在右侧滚动）…")
        });
        let panel = Self { focus_handle: cx.focus_handle(), prose: prose.clone(), prompt, word_target };
        // 正文一变就重渲染（字数/进度实时）
        cx.subscribe(&prose, |_, _, _: &InputEvent, cx| cx.notify()).detach();
        panel
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

        // 实时字数与目标进度（渲染期计算：章级体量可忽略）
        let text = self.prose.read(cx).value().to_string();
        let words = crate::store::count_words(&text);
        let target = self.word_target.max(1);
        let pct = ((words as f32 / target as f32) * 100.).min(100.) as u32;

        div()
            .flex()
            .flex_col()
            .size_full()
            .bg(rgb(0x17181c))
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap_3()
                    .px_8()
                    .pt_3()
                    .child(
                        div()
                            .text_xs()
                            .text_color(rgb(0x8e8b85))
                            .child(SharedString::from(format!("{words} / {target} 字"))),
                    )
                    .child(
                        div()
                            .w(px(120.))
                            .h(px(5.))
                            .rounded(px(3.))
                            .bg(rgb(0x24262d))
                            .child(
                                div()
                                    .w(px(120. * pct as f32 / 100.))
                                    .h(px(5.))
                                    .rounded(px(3.))
                                    .bg(rgb(0xc96f4a)),
                            ),
                    )
                    .child(
                        div()
                            .text_xs()
                            .text_color(rgb(0x5d5b57))
                            .child(SharedString::from(format!("{pct}%"))),
                    ),
            )
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
                    .flex()
                    .gap_2()
                    .items_center()
                    .px_8()
                    .pt_1()
                    .flex_wrap()
                    .child(
                        div()
                            .text_xs()
                            .text_color(rgb(0x5d5b57))
                            .child("选区创作（先选中正文一段）："),
                    )
                    .children(crate::prompts::SELECTION_BUTTONS.iter().map(|(label, kind)| {
                        let kind: &'static str = kind;
                        Button::new(format!("sel-{kind}"))
                            .xsmall()
                            .label(*label)
                            .on_click(cx.listener(move |_this, _, _, cx| {
                                cx.emit(EditorEvent::SelectionAction(kind));
                            }))
                    })),
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
                    .child(
                        div()
                            .flex()
                            .justify_center()
                            .w_full()
                            .child(
                                div()
                                    .w_full()
                                    .max_w(px(680.))
                                    .rounded_lg()
                                    .bg(rgb(0x1b1d22))
                                    .border_1()
                                    .border_color(rgb(0x2e3138))
                                    .px_6()
                                    .py_4()
                                    .child(
                                        Textarea::new(&prose)
                                            .bordered(false)
                                            .appearance(false),
                                    ),
                            ),
                    ),
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
