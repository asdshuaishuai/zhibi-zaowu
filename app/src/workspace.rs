//! 工作区：标题栏 + [侧栏(真章节树) | DockArea(编辑器 + 右侧轨迹)] + 状态条。
//! 职责：项目句柄、章节切换与正文保存、DSH 生命周期与事件分发。

use anyhow::Result;
use gpui_kit::component::button::{Button, ButtonCustomVariant, ButtonVariants as _};
use gpui_kit::component::dock::{
    DockArea, DockEvent, DockLayout, DockPlacement, DockSkin, panel_handle,
};
use gpui_kit::component::input::TextareaState;
use gpui_kit::component::spinner::Spinner;
use gpui_kit::component::status_bar::StatusBar;
use gpui_kit::assets::IconName;
use gpui_kit::component::{Icon, Sizable, TitleBar, WindowExt as _};
use gpui_kit::{
    AnyElement, App, AppContext as _, AsyncApp, Context, Entity, EventEmitter, FontWeight,
    InteractiveElement, IntoElement, ParentElement, Render, SharedString,
    StatefulInteractiveElement, Styled, Window, div, relative,
};
use serde_json::Value;
use std::rc::Rc;

use crate::chat::{ChatEvent, ChatMsg, ChatPanel};
use crate::canvas::{CanvasPanel, OracleEvent, OraclePanel};
use crate::director::{DirectorEvent, DirectorPanel};
use crate::dsh::{DshClient, DshEvent, model_route};
use crate::editor::{EditorEvent, EditorPanel};
use crate::inbox::{InboxEvent, InboxPanel};
use crate::prompts;
use crate::store::{self, ProjectHandle};
use crate::theme;
use crate::trajectory::{TrajEvent, TrajKind, TrajectoryPanel};
use crate::views::{
    CanonPanel, CharactersPanel, CluesPanel, GraphPanel, ImportEvent, ImportPanel, MemoryPanel,
    OutlinePanel, SettingsEvent, SettingsPanel, SnapEvent, SnapshotsPanel, StatsPanel, ViewEvent,
};

gpui_kit::actions!(zhibi, [SaveProse, ToggleFocusMode, BranchesNow]);

const DOCK_AREA_ID: &str = "zhibi-main-dock";
const DOCK_AREA_VERSION: usize = 1;

#[derive(Debug, Clone)]
pub enum WorkspaceEvent {
    BackToShelf,
}

#[derive(PartialEq, Clone, Copy)]
pub enum Mode {
    Editor,
    Outline,
    Canon,
    Memory,
    Stats,
    Clues,
    Characters,
    Settings,
    Snapshots,
    Import,
    Graph,
    Canvas,
    Director,
    Oracle,
}

impl Mode {
    fn label(&self) -> &'static str {
        match self {
            Mode::Editor => "章节",
            Mode::Outline => "大纲 · 时间线",
            Mode::Canon => "设定",
            Mode::Memory => "记忆中枢",
            Mode::Stats => "写作统计",
            Mode::Clues => "伏笔台账",
            Mode::Characters => "人物卡",
            Mode::Settings => "设置",
            Mode::Snapshots => "快照",
            Mode::Import => "导入旧稿",
            Mode::Graph => "记忆图谱",
            Mode::Canvas => "剧情画板",
            Mode::Director => "导演台",
            Mode::Oracle => "预言书",
        }
    }

    /// 侧栏导航行的图标（原型 workspace.html:188-201 的 lucide 对应位）。
    fn icon(&self) -> IconName {
        match self {
            Mode::Editor => IconName::SquarePen,
            Mode::Outline => IconName::ScrollText,
            Mode::Canon => IconName::BookMarked,
            Mode::Memory => IconName::Brain,
            Mode::Stats => IconName::ChartColumn,
            Mode::Clues => IconName::Link,
            Mode::Characters => IconName::Users,
            Mode::Settings => IconName::Settings,
            Mode::Snapshots => IconName::Camera,
            Mode::Import => IconName::FolderInput,
            Mode::Graph => IconName::Network,
            Mode::Canvas => IconName::Waypoints,
            Mode::Director => IconName::Kanban,
            Mode::Oracle => IconName::Sparkle,
        }
    }
}

/// 侧栏导航顺序（与原实现一致，只是改为数据表以便统一出行）。
const NAV: [Mode; 14] = [
    Mode::Editor,
    Mode::Outline,
    Mode::Canon,
    Mode::Memory,
    Mode::Stats,
    Mode::Clues,
    Mode::Characters,
    Mode::Canvas,
    Mode::Director,
    Mode::Oracle,
    Mode::Graph,
    Mode::Snapshots,
    Mode::Import,
    Mode::Settings,
];

/// 颜色 token 是 `Rgba` 函数（theme.rs 约定），组件库色位要 `Hsla`：Rgba: Into<Hsla>。
fn hsla(c: gpui_kit::Rgba) -> gpui_kit::Hsla {
    c.into()
}

/// iconbtn 语言（design.css:36）：次级面板底 + 描边 + 次级文字，hover 提亮转朱砂描边。
fn icon_btn(cx: &App) -> ButtonCustomVariant {
    ButtonCustomVariant::new(cx)
        .color(hsla(theme::PANEL2()))
        .foreground(hsla(theme::DIM()))
        .hover(hsla(theme::ACCENT_SOFT()))
        .active(hsla(theme::LINE()))
}

/// 主能力按钮语言（design.css:37 .iconbtn.primary、workspace.html:111 .prominent）：
/// 朱砂软底 + 正文色文字。
fn prominent_btn(cx: &App) -> ButtonCustomVariant {
    ButtonCustomVariant::new(cx)
        .color(hsla(theme::ACCENT_SOFT()))
        .foreground(hsla(theme::TEXT()))
        .hover(hsla(theme::ACCENT_BORDER()))
        .active(hsla(theme::LINE()))
}

/// 语义 chip（规则 6：FONT_TAG + RADIUS_CHIP + 1px 语义描边，不用裸灰字表达状态）。
fn chip(text: &str, fg: gpui_kit::Rgba, bg: gpui_kit::Rgba, border: gpui_kit::Rgba) -> AnyElement {
    // kit Tag::custom：语义三色由 token 传入（规则 6/10）
    return gpui_kit::component::tag::Tag::custom(bg.into(), fg.into(), border.into())
        .outline()
        .child(SharedString::from(text.to_string()))
        .into_any_element();
    #[allow(unreachable_code)]
    let text: SharedString = text.into();
    div()
        .flex()
        .items_center()
        .flex_shrink_0()
        .px(theme::PAD_SM)
        .rounded(theme::RADIUS_CHIP)
        .border_1()
        .border_color(border)
        .bg(bg)
        .text_size(theme::FONT_TAG)
        // 原型 .chip 高 18px = 10.5px × 1.71（design.css:39），故行高取 1.7
        .line_height(relative(1.7))
        .text_color(fg)
        .child(text)
        .into_any_element()
}

/// 运行中 chip（规则 6/15：BLUE 系 + spinner，不能靠静默表达进行中）。
fn live_chip(text: &str) -> AnyElement {
    let text: SharedString = text.into();
    gpui_kit::component::tag::Tag::custom(
        theme::BLUE_SOFT().into(),
        theme::BLUE().into(),
        theme::BLUE_BORDER().into(),
    )
    .outline()
    .child(
        div()
            .flex()
            .items_center()
            .gap(theme::GAP_XS)
            .child(
                Spinner::new()
                    .with_size(theme::FONT_TAG)
                    .color(hsla(theme::BLUE())),
            )
            .child(text),
    )
    .into_any_element()
}

impl EventEmitter<WorkspaceEvent> for Workspace {}

pub struct Workspace {
    root: std::path::PathBuf,
    handle: Rc<ProjectHandle>,
    dock_area: Entity<DockArea>,
    inbox: Entity<InboxPanel>,
    /// 正文读写走 prose Entity；分支卡经 set_branches 同步
    editor: Entity<EditorPanel>,
    canon_view: Entity<CanonPanel>,
    outline_view: Entity<OutlinePanel>,
    memory_view: Entity<MemoryPanel>,
    stats_view: Entity<StatsPanel>,
    clues_view: Entity<CluesPanel>,
    characters_view: Entity<CharactersPanel>,
    settings_view: Entity<SettingsPanel>,
    snapshots_view: Entity<SnapshotsPanel>,
    import_view: Entity<ImportPanel>,
    graph_view: Entity<GraphPanel>,
    canvas_view: Entity<CanvasPanel>,
    director_view: Entity<DirectorPanel>,
    oracle_view: Entity<OraclePanel>,
    chat: Entity<ChatPanel>,
    /// 分支卡数据（与 EditorPanel 同步；采纳读这里）
    branches: Vec<String>,
    toast: Option<SharedString>,
    toast_seq: u64,
    mode: Mode,
    trajectory: Entity<TrajectoryPanel>,
    prose: Entity<TextareaState>,
    chapters: Vec<store::ChapterInfo>,
    chapter: u32,
    client: Option<DshClient>,
    initialized: bool,
    autosave: Option<gpui_kit::Task<()>>,
    /// 最近一次落盘的时刻串（与 EditorPanel 同源）；底栏「已保存 / 未保存」chip 只读它是否非空，
    /// 不显示这个 UTC 串本体（见 save_current 的 NOTE(utc)）
    last_saved: String,
    /// DSH 会话是否正在跑（session.status 事件驱动，标题栏 BLUE 运行态的事实源）
    dsh_running: bool,
    focus: bool,
    dock_hidden: bool,
    workspace_focus: gpui_kit::FocusHandle,
    /// 选区创作请求的原文（应答到达时转成 SelectionDraft 气泡）
    pending_sel: Option<String>,
}

impl Workspace {
    pub fn new(root: std::path::PathBuf, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let handle = Rc::new(ProjectHandle::open(&root).expect("项目打开失败"));
        let mut chapters = handle.chapters();
        // 默认落到最近一章（有正文的最后一章），没有则建第 1 章
        let chapter = chapters
            .iter()
            .rev()
            .find(|c| c.has_prose)
            .or_else(|| chapters.last())
            .map(|c| c.n)
            .unwrap_or(1);
        let _ = handle.ensure_chapter(chapter);
        let prose_text = handle.prose(chapter);
        chapters = handle.chapters();

        let (dock_area, _skin) =
            DockSkin::dock_area(DOCK_AREA_ID, Some(DOCK_AREA_VERSION), window, cx);

        let prose = cx.new(|cx| TextareaState::new(window, cx).auto_grow(6, 10_000));
        prose.update(cx, |state, cx| {
            state.set_value(prose_text.as_str(), window, cx);
        });

        let editor =
            cx.new(|cx| EditorPanel::new(window, cx, prose.clone(), handle.chapter_word_target()));
        {
            let title = chapters
                .iter()
                .find(|c| c.n == chapter)
                .map(|c| c.title.clone())
                .unwrap_or_default();
            let notes = handle.chapter_notes(chapter);
            editor.update(cx, |panel, cx| {
                panel
                    .title
                    .update(cx, |state, cx| state.set_value(title.as_str(), window, cx));
                panel
                    .notes
                    .update(cx, |state, cx| state.set_value(notes.as_str(), window, cx));
            });
        }
        let trajectory = cx.new(|cx| TrajectoryPanel::new(cx));
        let inbox = cx.new(|cx| InboxPanel::new(window, cx, handle.clone()));

        cx.subscribe_in(
            &inbox,
            window,
            |this, _, ev: &InboxEvent, _window, cx| match ev {
                InboxEvent::Accept(id) => this.accept_proposal(id, cx),
                InboxEvent::Reject(id, reason) => this.reject_proposal(id, &reason, cx),
                InboxEvent::Refresh => {
                    this.inbox.update(cx, |panel, cx| panel.refresh(cx));
                }
            },
        )
        .detach();

        let canon_view = cx.new(|cx| CanonPanel::new(cx, handle.clone()));
        let outline_view = cx.new(|cx| OutlinePanel::new(cx, handle.clone()));
        let memory_view = cx.new(|cx| MemoryPanel::new(cx, handle.clone()));
        let stats_view = cx.new(|cx| StatsPanel::new(cx, handle.clone()));
        let clues_view = cx.new(|cx| CluesPanel::new(cx, handle.clone()));
        let characters_view = cx.new(|cx| CharactersPanel::new(cx, handle.clone()));
        let settings_view = cx.new(|cx| SettingsPanel::new(window, cx));
        let snapshots_view = cx.new(|cx| SnapshotsPanel::new(cx, handle.clone(), chapter));
        let import_view = cx.new(|cx| ImportPanel::new(window, cx, handle.clone()));
        let graph_view = cx.new(|cx| GraphPanel::new(cx, handle.clone()));
        let canvas_view = cx.new(|cx| CanvasPanel::new(cx, handle.clone()));
        let director_view = cx.new(|cx| DirectorPanel::new(cx, handle.clone(), chapter));
        let oracle_view = cx.new(|cx| OraclePanel::new(window, cx, handle.clone()));
        let chat = cx.new(|cx| ChatPanel::new(window, cx));

        cx.subscribe_in(
            &import_view,
            window,
            |this, _, ev: &ImportEvent, _window, cx| match ev {
                ImportEvent::Run(path) => {
                    if path.trim().is_empty() {
                        this.import_view.update(cx, |panel, cx| {
                            panel.set_result("先填目录路径".into(), cx);
                        });
                        return;
                    }
                    let path = std::path::PathBuf::from(path);
                    // Rc 不能跨线程：把句柄拆成纯数据传入后台
                    let root = this.handle.root.clone();
                    let title = this.handle.title.clone();
                    let import_view = this.import_view.clone();
                    let inbox = this.inbox.clone();
                    let traj = this.trajectory.clone();
                    cx.spawn(async move |_, cx| {
                        let result = cx
                            .background_executor()
                            .spawn(async move {
                                let h = ProjectHandle { root, title };
                                h.import_dir(&path)
                            })
                            .await;
                        let _ = cx.update(|cx| {
                            let msg = match result {
                                Ok((d, c)) => format!(
                                    "扫描完成：草稿提案 {d} 份 · 设定提案 {c} 份——去收件箱逐条采纳"
                                ),
                                Err(e) => format!("导入失败：{e}"),
                            };
                            import_view.update(cx, |panel, cx| panel.set_result(msg.clone(), cx));
                            inbox.update(cx, |panel, cx| panel.refresh(cx));
                            traj.update(cx, |panel, cx| panel.push(TrajKind::Info(msg), cx));
                        });
                    })
                    .detach();
                }
            },
        )
        .detach();

        cx.subscribe_in(
            &settings_view,
            window,
            |this, _, ev: &SettingsEvent, _window, cx| match ev {
                SettingsEvent::Save {
                    base_url,
                    model,
                    provider,
                } => {
                    let cfg = crate::store::AppSettings {
                        base_url: base_url.clone(),
                        model: model.clone(),
                        provider: provider.clone(),
                    };
                    let msg = match crate::store::save_settings(&cfg) {
                        Ok(()) => "设置已保存".to_string(),
                        Err(e) => format!("保存失败：{e}"),
                    };
                    this.trajectory.update(cx, |panel, cx| {
                        panel.push(TrajKind::Info(msg), cx);
                    });
                }
                SettingsEvent::Test(base_url) => {
                    let base_url = base_url.clone();
                    let settings = this.settings_view.clone();
                    cx.spawn(async move |_, cx| {
                        let url = format!("{}/models", base_url.trim_end_matches('/'));
                        let result = cx
                            .background_executor()
                            .spawn(async move {
                                ureq::get(&url)
                                    .timeout(std::time::Duration::from_secs(8))
                                    .call()
                            })
                            .await;
                        let ok = result.is_ok();
                        let msg = match result {
                            Ok(resp) => {
                                let body = resp.into_string().unwrap_or_default();
                                let n = body.matches("\"").count() / 2;
                                format!("连通成功，服务端清单约 {n} 项")
                            }
                            Err(e) => format!("连接失败：{e}"),
                        };
                        let _ = cx.update(|cx| {
                            settings.update(cx, |panel, cx| panel.set_result(ok, msg, cx));
                        });
                    })
                    .detach();
                }
            },
        )
        .detach();

        cx.subscribe_in(
            &snapshots_view,
            window,
            |this, _, ev: &SnapEvent, window, cx| match ev {
                SnapEvent::Restore(name) => {
                    let n = this.chapter;
                    match this.handle.restore_snapshot(n, name) {
                        Ok(()) => {
                            let text = this.handle.prose(n);
                            this.prose.update(cx, |state, cx| {
                                state.set_value(text.as_str(), window, cx);
                            });
                            this.snapshots_view
                                .update(cx, |panel, cx| panel.reload(n, cx));
                            this.trajectory.update(cx, |panel, cx| {
                                panel.push(TrajKind::Info(format!("已恢复快照「{name}」")), cx);
                            });
                        }
                        Err(e) => {
                            this.trajectory.update(cx, |panel, cx| {
                                panel.push(TrajKind::Fail(format!("恢复失败：{e}")), cx);
                            });
                        }
                    }
                }
            },
        )
        .detach();

        cx.subscribe_in(
            &canon_view,
            window,
            |this, _, ev: &ViewEvent, window, cx| match ev {
                ViewEvent::RunCapability(cap) => {
                    let task = prompts::task_for(cap, this.chapter);
                    this.on_send(task, cx);
                    this.trajectory.update(cx, |panel, cx| {
                        panel.push(
                            TrajKind::Info("台账页触发——产出进收件箱，轨迹窗全程可见".into()),
                            cx,
                        );
                    });
                    let _ = window;
                }
            },
        )
        .detach();
        cx.subscribe_in(
            &outline_view,
            window,
            |this, _, ev: &ViewEvent, _window, cx| match ev {
                ViewEvent::RunCapability(cap) => {
                    let task = prompts::task_for(cap, this.chapter);
                    this.on_send(task, cx);
                }
            },
        )
        .detach();
        cx.subscribe_in(
            &memory_view,
            window,
            |this, _, ev: &ViewEvent, _window, cx| match ev {
                ViewEvent::RunCapability(cap) => {
                    let task = prompts::task_for(cap, this.chapter);
                    this.on_send(task, cx);
                }
            },
        )
        .detach();
        cx.subscribe_in(
            &clues_view,
            window,
            |this, _, ev: &ViewEvent, _window, cx| match ev {
                ViewEvent::RunCapability(cap) => {
                    let task = prompts::task_for(cap, this.chapter);
                    this.on_send(task, cx);
                }
            },
        )
        .detach();

        let center = DockLayout::tabs().panel_view(panel_handle(editor.clone()), cx);
        let right = DockLayout::tabs()
            .panel_view(panel_handle(trajectory.clone()), cx)
            .panel_view(panel_handle(inbox.clone()), cx)
            .panel_view(panel_handle(chat.clone()), cx);
        dock_area.update(cx, |area, cx| {
            area.set_center(center, window, cx);
            area.set_dock(DockPlacement::Right, right, window, cx);
            area.set_dock_size(DockPlacement::Right, theme::DOCK_W, window, cx);
        });

        cx.subscribe_in(&dock_area, window, |_, _, _: &DockEvent, _, _| {})
            .detach();

        cx.subscribe_in(&chat, window, |this, _, ev: &ChatEvent, _window, cx| match ev {
            ChatEvent::Send(text) => {
                this.chat.update(cx, |panel, cx| {
                    panel.push(ChatMsg { role: "user".into(), text: text.clone() }, cx)
                });
                this.on_send(text.clone(), cx);
            }
        })
        .detach();

        // 预言书：开线推演 → 走同一条 DSH 会话（轨迹窗全程可见）；晋升 → 刷新收件箱
        cx.subscribe_in(
            &oracle_view,
            window,
            |this, _, ev: &OracleEvent, _window, cx| match ev {
                OracleEvent::Simulate { premise, fork } => {
                    let fork = if *fork == 0 { this.chapter } else { *fork };
                    this.on_send(prompts::oracle_task(premise, fork), cx);
                }
                OracleEvent::Forecast => {
                    this.on_send(prompts::forecast_task(), cx);
                }
                OracleEvent::Promoted { worldline, proposal_id } => {
                    this.inbox.update(cx, |panel, cx| panel.refresh(cx));
                    let (wl, pid) = (worldline.clone(), proposal_id.clone());
                    this.trajectory.update(cx, |panel, cx| {
                        panel.push(
                            TrajKind::Info(format!("线{wl} 影响晋升 → 提案 {pid} 进收件箱（正典未动）")),
                            cx,
                        );
                    });
                }
            },
        )
        .detach();

// 导演台：AI 核对走 DSH 会话（轨迹窗可见），拖卡改期是作者亲笔直写
        cx.subscribe(
            &director_view,
            |this, _, ev: &DirectorEvent, cx| match ev {
                DirectorEvent::Review(chap) => {
                    let ch = if *chap == 0 { this.chapter } else { *chap };
                    this.on_send(prompts::director_task(ch), cx);
                }
            },
        )
        .detach();

        cx.subscribe(
            &prose,
            |this, _, ev: &gpui_kit::component::input::InputEvent, cx| {
                if matches!(ev, gpui_kit::component::input::InputEvent::Change) {
                    this.schedule_autosave(cx);
                }
            },
        )
        .detach();

        // 轨迹气泡「采纳到正文」：先快照，再追加——共写落笔永远在作者一键之后
        cx.subscribe_in(
            &trajectory,
            window,
            |this, _, ev: &TrajEvent, window, cx| match ev {
                TrajEvent::AdoptSelection(text) => {
                    let _ = this.handle.snapshot_prose(this.chapter);
                    this.prose.update(cx, |state, cx| {
                        state.replace(text.as_str(), window, cx);
                    });
                    this.save_current(cx);
                    this.trajectory.update(cx, |panel, cx| {
                        panel.push(TrajKind::Info("选区已替换（快照已存）".into()), cx);
                    });
                }
                TrajEvent::Adopt(text) => {
                    let _ = this.handle.snapshot_prose(this.chapter);
                    let cur = this.prose.read(cx).value().to_string();
                    let sep = if cur.ends_with('\n') || cur.is_empty() {
                        ""
                    } else {
                        "\n"
                    };
                    let next = format!("{cur}{sep}{text}");
                    this.prose.update(cx, |state, cx| {
                        state.set_value(next.as_str(), window, cx);
                    });
                    this.save_current(cx);
                    this.trajectory.update(cx, |panel, cx| {
                        panel.push(TrajKind::Info("已采纳到正文（快照已存）".into()), cx);
                    });
                }
            },
        )
        .detach();

        cx.subscribe_in(
            &editor,
            window,
            |this, _, ev: &EditorEvent, window, cx| match ev {
                EditorEvent::SendRequested(text) => this.on_send(text.clone(), cx),
                EditorEvent::SaveRequested => this.save_current(cx),
                EditorEvent::RunCapability(cap) => {
                    let mut task = prompts::task_for(cap, this.chapter);
                    if cap == "memory" {
                        let notes = this.editor.read(cx).notes.read(cx).value().to_string();
                        if !notes.trim().is_empty() {
                            task.push_str("\n\n## 作者的随手记（请核实正文后一并归档或修正）\n");
                            task.push_str(notes.trim());
                        }
                    }
                    this.on_send(task, cx);
                }
                EditorEvent::AdoptBranch(ix) => {
                    if let Some(body) = this.branches.get(*ix).cloned() {
                        let _ = this.handle.snapshot_prose(this.chapter);
                        // 去掉【X】标记行，取走向正文
                        let content = body
                            .splitn(2, '】')
                            .nth(1)
                            .map(|t| t.trim_start_matches(['\n', ' ', '\r']).to_string())
                            .unwrap_or(body.clone());
                        let cur = this.prose.read(cx).value().to_string();
                        let sep = if cur.is_empty() || cur.ends_with('\n') { "" } else { "\n\n" };
                        let next = format!("{cur}{sep}{content}");
                        this.prose.update(cx, |state, cx| {
                            state.set_value(next.as_str(), window, cx);
                        });
                        this.branches.clear();
                        this.editor.update(cx, |panel, cx| {
                            panel.set_branches(Vec::new(), cx)
                        });
                        this.save_current(cx);
                        this.show_toast("已采纳走向（快照已存）", cx);
                        this.trajectory.update(cx, |panel, cx| {
                            panel.push(TrajKind::Info("走向已并入正文 · 快照已存".into()), cx);
                        });
                    }
                }
                EditorEvent::RerollBranches => {
                    let task = prompts::task_for("branches", this.chapter);
                    this.on_send(task, cx);
                }
                EditorEvent::DismissBranches => {
                    this.branches.clear();
                    this.editor.update(cx, |panel, cx| panel.set_branches(Vec::new(), cx));
                    this.show_toast("走向已收起", cx);
                }
                EditorEvent::SelectionAction(kind) => {
                    let selected = this.prose.read(cx).selected_text().to_string();
                    if selected.trim().is_empty() {
                        this.trajectory.update(cx, |panel, cx| {
                            panel.push(
                                TrajKind::Info("先在稿纸里选中一段，再用选区创作".into()),
                                cx,
                            );
                        });
                        return;
                    }
                    this.pending_sel = Some(selected.clone());
                    let task = prompts::selection_instruction(kind, &selected);
                    this.on_send(task, cx);
                }
            },
        )
        .detach();

        Self {
            root,
            handle,
            dock_area,
            inbox,
            editor,
            canon_view,
            outline_view,
            memory_view,
            stats_view,
            clues_view,
            characters_view,
            settings_view,
            snapshots_view,
            import_view,
            graph_view,
            canvas_view,
            director_view,
            oracle_view,
            chat,
            branches: Vec::new(),
            toast: None,
            toast_seq: 0,
            mode: Mode::Editor,
            trajectory,
            prose,
            chapters,
            chapter,
            client: None,
            initialized: false,
            pending_sel: None,
            autosave: None,
            last_saved: String::new(),
            dsh_running: false,
            focus: false,
            dock_hidden: false,
            workspace_focus: cx.focus_handle(),
        }
    }

    /// 输入变化 → 1.5 秒防抖自动保存（先落盘纪律）
    fn show_toast(&mut self, msg: &str, cx: &mut Context<Self>) {
        self.toast_seq += 1;
        let seq = self.toast_seq;
        self.toast = Some(SharedString::from(msg.to_string()));
        cx.notify();
        let weak = cx.weak_entity();
        cx.spawn(async move |_, cx| {
            cx.background_executor()
                .timer(std::time::Duration::from_millis(2500))
                .await;
            let _ = weak.update(cx, |ws, cx| {
                if ws.toast_seq == seq {
                    ws.toast = None;
                    cx.notify();
                }
            });
        })
        .detach();
    }

    fn focus_handle(&self, _cx: &Context<Self>) -> gpui_kit::FocusHandle {
        self.workspace_focus.clone()
    }

    fn schedule_autosave(&mut self, cx: &mut Context<Self>) {
        let weak = cx.weak_entity();
        self.autosave = Some(cx.spawn(async move |_, cx| {
            cx.background_executor()
                .timer(std::time::Duration::from_millis(1500))
                .await;
            let _ = weak.update(cx, |ws, cx| {
                ws.save_current(cx);
                ws.autosave = None;
            });
        }));
    }

    fn accept_proposal(&mut self, id: &str, cx: &mut Context<Self>) {
        match self.handle.accept_proposal(id) {
            Ok((applied, skipped)) => {
                let note = if skipped.is_empty() {
                    format!("采纳完成：{} 项入库", applied)
                } else {
                    format!("采纳完成：{} 项入库；跳过 {}", applied, skipped.join("、"))
                };
                self.trajectory.update(cx, |panel, cx| {
                    panel.push(TrajKind::Info(note), cx);
                });
            }
            Err(e) => {
                self.trajectory.update(cx, |panel, cx| {
                    panel.push(TrajKind::Fail(format!("采纳失败：{e}")), cx);
                });
            }
        }
        self.inbox.update(cx, |panel, cx| panel.refresh(cx));
        self.chapters = self.handle.chapters();
        cx.notify();
    }

    fn reject_proposal(&mut self, id: &str, reason: &str, cx: &mut Context<Self>) {
        match self.handle.reject_proposal(id, reason) {
            Ok(()) => {
                self.trajectory.update(cx, |panel, cx| {
                    panel.push(
                        TrajKind::Info(format!("已拒收（理由将回灌）：{reason}")),
                        cx,
                    );
                });
            }
            Err(e) => {
                self.trajectory.update(cx, |panel, cx| {
                    panel.push(TrajKind::Fail(format!("拒收失败：{e}")), cx);
                });
            }
        }
        self.inbox.update(cx, |panel, cx| panel.refresh(cx));
    }

    fn switch_center(&mut self, mode: Mode, window: &mut Window, cx: &mut Context<Self>) {
        if self.mode == mode {
            return;
        }
        // 离开编辑器先存正文
        if self.mode == Mode::Editor {
            self.save_current(cx);
        }
        self.mode = mode;
        let view = match mode {
            Mode::Editor => panel_handle(self.editor.clone()),
            Mode::Outline => panel_handle(self.outline_view.clone()),
            Mode::Canon => panel_handle(self.canon_view.clone()),
            Mode::Memory => panel_handle(self.memory_view.clone()),
            Mode::Stats => panel_handle(self.stats_view.clone()),
            Mode::Clues => panel_handle(self.clues_view.clone()),
            Mode::Characters => panel_handle(self.characters_view.clone()),
            Mode::Settings => panel_handle(self.settings_view.clone()),
            Mode::Snapshots => {
                self.snapshots_view
                    .update(cx, |panel, cx| panel.reload(self.chapter, cx));
                panel_handle(self.snapshots_view.clone())
            }
            Mode::Import => panel_handle(self.import_view.clone()),
            Mode::Graph => panel_handle(self.graph_view.clone()),
            Mode::Canvas => {
                self.canvas_view.update(cx, |panel, _cx| panel.refresh());
                panel_handle(self.canvas_view.clone())
            }
            Mode::Director => {
                self.director_view.update(cx, |panel, _cx| panel.refresh());
                panel_handle(self.director_view.clone())
            }
            Mode::Oracle => panel_handle(self.oracle_view.clone()),
        };
        let center = DockLayout::tabs().panel_view(view, cx);
        self.dock_area.update(cx, |area, cx| {
            area.set_center(center, window, cx);
        });
        cx.notify();
    }

    fn word_count(&self, cx: &Context<Self>) -> usize {
        store::count_words(&self.prose.read(cx).value())
    }

    fn save_current(&mut self, cx: &mut Context<Self>) {
        let text = self.prose.read(cx).value().to_string();
        let title = self.editor.read(cx).title.read(cx).value().to_string();
        if !title.trim().is_empty() {
            let _ = self.handle.set_chapter_title(self.chapter, title.trim());
        }
        let notes = self.editor.read(cx).notes.read(cx).value().to_string();
        let _ = self.handle.set_chapter_notes(self.chapter, &notes);
        // NOTE(utc): 此处按 UNIX 秒取时分秒，输出的是 UTC 而非本地时间（与
        // trajectory.rs:147 utc_hhmmss 同源缺陷）。规则 15 要求本地/相对时间，但本地
        // 偏移需要 chrono（app/Cargo.toml 未引入，见其 dependencies 仅有 anyhow/serde/
        // serde_json/async-channel/ureq），标准库无本地时区 API，故留待依赖变更时处理。
        let stamp = {
            let secs = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_secs())
                .unwrap_or(0);
            format!(
                "{:02}:{:02}:{:02}",
                (secs / 3600) % 24,
                (secs / 60) % 60,
                secs % 60
            )
        };
        let chip_stamp = stamp.clone();
        self.editor
            .update(cx, |panel, cx| panel.set_saved_at(stamp, cx));
        if let Err(e) = self.handle.save_prose(self.chapter, &text) {
            self.trajectory.update(cx, |panel, cx| {
                panel.push(TrajKind::Fail(format!("正文保存失败：{e}")), cx);
            });
            return;
        }
        // 只有真的写盘成功才点亮底栏「已保存」，失败路径保持原状态
        self.last_saved = chip_stamp;
        self.chapters = self.handle.chapters();
        cx.notify();
    }

    fn switch_chapter(&mut self, n: u32, window: &mut Window, cx: &mut Context<Self>) {
        if n == self.chapter {
            return;
        }
        self.save_current(cx);
        let _ = self.handle.ensure_chapter(n);
        let prose_text = self.handle.prose(n);
        let title = self
            .chapters
            .iter()
            .find(|c| c.n == n)
            .map(|c| c.title.clone())
            .unwrap_or_default();
        self.chapter = n;
        self.prose.update(cx, |state, cx| {
            state.set_value(prose_text.as_str(), window, cx);
        });
        let notes = self.handle.chapter_notes(n);
        self.editor.update(cx, |panel, cx| {
            panel.title.update(cx, |state, cx| {
                state.set_value(title.as_str(), window, cx);
            });
            panel.notes.update(cx, |state, cx| {
                state.set_value(notes.as_str(), window, cx);
            });
        });
        self.chapters = self.handle.chapters();
        cx.notify();
    }

    fn ensure_client(&mut self, cx: &mut Context<Self>) -> Result<DshClient> {
        if let Some(client) = &self.client {
            return Ok(client.clone());
        }
        let (client, rx) = DshClient::spawn(&self.root)?;
        self.trajectory.update(cx, |panel, cx| {
            panel.push(
                TrajKind::Info("已启动 DSH 运行时（--profile sdk）".into()),
                cx,
            );
        });

        cx.spawn(async move |this, cx| {
            while let Ok(event) = rx.recv().await {
                match event {
                    DshEvent::Notification { method, params } => {
                        let _ = this.update(cx, |ws, cx| ws.on_notification(&method, params, cx));
                    }
                    DshEvent::Error(text) => {
                        let _ = this.update(cx, |ws, cx| {
                            ws.trajectory
                                .update(cx, |panel, cx| panel.push(TrajKind::Conn(text), cx));
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

    fn on_send(&mut self, text: String, cx: &mut Context<Self>) {
        self.chat.update(cx, |panel, cx| panel.set_busy(true, cx));
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
        let session_id = format!(
            "zhibi-{}",
            self.root
                .file_stem()
                .map(|s| s.to_string_lossy().to_string())
                .unwrap_or_else(|| "main".into())
        );
        let query_text = text;
        cx.spawn(async move |_, cx| {
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
            let r = client.session_query(&session_id, &query_text).await;
            let _ = weak.update(cx, |ws, cx| {
                ws.trajectory.update(cx, |panel, cx| match r {
                    Ok(_) => panel.push(TrajKind::Info("已入队，等待 DSH 会话事件…".into()), cx),
                    Err(e) => panel.push(TrajKind::Fail(format!("发送失败：{e}")), cx),
                });
                ws.inbox.update(cx, |panel, cx| panel.refresh(cx));
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
                self.dsh_running = running;
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
                let text = assistant_text_full(&data);
                if text.is_empty() {
                    None
                } else if text.contains('【') && (text.contains("A】") || text.contains("B】")) {
                    // 分支卡：解析【A】【B】【C】段落
                    let branches: Vec<String> = text
                        .split('【')
                        .filter(|seg| !seg.trim().is_empty())
                        .map(|seg| format!("【{seg}").trim_end().to_string())
                        .collect();
                    self.branches = branches.clone();
                    let n = branches.len();
                    self.editor.update(cx, |panel, cx| panel.set_branches(branches, cx));
                    self.chat.update(cx, |panel, cx| {
                        panel.set_busy(false, cx);
                        panel.push(ChatMsg { role: "assistant".into(), text: "给了几个走向——在稿纸下方选一个，或换一批。".into() }, cx);
                    });
                    self.show_toast("走向已就绪", cx);
                    Some(TrajKind::Info(format!("{n} 个走向已就绪，稿纸下方可选")))
                } else if let Some(original) = self.pending_sel.take() {
                    self.chat.update(cx, |panel, cx| {
                        panel.set_busy(false, cx);
                        panel.push(ChatMsg { role: "assistant".into(), text: text.clone() }, cx);
                    });
                    Some(TrajKind::SelectionDraft { original, text })
                } else {
                    let t = text.clone();
                    self.chat.update(cx, |panel, cx| {
                        panel.set_busy(false, cx);
                        panel.push(ChatMsg { role: "assistant".into(), text: t }, cx);
                    });
                    Some(TrajKind::Text(text))
                }
            }
            "turn/end" => {
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
            self.trajectory
                .update(cx, |panel, cx| panel.push(entry, cx));
        }
    }
}

/// 进程级握手：记录 cwd，选择 provider/model 路由。
async fn initialize_with(
    client: &DshClient,
    weak: &gpui_kit::WeakEntity<Workspace>,
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
                TrajKind::Info(format!("运行时 {name} · provider={provider} model={model}")),
                cx,
            );
        });
    })?;
    Ok(())
}

fn assistant_text_full(data: &Value) -> String {
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
        let reason = err.get("reason").and_then(|r| r.as_str()).unwrap_or("失败");
        return reason.chars().take(160).collect();
    }
    let head: String = content.chars().take(160).collect();
    if head.is_empty() {
        "（空结果）".into()
    } else {
        head
    }
}

impl Render for Workspace {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let dock = self.dock_area.clone();

        let current = self.chapters.iter().find(|c| c.n == self.chapter);
        let words = self.word_count(cx);
        let ch_label = SharedString::from(format!("{:02}", self.chapter));
        let book = SharedString::from(self.handle.title.clone());
        let head_tail: SharedString = if self.mode == Mode::Editor {
            current
                .map(|c| SharedString::from(c.title.clone()))
                .unwrap_or_else(|| SharedString::from(format!("第{}章", self.chapter)))
        } else {
            SharedString::from(self.mode.label())
        };
        let dsh_live = self.client.is_some();
        let dsh_running = self.dsh_running;
        let focus_on = self.focus;

        // 标题栏按钮：全部走 iconbtn/prominent 语言（库 ghost 无底色反馈，属缺陷）
        let back = Button::new("back-shelf")
            .custom(icon_btn(&*cx))
            .small()
            .rounded(theme::RADIUS_BTN_SM)
            .icon(IconName::ArrowLeft)
            .tooltip("返回书架（先落盘）")
            .on_click(cx.listener(|this, _, _window, cx| {
                this.save_current(cx);
                cx.emit(WorkspaceEvent::BackToShelf);
            }));
        let focus_btn = Button::new("focus-toggle")
            .custom(if focus_on {
                prominent_btn(&*cx)
            } else {
                icon_btn(&*cx)
            })
            .small()
            .rounded(theme::RADIUS_BTN_SM)
            .icon(if focus_on {
                IconName::Minimize
            } else {
                IconName::Maximize
            })
            .tooltip("专注模式")
            .on_click(cx.listener(|this, _, _window, cx| {
                this.focus = !this.focus;
                cx.notify();
            }));
        let toggle_right = Button::new("toggle-right-dock")
            .custom(icon_btn(&*cx))
            .small()
            .rounded(theme::RADIUS_BTN_SM)
            .icon(IconName::PanelRight)
            .tooltip("轨迹 / 提案 Dock")
            .on_click(cx.listener(move |this, _, window, cx| {
                this.dock_area.update(cx, |area, cx| {
                    area.toggle_dock(DockPlacement::Right, window, cx);
                });
            }));

        // 专注模式进出：右 Dock 收起/恢复（0.9.0 只有 toggle_dock，用标志位对齐）
        if self.focus && !self.dock_hidden {
            self.dock_area.update(cx, |area, cx| {
                area.toggle_dock(DockPlacement::Right, window, cx);
            });
            self.dock_hidden = true;
        }
        if !self.focus && self.dock_hidden {
            self.dock_area.update(cx, |area, cx| {
                area.toggle_dock(DockPlacement::Right, window, cx);
            });
            self.dock_hidden = false;
        }

        // kit 通知：手搓 overlay 退役，反馈统一走 push_notification
        if let Some(t) = self.toast.take() {
            window.push_notification(t, cx);
        }

        div()
            .id("zhibi-workspace-root")
            .key_context("zhibi-workspace")
            .on_action(cx.listener(|this, _: &SaveProse, _, cx| {
                this.save_current(cx);
                this.trajectory.update(cx, |panel, cx| {
                    panel.push(TrajKind::Info("已保存（⌘S）".into()), cx);
                });
            }))
            .on_action(cx.listener(|this, _: &ToggleFocusMode, _, cx| {
                this.focus = !this.focus;
                cx.notify();
            }))
            .on_action(cx.listener(|this, _: &BranchesNow, _, cx| {
                let task = prompts::task_for("branches", this.chapter);
                this.on_send(task, cx);
            }))
            .track_focus(&self.focus_handle(cx))
            .flex()
            .flex_col()
            .size_full()
            .font_family(theme::FAMILY_UI)
            .text_size(theme::FONT_BODY)
            .line_height(relative(theme::LINE_HEIGHT_BODY))
            .bg(theme::BG())
            .text_color(theme::TEXT())
            .child(
                TitleBar::new()
                    .bg(theme::PANEL())
                    .border_color(theme::LINE())
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap(theme::GAP_MD)
                            .flex_shrink_0()
                            .child(back)
                            .child(focus_btn),
                    )
                    .child(
                        div()
                            .flex()
                            .flex_1()
                            .min_w_0()
                            .items_center()
                            .gap(theme::GAP_LG)
                            .pl(theme::PAD_MD)
                            .pr(theme::PAD_XL)
                            .child(
                                div()
                                    .flex()
                                    .items_baseline()
                                    .min_w_0()
                                    .overflow_hidden()
                                    .child(
                                        div()
                                            .text_size(theme::FONT_LABEL)
                                            .text_color(theme::DIM())
                                            .child("执笔 3.0"),
                                    )
                                    .child(
                                        div()
                                            .ml(theme::PAD_SM)
                                            .min_w_0()
                                            .text_ellipsis()
                                            .font_weight(FontWeight::MEDIUM)
                                            .text_color(theme::ACCENT())
                                            .child(book),
                                    )
                                    .child(
                                        div()
                                            .ml(theme::PAD_SM)
                                            .min_w_0()
                                            .text_ellipsis()
                                            .text_color(theme::TEXT())
                                            .child(head_tail),
                                    ),
                            )
                            .child(
                                div()
                                    .ml_auto()
                                    .flex()
                                    .items_center()
                                    .gap(theme::GAP_MD)
                                    .child(
                                        div()
                                            .font_family(theme::FAMILY_MONO)
                                            .text_size(theme::FONT_LABEL)
                                            .text_color(theme::DIM())
                                            .child(SharedString::from(format!("{words} 字"))),
                                    )
                                    .child(if dsh_running {
                                        live_chip("DSH 运行中")
                                    } else if dsh_live {
                                        chip(
                                            "DSH 已连接",
                                            theme::GREEN(),
                                            theme::GREEN_SOFT(),
                                            theme::GREEN_BORDER(),
                                        )
                                    } else {
                                        chip(
                                            "DSH 未启动",
                                            theme::FAINT(),
                                            theme::CHIP_NEUTRAL(),
                                            theme::LINE(),
                                        )
                                    }),
                            ),
                    ),
            )
            .child(
                div()
                    .flex()
                    .flex_1()
                    .min_h_0()
                    .children(if self.focus {
                        Vec::new()
                    } else {
                        vec![self.sidebar(cx)]
                    })
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .min_h_0()
                            .bg(theme::BG())
                            .child(dock),
                    ),
            )
            .child(
                gpui_kit::div().hidden(),
            )
            .child(
                StatusBar::new()
                    .bg(theme::PANEL())
                    .border_color(theme::LINE())
                    .px(theme::PAD_XL)
                    .text_color(theme::DIM())
                    .left(
                        Button::new("save-status")
                            .custom(icon_btn(&*cx))
                            .small()
                            .rounded(theme::RADIUS_BTN_SM)
                            .label("保存")
                            .on_click(cx.listener(|this, _, _window, cx| {
                                this.save_current(cx);
                                this.trajectory.update(cx, |panel, cx| {
                                    panel.push(TrajKind::Info("正文已保存（先落盘）".into()), cx);
                                });
                            })),
                    )
                    .left(
                        Button::new("export-status")
                            .custom(icon_btn(&*cx))
                            .small()
                            .rounded(theme::RADIUS_BTN_SM)
                            .label("导出合并稿")
                            .on_click(cx.listener(|this, _, _window, cx| {
                                match this.handle.export_merged() {
                                    Ok(path) => {
                                        let p = path.display().to_string();
                                        this.trajectory.update(cx, |panel, cx| {
                                            panel.push(
                                                TrajKind::Info(format!("合并稿已导出：{p}")),
                                                cx,
                                            );
                                        });
                                    }
                                    Err(e) => {
                                        this.trajectory.update(cx, |panel, cx| {
                                            panel
                                                .push(TrajKind::Fail(format!("导出失败：{e}")), cx);
                                        });
                                    }
                                }
                            })),
                    )
                    .left(
                        div()
                            .flex()
                            .items_center()
                            .gap(theme::GAP_SM)
                            .child(
                                div()
                                    .font_family(theme::FAMILY_MONO)
                                    .text_size(theme::FONT_LABEL)
                                    .text_color(theme::FAINT())
                                    .child(ch_label),
                            )
                            .child(
                                div()
                                    .text_size(theme::FONT_LABEL)
                                    .text_color(theme::DIM())
                                    .child("章"),
                            )
                            .child(if self.last_saved.is_empty() {
                                chip(
                                    "未保存",
                                    theme::FAINT(),
                                    theme::CHIP_NEUTRAL(),
                                    theme::LINE(),
                                )
                            } else {
                                // 只说「已保存」，不拼时间：stamp 由 UNIX 秒推出（UTC），
                                // 直接展示会比本地钟慢/快数小时，宁可只给状态。
                                chip(
                                    "已保存",
                                    theme::GREEN(),
                                    theme::GREEN_SOFT(),
                                    theme::GREEN_BORDER(),
                                )
                            }),
                    )
                    .right(toggle_right),
            )
    }
}

impl Workspace {
    /// 导航行的标题：稿纸页叫「稿纸」（原型 workspace.html:188 起的 .ym-nav 文案），
    /// 其余沿用页面名（`Mode::label`）。
    fn nav_label(mode: Mode) -> &'static str {
        if mode == Mode::Editor {
            "稿纸"
        } else {
            mode.label()
        }
    }

    /// 侧栏导航行：选中 = 朱砂软底 + 朱砂描边 + 正文字 + 图标转朱砂（规则 5/7）。
    fn nav_row(&self, ix: usize, mode: Mode, cx: &mut Context<Self>) -> AnyElement {
        let active = self.mode == mode;
        let label = Self::nav_label(mode);
        div()
            .id(SharedString::from(format!("nav-{ix}")))
            .flex()
            .items_center()
            .gap(theme::GAP_MD)
            .px(theme::PAD_SM)
            .py(theme::PAD_XS)
            .rounded(theme::RADIUS_BTN_SM)
            .cursor_pointer()
            .border_1()
            .border_color(if active {
                theme::ACCENT_BORDER()
            } else {
                theme::PANEL()
            })
            .bg(if active {
                theme::ACCENT_SOFT()
            } else {
                theme::PANEL()
            })
            .text_size(theme::FONT_SMALL)
            .text_color(if active { theme::TEXT() } else { theme::DIM() })
            .hover(|s| s.bg(theme::PANEL2()).border_color(theme::ACCENT_BORDER()))
            .child(
                Icon::new(mode.icon())
                    .with_size(theme::FONT_TAG)
                    .text_color(hsla(if active {
                        theme::ACCENT()
                    } else {
                        theme::FAINT()
                    })),
            )
            .child(div().flex_1().min_w_0().text_ellipsis().child(label))
            .on_click(cx.listener(move |this, _, window, cx| {
                this.switch_center(mode, window, cx);
            }))
            .into_any_element()
    }

    /// 章节行：行内编号等宽码（选中转朱砂），未动笔用幽灵色（规则 8/10）。
    fn chapter_row(
        &self,
        ix: usize,
        info: &store::ChapterInfo,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let n = info.n;
        let active = n == self.chapter;
        let meta = if info.has_prose {
            SharedString::from(format!("{}字", info.words))
        } else {
            SharedString::from("未动笔")
        };
        div()
            .id(SharedString::from(format!("chapter-{ix}")))
            .flex()
            .items_center()
            .gap(theme::GAP_MD)
            .px(theme::PAD_SM)
            .py(theme::PAD_XS)
            .rounded(theme::RADIUS_BTN_SM)
            .cursor_pointer()
            .border_1()
            .border_color(if active {
                theme::ACCENT_BORDER()
            } else {
                theme::PANEL()
            })
            .bg(if active {
                theme::ACCENT_SOFT()
            } else {
                theme::PANEL()
            })
            .text_size(theme::FONT_SMALL)
            .text_color(if active { theme::TEXT() } else { theme::DIM() })
            .hover(|s| s.bg(theme::PANEL2()).border_color(theme::ACCENT_BORDER()))
            .child(
                div()
                    .flex_shrink_0()
                    .font_family(theme::FAMILY_MONO)
                    .text_size(theme::FONT_LABEL)
                    .text_color(if active {
                        theme::ACCENT()
                    } else {
                        theme::FAINT()
                    })
                    .child(SharedString::from(format!("{n:02}"))),
            )
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .text_ellipsis()
                    .child(SharedString::from(info.title.clone())),
            )
            .child(
                div()
                    .flex_shrink_0()
                    .text_size(theme::FONT_TAG)
                    .text_color(if info.has_prose {
                        theme::FAINT()
                    } else {
                        theme::GHOST()
                    })
                    .child(meta),
            )
            .on_click(cx.listener(move |this, _, window, cx| {
                this.switch_chapter(n, window, cx);
            }))
            .into_any_element()
    }

    /// 侧栏分组小标：11px 弱字 + 字距，组间用 1px 分隔线（规则 12）。
    fn section_label(label: &'static str) -> AnyElement {
        div()
            .flex()
            .items_center()
            .gap(theme::GAP_MD)
            .px(theme::PAD_SM)
            .pt(theme::GAP_GROUP_SM)
            .pb(theme::PAD_XS)
            .text_size(theme::FONT_LABEL)
            .text_color(theme::FAINT())
            .child(label)
            .into_any_element()
    }

    fn sidebar(&self, cx: &mut Context<Self>) -> AnyElement {
        let mut rows: Vec<AnyElement> = Vec::new();
        rows.push(Self::section_label("工作台"));
        for (ix, mode) in NAV.iter().copied().enumerate() {
            rows.push(self.nav_row(ix, mode, cx));
        }
        let count = SharedString::from(format!("{:02}", self.chapters.len()));
        rows.push(
            div()
                .flex()
                .items_center()
                .gap(theme::GAP_MD)
                .mt(theme::GAP_GROUP)
                .pt(theme::PAD_SM)
                .border_t_1()
                .border_color(theme::LINE())
                .child(
                    div()
                        .flex_1()
                        .text_size(theme::FONT_LABEL)
                        .text_color(theme::FAINT())
                        .child("章节"),
                )
                .child(
                    div()
                        .font_family(theme::FAMILY_MONO)
                        .text_size(theme::FONT_TAG)
                        .text_color(theme::FAINT())
                        .child(count),
                )
                .into_any_element(),
        );
        if self.chapters.is_empty() {
            rows.push(
                div()
                    .px(theme::PAD_SM)
                    .py(theme::PAD_MD)
                    .text_size(theme::FONT_SMALL)
                    .text_color(theme::DIM())
                    .child("还没有章节——下方「新建章」即可落笔。")
                    .into_any_element(),
            );
        } else {
            for (ix, info) in self.chapters.iter().enumerate() {
                rows.push(self.chapter_row(ix, info, cx));
            }
        }

        div()
            .flex()
            .flex_col()
            .flex_shrink_0()
            .w(theme::SIDEBAR_W)
            .h_full()
            .border_r_1()
            .border_color(theme::LINE())
            .bg(theme::PANEL())
            .text_color(theme::TEXT())
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(theme::GAP_MD)
                    .px(theme::PAD_MD)
                    .py(theme::PAD_MD)
                    .border_b_1()
                    .border_color(theme::LINE())
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .text_ellipsis()
                            .font_family(theme::FAMILY_SERIF)
                            .text_size(theme::FONT_TITLE_SM)
                            .font_weight(FontWeight::MEDIUM)
                            .child(SharedString::from(self.handle.title.clone())),
                    ),
            )
            .child(
                div()
                    .id("sidebar-list")
                    .flex_1()
                    .min_h_0()
                    .flex()
                    .flex_col()
                    .gap(theme::GAP_XS)
                    .overflow_y_scroll()
                    .px(theme::PAD_MD)
                    .pb(theme::PAD_MD)
                    .children(rows),
            )
            .child(
                div()
                    .px(theme::PAD_MD)
                    .py(theme::PAD_MD)
                    .border_t_1()
                    .border_color(theme::LINE())
                    .child(
                        Button::new("new-chapter")
                            .custom(prominent_btn(&*cx))
                            .outline()
                            .small()
                            .rounded(theme::RADIUS_BTN)
                            .w_full()
                            .icon(IconName::Plus)
                            .label("新建章")
                            .tooltip("新建一章并进入稿纸")
                            .on_click(cx.listener(|this, _, window, cx| {
                                let n = this.handle.next_chapter_number();
                                let _ = this.handle.ensure_chapter(n);
                                this.switch_center(Mode::Editor, window, cx);
                                this.switch_chapter(n, window, cx);
                            })),
                    ),
            )
            .into_any_element()
    }
}
