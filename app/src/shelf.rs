//! 书架（应用首页）：与作品内容严格分层——不挂侧栏/轨迹 Dock，只管发现与打开。
//! 视觉对齐 design/shelf.html（页头 + 3:4 书卡网格 + 新建入口）；
//! 颜色/间距/圆角/字号一律取自 `crate::theme`（唯一事实源，勿在此写裸色值）。

use gpui_kit::component::progress::ProgressCircle;
use gpui_kit::prelude::FluentBuilder;
use gpui_kit::{
    App, Context, EventEmitter, FocusHandle, Focusable, FontWeight, Hsla, InteractiveElement,
    IntoElement, ParentElement, PathBuilder, Pixels, Render, SharedString, StatefulInteractiveElement,
    Styled, Window, canvas, div, linear_color_stop, linear_gradient, point, px, relative, white,
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

/// Swift 版 BookCoverTile.palette 的五组书封配色（朱砂/靛青/竹青/紫檀/赭金），
/// 风格与配色同源于标题哈希——同一本书换风格不换色系。
fn cover_palette(title: &str) -> (Hsla, Hsla) {
    const P: [(u32, u32); 5] = [
        (0xC75238, 0x732A1E), // 0 朱砂
        (0x336B85, 0x1A384D), // 1 靛青
        (0x5C8047, 0x2E471F), // 2 竹青
        (0x7A619E, 0x3D2E5C), // 3 紫檀
        (0xB88038, 0x612F1A), // 4 赭金
    ];
    let h = title
        .chars()
        .fold(0u32, |acc, c| acc.wrapping_mul(31).wrapping_add(c as u32));
    let (a, b) = P[(h % 5) as usize];
    (gpui_kit::rgb(a).into(), gpui_kit::rgb(b).into())
}

/// 优先取作者在 project.json 里存的 coverStyle（Swift 版同字段），否则按书名哈希。
fn cover_style_of(b: &ProjectRef) -> u8 {
    b.cover_style.unwrap_or_else(|| cover_style(&b.title))
}

/// 书名哈希 → 默认风格（0 竖脊 / 1 横排 / 2 朱砂印 / 3 水墨远山 / 4 竹纹），
/// 与 Swift CoverStyle.defaultIndex 同式：unicodeScalars 折叠哈希 % 5。
fn cover_style(title: &str) -> u8 {
    let h = title
        .chars()
        .fold(0u32, |acc, c| acc.wrapping_mul(31).wrapping_add(c as u32));
    (h % 5) as u8
}

/// 封面渲染（照 Swift BookCover 五式重绘；宽 210 的 3:4 竖版）。
fn book_cover(title: &str, style: u8) -> gpui_kit::AnyElement {
    let (c0, c1) = cover_palette(title);
    let glyph: String = title.chars().next().map(|c| c.to_string()).unwrap_or_default();
    let card = div().relative().size_full().overflow_hidden();

    match style {
        // ── 1 横排大字（书衣双框 + 朱砂点线）
        1 => card
            .bg(linear_gradient(180., linear_color_stop(c0, 0.), linear_color_stop(c1, 1.)))
            .flex()
            .flex_col()
            .items_center()
            .justify_center()
            .gap(px(10.))
            .px(px(18.))
            .child(
                div()
                    .text_size(px(26.))
                    .line_height(relative(1.3))
                    .font_family(theme::FAMILY_SERIF)
                    .font_weight(FontWeight::BOLD)
                    .text_center()
                    .text_color(white().opacity(0.96))
                    .child(SharedString::from(title.chars().take(8).collect::<String>())),
            )
            .child(ornament_line())
            .child(
                // 内框（书衣双框）
                div()
                    .absolute()
                    .left(px(8.))
                    .top(px(8.))
                    .right(px(8.))
                    .bottom(px(8.))
                    .rounded(px(2.))
                    .border_1()
                    .border_color(white().opacity(0.32)),
            )
            .into_any_element(),

        // ── 2 朱砂印（米白纸面 + 大印）
        2 => card
            .bg(linear_gradient(
                180.,
                linear_color_stop(Hsla::from(gpui_kit::rgb(0xF2E9D6)), 0.),
                linear_color_stop(Hsla::from(gpui_kit::rgb(0xDED4BE)), 1.),
            ))
            .flex()
            .flex_col()
            .items_center()
            .justify_center()
            .gap(px(14.))
            .child(
                div()
                    .w(px(84.))
                    .h(px(84.))
                    .flex()
                    .items_center()
                    .justify_center()
                    .rounded(px(18.))
                    .bg(linear_gradient(
                        180.,
                        linear_color_stop(Hsla::from(gpui_kit::rgb(0xBB3B2D)), 0.),
                        linear_color_stop(Hsla::from(gpui_kit::rgb(0x8E2418)), 1.),
                    ))
                    .child(
                        div()
                            .text_size(px(56.))
                            .font_family(theme::FAMILY_SERIF)
                            .font_weight(FontWeight::BOLD)
                            .text_color(white().opacity(0.95))
                            .child(SharedString::from(glyph)),
                    ),
            )
            .child(
                div()
                    .text_size(px(20.))
                    .line_height(relative(1.35))
                    .font_family(theme::FAMILY_SERIF)
                    .font_weight(FontWeight::SEMIBOLD)
                    .text_center()
                    .text_color(gpui_kit::rgb(0x3A3026))
                    .child(SharedString::from(title.chars().take(8).collect::<String>())),
            )
            .into_any_element(),

        // ── 3 水墨远山（暗底 + 山影 + 白色题签）
        3 => card
            .bg(linear_gradient(
                180.,
                linear_color_stop(Hsla::from(gpui_kit::rgb(0x2A2E38)), 0.),
                linear_color_stop(Hsla::from(gpui_kit::rgb(0x111318)), 1.),
            ))
            .child(
                canvas(
                    |_, _, _| {},
                    |bounds, _, window, _| {
                        let w = bounds.size.width.as_f32();
                        let h = bounds.size.height.as_f32();
                        let ox = bounds.origin.x;
                        let oy = bounds.origin.y;
                        for (frac, seed, alpha) in [(0.38f32, 2.0f32, 0.30f32), (0.22, 5.0, 0.46)] {
                            let mut b = PathBuilder::fill();
                            b.move_to(point(ox, oy + px(h)));
                            let steps = 28;
                            for i in 0..=steps {
                                let t = i as f32 / steps as f32;
                                let x = w * t;
                                let y = h * 0.78
                                    - h * frac * (0.6 + 0.4 * (t * 5.2 + seed).sin())
                                    - h * frac * 0.2 * (t * 11.0 + seed * 3.0).sin();
                                b.line_to(point(ox + px(x), oy + px(y)));
                            }
                            b.line_to(point(ox + px(w), oy + px(h)));
                            b.close();
                            if let Ok(path) = b.build() {
                                window.paint_path(path, white().opacity(alpha * 0.35));
                            }
                        }
                    },
                )
                .absolute()
                .size_full(),
            )
            .child(
                div()
                    .absolute()
                    .left(px(14.))
                    .right(px(14.))
                    .bottom(px(16.))
                    .px(px(10.))
                    .py(px(7.))
                    .rounded(px(3.))
                    .bg(white().opacity(0.92))
                    .flex()
                    .justify_center()
                    .child(
                        div()
                            .text_size(px(18.))
                            .line_height(relative(1.3))
                            .font_family(theme::FAMILY_SERIF)
                            .font_weight(FontWeight::SEMIBOLD)
                            .text_center()
                            .text_color(gpui_kit::rgb(0x1C1A17))
                            .child(SharedString::from(title.chars().take(8).collect::<String>())),
                    ),
            )
            .into_any_element(),

        // ── 4 竹纹竖条
        4 => card
            .bg(linear_gradient(180., linear_color_stop(c1, 0.), linear_color_stop(c0, 1.)))
            .flex()
            .items_center()
            .justify_center()
            .child(
                // 竖条：10 道，隔道明暗
                div()
                    .absolute()
                    .size_full()
                    .flex()
                    .children((0..10).map(|i| {
                        div()
                            .flex_1()
                            .h_full()
                            .bg(white().opacity(if i % 2 == 0 { 0.045 } else { 0.015 }))
                    })),
            )
            .child(
                div()
                    .px(px(16.))
                    .text_size(px(24.))
                    .line_height(relative(1.32))
                    .font_family(theme::FAMILY_SERIF)
                    .font_weight(FontWeight::BOLD)
                    .text_center()
                    .text_color(white().opacity(0.96))
                    .child(SharedString::from(title.chars().take(7).collect::<String>())),
            )
            .child(
                div()
                    .absolute()
                    .left(px(40.))
                    .right(px(40.))
                    .bottom(px(22.))
                    .child(ornament_line()),
            )
            .into_any_element(),

        // ── 0（默认）竖脊：竖排书名 + 竹席横纹 + 右下印
        _ => card
            .bg(linear_gradient(135., linear_color_stop(c0, 0.), linear_color_stop(c1, 1.)))
            .flex()
            .flex_col()
            .items_center()
            .justify_center()
            .gap(px(3.))
            .child(
                div()
                    .absolute()
                    .size_full()
                    .flex()
                    .flex_col()
                    .justify_between()
                    .py(px(18.))
                    .children((0..9).map(|_| {
                        div().w_full().h(px(1.)).bg(white().opacity(0.05))
                    })),
            )
            .child(
                div()
                    .flex()
                    .flex_col()
                    .items_center()
                    .gap(px(2.))
                    .children(
                        title
                            .chars()
                            .filter(|c| !matches!(c, '，' | ',' | '。' | '·' | ' ' | '：' | ':'))
                            .take(5)
                            .map(|c| {
                                div()
                                    .text_size(px(26.))
                                    .line_height(relative(1.25))
                                    .font_family(theme::FAMILY_SERIF)
                                    .font_weight(FontWeight::SEMIBOLD)
                                    .text_color(white().opacity(0.96))
                                    .child(SharedString::from(c.to_string()))
                            })
                            .collect::<Vec<_>>(),
                    ),
            )
            .child(corner_seal())
            .into_any_element(),
    }
}

/// 书脊高光（左缘 12% 宽的白色渐隐）——挂到封面上做叠层。
fn spine_glint() -> gpui_kit::AnyElement {
    div()
        .absolute()
        .left(px(0.))
        .top(px(0.))
        .bottom(px(0.))
        .w(px(16.))
        .bg(linear_gradient(
            90.,
            linear_color_stop(white().opacity(0.14), 0.),
            linear_color_stop(white().opacity(0.0), 1.),
        ))
        .into_any_element()
}

/// 朱砂点线装饰（横排与竹纹款共用）。
fn ornament_line() -> gpui_kit::AnyElement {
    div()
        .flex()
        .items_center()
        .gap(px(6.))
        .child(div().w(px(34.)).h(px(1.)).bg(white().opacity(0.4)))
        .child(
            div()
                .w(px(5.))
                .h(px(5.))
                .rounded(px(3.))
                .bg(gpui_kit::rgb(0xC75238).alpha(0.9)),
        )
        .child(div().w(px(34.)).h(px(1.)).bg(white().opacity(0.4)))
        .into_any_element()
}

/// 右下角「执」字朱砂小印。
fn corner_seal() -> gpui_kit::AnyElement {
    div()
        .absolute()
        .right(px(10.))
        .bottom(px(10.))
        .w(px(26.))
        .h(px(26.))
        .flex()
        .items_center()
        .justify_center()
        .rounded(px(5.))
        .bg(gpui_kit::rgb(0xBB3B2D).alpha(0.92))
        .child(
            div()
                .text_size(px(14.))
                .font_family(theme::FAMILY_SERIF)
                .font_weight(FontWeight::BOLD)
                .text_color(white().opacity(0.92))
                .child("执"),
        )
        .into_any_element()
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
                        // 封面：照 Swift BookCover 五式（竖脊/横排/朱砂印/水墨远山/竹纹），
                        // 书名哈希定风格与配色；体裁写在卡脚，不压封面。
                        div()
                            .relative()
                            .aspect_ratio(COVER_ASPECT)
                            .w_full()
                            .child(book_cover(&b.title, cover_style_of(b)))
                            .child(spine_glint()),
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
