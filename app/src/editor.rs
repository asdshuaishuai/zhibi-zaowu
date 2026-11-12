//! 中央编辑器面板：真实正文（TextareaState，来自当前章节）+ 发话输入条。
//! 正文保存由 Workspace 在切章/回书架时读 state.value() 完成。
//!
//! 视觉全部走 `crate::theme` 令牌（墨底/面板/朱砂/状态四色 + 栏内 20px 节奏）：
//! gpui-component 走内置 light 主题，所以每个控件在这里显式覆写底色/描边/文字色
//! （Button 用 custom 变体 + 实例样式，Input/Textarea 用 Styled refine，二者在库内
//! 都是最后合入，能盖住主题派生值）。

use gpui_kit::component::button::{Button, ButtonCustomVariant, ButtonVariants as _};
use gpui_kit::prelude::FluentBuilder;
use gpui_kit::component::dock::PanelEvent;
use gpui_kit::component::dock::{BasePanel, Panel as ComponentPanel};
use gpui_kit::component::input::{Input, InputEvent, InputState, Textarea, TextareaState};
use gpui_kit::component::theme::Colorize;
use gpui_kit::component::{Disableable, Sizable};
use gpui_kit::{
    App, AppContext as _, Context, ElementId, Entity, EventEmitter, FocusHandle, Focusable,
    FontWeight, Hsla, InteractiveElement, IntoElement, ParentElement, Render, Rgba, SharedString,
    StatefulInteractiveElement, Styled, Window, div, px, relative,
};

use crate::theme;

#[derive(Debug, Clone)]
pub enum EditorEvent {
    SendRequested(String),
    /// 显式保存（按钮）
    SaveRequested,
    /// AI 能力按钮（capability 键，Workspace 映射为任务书）
    RunCapability(String),
    /// 选区创作（kind 键；Workspace 读选区文本拼任务书）
    SelectionAction(&'static str),
    /// 分支卡：采纳第 ix 个走向（快照后并入正文）
    AdoptBranch(usize),
    /// 分支卡：换一批
    RerollBranches,
    /// 分支卡：全部不要
    DismissBranches,
}

pub struct EditorPanel {
    focus_handle: FocusHandle,
    /// 当前章正文（Workspace 持有同一 Entity，用于读写与保存）
    pub prose: Entity<TextareaState>,
    prompt: Entity<InputState>,
    pub word_target: u32,
    /// 章标题（Workspace 切章时 set_value）
    pub title: Entity<InputState>,
    /// 速记（随手记，喂「让AI记一笔」）
    pub notes: Entity<InputState>,
    last_saved: String,
    /// 分支卡（原型三选一）：AI 产出的走向列表，采纳一个即并入正文
    pub branches: Vec<String>,
    /// 选区状态（kit 无 SelectionChanged 事件 → 220ms 轮询 selected_text 回填）
    has_sel: bool,
    sel_chars: usize,
    /// AI 参与过的段落（正文偏移）。只画左缘墨点，不改正文一个字节。
    pub ai_marks: Vec<crate::journal::AiMark>,
}

impl EditorPanel {
    pub fn new(window: &mut Window, cx: &mut Context<Self>, prose: Entity<TextareaState>, word_target: u32) -> Self {
        let prompt = cx.new(|cx| {
            InputState::new(window, cx)
                .placeholder("对 AI 说点什么（原话发给 DSH，轨迹在右侧滚动）…")
        });
        let title = cx.new(|cx| InputState::new(window, cx).placeholder("章标题"));
        prose.update(cx, |state, cx| {
            state.set_placeholder(
                "这一章还没动笔——先写下第一段，或让 AI 搭骨架（提案等你批准）…",
                window,
                cx,
            );
        });
        let notes = cx.new(|cx| InputState::new(window, cx).placeholder("速记：埋了什么、要改什么、下一步想写什么…（「让AI记一笔」会带上）"));
        // 发话框一变就重渲染：发送按钮的空文本置灰态需要跟着实时刷新
        cx.subscribe(&prompt, |_, _, _: &InputEvent, cx| cx.notify()).detach();
        let panel = Self { focus_handle: cx.focus_handle(), prose: prose.clone(), prompt, word_target, title, notes, last_saved: String::new(), branches: Vec::new(), has_sel: false, sel_chars: 0, ai_marks: Vec::new() };
        // 正文一变就重渲染（字数/进度实时）
        cx.subscribe(&prose, |_, _, _: &InputEvent, cx| cx.notify()).detach();
        // 选区轮询：库无 SelectionChanged 事件，但 selected_text 可读——
        // 220ms 比对一次，浮现/收起「选区创作」浮条（鼠标划选、Shift+方向都盖住）
        let weak = cx.weak_entity();
        cx.spawn(async move |_, cx| {
            loop {
                cx.background_executor().timer(std::time::Duration::from_millis(220)).await;
                let alive = weak
                    .update(cx, |this, cx| {
                        let n = this.prose.read(cx).selected_text().len_chars();
                        if n != this.sel_chars {
                            this.sel_chars = n;
                            let now = n > 0;
                            if now != this.has_sel {
                                this.has_sel = now;
                                cx.notify();
                            }
                        }
                    })
                    .is_ok();
                if !alive {
                    break;
                }
            }
        })
        .detach();
        panel
    }
}

impl EditorPanel {
    pub fn set_saved_at(&mut self, stamp: String, cx: &mut Context<Self>) {
        self.last_saved = stamp;
        cx.notify();
    }
}

impl EditorPanel {
    pub fn set_branches(&mut self, branches: Vec<String>, cx: &mut Context<Self>) {
        self.branches = branches;
        cx.notify();
    }

    /// 刷新 AI 参与标记（切章/撤销后调用）。
    pub fn set_ai_marks(&mut self, marks: Vec<crate::journal::AiMark>, cx: &mut Context<Self>) {
        self.ai_marks = marks;
        cx.notify();
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

        // ── 实时字数与目标进度（渲染期计算：章级体量可忽略）
        let text = self.prose.read(cx).value().to_string();
        let words = crate::store::count_words(&text);
        let target = self.word_target.max(1);
        let pct = ((words as f32 / target as f32) * 100.).min(100.) as u32;
        let dialogue = dialogue_share(&text);
        let on_target = pct >= 100;

        // ── 发话框内容决定发送按钮的可点态（空文本置灰，不再静默吞掉点击）
        let prompt_text = self.prompt.read(cx).value().to_string();
        let prompt_empty = prompt_text.trim().is_empty();
        let accent_h: Hsla = theme::ACCENT().into();
        let panel2_h: Hsla = theme::PANEL2().into();

        let save = Button::new("save-prose")
            .small()
            .rounded(theme::RADIUS_BTN)
            .px(theme::PAD_MD)
            .child(tool_label("保存"))
            .accessibility_label("保存")
            // 次要动作：透明无边框（原型 .za-btn.gh），hover 才有 PANEL2 底
            .custom(
                ButtonCustomVariant::new(&*cx)
                    .foreground(theme::DIM().into())
                    .hover(theme::PANEL2().into())
                    .active(panel2_h.darken(0.05)),
            )
            .text_color(theme::DIM())
            .on_click(cx.listener(|_this, _, _, cx| {
                cx.emit(EditorEvent::SaveRequested);
            }));

        let send_variant = if prompt_empty {
            ButtonCustomVariant::new(&*cx)
                .color(theme::LINE().into())
                .foreground(theme::GHOST().into())
                .hover(theme::PANEL2().into())
                .active(panel2_h.darken(0.05))
        } else {
            ButtonCustomVariant::new(&*cx)
                .color(accent_h)
                .foreground(theme::ON_ACCENT().into())
                .hover(accent_h.lighten(0.08))
                .active(accent_h.darken(0.08))
        };
        let send_base = Button::new("send-prompt")
            .small()
            .rounded(theme::RADIUS_BTN)
            .px(theme::PAD_MD)
            .child(tool_label("发送"))
            .accessibility_label("发送")
            .custom(send_variant)
            .disabled(prompt_empty)
            .border_1()
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
        let send = if prompt_empty {
            send_base
                .bg(theme::PANEL2())
                .border_color(theme::LINE())
                .text_color(theme::GHOST())
                .tooltip("先在输入框里写下要对 AI 说的话")
        } else {
            send_base
                .bg(theme::ACCENT())
                .border_color(theme::ACCENT())
                .text_color(theme::ON_ACCENT())
        };

        // ── AI 能力条：✦ 主能力 = 朱砂软底 + 朱砂描边，其余 = 中性工具（PANEL2 + LINE）
        let ai_buttons = crate::prompts::EDITOR_AI_BUTTONS
            .iter()
            .map(|b| {
                let cap = b.capability.to_string();
                let id = format!("ai-{cap}");
                let button = if b.label.starts_with('✦') {
                    accent_button(id, b.label, &*cx)
                } else {
                    // 工具条＝PANEL 底，次级块用 PANEL2 才亮得起来（层次铁律）
                    neutral_button(id, b.label, theme::PANEL2(), theme::DIM(), &*cx)
                };
                button.on_click(cx.listener(move |_this, _, _, cx| {
                    cx.emit(EditorEvent::RunCapability(cap.clone()));
                }))
            })
            .collect::<Vec<_>>();

        // ── 页头（原型 .za-edhead）：宋体 15/600 章标题 + 状态徽标 + 右侧保存回执
        let saved_chip = if self.last_saved.is_empty() {
            chip(
                SharedString::from("未保存 · 点保存或切章自动落盘"),
                theme::AMBER(),
                theme::AMBER().opacity(0.35),
                theme::AMBER_SOFT(),
            )
        } else {
            chip(
                SharedString::from(format!("✓ 已自动保存 {}", self.last_saved)),
                theme::GREEN(),
                theme::GREEN_BORDER(),
                theme::GREEN_SOFT(),
            )
        };
        let head = div()
            .flex()
            .items_center()
            .gap(theme::GAP_LG)
            .px(theme::PAD_XL)
            .pt(theme::PAD_MD)
            .pb(theme::GAP_MD)
            .child(
                div()
                    .w(px(280.))
                    .min_w_0()
                    // 章标题＝页头标题层级（宋体 15/600），去掉输入框白底黑框
                    .child(
                        Input::new(&self.title)
                            .appearance(false)
                            .px(theme::PAD_XS)
                            .text_size(theme::FONT_TITLE_SM)
                            .font_weight(FontWeight::SEMIBOLD)
                            .font_family(theme::FAMILY_SERIF)
                            .text_color(theme::TEXT()),
                    ),
            )
            .child(chip(
                SharedString::from("写作中"),
                theme::GREEN(),
                theme::GREEN_BORDER(),
                theme::BG(),
            ))
            .child(div().flex_1())
            .child(saved_chip);

        // ── 统计行：主数字 DIM / 目标与单位 FAINT，进度用 BAR token + 语义 chip
        let stats = div()
            .flex()
            .items_center()
            .gap(theme::GAP_MD)
            .px(theme::PAD_XL)
            .pb(theme::GAP_GROUP_SM)
            .child(
                div()
                    .text_size(theme::FONT_LABEL)
                    .text_color(theme::DIM())
                    .child(SharedString::from(count_text(words as u32))),
            )
            .child(
                div()
                    .text_size(theme::FONT_LABEL)
                    .text_color(theme::FAINT())
                    .child(SharedString::from(format!(" / {} 字", count_text(target)))),
            )
            .child(
                // kit Progress：线性进度条（规则 6：达标绿 / 进行中朱砂）
                gpui_kit::component::progress::Progress::new("word-progress")
                    .value(pct as f32 / 100.)
                    .color({
                        let c: gpui_kit::Hsla = if on_target {
                            theme::GREEN().into()
                        } else {
                            theme::ACCENT().into()
                        };
                        c
                    })
                    .w(px(120.)),
            )
            .child(chip(
                SharedString::from(format!("{pct}%")),
                if on_target { theme::GREEN() } else { theme::ACCENT() },
                if on_target { theme::GREEN_BORDER() } else { theme::ACCENT_BORDER() },
                theme::BG(),
            ))
            .child(chip(
                SharedString::from(format!("对白占比 {dialogue}%")),
                theme::DIM(),
                theme::LINE(),
                theme::CHIP_NEUTRAL(),
            ));

        // ── 速记行：分节靠 border-top + 12px 呼吸，输入框＝PANEL2 + LINE
        let notes_row = div()
            .flex()
            .items_center()
            .gap(theme::GAP_MD)
            .px(theme::PAD_XL)
            .py(theme::PAD_SM)
            .border_t_1()
            .border_color(theme::LINE())
            .child(
                div()
                    .text_size(theme::FONT_LABEL)
                    .text_color(theme::FAINT())
                    .child("速记"),
            )
            .child(div().flex_1().min_w_0().child(ink_input(&self.notes)));

        // ── 能力条（原型 .za-edtools：PANEL 底 + 顶部描边，比稿纸亮一档）
        let tools_row = div()
            .flex()
            .items_center()
            .gap(theme::GAP_SM)
            .flex_wrap()
            .px(theme::PAD_XL)
            .py(theme::PAD_SM)
            .bg(theme::PANEL())
            .border_t_1()
            .border_color(theme::LINE())
            .children(ai_buttons);

        // ── 选区创作：做成浮起的一叠面板（原型 .za-selmenu 观感），与能力条区分开；
        //    首选项「拉深」朱砂软底高亮
        let sel_row = div()
            .flex()
            .items_center()
            .gap(theme::GAP_SM)
            .flex_wrap()
            .mx(theme::PAD_XL)
            .mt(theme::GAP_MD)
            .px(theme::PAD_MD)
            .py(theme::PAD_SM)
            .rounded(theme::RADIUS_PANEL)
            .bg(theme::PANEL2())
            .border_1()
            .border_color(theme::LINE())
            .shadow_md()
            .child(
                div()
                    .text_size(theme::FONT_LABEL)
                    .text_color(if self.has_sel { theme::ACCENT() } else { theme::FAINT() })
                    .child(SharedString::from(format!(
                        "选区创作 ▸ 已选 {} 字（点按钮即以此段为改写对象）",
                        self.sel_chars
                    ))),
            )
            .children(
                crate::prompts::SELECTION_BUTTONS
                    .iter()
                    .enumerate()
                    .map(|(ix, (label, kind))| {
                        let label: &'static str = *label;
                        let kind: &'static str = *kind;
                        let id = format!("sel-{kind}");
                        let button = if ix == 0 {
                            // 首选项「拉深」＝朱砂软底高亮（原型 .mb.hl）
                            accent_button(id, label, &*cx)
                        } else {
                            neutral_button(id, label, theme::PANEL(), theme::TEXT(), &*cx)
                        };
                        button.on_click(cx.listener(move |_this, _, _, cx| {
                            cx.emit(EditorEvent::SelectionAction(kind));
                        }))
                    }),
            );

        // ── 稿纸：640 居中卡，宋体 14px / 行高 1.9 / 宣纸字色
        let scroll = div()
            .id("editor-scroll")
            .flex()
            .flex_col()
            .flex_1()
            .min_h_0()
            .overflow_y_scroll()
            .px(theme::PAD_XL)
            .py(theme::GAP_MD)
            .gap(theme::GAP_MD)
            .child(
                div()
                    .flex()
                    .justify_center()
                    .w_full()
                    .child(
                        div()
                            .w_full()
                            .max_w(px(640.))
                            .rounded(theme::RADIUS_CARD)
                            .bg(theme::PROSE_BG())
                            .border_1()
                            .border_color(theme::LINE())
                            .px(theme::PAD_XL)
                            .py(theme::PAD_LG)
                            .flex()
                            .child(
                                // 左缘墨点：这一版里 AI 参与过的段落。只标记，不改一个字。
                                // 段数通常是个位数，直接按行高比例画短竖条即可，
                                // 不必做真正的行测量。
                                if self.ai_marks.is_empty() {
                                    div().into_any_element()
                                } else {
                                    let total = prose.read(cx).value().chars().count().max(1);
                                    let mut gutter = div()
                                        .w(px(10.))
                                        .flex_shrink_0()
                                        .flex()
                                        .flex_col()
                                        .gap(px(2.));
                                    for m in self.ai_marks.iter().take(24) {
                                        // 用“第几成”近似位置：够点到对应段落，不追求像素级
                                        let frac = (m.start as f32 / total as f32).clamp(0., 1.);
                                        gutter = gutter.child(
                                            div()
                                                .h(px(10.))
                                                .w(px(3.))
                                                .rounded(px(2.))
                                                .bg(if m.mode == "ghostwrite" {
                                                    theme::AMBER()
                                                } else {
                                                    theme::ACCENT()
                                                })
                                                .opacity(0.55 + 0.45 * (1.0 - frac)),
                                        );
                                    }
                                    gutter.into_any_element()
                                },
                            )
                            .child(
                                div().flex_1().min_w_0().child(
                                    Textarea::new(&prose)
                                        .bordered(false)
                                        .appearance(false)
                                        .text_size(theme::FONT_PROSE)
                                        .line_height(relative(theme::LINE_HEIGHT_PROSE))
                                        .font_family(theme::FAMILY_SERIF)
                                        .text_color(theme::TEXT()),
                                ),
                            ),
                    ),
            );

        // ── 底部发话栏：左右 20px 与上方各栏对齐；主次按钮分明
        let footer = div()
            .flex()
            .items_center()
            .gap(theme::GAP_MD)
            .border_t_1()
            .border_color(theme::LINE())
            .px(theme::PAD_XL)
            .py(theme::PAD_SM)
            .bg(theme::PANEL())
            .child(div().flex_1().min_w_0().child(ink_input(&prompt)))
            .child(save)
            .child(send);


        // ── 分支卡（原型三选一）：AI 走向就绪时浮出，采纳一个即并入正文 ──
        let branch_section = {
            let branches = self.branches.clone();
            if branches.is_empty() {
                div().id("branches-empty").child(
                    div()
                        .flex()
                        .items_center()
                        .gap(theme::GAP_MD)
                        .mx(theme::PAD_XL)
                        .mt(theme::GAP_MD)
                        .px(theme::PAD_MD)
                        .py(theme::PAD_SM)
                        .rounded(theme::RADIUS_PANEL)
                        .bg(theme::PANEL2())
                        .border_1()
                        .border_color(theme::LINE())
                        .child(
                            div()
                                .text_size(theme::FONT_LABEL)
                                .text_color(theme::FAINT())
                                .child("还没有走向——按 ⌘⇧B 让 AI 给三个"),
                        ),
                )
            } else {
                let mut cards = Vec::new();
                for (ix, body) in branches.iter().enumerate() {
                    let name = body
                        .chars()
                        .take_while(|c| *c != '】' && *c != '\n')
                        .collect::<String>()
                        .trim_start_matches('【')
                        .to_string();
                    let preview = body.chars().skip(0).collect::<String>();
                    let preview: String = preview
                        .chars()
                        .skip(name.chars().count() + 1)
                        .take(130)
                        .collect();
                    let adopt_ix = ix;
                    cards.push(
                        div()
                            .flex()
                            .flex_1()
                            .min_w_0()
                            .flex_col()
                            .gap(theme::GAP_SM)
                            .p(theme::PAD_MD)
                            .rounded(theme::RADIUS_PANEL)
                            .bg(theme::PANEL())
                            .border_1()
                            .border_color(theme::LINE())
                            .hover(|s| s.border_color(theme::ACCENT_BORDER()))
                            .child(
                                div()
                                    .text_size(theme::FONT_SMALL)
                                    .font_weight(FontWeight::SEMIBOLD)
                                    .text_color(theme::ACCENT())
                                    .child(SharedString::from(if name.is_empty() {
                                        format!("走向 {}", (b'A' + ix as u8) as char)
                                    } else {
                                        name
                                    })),
                            )
                            .child(
                                div()
                                    .text_size(theme::FONT_LABEL)
                                    .line_height(relative(theme::LINE_HEIGHT_BODY))
                                    .text_color(theme::DIM())
                                    .child(SharedString::from(preview)),
                            )
                            .child(
                                accent_button(format!("adopt-branch-{ix}"), "采纳此走向", &*cx)
                                    .on_click(cx.listener(move |_this, _, _, cx| {
                                        cx.emit(EditorEvent::AdoptBranch(adopt_ix));
                                    })),
                            ),
                    );
                }
                div()
                    .id("branches-row")
                    .flex()
                    .flex_col()
                    .gap(theme::GAP_SM)
                    .mx(theme::PAD_XL)
                    .mt(theme::GAP_MD)
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap(theme::GAP_MD)
                            .child(
                                div()
                                    .text_size(theme::FONT_LABEL)
                                    .font_weight(FontWeight::SEMIBOLD)
                                    .text_color(theme::TEXT())
                                    .child("AI 给了几个走向——选一个，或换一批"),
                            )
                            .child(div().flex_1())
                            .child(
                                neutral_button("reroll", "换一批", theme::PANEL2(), theme::DIM(), &*cx)
                                    .on_click(cx.listener(|_this, _, _, cx| {
                                        cx.emit(EditorEvent::RerollBranches);
                                    })),
                            )
                            .child(
                                neutral_button("dismiss", "全部不要", theme::PANEL2(), theme::DIM(), &*cx)
                                    .on_click(cx.listener(|_this, _, _, cx| {
                                        cx.emit(EditorEvent::DismissBranches);
                                    })),
                            ),
                    )
                    .child(div().flex().gap(theme::GAP_MD).w_full().children(cards))
            }
        };

        div()
            .flex()
            .flex_col()
            .size_full()
            .bg(theme::BG())
            .text_size(theme::FONT_BODY)
            .text_color(theme::TEXT())
            .child(head)
            .child(stats)
            .child(notes_row)
            .child(tools_row)
            .child(branch_section)
            .when(self.has_sel, |d| d.child(sel_row))
            .child(scroll)
            .child(footer)
    }
}

/// 墨色输入框：PANEL2 底 + LINE 1px 描边 + 8px 圆角 + 宣纸字色
/// （gpui-component 的 Input 底色走库主题，必须在实例样式上覆写）。
fn ink_input(state: &Entity<InputState>) -> Input {
    Input::new(state)
        .appearance(true)
        .bordered(true)
        .bg(theme::PANEL2())
        .border_color(theme::LINE())
        .rounded(theme::RADIUS_BTN)
        .text_size(theme::FONT_BODY)
        .text_color(theme::TEXT())
}

/// 按钮文字＝11.5px（原型 .za-ib/.za-btn）：Button 的 label 字号由 size 档位决定，
/// 走 child 自己定字号才能取到 theme token。
fn tool_label(label: &'static str) -> impl IntoElement {
    div()
        .whitespace_nowrap()
        .text_size(theme::FONT_META)
        .child(label)
}

/// 按钮公共外壳：8px 圆角 + 12px 横向内边距 + 11.5px 文字（原型 .za-ib 节奏）。
fn button_shell(id: impl Into<ElementId>, label: &'static str) -> Button {
    Button::new(id)
        .small()
        .rounded(theme::RADIUS_BTN)
        .px(theme::PAD_MD)
        .child(tool_label(label))
        .accessibility_label(label)
}

/// 主能力按钮（原型 .za-ib.pri / .za-selmenu .mb.hl）：朱砂软底 + 朱砂描边 + 宣纸字。
/// 库的 ButtonCustomVariant 描边不随状态变（button.rs border_color 只看 variant），
/// 所以 hover 语言只能落在底色上：软底加深一档。
fn accent_button(id: impl Into<ElementId>, label: &'static str, cx: &App) -> Button {
    let accent_h: Hsla = theme::ACCENT().into();
    button_shell(id, label)
        .custom(
            ButtonCustomVariant::new(cx)
                .color(theme::ACCENT_BORDER().into())
                .foreground(theme::TEXT().into())
                .hover(accent_h.opacity(0.28))
                .active(accent_h.opacity(0.42)),
        )
        .border_1()
        .border_color(theme::ACCENT_BORDER())
        .bg(theme::ACCENT_SOFT())
        .text_color(theme::TEXT())
}

/// 中性工具按钮（原型 .za-ib / .za-selmenu .mb）：底色与文字色由所在面的 token 决定
/// （PANEL 工具条上＝PANEL2+DIM，PANEL2 浮层上＝PANEL+TEXT），hover 提亮一档。
fn neutral_button(
    id: impl Into<ElementId>,
    label: &'static str,
    bg: Rgba,
    fg: Rgba,
    cx: &App,
) -> Button {
    let bg_h: Hsla = bg.into();
    button_shell(id, label)
        .custom(
            ButtonCustomVariant::new(cx)
                .color(theme::LINE().into())
                .foreground(fg.into())
                .hover(bg_h.lighten(0.06))
                .active(bg_h.darken(0.05)),
        )
        .border_1()
        .border_color(theme::LINE())
        .bg(bg)
        .text_color(fg)
}

/// 语义徽标（原型 .za-chip：10.5px / 9px 圆角 / 1px 语义描边 / 17px 行高）。
/// 状态一律用 chip 表达，不用灰字裸文本。
fn chip(
    text: impl Into<SharedString>,
    fg: gpui_kit::Rgba,
    border: gpui_kit::Rgba,
    bg: gpui_kit::Rgba,
) -> gpui_kit::AnyElement {
    // kit Tag::custom：语义三色（规则 6/10）
    let t: SharedString = text.into();
    gpui_kit::component::tag::Tag::custom(bg.into(), fg.into(), border.into())
        .outline()
        .child(t)
        .into_any_element()
}

/// 千分位计数（原型统计写作 3,214 字）。
fn count_text(n: u32) -> String {
    let digits = n.to_string();
    let mut out = String::with_capacity(digits.len() + digits.len() / 3);
    for (ix, c) in digits.chars().enumerate() {
        if ix > 0 && (digits.len() - ix) % 3 == 0 {
            out.push(',');
        }
        out.push(c);
    }
    out
}

/// 对白占比：以「/“开头的行字数占总字数（网文结构信号，零 LLM）
fn dialogue_share(text: &str) -> u32 {
    let total = crate::store::count_words(text).max(1);
    let spoken: usize = text
        .lines()
        .filter(|l| l.trim_start().starts_with('「') || l.trim_start().starts_with('“'))
        .map(|l| crate::store::count_words(l))
        .sum();
    (spoken * 100 / total).min(100) as u32
}
