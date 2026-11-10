//! 执笔 3.0 —— 跨端 AI 小说写作工作台（GPUI 前端 + DSH agent 底座）。
//! 两层结构：书架首页（与内容严格分层，参考旧版执笔）→ 打开作品进三栏工作区。

mod app_root;
mod canvas;
mod chat;
mod director;
mod dsh;
mod editor;
mod inbox;
mod prompts;
mod shelf;
mod store;
mod theme;
mod trajectory;
mod views;
mod workspace;

use gpui_kit::component::theme::{Theme, ThemeMode};
use gpui_kit::{
    AppContext as _, Bounds, Size, WindowBounds, WindowKind, WindowOptions, component::TitleBar,
    point, px, rgba, rgb,
};

/// 全局主题初始化：墨底/宣纸/朱砂——库组件（Input/Button/Textarea/Sidebar/
/// TitleBar/StatusBar）全部从这里取色，不再与手绘 palette 两套体系打架。
fn init_theme(cx: &mut gpui_kit::App) {
    Theme::change(ThemeMode::Dark, None, cx);
    {
        let theme = cx.default_global::<Theme>();
        let c = &mut theme.colors;
        c.background = rgb(0x17181c).into();
        c.foreground = rgb(0xd8d5cf).into();
        c.border = rgb(0x2e3138).into();
        c.accent = rgba(0xc96f4a24).into();
        c.accent_foreground = rgb(0xd8d5cf).into();
        c.primary = rgb(0xc96f4a).into();
        c.primary_foreground = rgb(0xffffff).into();
        c.secondary = rgb(0x24262d).into();
        c.secondary_foreground = rgb(0xd8d5cf).into();
        c.muted = rgb(0x24262d).into();
        c.muted_foreground = rgb(0x8e8b85).into();
        c.input = rgb(0x24262d).into();
        c.popover = rgb(0x1e2026).into();
        c.popover_foreground = rgb(0xd8d5cf).into();
        c.danger = rgb(0xc0605c).into();
        c.success = rgb(0x7fae72).into();
        c.info = rgb(0x6f9bc4).into();
        c.scrollbar = rgb(0x1e2026).into();
        c.scrollbar_thumb = rgb(0x3a3d45).into();
        c.sidebar = rgb(0x1e2026).into();
        c.sidebar_foreground = rgb(0xd8d5cf).into();
        c.sidebar_accent = rgba(0xc96f4a24).into();
        c.sidebar_accent_foreground = rgb(0xd8d5cf).into();
        c.sidebar_border = rgb(0x2e3138).into();
        c.sidebar_primary = rgb(0xc96f4a).into();
        c.sidebar_primary_foreground = rgb(0xffffff).into();
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
                    window.set_window_title("执笔 3.0 — 人写正文，AI 理账");
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
