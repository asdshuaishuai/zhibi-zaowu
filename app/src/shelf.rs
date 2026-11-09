//! 书架（应用首页）：与作品内容严格分层——不挂侧栏/轨迹 Dock，只管发现与打开。
//! 参考旧版执笔 WelcomeView：书卡网格（封面占位 + 标题 + 统计）+ 新建。

use gpui_kit::component::button::Button;
use gpui_kit::{
    App, Context, EventEmitter, FocusHandle, Focusable, IntoElement, ParentElement, Render,
    SharedString, Styled, Window, div, rgb,
};

use crate::store::{self, ProjectRef};

#[derive(Debug, Clone)]
pub enum ShelfEvent {
    Open(std::path::PathBuf),
}

pub struct ShelfView {
    focus_handle: FocusHandle,
    books: Vec<ProjectRef>,
}

impl ShelfView {
    pub fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        let mut this = Self { focus_handle: cx.focus_handle(), books: Vec::new() };
        this.reload(window, cx);
        this
    }

    /// 后台扫描书架（目录级 I/O 不该占主线程）。
    pub fn reload(&mut self, _window: &mut Window, cx: &mut Context<Self>) {
        cx.spawn(async move |this, cx| {
            let books =
                cx.background_executor().spawn(async move { store::list_projects() }).await;
            this.update(cx, |this, cx| {
                this.books = books;
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    pub fn create_book(&mut self, _window: &mut Window, cx: &mut Context<Self>) {
        let title = format!("未命名作品 {}", chrono_like_date());
        match store::create_project(&title) {
            Ok(dir) => {
                cx.emit(ShelfEvent::Open(dir));
            }
            Err(e) => eprintln!("新建作品失败：{e}"),
        }
    }
}

fn chrono_like_date() -> String {
    // M2 简版：用当天 UTC 月-日 组名避免重名
    let secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    let days = secs / 86_400;
    format!("{days}")
}

impl EventEmitter<ShelfEvent> for ShelfView {}

impl Focusable for ShelfView {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus_handle.clone()
    }
}

fn cover_tint(title: &str) -> (u32, u32) {
    let h = title.bytes().fold(7u32, |acc, b| acc.wrapping_mul(31).wrapping_add(b as u32));
    match h % 4 {
        0 => (0x2a2033u32, 0x3a1f1a),
        1 => (0x1a2a33, 0x14324a),
        2 => (0x33261a, 0x4a3a14),
        _ => (0x21332a, 0x14321f),
    }
}

impl Render for ShelfView {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let mut cards = Vec::new();
        for b in &self.books {
            let (c1, _c2) = cover_tint(&b.title);
            cards.push(
                div()
                    .flex()
                    .flex_col()
                    .bg(rgb(0x1e2026))
                    .border_1()
                    .border_color(rgb(0x2e3138))
                    .rounded_lg()
                    .overflow_hidden()
                    .child(
                        div()
                            .aspect_square()
                            .bg(rgb(c1))
                            .flex()
                            .items_center()
                            .justify_center()
                            .child(
                                div()
                                    .text_xl()
                                    .text_color(rgb(0xe8ddca))
                                    .child(SharedString::from(b.title.chars().take(6).collect::<String>())),
                            ),
                    )
                    .child(
                        div()
                            .px_2()
                            .py_1p5()
                            .child(
                                div()
                                    .text_sm()
                                    .font_weight(gpui_kit::FontWeight::SEMIBOLD)
                                    .child(SharedString::from(b.title.clone())),
                            )
                            .child(
                                div()
                                    .text_xs()
                                    .text_color(rgb(0x5d5b57))
                                    .child(SharedString::from(format!(
                                        "{} · {} 章 · {} 字",
                                        if b.genre.is_empty() { "未设题材" } else { &b.genre },
                                        b.chapters,
                                        b.words
                                    ))),
                            ),
                    ),
            );
        }
        let grid = div()
            .flex()
            .flex_wrap()
            .gap_3()
            .w_full()
            .children(cards)
            .child(
                Button::new("new-book")
                    .label("＋ 新建作品")
                    .on_click(cx.listener(|this, _, window, cx| {
                        this.create_book(window, cx);
                    })),
            );

        div()
            .size_full()
            .bg(rgb(0x17181c))
            .p_4()
            .child(
                div()
                    .flex()
                    .items_baseline()
                    .gap_2()
                    .pb_3()
                    .child(
                        div()
                            .text_lg()
                            .font_weight(gpui_kit::FontWeight::SEMIBOLD)
                            .text_color(rgb(0xd8d5cf))
                            .child("书架"),
                    )
                    .child(
                        div()
                            .text_xs()
                            .text_color(rgb(0x5d5b57))
                            .child("人执笔，AI 研墨——选一本书继续，或开新书"),
                    ),
            )
            .child(grid)
    }
}
