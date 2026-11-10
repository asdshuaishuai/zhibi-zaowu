//! 书架（应用首页）：与作品内容严格分层——不挂侧栏/轨迹 Dock，只管发现与打开。
//! 视觉对齐 design/shelf.html（页头 + 3:4 书卡网格 + 新建入口）；
//! 颜色/间距/圆角/字号一律取自 `crate::theme`（唯一事实源，勿在此写裸色值）。

use gpui_kit::component::progress::ProgressCircle;
use gpui_kit::prelude::FluentBuilder;
use gpui_kit::{
    App, Context, EventEmitter, FocusHandle, Focusable, FontWeight, InteractiveElement,
    IntoElement, ParentElement, Pixels, Render, SharedString, StatefulInteractiveElement, Styled,
    Window, div, px, relative,
};

use crate::store::{self, ProjectRef};
use crate::theme;

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
        let mut this = Self {
            focus_handle: cx.focus_handle(),
            books: Vec::new(),
        };
        this.reload(window, cx);
        this
    }

    /// 后台扫描书架（目录级 I/O 不该占主线程）。
    pub fn reload(&mut self, _window: &mut Window, cx: &mut Context<Self>) {
        cx.spawn(async move |this, cx| {
            let books = cx
                .background_executor()
                .spawn(async move { store::list_projects() })
                .await;
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

/// 书卡宽 210px —— 原型 design/shelf.html:17 `minmax(210px, 1fr)` 的定宽档
/// （theme 无书卡 token，此处仅尺寸，非色板）。
const BOOK_CARD_W: Pixels = px(210.);

/// 封面宽高比 3:4 —— 原型 design/shelf.html:20 `aspect-ratio: 3/4`。
const COVER_ASPECT: f32 = 0.75;

/// 封面底色：按标题散列在四个语义色相间轮换，深浅一律由 token 派生——
/// 墨底 `BG()` 混入 16% 语义色，得到与原型封面渐变同气质的暗色底
/// （design/shelf.html:21-23 的紫青/蓝/褐金/绿四档对应朱砂/蓝/琥珀/绿）。
fn cover_tint(title: &str) -> gpui_kit::Rgba {
    let h = title
        .bytes()
        .fold(7u32, |acc, b| acc.wrapping_mul(31).wrapping_add(b as u32));
    let hue = match h % 4 {
        0 => theme::ACCENT(),
        1 => theme::BLUE(),
        2 => theme::AMBER(),
        _ => theme::GREEN(),
    };
    theme::BG().blend(hue.alpha(0.16))
}

impl Render for ShelfView {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let count = self.books.len();
        let today_words: usize = self.books.iter().map(|b| b.today_words).sum();
        let daily_goal: usize = 2500;
        let today_ratio = (today_words as f32 / daily_goal as f32).min(1.);
        let mut cards = Vec::new();
        for (i, b) in self.books.iter().enumerate() {
            let dir = b.dir.clone();
            cards.push(
                div()
                    .id(SharedString::from(format!("book-{i}")))
                    .flex()
                    .flex_col()
                    .w(BOOK_CARD_W)
                    .bg(theme::PANEL())
                    .border_1()
                    .border_color(theme::LINE())
                    .rounded(theme::RADIUS_CARD)
                    .overflow_hidden()
                    .cursor_pointer()
                    .hover(|s| s.border_color(theme::ACCENT_BORDER()))
                    .on_click(cx.listener(move |_this, _, _, cx| {
                        cx.emit(ShelfEvent::Open(dir.clone()));
                    }))
                    .child(
                        // 封面：3:4 暗底 + 居中衬线大字题签 + 朱砂短线 + 右下品牌印。
                        div()
                            .relative()
                            .aspect_ratio(COVER_ASPECT)
                            .bg(cover_tint(&b.title))
                            .flex()
                            .flex_col()
                            .items_center()
                            .justify_center()
                            .gap(theme::GAP_MD)
                            .child(
                                div()
                                    .text_size(px(30.))
                                    .line_height(relative(1.25))
                                    .font_family(theme::FAMILY_SERIF)
                                    .font_weight(FontWeight::SEMIBOLD)
                                    .text_color(theme::COVER_TEXT())
                                    .child(SharedString::from(
                                        b.title.chars().take(4).collect::<String>(),
                                    )),
                            )
                            .child(
                                div()
                                    .w(px(28.))
                                    .h(px(2.))
                                    .rounded(px(1.))
                                    .bg(theme::ACCENT().alpha(0.72)),
                            )
                            .child(
                                // 右下「执」字朱砂印——app icon 的隐喻落到书卡上。
                                div()
                                    .absolute()
                                    .bottom(theme::PAD_MD)
                                    .right(theme::PAD_MD)
                                    .w(px(22.))
                                    .h(px(22.))
                                    .flex()
                                    .items_center()
                                    .justify_center()
                                    .rounded(px(4.))
                                    .bg(theme::ACCENT().alpha(0.82))
                                    .child(
                                        div()
                                            .text_size(px(12.))
                                            .font_family(theme::FAMILY_SERIF)
                                            .text_color(theme::ON_ACCENT())
                                            .child("执"),
                                    ),
                            ),
                    )
                    .child(
                        // 卡脚：标题（正文档）+ 元信息行（标签档，三元素 gap 8）。
                        div()
                            .flex()
                            .flex_col()
                            .gap(theme::GAP_SM)
                            .px(theme::PAD_MD)
                            .pt(theme::PAD_SM)
                            .pb(theme::PAD_MD)
                            .child(
                                div()
                                    .text_size(theme::FONT_BODY)
                                    .line_height(relative(theme::LINE_HEIGHT_BODY))
                                    .font_weight(FontWeight::SEMIBOLD)
                                    .text_color(theme::TEXT())
                                    .child(SharedString::from(b.title.clone())),
                            )
                            .child(
                                div()
                                    .flex()
                                    .items_center()
                                    .gap(theme::GAP_SM)
                                    .child(status_chip(store::shelf_status(b.chapters, b.words)))
                                    .child(
                                        div()
                                            .flex_1()
                                            .min_w_0()
                                            .text_ellipsis()
                                            .text_size(theme::FONT_LABEL)
                                            .text_color(theme::FAINT())
                                            .child(SharedString::from(format!(
                                                "{} · {} 章 · {} 字",
                                                if b.genre.is_empty() { "未设题材" } else { &b.genre },
                                                b.chapters,
                                                store::fmt_thousands(b.words)
                                            ))),
                                    ),
                            )
                            .child(
                                div()
                                    .text_size(theme::FONT_TAG)
                                    .text_color(theme::FAINT())
                                    .child(SharedString::from(store::relative_time(b.updated))),
                            ),
                    ),
            );
        }

        let new_button = div()
            .id("new-book-card")
            .w(BOOK_CARD_W)
            .flex()
            .flex_col()
            .items_center()
            .justify_center()
            .gap(theme::GAP_MD)
            .border_1()
            .border_dashed()
            .border_color(theme::LINE())
            .rounded(theme::RADIUS_CARD)
            .cursor_pointer()
            .hover(|s| s.border_color(theme::ACCENT_BORDER()).bg(theme::PANEL()))
            .on_click(cx.listener(|this, _, window, cx| {
                this.create_book(window, cx);
            }))
            .child(
                div()
                    .aspect_ratio(COVER_ASPECT)
                    .w_full()
                    .flex()
                    .flex_col()
                    .items_center()
                    .justify_center()
                    .gap(theme::GAP_MD)
                    .child(
                        div()
                            .w(px(44.))
                            .h(px(44.))
                            .flex()
                            .items_center()
                            .justify_center()
                            .rounded(px(22.))
                            .bg(theme::PANEL2())
                            .child(
                                div()
                                    .text_size(px(22.))
                                    .text_color(theme::DIM())
                                    .child("＋"),
                            ),
                    )
                    .child(
                        div()
                            .text_size(theme::FONT_LABEL)
                            .text_color(theme::FAINT())
                            .child("新建作品"),
                    ),
            );

        let grid = div()
            .flex()
            .flex_wrap()
            .gap(theme::GAP_GROUP)
            .w_full()
            .children(cards)
            .child(new_button);

        // 空态必须有引导语（规则 15）：指向「＋ 新建作品」动作。
        use gpui_kit::component::empty::{
            Empty, EmptyDescription, EmptyHeader, EmptyMedia, EmptyMediaVariant, EmptyTitle,
        };
        let empty_hint = Empty::new()
            .w_full()
            .mb(theme::GAP_GROUP)
            .child(
                EmptyHeader::new()
                    .media(EmptyMedia::new().with_variant(EmptyMediaVariant::Icon))
                    .title(EmptyTitle::new().child("书架还是空的"))
                    .description(EmptyDescription::new().child(
                        "点下方「＋ 新建作品」开一本新书——执笔会建好书名目录与稿纸，第一次打开就能落笔。",
                    )),
            );

        div()
            .size_full()
            .bg(theme::BG())
            .p(theme::PAD_2XL)
            .child(
                // 页头：20px 大标题 + 11.5 DIM 引导语，右端 11 FAINT 计数。
                div()
                    .flex()
                    .items_baseline()
                    .gap(theme::GAP_LG)
                    .pb(theme::GAP_GROUP_LG)
                    .child(
                        div()
                            .text_size(theme::FONT_PAGE_TITLE)
                            .font_weight(FontWeight::SEMIBOLD)
                            .text_color(theme::TEXT())
                            .child("书架"),
                    )
                    .child(
                        div()
                            .text_size(theme::FONT_META)
                            .text_color(theme::DIM())
                            .child("人执笔，AI 研墨——选一本书继续，或开新书"),
                    )
                    .child(div().flex_1())
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap(theme::GAP_MD)
                            .child(
                                ProgressCircle::new("today-goal")
                                    .value(today_ratio)
                                    .color(theme::ACCENT()),
                            )
                            .child(
                                div()
                                    .flex()
                                    .flex_col()
                                    .gap(px(1.))
                                    .child(
                                        div()
                                            .text_size(theme::FONT_META)
                                            .font_weight(FontWeight::MEDIUM)
                                            .text_color(theme::TEXT())
                                            .child(SharedString::from(format!(
                                                "今日 {} 字",
                                                store::fmt_thousands(today_words)
                                            ))),
                                    )
                                    .child(
                                        div()
                                            .text_size(theme::FONT_TAG)
                                            .text_color(theme::FAINT())
                                            .child(SharedString::from(format!("{count} 本书"))),
                                    ),
                            ),
                    ),
            )
            .when(count == 0, |this| this.child(empty_hint))
            .child(grid)
    }
}

/// 状态 chip = kit Tag（规则 6/10）：写作中=success、规划中=info、新建=secondary。
fn status_chip(status: &'static str) -> impl IntoElement {
    use gpui_kit::component::tag::Tag;
    match status {
        "写作中" => Tag::success(),
        "规划中" => Tag::info(),
        _ => Tag::secondary(),
    }
    .outline()
    .child(status)
}
