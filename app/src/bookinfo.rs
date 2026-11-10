//! 书目信息页：题材 / 简介 / 目标章数 / 章字数目标的唯一编辑入口。
//! 只写 project.json（与 Swift 旧版书名档同格式，camelCase）；书名本体的显示名不动
//!（改书名涉及目录重命名，留待后续迁移工具）。

use std::rc::Rc;

use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::dock::{BasePanel, Panel as ComponentPanel, PanelEvent};
use gpui_kit::component::input::{Input, InputState};
use gpui_kit::component::Sizable;
use gpui_kit::prelude::FluentBuilder;
use gpui_kit::{
    App, AppContext as _, Context, Entity, EventEmitter, FocusHandle, Focusable, FontWeight,
    InteractiveElement, IntoElement, ParentElement, Render, SharedString,
    StatefulInteractiveElement, Styled,
    Window, div, px, relative,
};

use crate::store::ProjectHandle;
use crate::theme;

#[derive(Debug, Clone)]
pub enum BookInfoEvent {
    /// 保存成功（含新的章字数目标，编辑器进度环据此刷新）
    Saved { word_target: u32 },
}

pub struct BookInfoPanel {
    focus_handle: FocusHandle,
    handle: Rc<ProjectHandle>,
    genre: Entity<InputState>,
    premise: Entity<InputState>,
    target_chapters: Entity<InputState>,
    word_target: Entity<InputState>,
    message: Option<SharedString>,
}

impl BookInfoPanel {
    pub fn new(window: &mut Window, cx: &mut Context<Self>, handle: Rc<ProjectHandle>) -> Self {
        let p = handle.project();
        let genre = cx.new(|cx| {
            let mut s = InputState::new(window, cx).placeholder("题材（如：武侠 / 悬疑 / 科幻）");
            s.set_value(p.genre.as_str(), window, cx);
            s
        });
        let premise = cx.new(|cx| {
            let mut s = InputState::new(window, cx).placeholder("一句话简介（本书的核心命题）");
            s.set_value(p.premise.as_str(), window, cx);
            s
        });
        let target_chapters = cx.new(|cx| {
            let mut s = InputState::new(window, cx).placeholder("目标章数");
            s.set_value(if p.target_chapters == 0 { "100".into() } else { p.target_chapters.to_string() }, window, cx);
            s
        });
        let word_target = cx.new(|cx| {
            let mut s = InputState::new(window, cx).placeholder("章字数目标");
            s.set_value(handle.project_word_target().to_string(), window, cx);
            s
        });
        Self {
            focus_handle: cx.focus_handle(),
            handle,
            genre,
            premise,
            target_chapters,
            word_target,
            message: None,
        }
    }

    fn save(&mut self, cx: &mut Context<Self>) {
        let mut p = self.handle.project();
        p.genre = self.genre.read(cx).value().trim().to_string();
        p.premise = self.premise.read(cx).value().trim().to_string();
        let tc: u32 = self.target_chapters.read(cx).value().trim().parse().unwrap_or(p.target_chapters);
        let wt: u32 = self.word_target.read(cx).value().trim().parse().unwrap_or(p.chapter_word_target);
        p.target_chapters = if tc == 0 { 100 } else { tc };
        p.chapter_word_target = if wt == 0 { 3000 } else { wt };
        match self.handle.save_project(&p) {
            Ok(()) => {
                self.message = Some("已保存（project.json）".into());
                cx.emit(BookInfoEvent::Saved { word_target: p.chapter_word_target });
            }
            Err(e) => self.message = Some(format!("保存失败：{e}").into()),
        }
        cx.notify();
    }
}

impl EventEmitter<PanelEvent> for BookInfoPanel {}
impl EventEmitter<BookInfoEvent> for BookInfoPanel {}

impl Focusable for BookInfoPanel {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus_handle.clone()
    }
}

fn field(label: &'static str, hint: &'static str, input: &Entity<InputState>) -> impl IntoElement {
    div()
        .flex()
        .flex_col()
        .gap(theme::GAP_XS)
        .child(
            div()
                .text_size(theme::FONT_LABEL)
                .font_weight(FontWeight::MEDIUM)
                .text_color(theme::DIM())
                .child(label),
        )
        .child(
            div()
                .text_size(theme::FONT_MICRO)
                .text_color(theme::FAINT())
                .child(hint),
        )
        .child(div().pt(theme::GAP_XS).child(Input::new(input)))
}

impl Render for BookInfoPanel {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let p = self.handle.project();
        let title = if p.title.trim().is_empty() { self.handle.title.clone() } else { p.title.clone() };
        let msg = self.message.clone();
        let target = self.handle.project_word_target();

        div()
            .size_full()
            .flex()
            .flex_col()
            .bg(theme::BG())
            .child(
                div()
                    .id("bookinfo-scroll")
                    .flex_1()
                    .min_h_0()
                    .overflow_y_scroll()
                    .p(theme::PAD_XL)
                    .child(
                        div()
                            .mx_auto()
                            .w(px(560.))
                            .flex()
                            .flex_col()
                            .gap(theme::GAP_GROUP)
                            .child(
                                div()
                                    .flex()
                                    .flex_col()
                                    .gap(theme::GAP_XS)
                                    .child(
                                        div()
                                            .text_size(theme::FONT_TITLE_SM)
                                            .font_family(theme::FAMILY_SERIF)
                                            .font_weight(FontWeight::SEMIBOLD)
                                            .text_color(theme::TEXT())
                                            .child(SharedString::from(title)),
                                    )
                                    .child(
                                        div()
                                            .text_size(theme::FONT_LABEL)
                                            .line_height(relative(theme::LINE_HEIGHT_BODY))
                                            .text_color(theme::FAINT())
                                            .child("书目信息 · 只写 project.json；书名本体暂不支持改名（涉及目录迁移）"),
                                    ),
                            )
                            .child(field("题材", "书架卡片与筛选读这里", &self.genre))
                            .child(field("一句话简介", "立项时给 AI 的背景锚（可留空）", &self.premise))
                            .child(field("目标章数", "写作统计的进度分母", &self.target_chapters))
                            .child(field(
                                "章字数目标",
                                "编辑器字数进度环的分母（当前生效值会在保存后立即刷新）",
                                &self.word_target,
                            ))
                            .child(
                                div()
                                    .flex()
                                    .items_center()
                                    .gap(theme::GAP_MD)
                                    .child(
                                        Button::new("bookinfo-save")
                                            .primary()
                                            .small()
                                            .label("保存")
                                            .on_click(cx.listener(|this, _, _, cx| this.save(cx))),
                                    )
                                    .child(
                                        div()
                                            .text_size(theme::FONT_LABEL)
                                            .text_color(theme::GREEN())
                                            .child(SharedString::from(format!(
                                                "当前目标：{} 章 × {} 字",
                                                p.target_chapters.max(1),
                                                target
                                            ))),
                                    )
                                    .when_some(msg, |d, m| {
                                        d.child(
                                            div()
                                                .text_size(theme::FONT_LABEL)
                                                .text_color(theme::GREEN())
                                                .child(m),
                                        )
                                    }),
                            ),
                    ),
            )
    }
}

impl BasePanel for BookInfoPanel {
    fn panel_name(&self) -> &'static str {
        "bookinfo-view"
    }
}

impl ComponentPanel for BookInfoPanel {
    fn tab_name(&self, _: &App) -> Option<SharedString> {
        Some("书目".into())
    }

    fn title(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div()
            .text_size(theme::FONT_BODY)
            .font_weight(FontWeight::MEDIUM)
            .text_color(theme::TEXT())
            .child("书目信息")
    }
}
