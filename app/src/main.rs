//! 执笔 3.0 —— 跨端 AI 小说写作工作台（GPUI 前端 + DSH agent 底座）。
//! 两层结构：书架首页（与内容严格分层，参考旧版执笔）→ 打开作品进三栏工作区。

mod app_root;
mod dsh;
mod editor;
mod inbox;
mod prompts;
mod shelf;
mod store;
mod trajectory;
mod views;
mod workspace;

use gpui_kit::{
    AppContext as _, Bounds, Size, WindowBounds, WindowKind, WindowOptions, component::TitleBar,
    point, px,
};

fn main() {
    gpui_kit::application()
        .with_assets(gpui_kit::assets::Assets)
        .run(|cx| {
            gpui_kit::init(cx);
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
