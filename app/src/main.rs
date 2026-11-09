//! 执笔 3.0 —— 跨端 AI 小说写作工作台（GPUI 前端 + DSH agent 底座）。
//! M1：窗口三栏骨架 + `dsh --profile sdk` 子进程连通，轨迹面板实时滚动。

mod dsh;
mod editor;
mod trajectory;
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
            cx.activate(true);

            let bounds = WindowBounds::Windowed(Bounds {
                origin: point(px(80.), px(60.)),
                size: Size {
                    width: px(1440.),
                    height: px(920.),
                },
            });
            let options = WindowOptions {
                window_bounds: Some(bounds),
                window_min_size: Some(Size {
                    width: px(960.),
                    height: px(600.),
                }),
                kind: WindowKind::Normal,
                ..TitleBar::window_options()
            };

            let (handle, root) = gpui_kit::open_window(options, cx, |window, cx| {
                cx.new(|cx| workspace::Workspace::new(window, cx))
            })
            .expect("执笔：打开主窗口失败");

            handle
                .update(cx, |_view, window, _cx| {
                    window.set_window_title("执笔 3.0 — 人写正文，AI 理账");
                    window.activate_window();
                })
                .expect("failed to update window");

            // 关掉窗口就退出应用
            root.update(cx, |_, cx| {
                cx.on_release(|_, cx| {
                    cx.quit();
                })
                .detach();
            });
        });
}
