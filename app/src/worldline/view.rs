//! 世界线页：剧情的时间轴 + if 分支。
//!
//! 这一页回答一个问题：**"故事确定到哪儿了，之后有哪些可能？"**
//!
//! 与另外两页的分工（这三页最容易混，写在这里当下锚点）：
//! - **世界线（本页）**：时间轴。横轴 = 章节。已固化的剧情是实线，
//!   之后的开放段是虚线；每条 if 线从它的分叉章长出。回答"有哪些世界、从哪分叉"。
//! - **画布**：关系网。节点 = 人物/设定/道具/事件，边 = 人物关系与剧情关联。
//!   回答"这个关节跟谁有关"。via 双向弱引用可跳到本页的具体章节。
//! - **预言书**：if 工坊。选中一个固化点，在这里写假设、让 AI 推演、
//!   看影响清单、把某条 if 扶正。回答"这条假设要不要收进正典"。
//!
//! 为什么拆出来：旧的实现把这三件事全塞进 `OraclePanel`（"预言书"），
//! 结果"世界线"这个词在侧栏出现两次、而真正的世界线没有自己的页面。

use std::collections::BTreeMap;
use std::rc::Rc;

use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::dock::{BasePanel, Panel as ComponentPanel, PanelEvent};
use gpui_kit::component::Sizable;
use gpui_kit::prelude::FluentBuilder;
use gpui_kit::{
    App, AppContext as _, Context, Entity, EventEmitter, FocusHandle, Focusable, FontWeight,
    InteractiveElement, IntoElement, ParentElement, PathBuilder, Render, SharedString,
    StatefulInteractiveElement, Styled, Window, canvas, div, point, px, relative,
};

use crate::store::ProjectHandle;

/// Rgba → Hsla（画布路径与 Tag 需要 Hsla）。
fn hsla(c: gpui_kit::Rgba) -> gpui_kit::Hsla {
    c.into()
}
use crate::theme;
use crate::worldline::layout::{bands, forks_at, segments, tracks, Band, Track};
use crate::worldline::{Kind, Line, Store};

/// 世界线的三种看法。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WlTab {
    Timeline,
    Tree,
    Compare,
}

/// 世界线页的事件。写操作全部交给宿主，页面不碰磁盘。
#[derive(Debug, Clone)]
pub enum WorldlineEvent {
    /// 选中某条线（联动右侧详情与预言书）
    Select(String),
    /// 从某条线的第 N 章分叉
    Fork { from: String, chapter: u32 },
    /// 把某条 if 线扶正为正典后续
    Fix(String),
    /// 撤销扶正
    Unfix(String),
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
    /// 折叠的轨道（作者线多了要收起来）
    collapsed: std::collections::BTreeSet<String>,
    /// 世界树的钻取根（点两次同一节点 = 钻进它的子树）
    drill: Option<String>,
    /// 时间轴上点开的节点（线 id, 节点 id）。`Some` = 抽屉打开。
    ///
    /// 为什么是"点节点开抽屉"而不是把节点全塞进右侧详情：右侧详情是**这条线**
    /// 的信息（名字、假设、扶正/归档），节点是**线上的一个点**。两者混在一起，
    /// 作者点一个点就会把整条线的操作挤掉。抽屉只回答"这个点是什么、能对它做什么"。
    picked: Option<(String, String)>,
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
        Self {
            focus_handle: cx.focus_handle(),
            handle,
            selected: None,
            window: None,
            ed_name,
            ed_premise,
            collapsed: Default::default(),
            drill: None,
            picked: None,
            message: None,
            view: match std::env::var("ZHIBI_WL_TAB").as_deref() {
                Ok("tree") => WlTab::Tree,
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

    /// 抽屉指向的节点还在不在。不在就清掉——否则删完节点再重新渲染，
    /// 抽屉里那块区域会一直是空的（`and_then` 返回 None，什么都不画，
    /// 但状态的 picked 还挂着，点别的点时会"先收起再展开"多一次闪）。
    fn prune_picked(&mut self, lines: &[Line]) {
        let Some((lid, nid)) = &self.picked else { return };
        let alive = lines
            .iter()
            .find(|l| &l.id == lid)
            .map(|l| l.nodes.iter().any(|n| &n.id == nid))
            .unwrap_or(false);
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
    fn horizon(&self, lines: &[Line], boundary: u32, written: u32) -> u32 {
        lines
            .iter()
            .map(|l| l.last_chapter())
            .max()
            .unwrap_or(0)
            .max(boundary)
            .max(written)
            + 6
    }

    /// 缩放（算术在 `layout::zoom_window`，那里有测试）。
    fn zoom(&mut self, factor: f32, full: u32) {
        self.window = Some(crate::worldline::layout::zoom_window(self.window, factor, full));
    }

    /// 平移（算术在 `layout::pan_window`）。
    fn pan(&mut self, chapters: i32, full: u32) {
        self.window = Some(crate::worldline::layout::pan_window(self.window, chapters, full));
    }

    /// 取景框是否已偏离全览（决定要不要显示「全部」按钮）。
    fn is_zoomed(&self, full: u32) -> bool {
        match self.window {
            Some((a, b)) => a > 0 || b < full.max(12),
            None => false,
        }
    }

    /// 把视野挪到某一章（点节点 / 新建线后自动跟过去）。
    fn focus_chapter(&mut self, ch: u32, full: u32) {
        self.window = Some(crate::worldline::layout::focus_window(self.window, ch, full));
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
        let store = Store::new(&self.handle.root);
        let boundary = store.fixed_boundary(&lines);
        let written = self.written_through();

        // 视野：从 0 到「边界与最远节点取大」再留 6 章余量——作者要看见"前面还有路"。
        // 缩放/平移都是在这个默认范围上取一个子区间（self.window）。
        let horizon = self.horizon(&lines, boundary, written);
        let (from_ch, to_ch) = self.window.unwrap_or((0, horizon.max(12)));
        let span = (to_ch.saturating_sub(from_ch)).max(1);

        let tr = tracks(&lines, boundary);
        let mut segs = segments(&tr, to_ch);
        // 折叠：收起的轨道只留固化段（开放段是"这条线还想干什么"，收起即不想看）
        segs.retain(|sg| !self.collapsed.contains(&sg.track) || sg.fixed);
        let band_list = self.stage_bands();

        // —— 左：轨道列表 ——
        let mut track_rows: Vec<gpui_kit::AnyElement> = Vec::new();
        for t in &tr {
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
        let segs_c = segs.clone();
        let bands_c = band_list.clone();
        let tr_c = tr.clone();
        // 节点带上完整状态：时间轴上要区分「已写」（绿实心）、「已采纳但正文还没写到」
        // （蓝实心）、「仅计划」（空心）。只传一个 bool 会把前两者混成一样。
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
        let nodes_c = nodes_by_track.clone();
        // 命中区要用另一份：paint 闭包 move 走了 nodes_c / tr_c
        let hits_nodes = nodes_by_track.clone();
        let hits_tr = tr.clone();
        let labels_tr = tr.clone();
        let picked_c = self.picked.clone();
        let sel = self.selected.clone();
        // —— 扇形布局 ——
        //
        // 时间轴要表达的是"前面是一条线，后面是无数种可能"：
        // 正典是一根横贯的主干，每条 if 从它的分叉章弯出去，然后各自平走。
        // 旧实现把每条线摆成一行平行的车道——那看着像五条并列的线，
        // 表达不出"从一条线上长出来的"这层关系。
        let mut branches: Vec<&Track> = tr_c
            .iter()
            .filter(|t| !(t.kind == Kind::Base || t.id == "α"))
            .collect();
        branches.sort_by(|a, b| {
            a.fork_chapter
                .cmp(&b.fork_chapter)
                .then(a.id.cmp(&b.id))
        });
        // 先只算"往上几层/往下几层"，再定主干高度。
        // 顺序反了的话向上分支会落到负 y（画布外），只剩一段斜线露出画面下沿——
        // 看起来像"线歪了"，实际是坐标越界。
        let mut slots: Vec<(String, f32)> = Vec::new();
        let mut up = 0.0f32;
        let mut down = 0.0f32;
        for (i, t) in branches.iter().enumerate() {
            let off = if i % 2 == 0 {
                up += 1.;
                -up
            } else {
                down += 1.;
                down
            };
            slots.push((t.id.clone(), off));
        }
        let _ = (up, down);
        // —— 时间轴：**整页**画布 ——
        //
        // 世界线这一页只干一件事：让作者看见"现在是一条线，前面是无数种可能"。
        // 所以画布铺满整个内容区，轨道列表与详情都做成浮层——不再各占一条边。
        //
        // 版式参照定稿的线构图：左=现在，向右=未来；确定的是实线、可能是虚线；
        // 每个节点是一颗有光晕的点，分叉是平滑外弯的曲线。
        let fractions = {
            let mut m: BTreeMap<String, f32> = BTreeMap::new();
            let max_off = slots.iter().map(|(_, o)| o.abs()).fold(0.0f32, f32::max).max(1.0);
            // 纵向只用中间 84%：上下留出标题与图例
            let step = 0.42 / max_off;
            for (id, off) in &slots {
                m.insert(id.clone(), 0.5 + off * step);
            }
            m
        };
        let frac_c = fractions.clone();
        let y_of_frac = move |id: &str, is_base: bool| -> f32 {
            if is_base {
                0.5
            } else {
                frac_c.get(id).copied().unwrap_or(0.5)
            }
        };
        let frac_lbl = fractions.clone();

        let timeline = div()
            .absolute()
            .size_full()
            .child(
                canvas(
                    |_, _, _| {},
                    move |bounds, _, window, _| {
                        let w = bounds.size.width.as_f32();
                        let h = bounds.size.height.as_f32();
                        let ox = bounds.origin.x.as_f32();
                        let oy = bounds.origin.y.as_f32();
                        let p = |x: f32, y: f32| point(px(ox + x), px(oy + y));
                        let pad_l = 34.0f32;
                        let pad_r = 26.0f32;
                        let axis_w = (w - pad_l - pad_r).max(1.);
                        let x_of = |ch: u32| {
                            let rel = (ch.saturating_sub(from_ch)) as f32 / span as f32;
                            pad_l + rel.clamp(0., 1.) * axis_w
                        };
                        let y_of = |id: &str, is_base: bool| y_of_frac(id, is_base) * h;

                        // —— 深空底：星点 + 从"现在"散开的轨道弧 ——
                        //
                        // 确定性伪随机（LCG），不用 rand：同一本书每次打开星图一样，
                        // 不会因为随机种子让画面每次"抖"一下。
                        let mut seed = 0x2545_f491u32;
                        let mut next = || {
                            seed = seed.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
                            (seed >> 8) as f32 / 16_777_216.0
                        };
                        for _ in 0..110 {
                            let x = next() * w;
                            let y = next() * h;
                            let a = 0.03 + next() * 0.09;
                            let r = 0.6 + next() * 1.1;
                            let mut dot = PathBuilder::fill();
                            let k = 0.5523 * r;
                            dot.move_to(p(x + r, y));
                            dot.cubic_bezier_to(p(x + k, y + r), p(x, y + r), p(x + r, y + k));
                            dot.cubic_bezier_to(p(x - r, y + k), p(x - r, y), p(x - k, y + r));
                            dot.cubic_bezier_to(p(x - k, y - r), p(x, y - r), p(x - r, y - k));
                            dot.cubic_bezier_to(p(x + r, y - k), p(x + r, y), p(x + k, y - r));
                            if let Ok(path) = dot.build() {
                                window.paint_path(path, hsla(theme::TEXT()).opacity(a));
                            }
                        }
                        // 装饰性的轨道弧不画：它和"这是一条线"的视觉语言冲突——
                        // 试过一版从"现在"发散的同心弧，作者第一眼以为那是几条世界线。
                        // 世界线页只该有数据，不该有会冒充数据的装饰。

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

                        // 章节刻度：每 5 章一道细线
                        let mut tick = from_ch;
                        while tick <= to_ch {
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

                        // —— 线本身 ——
                        //
                        // 每条线画两遍：先一条宽而淡的（光晕），再一条细而实的。
                        // 这是最省事的"发光"——真滤波在 GPUI 里没有现成 API。
                        for seg in segs_c.iter() {
                            let Some(t) = tr_c.iter().find(|t| t.id == seg.track) else {
                                continue;
                            };
                            let is_base = t.kind == Kind::Base || t.id == "α";
                            let y = y_of(&t.id, is_base);
                            let x0 = x_of(seg.from);
                            let x1 = x_of(seg.to);
                            if x1 <= x0 {
                                continue;
                            }
                            let color = track_color(t);
                            let on = sel.as_deref() == Some(t.id.as_str());
                            let fork_ch = t.fork_chapter.max(1);
                            let mut path = PathBuilder::stroke(px(if on { 2.6 } else { 1.9 }));
                            if !seg.fixed {
                                path = path.dash_array(&[px(5.), px(5.)]);
                            }
                            if !is_base && seg.from == fork_ch {
                                // 从父线的高度平滑地弯到自己这一层
                                let fx = x_of(fork_ch);
                                let py = tr_c
                                    .iter()
                                    .find(|p2| p2.id == t.parent_id)
                                    .map(|p2| {
                                        y_of(&p2.id, p2.kind == Kind::Base || p2.id == "α")
                                    })
                                    .unwrap_or(h * 0.5);
                                let join = fx + 70.;
                                path.move_to(p(fx, py));
                                path.cubic_bezier_to(p(join, y), p(fx + 30., py), p(fx + 40., y));
                                if x1 > join {
                                    path.line_to(p(x1, y));
                                }
                            } else {
                                path.move_to(p(x0, y));
                                path.line_to(p(x1, y));
                            }
                            if let Ok(path) = path.build() {
                                let op = if seg.fixed { 1.0 } else { 0.62 };
                                window.paint_path(path, hsla(color).opacity(op));
                            }
                        }

                        // 节点：圆形 + 外圈光晕；已写实心、已采纳蓝实心、计划空心
                        for t in tr_c.iter() {
                            let Some(ns) = nodes_c.get(&t.id) else { continue };
                            let is_base = t.kind == Kind::Base || t.id == "α";
                            let y = y_of(&t.id, is_base);
                            let color = track_color(t);
                            for (ch, nid, _label, state) in ns {
                                if *ch < from_ch || *ch > to_ch {
                                    continue;
                                }
                                let x = x_of(*ch);
                                let picked = picked_c
                                    .as_ref()
                                    .map(|(l, n)| l == &t.id && n == nid)
                                    .unwrap_or(false);
                                let halo = if picked { 9.0 } else { 7.0 };
                                // 光晕
                                let mut g = PathBuilder::fill();
                                let k = 0.5523 * halo;
                                g.move_to(p(x + halo, y));
                                g.cubic_bezier_to(p(x + k, y + halo), p(x, y + halo), p(x + halo, y + k));
                                g.cubic_bezier_to(p(x - halo, y + k), p(x - halo, y), p(x - k, y + halo));
                                g.cubic_bezier_to(p(x - k, y - halo), p(x, y - halo), p(x - halo, y - k));
                                g.cubic_bezier_to(p(x + halo, y - k), p(x + halo, y), p(x + k, y - halo));
                                if let Ok(path) = g.build() {
                                    let c = if picked { theme::ACCENT() } else { color };
                                    window.paint_path(path, hsla(c).opacity(0.16));
                                }
                                // 本体
                                let r = if picked { 5.2 } else { 4.0 };
                                let k = 0.5523 * r;
                                if state.is_canon() {
                                    let fill = if *state == crate::worldline::NodeState::Written {
                                        theme::GREEN()
                                    } else {
                                        theme::BLUE()
                                    };
                                    let mut d = PathBuilder::fill();
                                    d.move_to(p(x + r, y));
                                    d.cubic_bezier_to(p(x + k, y + r), p(x, y + r), p(x + r, y + k));
                                    d.cubic_bezier_to(p(x - r, y + k), p(x - r, y), p(x - k, y + r));
                                    d.cubic_bezier_to(p(x - k, y - r), p(x, y - r), p(x - r, y - k));
                                    d.cubic_bezier_to(p(x + r, y - k), p(x + r, y), p(x + k, y - r));
                                    if let Ok(path) = d.build() {
                                        window.paint_path(path, hsla(fill));
                                    }
                                } else {
                                    let mut d = PathBuilder::stroke(px(1.3));
                                    d.move_to(p(x + r, y));
                                    d.cubic_bezier_to(p(x + k, y + r), p(x, y + r), p(x + r, y + k));
                                    d.cubic_bezier_to(p(x - r, y + k), p(x - r, y), p(x - k, y + r));
                                    d.cubic_bezier_to(p(x - k, y - r), p(x, y - r), p(x - r, y - k));
                                    d.cubic_bezier_to(p(x + r, y - k), p(x + r, y), p(x + k, y - r));
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
            // 固化边界标注
            .when(boundary > 0, |d| {
                d.child(
                    div()
                        .absolute()
                        .left(relative(
                            ((boundary.saturating_sub(from_ch)) as f32 / span as f32).clamp(0., 1.),
                        ))
                        .top(px(22.))
                        .ml(px(34.))
                        .text_size(theme::FONT_MICRO)
                        .font_family(theme::FAMILY_MONO)
                        .text_color(theme::ACCENT())
                        .child(SharedString::from(format!("固化至 {boundary}"))),
                )
            })
            // 线名：贴在每根线的右端
            .children(labels_tr.iter().filter_map(|t| {
                if t.kind == Kind::Base || t.id == "α" {
                    return None;
                }
                let f = frac_lbl.get(&t.id).copied()?;
                let id = t.id.clone();
                let color = track_color(t);
                let on = self.selected.as_deref() == Some(t.id.as_str());
                let pick = t.id.clone();
                Some(
                    div()
                        .id(SharedString::from(format!("wllbl-{}", t.id)))
                        .absolute()
                        .right(px(20.))
                        .top(relative(f))
                        .mt(px(-8.))
                        .cursor_pointer()
                        .px(px(4.))
                        .rounded(px(3.))
                        .bg(theme::BG())
                        .font_family(theme::FAMILY_MONO)
                        .text_size(px(10.))
                        .text_color(if on { theme::ACCENT() } else { color })
                        .child(SharedString::from(id))
                        .on_click(cx.listener(move |this, _, window, cx| {
                            this.selected = Some(pick.clone());
                            cx.emit(WorldlineEvent::Select(pick.clone()));
                            this.fill_meta(window, cx);
                            cx.notify();
                        }))
                        .into_any_element(),
                )
            }))
            // 节点命中区：点节点 → 抽屉
            .child(
                div()
                    .absolute()
                    .left(px(34.))
                    .right(px(26.))
                    .top_0()
                    .bottom_0()
                    .children({
                        let mut hits: Vec<gpui_kit::AnyElement> = Vec::new();
                        for t in hits_tr.iter() {
                            let Some(ns) = hits_nodes.get(&t.id) else { continue };
                            let is_base = t.kind == Kind::Base || t.id == "α";
                            let f = if is_base {
                                0.5
                            } else {
                                frac_lbl.get(&t.id).copied().unwrap_or(0.5)
                            };
                            for (ch, nid, label, _state) in ns {
                                if *ch < from_ch || *ch > to_ch {
                                    continue;
                                }
                                let ratio = (*ch).saturating_sub(from_ch) as f32 / span as f32;
                                let hid = SharedString::from(format!("wlhit-{}-{nid}", t.id));
                                let (lid, nid2, label2) = (t.id.clone(), nid.clone(), label.clone());
                                hits.push(
                                    div()
                                        .id(hid)
                                        .absolute()
                                        .left(relative(ratio.clamp(0., 1.)))
                                        .top(relative(f))
                                        .ml(px(-9.))
                                        .mt(px(-9.))
                                        .w(px(18.))
                                        .h(px(18.))
                                        .cursor_pointer()
                                        .on_click(cx.listener(move |this, _, _, cx| {
                                            let same = this
                                                .picked
                                                .as_ref()
                                                .map(|(a, b)| a == &lid && b == &nid2)
                                                .unwrap_or(false);
                                            this.picked = if same {
                                                None
                                            } else {
                                                Some((lid.clone(), nid2.clone()))
                                            };
                                            let _ = &label2;
                                            cx.notify();
                                        }))
                                        .into_any_element(),
                                );
                            }
                        }
                        hits
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
                    Kind::Fixed => "已扶正：作者从 if 里选定、正式收进正典后续的那一版。",
                    Kind::Forecast => "预测线：按大纲/成稿往前推演「接下来会怎样」。还没定。",
                    Kind::WhatIf => "假设线：如果某件事不一样会怎样。还没定。",
                };
                let forks_here = forks_at(&lines, l.fork_chapter());
                let l2 = l.id.clone();
                let l3 = l.id.clone();
                let l4 = l.id.clone();
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
                            .when(!l.is_base() && l.kind != Kind::Fixed, |d| {
                                d.child(
                                    Button::new("wl-fix")
                                        .primary()
                                        .xsmall()
                                        .label("扶正为正典后续")
                                        .on_click(cx.listener(move |_this, _, _, cx| {
                                            cx.emit(WorldlineEvent::Fix(l3.clone()));
                                        })),
                                )
                            })
                            .when(l.kind == Kind::Fixed, |d| {
                                d.child(
                                    Button::new("wl-unfix")
                                        .xsmall()
                                        .label("撤销扶正")
                                        .on_click(cx.listener(move |_this, _, _, cx| {
                                            cx.emit(WorldlineEvent::Unfix(l4.clone()));
                                        })),
                                )
                            })
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
                            .take(20)
                            .map(|n| {
                                let ch = n.chapter_or_guess();
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
                                            .w(px(42.))
                                            .flex_shrink_0()
                                            .cursor_pointer()
                                            .font_family(theme::FAMILY_MONO)
                                            .text_size(theme::FONT_MICRO)
                                            .text_color(theme::FAINT())
                                            .child(SharedString::from(if ch > 0 {
                                                format!("第{ch}章")
                                            } else {
                                                "排章".into()
                                            }))
                                            .on_click(cx.listener(move |_t, _, _, cx| {
                                                cx.emit(WorldlineEvent::NodePatch {
                                                    line: l1.clone(),
                                                    node: n1.clone(),
                                                    patch: crate::worldline::NodePatch::chapter(ch + 1),
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
                            .take(12)
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
        // —— 世界树：纵轴 = 章节，横轴按子树宽度分配 ——
        //
        // 与时间轴的分工：时间轴说"哪些章定了、之后有什么"，
        // 树说"这些 if 是从谁身上分出来的"。血缘用 parentId，所以有环或
        // 父节点不存在时会在 layout::tree 里被升成独立的树，不会消失。
        let tree_el: Option<gpui_kit::AnyElement> = if self.view == WlTab::Tree {
            // 钻取：只看某条线的子树
            let scoped = match self.drill.as_ref() {
                Some(r) => crate::worldline::layout::subtree(&lines, r),
                None => lines.clone(),
            };
            let laid = crate::worldline::layout::tree(&scoped);
            let drill = self.drill.clone();
            let sel_t = self.selected.clone();
            let pickable = lines.clone();
            let nodes_c = laid.nodes.clone();
            // paint 闭包会 move 走一份，标签层用这份
            let labels_nodes = laid.nodes.clone();

            // —— 布局：纵向分层（根在底，层往上）+ 正交走线 ——
            //
            // 这就是"树"的标准画法：每个父节点在自己的高度上拉一条**水平总线**，
            // 子节点坐在总线上方的不同 x 上，各自一条竖线上去。
            // 之前按"章节高度"摆、再用曲线连，结果是几根并排的柱子——
            // 结构信息全在"谁是谁的孩子"，不在"第几章"。
            let max_depth = laid.nodes.iter().map(|n| n.depth).max().unwrap_or(0);
            let tree = div()
                .relative()
                .flex_1()
                .min_h_0()
                .w_full()
                .child(
                    canvas(
                        |_, _, _| {},
                        move |bounds, _, window, _| {
                            let ox = bounds.origin.x.as_f32();
                            let oy = bounds.origin.y.as_f32();
                            let w = bounds.size.width.as_f32();
                            let h = bounds.size.height.as_f32();
                            let p = |x: f32, y: f32| point(px(ox + x), px(oy + y));
                            let pad_x = 56.0f32;
                            let pad_top = 44.0f32;
                            let pad_bottom = 64.0f32;
                            let x_of = |nx: f32| {
                                pad_x + nx.clamp(0., 1.) * (w - pad_x * 2.)
                            };
                            let layers = max_depth.max(1) as f32;
                            let gap = (h - pad_top - pad_bottom) / (layers + 0.6);
                            let y_of = |depth: usize| {
                                h - pad_bottom - depth as f32 * gap
                            };

                            // 每棵子树一个颜色（depth=1 的那条根分支决定整支的色），
                            // 正典主干永远是最亮的那根。作者认树靠的是颜色连贯。
                            let mut subtree_color: std::collections::BTreeMap<String, gpui_kit::Rgba> =
                                Default::default();
                            let mut branch_i = 0usize;
                            for n in nodes_c.iter() {
                                let c = if n.depth == 0 {
                                    // 主干（正典）用最亮的那个
                                    theme::ACCENT()
                                } else if n.depth == 1 {
                                    // 每条**发展线路**一个颜色：depth=1 的分支各开一色，
                                    // 它们的后代继承。全图一个色的话，"不同的世界线"
                                    // 就只是位置不同——颜色才是认路的。
                                    let palette = [
                                        theme::GREEN(), theme::AMBER(), theme::PURPLE(),
                                        theme::RED(), theme::BLUE(),
                                    ];
                                    let c = palette[branch_i % palette.len()];
                                    branch_i += 1;
                                    c
                                } else if let Some(pc) =
                                    n.parent.as_ref().and_then(|p| subtree_color.get(p)).copied()
                                {
                                    // 深层继承父支颜色
                                    pc
                                } else {
                                    let palette = [
                                        theme::GREEN(), theme::AMBER(), theme::PURPLE(),
                                        theme::RED(), theme::BLUE(),
                                    ];
                                    let c = palette[branch_i % palette.len()];
                                    branch_i += 1;
                                    c
                                };
                                subtree_color.insert(n.id.clone(), c);
                            }

                            // 走线：父节点 → 水平到子 x → 竖直到子节点。
                            // 画两遍：一遍宽而淡（光晕），一遍细而实。
                            for n in nodes_c.iter() {
                                let Some(color) = subtree_color.get(&n.id).copied() else { continue };
                                let Some(pid) = &n.parent else { continue };
                                let Some(par) = nodes_c.iter().find(|p2| &p2.id == pid) else {
                                    continue;
                                };
                                let x = x_of(n.x);
                                let y = y_of(n.depth);
                                let par_x = x_of(par.x);
                                let par_y = y_of(par.depth);
                                let on = sel_t.as_deref() == Some(n.id.as_str());
                                let dashed = n.id != "α" && !n.parent.as_deref().map(|p| p == "α").unwrap_or(false)
                                    && n.depth >= 2;
                                let width = if on { 3.4 } else if n.depth == 0 { 2.6 } else { 2.0 };

                                // 光晕（宽、淡）
                                let mut glow = PathBuilder::stroke(px(width + 4.0));
                                if dashed {
                                    glow = glow.dash_array(&[px(4.), px(5.)]);
                                }
                                glow.move_to(p(par_x, par_y));
                                glow.line_to(p(x, par_y));
                                glow.line_to(p(x, y));
                                if let Ok(path) = glow.build() {
                                    window.paint_path(path, hsla(color).opacity(0.14));
                                }
                                // 实线
                                let mut seg = PathBuilder::stroke(px(width));
                                if dashed {
                                    seg = seg.dash_array(&[px(4.), px(5.)]);
                                }
                                seg.move_to(p(par_x, par_y));
                                seg.line_to(p(x, par_y));
                                seg.line_to(p(x, y));
                                if let Ok(path) = seg.build() {
                                    let a = if n.archived { 0.4 } else { 0.95 };
                                    window.paint_path(path, hsla(color).opacity(a));
                                }
                            }

                            // 节点：圆 + 两圈光环（外圈虚线）
                            for n in nodes_c.iter() {
                                let Some(color) = subtree_color.get(&n.id).copied() else { continue };
                                let x = x_of(n.x);
                                let y = y_of(n.depth);
                                let on = sel_t.as_deref() == Some(n.id.as_str());
                                let root = n.depth == 0;
                                let r = if root { 7.0 } else if on { 6.0 } else { 4.5 };
                                let k = 0.5523 * r;
                                let mut d = PathBuilder::fill();
                                d.move_to(p(x + r, y));
                                d.cubic_bezier_to(p(x, y + r), p(x + k, y + r), p(x + r, y));
                                d.cubic_bezier_to(p(x + r, y), p(x + r, y - k), p(x + k, y - r));
                                d.cubic_bezier_to(p(x, y - r), p(x - k, y - r), p(x - r, y));
                                d.cubic_bezier_to(p(x - r, y), p(x - r, y - k), p(x - k, y - r));
                                // 光晕
                                if let Ok(path) = {
                                    let rr = r + 5.0;
                                    let k2 = 0.5523 * rr;
                                    let mut g = PathBuilder::fill();
                                    g.move_to(p(x + rr, y));
                                    g.cubic_bezier_to(p(x, y + rr), p(x + k2, y + rr), p(x + rr, y));
                                    g.cubic_bezier_to(p(x + rr, y), p(x + rr, y - k2), p(x + k2, y - rr));
                                    g.cubic_bezier_to(p(x, y - rr), p(x - k2, y - rr), p(x - rr, y));
                                    g.cubic_bezier_to(p(x - rr, y), p(x - rr, y - k2), p(x - k2, y - rr));
                                    g.build()
                                } {
                                    window.paint_path(path, hsla(color).opacity(if on { 0.30 } else { 0.14 }));
                                }
                                if let Ok(path) = d.build() {
                                    window.paint_path(path, hsla(color));
                                }
                                // 外圈
                                let mut ring = PathBuilder::stroke(px(1.0));
                                if root {
                                    ring = ring.dash_array(&[px(2.), px(4.)]);
                                }
                                let rr = r + if root { 8.0 } else { 3.5 };
                                let k2 = 0.5523 * rr;
                                ring.move_to(p(x + rr, y));
                                ring.cubic_bezier_to(p(x, y + rr), p(x + k2, y + rr), p(x + rr, y));
                                ring.cubic_bezier_to(p(x + rr, y), p(x + rr, y - k2), p(x + k2, y - rr));
                                ring.cubic_bezier_to(p(x, y - rr), p(x - k2, y - rr), p(x - rr, y));
                                ring.cubic_bezier_to(p(x - rr, y), p(x - rr, y - k2), p(x - k2, y - rr));
                                if let Ok(path) = ring.build() {
                                    window.paint_path(path, hsla(color).opacity(if root { 0.5 } else { 0.3 }));
                                }
                            }
                        },
                    )
                    .absolute()
                    .size_full(),
                )
                // 节点标签 + 命中区（覆盖层，与画布同一套坐标公式）
                .children(labels_nodes.iter().map(|n| {
                    let id = n.id.clone();
                    let id2 = n.id.clone();
                    let on = self.selected.as_deref() == Some(n.id.as_str());
                    let drilled = drill.as_deref() == Some(n.id.as_str());
                    div()
                        .id(SharedString::from(format!("tree-node-{}", n.id)))
                        .absolute()
                        .left(relative(n.x.clamp(0., 1.)))
                        .bottom(relative(
                            1.0 - (n.depth as f32 / (max_depth.max(1) as f32 + 0.6)),
                        ))
                        .mb(px(58.))
                        .ml(px(-40.))
                        .w(px(80.))
                        .text_center()
                        .cursor_pointer()
                        .child(
                            div()
                                .text_size(px(11.))
                                .font_family(theme::FAMILY_MONO)
                                .font_weight(FontWeight::SEMIBOLD)
                                .text_color(if on {
                                    theme::ACCENT()
                                } else if drilled {
                                    theme::AMBER()
                                } else {
                                    theme::TEXT()
                                })
                                .child(SharedString::from(n.id.clone())),
                        )
                        .when(!n.name.is_empty() && n.name != n.id, |d| {
                            d.child(
                                div()
                                    .text_size(px(9.))
                                    .text_ellipsis()
                                    .overflow_hidden()
                                    .whitespace_nowrap()
                                    .text_color(theme::FAINT())
                                    .child(SharedString::from(n.name.clone())),
                            )
                        })
                        .child(
                            div()
                                .text_size(px(8.5))
                                .font_family(theme::FAMILY_MONO)
                                .text_color(theme::FAINT())
                                .child(SharedString::from(format!(
                                    "{}章 · {}节点",
                                    n.fork_chapter, n.node_count
                                ))),
                        )
                        .on_click(cx.listener(move |this, _, _, cx| {
                            this.selected = Some(id.clone());
                            // 再点同一节点 = 钻取进它的子树
                            this.drill = if this.drill.as_deref() == Some(id2.as_str()) {
                                None
                            } else {
                                Some(id2.clone())
                            };
                            cx.notify();
                        }))
                        .into_any_element()
                }));

            let col = div()
                .flex_1()
                .min_h_0()
                .flex()
                .flex_col()
                .gap(theme::GAP_SM)
                .p(theme::PAD_MD)
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
                                .child("钻取"),
                        )
                        .child({
                            let on = drill.is_none();
                            div()
                                .id("drill-all")
                                .px(px(7.))
                                .py(px(2.))
                                .rounded(px(5.))
                                .border_1()
                                .border_color(if on { theme::ACCENT_BORDER() } else { theme::LINE() })
                                .bg(if on { theme::ACCENT_SOFT() } else { theme::PANEL2() })
                                .cursor_pointer()
                                .text_size(theme::FONT_MICRO)
                                .text_color(if on { theme::ACCENT() } else { theme::DIM() })
                                .child("全部")
                                .on_click(cx.listener(|this, _, _, cx| {
                                    this.drill = None;
                                    cx.notify();
                                }))
                        })
                        .children(pickable.iter().map(|l| {
                            let id = l.id.clone();
                            let on = drill.as_deref() == Some(l.id.as_str());
                            div()
                                .id(SharedString::from(format!("drill-{}", l.id)))
                                .px(px(7.))
                                .py(px(2.))
                                .rounded(px(5.))
                                .border_1()
                                .border_color(if on { theme::ACCENT_BORDER() } else { theme::LINE() })
                                .bg(if on { theme::ACCENT_SOFT() } else { theme::PANEL2() })
                                .cursor_pointer()
                                .text_size(theme::FONT_MICRO)
                                .text_color(if on { theme::ACCENT() } else { theme::DIM() })
                                .child(SharedString::from(l.id.clone()))
                                .on_click(cx.listener(move |this, _, _, cx| {
                                    this.drill = Some(id.clone());
                                    this.selected = Some(id.clone());
                                    cx.notify();
                                }))
                        })),
                )
                // 树直接挂在 col 下并用 flex_1 撑满——不要中间的包装层：
                // 多一层 flex 容器时高度会被内容（全是绝对定位，高 0）压扁，
                // 树就成了一条 1px 的线。
                .child(tree)
                .child(
                    div()
                        .text_size(theme::FONT_MICRO)
                        .text_color(theme::FAINT())
                        .child("根在底部（正典），枝往上长 · 每支一个颜色 · 点节点名选中，再点钻进它的子树"),
                );
            Some(col.into_any_element())
        } else {
            None
        };

        let compare_el: Option<gpui_kit::AnyElement> = if self.view == WlTab::Compare {
            self.compare_with.as_ref().map(|right| {
            let left = self
                .selected
                .clone()
                .or_else(|| lines.iter().find(|l| !l.is_base()).map(|l| l.id.clone()))
                .unwrap_or_else(|| "α".into());
            let cmp = crate::worldline::layout::compare(&lines, &left, right);
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
            let mut cols = div().flex().gap(theme::GAP_GROUP);
            for (title, list, color) in [
                (format!("{} 独有", cmp.a), cmp.only_a.clone(), theme::ACCENT()),
                (format!("{} 独有", cmp.b), cmp.only_b.clone(), theme::BLUE()),
            ] {
                let mut col = div().flex_1().min_w_0().flex().flex_col().gap(px(2.)).child(
                    div()
                        .text_size(theme::FONT_LABEL)
                        .font_weight(FontWeight::SEMIBOLD)
                        .text_color(color)
                        .child(SharedString::from(format!("{title} {}", list.len()))),
                );
                for (label, ch) in list {
                    col = col.child(
                        div()
                            .flex()
                            .gap(theme::GAP_SM)
                            .child(
                                div()
                                    .w(px(40.))
                                    .flex_shrink_0()
                                    .font_family(theme::FAMILY_MONO)
                                    .text_size(theme::FONT_MICRO)
                                    .text_color(theme::FAINT())
                                    .child(SharedString::from(format!("第{ch}章"))),
                            )
                            .child(
                                div()
                                    .text_size(theme::FONT_MICRO)
                                    .text_color(theme::TEXT())
                                    .child(SharedString::from(label)),
                            ),
                    );
                }
                cols = cols.child(col);
            }
            let mut conflict_el = div().flex().flex_col().gap(px(2.));
            if cmp.conflicts.is_empty() {
                conflict_el = conflict_el.child(
                    div()
                        .text_size(theme::FONT_MICRO)
                        .text_color(theme::FAINT())
                        .child("两条线对同一目标的影响方式一致，没有冲突。"),
                );
            }
            for c in &cmp.conflicts {
                conflict_el = conflict_el.child(
                    div()
                        .text_size(theme::FONT_MICRO)
                        .text_color(theme::AMBER())
                        .child(SharedString::from(format!("⚠ {c}"))),
                );
            }
            div()
                .flex_1()
                .min_h_0()
                .flex()
                .flex_col()
                .gap(theme::GAP_MD)
                .p(theme::PAD_MD)
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
                            seg2(id.clone(), on, format!("cmp-a-{id}")).on_click(cx.listener(
                                move |this, _, _, cx| {
                                    this.selected = Some(id.clone());
                                    cx.notify();
                                },
                            ))
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
                            seg2(id.clone(), on, format!("cmp-b-{id}")).on_click(cx.listener(
                                move |this, _, _, cx| {
                                    this.compare_with = Some(id.clone());
                                    cx.notify();
                                },
                            ))
                        })),
                )
                .child(
                    div()
                        .text_size(theme::FONT_MICRO)
                        .text_color(theme::FAINT())
                        .child(SharedString::from(if cmp.common_ancestor.is_empty() {
                            "两条线没有共同祖先：各自推演的全部节点都算差异".to_string()
                        } else {
                            format!("共同祖先：{}（只比较它之后的推演）", cmp.common_ancestor)
                        })),
                )
                .child(cols)
                .child(
                    div()
                        .flex()
                        .flex_col()
                        .gap(px(3.))
                        .border_t_1()
                        .border_color(theme::LINE())
                        .pt(theme::GAP_SM)
                        .child(
                            div()
                                .text_size(theme::FONT_LABEL)
                                .font_weight(FontWeight::SEMIBOLD)
                                .text_color(theme::TEXT())
                                .child(SharedString::from(format!(
                                    "共同推演 {} 件 · 冲突 {} 处",
                                    cmp.shared.len(),
                                    cmp.conflicts.len()
                                ))),
                        )
                        .child(conflict_el),
                )
                .into_any_element()
            })
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
                    .flex()
                    .items_baseline()
                    .gap(theme::GAP_MD)
                    .child(
                        div()
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
                            .child(SharedString::from({
                                let here = store.lines_at(&lines, boundary.max(1)).len();
                                format!(
                                    "剧情确定到第 {boundary} 章（成稿 {written} 章）；此后开放的 if，                                     当前位置有 {here} 条线在跑"
                                )
                            })),
                    )
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
                    // 取景：缩放 / 平移 / 回到全览。
                    //
                    // 只在时间轴这一页出现——树与对比各有自己的尺度，
                    // 把同一组按钮摆在所有页上会让人以为它管全部。
            .when(self.view == WlTab::Timeline, |d| {
                        let full = horizon;
                        d.child(
                            div()
                                .flex()
                                .items_center()
                                .gap(px(3.))
                                .child(
                                    div()
                                        .text_size(theme::FONT_MICRO)
                                        .text_color(theme::FAINT())
                                        .child(SharedString::from(format!(
                                            "第{from_ch}–{to_ch}章"
                                        ))),
                                )
                                .children([
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
                                }))
                                .child(
                                    Button::new("wl-pan-l")
                                        .xsmall()
                                        .label("◀")
                                        .on_click(cx.listener(move |this, _, _, cx| {
                                            let span = this
                                                .window
                                                .map(|(a, b)| (b - a).max(1))
                                                .unwrap_or(full.max(12));
                                            this.pan(-((span / 4).max(1) as i32), full);
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
                                            this.pan((span / 4).max(1) as i32, full);
                                            cx.notify();
                                        })),
                                )
                                // 「全部」只在真的缩放过时才有意义：
                                // 一直显示会让人以为"现在是缩放的"，而默认就是全览。
                                .when(self.is_zoomed(full), |d| {
                                    d.child(
                                        Button::new("wl-zoom-all")
                                            .xsmall()
                                            .label("全部")
                                            .on_click(cx.listener(move |this, _, _, cx| {
                                                this.window = None;
                                                cx.notify();
                                            })),
                                    )
                                }),
                        )
                    })
                    .when_some(self.message.clone(), |d, m| {
                        d.child(
                            div()
                                .text_size(theme::FONT_MICRO)
                                .text_color(theme::GREEN())
                                .child(m),
                        )
                    })
                    // 时间轴 / 对比：都属于"世界线"这一页的两种看法
                    .child(
                        div()
                            .flex()
                            .gap(px(4.))
                            .child(
                                div()
                                    .id("wl-tab-time")
                                    .px(px(9.))
                                    .py(px(3.))
                                    .rounded(px(6.))
                                    .border_1()
                                    .border_color(if self.view == WlTab::Timeline {
                                        theme::ACCENT_BORDER()
                                    } else {
                                        theme::LINE()
                                    })
                                    .bg(if self.view == WlTab::Timeline {
                                        theme::ACCENT_SOFT()
                                    } else {
                                        theme::PANEL2()
                                    })
                                    .cursor_pointer()
                                    .text_size(theme::FONT_MICRO)
                                    .text_color(if self.view == WlTab::Timeline {
                                        theme::ACCENT()
                                    } else {
                                        theme::DIM()
                                    })
                                    .child("时间轴")
                                    .on_click(cx.listener(|this, _, _, cx| {
                                        this.view = WlTab::Timeline;
                                        cx.notify();
                                    })),
                            )
                            .child(
                                div()
                                    .id("wl-tab-cmp")
                                    .px(px(9.))
                                    .py(px(3.))
                                    .rounded(px(6.))
                                    .border_1()
                                    .border_color(if self.view == WlTab::Tree {
                                        theme::ACCENT_BORDER()
                                    } else {
                                        theme::LINE()
                                    })
                                    .bg(if self.view == WlTab::Tree {
                                        theme::ACCENT_SOFT()
                                    } else {
                                        theme::PANEL2()
                                    })
                                    .cursor_pointer()
                                    .text_size(theme::FONT_MICRO)
                                    .text_color(if self.view == WlTab::Tree {
                                        theme::ACCENT()
                                    } else {
                                        theme::DIM()
                                    })
                                    .child("世界树")
                                    .on_click(cx.listener(|this, _, _, cx| {
                                        this.view = WlTab::Tree;
                                        // 抽屉是"时间轴上那个点"的上下文，切走就没意义了
                                        this.picked = None;
                                        cx.notify();
                                    })),
                            )
                            .child(
                                div()
                                    .id("wl-tab-cmp")
                                    .px(px(9.))
                                    .py(px(3.))
                                    .rounded(px(6.))
                                    .border_1()
                                    .border_color(if self.view == WlTab::Compare {
                                        theme::ACCENT_BORDER()
                                    } else {
                                        theme::LINE()
                                    })
                                    .bg(if self.view == WlTab::Compare {
                                        theme::ACCENT_SOFT()
                                    } else {
                                        theme::PANEL2()
                                    })
                                    .cursor_pointer()
                                    .text_size(theme::FONT_MICRO)
                                    .text_color(if self.view == WlTab::Compare {
                                        theme::ACCENT()
                                    } else {
                                        theme::DIM()
                                    })
                                    .child("对比")
                                    .on_click(cx.listener(|this, _, _, cx| {
                                        // 默认拿「选中线 vs 正典」，作者立刻看到东西
                                        if this.compare_with.is_none() {
                                            this.compare_with = Some("α".into());
                                        }
                                        this.view = WlTab::Compare;
                                        this.picked = None;
                                        cx.notify();
                                    })),
                            ),
                    ),
            )
            .when_some(compare_el, |d, cmp| d.child(cmp))
            .when_some(tree_el, |d, t| d.child(t))
            .when(self.view == WlTab::Timeline, |d| {
                // 整页都是时间轴。轨道列表与详情做成浮层：
                // 线名贴在各自的线尾，选中某条才在右侧浮出它的操作面板。
                d.child(
                    div()
                        .flex_1()
                        .min_h_0()
                        .w_full()
                        .relative()
                        .child(timeline)
                        // 右下：选中线的详情浮层（不占版面）
                        .when(self.selected.is_some(), |d| {
                            d.child(
                                div()
                                    .id("wl-detail")
                                    .absolute()
                                    .right(px(16.))
                                    .top(px(48.))
                                    .w(px(272.))
                                    .max_h(relative(0.72))
                                    .overflow_y_scroll()
                                    .p(theme::PAD_SM)
                                    .rounded(theme::RADIUS_PANEL)
                                    .border_1()
                                    .border_color(theme::LINE())
                                    // 半透明底：盖住的内容还能透出来一点，
                                    // 作者看得出"这是浮层，不是版面"
                                    .bg(theme::PANEL())
                                    .child(detail),
                            )
                        })
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
                        ),
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

/// 世界树的配色：一条世界线一个颜色。
///
/// 为什么按下标而不是按 kind：kind 只有四类，13 条线里必然撞色；
/// 而"不同的世界发展线路"要能一眼分开，所以按线的顺序取一轮调色板。
/// 正典固定蓝（它是基准，不该跟别的线抢色）。

fn track_color(t: &Track) -> gpui_kit::Rgba {
    if t.kind == Kind::Base {
        theme::BLUE()
    } else if t.kind == Kind::Fixed {
        theme::GREEN()
    } else if t.archived {
        theme::FAINT()
    } else if t.kind == Kind::Forecast {
        theme::AMBER()
    } else {
        theme::PURPLE()
    }
}
