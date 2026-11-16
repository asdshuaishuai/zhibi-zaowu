//! 世界线页：剧情的时间轴 + if 分支。
//!
//! 这一页回答一个问题：**"故事确定到哪儿了，之后有哪些可能？"**
//!
//! 与另外两页的分工（这三页最容易混，写在这里当下锚点）：
//! - **世界线（本页）**：一条已固化的直线 + 从"现在"扇出的几种未来。
//!   左边横轴 = 章节，只画到固化边界；右边不标章号，是从当前节点散开的
//!   预演（`forecast`），给作者看"接下来可能怎么走"。回答
//!   "故事确定到哪儿了，之后有哪些可能"。
//! - **画布**：关系网。节点 = 人物/设定/道具/事件，边 = 人物关系与剧情关联。
//!   回答"这个关节跟谁有关"。via 双向弱引用可跳到本页的具体章节。
//! - **预言书**：if 存根。写假设、让 AI 推演、看影响清单。这里的每条线都只是
//!   if 分支的存档，**与主线无关**：既不改正典，也不会被扶正成正典。
//!
//! 为什么拆出来：旧的实现把这三件事全塞进 `OraclePanel`（"预言书"），
//! 结果"世界线"这个词在侧栏出现两次、而真正的世界线没有自己的页面。

use std::cell::RefCell;
use std::collections::BTreeMap;
use std::rc::Rc;

use gpui_kit::component::button::Button;
use gpui_kit::component::dock::{BasePanel, Panel as ComponentPanel, PanelEvent};
use gpui_kit::component::Sizable;
use gpui_kit::prelude::FluentBuilder;
use gpui_kit::{
    App, AppContext as _, Context, CursorStyle, Entity, EventEmitter, FocusHandle, Focusable,
    FontWeight,
    InteractiveElement, IntoElement, MouseButton, MouseDownEvent, MouseMoveEvent, MouseUpEvent,
    ParentElement, PathBuilder, Render, ScrollDelta, ScrollEvent, SharedString,
    StatefulInteractiveElement, Styled, Window, canvas, div, point, px, relative,
};

use crate::store::ProjectHandle;

/// Rgba → Hsla（画布路径与 Tag 需要 Hsla）。
fn hsla(c: gpui_kit::Rgba) -> gpui_kit::Hsla {
    c.into()
}
use crate::theme;
use crate::worldline::layout::{
    bands, fan_beat_frac, fan_end_frac, forks_at, ruler_step, segments, tracks, Band, Side, Track,
};
use crate::worldline::{Kind, Line, Store};

/// 世界线的三种看法。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WlTab {
    Timeline,
    Compare,
}

/// 时间轴上点开的对象。`Some` = 抽屉打开。
///
/// 两种来源共用一个抽屉外壳，但内容不同：
/// - `Node`：某条世界线上的节点（可改章、采纳、推演、删、关联画布）。
/// - `Event`：主干（正典）上某一章的事件台账。正典在数据里没有节点，
///   它的"关节"来自 events.json——点主干上的点要看的是"这一章发生了什么"。
#[derive(Debug, Clone, PartialEq)]
enum Pick {
    Node { line: String, node: String },
    Event { chapter: u32 },
}

/// 世界线页的事件。写操作全部交给宿主，页面不碰磁盘。
#[derive(Debug, Clone)]
pub enum WorldlineEvent {
    /// 选中某条线（联动右侧详情与预言书）
    Select(String),
    /// 从某条线的第 N 章分叉
    Fork { from: String, chapter: u32 },
    /// 归档 / 恢复
    Archive(String),
    /// 删除
    Remove(String),
    /// 改一个节点的字段（章号 / 标题 / 状态）
    NodePatch {
        line: String,
        node: String,
        patch: crate::worldline::NodePatch,
    },
    /// 删一个节点
    NodeRemove { line: String, node: String },
    /// 逐节点采纳：把这个节点定为正典后续（产出提案，落库仍过收件箱）
    NodeAdopt { line: String, node: String },
    /// 派活给某个专属 Agent（能力表驱动）
    Delegate {
        agent: &'static str,
        key: &'static str,
        target: Option<String>,
    },
    /// 改线的名字 / 假设
    UpdateMeta {
        line: String,
        name: Option<String>,
        premise: Option<String>,
    },
    /// 加一条边
    AddEdge { line: String, from: String, to: String, kind: String },
    /// 删一条边
    RemoveEdge { line: String, from: String, to: String, kind: String },
    /// 切换边的暗线标记
    ToggleEdgeHidden { line: String, from: String, to: String, kind: String },
    /// 把世界线节点挂到画布节点（双向弱引用的一半）
    LinkCanvas {
        line: String,
        node: String,
        canvas: String,
    },
    /// 作者声明「已定剧情到第 N 章」（正文还没写到时的显式确认；0 = 清掉）
    ConfirmThrough(u32),
    /// 跳到章节排期板看这一章（导演台↔世界线共用章号坐标）
    ViewInDirector(u32),
    /// 新增一个节点
    NodeAdd {
        line: String,
        kind: String,
        label: String,
        chapter: u32,
    },
}

pub struct WorldlinePanel {
    focus_handle: FocusHandle,
    handle: Rc<ProjectHandle>,
    selected: Option<String>,
    /// 时间轴的可见章节范围（None = 自动：0 到 max(边界, 最后节点) + 余量）。
    ///
    /// 有它是因为"固定像素每章"在长书里会挤成一团：写到 300 章时，相邻两章
    /// 只差 2px，节点全叠在一起。`window` 就是缩放（拉开跨度）与平移
    /// （挪范围）的载体——它不是另存一份数据，只是同一个时间轴的取景框。
    window: Option<(u32, u32)>,
    message: Option<SharedString>,
    /// 世界线的三种看法。它们看的是同一份 worldlines.json，只是镜头不同：
    /// - 时间轴：横轴=章节，固化段实线/开放段虚线（回答"确定到哪、之后有什么"）
    /// - 对比：两条线共同祖先之后差在哪
    view: WlTab,
    /// 对比模式的右线
    compare_with: Option<String>,
    /// 改名 / 改假设的输入框
    ed_name: Entity<gpui_kit::component::input::InputState>,
    ed_premise: Entity<gpui_kit::component::input::InputState>,
    /// 「已定剧情到第几章」的输入框（正文没写到时，作者手动定边界）
    ed_confirmed: Entity<gpui_kit::component::input::InputState>,
    /// 折叠的轨道（作者线多了要收起来）
    collapsed: std::collections::BTreeSet<String>,
    /// 时间轴上点开的对象（节点或主干事件）。`Some` = 抽屉打开。
    ///
    /// 为什么是"点节点开抽屉"而不是把节点全塞进右侧详情：右侧详情是**这条线**
    /// 的信息（名字、假设、归档），节点是**线上的一个点**。两者混在一起，
    /// 作者点一个点就会把整条线的操作挤掉。抽屉只回答"这个点是什么、能对它做什么"。
    picked: Option<Pick>,
    /// 每帧算出的默认取景框。缩放/平移/定位都以它为起步——
    /// 否则第一次按＋会从"边界前一段"猛地跳到全书 0..horizon，作者会以为图坏了。
    default_window: (u32, u32),
    /// 时间轴拖拽平移：上一次指针 x、累计未换算成章的像素、是否已拖动。
    drag: Option<(f32, f32, bool)>,
    /// 滚轮缩放累计：触控板是连续小增量，一事件一缩放会飞。
    scroll_acc: f32,
    /// 当前每根线的时间轴纵向比例（render 时写入，点击选线用）。
    track_fracs: Rc<RefCell<Vec<(String, f32)>>>,
    /// 时间轴画布在窗口里的 bounds（prepaint 每帧写回）。
    stage_bounds: Rc<RefCell<Option<(f32, f32, f32, f32)>>>,
}

impl WorldlinePanel {
    pub fn new(window: &mut Window, cx: &mut Context<Self>, handle: Rc<ProjectHandle>) -> Self {
        let ed_name = cx.new(|cx| {
            gpui_kit::component::input::InputState::new(window, cx).placeholder("线名")
        });
        let ed_premise = cx.new(|cx| {
            gpui_kit::component::input::InputState::new(window, cx)
                .placeholder("这个 if 说的是什么（一句话）")
        });
        let confirmed_now = Store::new(&handle.root).confirmed_through();
        let ed_confirmed = cx.new(|cx| {
            gpui_kit::component::input::InputState::new(window, cx)
                .placeholder("章号")
                .default_value(if confirmed_now == 0 {
                    String::new()
                } else {
                    confirmed_now.to_string()
                })
        });
        Self {
            focus_handle: cx.focus_handle(),
            handle,
            selected: None,
            window: None,
            ed_name,
            ed_premise,
            ed_confirmed,
            collapsed: Default::default(),
            picked: None,
            default_window: (0, 0),
            drag: None,
            scroll_acc: 0.,
            track_fracs: Rc::new(RefCell::new(Vec::new())),
            stage_bounds: Rc::new(RefCell::new(None)),
            message: None,
            view: match std::env::var("ZHIBI_WL_TAB").as_deref() {
                Ok("compare") => WlTab::Compare,
                _ => WlTab::Timeline,
            },
            compare_with: None,
        }
    }

    /// 切人时把名字/假设灌进输入框。
    fn fill_meta(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let line = self
            .selected
            .as_ref()
            .and_then(|id| self.lines().into_iter().find(|l| &l.id == id));
        let (name, premise) = match line {
            Some(l) => (l.name, l.premise),
            None => (String::new(), String::new()),
        };
        self.ed_name
            .update(cx, |st, cx| st.set_value(&name, window, cx));
        self.ed_premise
            .update(cx, |st, cx| st.set_value(&premise, window, cx));
    }

    pub fn set_message(&mut self, msg: impl Into<SharedString>, cx: &mut Context<Self>) {
        self.message = Some(msg.into());
        cx.notify();
    }

    /// 从外部指定一个要看的世界线（预言书"看时间轴"、创建新线后定位）。
    ///
    /// 除了选中，还会把视野挪到这条线的分叉处。被叫过来看的作者打开页面时
    /// 若停在默认全览，一条在 300 章处的线只是一个点——等于没看见。
    pub fn focus_line(&mut self, id: String, cx: &mut Context<Self>) {
        let lines = self.lines();
        self.prune_picked(&lines);
        let store = Store::new(&self.handle.root);
        let boundary = store.fixed_boundary(&lines);
        let written = self.written_through();
        let full = self.horizon(&lines, boundary, written);
        if let Some(line) = lines.iter().find(|l| l.id == id) {
            self.focus_chapter(line.fork_chapter().max(1), full);
        }
        self.selected = Some(id);
        cx.notify();
    }

    /// 抽屉指向的对象还在不在。不在就清掉——否则删完节点再重新渲染，
    /// 抽屉里那块区域会一直是空的（`and_then` 返回 None，什么都不画，
    /// 但状态的 picked 还挂着，点别的点时会"先收起再展开"多一次闪）。
    fn prune_picked(&mut self, lines: &[Line]) {
        let alive = match &self.picked {
            None => true,
            Some(Pick::Node { line, node }) => lines
                .iter()
                .find(|l| &l.id == line)
                .map(|l| l.nodes.iter().any(|n| &n.id == node))
                .unwrap_or(false),
            Some(Pick::Event { chapter }) => self.handle.events().iter().any(|e| {
                let ch = e
                    .get("actualChapter")
                    .and_then(|x| x.as_u64())
                    .filter(|n| *n > 0)
                    .or_else(|| e.get("chapter").and_then(|x| x.as_u64()))
                    .unwrap_or(0) as u32;
                ch == *chapter
            }),
        };
        if !alive {
            self.picked = None;
        }
    }

    /// 读全部线，坏文件当空（界面不能因为一份坏 JSON 整个白屏）。
    fn lines(&self) -> Vec<Line> {
        Store::new(&self.handle.root).all().unwrap_or_default()
    }

    fn stage_bands(&self) -> Vec<Band> {
        let raw: Vec<(String, String, u32, u32)> = self
            .handle
            .stages()
            .iter()
            .map(|st| {
                (
                    s(st, "name"),
                    s(st, "theme"),
                    st.get("chapterStart").and_then(|v| v.as_u64()).unwrap_or(0) as u32,
                    st.get("chapterEnd").and_then(|v| v.as_u64()).unwrap_or(0) as u32,
                )
            })
            .collect();
        bands(&raw)
    }

    /// 全书的章节跨度（自动取景时用）。
    /// 视野右端：主干边界与扇形最远预演取大，再留余量。
    ///
    /// 扇形最小前瞻（layout::fan 里的 LOOKAHEAD）会超过所有节点的章号，
    /// 这里必须把它算进来，否则扇子右端会被视野裁掉。
    fn horizon(&self, lines: &[Line], boundary: u32, written: u32) -> u32 {
        // 未来区宽 = 主干的 40%（至少 8 章）——预演不占章号，
        // 这段只提供横向空间，刻度上不再标章号。
        let future = ((boundary / 2).max(8)) as u32;
        lines
            .iter()
            .map(|l| l.last_chapter())
            .max()
            .unwrap_or(0)
            .max(boundary)
            .max(written)
            .max(boundary + future)
    }

    /// 缩放（算术在 `layout::zoom_window`，那里有测试）。
    /// 起步用当前取景框；没有就用默认框，避免第一次缩放从全览猛跳。
    fn zoom(&mut self, factor: f32, full: u32) {
        let cur = self.window.or(Some(self.default_window));
        self.window = Some(crate::worldline::layout::zoom_window(cur, factor, full));
    }

    /// 平移（算术在 `layout::pan_window`）。
    fn pan(&mut self, chapters: i32, full: u32) {
        let cur = self.window.or(Some(self.default_window));
        self.window = Some(crate::worldline::layout::pan_window(cur, chapters, full));
    }

    /// 取景框是否已偏离默认取景（决定要不要显示「全部」按钮）。
    fn is_zoomed(&self, _full: u32) -> bool {
        match self.window {
            Some(w) => w != self.default_window,
            None => false,
        }
    }

    /// 把视野挪到某一章（点节点 / 新建线后自动跟过去）。
    fn focus_chapter(&mut self, ch: u32, full: u32) {
        let cur = self.window.or(Some(self.default_window));
        self.window = Some(crate::worldline::layout::focus_window(cur, ch, full));
    }

    /// 默认取景框：边界前 14 章正文做参照，右端到最远那条预演再留余量。
    ///
    /// 抽出来是因为 render 与跨页跳转都要用它。跳过来时面板可能**还没渲染过**
    /// （`default_window` 仍是 `(0,0)`），直接拿它算跨度会把视野压成 1 章，
    /// 跳过去只看到一根竖线——那正是"点了没反应"的样子。
    ///
    /// 不取 `&self`：它是纯函数（只依赖 lines 与 boundary），这样能直接测，
    /// 也能被跳转路径在面板未渲染时调用。
    fn auto_window(lines: &[Line], boundary: u32) -> (u32, u32) {
        let back = boundary.saturating_sub(14);
        // 用 fan 的 horizon（含推演章序），不是 lines 的 last_chapter——
        // 后者的节点没有章号会算成 0，右端就退回 boundary+6。
        let fan_h = crate::worldline::layout::fan(lines, boundary).horizon;
        (back, fan_h.max(boundary + 6))
    }

    /// 从别的页（排期板）跳过来看某一章：切页后把取景框挪到该章。
    pub fn goto_chapter(&mut self, ch: u32, cx: &mut Context<Self>) {
        let lines = self.lines();
        let store = Store::new(&self.handle.root);
        let boundary = store.fixed_boundary(&lines);
        let written = self.written_through();
        let full = self.horizon(&lines, boundary, written);
        let base = if self.default_window == (0, 0) {
            Self::auto_window(&lines, boundary)
        } else {
            self.default_window
        };
        self.window = Some(crate::worldline::layout::focus_window(
            self.window.or(Some(base)),
            ch.max(1),
            full,
        ));
        cx.notify();
    }

    /// 最新成稿章：作者"已经写到哪"。
    fn written_through(&self) -> u32 {
        self.handle
            .chapters()
            .iter()
            .filter(|c| c.has_prose)
            .map(|c| c.n)
            .max()
            .unwrap_or(0)
    }

    /// 底部抽屉：点时间轴上的一个点（世界线节点或主干事件）浮出，
    /// 回答"这个点是什么、能对它做什么"。再点同一个点 / 点 × 收起。
    ///
    /// 为什么独立于右侧详情：详情是**整条线**的操作面板，抽屉是**一个点**的。
    /// 点一个节点不该把整条线的名字/假设/归档挤掉。
    fn drawer(
        &self,
        lines: &[Line],
        boundary: u32,
        cx: &mut Context<Self>,
    ) -> Option<gpui_kit::AnyElement> {
        let pick = self.picked.clone()?;
        let inner = match &pick {
            Pick::Node { line, node } => self.drawer_node(lines, boundary, line, node, cx)?,
            Pick::Event { chapter } => self.drawer_event(*chapter, cx),
        };
        Some(
            div()
                .id("wl-drawer")
                .absolute()
                .left(px(14.))
                .right(px(14.))
                .bottom(px(44.))
                .max_h(relative(0.44))
                .overflow_y_scroll()
                .p(theme::PAD_MD)
                .rounded(theme::RADIUS_PANEL)
                .border_1()
                .border_color(theme::ACCENT_BORDER())
                .bg(theme::PANEL())
                .child(inner)
                .into_any_element(),
        )
    }

    /// 节点抽屉的内容（世界线节点：可改章、采纳、推演、关联、删）。
    fn drawer_node(
        &self,
        lines: &[Line],
        boundary: u32,
        line: &str,
        node: &str,
        cx: &mut Context<Self>,
    ) -> Option<gpui_kit::AnyElement> {
        let l = lines.iter().find(|l| l.id == line)?;
        let n = l.nodes.iter().find(|n| n.id == node)?;
        // 章号与图上同一口径：没排章的显示推演序（演N），排过章的显示真章号。
        let beats = crate::worldline::layout::beats_for(l, boundary);
        let (ch, sched) = beats
            .iter()
            .find(|b| b.id == node)
            .map(|b| (b.chapter, b.scheduled))
            .unwrap_or((n.chapter_or_guess(), n.chapter_or_guess() > 0));
        let state = n.state;
        let state_color = match state {
            crate::worldline::NodeState::Written => theme::GREEN(),
            crate::worldline::NodeState::Adopted => theme::BLUE(),
            _ => theme::PURPLE(),
        };
        let ch_text = if !sched {
            format!("演{ch} · 推演序")
        } else if ch > 0 {
            format!("第{ch}章")
        } else {
            "未排章".to_string()
        };

        let mut col = div().flex().flex_col().gap(px(5.));
        // 头：线号 + 标题 + 状态 + 章号 + 关闭
        col = col.child(
            div()
                .flex()
                .items_center()
                .gap(theme::GAP_SM)
                .child(
                    div()
                        .flex_shrink_0()
                        .font_family(theme::FAMILY_MONO)
                        .text_size(theme::FONT_MICRO)
                        .text_color(theme::ACCENT())
                        .child(SharedString::from(l.id.clone())),
                )
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .text_ellipsis()
                        .text_size(theme::FONT_LABEL)
                        .font_weight(FontWeight::SEMIBOLD)
                        .text_color(theme::TEXT())
                        .child(SharedString::from(n.label.clone())),
                )
                .child(
                    div()
                        .flex_shrink_0()
                        .text_size(px(9.))
                        .text_color(state_color)
                        .child(SharedString::from(state.label().to_string())),
                )
                .child(
                    div()
                        .flex_shrink_0()
                        .font_family(theme::FAMILY_MONO)
                        .text_size(theme::FONT_MICRO)
                        .text_color(if sched { theme::FAINT() } else { theme::PURPLE() })
                        .child(SharedString::from(ch_text)),
                )
                .child(
                    div()
                        .id("wl-drawer-x")
                        .flex_shrink_0()
                        .cursor_pointer()
                        .px(px(3.))
                        .text_size(theme::FONT_LABEL)
                        .text_color(theme::FAINT())
                        .child("×")
                        .on_click(cx.listener(|this, _, _, cx| {
                            this.picked = None;
                            cx.notify();
                        })),
                ),
        );
        // 备注
        if !n.note.trim().is_empty() {
            col = col.child(
                div()
                    .text_size(theme::FONT_MICRO)
                    .line_height(relative(1.4))
                    .text_color(theme::DIM())
                    .child(SharedString::from(format!("备注：{}", n.note))),
            );
        }
        // 因果链两向：← 上游（指向它的）、→ 下游（它指出的），含明/暗。
        let upstream: Vec<_> = l.edges.iter().filter(|e| e.to == node).collect();
        let downstream: Vec<_> = l.edges.iter().filter(|e| e.from == node).collect();
        if !upstream.is_empty() || !downstream.is_empty() {
            let mut chain = div().flex().flex_col().gap(px(1.));
            for e in upstream.iter() {
                chain = chain.child(edge_chain_row("←", &e.from, &e.kind, e.hidden));
            }
            for e in downstream.iter() {
                chain = chain.child(edge_chain_row("→", &e.to, &e.kind, e.hidden));
            }
            col = col.child(
                div()
                    .flex()
                    .flex_col()
                    .gap(px(2.))
                    .pt(px(2.))
                    .child(
                        div()
                            .text_size(px(9.))
                            .text_color(theme::FAINT())
                            .child("因果链"),
                    )
                    .child(chain),
            );
        }
        // 操作行
        let lid = l.id.clone();
        let nid = n.id.clone();
        let mut bar = div().flex().flex_wrap().items_center().gap(theme::GAP_SM).pt(px(2.));
        // 章号＋1（推演序则落实为这一章）
        {
            let (l1, n1) = (lid.clone(), nid.clone());
            let next_ch = if sched { ch + 1 } else { ch };
            let label = if sched {
                "章号＋1".to_string()
            } else {
                format!("落实为第{ch}章")
            };
            bar = bar.child(
                Button::new(SharedString::from(format!("wld-ch-{lid}-{nid}")))
                    .xsmall()
                    .label(SharedString::from(label))
                    .on_click(cx.listener(move |_t, _, _, cx| {
                        cx.emit(WorldlineEvent::NodePatch {
                            line: l1.clone(),
                            node: n1.clone(),
                            patch: crate::worldline::NodePatch::chapter(next_ch),
                        });
                    })),
            );
        }
        // 采纳进正典：仅未定稿且已排章。没排章的给提示，不静默。
        if !state.is_canon() {
            if sched {
                let (l2, n2) = (lid.clone(), nid.clone());
                bar = bar.child(
                    Button::new(SharedString::from(format!("wld-adopt-{lid}-{nid}")))
                        .xsmall()
                        .label("采纳进正典")
                        .on_click(cx.listener(move |_t, _, _, cx| {
                            cx.emit(WorldlineEvent::NodeAdopt {
                                line: l2.clone(),
                                node: n2.clone(),
                            });
                        })),
                );
            } else {
                bar = bar.child(
                    div()
                        .text_size(px(9.))
                        .text_color(theme::AMBER())
                        .child("先给它定第几章，才能采纳"),
                );
            }
        }
        // 从这儿推演
        {
            let ndesc = format!("世界线 {}「{}」的第{}章节点「{}」", l.id, l.name, ch, n.label);
            bar = bar.child(
                Button::new(SharedString::from(format!("wld-sim-{lid}-{nid}")))
                    .xsmall()
                    .label("从这儿推演")
                    .on_click(cx.listener(move |_t, _, _, cx| {
                        cx.emit(WorldlineEvent::Delegate {
                            agent: "worldline",
                            key: "wl-from-node",
                            target: Some(ndesc.clone()),
                        });
                    })),
            );
        }
        // 关联画布（双向弱引用的一半）
        {
            let (l3, n3) = (lid.clone(), nid.clone());
            let cur = n.canvas_ref.clone();
            let label = if cur.is_empty() { "关联画布" } else { "解除关联" };
            bar = bar.child(
                Button::new(SharedString::from(format!("wld-link-{lid}-{nid}")))
                    .xsmall()
                    .label(label)
                    .on_click(cx.listener(move |_t, _, _, cx| {
                        cx.emit(WorldlineEvent::LinkCanvas {
                            line: l3.clone(),
                            node: n3.clone(),
                            canvas: if cur.is_empty() { n3.clone() } else { String::new() },
                        });
                    })),
            );
        }
        // 排期板看此章：导演台↔世界线共用章号坐标，跳过去看这章排了什么。
        if ch > 0 {
            bar = bar.child(
                Button::new(SharedString::from(format!("wld-dir-{lid}-{nid}")))
                    .xsmall()
                    .label("排期板看此章")
                    .on_click(cx.listener(move |_t, _, _, cx| {
                        cx.emit(WorldlineEvent::ViewInDirector(ch));
                    })),
            );
        }
        // 删节点
        {
            let (l4, n4) = (lid.clone(), nid.clone());
            bar = bar.child(
                Button::new(SharedString::from(format!("wld-del-{lid}-{nid}")))
                    .xsmall()
                    .label("删节点")
                    .on_click(cx.listener(move |_t, _, _, cx| {
                        cx.emit(WorldlineEvent::NodeRemove {
                            line: l4.clone(),
                            node: n4.clone(),
                        });
                    })),
            );
        }
        col = col.child(bar);
        Some(col.into_any_element())
    }

    /// 事件抽屉的内容（正典主干上的点：这一章发生了什么，来自事件台账）。
    fn drawer_event(&self, chapter: u32, cx: &mut Context<Self>) -> gpui_kit::AnyElement {
        let evs: Vec<_> = self
            .handle
            .events()
            .iter()
            .filter(|e| !e.get("dropped").and_then(|x| x.as_bool()).unwrap_or(false))
            .filter(|e| {
                let ch = e
                    .get("actualChapter")
                    .and_then(|x| x.as_u64())
                    .filter(|n| *n > 0)
                    .or_else(|| e.get("chapter").and_then(|x| x.as_u64()))
                    .unwrap_or(0) as u32;
                ch == chapter
            })
            .cloned()
            .collect();
        let mut col = div().flex().flex_col().gap(px(5.));
        col = col.child(
            div()
                .flex()
                .items_center()
                .gap(theme::GAP_SM)
                .child(
                    div()
                        .flex_shrink_0()
                        .font_family(theme::FAMILY_MONO)
                        .text_size(theme::FONT_MICRO)
                        .text_color(theme::BLUE())
                        .child("α 正典"),
                )
                .child(
                    div()
                        .flex_1()
                        .text_size(theme::FONT_LABEL)
                        .font_weight(FontWeight::SEMIBOLD)
                        .text_color(theme::TEXT())
                        .child(SharedString::from(format!("第{chapter}章 · {} 件事", evs.len()))),
                )
                .child(
                    div()
                        .id("wl-drawer-ev-x")
                        .cursor_pointer()
                        .px(px(3.))
                        .text_size(theme::FONT_LABEL)
                        .text_color(theme::FAINT())
                        .child("×")
                        .on_click(cx.listener(|this, _, _, cx| {
                            this.picked = None;
                            cx.notify();
                        })),
                ),
        );
        if evs.is_empty() {
            col = col.child(
                div()
                    .text_size(theme::FONT_MICRO)
                    .text_color(theme::FAINT())
                    .child("这一章还没有事件入账。"),
            );
        }
        for e in evs.iter() {
            let happened = e.get("happened").and_then(|x| x.as_bool()).unwrap_or(false);
            let revealed = e.get("revealed").and_then(|x| x.as_bool()).unwrap_or(false);
            let fact = s(e, "objectiveFact");
            let sid = s(e, "id");
            col = col.child(
                div()
                    .flex()
                    .items_center()
                    .gap(theme::GAP_SM)
                    .py(px(1.))
                    .child(
                        div()
                            .flex_shrink_0()
                            .font_family(theme::FAMILY_MONO)
                            .text_size(px(9.))
                            .text_color(theme::FAINT())
                            .child(SharedString::from(sid)),
                    )
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .text_ellipsis()
                            .text_size(theme::FONT_MICRO)
                            .text_color(theme::TEXT())
                            .child(SharedString::from(if fact.is_empty() { "（无描述）".into() } else { fact })),
                    )
                    .child(
                        div()
                            .flex_shrink_0()
                            .text_size(px(9.))
                            .text_color(if happened { theme::GREEN() } else { theme::AMBER() })
                            .child(if happened { "已发生" } else { "计划" }),
                    )
                    .child(
                        div()
                            .flex_shrink_0()
                            .text_size(px(9.))
                            .text_color(if revealed { theme::BLUE() } else { theme::PURPLE() })
                            .child(if revealed { "读者已知" } else { "未揭示" }),
                    ),
            );
        }
        col = col.child(
            div()
                .pt(px(2.))
                .text_size(px(9.))
                .text_color(theme::FAINT())
                .child("正典的关节来自事件台账；要改它，去大纲 / 章节排期页。"),
        );
        col.into_any_element()
    }
}

/// 因果链的一行：方向箭头 + 对方节点 + 类型 + 明/暗。
fn edge_chain_row(arrow: &str, other: &str, kind: &str, hidden: bool) -> gpui_kit::AnyElement {
    div()
        .flex()
        .items_center()
        .gap(px(4.))
        .text_size(px(9.))
        .child(
            div()
                .flex_shrink_0()
                .text_color(theme::FAINT())
                .child(SharedString::from(arrow.to_string())),
        )
        .child(
            div()
                .flex_1()
                .min_w_0()
                .text_ellipsis()
                .text_color(if hidden { theme::PURPLE() } else { theme::DIM() })
                .child(SharedString::from(other.to_string())),
        )
        .child(
            div()
                .flex_shrink_0()
                .text_color(theme::FAINT())
                .child(SharedString::from(if kind.is_empty() { "influence".into() } else { kind.to_string() })),
        )
        .child(
            div()
                .flex_shrink_0()
                .text_color(if hidden { theme::PURPLE() } else { theme::FAINT() })
                .child(if hidden { "暗" } else { "明" }),
        )
        .into_any_element()
}

fn s(v: &serde_json::Value, k: &str) -> String {
    v.get(k).and_then(|x| x.as_str()).unwrap_or("").to_string()
}

impl EventEmitter<PanelEvent> for WorldlinePanel {}
impl EventEmitter<WorldlineEvent> for WorldlinePanel {}

impl Focusable for WorldlinePanel {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus_handle.clone()
    }
}

impl Render for WorldlinePanel {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let lines = self.lines();
        // 抽屉跟着数据走：对象被删/线被归档后，每帧检查一次，不在就收起。
        self.prune_picked(&lines);
        let store = Store::new(&self.handle.root);
        let boundary = store.fixed_boundary(&lines);
        let written = self.written_through();
        // 操作反馈（分叉/删除/采纳/加边…的结果）。宿主每次写操作都 set_message，
        // 这里必须渲染出来——早先只存不画，作者点了按钮看不到任何回应。
        let msg = self.message.clone();

        // 视野：从 0 到「边界与最远节点取大」再留 6 章余量——作者要看见"前面还有路"。
        // 缩放/平移都是在这个默认范围上取一个子区间（self.window）。
        // 视野要同时容下"主干画到边界"和"扇形推到最远的那条预演"。
        let horizon = self.horizon(&lines, boundary, written);
        // 默认取景：不是从第 0 章铺到最远，而是"边界前一段 + 推演那几章"。
        //
        // 全量铺开时 45 章压进 900px = 每章 20px，预测的 5 个节点
        //（第 31–35 章）挤在 100px 里，糊成一团；作者得先缩放才能看清，
        // 这就违背了"辅助而不是让人多操作"。默认直接给到读得清的比例。
        let default_window = Self::auto_window(&lines, boundary);
        self.default_window = default_window;
        let (from_ch, to_ch) = self.window.unwrap_or(default_window);
        let span = (to_ch.saturating_sub(from_ch)).max(1);

        // 横向比例：主干按真实章号线性；**推演区按固定比例放大**。
        //
        // 预测通常只往前推 4–6 章，线性映射下那几章只占 100px，
        // 五个关节点糊在一起、标签也压成一团。这里把"边界之后"那一段
        // 单独拉宽——主干的比例尺不变（刻度照样对得上正文），
        // 只是未来那一段看得清。图例里写明"推演区放大"，不骗人。
        let future_span = (to_ch.saturating_sub(boundary)).max(1) as f32;
        let pivot_frac = ((boundary.saturating_sub(from_ch)) as f32 / span as f32).clamp(0., 1.);
        let x_frac = move |ch: u32| -> f32 {
            if boundary == 0 || ch <= boundary {
                ((ch.saturating_sub(from_ch)) as f32 / span as f32).clamp(0., 1.)
            } else {
                (pivot_frac + (ch - boundary) as f32 / future_span * (1. - pivot_frac))
                    .clamp(0., 1.)
            }
        };

        let mut tr = tracks(&lines, boundary);
        // 世界线只画「已经固化的一条正典」+「从固化点延伸的未来预演」。
        // 假设线属于预言书，不能混进时间轴冒充主线。
        tr.retain(|t| matches!(t.kind, Kind::Base | Kind::Forecast));
        let mut segs = segments(&tr, to_ch);
        // 折叠：收起的轨道只留固化段（开放段是"这条线还想干什么"，收起即不想看）
        segs.retain(|sg| !self.collapsed.contains(&sg.track) || sg.fixed);
        let band_list = self.stage_bands();

        // —— 左：分组列表 ——
        //
        // 读法跟着画布走：上面「已固化」永远只有一条（正文），
        // 下面「未来预演」是从现在扇出去的那几条。分组本身就在说明
        // "哪些是事实、哪些是候选"。
        let mut track_rows: Vec<gpui_kit::AnyElement> = Vec::new();
        track_rows.push(
            div()
                .w_full()
                .pt(theme::GAP_XS)
                .pb(px(2.))
                .px(px(2.))
                .text_size(px(9.))
                .font_weight(FontWeight::SEMIBOLD)
                .text_color(theme::BLUE())
                .child("已固化 · 正文写到哪儿就是哪儿")
                .into_any_element(),
        );
        let mut fan_head_done = false;
        for t in &tr {
            if !(t.kind == Kind::Base || t.id == "α") && !fan_head_done {
                fan_head_done = true;
                track_rows.push(
                    div()
                        .w_full()
                        .pt(theme::GAP_MD)
                        .pb(px(2.))
                        .px(px(2.))
                        .text_size(px(9.))
                        .font_weight(FontWeight::SEMIBOLD)
                        .text_color(theme::PURPLE())
                        .child("未来预演 · 从现在散开的几种可能")
                        .into_any_element(),
                );
            }
            let on = self.selected.as_deref() == Some(t.id.as_str());
            let color = track_color(t);
            let id = t.id.clone();
            let pick = t.id.clone();
            track_rows.push(
                div()
                    .id(SharedString::from(format!("wltr-{}", t.id)))
                    .h(px(24.))
                    .flex()
                    .items_center()
                    .gap(px(4.))
                    .cursor_pointer()
                    .when(on, |d| d.bg(theme::ACCENT_SOFT()))
                    .on_click(cx.listener(move |this, _, window, cx| {
                        this.selected = Some(pick.clone());
                        cx.emit(WorldlineEvent::Select(pick.clone()));
                        this.fill_meta(window, cx);
                        cx.notify();
                    }))
                    .child(
                        div()
                            .w(px(28.))
                            .flex_shrink_0()
                            .font_family(theme::FAMILY_MONO)
                            .text_size(theme::FONT_MICRO)
                            .text_color(color)
                            .child(SharedString::from(id)),
                    )
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .text_ellipsis()
                            .text_size(theme::FONT_MICRO)
                            .text_color(if on { theme::TEXT() } else { theme::DIM() })
                            .child(SharedString::from(t.name.clone())),
                    )
                    .child(
                        div()
                            .text_size(px(9.))
                            .text_color(theme::FAINT())
                            .child(t.kind.label()),
                    )
                    .child({
                        // 折叠：线多了要能收起来。收起后时间轴上只剩它的固化段。
                        let cid = t.id.clone();
                        let folded = self.collapsed.contains(&cid);
                        div()
                            .id(SharedString::from(format!("wlfold-{}", t.id)))
                            .flex_shrink_0()
                            .px(px(2.))
                            .cursor_pointer()
                            .text_size(px(9.))
                            .text_color(theme::FAINT())
                            .child(if folded { "▸" } else { "▾" })
                            .on_click(cx.listener(move |this, _, _, cx| {
                                if this.collapsed.contains(&cid) {
                                    this.collapsed.remove(&cid);
                                } else {
                                    this.collapsed.insert(cid.clone());
                                }
                                cx.notify();
                            }))
                    })
                    .into_any_element(),
            );
        }

        // —— 右：时间轴画布 ——
        let bands_c = band_list.clone();
        // 节点带上完整状态：时间轴上要区分「已写」（绿实心）、「已采纳但正文还没写到」
        // （蓝实心）、「仅计划」（空心）。只传一个 bool 会把前两者混成一样。
        // —— 版式：一条已固化的直线 + 从"现在"扇出的多种未来 ——
        //
        // 作者要的读法很明确：左边是一根**不分支的直线**（正文写了什么就是什么），
        // 右边从最新那个点开始，向上下散开若干条**未来可能性**，每条一条推演，
        // 给作者参考。所以这里不再用"多条平行轨道"，而是一把扇子。
        //
        // 扇形越往外，线走得越远（layout::fan 按 end_chapter 排序分配角度），
        // 这样"推得远的路摆在外面"是稳定可复现的，不会每次重绘换位置。
        let fan = crate::worldline::layout::fan(&lines, boundary);
        // 每章的事件数：主干的粗细用它，所以先按章数好。
        // 用 actualChapter（真发生了）优先，没发生才用 plan 的 chapter。
        let dens: Vec<u32> = {
            let mut v = vec![0u32; 4096];
            for e in self.handle.events().iter() {
                if e.get("dropped").and_then(|x| x.as_bool()).unwrap_or(false) {
                    continue;
                }
                let ch = e
                    .get("actualChapter")
                    .and_then(|x| x.as_u64())
                    .filter(|n| *n > 0)
                    .or_else(|| e.get("chapter").and_then(|x| x.as_u64()))
                    .unwrap_or(0) as usize;
                if ch > 0 && ch < v.len() {
                    v[ch] += 1;
                }
            }
            v
        };
        let dens_c = dens.clone();
        // 轴上的事件点：(章号, 是否已发生)。正典线在数据里没有节点，
        // 主干要有点只能来自事件台账——那才是最真实的"关节"。
        let ev_dots: Vec<(u32, bool)> = self
            .handle
            .events()
            .iter()
            .filter(|e| !e.get("dropped").and_then(|x| x.as_bool()).unwrap_or(false))
            .map(|e| {
                let happened = e.get("happened").and_then(|x| x.as_bool()).unwrap_or(false);
                let ch = e
                    .get("actualChapter")
                    .and_then(|x| x.as_u64())
                    .filter(|n| *n > 0)
                    .or_else(|| e.get("chapter").and_then(|x| x.as_u64()))
                    .unwrap_or(0) as u32;
                (ch, happened)
            })
            .filter(|(ch, _)| *ch > 0)
            .collect();
        let ev_c = ev_dots.clone();

        // "现在"欠着几笔账：未兑现的伏笔。选路之前先看这个——
        // 一条未来好不好，很大程度取决于它能不能把这些收掉。
        let open_clues = self
            .handle
            .clues_full()
            .iter()
            .filter(|c| {
                let st = c.get("status").and_then(|v| v.as_str()).unwrap_or("");
                !st.contains("兑现") && !st.contains("废弃")
            })
            .count();

        // 枢轴 = 固化边界：左边是定下来的直线，右边是还要挑的未来。
        let fan_c = fan.clone();
        // DOM 层（束名标签）也要用扇束的终点，paint 闭包会把 fan_c move 走。
        let fan_lbl = fan.branches.clone();
        // 每束的纵向位置（相对主干），命中测试与绘制共用同一份。
        // 扇子半高：占画布高度的 38%，留出上下标题与图例。
        let half_h_frac = 0.38f32;
        // 点击选线用的纵向比例：必须与 paint/标签同一口径（fan_end_frac），
        // 早先这里全填 0.5，导致点束线永远选不中（只有正中那条碰巧命中）。
        *self.track_fracs.borrow_mut() = fan
            .branches
            .iter()
            .map(|b| (b.id.clone(), fan_end_frac(b.angle, half_h_frac)))
            .collect();

        let nodes_by_track: BTreeMap<String, Vec<(u32, String, String, crate::worldline::NodeState)>> =
            lines
                .iter()
                .map(|l| {
                    let mut ns: Vec<(u32, String, String, crate::worldline::NodeState)> = l
                        .nodes
                        .iter()
                        .map(|n| {
                            (
                                n.chapter_or_guess(),
                                n.id.clone(),
                                n.label.clone(),
                                n.state,
                            )
                        })
                        .filter(|(ch, _, _, _)| *ch > 0)
                        .collect();
                    ns.sort_by_key(|(ch, _, _, _)| *ch);
                    (l.id.clone(), ns)
                })
                .collect();
        // 命中层要用的纵向比例：与 paint 里同一套公式（角度 → 高度比例），
        // 否则点上去的位置和画出来的位置对不上。
        let fan_frac: std::collections::BTreeMap<String, f32> = fan_c
            .branches
            .iter()
            .map(|b| (b.id.clone(), fan_end_frac(b.angle, half_h_frac)))
            .collect();
        let frac_lbl = fan_frac.clone();
        let nodes_c = nodes_by_track.clone();
        // 命中区要用另一份：paint 闭包 move 走了 nodes_c / tr_c
        let hits_nodes = nodes_by_track.clone();
        let hits_tr = tr.clone();
        // 每条束的"重量"：把数据里本来就有、却没露过面的三样东西
        // 编码进视觉，作者不点开也能比较——
        //   粗细 = max_severity（动得多狠：改设定 > 加一场戏）
        //   长短 = 推进节数（这条线有几拍）
        //   端点 = touched（波及多少人物/线索/设定）
        // 这是信息编码，不是装饰：三个量都能在浮层里被文字复述。
        let mut weights: BTreeMap<String, (u8, usize, usize)> = BTreeMap::new();
        {
            let beats_max = fan_c
                .branches
                .iter()
                .map(|b| b.beats.len())
                .max()
                .unwrap_or(1)
                .max(1);
            for b in fan_c.branches.iter() {
                let Some(l) = lines.iter().find(|l| l.id == b.id) else {
                    continue;
                };
                weights.insert(b.id.clone(), (l.max_severity(), l.touched(), beats_max));
            }
        }
        let weights_c = weights.clone();
        // 节点标签 → 章号（供详情面板显示）。预演节点在数据里没有章号，
        // 章号是 layout 按"边界之后逐章"推出来的，这里要把同一个值取回来，
        // 否则面板会显示"未排章"，和图上画的位置自相矛盾。
        let beat_ch: BTreeMap<String, (u32, bool)> = fan_lbl
            .iter()
            .flat_map(|b| b.beats.iter().map(|x| (x.label.clone(), (x.chapter, x.scheduled))))
            .collect();
        let beat_ch_p = beat_ch.clone();
        // paint 闭包会 move 走 weights，DOM 标签层要另一份。
        let weights_lbl = weights.clone();

        // 标签层只列扇形里的束（正典不贴名字，它就是主干）
        let labels_lbl: Vec<crate::worldline::layout::FanBranch> = fan_lbl.clone();
        let pivot_x_ch = fan.pivot_chapter;
        let picked_c = self.picked.clone();
        let sel = self.selected.clone();
        let bounds_cell = self.stage_bounds.clone();
        let timeline = div()
            .absolute()
            .size_full()
            .child(
                canvas(
                    move |bounds, _, _| {
                        *bounds_cell.borrow_mut() = Some((
                            bounds.origin.x.as_f32(),
                            bounds.origin.y.as_f32(),
                            bounds.size.width.as_f32(),
                            bounds.size.height.as_f32(),
                        ));
                    },
                    move |bounds, _, window, _| {
                        let w = bounds.size.width.as_f32();
                        let h = bounds.size.height.as_f32();
                        let ox = bounds.origin.x.as_f32();
                        let oy = bounds.origin.y.as_f32();
                        let p = |x: f32, y: f32| point(px(ox + x), px(oy + y));
                        let pad_l = 34.0f32;
                        let pad_r = 26.0f32;
                        let axis_w = (w - pad_l - pad_r).max(1.);
                        let x_of = |ch: u32| pad_l + x_frac(ch) * axis_w;
                        // 主干永远在正中；扇出的每一束按角度落在上下两侧。
                        let mid = h * 0.5;
                        let y_of = |id: &str, is_base: bool| -> f32 {
                            if is_base {
                                mid
                            } else {
                                let ang = fan_c
                                    .branches
                                    .iter()
                                    .find(|b| b.id == id)
                                    .map(|b| b.angle)
                                    .unwrap_or(0.);
                                // 与标签、命中区共用 fan_end_frac（占画布高的比例），
                                // 不再用 w*0.45——那会让宽屏上曲线甩到边缘、标签却停在中间。
                                h * fan_end_frac(ang, half_h_frac)
                            }
                        };

                        // 不要任何装饰性点阵。
                        //
                        // 这里曾经铺过一层"星点"（确定性 LCG，110 个淡点），
                        // 本意是太空感；实际效果是作者把它们当成了没标注的数据点，
                        // 追问"右上角那几个点是哪条线"。世界线页只该有数据。
                        // 同理由也适用于"从现在发散的同心弧"——那会冒充世界线。

                        // 幕带：竖向交替底色，标出卷/幕的范围
                        for (bi, b) in bands_c.iter().enumerate() {
                            let x0 = x_of(b.start.saturating_sub(1));
                            let x1 = x_of(b.end);
                            if x1 <= x0 {
                                continue;
                            }
                            let mut r = PathBuilder::fill();
                            r.move_to(p(x0, 0.));
                            r.line_to(p(x1, 0.));
                            r.line_to(p(x1, h));
                            r.line_to(p(x0, h));
                            r.close();
                            if let Ok(path) = r.build() {
                                let a = if bi % 2 == 0 { 0.05 } else { 0.022 };
                                window.paint_path(path, hsla(theme::PURPLE()).opacity(a));
                            }
                        }

                        // 章节刻度：每 5 章一道细线。**只画到固化边界**——
                        // 边界之后是推演区，在那一带标章号等于说"预演排到第几章"，
                        // 而预演其实一个章都没排。
                        let tick_end = to_ch.min(boundary.max(from_ch));
                        let mut tick = from_ch;
                        while tick <= tick_end {
                            if tick > 0 && tick % 5 == 0 {
                                let x = x_of(tick);
                                let mut t = PathBuilder::stroke(px(1.));
                                t.move_to(p(x, 8.));
                                t.line_to(p(x, h - 8.));
                                if let Ok(path) = t.build() {
                                    window.paint_path(path, hsla(theme::LINE()).opacity(0.45));
                                }
                            }
                            tick += 1;
                        }

                        // 固化边界：一条竖向实线，标"剧情确定到这儿"
                        if boundary > 0 {
                            let x = x_of(boundary);
                            let mut b = PathBuilder::stroke(px(1.6));
                            b.move_to(p(x, 10.));
                            b.line_to(p(x, h - 10.));
                            if let Ok(path) = b.build() {
                                window.paint_path(path, hsla(theme::ACCENT()).opacity(0.75));
                            }
                        }

                        // —— 主干：已固化的那条直线（节点轴） ——
                        //
                        // 它从第 0 章画到固化边界，不分支。**粗细不是装饰**：
                        // 按这一章有几个事件来定——堆了三件事的章，轴更粗，
                        // 作者扫一眼就知道哪几章是戏肉、哪几章是过渡。
                        // 数据来自 events.json，不是估计。
                        let trunk_y = mid;
                        let trunk_x0 = x_of(from_ch);
                        let trunk_x1 = x_of(boundary).max(trunk_x0);
                        let max_d = dens_c.iter().copied().max().unwrap_or(0).max(1) as f32;
                        if trunk_x1 > trunk_x0 {
                            // 一层固定的淡光晕，托住这条轴
                            let mut halo = PathBuilder::stroke(px(11.));
                            halo.move_to(p(trunk_x0, trunk_y));
                            halo.line_to(p(trunk_x1, trunk_y));
                            if let Ok(hp) = halo.build() {
                                window.paint_path(hp, hsla(theme::BLUE()).opacity(0.08));
                            }
                            // 逐章画：每章一段，宽度由该章的事件数决定。
                            // 章跨度大时相邻章只差几像素，段与段自然连成渐变。
                            let last = boundary.max(from_ch);
                            for ch in from_ch..=last {
                                let d = dens_c.get(ch as usize).copied().unwrap_or(0) as f32;
                                let w = 1.5 + 2.6 * (d / max_d);
                                let xa = x_of(ch);
                                let xb = x_of((ch + 1).min(last + 1));
                                if xb <= xa {
                                    continue;
                                }
                                let mut seg = PathBuilder::stroke(px(w));
                                seg.move_to(p(xa, trunk_y));
                                seg.line_to(p(xb, trunk_y));
                                if let Ok(path) = seg.build() {
                                    window.paint_path(path, hsla(theme::BLUE()).opacity(0.95));
                                }
                            }
                        }
                        // 主轴上的**事件节点**：每一个真实事件一个点。
                        // 之前这条轴上一个点都没有（正典线在数据里没有节点），
                        // 所以它看起来像一根装饰线；事件台账里明明有 30 条。
                        for (ch, happened) in ev_c.iter() {
                            if *ch < from_ch || *ch > to_ch || *ch > boundary {
                                continue;
                            }
                            let x = x_of(*ch);
                            let ev_picked = matches!(&picked_c, Some(Pick::Event { chapter }) if *chapter == *ch);
                            // 点开的章带一圈光晕，作者一眼知道抽屉说的是哪个点。
                            if ev_picked {
                                let mut g = PathBuilder::fill();
                                for (j, (px_, py_)) in circle_pts(x, trunk_y, 9.0).iter().enumerate() {
                                    if j == 0 {
                                        g.move_to(p(*px_, *py_));
                                    } else {
                                        g.line_to(p(*px_, *py_));
                                    }
                                }
                                g.close();
                                if let Ok(path) = g.build() {
                                    window.paint_path(path, hsla(theme::ACCENT()).opacity(0.18));
                                }
                            }
                            // 点要略大于轴，否则会被轴吃掉——轴是"串"，点是"珠"。
                            let r = if ev_picked { 4.6 } else { 3.4f32 };
                            let mut d = if *happened {
                                PathBuilder::fill()
                            } else {
                                PathBuilder::stroke(px(1.2))
                            };
                            for (j, (px_, py_)) in circle_pts(x, trunk_y, r).iter().enumerate() {
                                if j == 0 {
                                    d.move_to(p(*px_, *py_));
                                } else {
                                    d.line_to(p(*px_, *py_));
                                }
                            }
                            d.close();
                            if let Ok(path) = d.build() {
                                let c = if ev_picked { theme::ACCENT() } else { theme::TEXT() };
                                window.paint_path(path, hsla(c).opacity(if ev_picked { 1.0 } else { 0.55 }));
                            }
                        }

                        // —— 扇形：从"现在"散开的多种未来 ——
                        //
                        // 一束一条预演。**不落章号**：预演阶段本来就没有章号，
                        // 现在可以按章号画了——因为每个节点都带了章号：
                        // 排过章的用真章号，没排章的用**推演章序**（第 31、32…），
                        // 并在界面上标成"推演"。这跟上一版"偷偷编一个 42"不同：
                        // 这次章号是显式的、可解释的，作者知道自己在看预测。
                        let px0 = x_of(pivot_x_ch);
                        for b in fan_c.branches.iter() {
                            let on = sel.as_deref() == Some(b.id.as_str());
                            let y1 = y_of(&b.id, false);
                            let color = if b.kind == Kind::Forecast {
                                theme::GREEN()
                            } else {
                                theme::PURPLE()
                            };
                            let (sev, touch, _beats_max) = weights_c
                                .get(&b.id)
                                .copied()
                                .unwrap_or((0, 0, 1));
                            // 束的长度 = 这条预测推到第几章。章号来自节点本身。
                            let end_ch = b.last_beat_chapter().max(pivot_x_ch + 1);
                            let x1 = x_of(end_ch).max(px0 + 24.);
                            let dx = x1 - px0;
                            let c1 = px0 + dx * 0.35;
                            let c2 = px0 + dx * 0.72;
                            let c2y = y1 * 0.45 + trunk_y * 0.55;
                            // 曲线用**采样折线**画，不用 cubic_bezier_to：
                            // 后者在这个 PathBuilder 上会被画成两段（一条曲线 +
                            // 一段悬空的续段），看起来像凭空多出一条世界线。
                            // 24 段足够平滑，而且可复现。
                            const SEG: usize = 24;
                            // 粗细 = 动得多狠：high 影响（动到设定层）明显更粗。
                            let base_w = 1.3 + 0.55 * sev as f32;
                            let mut path = PathBuilder::stroke(px(if on { base_w + 0.9 } else { base_w }));
                            path = path.dash_array(&[px(5.), px(5.)]);
                            for i in 0..=SEG {
                                let t = i as f32 / SEG as f32;
                                let mt = 1. - t;
                                let bx = mt * mt * mt * px0
                                    + 3. * mt * mt * t * c1
                                    + 3. * mt * t * t * c2
                                    + t * t * t * x1;
                                let by = mt * mt * mt * trunk_y
                                    + 3. * mt * mt * t * trunk_y
                                    + 3. * mt * t * t * c2y
                                    + t * t * t * y1;
                                if i == 0 {
                                    path.move_to(p(bx, by));
                                } else {
                                    path.line_to(p(bx, by));
                                }
                            }
                            if let Ok(path) = path.build() {
                                window.paint_path(path, hsla(color).opacity(if on { 1.0 } else { 0.7 }));
                            }
                            // 末端光点
                            let mut dot = PathBuilder::fill();
                            // 端点大小 = 波及面：牵扯的正典条目越多，点越大。
                            let r: f32 = (2.8f32 + 0.45 * touch as f32).clamp(2.8, 6.5)
                                + if on { 1.2 } else { 0. };
                            for (j, (px_, py_)) in circle_pts(x1, y1, r).iter().enumerate() {
                                if j == 0 {
                                    dot.move_to(p(*px_, *py_));
                                } else {
                                    dot.line_to(p(*px_, *py_));
                                }
                            }
                            dot.close();
                            if let Ok(d) = dot.build() {
                                window.paint_path(d, hsla(color).opacity(0.9));
                            }
                            // 束上的**多个关键节点**：一直画，不是只画选中。
                            //
                            // 上一版只在选中时画点、平时只留一个端点圆——作者
                            // 看到的就是"这条未来只预测了一件事"。而一条未来
                            // 本来是一串关节（遇伏 → 交底 → 抬升线索 → 收束）。
                            // 位置按**章号**落：预测有真实章号的按真实章号，
                            // 没章号的按推演章序（第 31、32…）逐章排。
                            // 实心 = 已排章；空心 = 推演序（预测，未定稿）。
                            for beat in b.beats.iter() {
                                let xa = x_of(beat.chapter);
                                if xa < px0 {
                                    continue;
                                }
                                // 与命中区共用 fan_beat_frac：画出来的点和点得中的点
                                // 必须是同一个，否则作者点节点会点空。
                                let (bfx, bfy) = fan_beat_frac(
                                    x_frac(pivot_x_ch),
                                    x_frac(end_ch),
                                    fan_end_frac(b.angle, half_h_frac),
                                    pivot_x_ch,
                                    end_ch,
                                    beat.chapter,
                                );
                                let nx = pad_l + bfx * axis_w;
                                let ny = bfy * h;
                                let rr = if on { 3.6 } else { 2.8 };
                                let mut d = if beat.scheduled {
                                    PathBuilder::fill()
                                } else {
                                    PathBuilder::stroke(px(1.2))
                                };
                                for (j, (px_, py_)) in circle_pts(nx, ny, rr).iter().enumerate() {
                                    if j == 0 {
                                        d.move_to(p(*px_, *py_));
                                    } else {
                                        d.line_to(p(*px_, *py_));
                                    }
                                }
                                d.close();
                                if let Ok(d) = d.build() {
                                    window.paint_path(
                                        d,
                                        hsla(color).opacity(if on { 1.0 } else { 0.8 }),
                                    );
                                }
                            }
                        }

                        // —— 枢轴点：直线与扇形的交汇处 ——
                        //
                        // 这是"现在"：左边是定下来的，右边是还要挑的。
                        // 画一个明确的空心环 + 一圈光晕，让作者一眼找到它。
                        {
                            let r = 6.0f32;
                            let mut halo = PathBuilder::fill();
                            let hr = 14.0f32;
                            for (j, (px_, py_)) in circle_pts(px0, trunk_y, hr).iter().enumerate() {
                                if j == 0 {
                                    halo.move_to(p(*px_, *py_));
                                } else {
                                    halo.line_to(p(*px_, *py_));
                                }
                            }
                            halo.close();
                            if let Ok(h) = halo.build() {
                                window.paint_path(h, hsla(theme::ACCENT()).opacity(0.14));
                            }
                            let mut ring = PathBuilder::stroke(px(2.));
                            for (j, (px_, py_)) in circle_pts(px0, trunk_y, r).iter().enumerate() {
                                if j == 0 {
                                    ring.move_to(p(*px_, *py_));
                                } else {
                                    ring.line_to(p(*px_, *py_));
                                }
                            }
                            ring.close();
                            if let Ok(ring) = ring.build() {
                                window.paint_path(ring, hsla(theme::ACCENT()));
                            }
                        }

                        // —— 节点 ——
                        //
                        // 主干上的节点：已写 = 绿实心；已采纳待写 = 蓝实心；纯计划 = 空心。
                        // 扇形里的节点：一律空心（预演还没发生）。
                        for (tid, ns) in nodes_c.iter() {
                            let is_base = tid == "α";
                            let y = y_of(tid, is_base);
                            let color = if is_base {
                                theme::BLUE()
                            } else if fan_c.branches.iter().any(|b| &b.id == tid && b.kind == Kind::Forecast) {
                                theme::GREEN()
                            } else {
                                theme::PURPLE()
                            };
                            // 扇形上的节点不画：它们本来就分布在束线上，
                            // 画出来会和束末的光点、束名标签糊在一起
                            //（曾经画过，作者读到的是"右上角这几个点是什么"）。
                            // 要看某条线推到哪一章、有哪些节点，点它的名字就行——
                            // 右侧详情会按章列出完整发展路线。
                            if !is_base {
                                continue;
                            }
                            for (ch, nid, _label, state) in ns {
                                if *ch < from_ch || *ch > to_ch {
                                    continue;
                                }
                                let x = x_of(*ch);
                                let picked = picked_c
                                    .as_ref()
                                    .map(|p| matches!(p, Pick::Node { line, node } if line == tid && node == nid))
                                    .unwrap_or(false);
                                let halo = if picked { 9.0 } else { 7.0 };
                                let mut g = PathBuilder::fill();
                                for (j, (px_, py_)) in circle_pts(x, y, halo).iter().enumerate() {
                                    if j == 0 {
                                        g.move_to(p(*px_, *py_));
                                    } else {
                                        g.line_to(p(*px_, *py_));
                                    }
                                }
                                g.close();
                                if let Ok(path) = g.build() {
                                    let c = if picked { theme::ACCENT() } else { color };
                                    window.paint_path(path, hsla(c).opacity(0.16));
                                }
                                let r = if picked { 5.2 } else { 4.0 };
                                if state.is_canon() {
                                    let fill = if *state == crate::worldline::NodeState::Written {
                                        theme::GREEN()
                                    } else {
                                        theme::BLUE()
                                    };
                                    let mut d = PathBuilder::fill();
                                    for (j, (px_, py_)) in circle_pts(x, y, r).iter().enumerate() {
                                        if j == 0 {
                                            d.move_to(p(*px_, *py_));
                                        } else {
                                            d.line_to(p(*px_, *py_));
                                        }
                                    }
                                    d.close();
                                    if let Ok(path) = d.build() {
                                        window.paint_path(path, hsla(fill));
                                    }
                                } else {
                                    let mut d = PathBuilder::stroke(px(1.3));
                                    for (j, (px_, py_)) in circle_pts(x, y, r).iter().enumerate() {
                                        if j == 0 {
                                            d.move_to(p(*px_, *py_));
                                        } else {
                                            d.line_to(p(*px_, *py_));
                                        }
                                    }
                                    d.close();
                                    if let Ok(path) = d.build() {
                                        window.paint_path(path, hsla(color).opacity(0.9));
                                    }
                                }
                            }
                        }
                    },
                )
                .absolute()
                .size_full(),
            )
            // 手势层：拖拽平移、滚轮缩放、点线选中。
            //
            // 放在画布之上、线名/节点命中区之下：空白拖动不会误触节点，
            // 而节点与线名仍然优先拿到点击。
            .child(
                div()
                    .id("wl-pan-layer")
                    .absolute()
                    .size_full()
                    // 可以拖的画布 = 小手。按住时握拳（ClosedHand），
                    // 这是"这张图能抓起来挪"的唯一提示。
                    .cursor(if self.drag.is_some() {
                        CursorStyle::ClosedHand
                    } else {
                        CursorStyle::OpenHand
                    })
                    .on_mouse_down(MouseButton::Left, cx.listener(|this, ev: &MouseDownEvent, _, cx| {
                        this.drag = Some((ev.position.x.as_f32(), 0., false));
                        // 立刻重绘一次：小手要在按下的那一帧就变成握拳，
                        // 否则要等下一次数据变化才换光标。
                        cx.notify();
                    }))
                    .on_mouse_move(cx.listener(move |this, ev: &MouseMoveEvent, _, cx| {
                        // 每章占多少像素：按当前取景框的实际比例算，不再固定 18px。
                        // 固定值在放大后（一章几十像素）拖一下跳好几章，缩小时又拖不动。
                        let px_per_ch = {
                            let w = this
                                .stage_bounds
                                .borrow()
                                .map(|(_, _, w, _)| w)
                                .unwrap_or(0.);
                            let axis_w = (w - 60.).max(1.);
                            (axis_w / span as f32).max(2.)
                        };
                        let step = {
                            let Some((last_x, residual, moved)) = &mut this.drag else {
                                return;
                            };
                            let dx = ev.position.x.as_f32() - *last_x;
                            *last_x = ev.position.x.as_f32();
                            if dx.abs() > 0.5 {
                                *moved = true;
                            }
                            *residual += dx;
                            // 按整章平移，避免窗口反复重排；余数留在 residual 里累计。
                            let step = (*residual / px_per_ch).trunc();
                            if step != 0.0 {
                                *residual -= step * px_per_ch;
                            }
                            step
                        };
                        if step != 0.0 {
                            // 向右拖 = 看更早的章节（内容跟手）
                            this.pan(-(step as i32), horizon);
                            cx.notify();
                        }
                    }))
                    .on_mouse_up(MouseButton::Left, cx.listener(|this, ev: &MouseUpEvent, window, cx| {
                        let Some((_, _, moved)) = this.drag.take() else {
                            return;
                        };
                        cx.notify();
                        if moved {
                            return;
                        }
                        // 没拖动 = 点击：按纵坐标找最近的那条线。
                        // 这样点线上任意位置都能选中，不用正好点到右端的线名小标签。
                        let Some((ox, oy, w, h)) = *this.stage_bounds.borrow() else {
                            return;
                        };
                        let x = ev.position.x.as_f32();
                        let y = ev.position.y.as_f32();
                        if x < ox || x > ox + w || y < oy || y > oy + h {
                            return;
                        }
                        let frac = ((y - oy) / h.max(1.)).clamp(0., 1.);
                        let best = this
                            .track_fracs
                            .borrow()
                            .iter()
                            .map(|(id, f)| (id.clone(), (f - frac).abs()))
                            .min_by(|a, b| {
                                a.1.partial_cmp(&b.1).unwrap_or(std::cmp::Ordering::Equal)
                            });
                        let Some((id, dist)) = best else { return };
                        if dist > 0.06 {
                            return;
                        }
                        this.selected = Some(id.clone());
                        cx.emit(WorldlineEvent::Select(id));
                        this.fill_meta(window, cx);
                        cx.notify();
                    }))
                    .on_mouse_up_out(MouseButton::Left, cx.listener(|this, _, _, _| {
                        this.drag = None;
                    }))
                    .on_scroll(cx.listener(move |this, ev: &ScrollEvent, _, cx| {
                        let dy = match ev.delta {
                            ScrollDelta::Pixels(p) => p.y.as_f32(),
                            ScrollDelta::Lines(p) => p.y * 24.0,
                        };
                        this.scroll_acc += dy;
                        // 触控板一次手势会发很多小增量，累计到阈值再缩一档。
                        if this.scroll_acc.abs() >= 30.0 {
                            // zoom_window: factor>1 看得更远，factor<1 放大
                            let factor = if this.scroll_acc > 0.0 { 1.25 } else { 0.8 };
                            this.zoom(factor, horizon);
                            this.scroll_acc = 0.;
                            cx.notify();
                        }
                    })),
            )
            // 幕名（顶部）
            .child(
                div()
                    .absolute()
                    .left(px(34.))
                    .right(px(26.))
                    .top(px(6.))
                    .h(px(14.))
                    .relative()
                    .children(band_list.iter().map(|b| {
                        let start = (b.start.saturating_sub(from_ch)) as f32 / span as f32;
                        let width = ((b.end.saturating_sub(b.start) + 1) as f32 / span as f32)
                            .clamp(0.03, 1.);
                        div()
                            .absolute()
                            .left(relative(start))
                            .w(relative(width))
                            .overflow_hidden()
                            .whitespace_nowrap()
                            .text_size(theme::FONT_MICRO)
                            .text_color(theme::PURPLE())
                            .child(SharedString::from(b.name.clone()))
                    })),
            )
            // 固化边界不再单独标注：枢轴上的「现在 · 第N章」卡片已经说了这件事，
            // 两份文字在窄窗口里会压在一起。边界章号只留一处。
            // 束名：贴在画布右缘、按各自角度上下排。
            //
            // 两行结构，因为"选哪条未来"要一眼比得出三件事：
            //   第一行：线号 + 名字 + 节数（这是什么路）
            //   第二行：赌注（premise，这条线相信会发生什么）
            // 赌注只在束少的时候显示——束一多，两行标签会在垂直方向互相压住，
            // 那时候退回单行，靠左栏和点开看。
            .children(labels_lbl.iter().filter_map(|t| {
                let f = frac_lbl.get(&t.id).copied()?;
                let b = fan_lbl.iter().find(|b| b.id == t.id)?;
                let id = t.id.clone();
                let color = if t.kind == Kind::Forecast {
                    theme::GREEN()
                } else {
                    theme::PURPLE()
                };
                let on = self.selected.as_deref() == Some(t.id.as_str());
                // 聚焦：已经选中一条时，其余束名压暗。
                // 导演看分镜也是这个道理——选定了就压别的，别让它们抢眼。
                let dimmed = self.selected.is_some() && !on;
                let pick = t.id.clone();
                let name = b.name.clone();
                // 束末端的横向比例（章号 → 画布位置），与 paint 的 x_of 同源
                let end_ch = b.last_beat_chapter().max(pivot_x_ch + 1);
                let end_frac = x_frac(end_ch).clamp(0.05, 0.92);
                // 标签往**扇外**挪：向上那束的标签压在上方，向下的压在下方。
                // 压在曲线的延长线上，标签就会盖住自己的节点（试过，很脏）。
                let f_shift = if b.angle < 0. { -0.155 } else { 0.045 };
                let f_lbl = (f + f_shift).clamp(0.02, 0.80);
                let (sev, touch, _) = weights_lbl.get(&t.id).copied().unwrap_or((0, 0, 1));
                let premise = b.premise.clone();
                let show_premise = fan_lbl.len() <= 4 && !premise.trim().is_empty();
                // 严重度标签：只有真的写了影响才显示，否则不摆一个"无"
                let sev_chip = match sev {
                    3 => Some(("改设定", theme::AMBER())),
                    2 => Some(("改剧情", theme::BLUE())),
                    1 => Some(("小改", theme::FAINT())),
                    _ => None,
                };
                Some(
                    div()
                        .id(SharedString::from(format!("wllbl-{}", t.id)))
                        .absolute()
                        // 贴在这束**自己的末端**：束按章号收尾，一律靠右缘
                        // 会把标签甩到离曲线很远的地方。
                        // 靠右的束要把标签翻到左边，否则会被画布裁掉半句。
                        .top(relative(f_lbl))
                        .when(end_frac > 0.62, |d| {
                            d.right(relative((1. - end_frac).max(0.))).mr(px(12.))
                        })
                        .when(end_frac <= 0.62, |d| d.left(relative(end_frac)).ml(px(12.)))
                        .mt(px(if show_premise { -16. } else { -9. }))
                        .cursor_pointer()
                        .px(px(5.))
                        .py(px(2.))
                        .rounded(px(4.))
                        .bg(theme::BG())
                        .when(dimmed, |d| d.opacity(0.45))
                        .when(on, |d| d.border_1().border_color(theme::ACCENT_BORDER()))
                        .max_w(px(if show_premise { 236. } else { 168. }))
                        .flex()
                        .flex_col()
                        .gap(px(1.))
                        .child(
                            div()
                                .flex()
                                .items_center()
                                .gap(px(4.))
                                .text_size(px(10.))
                                .text_color(if on { theme::ACCENT() } else { color })
                                .child(
                                    div()
                                        .flex_shrink_0()
                                        .font_family(theme::FAMILY_MONO)
                                        .child(SharedString::from(id)),
                                )
                                .child(
                                    div()
                                        .flex_1()
                                        .min_w_0()
                                        .text_ellipsis()
                                        .child(SharedString::from(name)),
                                )
                                .child(
                                    div()
                                        .flex_shrink_0()
                                        .font_family(theme::FAMILY_MONO)
                                        .text_size(px(9.))
                                        .text_color(theme::FAINT())
                                        .child(SharedString::from(format!("{} 节", t.beats.len()))),
                                ),
                        )
                        .when(show_premise, |d| {
                            d.child(
                                div()
                                    .text_size(px(9.))
                                    .line_height(relative(1.35))
                                    .text_color(theme::FAINT())
                                    .line_clamp(2)
                                    .child(SharedString::from(premise)),
                            )
                        })
                        .when_some(sev_chip, |d, (label, c)| {
                            d.child(
                                div()
                                    .flex()
                                    .items_center()
                                    .gap(px(6.))
                                    .text_size(px(9.))
                                    .text_color(c)
                                    .child(label)
                                    .child(
                                        div()
                                            .font_family(theme::FAMILY_MONO)
                                            .text_color(theme::FAINT())
                                            .child(SharedString::from(format!("波及 {touch}"))),
                                    ),
                            )
                        })
                        .on_click(cx.listener(move |this, _, window, cx| {
                            this.selected = Some(pick.clone());
                            cx.emit(WorldlineEvent::Select(pick.clone()));
                            this.fill_meta(window, cx);
                            cx.notify();
                        }))
                        .into_any_element(),
                )
            }))
            // 枢轴与推演区的说明文字：贴在画布上沿。
            // "现在"是这张图唯一的时间锚，必须点名，否则作者会以为
            // 扇形的起点是随机的某章。
            .children({
                let mut v: Vec<gpui_kit::AnyElement> = Vec::new();
                let pivot_frac = if span > 0 {
                    ((pivot_x_ch.saturating_sub(from_ch)) as f32 / span as f32).clamp(0., 1.)
                } else {
                    1.0
                };
                // "现在"不只是一个点：选路之前必须先知道**当下还欠着什么**。
                // 未闭合的伏笔 = 必须在这条路上收掉的账；已排期未写的事件 =
                // 未来并不是完全空的。两个数都直接从台账读，不猜。
                v.push(
                    div()
                        .absolute()
                        .left(relative(pivot_frac))
                        .top(px(6.))
                        .ml(px(-22.))
                        .px(px(5.))
                        .py(px(2.))
                        .rounded(px(4.))
                        .bg(theme::BG())
                        .border_1()
                        .border_color(theme::ACCENT_BORDER())
                        .flex()
                        .flex_col()
                        .items_center()
                        .gap(px(1.))
                        .child(
                            div()
                                .text_size(px(9.))
                                .text_color(theme::ACCENT())
                                .child("现在"),
                        )
                        .child(
                            div()
                                .font_family(theme::FAMILY_MONO)
                                .text_size(px(8.))
                                .text_color(theme::FAINT())
                                .child(SharedString::from(format!(
                                    "第{boundary}章 · 欠 {open_clues} 笔"
                                ))),
                        )
                        .into_any_element(),
                );
                v.push(
                    div()
                        .absolute()
                        // 推演区的标题：放在这片区域的上沿居中，
                        // 但避开"现在"卡片（卡片占了枢轴右侧一小块）。
                        .left(relative((pivot_frac + 1.) / 2.))
                        .top(px(8.))
                        .ml(px(-40.))
                        .opacity(0.75)
                        .text_size(px(9.))
                        .text_color(theme::PURPLE())
                        .child("推演区 · 横向放大")
                        .into_any_element(),
                );
                v
            })
            // 命中区：点画出来的点 → 抽屉。三类点各建热区，坐标与 paint 同源：
            //   主干节点（α）→ x_frac(ch)；扇束节点 → fan_beat_frac；主干事件 → x_frac(ch)。
            // 早先只给 α 建热区、且用线性比例（忽略推演区放大），点扇束和边界后的点都点空。
            .child(
                div()
                    .absolute()
                    .left(px(34.))
                    .right(px(26.))
                    .top_0()
                    .bottom_0()
                    .children({
                        let mut hits: Vec<gpui_kit::AnyElement> = Vec::new();
                        // —— 主干（正典）节点 ——
                        for t in hits_tr.iter() {
                            if !(t.kind == Kind::Base || t.id == "α") {
                                continue;
                            }
                            let Some(ns) = hits_nodes.get(&t.id) else { continue };
                            for (ch, nid, _label, _state) in ns {
                                if *ch < from_ch || *ch > to_ch {
                                    continue;
                                }
                                let fx = x_frac(*ch);
                                let hid = SharedString::from(format!("wlhit-{}-{nid}", t.id));
                                let target = Pick::Node { line: t.id.clone(), node: nid.clone() };
                                hits.push(hit_dot(hid, fx, 0.5, target, cx));
                            }
                        }
                        // —— 扇束上的节点（预演关节）——
                        for b in fan_lbl.iter() {
                            let end_ch = b.last_beat_chapter().max(pivot_x_ch + 1);
                            let ey = fan_end_frac(b.angle, half_h_frac);
                            for beat in b.beats.iter() {
                                if beat.chapter < from_ch || beat.chapter > to_ch {
                                    continue;
                                }
                                let (fx, fy) = fan_beat_frac(
                                    x_frac(pivot_x_ch),
                                    x_frac(end_ch),
                                    ey,
                                    pivot_x_ch,
                                    end_ch,
                                    beat.chapter,
                                );
                                let hid = SharedString::from(format!("wlhit-{}-{}", b.id, beat.id));
                                let target = Pick::Node { line: b.id.clone(), node: beat.id.clone() };
                                hits.push(hit_dot(hid, fx, fy, target, cx));
                            }
                        }
                        // —— 主干上的事件点（正典没有节点，关节来自事件台账）——
                        let mut ev_chs: Vec<u32> = ev_dots
                            .iter()
                            .map(|(ch, _)| *ch)
                            .filter(|ch| *ch >= from_ch && *ch <= to_ch && *ch <= boundary)
                            .collect();
                        ev_chs.sort_unstable();
                        ev_chs.dedup();
                        for ch in ev_chs {
                            let hid = SharedString::from(format!("wlevhit-{ch}"));
                            hits.push(hit_dot(hid, x_frac(ch), 0.5, Pick::Event { chapter: ch }, cx));
                        }
                        hits
                    }),
            )
            // 章号刻度尺（底部）：横轴是真实章节，作者要能读出每个点落在第几章。
            // 只标到固化边界——边界后是推演区，标章号等于把预测当排期。
            // 间距按跨度自适应（ruler_step），长书不糊、放大后不只剩一两道。
            .child(
                div()
                    .absolute()
                    .left(px(34.))
                    .right(px(26.))
                    .bottom(px(26.))
                    .h(px(12.))
                    .children({
                        let step = ruler_step(span);
                        let mut ch = ((from_ch + step - 1) / step) * step;
                        let mut marks: Vec<gpui_kit::AnyElement> = Vec::new();
                        while ch <= to_ch.min(boundary) {
                            if ch > 0 {
                                marks.push(
                                    div()
                                        .absolute()
                                        .left(relative(x_frac(ch)))
                                        .ml(px(-8.))
                                        .text_size(px(9.))
                                        .font_family(theme::FAMILY_MONO)
                                        .text_color(theme::FAINT())
                                        .child(SharedString::from(format!("{ch}")))
                                        .into_any_element(),
                                );
                            }
                            ch += step;
                        }
                        marks
                    }),
            )
            // 图例（左下角）
            .child(
                div()
                    .absolute()
                    .left(px(20.))
                    .bottom(px(10.))
                    .flex()
                    .items_center()
                    .gap(theme::GAP_MD)
                    .text_size(theme::FONT_MICRO)
                    .text_color(theme::FAINT())
                    .child("—— 已确定")
                    .child(
                        div()
                            .text_color(theme::PURPLE())
                            .child("- - - 开放的 if"),
                    )
                    .child("● 已写")
                    .child("○ 计划")
                    .child(
                        // 图例必须解释编码，否则粗细长短又变成看不出来的装饰。
                        // 每句对应 paint 里的一个真实量：
                        // 事件密度 / 束的末端章号 / Beat::scheduled。
                        div()
                            .pl(theme::GAP_MD)
                            .border_l_1()
                            .border_color(theme::LINE_SOFT())
                            .child("主线越粗=该章事件越多 · 束的长短=预测推到第几章 · 空心点=推演章序"),
                    )
                    .child(SharedString::from(format!("第 {from_ch}–{to_ch} 章"))),
            );

        // —— 右侧详情：选中线的操作 ——
        let mut detail = div().flex().flex_col().gap(theme::GAP_SM);
        let sel_line = self
            .selected
            .as_ref()
            .and_then(|id| lines.iter().find(|l| &l.id == id))
            .cloned();
        match &sel_line {
            Some(l) => {
                let kind_note = match l.kind {
                    Kind::Base => "正典：已经写进正文的那条线。它的固化边界就是「剧情确定到哪儿」。",
                    Kind::Forecast => "预测线：按大纲/成稿往前推演「接下来会怎样」。还没定。",
                    Kind::WhatIf => "假设线：如果某件事不一样会怎样。还没定。",
                };
                let forks_here = forks_at(&lines, l.fork_chapter());
                let l2 = l.id.clone();

                let l5 = l.id.clone();
                detail = detail
                    .child(
                        div()
                            .text_size(theme::FONT_LABEL)
                            .font_weight(FontWeight::SEMIBOLD)
                            .text_color(theme::TEXT())
                            .child(SharedString::from(format!("{} · {}", l.id, l.name))),
                    )
                    .child(
                        div()
                            .text_size(theme::FONT_MICRO)
                            .line_height(relative(1.45))
                            .text_color(theme::DIM())
                            .child(SharedString::from(kind_note)),
                    )
                    // 名字与假设：AI 推演写的 premise 常常很糙，作者要能就地改
                    .when(!l.is_base(), |d| {
                        d.child(
                            div()
                                .flex()
                                .flex_col()
                                .gap(px(3.))
                                .child(field("线名", &self.ed_name))
                                .child(field("假设", &self.ed_premise))
                                .child(
                                    Button::new("wl-save-meta")
                                        .xsmall()
                                        .label("保存名字与假设")
                                        .on_click(
                                        cx.listener({
                                            let lid = l.id.clone();
                                            move |this, _, _, cx| {
                                                let name = if this.ed_name.read(cx).value().trim().is_empty() {
                                                    None
                                                } else {
                                                    Some(this.ed_name.read(cx).value().to_string())
                                                };
                                                cx.emit(WorldlineEvent::UpdateMeta {
                                                    line: lid.clone(),
                                                    name,
                                                    premise: Some(
                                                        this.ed_premise.read(cx).value().to_string(),
                                                    ),
                                                });
                                            }
                                        }),
                                    ),
                                ),
                        )
                    })
                    .child(
                        div()
                            .text_size(theme::FONT_MICRO)
                            .font_family(theme::FAMILY_MONO)
                            .text_color(theme::FAINT())
                            .child(SharedString::from(format!(
                                "分叉于第{}章 · 节点 {} · 未晋升 {}",
                                l.fork_chapter(),
                                l.nodes.len(),
                                l.pending_diffs()
                            ))),
                    )
                    .child(
                        div()
                            .text_size(theme::FONT_MICRO)
                            .text_color(theme::FAINT())
                            .child(SharedString::from(format!(
                                "同一个分叉点上共有 {} 条线",
                                forks_here.len()
                            ))),
                    )
                    // 影响清单：这条线**动了什么**。
                    //
                    // 这是挑未来时最先要比的东西，之前整页都没有入口——
                    // 作者看得到"会发生什么"（节点），看不到"要付出什么"
                    //（改了哪些设定/剧情、伤口多深）。严重度从高到低排，
                    // high 会动到设定层，medium 是加戏/换人，low 是小修。
                    .when(!l.diffs.is_empty(), |d| {
                        let mut ds = l.diffs.clone();
                        ds.sort_by_key(|x| match x.severity.as_str() {
                            "high" => 0,
                            "medium" => 1,
                            _ => 2,
                        });
                        let total = ds.len();
                        d.child(
                            div()
                                .flex()
                                .flex_col()
                                .gap(px(3.))
                                .pt(theme::GAP_SM)
                                .child(
                                    div()
                                        .flex()
                                        .items_center()
                                        .gap(theme::GAP_SM)
                                        .child(
                                            div()
                                                .text_size(theme::FONT_MICRO)
                                                .text_color(theme::FAINT())
                                                .child("影响清单"),
                                        )
                                        .child(
                                            div()
                                                .font_family(theme::FAMILY_MONO)
                                                .text_size(px(9.))
                                                .text_color(theme::FAINT())
                                                .child(SharedString::from(format!(
                                                    "{total} 处 · 波及 {} 条正典",
                                                    l.touched()
                                                ))),
                                        ),
                                )
                                .children(ds.into_iter().map(|x| {
                                    let (tag, c) = match x.severity.as_str() {
                                        "high" => ("改设定", theme::AMBER()),
                                        "medium" => ("改剧情", theme::BLUE()),
                                        "low" => ("小改", theme::FAINT()),
                                        _ => ("细修", theme::FAINT()),
                                    };
                                    div()
                                        .flex()
                                        .flex_col()
                                        .gap(px(1.))
                                        .py(px(2.))
                                        .child(
                                            div()
                                                .flex()
                                                .items_center()
                                                .gap(theme::GAP_SM)
                                                .text_size(theme::FONT_MICRO)
                                                .child(
                                                    div()
                                                        .flex_shrink_0()
                                                        .text_size(px(9.))
                                                        .text_color(c)
                                                        .child(tag),
                                                )
                                                .child(
                                                    div()
                                                        .flex_shrink_0()
                                                        .text_color(theme::DIM())
                                                        .child(SharedString::from(x.target.clone())),
                                                )
                                                .child(
                                                    div()
                                                        .flex_1()
                                                        .min_w_0()
                                                        .text_ellipsis()
                                                        .text_color(theme::TEXT())
                                                        .child(SharedString::from(x.after.clone())),
                                                ),
                                        )
                                        .child(
                                            div()
                                                .pl(px(30.))
                                                .text_size(px(9.))
                                                .text_color(theme::FAINT())
                                                .line_clamp(2)
                                                .child(SharedString::from(x.reason.clone())),
                                        )
                                })),
                        )
                    })
                    // 这里原本还有一个「发展路线」清单，按章列出节点标题。
                    // 它和下面的「节点」列表几乎是同一份数据（同一个排序、
                    // 同样的章号/未排章），两处并排只是让浮层更长。
                    // 保留下面那份：它能改状态、能跳转、能删。
                    .child(
                        div()
                            .flex()
                            .flex_wrap()
                            .gap(theme::GAP_SM)
                            // 针对这条线的 Agent 操作（能力表驱动）
                            .children({
                                let desc = format!("世界线 {}「{}」（分叉于第{}章）", l.id, l.name, l.fork_chapter());
                                let mut bar = div().flex().gap(theme::GAP_SM);
                                for aid in ["worldline", "oracle"] {
                                    let Some(spec) = crate::capabilities::agent(aid) else { continue };
                                    for op in spec.targeted {
                                        if op.target_kind != crate::capabilities::TargetKind::Line {
                                            continue;
                                        }
                                        let d = desc.clone();
                                        bar = bar.child(
                                            Button::new(SharedString::from(op.key))
                                                .xsmall()
                                                .label(op.label)
                                                .on_click(cx.listener(move |_this, _, _, cx| {
                                                    cx.emit(WorldlineEvent::Delegate {
                                                        agent: aid,
                                                        key: op.key,
                                                        target: Some(d.clone()),
                                                    });
                                                })),
                                        );
                                    }
                                }
                                vec![bar.into_any_element()]
                            })
                            .when(!l.is_base(), |d| {
                                d.child(
                                    Button::new("wl-fork")
                                        .xsmall()
                                        .label("从此处再分叉")
                                        .on_click(cx.listener(move |_this, _, _, cx| {
                                            cx.emit(WorldlineEvent::Fork {
                                                from: l2.clone(),
                                                chapter: 0,
                                            });
                                        })),
                                )
                            })
                            // 世界线是预演未来的时间轴，不是晋升入口：
                            // if 线永远留在预言书，不能被「扶正」成正典后续。
                            .when(!l.is_base(), |d| {
                                d.child(
                                    Button::new("wl-arch")
                                        .xsmall()
                                        .label(if l.is_archived() { "恢复" } else { "归档" })
                                        .on_click(cx.listener(move |_this, _, _, cx| {
                                            cx.emit(WorldlineEvent::Archive(l5.clone()));
                                        })),
                                )
                            })
                            .when(!l.is_base(), |d| {
                                // 删线：归档是"封存可恢复"，删是"真的不要了"。
                                // 子线不删，只断父指针（Store::remove 负责）。
                                let l6 = l.id.clone();
                                d.child(
                                    Button::new("wl-del")
                                        .xsmall()
                                        .label("删除")
                                        .on_click(cx.listener(move |_this, _, _, cx| {
                                            cx.emit(WorldlineEvent::Remove(l6.clone()));
                                        })),
                                )
                            }),
                    )
                    // 节点清单：时间轴上的关节，作者可以改章、删、加
                    .child(
                        div()
                            .pt(theme::GAP_SM)
                            .text_size(theme::FONT_MICRO)
                            .font_weight(FontWeight::SEMIBOLD)
                            .text_color(theme::DIM())
                            .child(SharedString::from(format!("节点 · {}", l.nodes.len()))),
                    )
                    .children({
                        let mut nodes = l.nodes.clone();
                        nodes.sort_by_key(|n| n.chapter_or_guess());
                        let lid = l.id.clone();
                        let next_ch = nodes
                            .iter()
                            .map(|n| n.chapter_or_guess())
                            .max()
                            .unwrap_or(boundary)
                            + 1;
                        let mut rows: Vec<gpui_kit::AnyElement> = nodes
                            .iter()
                            .map(|n| {
                                let raw_ch = n.chapter_or_guess();
                                // 预演节点在台账里没有章号，图上是"推演章序"。
                                // 面板必须显示同一个数，否则会出现
                                // "图上点在第 32 章、面板写未排章"。
                                let (ch, sched) = beat_ch_p
                                    .get(&n.label)
                                    .copied()
                                    .unwrap_or((raw_ch, raw_ch > 0));
                                let nid = n.id.clone();
                                let l1 = lid.clone();
                                let l2 = lid.clone();
                                let n1 = nid.clone();
                                let n2 = nid.clone();
                                div()
                                    .flex()
                                    .items_center()
                                    .gap(px(3.))
                                    .py(px(1.))
                                    .child(
                                        // 点章号 +1：改排章是最常做的动作
                                        div()
                                            .id(SharedString::from(format!("wn-ch-{lid}-{nid}")))
                                            .w(px(46.))
                                            .flex_shrink_0()
                                            .cursor_pointer()
                                            .font_family(theme::FAMILY_MONO)
                                            .text_size(theme::FONT_MICRO)
                                            .text_color(if !sched {
                                                theme::PURPLE()
                                            } else if ch > 0 {
                                                theme::FAINT()
                                            } else {
                                                theme::AMBER()
                                            })
                                            .child(SharedString::from(if !sched {
                                                format!("演{ch}")
                                            } else if ch > 0 {
                                                format!("第{ch}章")
                                            } else {
                                                "未排章".into()
                                            }))
                                            .on_click(cx.listener(move |_t, _, _, cx| {
                                                // 已排章的：点一下往后挪一章（最常用的微调）。
                                                // 推演序的：点一下把它**落实**成这一章——
                                                // "演31"变"第31章"，这才是作者想要的动作，
                                                // 而不是又推一章。
                                                cx.emit(WorldlineEvent::NodePatch {
                                                    line: l1.clone(),
                                                    node: n1.clone(),
                                                    patch: crate::worldline::NodePatch::chapter(
                                                        if sched { ch + 1 } else { ch },
                                                    ),
                                                });
                                            })),
                                    )
                                    .child(
                                        div()
                                            .flex_1()
                                            .min_w_0()
                                            .text_ellipsis()
                                            .text_size(theme::FONT_MICRO)
                                            .text_color(theme::DIM())
                                            .child(SharedString::from(n.label.clone())),
                                    )
                                    .child(
                                        // 已写进正文 / 已采纳 / 仅计划：这个区别决定它算不算
                                        // "已定剧情"。计划态点一下 = 逐节点采纳（产出提案），
                                        // 已经定下来的就不再可点，免得误删既成事实。
                                        div()
                                            .id(SharedString::from(format!("wn-adopt-{lid}-{nid}")))
                                            .flex_shrink_0()
                                            .px(px(2.))
                                            .when(!n.state.is_canon(), |d| {
                                                d.cursor_pointer().text_color(theme::ACCENT())
                                            })
                                            .when(n.state == crate::worldline::NodeState::Written, |d| {
                                                d.text_color(theme::GREEN())
                                            })
                                            .when(
                                                n.state == crate::worldline::NodeState::Adopted,
                                                |d| d.text_color(theme::BLUE()),
                                            )
                                            .text_size(px(9.))
                                            .child(SharedString::from(if n.state.is_canon() {
                                                n.state.label().to_string()
                                            } else {
                                                format!("{}·采纳", n.state.label())
                                            }))
                                            .on_click(cx.listener({
                                                let l4 = lid.clone();
                                                let n4 = nid.clone();
                                                move |_t, _, _, cx| {
                                                    cx.emit(WorldlineEvent::NodeAdopt {
                                                        line: l4.clone(),
                                                        node: n4.clone(),
                                                    });
                                                }
                                            })),
                                    )
                                    .child(
                                        // 双向弱引用：把这个时间轴关节挂到画布上的某个节点，
                                        // 之后在画布点那个人物就能反查"他在哪几条线的哪一章"。
                                        div()
                                            .id(SharedString::from(format!("wn-link-{lid}-{nid}")))
                                            .flex_shrink_0()
                                            .cursor_pointer()
                                            .px(px(2.))
                                            .text_size(theme::FONT_MICRO)
                                            .text_color(if n.canvas_ref.is_empty() {
                                                theme::FAINT()
                                            } else {
                                                theme::BLUE()
                                            })
                                            .child(SharedString::from(if n.canvas_ref.is_empty() {
                                                "关联"
                                            } else {
                                                "已关联"
                                            }))
                                            .on_click(cx.listener({
                                                let l3 = lid.clone();
                                                let n3 = nid.clone();
                                                let cur = n.canvas_ref.clone();
                                                move |_t, _, _, cx| {
                                                    // 就地点一下：有引用就清掉，没有就用自己的
                                                    // id 当默认（最常见的场景是两边同名同 id）。
                                                    cx.emit(WorldlineEvent::LinkCanvas {
                                                        line: l3.clone(),
                                                        node: n3.clone(),
                                                        canvas: if cur.is_empty() {
                                                            n3.clone()
                                                        } else {
                                                            String::new()
                                                        },
                                                    });
                                                }
                                            })),
                                    )
                                    .child(
                                        // 从这个节点起让 AI 接着推：交给预言书页（推演是那边的事）
                                        div()
                                            .id(SharedString::from(format!("wn-sim-{lid}-{nid}")))
                                            .flex_shrink_0()
                                            .cursor_pointer()
                                            .px(px(2.))
                                            .text_size(px(9.))
                                            .text_color(theme::ACCENT())
                                            .child("推演")
                                            .on_click(cx.listener({
                                                let ndesc = format!(
                                                    "世界线 {}「{}」的第{}章节点「{}」",
                                                    l.id, l.name, ch, n.label
                                                );
                                                move |_t, _, _, cx| {
                                                    cx.emit(WorldlineEvent::Delegate {
                                                        agent: "worldline",
                                                        key: "wl-from-node",
                                                        target: Some(ndesc.clone()),
                                                    });
                                                }
                                            })),
                                    )
                                    .child(
                                        div()
                                            .id(SharedString::from(format!("wn-del-{lid}-{nid}")))
                                            .flex_shrink_0()
                                            .cursor_pointer()
                                            .text_size(theme::FONT_MICRO)
                                            .text_color(theme::FAINT())
                                            .child("×")
                                            .on_click(cx.listener(move |_t, _, _, cx| {
                                                cx.emit(WorldlineEvent::NodeRemove {
                                                    line: l2.clone(),
                                                    node: n2.clone(),
                                                });
                                            })),
                                    )
                                    .into_any_element()
                            })
                            .collect();
                        rows.push(
                            div()
                                .id("wn-add")
                                .cursor_pointer()
                                .pt(px(2.))
                                .text_size(theme::FONT_MICRO)
                                .text_color(theme::ACCENT())
                                .child("＋ 加节点")
                                .on_click(cx.listener(move |_t, _, _, cx| {
                                    cx.emit(WorldlineEvent::NodeAdd {
                                        line: lid.clone(),
                                        kind: "event".into(),
                                        label: "新节点".into(),
                                        chapter: next_ch,
                                    });
                                }))
                                .into_any_element(),
                        );
                        rows
                    })
                    // 边：这条线内部的因果连接。作者最常做的是标暗线。
                    .child(
                        div()
                            .pt(theme::GAP_SM)
                            .text_size(theme::FONT_MICRO)
                            .font_weight(FontWeight::SEMIBOLD)
                            .text_color(theme::DIM())
                            .child(SharedString::from(format!("因果边 · {}", l.edges.len()))),
                    )
                    .children({
                        let lid = l.id.clone();
                        let nodes = l.nodes.clone();
                        let mut rows: Vec<gpui_kit::AnyElement> = l
                            .edges
                            .iter()
                            .map(|e| {
                                let (f, t, k) = (e.from.clone(), e.to.clone(), e.kind.clone());
                                let (f2, t2, k2) = (f.clone(), t.clone(), k.clone());
                                let l1 = lid.clone();
                                let l2 = lid.clone();
                                div()
                                    .flex()
                                    .items_center()
                                    .gap(px(3.))
                                    .py(px(1.))
                                    .child(
                                        div()
                                            .flex_1()
                                            .min_w_0()
                                            .text_ellipsis()
                                            .text_size(theme::FONT_MICRO)
                                            .text_color(if e.hidden { theme::PURPLE() } else { theme::DIM() })
                                            .child(SharedString::from(format!(
                                                "{} → {}",
                                                e.from, e.to
                                            ))),
                                    )
                                    .child(
                                        div()
                                            .text_size(px(9.))
                                            .text_color(theme::FAINT())
                                            .child(SharedString::from(if e.kind.is_empty() {
                                                "influence".to_string()
                                            } else {
                                                e.kind.clone()
                                            })),
                                    )
                                    .child(
                                        // 明线/暗线：点一下切。暗线 = 读者还不知道的因果。
                                        div()
                                            .id(SharedString::from(format!("edge-h-{f}-{t}")))
                                            .cursor_pointer()
                                            .text_size(px(9.))
                                            .text_color(if e.hidden { theme::PURPLE() } else { theme::FAINT() })
                                            .child(if e.hidden { "暗" } else { "明" })
                                            .on_click(cx.listener(move |_t, _, _, cx| {
                                                cx.emit(WorldlineEvent::ToggleEdgeHidden {
                                                    line: l1.clone(),
                                                    from: f.clone(),
                                                    to: t.clone(),
                                                    kind: k.clone(),
                                                });
                                            })),
                                    )
                                    .child(
                                        div()
                                            .id(SharedString::from(format!("edge-x-{f2}-{t2}")))
                                            .cursor_pointer()
                                            .text_size(theme::FONT_MICRO)
                                            .text_color(theme::FAINT())
                                            .child("×")
                                            .on_click(cx.listener(move |_t, _, _, cx| {
                                                cx.emit(WorldlineEvent::RemoveEdge {
                                                    line: l2.clone(),
                                                    from: f2.clone(),
                                                    to: t2.clone(),
                                                    kind: k2.clone(),
                                                });
                                            })),
                                    )
                                    .into_any_element()
                            })
                            .collect();
                        // 加边：用最后两个节点当默认（最常见的连线是"接着往下走"）
                        if nodes.len() >= 2 {
                            let l3 = lid.clone();
                            let a = nodes[nodes.len() - 2].id.clone();
                            let b = nodes[nodes.len() - 1].id.clone();
                            rows.push(
                                div()
                                    .id("edge-add")
                                    .cursor_pointer()
                                    .pt(px(2.))
                                    .text_size(theme::FONT_MICRO)
                                    .text_color(theme::ACCENT())
                                    .child(SharedString::from(format!("＋ 连一条边（{a}→{b}）")))
                                    .on_click(cx.listener(move |_t, _, _, cx| {
                                        cx.emit(WorldlineEvent::AddEdge {
                                            line: l3.clone(),
                                            from: a.clone(),
                                            to: b.clone(),
                                            kind: "influence".into(),
                                        });
                                    }))
                                    .into_any_element(),
                            );
                        }
                        rows
                    });
            }
            None => {
                detail = detail.child(
                    div()
                        .text_size(theme::FONT_MICRO)
                        .text_color(theme::FAINT())
                        .child("选一条线看详情。实线是已经定下的剧情，虚线是后面还有的 if 空间。"),
                );
            }
        }

        // 对比视图：选两条线，看共同祖先之后各自差在哪。
        // 对比属于世界线的第二种看法（时间轴回答"有哪些世界"，对比回答"差在哪"），
        // 所以不另开页，用页头的两个小页签切。
        // —— 对比页：一条章轴上的泳道图（甘特式） ——
        //
        // 上一版是两列文字清单，占着整块画布却只能说"左边有 5 个、右边有 0 个"。
        // 对比真正要看的是**对齐**：谁在哪一章分叉、谁的关节更密、同一段
        // 两边走了多远。那必须有共同横轴，所以这里画成泳道：
        // 每行一条世界线，横轴是章，块是节点，竖线是分叉点。
        let compare_el: Option<gpui_kit::AnyElement> = if self.view == WlTab::Compare {
            self.compare_with.as_ref().map(|right| {
                let left = self
                    .selected
                    .clone()
                    .or_else(|| lines.iter().find(|l| !l.is_base()).map(|l| l.id.clone()))
                    .unwrap_or_else(|| "α".into());
                let g = crate::worldline::layout::gantt(&lines, &left, right, boundary);
                let pickable = lines.clone();
                let right_id = right.clone();
                let seg2 = |label: String, on: bool, key: String| {
                    div()
                        .id(SharedString::from(key))
                        .px(px(7.))
                        .py(px(2.))
                        .rounded(px(5.))
                        .border_1()
                        .border_color(if on { theme::ACCENT_BORDER() } else { theme::LINE() })
                        .bg(if on { theme::ACCENT_SOFT() } else { theme::PANEL2() })
                        .cursor_pointer()
                        .text_size(theme::FONT_MICRO)
                        .text_color(if on { theme::ACCENT() } else { theme::DIM() })
                        .child(SharedString::from(label))
                };
                // 横向比例：主干线性、推演区放大（和时间轴同一套口径）。
                // 不放大时 5 个预测节点全挤在右边缘 10% 里，块会互相盖住，
                // 这正是上一版"小气"的根源之一。
                let future_span = (g.to.saturating_sub(g.boundary)).max(1) as f32;
                // 主干占左 58%，推演占右 42%。
                //
                // 一开始按"边界/总章数"算（30/36 = 83%），结果是推演区只剩 17%，
                // 五个预测块照样糊在右边缘——放大等于没放。这里反过来：
                // 未来那几章本来就少，就该给它们大块地方。
                let pivot_frac: f32 = if g.boundary == 0 || g.to <= g.boundary {
                    1.
                } else {
                    0.50
                };
                let trunk_span = (g.boundary.saturating_sub(g.from)).max(1) as f32;
                let xf = move |ch: u32| -> f32 {
                    if g.boundary == 0 || ch <= g.boundary {
                        ((ch.saturating_sub(g.from)) as f32 / trunk_span).clamp(0., 1.)
                            * pivot_frac
                    } else {
                        (pivot_frac
                            + (ch - g.boundary) as f32 / future_span * (1. - pivot_frac))
                            .clamp(0., 1.)
                    }
                };
                const NAME_W: f32 = 196.;
                const LANE_H: f32 = 92.;
                const RULER_H: f32 = 28.;

                // 画布：章格、固化边界、分叉连线。
                let g_canvas = g.clone();
                let xf_c = xf;
                let gantt_body = canvas(
                    |_, _, _| {},
                    move |bounds, _, window, _| {
                        let w = bounds.size.width.as_f32();
                        let h = bounds.size.height.as_f32();
                        let ox = bounds.origin.x.as_f32();
                        let oy = bounds.origin.y.as_f32();
                        let p = |x: f32, y: f32| point(px(ox + x), px(oy + y));
                        let axis_w = (w - NAME_W - 20.).max(40.);
                        let px_of = |ch: u32| NAME_W + xf_c(ch) * axis_w;
                        let top = RULER_H;
                        let lanes = g_canvas.lanes.len();
                        let bottom = top + lanes as f32 * LANE_H;
                        let _ = h;
                        // 章格 + 每 5 章的分隔
                        let step = if g_canvas.to.saturating_sub(g_canvas.from) > 40 {
                            5
                        } else {
                            2
                        };
                        let mut ch = g_canvas.from;
                        while ch <= g_canvas.to {
                            let x = px_of(ch);
                            let mut ln = PathBuilder::stroke(px(if ch % (step * 2) == 0 {
                                1.3
                            } else {
                                1.
                            }));
                            ln.move_to(p(x, top));
                            ln.line_to(p(x, bottom));
                            if let Ok(pth) = ln.build() {
                                window.paint_path(
                                    pth,
                                    hsla(theme::LINE()).opacity(if ch == 0 { 0.6 } else { 0.4 }),
                                );
                            }
                            ch += step;
                        }
                        // 行分隔
                        for i in 0..=lanes {
                            let y = top + i as f32 * LANE_H;
                            let mut ln = PathBuilder::stroke(px(1.));
                            ln.move_to(p(NAME_W, y));
                            ln.line_to(p(w - 12., y));
                            if let Ok(pth) = ln.build() {
                                window.paint_path(pth, hsla(theme::LINE_SOFT()).opacity(0.8));
                            }
                        }
                        // 固化边界：一条明显的竖线，右边全是推演
                        {
                            let bx = px_of(g_canvas.boundary);
                            let mut ln = PathBuilder::stroke(px(1.6));
                            ln.move_to(p(bx, top - 8.));
                            ln.line_to(p(bx, bottom));
                            if let Ok(pth) = ln.build() {
                                window.paint_path(pth, hsla(theme::ACCENT()).opacity(0.7));
                            }
                        }
                        // 分叉连线：从正典行中心垂到本行，再拐向第一个块。
                        // 这就是"架构图"的依赖线，两列文字永远画不出来。
                        for (i, lane) in g_canvas.lanes.iter().enumerate() {
                            if lane.fork == 0 || i == 0 {
                                continue;
                            }
                            let y = top + i as f32 * LANE_H + LANE_H * 0.5;
                            let fx = px_of(lane.fork);
                            let first_x = lane
                                .blocks
                                .first()
                                .map(|b| px_of(b.chapter))
                                .unwrap_or(fx)
                                .max(fx);
                            let color = if lane.is_a {
                                theme::ACCENT()
                            } else if lane.is_b {
                                theme::BLUE()
                            } else {
                                theme::LINE()
                            };
                            let mut ln = PathBuilder::stroke(px(if lane.is_a || lane.is_b {
                                1.6
                            } else {
                                1.
                            }));
                            ln.move_to(p(fx, top + LANE_H * 0.5));
                            ln.line_to(p(fx, y));
                            ln.line_to(p(first_x - 4., y));
                            if let Ok(pth) = ln.build() {
                                window.paint_path(
                                    pth,
                                    hsla(color).opacity(if lane.is_a || lane.is_b { 0.6 } else { 0.35 }),
                                );
                            }
                            // 分叉点上的小圆
                            let mut dot = PathBuilder::fill();
                            for (j, (dx, dy)) in circle_pts(fx, y, 2.6).iter().enumerate() {
                                if j == 0 {
                                    dot.move_to(p(*dx, *dy));
                                } else {
                                    dot.line_to(p(*dx, *dy));
                                }
                            }
                            dot.close();
                            if let Ok(d) = dot.build() {
                                window.paint_path(d, hsla(color).opacity(0.8));
                            }
                        }
                    },
                )
                .absolute()
                .left(px(0.))
                .top(px(0.))
                .size_full();

                // 刻度尺：章号（DOM，能放字）。用固定 px 偏移把尺子对齐到轴线，
                // 不是相对整页宽度——否则刻度会和块错位一格。
                let mut ticks = div().absolute().left(px(NAME_W)).right(px(12.)).top(px(0.)).bottom(px(0.));
                let step_r = 5u32;
                let mut ch = ((g.from + step_r - 1) / step_r) * step_r;
                while ch <= g.to {
                    if xf(ch) > 0.96 {
                        break;
                    }
                    ticks = ticks.child(
                        div()
                            .absolute()
                            .left(relative(xf(ch)))
                            .top(px(4.))
                            .ml(px(-5.))
                            .text_size(px(9.5))
                            .font_family(theme::FAMILY_MONO)
                            .text_color(theme::FAINT())
                            .child(SharedString::from(format!("{ch}"))),
                    );
                    ch += step_r;
                }
                let ruler = div()
                    .h(px(RULER_H))
                    .relative()
                    .child(ticks)
                    .child(
                        div()
                            .absolute()
                            .left(relative(xf(g.boundary)))
                            .top(px(3.))
                            .ml(px(NAME_W - 16.))
                            .px(px(5.))
                            .rounded(px(3.))
                            .bg(theme::ACCENT_SOFT())
                            .text_size(px(9.5))
                            .font_weight(FontWeight::SEMIBOLD)
                            .text_color(theme::ACCENT())
                            .child("现在"),
                    );

                // 泳道
                let lanes_el = g
                    .lanes
                    .iter()
                    .map(|lane| {
                        let id = lane.id.clone();
                        let on_a = lane.is_a;
                        let on_b = lane.is_b;
                        let lane_color = if on_a {
                            theme::ACCENT()
                        } else if on_b {
                            theme::BLUE()
                        } else {
                            theme::FAINT()
                        };
                        let blocks = lane
                            .blocks
                            .iter()
                            .map(|blk| {
                                let start = xf(blk.chapter);
                                let end = xf(blk.chapter + blk.span.max(1));
                                let w = (end - start).max(0.012);
                                let (bg, fg, border) = match blk.side {
                                    Side::Both => (theme::PANEL2(), theme::DIM(), theme::LINE()),
                                    Side::A => (theme::ACCENT_SOFT(), theme::ACCENT(), theme::ACCENT_BORDER()),
                                    Side::B => (theme::BLUE_SOFT(), theme::BLUE(), theme::BLUE()),
                                    Side::Neither => (theme::PANEL2(), theme::DIM(), theme::LINE()),
                                };
                                div()
                                    .absolute()
                                    .left(relative(start))
                                    .w(relative(w))
                                    .top(px(24.))
                                    .h(px(44.))
                                    .rounded(px(4.))
                                    .border_1()
                                    .border_color(border)
                                    .bg(bg)
                                    .overflow_hidden()
                                    .flex()
                                    .items_center()
                                    .px(px(5.))
                                    .text_size(px(11.5))
                                    .text_color(fg)
                                    .text_ellipsis()
                                    .when(lane.aside, |d| d.opacity(0.4))
                                    .child(SharedString::from(blk.label.clone()))
                                    .into_any_element()
                            })
                            .collect::<Vec<_>>();
                        div()
                            .h(px(LANE_H))
                            .relative()
                            .when(on_a, |d| d.bg(theme::ACCENT_SOFT()))
                            .when(on_b, |d| {
                                d.bg(hsla(theme::BLUE()).opacity(0.10))
                            })
                            .child(
                                div()
                                    .id(SharedString::from(format!("cmp-lane-{}", id)))
                                    .absolute()
                                    .left(px(0.))
                                    .top(px(0.))
                                    .w(px(NAME_W))
                                    .h_full()
                                    .flex()
                                    .items_center()
                                    .gap(px(8.))
                                    .pl(px(8.))
                                    .cursor_pointer()
                                    .when(on_a || on_b, |d| {
                                        d.border_l_2().border_color(lane_color)
                                    })
                                    .child(
                                        div()
                                            .w(px(20.))
                                            .text_size(px(15.))
                                            .font_weight(FontWeight::SEMIBOLD)
                                            .text_color(lane_color)
                                            .child(SharedString::from(lane.id.clone())),
                                    )
                                    .child(
                                        div()
                                            .flex_1()
                                            .min_w_0()
                                            .flex()
                                            .flex_col()
                                            .gap(px(1.))
                                            .child(
                                                div()
                                                    .text_size(px(13.))
                                                    .text_color(if on_a || on_b {
                                                        theme::TEXT()
                                                    } else {
                                                        theme::DIM()
                                                    })
                                                    .text_ellipsis()
                                                    .child(SharedString::from(lane.name.clone())),
                                            )
                                            .child(
                                                div()
                                                    .text_size(px(10.5))
                                                    .font_family(theme::FAMILY_MONO)
                                                    .text_color(theme::FAINT())
                                                    .child(SharedString::from(format!(
                                                        "分叉 {} · 改 {} · 波及 {}",
                                                        if lane.fork > 0 {
                                                            format!("#{}", lane.fork)
                                                        } else {
                                                            "—".into()
                                                        },
                                                        lane.diffs,
                                                        lane.touched
                                                    ))),
                                            ),
                                    )
                                    .on_click(cx.listener(move |this, _, window, cx| {
                                        this.selected = Some(id.clone());
                                        cx.emit(WorldlineEvent::Select(id.clone()));
                                        this.fill_meta(window, cx);
                                        cx.notify();
                                    })),
                            )
                            .child(
                                div()
                                    .absolute()
                                    .left(px(NAME_W))
                                    .right(px(20.))
                                    .top(px(0.))
                                    .bottom(px(0.))
                                    .children(blocks),
                            )
                            .into_any_element()
                    })
                    .collect::<Vec<_>>();

                let legend = div()
                    .flex()
                    .items_center()
                    .gap(theme::GAP_SM)
                    .text_size(theme::FONT_MICRO)
                    .text_color(theme::FAINT())
                    .child(
                        div()
                            .px(px(6.))
                            .rounded(px(3.))
                            .border_1()
                            .border_color(theme::ACCENT_BORDER())
                            .bg(theme::ACCENT_SOFT())
                            .text_color(theme::ACCENT())
                            .child(SharedString::from(format!("{} 独有", g.a))),
                    )
                    .child(
                        div()
                            .px(px(6.))
                            .rounded(px(3.))
                            .border_1()
                            .border_color(theme::BLUE())
                            .bg(theme::BLUE_SOFT())
                            .text_color(theme::BLUE())
                            .child(SharedString::from(format!("{} 独有", g.b))),
                    )
                    .child(
                        div()
                            .px(px(6.))
                            .rounded(px(3.))
                            .border_1()
                            .border_color(theme::LINE())
                            .bg(theme::PANEL2())
                            .child("两边都有"),
                    )
                    .child(
                        div()
                            .px(px(6.))
                            .rounded(px(3.))
                            .border_1()
                            .border_color(theme::LINE())
                            .bg(theme::PANEL2())
                            .text_color(theme::DIM())
                            .child("其他线（压暗=不参与对比）"),
                    );
                let only_a = g
                    .lanes
                    .iter()
                    .filter(|l| l.is_a)
                    .flat_map(|l| l.blocks.iter())
                    .filter(|b| b.side == Side::A)
                    .count();
                let only_b = g
                    .lanes
                    .iter()
                    .filter(|l| l.is_b)
                    .flat_map(|l| l.blocks.iter())
                    .filter(|b| b.side == Side::B)
                    .count();
                let both = g
                    .lanes
                    .iter()
                    .filter(|l| l.is_a || l.is_b)
                    .flat_map(|l| l.blocks.iter())
                    .filter(|b| b.side == Side::Both)
                    .count();
                div()
                    .flex_1()
                    .min_h_0()
                    .flex()
                    .flex_col()
                    .p(theme::PAD_MD)
                    .gap(theme::GAP_SM)
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .flex_wrap()
                            .gap(px(4.))
                            .child(
                                div()
                                    .text_size(theme::FONT_MICRO)
                                    .text_color(theme::FAINT())
                                    .child("左"),
                            )
                            .children(pickable.iter().map(|l| {
                                let id = l.id.clone();
                                let on = id == left;
                                seg2(id.clone(), on, format!("cmp-a-{id}")).on_click(
                                    cx.listener(move |this, _, window, cx| {
                                        this.selected = Some(id.clone());
                                        cx.emit(WorldlineEvent::Select(id.clone()));
                                        this.fill_meta(window, cx);
                                        cx.notify();
                                    }),
                                )
                            }))
                            .child(
                                div()
                                    .px(px(5.))
                                    .text_size(theme::FONT_MICRO)
                                    .text_color(theme::FAINT())
                                    .child("vs"),
                            )
                            .children(pickable.iter().map(|l| {
                                let id = l.id.clone();
                                let on = id == right_id;
                                seg2(id.clone(), on, format!("cmp-b-{id}")).on_click(
                                    cx.listener(move |this, _, _, cx| {
                                        this.compare_with = Some(id.clone());
                                        cx.notify();
                                    }),
                                )
                            }))
                            .child(
                                div()
                                    .ml_auto()
                                    .text_size(theme::FONT_MICRO)
                                    .text_color(theme::FAINT())
                                    .child(SharedString::from(format!(
                                        "只属于 {} {} · 只属于 {} {} · 共有 {}",
                                        g.a, only_a, g.b, only_b, both
                                    ))),
                            ),
                    )
                    .child(legend)
                    // 甘特主体：固定高度（行高 × 条数），不再只占顶部一小条。
                    .child(
                        div()
                            .h(px(RULER_H + g.lanes.len() as f32 * LANE_H))
                            .relative()
                            .border_1()
                            .border_color(theme::LINE_SOFT())
                            .rounded(theme::RADIUS_PANEL)
                            .child(gantt_body)
                            .child(
                                div()
                                    .absolute()
                                    .left(px(0.))
                                    .right(px(0.))
                                    .top(px(0.))
                                    .child(ruler),
                            )
                            .child(
                                div()
                                    .absolute()
                                    .left(px(0.))
                                    .right(px(0.))
                                    .top(px(RULER_H))
                                    .children(lanes_el),
                            ),
                    )
                    // 剩下的高度给差异明细：这是"哪一章、谁独有的什么"，
                    // 甘特图看得出位置，看不出文字理由。
                    .child({
                        let rows_a = diff_rows(&g, Side::A, cx);
                        let rows_b = diff_rows(&g, Side::B, cx);
                        div()
                            .flex_1()
                            .min_h_0()
                            .flex()
                            .gap(theme::GAP_MD)
                            .child(
                                div()
                                    .flex_1()
                                    .min_w_0()
                                    .flex()
                                    .flex_col()
                                    .gap(px(3.))
                                    .child(section_title(
                                        format!("{} 独有 {}", g.a, rows_a.0),
                                        theme::ACCENT(),
                                    ))
                                    .when(rows_a.0 == 0, |d| {
                                        d.child(
                                            div()
                                                .text_size(theme::FONT_MICRO)
                                                .text_color(theme::FAINT())
                                                .child(SharedString::from(format!(
                                                    "{} 是底座：只有正文那一条，没有自己推演的节点",
                                                    g.a
                                                ))),
                                        )
                                    })
                                    .child(
                                        div()
                                            .id("cmp-only-a")
                                            .flex_1()
                                            .min_h_0()
                                            .overflow_y_scroll()
                                            .flex()
                                            .flex_col()
                                            .gap(px(2.))
                                            .children(rows_a.1),
                                    ),
                            )
                            .child(
                                div()
                                    .w(px(1.))
                                    .h_full()
                                    .bg(theme::LINE_SOFT()),
                            )
                            .child(
                                div()
                                    .flex_1()
                                    .min_w_0()
                                    .flex()
                                    .flex_col()
                                    .gap(px(3.))
                                    .child(section_title(
                                        format!("{} 独有 {}", g.b, rows_b.0),
                                        theme::BLUE(),
                                    ))
                                    .when(rows_b.0 == 0, |d| {
                                        d.child(
                                            div()
                                                .text_size(theme::FONT_MICRO)
                                                .text_color(theme::FAINT())
                                                .child(SharedString::from(format!(
                                                    "{} 没有自己的推演节点：它就是正文那一条",
                                                    g.b
                                                ))),
                                        )
                                    })
                                    .child(
                                        div()
                                            .id("cmp-only-b")
                                            .flex_1()
                                            .min_h_0()
                                            .overflow_y_scroll()
                                            .flex()
                                            .flex_col()
                                            .gap(px(2.))
                                            .children(rows_b.1),
                                    ),
                            )
                    })
                    .child(
                        // 冲突只有逐条比影响清单才看得出：同一目标被两条线
                        // 改成了不同的东西（一个 add 一个 remove）。
                        // 位置、密度都表达不了它，所以压成一行挂在页脚——
                        // 不占版面，但漏了就是事故。
                        {
                            let conflicts =
                                crate::worldline::layout::compare(&lines, &g.a, &g.b).conflicts;
                            div()
                                .text_size(theme::FONT_MICRO)
                                .text_color(if conflicts.is_empty() {
                                    theme::FAINT()
                                } else {
                                    theme::AMBER()
                                })
                                .child(SharedString::from(if conflicts.is_empty() {
                                    "无冲突：两条线没有把同一个目标改成不同的东西".to_string()
                                } else {
                                    format!("冲突 {} 处：{}", conflicts.len(), conflicts.join(" · "))
                                }))
                        },
                    )
                    .child(
                        div()
                            .text_size(theme::FONT_MICRO)
                            .text_color(theme::FAINT())
                            .child(SharedString::from(if g.ancestor.is_empty() {
                                "两条线没有共同祖先：分叉点各不相同，块的位置就是全部信息"
                                    .to_string()
                            } else {
                                format!(
                                    "共同祖先 {}：竖线处之后才开始不一样（右半边是推演）",
                                    g.ancestor
                                )
                            })),
                    )
                    .into_any_element()
            })
        } else {
            None
        };

        // 节点/事件抽屉（只在时间轴页；对比页没有点选）。
        let drawer_el = if self.view == WlTab::Timeline {
            self.drawer(&lines, boundary, cx)
        } else {
            None
        };

        div()
            .size_full()
            .flex()
            .flex_col()
            .bg(theme::BG())
            .child(
                div()
                    .flex_shrink_0()
                    .px(theme::PAD_LG)
                    .pt(theme::PAD_MD)
                    .pb(px(2.))
                    .flex()
                    .items_baseline()
                    .gap(theme::GAP_MD)
                    .child(
                        div()
                            .flex_shrink_0()
                            .text_size(theme::FONT_META)
                            .font_weight(FontWeight::SEMIBOLD)
                            .text_color(theme::TEXT())
                            .child("世界线"),
                    )
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .text_size(theme::FONT_MICRO)
                            .text_color(theme::FAINT())
                            // 这行只回答"我到底写到哪了"：边界 / 成稿两个数字。
                            // 原本的长句在 1440 宽下会折成两行、把第一行撑高，
                            // 而它要传达的信息其实就是这两个数。
                            .child(SharedString::from(format!("确定 {boundary} · 成稿 {written}")))
                            .when(boundary != written, |d| {
                                d.child(
                                    div()
                                        .ml(theme::PAD_SM)
                                        .text_size(theme::FONT_MICRO)
                                        .text_color(theme::AMBER())
                                        .child("（边界与成稿不一致）"),
                                )
                            }),
                    )
                    // 边界的来源只有一个：正文（+ 作者在这里的显式确认）。
                    // 预演线永远推不动它——这条注释就是给下一个改这里的人看的。
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap(px(6.))
                            .flex_shrink_0()
                            .py(px(3.))
                            .px(theme::PAD_SM)
                            .rounded(theme::RADIUS_CHIP)
                            .bg(theme::PANEL())
                            .border_1()
                            .border_color(theme::LINE())
                            .child(
                                div()
                                    .text_size(theme::FONT_MICRO)
                                    .text_color(theme::DIM())
                                    .child("已定剧情到第"),
                            )
                            .child(
                                div()
                                    .w(px(52.))
                                    .child(gpui_kit::component::input::Input::new(&self.ed_confirmed).xsmall()),
                            )
                            .child(
                                div()
                                    .text_size(theme::FONT_MICRO)
                                    .text_color(theme::DIM())
                                    .child("章"),
                            )
                            .child(
                                Button::new("wl-confirm")
                                    .xsmall()
                                    .label("确认")
                                    .tooltip("正文还没写到、但剧情已经定了？在这里标注边界。if 线推演不会改变它。")
                                    .on_click(cx.listener(|this, _, _, cx| {
                                        let raw = this.ed_confirmed.read(cx).value().to_string();
                                        let trimmed = raw.trim();
                                        if trimmed.is_empty() {
                                            cx.emit(WorldlineEvent::ConfirmThrough(0));
                                            return;
                                        }
                                        match trimmed.parse::<u32>() {
                                            Ok(n) => cx.emit(WorldlineEvent::ConfirmThrough(n)),
                                            Err(_) => this.set_message(
                                                "边界格式错误：填正整数章号，留空才清掉手动边界",
                                                cx,
                                            ),
                                        }
                                    })),
                            ),
                    )
                    .child(div().w(px(1.)).h(px(16.)).flex_shrink_0().bg(theme::LINE()))
                    // 模块一键能力：整理节点（能力表驱动）
                    .children({
                        let mut bar = div().flex().gap(px(4.));
                        if let Some(spec) = crate::capabilities::agent("worldline") {
                            for cap in spec.bulk {
                                bar = bar.child(
                                    Button::new(SharedString::from(cap.key))
                                        .xsmall()
                                        .label(cap.label)
                                        .on_click(cx.listener(move |_t, _, _, cx| {
                                            cx.emit(WorldlineEvent::Delegate {
                                                agent: "worldline",
                                                key: cap.key,
                                                target: None,
                                            });
                                        })),
                                );
                            }
                        }
                        vec![bar.into_any_element()]
                    })
                    .child(div().flex_1())
                    // 取景：只在时间轴这一页出现——对比页有自己的尺度，
                    // 把缩放摆在所有页上会让人以为它管全部。
                    .children({
                        let full = horizon;
                        let mut bar = div().flex().items_center().gap(px(3.));
                        if self.view == WlTab::Timeline {
                            bar = bar
                                .child(
                                    div()
                                        .text_size(theme::FONT_MICRO)
                                        .text_color(theme::FAINT())
                                        .child(SharedString::from(format!(
                                            "第{from_ch}–{to_ch}章"
                                        ))),
                                )
                                .children(
                                    [
                                        ("wl-zoom-out", "－", 1.8f32),
                                        ("wl-zoom-in", "＋", 1. / 1.8),
                                    ]
                                    .into_iter()
                                    .map(|(id, label, factor)| {
                                        Button::new(id)
                                            .xsmall()
                                            .label(label)
                                            .on_click(cx.listener(move |this, _, _, cx| {
                                                this.zoom(factor, full);
                                                cx.notify();
                                            }))
                                            .into_any_element()
                                    }),
                                )
                                .child(
                                    Button::new("wl-pan-l")
                                        .xsmall()
                                        .label("◀")
                                        .on_click(cx.listener(move |this, _, _, cx| {
                                            let span = this
                                                .window
                                                .map(|(a, b)| (b - a).max(1))
                                                .unwrap_or(full.max(12));
                                            this.pan(-(span as i32 / 3).max(1), full);
                                            cx.notify();
                                        })),
                                )
                                .child(
                                    Button::new("wl-pan-r")
                                        .xsmall()
                                        .label("▶")
                                        .on_click(cx.listener(move |this, _, _, cx| {
                                            let span = this
                                                .window
                                                .map(|(a, b)| (b - a).max(1))
                                                .unwrap_or(full.max(12));
                                            this.pan((span as i32 / 3).max(1), full);
                                            cx.notify();
                                        })),
                                )
                                .when(self.is_zoomed(full), |d| {
                                    d.child(
                                        Button::new("wl-fit")
                                            .xsmall()
                                            .label("全部")
                                            .on_click(cx.listener(move |this, _, _, cx| {
                                                this.window = None;
                                                cx.notify();
                                            })),
                                    )
                                });
                        }
                        vec![bar.into_any_element()]
                    })
                    .child(
                        // 视图切换：时间轴 / 对比（同一份数据的两个镜头）
                        div()
                            .flex()
                            .gap(px(4.))
                            .child(tab_chip(
                                "wl-tab-time",
                                "时间轴",
                                self.view == WlTab::Timeline,
                                cx,
                                |this| {
                                    this.view = WlTab::Timeline;
                                },
                            ))
                            .child(tab_chip(
                                "wl-tab-cmp",
                                "对比",
                                self.view == WlTab::Compare,
                                cx,
                                |this| {
                                    if this.compare_with.is_none() {
                                        this.compare_with = Some("α".into());
                                    }
                                    this.view = WlTab::Compare;
                                    this.picked = None;
                                },
                            )),
                    ),
            )
            // 操作反馈条：成功绿、失败琥珀，可一键关闭。宿主的所有写操作结果都落在这里。
            .when_some(msg, |d, m| {
                let c = if theme::feedback_is_error(&m) {
                    theme::AMBER()
                } else {
                    theme::GREEN()
                };
                d.child(
                    div()
                        .flex_shrink_0()
                        .mx(theme::PAD_LG)
                        .mt(px(2.))
                        .px(theme::PAD_SM)
                        .py(px(3.))
                        .rounded(theme::RADIUS_CHIP)
                        .bg(theme::PANEL())
                        .border_1()
                        .border_color(c)
                        .flex()
                        .items_center()
                        .gap(theme::GAP_SM)
                        .child(
                            div()
                                .flex_1()
                                .min_w_0()
                                .text_size(theme::FONT_MICRO)
                                .text_color(c)
                                .child(m),
                        )
                        .child(
                            div()
                                .id("wl-msg-x")
                                .cursor_pointer()
                                .px(px(2.))
                                .text_size(theme::FONT_MICRO)
                                .text_color(theme::FAINT())
                                .child("×")
                                .on_click(cx.listener(|this, _, _, cx| {
                                    this.message = None;
                                    cx.notify();
                                })),
                        ),
                )
            })
            .when_some(compare_el, |d, cmp| d.child(cmp))
            .when(self.view == WlTab::Timeline, |d| {
                // 画布 + 详情**并排**，不是浮层压上去。
                //
                // 浮层试过两版：贴右上盖掉整个扇形；贴左下再限高，详情被削到
                // 只能看见前两段、操作按钮要滚动才够得着。这一页整幅都是数据
                // （主干、扇束、束名），压哪儿都是压。
                //
                // 改成 flex 兄弟后，选中某条线时画布让出 300px，扇束按新宽度
                // 重新排布——没有数据被遮住，只是"看近了"。
                d.child(
                    div()
                        .flex_1()
                        .min_h_0()
                        .w_full()
                        .flex()
                      .child(
                        div()
                        .flex_1()
                        .min_w_0()
                        .h_full()
                        .relative()
                        .child(timeline)
                        // 左上：线列表（紧凑，可折叠）
                        .child(
                            div()
                                .id("wl-lines")
                                .absolute()
                                .left(px(14.))
                                .top(px(14.))
                                .w(px(196.))
                                .max_h(relative(0.62))
                                .overflow_y_scroll()
                                .p(px(6.))
                                .rounded(theme::RADIUS_PANEL)
                                .border_1()
                                .border_color(theme::LINE())
                                .bg(theme::PANEL())
                                .flex()
                                .flex_col()
                                .gap(px(1.))
                                .children(track_rows),
                        )
                        .when_some(drawer_el, |d, dr| d.child(dr)),
                      )
                        .when(self.selected.is_some(), |d| {
                            d.child(
                                div()
                                    .id("wl-detail")
                                    .w(px(300.))
                                    .flex_shrink_0()
                                    .h_full()
                                    .overflow_y_scroll()
                                    .px(theme::PAD_MD)
                                    .py(theme::PAD_SM)
                                    .border_l_1()
                                    .border_color(theme::LINE())
                                    .bg(theme::PANEL())
                                    .child(detail),
                            )
                        })
                )
            })
    }
}

impl BasePanel for WorldlinePanel {
    fn panel_name(&self) -> &'static str {
        "worldline-view"
    }
}

impl ComponentPanel for WorldlinePanel {
    fn tab_name(&self, _: &App) -> Option<SharedString> {
        Some("世界线".into())
    }

    fn title(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div()
            .text_size(theme::FONT_BODY)
            .font_weight(FontWeight::MEDIUM)
            .text_color(theme::TEXT())
            .child("世界线 · 时间轴")
    }
}

/// 一个带标题的输入框。与 views.rs 里同名函数同形状——
/// 各面板各有一份是因为 views 里的那个是私有的，而这里只需要这一个形状。
fn field(label: &str, state: &Entity<gpui_kit::component::input::InputState>) -> gpui_kit::AnyElement {
    div()
        .flex()
        .flex_col()
        .gap(px(1.))
        .child(
            div()
                .text_size(theme::FONT_MICRO)
                .text_color(theme::FAINT())
                .child(SharedString::from(label.to_string())),
        )
        .child(gpui_kit::component::input::Input::new(state))
        .into_any_element()
}

/// 一个 18px 的隐形命中区：点一下开抽屉（再点同一个收起）。
/// 坐标用归一化比例（fx 相对轴线宽、fy 相对画布高），与 paint 同源。
fn hit_dot(
    id: SharedString,
    fx: f32,
    fy: f32,
    target: Pick,
    cx: &mut Context<WorldlinePanel>,
) -> gpui_kit::AnyElement {
    div()
        .id(id)
        .absolute()
        .left(relative(fx.clamp(0., 1.)))
        .top(relative(fy.clamp(0., 1.)))
        .ml(px(-9.))
        .mt(px(-9.))
        .w(px(18.))
        .h(px(18.))
        .cursor_pointer()
        .on_click(cx.listener(move |this, _, _, cx| {
            let same = this.picked.as_ref() == Some(&target);
            this.picked = if same { None } else { Some(target.clone()) };
            cx.notify();
        }))
        .into_any_element()
}

/// 圆周采样点（多边形逼近）。
///
/// 为什么不用 `cubic_bezier_to` 画圆：这个 gpui 版本的 PathBuilder 对
/// 连续三次贝塞尔的处理有问题——曲线会被画成两段，圆会碎成几个弧片
/// （表现为"节点变成 ✗ 形"）。折线采样没有这个毛病，18 段肉眼看不出棱角。
fn circle_pts(cx: f32, cy: f32, r: f32) -> Vec<(f32, f32)> {
    const N: usize = 18;
    (0..N)
        .map(|i| {
            let a = std::f32::consts::TAU * i as f32 / N as f32;
            (cx + r * a.cos(), cy + r * a.sin())
        })
        .collect()
}

/// 差异明细的一列：只属于 A（或 B）的块，按章号排。
///
/// 甘特图表达"在哪一章、靠左靠右"，表达不了"这一块叫什么、挨着谁"。
/// 所以下面留一栏逐条读——两张图各管一半，不是重复。
fn diff_rows(
    g: &crate::worldline::layout::Gantt,
    side: Side,
    cx: &mut Context<WorldlinePanel>,
) -> (usize, Vec<gpui_kit::AnyElement>) {
    let mut out = Vec::new();
    let mut n = 0usize;
    for lane in g.lanes.iter() {
        if !(lane.is_a || lane.is_b) {
            continue;
        }
        for b in lane.blocks.iter().filter(|b| b.side == side) {
            n += 1;
            let _ = cx;
            out.push(
                div()
                    .flex()
                    .items_center()
                    .gap(px(6.))
                    .child(
                        div()
                            .w(px(40.))
                            .flex_shrink_0()
                            .font_family(theme::FAMILY_MONO)
                            .text_size(px(10.))
                            .text_color(theme::FAINT())
                            .child(SharedString::from(format!("#{}", b.chapter))),
                    )
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .text_ellipsis()
                            .text_size(px(11.))
                            .text_color(theme::TEXT())
                            .child(SharedString::from(b.label.clone())),
                    )
                    .into_any_element(),
            );
        }
    }
    (n, out)
}

/// 一栏的小标题（颜色区分 A / B）。
fn section_title(text: String, color: gpui_kit::Rgba) -> gpui_kit::AnyElement {
    div()
        .text_size(theme::FONT_LABEL)
        .font_weight(FontWeight::SEMIBOLD)
        .text_color(color)
        .child(SharedString::from(text))
        .into_any_element()
}

/// 视图切换的小胶囊（时间轴 / 对比）。
fn tab_chip(
    id: &'static str,
    label: &'static str,
    on: bool,
    cx: &mut Context<WorldlinePanel>,
    apply: fn(&mut WorldlinePanel),
) -> gpui_kit::AnyElement {
    div()
        .id(id)
        .px(px(9.))
        .py(px(3.))
        .rounded(px(6.))
        .border_1()
        .border_color(if on { theme::ACCENT_BORDER() } else { theme::LINE() })
        .bg(if on { theme::ACCENT_SOFT() } else { theme::PANEL2() })
        .cursor_pointer()
        .text_size(theme::FONT_MICRO)
        .text_color(if on { theme::ACCENT() } else { theme::DIM() })
        .child(label)
        .on_click(cx.listener(move |this, _, _, cx| {
            apply(this);
            cx.notify();
        }))
        .into_any_element()
}

/// 时间轴的线色：一条世界线一个颜色。
///
/// 为什么按下标而不是按 kind：kind 只有四类，线一多必然撞色；
/// 而"不同的世界发展线路"要能一眼分开，所以按线的顺序取一轮调色板。
/// 正典固定蓝（它是基准，不该跟别的线抢色）。
fn track_color(t: &Track) -> gpui_kit::Rgba {
    if t.kind == Kind::Base {
        theme::BLUE()
    } else if t.archived {
        theme::FAINT()
    } else if t.kind == Kind::Forecast {
        theme::GREEN()
    } else {
        theme::PURPLE()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::worldline::Node;

    /// 正典基准 + 一条推到第 30 章的预演线。
    fn lines_with_forecast() -> Vec<Line> {
        let alpha = Line::new("α", "正典", Kind::Base, "", "", 0);
        let mut fc = Line::new("β", "推演", Kind::Forecast, "α", "n1", 12);
        let mut n = Node::new("n2", "事件", "第30章那个关节");
        n.chapter = 30;
        fc.nodes.push(n);
        vec![alpha, fc]
    }

    #[test]
    fn auto_window_leaves_a_readable_span_before_the_boundary() {
        // 边界 20 章：左端给 14 章正文做参照（20-14=6），右端至少留到边界+6。
        let (from, to) = WorldlinePanel::auto_window(&[], 20);
        assert_eq!(from, 6, "左端应为 boundary-14");
        assert!(to >= 26, "右端至少 boundary+6，实际 {to}");
        assert!(to > from, "跨度必须为正——跨度塌成 1 章就是把视野压成一根竖线");
    }

    #[test]
    fn auto_window_still_spans_when_the_book_is_empty() {
        // 新书：边界 0。左端 saturating_sub 不能下溢，跨度仍要够读。
        let (from, to) = WorldlinePanel::auto_window(&[], 0);
        assert_eq!(from, 0);
        assert!(to >= 6, "空书也要给足 6 章的取景，否则时间轴上看不到东西");
    }

    #[test]
    fn auto_window_reaches_the_furthest_forecast() {
        // 预演推到第 30 章而边界只有 20：右端必须越过 30，
        // 否则"推得最远的那条线"被裁在视野外（这正是 fan::horizon 存在的理由）。
        let lines = lines_with_forecast();
        let (from, to) = WorldlinePanel::auto_window(&lines, 20);
        assert_eq!(from, 6);
        assert!(to > 30, "右端要容下第 30 章的预演节点，实际 {to}");
    }

    #[test]
    fn goto_chapter_would_never_run_on_a_one_chapter_span() {
        // 跨页跳转的真实现场：面板还没渲染过 → default_window 仍是 (0,0)。
        // 若直接拿 (0,0) 当基准算跨度，span 会退化成 1，focus_window 把目标
        // 硬塞进 1 章宽的框里。这里断言「未渲染时先算默认框」这条路是通的：
        // 用 auto_window 作基准，聚焦第 25 章后视野跨度仍然可读。
        let lines = lines_with_forecast();
        let boundary = 20;
        let base = WorldlinePanel::auto_window(&lines, boundary);
        let full = 40;
        let (from, to) = crate::worldline::layout::focus_window(Some(base), 25, full);
        assert!(to - from >= 8, "跳过去之后跨度不能塌：{from}..{to}");
        assert!(25 >= from && 25 <= to, "目标章必须落在视野内：{from}..{to}");
    }
}
