//! 应用根：书架 ⇄ 工作区两阶段切换。
//! 书架层不带任何工作台镶边（侧栏/轨迹 Dock 都不属于书架）——分层纪律参考旧版执笔。

use gpui_kit::{AppContext as _, Context, Entity, IntoElement, ParentElement, Render, Styled, Window, div};
use gpui_kit::component::Root;

use crate::shelf::{ShelfEvent, ShelfView};
use crate::workspace::{Workspace, WorkspaceEvent};

pub struct AppRoot {
    shelf: Option<Entity<ShelfView>>,
    workspace: Option<Entity<Workspace>>,
}

impl AppRoot {
    pub fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        let shelf = cx.new(|cx| ShelfView::new(window, cx));
        cx.subscribe_in(&shelf, window, |this, _, ev: &ShelfEvent, window, cx| match ev {
            ShelfEvent::Open(dir) => this.open_project(dir.clone(), window, cx),
        })
        .detach();
        Self { shelf: Some(shelf), workspace: None }
    }

    fn open_project(&mut self, dir: std::path::PathBuf, window: &mut Window, cx: &mut Context<Self>) {
        let ws = cx.new(|cx| Workspace::new(dir, window, cx));
        let this = self;
        cx.subscribe_in(&ws, window, |this, _, ev: &WorkspaceEvent, window, cx| match ev {
            WorkspaceEvent::BackToShelf => {
                this.workspace = None;
                // 进工作区时 shelf 被置 None（避免双份状态）；回来必须重建书架，
                // 否则 render 落到空 div —— 就是黑屏。
                let shelf = cx.new(|cx| ShelfView::new(window, cx));
                cx.subscribe_in(&shelf, window, |this, _, ev: &ShelfEvent, window, cx| match ev {
                    ShelfEvent::Open(dir) => this.open_project(dir.clone(), window, cx),
                })
                .detach();
                this.shelf = Some(shelf);
                cx.notify();
            }
        })
        .detach();
        this.workspace = Some(ws);
        this.shelf = None; // 工作区阶段不再渲染书架（避免双份状态）
        cx.notify();
    }
}

impl Render for AppRoot {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        if let Some(ws) = &self.workspace {
            div().size_full().child(ws.clone())
        } else if let Some(shelf) = &self.shelf {
            div().size_full().child(shelf.clone())
        } else {
            div().size_full()
        }
    }
}

// Root 由窗口壳持有（open_window 已包裹），此处仅作类型占位使用。
#[allow(dead_code)]
type _RootAlias = Root;
