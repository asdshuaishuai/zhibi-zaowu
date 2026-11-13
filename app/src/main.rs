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

use gpui_kit::component::theme::{Theme, ThemeMode};
use gpui_kit::{
    AppContext as _, Bounds, Size, WindowBounds, WindowKind, WindowOptions, component::TitleBar,
    point, px, rgba, rgb,
};

/// 全局主题初始化：墨底/宣纸/朱砂——库组件（Input/Button/Textarea/Sidebar/
/// TitleBar/StatusBar）全部从这里取色，不再与手绘 palette 两套体系打架。
fn init_theme(cx: &mut gpui_kit::App) {
    // 启动即按设置切主题（settings.json 的 theme：ink=墨 / xuan=宣纸）
    let t = crate::store::load_settings().theme;
    crate::theme::set_theme_kind(match t.trim() {
        "xuan" => crate::theme::ThemeKind::Xuan,
        _ => crate::theme::ThemeKind::Ink,
    });
    apply_gpui_theme(cx);
}

/// 把组件库主题（按钮/输入/侧栏等）按当前主题形态重刷。
///
/// 我们的 theme.rs 颜色函数每帧都会重新取值，切完即生效；
/// 组件库的 Theme 是全局单例，必须在这里整体替换。
pub fn apply_gpui_theme(cx: &mut gpui_kit::App) {
    match crate::theme::theme_kind() {
        crate::theme::ThemeKind::Xuan => {
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
