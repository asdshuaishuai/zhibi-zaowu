//! 执笔造物 —— 跨端 AI 小说/剧本创作工作台（GPUI 前端 + 内嵌 DSH agent 底座）。
//! 两层结构：书架首页（与内容严格分层，参考旧版执笔）→ 打开作品进三栏工作区。

mod account;
mod app_root;
mod bookinfo;
mod canvas;
mod capabilities;
mod characters;
#[cfg(test)]
mod conformance;
#[cfg(test)]
mod conformance_helpers;
mod chat;
mod director;
mod dsh;
mod editor;
mod inbox;
mod graph_layout;
mod journal;
mod plugins;
mod prompts;
mod relations;
mod schema;
mod shelf;
mod store;
mod theme;
mod trajectory;
mod views;
mod workspace;
mod worldline;

use gpui_kit::component::theme::{Theme, ThemeMode, ThemeRegistry};
use gpui_kit::{
    AppContext as _, Bounds, Size, WindowBounds, WindowKind, WindowOptions, component::TitleBar,
    point, px, rgba, rgb,
};

/// 全局主题初始化：墨底/宣纸/朱砂——库组件（Input/Button/Textarea/Sidebar/
/// TitleBar/StatusBar）全部从这里取色，不再与手绘 palette 两套体系打架。
fn init_theme(cx: &mut gpui_kit::App) {
    // 启动即按设置切主题（settings.json 的 theme：ink=墨 / xuan=宣纸）
    let t = crate::store::load_settings().theme;
    crate::theme::set_theme_kind(crate::theme::ThemeKind::from_setting(&t));
    apply_gpui_theme(cx);
}

/// 把组件库主题调成 Canonical（该模式已注册的默认主题）。
///
/// **为什么每次重刷都要先做这一步**：`Theme::edit` 会把本次闭包对 colors 的
/// 改动反向写回 `tokens`（`reconcile`：colors 变了 → token 跟着变成这个实色）。
/// 主题一旦被我们的朱砂色改写，库里那些「颜色 + 渐变 + 描边」的 token 就永久
/// 绑死在墨底那套值上；下一次切主题时 `Theme::change` 只是重放另一套**库默认**
/// 颜色，而这些 token 不再重放——于是切过去的一瞬间整页又被改回旧配色。
/// 实测：墨 → 宣纸 → 墨，第二次「宣纸」看起来什么都没变（真的踩过）。
/// 先把库主题还原成规范色板，再叠我们这一层的覆盖，每次结果都一样。
fn reset_component_theme(cx: &mut gpui_kit::App) {
    let registry = ThemeRegistry::global(cx);
    let is_dark = matches!(crate::theme::theme_kind(), crate::theme::ThemeKind::Ink);
    let canonical = if is_dark {
        registry.default_dark_theme().clone()
    } else {
        registry.default_light_theme().clone()
    };
    let theme = cx.default_global::<Theme>();
    theme.apply_config(&canonical);
}

/// 按当前主题形态重刷全局配色（手绘 token + 组件库主题），并立即重绘全部窗口。
///
/// 这是**唯一的切换入口**：设置页点选、标题栏快捷键、启动初始化都走它。
/// 只在这里 `Theme::update`（会同步 gpui-base 的滚动条等并 refresh 全部窗口），
/// 调用方不需要再自己刷窗口。
pub fn apply_gpui_theme(cx: &mut gpui_kit::App) {
    apply_gpui_theme_inner(cx);
    // 改完 colors 不投影到 gpui-base（滚动条/拖拽把手）也不重绘；
    // Theme::update 会跑一遍 sync_base 并 refresh 所有窗口。
    Theme::update(cx, |_| {});
}

fn apply_gpui_theme_inner(cx: &mut gpui_kit::App) {
    match crate::theme::theme_kind() {
        crate::theme::ThemeKind::Xuan => {
            reset_component_theme(cx);
            Theme::change(ThemeMode::Light, None, cx);
            let theme = cx.default_global::<Theme>();
            let c = &mut theme.colors;
            c.background = rgb(0xf2ecdd).into();
            c.foreground = rgb(0x37322a).into();
            c.border = rgb(0xd6cbae).into();
            c.accent = rgba(0xb04a3020).into();
            c.accent_foreground = rgb(0x37322a).into();
            c.primary = rgb(0xb04a30).into();
            c.primary_foreground = rgb(0xfff8ee).into();
            c.secondary = rgb(0xeae0c9).into();
            c.secondary_foreground = rgb(0x37322a).into();
            c.muted = rgb(0xeae0c9).into();
            c.muted_foreground = rgb(0x7c7462).into();
            c.input = rgb(0xefe7d4).into();
            c.popover = rgb(0xf9f4e7).into();
            c.popover_foreground = rgb(0x37322a).into();
            c.danger = rgb(0xb0432e).into();
            c.success = rgb(0x4e7d55).into();
            c.info = rgb(0x4f6d99).into();
            c.scrollbar = rgb(0xeae0c9).into();
            c.scrollbar_thumb = rgb(0xcfc4a8).into();
            c.sidebar = rgb(0xf6f0e1).into();
            c.sidebar_foreground = rgb(0x37322a).into();
            c.sidebar_accent = rgba(0xb04a3020).into();
            c.sidebar_accent_foreground = rgb(0x37322a).into();
            c.sidebar_border = rgb(0xd6cbae).into();
            c.sidebar_primary = rgb(0xb04a30).into();
            c.sidebar_primary_foreground = rgb(0xfff8ee).into();
        }
        crate::theme::ThemeKind::Ink => {
            reset_component_theme(cx);
            Theme::change(ThemeMode::Dark, None, cx);
            let theme = cx.default_global::<Theme>();
            let c = &mut theme.colors;
            c.background = rgb(0x1b1d25).into();
            c.foreground = rgb(0xd8d5cf).into();
            c.border = rgb(0x454b61).into();
            c.accent = rgba(0xc96f4a24).into();
            c.accent_foreground = rgb(0xd8d5cf).into();
            c.primary = rgb(0xc96f4a).into();
            c.primary_foreground = rgb(0xffffff).into();
            c.secondary = rgb(0x31374a).into();
            c.secondary_foreground = rgb(0xd8d5cf).into();
            c.muted = rgb(0x31374a).into();
            c.muted_foreground = rgb(0x9a9da8).into();
            c.input = rgb(0x31374a).into();
            c.popover = rgb(0x262a35).into();
            c.popover_foreground = rgb(0xd8d5cf).into();
            c.danger = rgb(0xc0605c).into();
            c.success = rgb(0x7fae72).into();
            c.info = rgb(0x6f9bc4).into();
            c.scrollbar = rgb(0x262a35).into();
            c.scrollbar_thumb = rgb(0x454b61).into();
            c.sidebar = rgb(0x262a35).into();
            c.sidebar_foreground = rgb(0xd8d5cf).into();
            c.sidebar_accent = rgba(0xc96f4a24).into();
            c.sidebar_accent_foreground = rgb(0xd8d5cf).into();
            c.sidebar_border = rgb(0x454b61).into();
            c.sidebar_primary = rgb(0xc96f4a).into();
            c.sidebar_primary_foreground = rgb(0xffffff).into();
        }
    }
}

/// 切换主题并**落盘**（设置页点选、标题栏快捷键都走这里）。
///
/// 只切不存的话，重启又回到 settings.json 里的旧值——作者会以为"切了没生效"。
/// 返回切换后的形态，调用方拿去写回执。
pub fn switch_theme(kind: crate::theme::ThemeKind, cx: &mut gpui_kit::App) -> Result<(), String> {
    crate::theme::set_theme_kind(kind);
    apply_gpui_theme(cx);
    let mut cfg = crate::store::load_settings();
    cfg.theme = kind.persist_key().to_string();
    crate::store::save_settings(&cfg)
}

fn main() {
    gpui_kit::application()
        .with_assets(gpui_kit::assets::Assets)
        .run(|cx| {
            gpui_kit::init(cx);
            init_theme(cx);
            cx.bind_keys(vec![
                gpui_kit::KeyBinding::new("cmd-s", workspace::SaveProse, Some("zhibi-workspace")),
                gpui_kit::KeyBinding::new("cmd-shift-f", workspace::ToggleFocusMode, Some("zhibi-workspace")),
                gpui_kit::KeyBinding::new("cmd-shift-b", workspace::BranchesNow, Some("zhibi-workspace")),
            ]);
            cx.activate(true);

            let bounds = WindowBounds::Windowed(Bounds {
                origin: point(px(80.), px(60.)),
                size: Size { width: px(1440.), height: px(920.) },
            });
            let options = WindowOptions {
                window_bounds: Some(bounds),
                window_min_size: Some(Size { width: px(960.), height: px(600.) }),
                kind: WindowKind::Normal,
                // 拖动必须交回 AppKit。
                //
                // gpui-component 的 TitleBar 默认 app_owns_titlebar_drag=true，期望
                // 自己调 start_window_move。但当前 GPUI 的 macOS 实现是在 mouse_move
                // 阶段才调 performWindowDragWithEvent:，而 AppKit 只认 mouseDown 事件
                // ——结果是整个窗口拖不动（书架页连 TitleBar 都没有，更没得拖）。
                // false = 标题栏区域由 AppKit 原生拖动，点击事件仍照常送到内容区。
                app_owns_titlebar_drag: false,
                ..TitleBar::window_options()
            };

            let (handle, root) = gpui_kit::open_window(options, cx, |window, cx| {
                cx.new(|cx| app_root::AppRoot::new(window, cx))
            })
            .expect("执笔：打开主窗口失败");

            handle
                .update(cx, |_view, window, _cx| {
                    window.set_window_title("执笔造物 — 人写正文，AI 理账");
                    window.activate_window();
                })
                .expect("failed to update window");

            root.update(cx, |_, cx| {
                cx.on_release(|_, cx| {
                    cx.quit();
                })
                .detach();
            });
        });
}
