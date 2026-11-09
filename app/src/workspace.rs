//! 工作台：标题栏 + [侧栏 | DockArea（中央编辑器 + 右侧轨迹）] + 状态条，
//! 以及 DSH 客户端生命周期与事件分发。

use anyhow::Result;
use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::dock::{
    DockArea, DockEvent, DockLayout, DockPlacement, DockSkin, panel_handle,
};
use gpui_kit::component::sidebar::{Sidebar, SidebarMenu, SidebarMenuItem};
use gpui_kit::component::status_bar::StatusBar;
use gpui_kit::component::{IconName, TitleBar};
use gpui_kit::{
    AppContext as _, AsyncApp, Context, Entity, IntoElement, ParentElement, Render, SharedString,
    Styled, WeakEntity, Window, div, px, rgb,
};
use serde_json::Value;

use crate::dsh::{DshClient, DshEvent, model_route};
use crate::editor::{EditorEvent, EditorPanel};
use crate::trajectory::{TrajKind, TrajectoryPanel};

const SESSION_ID: &str = "zhibi-main";
const DOCK_AREA_ID: &str = "zhibi-main-dock";
const DOCK_AREA_VERSION: usize = 1;

pub struct Workspace {
    dock_area: Entity<DockArea>,
    // 订阅已经建立，句柄保留便于后续直接驱动编辑器
    #[allow(dead_code)]
    editor: Entity<EditorPanel>,
    trajectory: Entity<TrajectoryPanel>,
    client: Option<DshClient>,
    initialized: bool,
}

impl Workspace {
    pub fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        let (dock_area, _skin) =
            DockSkin::dock_area(DOCK_AREA_ID, Some(DOCK_AREA_VERSION), window, cx);

        let editor = cx.new(|cx| EditorPanel::new(window, cx));
        let trajectory = cx.new(|cx| TrajectoryPanel::new(cx));

        // 默认布局：中央编辑器 + 右侧轨迹（布局持久化 M2 接 DockAreaState）
        let center = DockLayout::tabs().panel_view(panel_handle(editor.clone()), cx);
        let right = DockLayout::tabs().panel_view(panel_handle(trajectory.clone()), cx);
        dock_area.update(cx, |area, cx| {
            area.set_center(center, window, cx);
            area.set_dock(DockPlacement::Right, right, window, cx);
            area.set_dock_size(DockPlacement::Right, px(340.), window, cx);
        });

        cx.subscribe_in(&dock_area, window, |_, _, _: &DockEvent, _, _| {}).detach();

        // 编辑器发送 → DSH
        cx.subscribe_in(
            &editor,
            window,
            |this, _, ev: &EditorEvent, window, cx| match ev {
                EditorEvent::SendRequested(text) => this.on_send(text.clone(), window, cx),
            },
        )
        .detach();

        Self {
            dock_area,
            editor,
            trajectory,
            client: None,
            initialized: false,
        }
    }

    fn ensure_client(&mut self, cx: &mut Context<Self>) -> Result<DshClient> {
        if let Some(client) = &self.client {
            return Ok(client.clone());
        }
        let (client, rx) = DshClient::spawn()?;
        self.trajectory.update(cx, |panel, cx| {
            panel.push(
                TrajKind::Info("已启动 DSH 运行时（--profile sdk）".into()),
                cx,
            );
        });

        // 读线程 → UI 事件分发（session.event 流 → 轨迹面板）
        cx.spawn(async move |this, cx| {
            while let Ok(event) = rx.recv().await {
                match event {
                    DshEvent::Notification { method, params } => {
                        let _ = this.update(cx, |ws, cx| {
                            ws.on_notification(&method, params, cx)
                        });
                    }
                    DshEvent::Error(text) => {
                        let _ = this.update(cx, |ws, cx| {
                            ws.trajectory.update(cx, |panel, cx| {
                                panel.push(TrajKind::Conn(text), cx)
                            });
                        });
                        break;
                    }
                }
            }
        })
        .detach();

        self.client = Some(client.clone());
        Ok(client)
    }

    fn on_send(&mut self, text: String, _window: &mut Window, cx: &mut Context<Self>) {
        self.trajectory.update(cx, |panel, cx| {
            panel.push(TrajKind::User(text.clone()), cx);
        });

        let client = match self.ensure_client(cx) {
            Ok(c) => c,
            Err(e) => {
                self.trajectory.update(cx, |panel, cx| {
                    panel.push(TrajKind::Fail(format!("启动 DSH 失败：{e}")), cx);
                });
                return;
            }
        };

        let weak = cx.weak_entity();
        let query_text = text;
        cx.spawn(async move |_, cx| {
            // 首次发送前先完成进程级握手（initialize 只发一次）
            let need_init = weak
                .update(cx, |ws, _| {
                    let need = !ws.initialized;
                    if need {
                        ws.initialized = true;
                    }
                    need
                })
                .unwrap_or(false);
            if need_init {
                if let Err(e) = initialize_with(&client, &weak, cx).await {
                    let _ = weak.update(cx, |ws, cx| {
                        ws.trajectory.update(cx, |panel, cx| {
                            panel.push(TrajKind::Fail(format!("握手失败：{e}")), cx)
                        });
                    });
                    return;
                }
            }
            let r = client.session_query(SESSION_ID, &query_text).await;
            let _ = weak.update(cx, |ws, cx| {
                ws.trajectory.update(cx, |panel, cx| match r {
                    Ok(_) => panel.push(
                        TrajKind::Info("已入队，等待 DSH 会话事件…".into()),
                        cx,
                    ),
                    Err(e) => panel.push(TrajKind::Fail(format!("发送失败：{e}")), cx),
                });
            });
        })
        .detach();
    }

    fn on_notification(&mut self, method: &str, params: Value, cx: &mut Context<Self>) {
        match method {
            "session.event" => {
                if let Some(event) = params.get("event") {
                    self.on_session_event(event, cx);
                }
            }
            "session.status" => {
                let status = params.get("status").and_then(|s| s.as_str()).unwrap_or("?");
                let running = status == "running";
                self.trajectory.update(cx, |panel, cx| {
                    panel.push(
                        if running {
                            TrajKind::Info("会话开始运行".into())
                        } else {
                            TrajKind::Done("会话空闲".into())
                        },
                        cx,
                    );
                });
            }
            "session.query.completed" => {
                let final_response = params
                    .get("finalResponse")
                    .and_then(|f| f.as_str())
                    .unwrap_or("")
                    .trim()
                    .to_string();
                let head: String = final_response.chars().take(120).collect();
                self.trajectory.update(cx, |panel, cx| {
                    panel.push(
                        if head.is_empty() {
                            TrajKind::Done("查询完成".into())
                        } else {
                            TrajKind::Done(format!("查询完成：{head}"))
                        },
                        cx,
                    );
                });
            }
            "subagent.started" => {
                self.trajectory.update(cx, |panel, cx| {
                    panel.push(TrajKind::Info("子 agent 启动".into()), cx)
                });
            }
            "subagent.finished" => {
                self.trajectory.update(cx, |panel, cx| {
                    panel.push(TrajKind::Info("子 agent 结束".into()), cx)
                });
            }
            _ => {}
        }
    }

    fn on_session_event(&mut self, event: &Value, cx: &mut Context<Self>) {
        let kind = event.get("type").and_then(|t| t.as_str()).unwrap_or("");
        let data = event.get("data").cloned().unwrap_or(Value::Null);
        let entry = match kind {
            "turn/start" => {
                let turn = data.get("turn").and_then(|t| t.as_u64()).unwrap_or(0);
                Some(TrajKind::Turn(turn))
            }
            "tool/call" => {
                let name = data
                    .get("name")
                    .and_then(|n| n.as_str())
                    .unwrap_or("?")
                    .to_string();
                let raw = data
                    .get("arguments")
                    .and_then(|a| a.as_str())
                    .unwrap_or("")
                    .to_string();
                let args: String = raw.chars().take(160).collect();
                Some(TrajKind::Tool { name, args })
            }
            "tool/result" => {
                let is_err = data
                    .get("message")
                    .and_then(|m| m.get("isError"))
                    .and_then(|e| e.as_bool())
                    .unwrap_or(false)
                    || data.get("error").is_some();
                let content = summarize_tool_result(&data);
                Some(if is_err {
                    TrajKind::ToolErr(content)
                } else {
                    TrajKind::ToolOk(content)
                })
            }
            "assistant/message" => {
                let text = assistant_text(&data);
                if text.is_empty() {
                    None
                } else {
                    Some(TrajKind::Text(text))
                }
            }
            "turn/end" => {
                // TurnEndReason 是 merge-extensible 的对象：{kind, error?}
                let reason = data.get("reason");
                let kind = reason
                    .and_then(|r| r.get("kind"))
                    .and_then(|k| k.as_str())
                    .unwrap_or("?");
                let message = reason
                    .and_then(|r| r.get("error"))
                    .and_then(|e| e.get("message"))
                    .and_then(|m| m.as_str())
                    .unwrap_or("");
                let entry = match kind {
                    "completed" => TrajKind::Done("本轮完成".into()),
                    "aborted" => TrajKind::Fail("本轮被中止".into()),
                    "blocked" => TrajKind::Fail("本轮被闸门拦下".into()),
                    "error" => {
                        let head: String = if message.is_empty() {
                            "模型调用失败".into()
                        } else {
                            format!("失败：{}", message.chars().take(160).collect::<String>())
                        };
                        TrajKind::Fail(head)
                    }
                    "max-tokens" => TrajKind::Info("本轮触顶输出上限（插件已续跑）".into()),
                    "interrupted" => TrajKind::Fail("本轮因崩溃被补记收尾".into()),
                    "forked" => TrajKind::Info("fork 种子收尾".into()),
                    other => TrajKind::Info(format!("本轮结束：{other}")),
                };
                Some(entry)
            }
            _ => None,
        };
        if let Some(entry) = entry {
            self.trajectory.update(cx, |panel, cx| panel.push(entry, cx));
        }
    }
}

/// 进程级握手：记录 cwd，选择 provider/model 路由。
async fn initialize_with(
    client: &DshClient,
    weak: &WeakEntity<Workspace>,
    cx: &mut AsyncApp,
) -> Result<()> {
    let cwd = std::env::current_dir()
        .map(|p| p.display().to_string())
        .unwrap_or_else(|_| ".".into());
    let (provider, model) = model_route();
    let result = client.initialize(&cwd, &provider, &model).await?;
    let name = result
        .get("serverInfo")
        .and_then(|s| s.get("name"))
        .and_then(|n| n.as_str())
        .unwrap_or("?")
        .to_string();
    weak.update(cx, |ws, cx| {
        ws.trajectory.update(cx, |panel, cx| {
            panel.push(
                TrajKind::Info(format!(
                    "运行时 {name} · provider={provider} model={model}"
                )),
                cx,
            );
        });
    })?;
    Ok(())
}

fn summarize_tool_result(data: &Value) -> String {
    let message = data.get("message");
    let content = message
        .and_then(|m| m.get("content"))
        .and_then(|c| c.as_array())
        .map(|blocks| {
            blocks
                .iter()
                .filter_map(|b| b.get("text").and_then(|t| t.as_str()).map(String::from))
                .collect::<Vec<_>>()
                .join(" ")
        })
        .unwrap_or_default();
    if let Some(err) = data.get("error") {
        let reason = err
            .get("reason")
            .and_then(|r| r.as_str())
            .unwrap_or("失败");
        return reason.chars().take(160).collect();
    }
    let head: String = content.chars().take(160).collect();
    if head.is_empty() {
        "（空结果）".into()
    } else {
        head
    }
}

fn assistant_text(data: &Value) -> String {
    data.get("message")
        .and_then(|m| m.get("content"))
        .and_then(|c| c.as_array())
        .map(|blocks| {
            blocks
                .iter()
                .filter_map(|b| {
                    if b.get("type").and_then(|t| t.as_str()) == Some("text") {
                        b.get("text").and_then(|t| t.as_str())
                    } else {
                        None
                    }
                })
                .collect::<Vec<_>>()
                .join(" ")
        })
        .unwrap_or_default()
        .chars()
        .take(300)
        .collect()
}

impl Render for Workspace {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let dock = self.dock_area.clone();

        let sidebar = Sidebar::new("project-nav")
            .header(
                div()
                    .px_2()
                    .py_1()
                    .text_sm()
                    .font_weight(gpui_kit::FontWeight::MEDIUM)
                    .text_color(rgb(0xd8d5cf))
                    .child("《赤星》"),
            )
            .child(
                SidebarMenu::new()
                    .child(SidebarMenuItem::new("章节").active(true))
                    .child(SidebarMenuItem::new("大纲 · 时间线"))
                    .child(SidebarMenuItem::new("伏笔台账"))
                    .child(SidebarMenuItem::new("设定"))
                    .child(SidebarMenuItem::new("人物卡"))
                    .child(SidebarMenuItem::new("记忆中枢"))
                    .child(SidebarMenuItem::new("提案收件箱")),
            );

        let toggle_right = Button::new("toggle-right-dock")
            .ghost()
            .icon(IconName::PanelRight)
            .on_click(cx.listener(move |this, _, window, cx| {
                this.dock_area.update(cx, |area, cx| {
                    area.toggle_dock(DockPlacement::Right, window, cx);
                });
            }));

        let dsh_state = if self.client.is_some() {
            "已连接"
        } else {
            "未启动"
        };

        div()
            .flex()
            .flex_col()
            .size_full()
            .bg(rgb(0x17181c))
            .text_color(rgb(0xd8d5cf))
            .child(
                TitleBar::new().child(
                    div()
                        .flex()
                        .items_baseline()
                        .gap_2()
                        .child(
                            div()
                                .px_1()
                                .text_sm()
                                .text_color(rgb(0xd8d5cf))
                                .child("执笔 3.0 — 人写正文，AI 理账"),
                        )
                        .child(
                            div()
                                .text_xs()
                                .text_color(rgb(0x5d5b57))
                                .child(SharedString::from(format!("DSH：{dsh_state}"))),
                        ),
                ),
            )
            .child(
                div()
                    .flex()
                    .flex_1()
                    .min_h_0()
                    .child(sidebar.w(px(190.)))
                    .child(div().flex_1().min_h_0().child(dock)),
            )
            .child(StatusBar::new().right(toggle_right))
    }
}
