//! 工作区：标题栏 + [侧栏(真章节树) | DockArea(编辑器 + 右侧轨迹)] + 状态条。
//! 职责：项目句柄、章节切换与正文保存、DSH 生命周期与事件分发。

use anyhow::Result;
use gpui_kit::component::button::{Button, ButtonCustomVariant, ButtonVariants as _};
use gpui_kit::component::dock::{
    DockArea, DockEvent, DockLayout, DockPlacement, DockSkin, panel_handle,
};
use gpui_kit::component::input::{Input, InputState, TextareaState};
use gpui_kit::component::spinner::Spinner;
use gpui_kit::component::status_bar::StatusBar;
use gpui_kit::assets::IconName;
use gpui_kit::component::{Icon, Sizable, TitleBar, WindowExt as _};
use gpui_kit::prelude::FluentBuilder;
use gpui_kit::{
    AnyElement, App, AppContext as _, AsyncApp, Context, Entity, EventEmitter, FontWeight,
    InteractiveElement, IntoElement, ParentElement, Render, SharedString,
    StatefulInteractiveElement, Styled, Window, div, px, relative,
};
use serde_json::Value;
use std::rc::Rc;

use crate::chat::{ChatEvent, ChatMsg, ChatPanel};
use crate::bookinfo::{BookInfoEvent, BookInfoPanel};
use crate::canvas::{CanvasEvent, CanvasPanel, OracleEvent, OraclePanel};
use crate::director::{DirectorEvent, DirectorPanel};
use crate::dsh::{self, AgentMode, DshClient, DshEvent, model_route};
use crate::editor::{EditorEvent, EditorPanel};
use crate::inbox::{InboxEvent, InboxPanel};
use crate::prompts;
use crate::store::{self, ProjectHandle};
use crate::theme;
use crate::trajectory::{TrajEvent, TrajKind, TrajectoryPanel};
use crate::views::{
    CanonPanel, CharactersPanel, CluesPanel, GraphPanel, ImportEvent, ImportPanel,
    CharacterEvent, JournalEvent, JournalPanel, MemoryPanel,
    OutlinePanel, SettingsEvent, SettingsPanel, SnapEvent, SnapshotsPanel, StatsPanel, ViewEvent,
};

gpui_kit::actions!(zhibi, [SaveProse, ToggleFocusMode, BranchesNow]);

const DOCK_AREA_ID: &str = "zhibi-main-dock";
const DOCK_AREA_VERSION: usize = 1;

#[derive(Debug, Clone)]
pub enum WorkspaceEvent {
    BackToShelf,
}

/// 一次待作者裁决的 DSH 审批（来自 `approval/request`）。
///
/// 典型来源：创造模式用 `plugin_manager` 装本书专属 bundle（需 danger-full-access）、
/// 或 AI 在只读档尝试提权。DSH 对无应答一律 fail-closed，所以这些必须由 GUI 回。
#[derive(Debug, Clone)]
#[allow(dead_code)] // session_id/tool 用于后续“按 Agent 归属”显示
pub struct ApprovalRequest {
    pub id: serde_json::Value,
    pub session_id: String,
    pub tool: String,
    pub reason: String,
}

/// 一次待作者回答的 `ask_user_question`（来自 `user-question/ask`）。
#[derive(Debug, Clone)]
#[allow(dead_code)] // session_id/request_id 用于会话级取消与回放
pub struct QuestionRequest {
    pub id: serde_json::Value,
    pub session_id: String,
    pub request_id: String,
    pub questions: serde_json::Value,
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
    Journal,
    Import,
    Graph,
    Canvas,
    Director,
    Worldline,
    Oracle,
    BookInfo,
}

impl Mode {
    /// 开发预览用：ZHIBI_MODE 的键 → 模式（也接受中文 label）
    pub(crate) fn from_key(k: &str) -> Option<Mode> {
        let k = k.trim().to_ascii_lowercase();
        Some(match k.as_str() {
            "editor" | "章节" => Mode::Editor,
            "outline" | "大纲" => Mode::Outline,
            "canon" | "设定" => Mode::Canon,
            "memory" | "记忆" => Mode::Memory,
            "stats" | "统计" => Mode::Stats,
            "clues" | "伏笔" => Mode::Clues,
            "characters" | "人物" => Mode::Characters,
            "settings" | "设置" => Mode::Settings,
            "snapshots" | "快照" => Mode::Snapshots,
            "journal" | "改动记录" => Mode::Journal,
            "import" | "导入" => Mode::Import,
            "graph" | "图谱" => Mode::Graph,
            "canvas" | "画板" => Mode::Canvas,
            "director" | "世界线" => Mode::Director,
            "oracle" | "预言书" | "预言" => Mode::Oracle,
            "bookinfo" | "书目" => Mode::BookInfo,
            _ => return None,
        })
    }

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
            Mode::Journal => "改动记录",
            Mode::Import => "导入旧稿",
            Mode::Graph => "记忆图谱",
            Mode::Canvas => "剧情画板",
            Mode::Director => "章节排期",
            Mode::Worldline => "世界线",
            Mode::Oracle => "预言书",
            Mode::BookInfo => "书目信息",
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
            // 没有历史/撤销图标时用 Camera 的语义邻居：这里借用 ListTree 表示“流水”
            Mode::Journal => IconName::ListTree,
            Mode::Import => IconName::FolderInput,
            Mode::Graph => IconName::Network,
            Mode::Canvas => IconName::Waypoints,
            Mode::Director => IconName::Kanban,
            Mode::Worldline => IconName::GitBranch,
            Mode::Oracle => IconName::Sparkle,
            Mode::BookInfo => IconName::Notebook,
        }
    }
}

/// 侧栏导航顺序（与原实现一致，只是改为数据表以便统一出行）。
const NAV: [Mode; 16] = [
    Mode::Editor,
    Mode::Outline,
    Mode::Canon,
    Mode::Memory,
    Mode::Stats,
    Mode::Clues,
    Mode::Characters,
    Mode::Canvas,
    Mode::Director,
    Mode::Worldline,
    Mode::Oracle,
    Mode::BookInfo,
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

/// 侧栏「作品信息」的紧凑字段行（题材/故事核/目标章数/单章字数）。
fn meta_field(label: &'static str, state: &Entity<InputState>) -> AnyElement {
    div()
        .flex()
        .items_center()
        .gap(theme::GAP_SM)
        .w_full()
        .child(
            div()
                .w(px(56.))
                .flex_shrink_0()
                .text_size(theme::FONT_TAG)
                .text_color(theme::DIM())
                .child(label),
        )
        .child(
            div().flex_1().min_w_0().child(
                Input::new(state)
                    .bg(theme::PANEL2())
                    .border_1()
                    .border_color(theme::LINE())
                    .rounded(theme::RADIUS_BTN_SM)
                    .text_size(theme::FONT_SMALL)
                    .text_color(theme::TEXT()),
            ),
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
    journal_view: Entity<JournalPanel>,
    import_view: Entity<ImportPanel>,
    graph_view: Entity<GraphPanel>,
    canvas_view: Entity<CanvasPanel>,
    director_view: Entity<DirectorPanel>,
    oracle_view: Entity<OraclePanel>,
    worldline_view: Entity<crate::worldline::view::WorldlinePanel>,
    bookinfo_view: Entity<BookInfoPanel>,
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
    /// 最近一次与 DSH 交互的时刻。空闲回收用它判断"这个进程放着多久没用了"。
    dsh_last_active: std::time::Instant,
    focus: bool,
    dock_hidden: bool,
    workspace_focus: gpui_kit::FocusHandle,
    /// 选区创作时的原文。采纳改写前用它校验“原文还在”，防止把建议套到别处。
    selected_original: Option<String>,
    /// 主 Agent 形态（托管 / 极简 / 创造）。切模式 = 换 patch 重启本书的 DSH 进程。
    agent_mode: dsh::AgentMode,
    /// 写作档位（辅助/共写/代笔）+ 代笔授权章节。写进书目录，novel-pack 立即读到。
    book_auth: crate::journal::BookAuth,
    /// 待应答的审批（GUI 渲染成卡片，必须回；不回则 DSH 按拒绝处理）
    pending_approval: Option<ApprovalRequest>,
    /// 待应答的提问（`ask_user_question`）
    pending_question: Option<QuestionRequest>,
    /// 本轮是否已有任务在跑（拦连点，避免并发污染同一会话）
    turn_active: bool,
    /// 本书的 DSH 会话 id。跨进程重启保持同一个 id，切模式后用 session/resume 接回历史。
    session_id: String,
    /// 新进程已经接过一次会话（避免每次发送都 resume）
    session_attached: bool,
    /// 作品信息（project.json 的 genre/premise/targetChapters/chapterWordTarget）编辑入口
    meta_genre: Entity<InputState>,
    meta_premise: Entity<InputState>,
    meta_target: Entity<InputState>,
    meta_word_target: Entity<InputState>,
    meta_open: bool,
}

impl Drop for Workspace {
    /// 关掉书 = 杀掉这本书的 DSH 进程。
    ///
    /// 不做这件事的后果是肉眼看不见的：界面回到书架，`ps` 里还留着一个 Node
    /// （一本 100-200MB），继续开着直到整个 App 退出。`Child` 的 Drop 不杀进程，
    /// 读线程还持着 `Arc<Inner>`，所以光靠丢智能指针也回收不掉——
    /// 必须显式 kill，让 stdout 关闭、读线程退出，`Inner` 才跟着释放。
    fn drop(&mut self) {
        if let Some(client) = self.client.take() {
            client.shutdown();
        }
    }
}

impl Workspace {
    pub fn new(root: std::path::PathBuf, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let handle = Rc::new(ProjectHandle::open(&root).expect("项目打开失败"));
        // 会话 id 按书目录名定，跨进程重启保持不变（切模式后靠它 resume）
        let session_id = format!(
            "zhibi-{}",
            root.file_stem()
                .map(|s| s.to_string_lossy().to_string())
                .unwrap_or_else(|| "main".into())
        );
        let book_auth = crate::journal::BookAuth::load(&root);
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
            |this, _, ev: &InboxEvent, window, cx| match ev {
                InboxEvent::Accept(id) => this.accept_proposal(id, window, cx),
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
        let characters_view = cx.new(|cx| CharactersPanel::new(window, cx, handle.clone()));
        let settings_view = cx.new(|cx| SettingsPanel::new(window, cx));
        // 这本书可能已经有专属 Agent（上次创造模式的产物）：开局回填，作者一进设置就看到
        settings_view.update(cx, |panel, cx| {
            let preset = crate::dsh::BookPreset::load(&crate::dsh::home_dir(&root));
            panel.load_book_preset(preset, window, cx);
        });
        let snapshots_view = cx.new(|cx| SnapshotsPanel::new(cx, handle.clone(), chapter));
        let journal_view = cx.new(|cx| JournalPanel::new(window, cx, handle.clone()));
        let import_view = cx.new(|cx| ImportPanel::new(window, cx, handle.clone()));
        let graph_view = cx.new(|cx| GraphPanel::new(cx, handle.clone()));
        let canvas_view = cx.new(|cx| CanvasPanel::new(window, cx, handle.clone()));
        let director_view = cx.new(|cx| DirectorPanel::new(cx, handle.clone(), chapter));
        let oracle_view = cx.new(|cx| OraclePanel::new(window, cx, handle.clone()));
        let worldline_view =
            cx.new(|cx| crate::worldline::view::WorldlinePanel::new(window, cx, handle.clone()));
        let bookinfo_view = cx.new(|cx| BookInfoPanel::new(window, cx, handle.clone()));
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
                SettingsEvent::Persist(cfg) => {
                    // 整份设置来自表单（含模型、推理强度、写作档位、开关），
                    // 直接写盘；模型相关项下次启动运行时生效。
                    let msg = match crate::store::save_settings(cfg) {
                        Ok(()) => "设置已保存（模型相关项下次启动运行时生效）".to_string(),
                        Err(e) => format!("保存失败：{e}"),
                    };
                    this.trajectory.update(cx, |panel, cx| {
                        panel.push(TrajKind::Info(msg), cx);
                    });
                }
                SettingsEvent::SaveBookPreset { name, persona, takeover } => {
                    // 空 persona = 作者清空了输入框 = 删除（"删除即恢复默认"）
                    let home = crate::dsh::home_dir(&this.root);
                    let msg = if persona.trim().is_empty() {
                        match crate::dsh::BookPreset::remove(&home) {
                            Ok(()) => "已删除本书专属 Agent，回到默认托管/极简".to_string(),
                            Err(e) => format!("删除失败：{e}"),
                        }
                    } else {
                        let preset = crate::dsh::BookPreset {
                            name: name.clone(),
                            persona: persona.clone(),
                            deny_extra: Vec::new(),
                            takeover: *takeover,
                        };
                        match preset.save(&home) {
                            Ok(()) => {
                                // 主 Agent 形态变了 → 必须重启运行时，否则 patch 不生效
                                this.restart_runtime(cx);
                                format!(
                                    "已保存「{}」{}；运行时已重启",
                                    preset.name,
                                    if *takeover { "（已接管默认派活）" } else { "" }
                                )
                            }
                            Err(e) => format!("保存失败：{e}"),
                        }
                    };
                    this.trajectory.update(cx, |panel, cx| {
                        panel.push(TrajKind::Info(msg), cx);
                    });
                }
                SettingsEvent::RefreshAccount => {
                    // 余额查询走网络，不能卡住 UI：后台线程算完再回填
                    let home = crate::dsh::home_dir(&this.root);
                    let weak = this.settings_view.clone();
                    cx.spawn(async move |_, cx| {
                        let snap = cx
                            .background_executor()
                            .spawn(async move { crate::account::fetch_balance(&home) })
                            .await;
                        let _ = weak.update(cx, |panel, cx| panel.set_account(snap, cx));
                    })
                    .detach();
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
                ViewEvent::Delegate { agent, key, target } => {
                    this.delegate(agent, key, target.clone(), cx);
                }
                // 设定页暂不做台账增删（设定是 Markdown 文件，不是 JSON 台账），
                // 但事件类型是共享的，这里显式忽略而不是漏处理。
                ViewEvent::LedgerAdd(..) | ViewEvent::LedgerUpdate(..) | ViewEvent::LedgerRemove(..) => {}
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
                ViewEvent::Delegate { agent, key, target } => {
                    this.delegate(agent, key, target.clone(), cx);
                }
                // 作者亲笔的台账增删改统一走 on_ledger_event（store 的 CRUD 自带日志与撤销）。
                ViewEvent::LedgerAdd(..) | ViewEvent::LedgerUpdate(..) | ViewEvent::LedgerRemove(..) => {
                    this.on_ledger_event(ev, cx);
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
                ViewEvent::Delegate { agent, key, target } => {
                    this.delegate(agent, key, target.clone(), cx);
                }
                // 作者亲笔的台账增删改统一走 on_ledger_event（store 的 CRUD 自带日志与撤销）。
                ViewEvent::LedgerAdd(..) | ViewEvent::LedgerUpdate(..) | ViewEvent::LedgerRemove(..) => {
                    this.on_ledger_event(ev, cx);
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
                ViewEvent::Delegate { agent, key, target } => {
                    this.delegate(agent, key, target.clone(), cx);
                }
                // 作者亲笔的台账增删改统一走 on_ledger_event（store 的 CRUD 自带日志与撤销）。
                ViewEvent::LedgerAdd(..) | ViewEvent::LedgerUpdate(..) | ViewEvent::LedgerRemove(..) => {
                    this.on_ledger_event(ev, cx);
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

        // 世界线页：时间轴上的手工操作（分叉 / 扶正 / 归档 / 删除 / 跳推演）
        cx.subscribe_in(
            &worldline_view,
            window,
            |this, _, ev: &crate::worldline::view::WorldlineEvent, window, cx| {
                use crate::worldline::view::WorldlineEvent as WE;
                let store = crate::worldline::Store::new(&this.handle.root);
                match ev {
                    WE::Select(id) => {
                        // 选中联动预言书：在两页看到同一条线
                        let id = id.clone();
                        this.oracle_view.update(cx, |panel, cx| {
                            panel.select_line(id, cx);
                        });
                    }
                    WE::Fork { from, chapter } => {
                        let next = next_line_id(&store);
                        let ch = if *chapter == 0 {
                            store
                                .find(from)
                                .ok()
                                .flatten()
                                .map(|l| l.fixed_boundary().max(l.fork_chapter()))
                                .unwrap_or(0)
                        } else {
                            *chapter
                        };
                        let name = format!("{next}·从第{ch}章分叉");
                        match store.fork_at_chapter(
                            from,
                            ch,
                            &next,
                            &name,
                            "",
                            crate::worldline::Kind::WhatIf,
                        ) {
                            Ok(l) => {
                                let msg = format!("已开线 {}（自第{ch}章）", l.id);
                                let lid = l.id.clone();
                                this.worldline_view.update(cx, |p, cx| {
                                    p.select(lid, cx);
                                    p.set_message(msg, cx);
                                });
                            }
                            Err(e) => {
                                this.worldline_view.update(cx, |p, cx| {
                                    p.set_message(format!("分叉失败：{e}"), cx)
                                });
                            }
                        }
                    }
                    WE::Fix(id) => match store.fix_line(id) {
                        Ok(l) => {
                            let msg = format!(
                                "已扶正：{} 成为正典后续（固化边界推到第 {} 章）",
                                l.name,
                                l.fixed_boundary()
                            );
                            this.worldline_view.update(cx, |p, cx| p.set_message(msg, cx));
                            this.oracle_view.update(cx, |_, cx| cx.notify());
                            let _ = window;
                        }
                        Err(e) => this
                            .worldline_view
                            .update(cx, |p, cx| p.set_message(format!("扶正失败：{e}"), cx)),
                    },
                    WE::Unfix(id) => match store.unfix_line(id) {
                        Ok(()) => this
                            .worldline_view
                            .update(cx, |p, cx| p.set_message(format!("已撤销线{id}的扶正"), cx)),
                        Err(e) => this
                            .worldline_view
                            .update(cx, |p, cx| p.set_message(format!("撤销失败：{e}"), cx)),
                    },
                    WE::Archive(id) => match this.handle.toggle_worldline_archive(id) {
                        Ok(st) => this
                            .worldline_view
                            .update(cx, |p, cx| p.set_message(format!("线{id} → {st}"), cx)),
                        Err(e) => this
                            .worldline_view
                            .update(cx, |p, cx| p.set_message(format!("归档失败：{e}"), cx)),
                    },
                    WE::Remove(id) => match store.remove(id) {
                        Ok(()) => this.worldline_view.update(cx, |p, cx| {
                            p.set_message(format!("已删除线{id}（子线保留）"), cx)
                        }),
                        Err(e) => this
                            .worldline_view
                            .update(cx, |p, cx| p.set_message(format!("删除失败：{e}"), cx)),
                    },
                    WE::NodePatch { line, node, patch } => {
                        match store.update_node(line, node, patch.clone()) {
                            Ok(()) => this
                                .worldline_view
                                .update(cx, |p, cx| p.set_message(format!("已更新节点 {node}"), cx)),
                            Err(e) => this.worldline_view.update(cx, |p, cx| {
                                p.set_message(format!("更新失败：{e}"), cx)
                            }),
                        }
                    }
                    WE::NodeRemove { line, node } => match store.remove_node(line, node) {
                        Ok(()) => this.worldline_view.update(cx, |p, cx| {
                            p.set_message(format!("已删除节点 {node}（线{line}）"), cx)
                        }),
                        Err(e) => this
                            .worldline_view
                            .update(cx, |p, cx| p.set_message(format!("删除失败：{e}"), cx)),
                    },
                    WE::NodeAdopt { line, node } => {
                        match this.handle.adopt_node(line, node) {
                            Ok(pid) => {
                                this.worldline_view.update(cx, |p, cx| {
                                    p.set_message(
                                        format!("已逐节点采纳 → 提案 {pid}（收件箱待裁决）"),
                                        cx,
                                    )
                                });
                                this.inbox.update(cx, |panel, cx| panel.refresh(cx));
                            }
                            Err(e) => this
                                .worldline_view
                                .update(cx, |p, cx| p.set_message(format!("采纳失败：{e}"), cx)),
                        }
                    }
                    WE::NodeAdd { line, kind, label, chapter } => {
                        let nid = next_node_id(line, &this.handle);
                        let mut n = crate::worldline::Node::new(&nid, kind, label);
                        n.chapter = *chapter;
                        n.state = crate::worldline::NodeState::Planned;
                        match store.add_node(line, n) {
                            Ok(()) => this
                                .worldline_view
                                .update(cx, |p, cx| p.set_message("已新增节点", cx)),
                            Err(e) => this
                                .worldline_view
                                .update(cx, |p, cx| p.set_message(format!("新增失败：{e}"), cx)),
                        }
                    }
                    WE::Delegate { agent, key, target } => {
                        this.delegate(agent, key, target.clone(), cx);
                    }
                    WE::UpdateMeta { line, name, premise } => {
                        match store.update_line_meta(line, name.clone(), premise.clone()) {
                            Ok(()) => this
                                .worldline_view
                                .update(cx, |p, cx| p.set_message("已保存", cx)),
                            Err(e) => this
                                .worldline_view
                                .update(cx, |p, cx| p.set_message(format!("保存失败：{e}"), cx)),
                        }
                    }
                    WE::AddEdge { line, from, to, kind } => {
                        let e = crate::worldline::Edge {
                            from: from.clone(),
                            to: to.clone(),
                            kind: kind.clone(),
                            hidden: false,
                            ..Default::default()
                        };
                        match store.add_edge(line, e) {
                            Ok(()) => this
                                .worldline_view
                                .update(cx, |p, cx| p.set_message("已加边", cx)),
                            Err(err) => this
                                .worldline_view
                                .update(cx, |p, cx| p.set_message(format!("加边失败：{err}"), cx)),
                        }
                    }
                    WE::RemoveEdge { line, from, to, kind } => {
                        match store.remove_edge(line, from, to, kind) {
                            Ok(()) => this
                                .worldline_view
                                .update(cx, |p, cx| p.set_message("已删边", cx)),
                            Err(e) => this
                                .worldline_view
                                .update(cx, |p, cx| p.set_message(format!("删边失败：{e}"), cx)),
                        }
                    }
                    WE::ToggleEdgeHidden { line, from, to, kind } => {
                        // 先读当前值再翻转
                        let cur = store
                            .find(line)
                            .ok()
                            .flatten()
                            .and_then(|l| {
                                l.edges
                                    .iter()
                                    .find(|e| &e.from == from && &e.to == to && &e.kind == kind)
                                    .map(|e| e.hidden)
                            })
                            .unwrap_or(false);
                        match store.set_edge_hidden(line, from, to, kind, !cur) {
                            Ok(()) => this.worldline_view.update(cx, |p, cx| {
                                p.set_message(if cur { "已改为明线" } else { "已改为暗线" }, cx)
                            }),
                            Err(e) => this
                                .worldline_view
                                .update(cx, |p, cx| p.set_message(format!("切换失败：{e}"), cx)),
                        }
                    }
                    WE::LinkCanvas { line, node, canvas } => {
                        match store.link_canvas(line, node, canvas) {
                            Ok(()) => this.worldline_view.update(cx, |p, cx| {
                                p.set_message(format!("已把节点 {node} 关联到画布 {canvas}"), cx)
                            }),
                            Err(e) => this.worldline_view.update(cx, |p, cx| {
                                p.set_message(format!("关联失败：{e}"), cx)
                            }),
                        }
                    }
                }
            },
        )
        .detach();

        // 画布（关系维）的 Agent 入口：只转交，不自己拼指令。
        cx.subscribe_in(
            &canvas_view,
            window,
            |this, _, ev: &CanvasEvent, _window, cx| match ev {
                CanvasEvent::Delegate { agent, key, target } => {
                    this.delegate(agent, key, target.clone(), cx);
                }
            },
        )
        .detach();

        // 预言书：开线推演 → 走同一条 DSH 会话（轨迹窗全程可见）；晋升 → 刷新收件箱
        cx.subscribe_in(
            &oracle_view,
            window,
            |this, _, ev: &OracleEvent, _window, cx| match ev {
                OracleEvent::Simulate { premise, fork } => {
                    let fork = if *fork == 0 { this.chapter } else { *fork };
                    // 先由宿主把这条线的血缘落下（parentId / forkNode / 分叉章），
                    // 再交给 AI 往里填节点。顺序很重要：AI 侧 propose_worldline 只
                    // 知道"fork 第 N 章"，不知道作者是从哪条线的哪个节点按下去的；
                    // 血缘只在这里有，错过就只能靠猜。
                    if let Some(parent) = this.oracle_view.read(cx).active_lane_root() {
                        let store = crate::worldline::Store::new(&this.handle.root);
                        let next_id = next_line_id(&store);
                        let name: String = premise.chars().take(24).collect();
                        match store.fork_at_chapter(
                            &parent,
                            fork,
                            &next_id,
                            &name,
                            premise,
                            crate::worldline::Kind::WhatIf,
                        ) {
                            Ok(line) => {
                                this.canvas_view.update(cx, |panel, _cx| {
                                    panel.focus_worldline(line.id.clone())
                                });
                                this.trajectory.update(cx, |panel, cx| {
                                    panel.push(
                                        TrajKind::Info(format!(
                                            "已开线 {}（自分叉章 {fork} 起，父线 {parent}）",
                                            line.id
                                        )),
                                        cx,
                                    )
                                });
                            }
                            Err(e) => {
                                this.trajectory.update(cx, |panel, cx| {
                                    panel.push(TrajKind::Fail(format!("开线失败，仍交给 AI：{e}")), cx)
                                });
                            }
                        }
                    }
                    this.on_send(prompts::oracle_task(premise, fork), cx);
                }
                OracleEvent::Forecast => {
                    this.on_send(prompts::forecast_task(), cx);
                }
                OracleEvent::ViewOnCanvas { worldline } => {
                    let wl = worldline.clone();
                    this.canvas_view.update(cx, |panel, _cx| panel.focus_worldline(wl));
                    this.switch_center(Mode::Canvas, _window, cx);
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

// 书目信息保存成功：编辑器进度环分母立即生效
        cx.subscribe_in(
            &bookinfo_view,
            window,
            |this, _, ev: &BookInfoEvent, _window, cx| match ev {
                BookInfoEvent::Saved { word_target } => {
                    let wt = *word_target;
                    this.editor.update(cx, |panel, cx| {
                        panel.word_target = wt;
                        cx.notify();
                    });
                }
            },
        )
        .detach();

// 人物档案：所有写操作走这里——写盘 + 记日志，页面只发事件不碰磁盘
        cx.subscribe_in(
            &characters_view,
            window,
            |this, _, ev: &CharacterEvent, window, cx| {
                let roster = crate::characters::Roster::new(&this.handle.root);
                let journal = crate::journal::Journal::new(&this.handle.root);
                let snapshot = |list: &[crate::characters::Character]| -> String {
                    serde_json::to_string(list).unwrap_or_default()
                };
                // 每次写都先读旧内容，供日志记前像
                let before = snapshot(&roster.all());
                let record = |what: &str, after: &str| {
                    if before != after {
                        let _ = journal.record(
                            &this.handle.root,
                            "characters.json",
                            &before,
                            after,
                            &crate::journal::Actor::Author,
                            "edit",
                            &format!("b{}", crate::journal::now_millis()),
                            what,
                        );
                    }
                };
                // 派活不走「提示消息」这条路：它要发到 AI 会话里，不是页面反馈。
                if let CharacterEvent::Delegate { key, target } = ev {
                    this.delegate("character", key, target.clone(), cx);
                    return;
                }
                let msg = match ev {
                    // 上面已提前 return；这里显式列出，避免以后加分支时漏处理
                    CharacterEvent::Delegate { .. } => None,
                    CharacterEvent::Create(name) => {
                        let name = name.trim().to_string();
                        if name.is_empty() {
                            Some("名字不能为空".to_string())
                        } else if roster.find(&name).is_some() {
                            // 同名会让“找谁”变得不确定，先拒绝并要求明确
                            Some(format!("已有叫「{name}」的人物，请用别名或改名"))
                        } else {
                            let c = crate::characters::Character::new(&name);
                            match roster.upsert(c) {
                                Ok(()) => {
                                    record(&format!("新建人物 {name}"), &snapshot(&roster.all()));
                                    Some(format!("已新建人物「{name}」"))
                                }
                                Err(e) => Some(format!("新建失败：{e}")),
                            }
                        }
                    }
                    CharacterEvent::Save(c) => match roster.upsert((**c).clone()) {
                        Ok(()) => {
                            record(&format!("编辑人物 {}", c.name), &snapshot(&roster.all()));
                            Some(format!("已保存「{}」", c.name))
                        }
                        Err(e) => Some(format!("保存失败：{e}")),
                    },
                    CharacterEvent::Delete(id) => match roster.remove(id) {
                        Ok(()) => {
                            record("删除人物", &snapshot(&roster.all()));
                            Some("已删除".to_string())
                        }
                        Err(e) => Some(format!("删除失败：{e}")),
                    },
                    CharacterEvent::Merge { into, from } => {
                        match roster.merge(into, from) {
                            Ok(()) => {
                                record("合并人物", &snapshot(&roster.all()));
                                Some("已合并（别名并集、空格子补齐）".to_string())
                            }
                            Err(e) => Some(format!("合并失败：{e}")),
                        }
                    }
                    CharacterEvent::AddAlias(alias) => {
                        let Some(mut c) = this
                            .characters_view
                            .read(cx)
                            .draft(cx)
                        else {
                            return;
                        };
                        let a = alias.trim().to_string();
                        if a.is_empty() {
                            Some("别名不能为空".to_string())
                        } else if c.aliases.iter().any(|x| *x == a) || c.name == a {
                            Some("这个别名已经有了".to_string())
                        } else {
                            c.aliases.push(a.clone());
                            match roster.upsert(c) {
                                Ok(()) => {
                                    record("加别名", &snapshot(&roster.all()));
                                    Some(format!("已加别名「{a}」"))
                                }
                                Err(e) => Some(format!("加别名失败：{e}")),
                            }
                        }
                    }
                    CharacterEvent::RemoveAlias(alias) => {
                        let Some(mut c) = this.characters_view.read(cx).draft(cx) else {
                            return;
                        };
                        c.aliases.retain(|x| x != alias);
                        match roster.upsert(c) {
                            Ok(()) => {
                                record("移除别名", &snapshot(&roster.all()));
                                Some(format!("已移除别名「{alias}」"))
                            }
                            Err(e) => Some(format!("移除失败：{e}")),
                        }
                    }
                    CharacterEvent::Migrate => {
                        let (added, _) = roster.migrate_from_memory(
                            &this.handle.memory_facts(),
                            &this.handle.aliases(),
                        );
                        if added > 0 {
                            record("从记忆迁移人物", &snapshot(&roster.all()));
                        }
                        Some(if added > 0 {
                            format!("已从记忆建了 {added} 个人物档案（只补空，不覆盖你写的）")
                        } else {
                            "没有新人物可迁移（别名与事实都已在档案里）".to_string()
                        })
                    }
                    CharacterEvent::ToggleLock(field) => {
                        let Some(mut c) = this.characters_view.read(cx).draft(cx) else {
                            return;
                        };
                        if c.is_locked(field) {
                            c.locked_fields.retain(|f| f != field);
                        } else {
                            c.locked_fields.push(field.clone());
                        }
                        match roster.upsert(c) {
                            Ok(()) => {
                                record("切换字段锁", &snapshot(&roster.all()));
                                None
                            }
                            Err(e) => Some(format!("切换锁失败：{e}")),
                        }
                    }
                };
                if let Some(text) = msg {
                    this.characters_view
                        .update(cx, |p, cx| p.set_message(text, window, cx));
                }
            },
        )
        .detach();

// 改动记录：撤销要把结果如实说出来——尤其“冲突”不能当成功
        cx.subscribe_in(
            &journal_view,
            window,
            |this, _, ev: &JournalEvent, window, cx| {
                let journal = crate::journal::Journal::new(&this.handle.root);
                match ev {
                    JournalEvent::Refresh => {
                        this.journal_view.update(cx, |panel, cx| panel.reload(cx));
                    }
                    JournalEvent::RevertBreaches => {
                        let auth = crate::journal::BookAuth::load(&this.handle.root);
                        let breaches = crate::journal::find_breaches(&this.handle.root, &auth, 200);
                        if breaches.is_empty() {
                            this.journal_view.update(cx, |panel, cx| {
                                panel.set_message("没有越界改动".into(), 0, cx)
                            });
                        } else {
                            let (ok, errs) = crate::journal::revert_breaches(&this.handle.root, &breaches);
                            let text = if errs.is_empty() {
                                format!("已撤回 {ok} 处越界改动")
                            } else {
                                format!("撤回 {ok} 处；{}", errs.join("；"))
                            };
                            let tone = if errs.is_empty() { 0u8 } else { 1u8 };
                            this.reload_prose_from_disk(window, cx);
                            this.refresh_ai_marks(cx);
                            this.journal_view
                                .update(cx, |panel, cx| panel.set_message(text, tone, cx));
                        }
                    }
                    JournalEvent::Undo(id) => {
                        let outcome = journal.undo(&this.handle.root, id);
                        let (text, tone): (String, u8) = match outcome {
                            Ok(crate::journal::UndoOutcome::Restored) => {
                                ("已撤销：文件写回改动前的内容".to_string(), 0)
                            }
                            Ok(crate::journal::UndoOutcome::Removed) => {
                                ("已撤销：那是新建的文件，已删除".to_string(), 0)
                            }
                            Ok(crate::journal::UndoOutcome::Conflict { .. }) => (
                                "撤销被拒：这个文件在那之后又被改过，避免覆盖新内容".to_string(),
                                1,
                            ),
                            Err(e) => (format!("撤销失败：{e}"), 2),
                        };
                        // 正文可能被撤销改回：重载编辑器缓冲，避免自动保存把旧内容写回去
                        this.reload_prose_from_disk(window, cx);
                        this.refresh_ai_marks(cx);
                        this.journal_view
                            .update(cx, |panel, cx| panel.set_message(text, tone, cx));
                    }
                    JournalEvent::UndoBatch(batch) => {
                        let results = journal.undo_batch(&this.handle.root, &batch);
                        let ok = results
                            .iter()
                            .filter(|(_, r)| {
                                matches!(r, Ok(crate::journal::UndoOutcome::Restored | crate::journal::UndoOutcome::Removed))
                            })
                            .count();
                        let conflicts = results
                            .iter()
                            .filter(|(_, r)| matches!(r, Ok(crate::journal::UndoOutcome::Conflict { .. })))
                            .count();
                        let text = if conflicts == 0 {
                            format!("整批撤销完成：{ok} 个文件已还原")
                        } else {
                            format!("整批撤销：{ok} 个已还原，{conflicts} 个因之后被改过而跳过")
                        };
                        let tone = if conflicts == 0 { 0u8 } else { 1u8 };
                        this.reload_prose_from_disk(window, cx);
                        this.refresh_ai_marks(cx);
                        this.journal_view
                            .update(cx, |panel, cx| panel.set_message(text, tone, cx));
                    }
                }
            },
        )
        .detach();

// 世界线（排期板）：AI 核对走 DSH 会话（轨迹窗可见），拖卡改期是作者亲笔直写
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
                    // 采纳前校验原文还在：工具给了 original，作者可能在等待期间改了稿。
                    // 直接把建议套到当前光标/选区上会把文字塞进不相干的位置。
                    let doc = this.prose.read(cx).value().to_string();
                    let original = this.selected_original.clone().unwrap_or_default();
                    if !original.trim().is_empty() && !doc.contains(&original) {
                        this.trajectory.update(cx, |panel, cx| {
                            panel.push(
                                TrajKind::Fail(
                                    "原文已被改动，这条改写建议作废（请重新生成）".into(),
                                ),
                                cx,
                            );
                        });
                        this.selected_original = None;
                        return;
                    }
                    let _ = this.handle.snapshot_prose(this.chapter);
                    this.prose.update(cx, |state, cx| {
                        state.replace(text.as_str(), window, cx);
                    });
                    this.selected_original = None;
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
                        // branches 里存的就是走向正文（工具参数直接给），无需再剥前缀
                        let content = body.clone();
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
                    this.selected_original = Some(selected.clone());
                    let task = prompts::selection_instruction(kind, &selected);
                    this.on_send(task, cx);
                }
            },
        )
        .detach();

        let meta = handle.project_meta();
        let meta_genre = cx.new(|cx| {
            InputState::new(window, cx)
                .placeholder("题材，如 武侠 / 悬疑")
                .default_value(meta.genre.clone())
        });
        let meta_premise = cx.new(|cx| {
            InputState::new(window, cx)
                .placeholder("一句话故事核（premise）")
                .default_value(meta.premise.clone())
        });
        let meta_target = cx.new(|cx| {
            InputState::new(window, cx)
                .placeholder("目标章数")
                .default_value(meta.target_chapters.to_string())
        });
        let meta_word_target = cx.new(|cx| {
            InputState::new(window, cx)
                .placeholder("单章目标字数")
                .default_value(meta.chapter_word_target.to_string())
        });

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
            journal_view,
            import_view,
            graph_view,
            canvas_view,
            director_view,
            oracle_view,
            worldline_view,
            bookinfo_view,
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
            selected_original: None,
            agent_mode: dsh::AgentMode::Managed,
            book_auth,
            session_id,
            session_attached: false,
            pending_approval: None,
            pending_question: None,
            turn_active: false,
            meta_genre,
            meta_premise,
            meta_target,
            meta_word_target,
            meta_open: false,
            autosave: None,
            last_saved: String::new(),
            dsh_running: false,
            dsh_last_active: std::time::Instant::now(),
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

    fn accept_proposal(&mut self, id: &str, window: &mut Window, cx: &mut Context<Self>) {
        // 采纳 draft/deslop 会直接改磁盘正文：成功之后必须把编辑器缓冲区从磁盘重载，
        // 否则随后任意输入触发的 1.5s 自动保存都会用旧缓冲把刚采纳的正文写回去。
        let touches_current_prose = self
            .handle
            .proposals()
            .into_iter()
            .find(|p| p.id == id)
            .is_some_and(|p| {
                let kind = p.payload.get("kind").and_then(|k| k.as_str());
                let chapter = p.payload.get("chapter").and_then(|c| c.as_u64()).unwrap_or(0) as u32;
                matches!(kind, Some("draft") | Some("deslop")) && chapter == self.chapter
            });

        // 采纳 draft/deslop 会先对磁盘正文做快照：1.5s 自动保存窗口内的作者手写
        // 必须先落盘，否则会被采纳后的磁盘重载覆盖，而且不在快照里。
        if touches_current_prose {
            self.save_current(cx);
            // 落盘失败时不能继续采纳：否则快照与随后的磁盘重载都会吃掉未保存手写。
            let buffer = self.prose.read(cx).value().to_string();
            if self.handle.prose(self.chapter) != buffer {
                self.trajectory.update(cx, |panel, cx| {
                    panel.push(
                        TrajKind::Fail("正文未落盘，采纳已取消（先处理保存失败）".into()),
                        cx,
                    );
                });
                return;
            }
        }

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
                if applied > 0 && touches_current_prose {
                    self.reload_prose_from_disk(window, cx);
                }
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

    /// 用磁盘正文覆盖编辑器缓冲（仅用于磁盘侧刚被确定性采纳改写的章节）。
    fn reload_prose_from_disk(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let text = self.handle.prose(self.chapter);
        let dirty = self.prose.read(cx).value().to_string() != text;
        if dirty {
            self.prose.update(cx, |state, cx| {
                state.set_value(text.as_str(), window, cx);
            });
        }
    }

    /// 重新读当前章的 AI 参与标记（撤销后正文变回作者原文，墨点应随之消失）。
    fn refresh_ai_marks(&mut self, cx: &mut Context<Self>) {
        let marks =
            crate::journal::Journal::new(&self.handle.root).marks(&self.handle.root, self.chapter);
        self.editor.update(cx, |panel, cx| panel.set_ai_marks(marks, cx));
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

    /// 开发预览入口：打开作品后直接切到某页（ZHIBI_MODE）
    pub(crate) fn open_mode(&mut self, mode: Mode, window: &mut Window, cx: &mut Context<Self>) {
        self.switch_center(mode, window, cx);
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
            Mode::Journal => {
                self.journal_view.update(cx, |panel, cx| panel.reload(cx));
                panel_handle(self.journal_view.clone())
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
            Mode::Worldline => panel_handle(self.worldline_view.clone()),
            Mode::Oracle => panel_handle(self.oracle_view.clone()),
            Mode::BookInfo => panel_handle(self.bookinfo_view.clone()),
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
        let marks = crate::journal::Journal::new(&self.handle.root).marks(&self.handle.root, n);
        self.editor.update(cx, |panel, cx| panel.set_ai_marks(marks, cx));
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
        // 空闲回收：进程放着不用就占内存（一本 DSH 约 100-200MB）。
        // 每次要用之前检查一次——比后台定时器简单，也不会在作者正忙时误杀。
        self.recycle_if_idle(cx);
        if let Some(client) = &self.client {
            if client.is_alive() {
                self.dsh_last_active = std::time::Instant::now();
                return Ok(client.clone());
            }
            // 进程已经没了（被上限顶掉 / 崩了）：丢掉死句柄重新起一个。
            // 会话历史在 DSH 的 jsonl 里，重建后 resume 接得回来。
            self.trajectory.update(cx, |panel, cx| {
                panel.push(TrajKind::Info("DSH 进程已不在，正在重启（会话历史保留）".into()), cx)
            });
            self.client = None;
            self.initialized = false;
            self.session_attached = false;
        }
        self.dsh_last_active = std::time::Instant::now();
        let mode = self.agent_mode;
        let (client, rx) = DshClient::spawn(&self.root, mode)?;
        self.trajectory.update(cx, |panel, cx| {
            panel.push(
                TrajKind::Info(format!(
                    "已启动 DSH 运行时（--profile sdk，{}模式，独立 home）",
                    match mode {
                        dsh::AgentMode::Managed => "托管",
                        dsh::AgentMode::Minimal => "极简",
                        dsh::AgentMode::Creator => "创造",
                    }
                )),
                cx,
            );
        });

        cx.spawn(async move |this, cx| {
            while let Ok(event) = rx.recv().await {
                match event {
                    DshEvent::Notification { method, params } => {
                        let _ = this.update(cx, |ws, cx| ws.on_notification(&method, params, cx));
                    }
                    DshEvent::Approval { id, session_id, tool, reason } => {
                        let _ = this.update(cx, |ws, cx| {
                            ws.on_approval(id, session_id, tool, reason, cx)
                        });
                    }
                    DshEvent::Question { id, session_id, request_id, questions } => {
                        let _ = this.update(cx, |ws, cx| {
                            ws.on_question(id, session_id, request_id, questions, cx)
                        });
                    }
                    DshEvent::Error(text) => {
                        let _ = this.update(cx, |ws, cx| {
                            // 运行时退出：清掉缓存 client 与握手标志，下次发送时重建进程；
                            // 否则之后所有会话都跳过握手、聊天永远停在「思考中…」。
                            ws.client = None;
                            ws.initialized = false;
                            ws.dsh_running = false;
                            ws.chat.update(cx, |panel, cx| panel.set_busy(false, cx));
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
        // 同一会话同时只跑一轮：并发发送会让两轮的工具调用、提案、选区改写互相串台。
        if self.turn_active || self.dsh_running {
            self.trajectory.update(cx, |panel, cx| {
                panel.push(
                    TrajKind::Info("上一轮还在跑——等它结束，或先停止".into()),
                    cx,
                );
            });
            return;
        }
        self.turn_active = true;
        self.chat.update(cx, |panel, cx| panel.set_busy(true, cx));
        self.trajectory.update(cx, |panel, cx| {
            panel.push(TrajKind::User(text.clone()), cx);
        });

        let client = match self.ensure_client(cx) {
            Ok(c) => c,
            Err(e) => {
                self.turn_active = false;
                self.chat.update(cx, |panel, cx| panel.set_busy(false, cx));
                self.trajectory.update(cx, |panel, cx| {
                    panel.push(TrajKind::Fail(format!("启动 DSH 失败：{e}")), cx);
                });
                return;
            }
        };

        let weak = cx.weak_entity();
        let session_id = self.session_id.clone();
        let query_text = text;
        cx.spawn(async move |_, cx| {
            let need_init = weak.update(cx, |ws, _| !ws.initialized).unwrap_or(false);
            if need_init {
                if let Err(e) = initialize_with(&client, &weak, cx).await {
                    // 握手失败不能把 initialized 留在 true：本次会话后续每次发送都要能重试
                    let _ = weak.update(cx, |ws, cx| {
                        ws.initialized = false;
                        ws.client = None;
                        ws.chat.update(cx, |panel, cx| panel.set_busy(false, cx));
                        ws.trajectory.update(cx, |panel, cx| {
                            panel.push(TrajKind::Fail(format!("握手失败：{e}")), cx)
                        });
                    });
                    return;
                }
                // 只有 initialize 成功才算完成进程级握手
                let _ = weak.update(cx, |ws, _| ws.initialized = true);
            }

            // 新进程接手：若本书已有历史会话，先 resume 再接回（切模式、崩溃重启都走这里）。
            // 会话不存在属正常（第一次用这本书），不算错误，也不提示。
            let need_resume = weak
                .update(cx, |ws, _| !ws.session_attached)
                .unwrap_or(false);
            if need_resume {
                match client.session_resume(&session_id).await {
                    Ok(_) => {
                        let _ = weak.update(cx, |ws, cx| {
                            ws.session_attached = true;
                            ws.trajectory.update(cx, |panel, cx| {
                                panel.push(
                                    TrajKind::Info("已接回本书的历史会话".into()),
                                    cx,
                                )
                            });
                        });
                    }
                    Err(_) => {
                        // 首次使用（没有历史）——静默，不打扰作者
                        let _ = weak.update(cx, |ws, _| ws.session_attached = true);
                    }
                }
            }

            let r = client.session_query(&session_id, &query_text).await;
            let _ = weak.update(cx, |ws, cx| {
                match r {
                    Ok(_) => ws.trajectory.update(cx, |panel, cx| {
                        panel.push(TrajKind::Info("已入队，等待 DSH 会话事件…".into()), cx)
                    }),
                    Err(e) => {
                        // 发送失败：结束「思考中」，并让下次发送重建运行时
                        ws.chat.update(cx, |panel, cx| panel.set_busy(false, cx));
                        ws.turn_active = false;
                        ws.client = None;
                        ws.initialized = false;
                        ws.trajectory.update(cx, |panel, cx| {
                            panel.push(TrajKind::Fail(format!("发送失败：{e}")), cx)
                        });
                    }
                }
                ws.inbox.update(cx, |panel, cx| panel.refresh(cx));
            });
        })
        .detach();
    }

    /// 审批请求：记下来交给 UI 渲染，同时在轨迹里留痕（不论作者怎么选都可见）。
    ///
    /// 这里不自动批准。DSH 的 fail-closed 语义正好保证「作者不点 = 不执行」，
    /// 所以只把卡片摆出来即可，不设超时兜底。
    /// 派活给某个专属 Agent。
    ///
    /// 两条路走同一个入口，区别只在有没有指定对象：
    /// - `target = None`：模块级一键能力（人物页「全量补全」）——已经点名了 preset，
    ///   主 Agent 只负责转交与汇总。
    /// - `target = Some(desc)`：针对某个指定对象（「补全沈砚」）——描述带上对象，
    ///   子 Agent 不用自己再查一遍才知道在说谁。
    ///
    /// 为什么页面要写死路由：作者点了「补全沈砚」，意图已经明确。这时再让主 Agent
    /// 自己挑 Agent，它可能挑错、可能自己干了、也可能反过来问作者——三次都算失败。
    fn delegate(
        &mut self,
        agent: &'static str,
        key: &'static str,
        target: Option<String>,
        cx: &mut Context<Self>,
    ) {
        let task = match target {
            Some(desc) => crate::capabilities::targeted_task(agent, key, &desc),
            None => crate::capabilities::bulk_task(agent, key),
        };
        match task {
            Some(t) => {
                // 轨迹里写清派给了谁：作者点了按钮之后要能确认"它真的去做了"，
                // 而不是只有一句"已交给 AI"。
                let label = crate::capabilities::agent(agent).map(|a| a.label).unwrap_or(agent);
                self.trajectory.update(cx, |panel, cx| {
                    panel.push(
                        TrajKind::Info(format!("已派给「{label}」Agent（轨迹窗可见全程）")),
                        cx,
                    )
                });
                self.on_send(t, cx);
            }
            // key 写错时宁可不发，也不要发一条主 Agent 看不懂的指令
            None => self.trajectory.update(cx, |panel, cx| {
                panel.push(
                    TrajKind::Fail(format!("内部错误：未知能力 {agent}/{key}")),
                    cx,
                )
            }),
        }
    }

    /// 空闲回收：闲置超过设置里 `idleRecycleMin` 分钟就关掉进程。
    ///
    /// 三条保护，避免把作者正在用的进程杀掉：
    /// 1. 回合进行中不回收（`turn_active` / `dsh_running`）；
    /// 2. 有待审批/待回答不回收（杀了等于替他拒绝）；
    /// 3. 设置为 0 表示永不回收。
    fn recycle_if_idle(&mut self, cx: &mut Context<Self>) {
        let Some(client) = self.client.as_ref() else { return };
        if self.turn_active || self.dsh_running {
            return;
        }
        if self.pending_approval.is_some() || self.pending_question.is_some() {
            return;
        }
        let minutes = crate::store::load_settings().idle_recycle_min;
        if minutes == 0 {
            return;
        }
        if self.dsh_last_active.elapsed() < std::time::Duration::from_secs(minutes as u64 * 60) {
            return;
        }
        // 关进程但保留会话：下次发送会重建 + session/resume 接回历史
        client.shutdown();
        self.client = None;
        self.initialized = false;
        self.session_attached = false;
        self.trajectory.update(cx, |panel, cx| {
            panel.push(
                TrajKind::Info(format!(
                    "DSH 运行时闲置 {minutes} 分钟，已释放内存（下次发送自动重建，会话历史保留）"
                )),
                cx,
            )
        });
    }

    /// 回合结束后的越界检查：AI 改了没有代笔授权的章 → 通报并给一键撤回。
    ///
    /// 为什么放在回合结束而不是每次写文件时：一次回合可能写很多次，
    /// 逐次通报会把轨迹刷满；而且作者在一次回合里更关心"这批改动整体合规吗"。
    fn check_breaches(&mut self, cx: &mut Context<Self>) {
        // 先查台账结构：AI 绕过工具直写的台账 JSON 如果结构坏了，这里自动回滚。
        //
        // 顺序放在越界检查之前：越界检查是基于 journal 的（AI 改过哪些章），
        // 而台账回滚本身也走 journal 的 undo——先回滚能把"已撤回"的状态定下来，
        // 后面的越界统计才不会把一条已经退回去的改动再报一次。
        let broken = crate::schema::validate_and_rollback_ai_edits(&self.root, 200);
        for p in &broken {
            self.trajectory.update(cx, |panel, cx| {
                panel.push(TrajKind::Fail(format!("⚠ {}", p.summary())), cx)
            });
        }

        let auth = crate::journal::BookAuth::load(&self.root);
        let breaches = crate::journal::find_breaches(&self.root, &auth, 200);
        if breaches.is_empty() {
            return;
        }
        let chapters: Vec<String> = breaches.iter().map(|b| format!("第{}章", b.chapter)).collect();
        let msg = format!(
            "⚠ AI 越界改了 {} 章（{}）：当前档位「{}」，这些章没有代笔授权。\
             改动已记录，可在「改动记录」撤回；或到设置里授权这些章。",
            breaches.len(),
            chapters.join("、"),
            auth.mode().label()
        );
        self.trajectory.update(cx, |panel, cx| {
            panel.push(TrajKind::Fail(msg.clone()), cx)
        });
        // 这里只通报，不自动撤。
        //
        // 为什么不在回合结束时自动撤销：撤销要看有没有冲突（作者中途手改过），
        // 而且"AI 越界改了正文"是作者必须自己做的判断——自动撤掉可能连
        // 他刚点过采纳的内容一起带走。把事实摆清楚，撤销放在改动记录页
        // （那里有逐条冲突提示），比悄悄改回去更尊重作者。
    }

    /// 停止当前回合。
    ///
    /// sdk 协议没有 `session/abort`，所以只能杀掉该书进程并让下次发送重建——
    /// 会话历史在 DSH 的 jsonl 里，重建后 `session/resume` 会把它接回来，对话不丢。
    /// 只影响本书，不动其他书的进程。
    fn stop_turn(&mut self, cx: &mut Context<Self>) {
        if !self.turn_active && !self.dsh_running {
            self.trajectory.update(cx, |panel, cx| {
                panel.push(TrajKind::Info("当前没有在跑的任务".into()), cx);
            });
            return;
        }
        if let Some(client) = self.client.take() {
            client.shutdown();
        }
        self.initialized = false;
        self.dsh_running = false;
        self.turn_active = false;
        self.pending_approval = None;
        self.pending_question = None;
        // 下次发送时重建进程并 resume 同一会话
        self.session_attached = false;
        self.chat.update(cx, |panel, cx| panel.set_busy(false, cx));
        self.trajectory.update(cx, |panel, cx| {
            panel.push(
                TrajKind::Info("已停止本轮（运行时将按需重建，会话历史保留）".into()),
                cx,
            );
        });
        cx.notify();
    }

    /// 切换主 Agent 形态。
    ///
    /// DSH 的执行器与 preset 是进程级组合，没法热切；所以：关掉当前进程 → 用新 patch 重启
    /// → 用 `session/resume` 把原会话接回来。会话历史在 DSH 的 jsonl 里，聊天不丢。
    fn switch_agent_mode(&mut self, mode: dsh::AgentMode, cx: &mut Context<Self>) {
        if self.agent_mode == mode {
            return;
        }
        self.agent_mode = mode;
        let label = match mode {
            dsh::AgentMode::Managed => "托管",
            dsh::AgentMode::Minimal => "极简",
            dsh::AgentMode::Creator => "创造",
        };
        // 关旧进程（shutdown 后 stdin 关闭，读线程会发 Error 让 UI 复位）
        if let Some(client) = self.client.take() {
            client.shutdown();
        }
        self.initialized = false;
        self.dsh_running = false;
        self.turn_active = false;
        // 新进程要重新接回会话：session_id 不变，历史在 DSH 的 jsonl 里
        self.session_attached = false;
        self.pending_approval = None;
        self.pending_question = None;
        self.chat.update(cx, |panel, cx| panel.set_busy(false, cx));
        self.trajectory.update(cx, |panel, cx| {
            panel.push(
                TrajKind::Info(format!("已切到{label}模式——运行时将按新组合重启，会话历史保留")),
                cx,
            );
        });
        cx.notify();
    }

    /// 同模式下的运行时重启（改 patch 内容后调用，例如存了本书专属 Agent）。
    ///
    /// 与 switch_agent_mode 的区别：模式没变，但生成的 patch 变了，
    /// 必须让进程重新读一遍。会话历史同样靠 session/resume 接回。
    fn restart_runtime(&mut self, cx: &mut Context<Self>) {
        if let Some(client) = self.client.take() {
            client.shutdown();
        }
        self.initialized = false;
        self.dsh_running = false;
        self.turn_active = false;
        self.session_attached = false;
        self.pending_approval = None;
        self.pending_question = None;
        self.chat.update(cx, |panel, cx| panel.set_busy(false, cx));
        cx.notify();
    }

    fn on_approval(
        &mut self,
        id: Value,
        session_id: String,
        tool: String,
        reason: String,
        cx: &mut Context<Self>,
    ) {
        self.trajectory.update(cx, |panel, cx| {
            panel.push(
                TrajKind::Info(format!("待审批 · {tool}：{reason}")),
                cx,
            );
        });
        self.pending_approval = Some(ApprovalRequest { id, session_id, tool, reason });
        cx.notify();
    }

    /// 作者点了审批卡。
    fn resolve_approval(&mut self, allow: bool, cx: &mut Context<Self>) {
        let Some(req) = self.pending_approval.take() else {
            return;
        };
        let outcome = if allow { "allowed-once" } else { "rejected" };
        let what = if allow { "已允许" } else { "已拒绝" };
        let result = self
            .client
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("运行时已退出"))
            .and_then(|c| c.answer_approval(&req.id, outcome));
        self.trajectory.update(cx, |panel, cx| {
            panel.push(
                match &result {
                    Ok(()) => TrajKind::Info(format!("{what}：{}", req.tool)),
                    Err(e) => TrajKind::Fail(format!("审批应答失败：{e}")),
                },
                cx,
            );
        });
        cx.notify();
    }

    /// 提问请求：把问题摆出来等作者回答；作者取消也能正常收尾。
    fn on_question(
        &mut self,
        id: Value,
        session_id: String,
        request_id: String,
        questions: Value,
        cx: &mut Context<Self>,
    ) {
        let head = questions
            .as_array()
            .and_then(|a| a.first())
            .and_then(|q| q.get("question"))
            .and_then(|q| q.as_str())
            .unwrap_or("（AI 有问题要问你）")
            .to_string();
        self.trajectory.update(cx, |panel, cx| {
            panel.push(TrajKind::Info(format!("待回答 · {head}")), cx);
        });
        self.pending_question = Some(QuestionRequest { id, session_id, request_id, questions });
        cx.notify();
    }

    /// 作者回答了提问。`answers` 为 `None` 表示取消（DSH 会结束本轮，不算批准）。
    fn resolve_question(&mut self, answers: Option<Value>, cx: &mut Context<Self>) {
        let Some(req) = self.pending_question.take() else {
            return;
        };
        let result = self
            .client
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("运行时已退出"))
            .and_then(|c| c.answer_question(&req.id, answers));
        self.trajectory.update(cx, |panel, cx| {
            panel.push(
                match &result {
                    Ok(()) => TrajKind::Info("已回答".into()),
                    Err(e) => TrajKind::Fail(format!("提问应答失败：{e}")),
                },
                cx,
            );
        });
        cx.notify();
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
                if !running {
                    // 兜底：空闲态一定结束「思考中…」（turn/end 可能因协议演进缺失）
                    self.chat.update(cx, |panel, cx| panel.set_busy(false, cx));
                    self.turn_active = false;
                }
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
                self.chat.update(cx, |panel, cx| panel.set_busy(false, cx));
                // AI 回合中登记的 propose_* 提案要自动出现在收件箱，不能等作者手动刷新
                self.inbox.update(cx, |panel, cx| panel.refresh(cx));
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

    /// 作者亲笔的台账增删改。
    ///
    /// 三条来源（大纲页 / 记忆页 / 伏笔页）走同一份实现：store 的 CRUD 会自己
    /// 记改动日志，所以撤销入口不需要在这里额外发事件。
    fn on_ledger_event(&mut self, ev: &ViewEvent, cx: &mut Context<Self>) {
        let result: Result<String, String> = match ev {
            ViewEvent::LedgerAdd(which, item) => self
                .handle
                .ledger_add(*which, item.clone(), &format!("新建{}", which.label()))
                .map(|id| format!("已新建{} {id}", which.label())),
            ViewEvent::LedgerUpdate(which, id, patch) => self
                .handle
                .ledger_update(*which, id, patch.clone(), &format!("编辑{}", which.label()))
                .map(|()| format!("已保存{} {id}", which.label())),
            ViewEvent::LedgerRemove(which, id) => self
                .handle
                .ledger_remove(*which, id, &format!("删除{}", which.label()))
                .map(|()| format!("已删除{} {id}", which.label())),
            // 调用方只会传这三种；其余是订阅点共用的宽分支，忽略。
            _ => return,
        };
        match result {
            Ok(msg) => self.trajectory.update(cx, |panel, cx| panel.push(TrajKind::Info(msg), cx)),
            Err(e) => self
                .trajectory
                .update(cx, |panel, cx| panel.push(TrajKind::Fail(format!("台账操作失败：{e}")), cx)),
        }
    }
}

/// 生成一个该线内不重复的节点 id（作者亲笔建的节点没有 AI 给的 id）。
fn next_node_id(line: &str, handle: &crate::store::ProjectHandle) -> String {
    let existing: Vec<String> = crate::worldline::Store::new(&handle.root)
        .find(line)
        .ok()
        .flatten()
        .map(|l| l.nodes.into_iter().map(|n| n.id).collect())
        .unwrap_or_default();
    let mut n = 1;
    loop {
        let cand = format!("{line}-n{n}");
        if !existing.iter().any(|e| e == &cand) {
            return cand;
        }
        n += 1;
    }
}

/// 生成下一个未占用的世界线 id（β、γ…，用完了就叫 β2）。
fn next_line_id(store: &crate::worldline::Store) -> String {
    const GREEK: &[&str] = &[
        "β", "γ", "δ", "ε", "ζ", "η", "θ", "ι", "κ", "λ", "μ", "ν", "ξ", "ο", "π", "ρ", "σ",
        "τ", "υ", "φ", "χ", "ψ", "ω",
    ];
    let used: Vec<String> = store
        .all()
        .unwrap_or_default()
        .into_iter()
        .map(|l| l.id)
        .collect();
    for g in GREEK {
        if !used.iter().any(|u| u == g) {
            return (*g).to_string();
        }
    }
    let mut n = 2;
    loop {
        let cand = format!("β{n}");
        if !used.iter().any(|u| u == &cand) {
            return cand;
        }
        n += 1;
    }
}

impl Workspace {
    /// 三个轻交互工具的参数直接落成 UI 状态。
    ///
    /// 这是“不再解析模型文本”的落点：分支卡、改写建议、续写都由工具参数给出，
    /// 前端只读参数，不猜格式，也不会把普通聊天里出现的符号误判成卡片。
    fn on_card_tool(&mut self, name: &str, args: &Value, cx: &mut Context<Self>) {
        match name {
            "offer_branches" => {
                let branches: Vec<String> = args
                    .get("branches")
                    .and_then(|b| b.as_array())
                    .map(|arr| {
                        arr.iter()
                            .filter_map(|b| {
                                // 只存正文：标签/情绪由工具参数单独给出，
                                // 采纳时不需要再从字符串里剥前缀。
                                b.get("text")
                                    .and_then(|t| t.as_str())
                                    .map(|t| t.to_string())
                            })
                            .collect()
                    })
                    .unwrap_or_default();
                if branches.is_empty() {
                    return;
                }
                let n = branches.len();
                self.branches = branches.clone();
                self.editor.update(cx, |panel, cx| panel.set_branches(branches, cx));
                self.chat.update(cx, |panel, cx| {
                    panel.push(
                        ChatMsg {
                            role: "assistant".into(),
                            text: "给了几个走向——在稿纸下方选一个，或换一批。".into(),
                        },
                        cx,
                    );
                });
                self.trajectory.update(cx, |panel, cx| {
                    panel.push(
                        TrajKind::Info(format!("{n} 个走向已就绪，稿纸下方可选")),
                        cx,
                    );
                });
            }
            "offer_rewrite" => {
                let original = args
                    .get("original")
                    .and_then(|o| o.as_str())
                    .unwrap_or("")
                    .to_string();
                let replacement = args
                    .get("replacement")
                    .and_then(|r| r.as_str())
                    .unwrap_or("")
                    .to_string();
                let kind = args.get("kind").and_then(|k| k.as_str()).unwrap_or("rewrite");
                if original.trim().is_empty() || replacement.trim().is_empty() {
                    return;
                }
                // 记下原文，采纳时校验（作者可能在等待期间改了稿）
                self.selected_original = Some(original.clone());
                self.trajectory.update(cx, |panel, cx| {
                    panel.push(
                        TrajKind::SelectionDraft {
                            original,
                            text: replacement,
                        },
                        cx,
                    );
                });
                self.trajectory.update(cx, |panel, cx| {
                    panel.push(
                        TrajKind::Info(format!("改写建议（{kind}）已就绪，可就地采纳")),
                        cx,
                    );
                });
            }
            "offer_continuation" => {
                let text = args
                    .get("text")
                    .and_then(|t| t.as_str())
                    .unwrap_or("")
                    .to_string();
                if text.trim().is_empty() {
                    return;
                }
                self.trajectory.update(cx, |panel, cx| {
                    panel.push(TrajKind::Text(text), cx);
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

                // 轻交互卡片：参数是结构化的，直接在这里落成 UI 状态。
                // 这是替代“解析模型文本猜意图”的地方——工具参数不会误判。
                if let Ok(args) = serde_json::from_str::<Value>(&raw) {
                    self.on_card_tool(&name, &args, cx);
                }

                let args: String = raw.chars().take(160).collect();
                let call_id = data
                    .get("callId")
                    .and_then(|c| c.as_str())
                    .map(String::from);
                Some(TrajKind::Tool { name, args, call_id })
            }
            "tool/result" => {
                let is_err = data
                    .get("message")
                    .and_then(|m| m.get("isError"))
                    .and_then(|e| e.as_bool())
                    .unwrap_or(false)
                    || data.get("error").is_some();
                let content = summarize_tool_result(&data);
                // DSH tool/result 的 callId 在 message.source.callId（兼容 toolCallId/callId）：
                // 带上它，轨迹按 id 回填，乱序/并行结果不会错配。
                let call_id = data
                    .get("message")
                    .and_then(|m| {
                        m.get("source")
                            .and_then(|s| s.get("callId"))
                            .or_else(|| m.get("toolCallId"))
                    })
                    .or_else(|| data.get("callId"))
                    .and_then(|c| c.as_str())
                    .map(String::from);
                Some(if is_err {
                    TrajKind::ToolErr { text: content, call_id }
                } else {
                    TrajKind::ToolOk { text: content, call_id }
                })
            }
            "assistant/message" => {
                let text = assistant_text_full(&data);
                if text.is_empty() {
                    None
                } else {
                    // 走向/改写/续写这些结构化产出走 offer_* 工具，不走文本。
                    // 这里的纯文本就是对话本身。
                    let t = text.clone();
                    self.chat.update(cx, |panel, cx| {
                        panel.push(ChatMsg { role: "assistant".into(), text: t }, cx);
                    });
                    Some(TrajKind::Text(text))
                }
            }
            "turn/end" => {
                // 无论完成/中止/失败，本轮都结束了——聊天不能继续停在「思考中…」；
                // 本轮登记的提案也在这里自动刷新进收件箱。
                self.chat.update(cx, |panel, cx| panel.set_busy(false, cx));
                self.turn_active = false;
                self.inbox.update(cx, |panel, cx| panel.refresh(cx));
                // 越界检查：事前 write-policy 拦工具调用，但 AI 也可能绕过
                // novel-pack 直接写文件。回合结束时扫一遍改动日志，
                // 发现"改了没授权的章"就地通报，作者可以一键撤回。
                self.check_breaches(cx);
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
    // cwd 必须是书目录：DSH 的 workspace-write 边界、项目内 AGENTS.md 与技能目录
    // 都按它解析。用 GUI 进程的 cwd 会让沙箱边界完全错位。
    let (root, settings) = weak
        .update(cx, |ws, _| (ws.root.clone(), crate::store::load_settings()))
        .map_err(|_| anyhow::anyhow!("工作区已关闭"))?;
    let cwd = root.display().to_string();
    let home = crate::dsh::home_dir(&root);
    let (provider, model) = model_route(&settings, &home);
    let result = client
        .initialize(
            &cwd,
            &provider,
            &model,
            settings.effort(),
            Some(settings.max_tokens),
        )
        .await?;
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
            // 审批 / 提问浮层挂在根节点上：它们在哪个页面都可能出现，不能只在稿纸页
            .relative()
            .children(self.pending_card(cx))
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
                                            .child("执笔造物"),
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
                                    .child(self.writing_chip(cx))
                                    .child(self.mode_chip(cx))
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
                    // 停止只在真的在跑时出现：没有可停的就不要摆一个按不动的按钮
                    .children(if dsh_running {
                        Some(
                            Button::new("stop-turn")
                                .custom(icon_btn(&*cx))
                                .small()
                                .rounded(theme::RADIUS_BTN_SM)
                                .label("停止")
                                .on_click(cx.listener(|this, _, _window, cx| {
                                    this.stop_turn(cx);
                                })),
                        )
                    } else {
                        None
                    })
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
    /// 写作档位切换器（辅助 / 共写 / 代笔）。
    ///
    /// 切档位只写书目录的 zhibi-auth.json，不重启运行时——novel-pack 每次工具调用
    /// 重读它，所以立刻生效。代笔档还需要作者勾选具体章节。
    fn writing_chip(&self, cx: &mut Context<Self>) -> AnyElement {
        use crate::journal::WritingMode;
        let cur = self.book_auth.mode();
        let mk = |label: &'static str, key: &'static str, mode: WritingMode| {
            let on = cur == mode;
            div()
                .id(key)
                .px(px(9.))
                .py(px(2.))
                .rounded(px(5.))
                .border_1()
                .border_color(if on {
                    match mode {
                        WritingMode::Assist => theme::LINE(),
                        WritingMode::Cowrite => theme::ACCENT_BORDER(),
                        WritingMode::Ghostwrite => theme::AMBER(),
                    }
                } else {
                    theme::LINE()
                })
                .bg(if on { theme::ACCENT_SOFT() } else { theme::PANEL2() })
                .cursor_pointer()
                .child(
                    div()
                        .text_size(theme::FONT_MICRO)
                        .text_color(if on { theme::ACCENT() } else { theme::DIM() })
                        .child(label),
                )
                .on_click(cx.listener(move |this, _, _, cx| {
                    this.set_writing_mode(mode, cx);
                }))
        };
        let mut row = div()
            .flex()
            .items_center()
            .gap(px(3.))
            .child(mk("辅助", "wm-assist", WritingMode::Assist))
            .child(mk("共写", "wm-cowrite", WritingMode::Cowrite))
            .child(mk("代笔", "wm-ghost", WritingMode::Ghostwrite));

        // 代笔档要有明确的“哪一章被授权”，否则是个隐形的危险开关
        if cur == WritingMode::Ghostwrite {
            let ch = self.chapter;
            let authorized = self.book_auth.ghostwrite_chapters.contains(&ch);
            row = row.child(
                div()
                    .id("wm-authorize")
                    .ml(px(6.))
                    .px(px(8.))
                    .py(px(2.))
                    .rounded(px(5.))
                    .border_1()
                    .border_color(if authorized { theme::GREEN() } else { theme::LINE() })
                    .cursor_pointer()
                    .child(
                        div()
                            .text_size(theme::FONT_MICRO)
                            .text_color(if authorized { theme::GREEN() } else { theme::DIM() })
                            .child(if authorized {
                                format!("第{ch}章已授权")
                            } else {
                                format!("授权第{ch}章")
                            }),
                    )
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.toggle_ghostwrite_chapter(cx);
                    })),
            );
        }
        row.into_any_element()
    }

    fn set_writing_mode(&mut self, mode: crate::journal::WritingMode, cx: &mut Context<Self>) {
        self.book_auth.set_mode(mode);
        match self.book_auth.save(&self.handle.root) {
            Ok(()) => {
                let extra = if mode == crate::journal::WritingMode::Ghostwrite {
                    "（还需勾选具体章节，AI 才能写那一章正文）"
                } else {
                    ""
                };
                self.trajectory.update(cx, |panel, cx| {
                    panel.push(
                        TrajKind::Info(format!("写作档位 → {}{extra}", mode.label())),
                        cx,
                    );
                });
            }
            Err(e) => {
                self.trajectory.update(cx, |panel, cx| {
                    panel.push(TrajKind::Fail(format!("切档位失败：{e}")), cx);
                });
            }
        }
        cx.notify();
    }

    fn toggle_ghostwrite_chapter(&mut self, cx: &mut Context<Self>) {
        let ch = self.chapter;
        self.book_auth.toggle_chapter(ch);
        let on = self.book_auth.ghostwrite_chapters.contains(&ch);
        if let Err(e) = self.book_auth.save(&self.handle.root) {
            self.trajectory.update(cx, |panel, cx| {
                panel.push(TrajKind::Fail(format!("保存授权失败：{e}")), cx);
            });
        } else {
            self.trajectory.update(cx, |panel, cx| {
                panel.push(
                    TrajKind::Info(format!(
                        "第{ch}章代笔授权：{}",
                        if on { "已开启" } else { "已取消" }
                    )),
                    cx,
                );
            });
        }
        cx.notify();
    }

    /// 主 Agent 形态的三段切换器（托管 / 极简 / 创造）。
    ///
    /// 切换要重启本书的 DSH 进程（执行器与 preset 是进程级组合），所以点之前
    /// 在轨迹窗写清后果；已完成的对话历史由 DSH 会话日志保留。
    fn mode_chip(&self, cx: &mut Context<Self>) -> AnyElement {
        let cur = self.agent_mode;
        let seg = |label: &'static str, key: &'static str, mode: AgentMode| {
            let on = cur == mode;
            div()
                .id(key)
                .px(px(9.))
                .py(px(2.))
                .rounded(px(5.))
                .border_1()
                .border_color(if on { theme::ACCENT_BORDER() } else { theme::LINE() })
                .bg(if on { theme::ACCENT_SOFT() } else { theme::PANEL2() })
                .cursor_pointer()
                .child(
                    div()
                        .text_size(theme::FONT_MICRO)
                        .text_color(if on { theme::ACCENT() } else { theme::DIM() })
                        .child(label),
                )
                .on_click(cx.listener(move |this, _, _window, cx| {
                    this.switch_agent_mode(mode, cx);
                }))
        };
        div()
            .flex()
            .items_center()
            .gap(px(3.))
            .child(seg("托管", "agent-mode-managed", AgentMode::Managed))
            .child(seg("极简", "agent-mode-minimal", AgentMode::Minimal))
            .child(seg("创造", "agent-mode-creator", AgentMode::Creator))
            .into_any_element()
    }

    /// 待审批 / 待回答的浮层：两者都必须由作者点，否则 DSH 会拒绝或空等。
    fn pending_card(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        if let Some(req) = &self.pending_approval {
            let tool = req.tool.clone();
            let reason = req.reason.clone();
            let allow = Button::new("approve-yes")
                .custom(prominent_btn(&*cx))
                .small()
                .rounded(theme::RADIUS_BTN_SM)
                .label("允许一次")
                .on_click(cx.listener(|this, _, _window, cx| this.resolve_approval(true, cx)));
            let deny = Button::new("approve-no")
                .custom(icon_btn(&*cx))
                .small()
                .rounded(theme::RADIUS_BTN_SM)
                .label("拒绝")
                .on_click(cx.listener(|this, _, _window, cx| this.resolve_approval(false, cx)));
            return Some(
                div()
                    .absolute()
                    .bottom(px(48.))
                    .left(px(0.))
                    .right(px(0.))
                    .flex()
                    .justify_center()
                    .child(
                        div()
                            .w(px(520.))
                            .rounded(theme::RADIUS_CARD)
                            .border_1()
                            .border_color(theme::AMBER())
                            .bg(theme::PANEL())
                            .p(theme::PAD_LG)
                            .flex()
                            .flex_col()
                            .gap(theme::GAP_SM)
                            .child(
                                div()
                                    .text_size(theme::FONT_LABEL)
                                    .font_weight(FontWeight::MEDIUM)
                                    .text_color(theme::AMBER())
                                    .child(SharedString::from(format!("AI 请求：{tool}"))),
                            )
                            .child(
                                div()
                                    .text_size(theme::FONT_MICRO)
                                    .text_color(theme::DIM())
                                    .child(SharedString::from(reason)),
                            )
                            .child(
                                div()
                                    .flex()
                                    .gap(theme::GAP_SM)
                                    .child(allow)
                                    .child(deny),
                            ),
                    )
                    .into_any_element(),
            );
        }
        let req = self.pending_question.as_ref()?;
        let first = req
            .questions
            .as_array()
            .and_then(|a| a.first())
            .cloned()
            .unwrap_or(serde_json::Value::Null);
        let text = first
            .get("question")
            .and_then(|q| q.as_str())
            .unwrap_or("AI 有问题要问你")
            .to_string();
        let options: Vec<String> = first
            .get("options")
            .and_then(|o| o.as_array())
            .map(|arr| {
                arr.iter()
                    .filter_map(|o| o.get("label").and_then(|l| l.as_str()).map(String::from))
                    .collect()
            })
            .unwrap_or_default();
        let qid = first
            .get("id")
            .and_then(|i| i.as_str())
            .unwrap_or("q")
            .to_string();

        // 选项就地点；没有选项只能取消（自由文本输入留到后续版本）
        let mut buttons: Vec<AnyElement> = Vec::new();
        for (ix, label) in options.into_iter().enumerate() {
            let qid = qid.clone();
            let label2 = label.clone();
            buttons.push(
                Button::new(SharedString::from(format!("q-opt-{ix}")))
                    .custom(icon_btn(&*cx))
                    .small()
                    .rounded(theme::RADIUS_BTN_SM)
                    .label(SharedString::from(label))
                    .on_click(cx.listener(move |this, _, _window, cx| {
                        let answers = serde_json::json!([{ "id": qid, "selected": [label2] }]);
                        this.resolve_question(Some(answers), cx);
                    }))
                    .into_any_element(),
            );
        }
        buttons.push(
            Button::new("q-cancel")
                .custom(icon_btn(&*cx))
                .small()
                .rounded(theme::RADIUS_BTN_SM)
                .label("取消")
                .on_click(cx.listener(|this, _, _window, cx| this.resolve_question(None, cx)))
                .into_any_element(),
        );

        Some(
            div()
                .absolute()
                .bottom(px(48.))
                .left(px(0.))
                .right(px(0.))
                .flex()
                .justify_center()
                .child(
                    div()
                        .w(px(560.))
                        .rounded(theme::RADIUS_CARD)
                        .border_1()
                        .border_color(theme::ACCENT_BORDER())
                        .bg(theme::PANEL())
                        .p(theme::PAD_LG)
                        .flex()
                        .flex_col()
                        .gap(theme::GAP_SM)
                        .child(
                            div()
                                .text_size(theme::FONT_LABEL)
                                .text_color(theme::TEXT())
                                .child(SharedString::from(text)),
                        )
                        .child(
                            div()
                                .flex()
                                .flex_wrap()
                                .gap(theme::GAP_SM)
                                .children(buttons),
                        ),
                )
                .into_any_element(),
        )
    }

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
                    .flex()
                    .flex_col()
                    .gap(theme::GAP_SM)
                    .child(
                        Button::new("meta-toggle")
                            .custom(icon_btn(&*cx))
                            .outline()
                            .small()
                            .rounded(theme::RADIUS_BTN_SM)
                            .w_full()
                            .label(if self.meta_open { "收起作品信息" } else { "作品信息" })
                            .tooltip("编辑题材 / 故事核 / 目标章数 / 单章字数")
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.meta_open = !this.meta_open;
                                cx.notify();
                            })),
                    )
                    .when(self.meta_open, |d| {
                        d.child(
                            div()
                                .flex()
                                .flex_col()
                                .gap(theme::GAP_XS)
                                .child(meta_field("题材", &self.meta_genre))
                                .child(meta_field("故事核", &self.meta_premise))
                                .child(meta_field("目标章数", &self.meta_target))
                                .child(meta_field("单章字数", &self.meta_word_target))
                                .child(
                                    Button::new("meta-save")
                                        .custom(prominent_btn(&*cx))
                                        .outline()
                                        .small()
                                        .rounded(theme::RADIUS_BTN_SM)
                                        .w_full()
                                        .label("保存作品信息")
                                        .on_click(cx.listener(|this, _, _, cx| {
                                            let genre = this.meta_genre.read(cx).value().trim().to_string();
                                            let premise = this.meta_premise.read(cx).value().trim().to_string();
                                            let target = this
                                                .meta_target
                                                .read(cx)
                                                .value()
                                                .trim()
                                                .parse::<u32>()
                                                .unwrap_or(0);
                                            let word_target = this
                                                .meta_word_target
                                                .read(cx)
                                                .value()
                                                .trim()
                                                .parse::<u32>()
                                                .unwrap_or(0);
                                            match this.handle.update_project_meta(
                                                &genre,
                                                &premise,
                                                target,
                                                word_target,
                                            ) {
                                                Ok(()) => {
                                                    this.editor.update(cx, |panel, _| {
                                                        panel.word_target = word_target;
                                                    });
                                                    this.show_toast("作品信息已保存", cx);
                                                }
                                                Err(e) => {
                                                    this.show_toast(&format!("保存失败：{e}"), cx)
                                                }
                                            }
                                        })),
                                ),
                        )
                    })
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
