//! M7-1 剧情画板 + M7-2 预言书面板（照 design/canvas.html 原型实现）。
//!
//! 画板：节点 = 故事线/事件/人物/伏笔/设定（事实源 store，与 novel-pack
//! get_world_state 同一映射）；边 = influence/plant/payoff，暗线虚线。
//! 交互 = 拖动节点 / 空白平移 / ±缩放 / 点选看一阶影响半径 / 读者视角隐藏暗线。
//! 视图态持久化 canvas.json（view/pos/reader），不碰任何正典数据。
//!
//! 预言书：worldlines.json 沙盒浏览器 + 「开线推演」发射 oracle_task 到 DSH；
//! diffs 逐条晋升（store.promote_diff，幂等）→ 收件箱 → 采纳才改正典。

use std::cell::RefCell;
use std::rc::Rc;

use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::Sizable;
use gpui_kit::component::dock::{BasePanel, Panel as ComponentPanel, PanelEvent};
use gpui_kit::component::input::{Input, InputState};
use gpui_kit::component::tag::Tag;
use gpui_kit::prelude::FluentBuilder;
use gpui_kit::{
    AnyElement, App, AppContext as _, Context, Entity, EventEmitter, FocusHandle, Focusable,
    FontWeight, InteractiveElement, IntoElement, MouseButton, MouseDownEvent, MouseMoveEvent,
    MouseUpEvent, ParentElement, PathBuilder, Pixels, Point, Render, SharedString,
    StatefulInteractiveElement, Styled, Window, canvas, div, point, px, relative,
};

use crate::store::ProjectHandle;
use crate::theme;

fn hsla(c: gpui_kit::Rgba) -> gpui_kit::Hsla {
    c.into()
}

// ═══════════════════════════ 剧情画板 ═══════════════════════════

#[derive(Clone)]
pub struct CNode {
    id: String,
    kind: &'static str, // storyline|event|character|clue|canon
    label: String,
    meta: String,
    hidden: bool, // 未揭示事件 = 暗线
    x: f32,
    y: f32,
}

#[derive(Clone)]
pub struct CEdge {
    from: usize,
    to: usize,
    kind: &'static str, // influence|plant|payoff
    hidden: bool,
}

enum Drag {
    Node(Vec<usize>, Point<Pixels>),
    Pan(Point<Pixels>),
    Marquee(Point<Pixels>),
}

const NODE_W: f32 = 150.;
const NODE_H: f32 = 76.;

pub struct CanvasPanel {
    focus_handle: FocusHandle,
    handle: Rc<ProjectHandle>,
    nodes: Vec<CNode>,
    edges: Vec<CEdge>,
    view: (f32, f32, f32), // x, y, k
    sel: Vec<usize>,
    drag: Option<Drag>,
    /// 橡皮筋（stage 局部坐标）
    marquee: Option<((f32, f32), (f32, f32))>,
    /// canvas prepaint 每帧写回的 stage 窗口原点 → 事件坐标可换算
    stage_origin: Rc<RefCell<Option<(f32, f32)>>>,
    reader: bool,
}

fn s(v: &serde_json::Value, key: &str) -> String {
    v.get(key).and_then(|x| x.as_str()).unwrap_or("").to_string()
}

impl CanvasPanel {
    pub fn new(cx: &mut Context<Self>, handle: Rc<ProjectHandle>) -> Self {
        let mut this = Self {
            focus_handle: cx.focus_handle(),
            handle,
            nodes: Vec::new(),
            edges: Vec::new(),
            view: (0., 0., 1.),
            sel: Vec::new(),
            drag: None,
            marquee: None,
            stage_origin: Rc::new(RefCell::new(None)),
            reader: false,
        };
        this.refresh();
        this
    }

    /// 正典 → 节点/边 重建；视图态（平移缩放/坐标/读者视角）从 canvas.json 恢复。
    pub fn refresh(&mut self) {
        let state = self.handle.canvas_state();
        if let Some(v) = state.get("view") {
            self.view = (
                v.get("x").and_then(|x| x.as_f64()).unwrap_or(0.) as f32,
                v.get("y").and_then(|x| x.as_f64()).unwrap_or(0.) as f32,
                v.get("k").and_then(|x| x.as_f64()).unwrap_or(1.) as f32,
            );
        }
        self.reader = state.get("reader").and_then(|r| r.as_bool()).unwrap_or(false);

        let col = |kind: &str| match kind {
            "storyline" => 50.,
            "event" => 300.,
            "character" => 540.,
            "clue" => 760.,
            _ => 980.,
        };
        let mut counts = [0usize; 5];
        let mut nodes: Vec<CNode> = Vec::new();

        for l in self.handle.storylines() {
            let id = s(&l, "id");
            nodes.push(CNode {
                kind: "storyline",
                label: s(&l, "name"),
                meta: s(&l, "status"),
                hidden: false,
                x: col("storyline"),
                y: 60. + counts[0] as f32 * (NODE_H + 44.),
                id,
            });
            counts[0] += 1;
        }
        for e in self.handle.events() {
            let revealed = e.get("revealed").and_then(|r| r.as_bool()).unwrap_or(true);
            let fact = s(&e, "objectiveFact");
            let chap = e.get("chapter").and_then(|c| c.as_u64()).unwrap_or(0);
            nodes.push(CNode {
                kind: "event",
                id: s(&e, "id"),
                label: fact.chars().take(14).collect(),
                meta: format!("第{chap}章 · {}", if revealed { "读者已知" } else { "未揭示·暗线" }),
                hidden: !revealed,
                x: col("event"),
                y: 60. + counts[1] as f32 * (NODE_H + 44.),
            });
            counts[1] += 1;
        }
        for a in self.handle.aliases() {
            let canon = s(&a, "canonicalName");
            let aka = a
                .get("aliases")
                .and_then(|v| v.as_array())
                .map(|arr| arr.iter().filter_map(|x| x.as_str()).collect::<Vec<_>>().join("、"))
                .unwrap_or_default();
            nodes.push(CNode {
                kind: "character",
                id: format!("A:{canon}"),
                label: canon,
                meta: aka,
                hidden: false,
                x: col("character"),
                y: 60. + counts[2] as f32 * (NODE_H + 44.),
            });
            counts[2] += 1;
        }
        for c in self.handle.clues_full() {
            let planted = c.get("plantedChapter").and_then(|v| v.as_u64()).unwrap_or(0);
            nodes.push(CNode {
                kind: "clue",
                id: s(&c, "id"),
                label: s(&c, "title"),
                meta: format!("{} · 埋第{planted}章", s(&c, "status")),
                hidden: false,
                x: col("clue"),
                y: 60. + counts[3] as f32 * (NODE_H + 44.),
            });
            counts[3] += 1;
        }
        for (title, category, _) in self.handle.canon_docs().into_iter().take(10) {
            nodes.push(CNode {
                kind: "canon",
                id: format!("W:{title}"),
                label: title,
                meta: category,
                hidden: false,
                x: col("canon"),
                y: 60. + counts[4] as f32 * (NODE_H + 44.),
            });
            counts[4] += 1;
        }

        // 恢复持久化坐标
        if let Some(pos) = state.get("pos").and_then(|p| p.as_object()) {
            for n in nodes.iter_mut() {
                if let Some(xy) = pos.get(&n.id).and_then(|v| v.as_array()) {
                    if xy.len() == 2 {
                        n.x = xy[0].as_f64().unwrap_or(n.x as f64) as f32;
                        n.y = xy[1].as_f64().unwrap_or(n.y as f64) as f32;
                    }
                }
            }
        }

        // 边（数据驱动：事件挂线 / 伏笔埋收 / 事实牵线）
        let mut edges: Vec<CEdge> = Vec::new();
        // involve：记忆事实 subject=人名、fromChapter=事件章 → 人物↔事件
        for f in self.handle.memory_facts() {
            let subj = s(&f, "subject");
            let ch = f.get("fromChapter").and_then(|v| v.as_u64());
            let Some(ci) = nodes.iter().position(|n| n.kind == "character" && n.label == subj) else { continue };
            for e in self.handle.events() {
                if e.get("chapter").and_then(|v| v.as_u64()) == ch {
                    if let Some(ei) = nodes.iter().position(|n| n.id == s(&e, "id")) {
                        let revealed = e.get("revealed").and_then(|r| r.as_bool()).unwrap_or(true);
                        let has = edges.iter().any(|x| (x.from == ci && x.to == ei) || (x.from == ei && x.to == ci));
                        if !has {
                            edges.push(CEdge { from: ci, to: ei, kind: "involve", hidden: !revealed && f.get("publicToReader").and_then(|p| p.as_bool()) == Some(false) });
                        }
                    }
                }
            }
        }
        for e in self.handle.events() {
            let eid = s(&e, "id");
            let revealed = e.get("revealed").and_then(|r| r.as_bool()).unwrap_or(true);
            if let Some(sids) = e.get("storylineIDs").and_then(|x| x.as_array()).cloned() {
                for sid in sids.iter().filter_map(|x| x.as_str()) {
                    if let (Some(fi), Some(ti)) = (
                        nodes.iter().position(|n| n.id == sid),
                        nodes.iter().position(|n| n.id == eid),
                    ) {
                        edges.push(CEdge { from: fi, to: ti, kind: "influence", hidden: !revealed });
                    }
                }
            }
        }
        for c in self.handle.clues_full() {
            let cid = s(&c, "id");
            let planted = c.get("plantedChapter").and_then(|v| v.as_u64());
            let last = c.get("lastActionChapter").and_then(|v| v.as_u64());
            let status = s(&c, "status");
            for e in self.handle.events() {
                let ch = e.get("chapter").and_then(|v| v.as_u64());
                let eid = s(&e, "id");
                if let (Some(ci), Some(ei)) = (
                    nodes.iter().position(|n| n.id == cid),
                    nodes.iter().position(|n| n.id == eid),
                ) {
                    if planted.is_some() && planted == ch {
                        edges.push(CEdge { from: ci, to: ei, kind: "plant", hidden: false });
                    }
                    if status == "已兑现" && last.is_some() && last == ch {
                        edges.push(CEdge { from: ei, to: ci, kind: "payoff", hidden: false });
                    }
                }
            }
        }

        self.nodes = nodes;
        self.edges = edges;
        self.sel.clear();
        self.marquee = None;
    }

    fn save_state(&self) {
        let mut pos = serde_json::Map::new();
        for n in &self.nodes {
            pos.insert(n.id.clone(), serde_json::json!([n.x, n.y]));
        }
        let v = serde_json::json!({
            "view": { "x": self.view.0, "y": self.view.1, "k": self.view.2 },
            "pos": pos,
            "reader": self.reader,
        });
        let _ = self.handle.save_canvas_state(&v);
    }

    fn sx(&self, n: &CNode) -> f32 {
        n.x * self.view.2 + self.view.0 + NODE_W * self.view.2 * 0.5
    }
    fn sy(&self, n: &CNode) -> f32 {
        n.y * self.view.2 + self.view.1 + NODE_H * self.view.2 * 0.5
    }

    /// 选中节点的一阶影响邻域（侧板行）。
    fn neighbor_rows(&self) -> Vec<(String, String)> {
        let Some(&sel) = self.sel.last() else { return Vec::new() };
        let mut out = Vec::new();
        for e in &self.edges {
            let (name, dir) = if e.from == sel {
                (self.nodes[e.to].label.clone(), "→")
            } else if e.to == sel {
                (self.nodes[e.from].label.clone(), "←")
            } else {
                continue;
            };
            let kind = match e.kind {
                "plant" => "埋设",
                "payoff" => "兑现",
                "involve" => "牵涉",
                _ => "影响",
            };
            out.push((format!("{kind}{dir}{}", if e.hidden { "·暗" } else { "" }), name));
        }
        out.truncate(12);
        out
    }

    fn zoom(&mut self, f: f32, cx: &mut Context<Self>) {
        self.view.2 = (self.view.2 * f).clamp(0.4, 1.8);
        self.save_state();
        cx.notify();
    }
}

impl EventEmitter<PanelEvent> for CanvasPanel {}

impl Focusable for CanvasPanel {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus_handle.clone()
    }
}

impl Render for CanvasPanel {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        // 边段预计算（stage 局部坐标；paint 里叠加 bounds 原点）
        let reader = self.reader;
        let mut segs: Vec<(f32, f32, f32, f32, &'static str, bool)> = Vec::new();
        for e in &self.edges {
            if reader && e.hidden {
                continue;
            }
            let a = &self.nodes[e.from];
            let b = &self.nodes[e.to];
            segs.push((self.sx(a), self.sy(a), self.sx(b), self.sy(b), e.kind, e.hidden));
        }

        let marquee_now = self.marquee;
        let origin_cell = self.stage_origin.clone();
        let mut stage = div()
            .id("canvas-stage")
            .size_full()
            .relative()
            .overflow_hidden()
            .bg(theme::BG())
            .child(
                canvas(
                    move |bounds, _, _| {
                        *origin_cell.borrow_mut() = Some((bounds.origin.x.as_f32(), bounds.origin.y.as_f32()));
                    },
                    move |bounds, _, window, _| {
                        if let Some(((ax, ay), (bx, by))) = marquee_now {
                            let mut r = PathBuilder::stroke(px(1.2));
                            let o = bounds.origin;
                            r.move_to(point(o.x + px(ax), o.y + px(ay)));
                            r.line_to(point(o.x + px(bx), o.y + px(ay)));
                            r.line_to(point(o.x + px(bx), o.y + px(by)));
                            r.line_to(point(o.x + px(ax), o.y + px(by)));
                            r.line_to(point(o.x + px(ax), o.y + px(ay)));
                            if let Ok(path) = r.build() {
                                window.paint_path(path, hsla(theme::ACCENT()));
                            }
                        }
                        for (x1, y1, x2, y2, kind, hidden) in &segs {
                            let color = if *hidden {
                                theme::PURPLE()
                            } else {
                                match *kind {
                                    "plant" | "payoff" => theme::AMBER(),
                                    "involve" => theme::GREEN(),
                                    _ => theme::DIM(),
                                }
                            };
                            let mut b = PathBuilder::stroke(px(1.4));
                            if *hidden {
                                b = b.dash_array(&[px(5.), px(5.)]);
                            }
                            let start = point(bounds.origin.x + px(*x1), bounds.origin.y + px(*y1));
                            let end = point(bounds.origin.x + px(*x2), bounds.origin.y + px(*y2));
                            b.move_to(start);
                            let mx = (x1 + x2) * 0.5;
                            b.cubic_bezier_to(
                                point(bounds.origin.x + px(mx), bounds.origin.y + px(*y1)),
                                point(bounds.origin.x + px(mx), bounds.origin.y + px(*y2)),
                                end,
                            );
                            if let Ok(path) = b.build() {
                                window.paint_path(path, hsla(color));
                            }
                        }
                    },
                )
                .absolute()
                .size_full(),
            );

        // 节点卡
        for (i, n) in self.nodes.iter().enumerate() {
            if reader && n.hidden {
                continue;
            }
            let sel = self.sel.contains(&i);
            let dot = match n.kind {
                "storyline" => theme::ACCENT(),
                "event" => theme::BLUE(),
                "character" => theme::GREEN(),
                "clue" => theme::AMBER(),
                _ => theme::PURPLE(),
            };
            let node = div()
                .absolute()
                .left(px(n.x * self.view.2 + self.view.0))
                .top(px(n.y * self.view.2 + self.view.1))
                .w(px(NODE_W * self.view.2))
                .h(px(NODE_H * self.view.2))
                .px(theme::PAD_SM)
                .py(px(6.))
                .rounded(theme::RADIUS_PANEL)
                .border_1()
                .border_color(if sel { theme::ACCENT_BORDER() } else { theme::LINE() })
                .bg(if sel { theme::ACCENT_SOFT() } else { theme::PANEL() })
                .on_mouse_down(
                    MouseButton::Left,
                    cx.listener(move |this, ev: &MouseDownEvent, _, cx| {
                        if ev.modifiers.shift {
                            // Shift 点选：加/去多选集
                            match this.sel.iter().position(|x| *x == i) {
                                Some(pos) => {
                                    this.sel.remove(pos);
                                    this.drag = None;
                                }
                                None => {
                                    this.sel.push(i);
                                    this.drag = Some(Drag::Node(this.sel.clone(), ev.position));
                                }
                            }
                        } else {
                            if !this.sel.contains(&i) {
                                this.sel = vec![i];
                            }
                            this.drag = Some(Drag::Node(this.sel.clone(), ev.position));
                        }
                        cx.notify();
                    }),
                )
                .child(
                    div()
                        .flex()
                        .items_center()
                        .gap(theme::GAP_XS)
                        .child(div().text_size(theme::FONT_TAG).text_color(dot).child("●"))
                        .child(
                            div()
                                .overflow_hidden()
                                .whitespace_nowrap()
                                .text_size(theme::FONT_TAG)
                                .font_weight(FontWeight::SEMIBOLD)
                                .font_family(theme::FAMILY_SERIF)
                                .text_color(theme::TEXT())
                                .child(SharedString::from(n.label.clone())),
                        ),
                )
                .child(
                    div()
                        .font_family(theme::FAMILY_MONO)
                        .text_size(theme::FONT_MICRO)
                        .text_color(theme::FAINT())
                        .child(SharedString::from(n.id.clone())),
                )
                .child(
                    div()
                        .overflow_hidden()
                        .whitespace_nowrap()
                        .text_size(theme::FONT_MICRO)
                        .text_color(theme::DIM())
                        .child(SharedString::from(n.meta.clone())),
                );
            stage = stage.child(node);
        }

        // 舞台级手势：空白按下=平移开始；移动=拖拽；抬起=落盘
        stage = stage
            .on_mouse_down(MouseButton::Left, cx.listener(|this, ev: &MouseDownEvent, _, _| {
                // 节点按下处理器先执行（bubble）：已设 Node 就不进空白手势
                if this.drag.is_none() {
                    if ev.modifiers.shift {
                        // 空白 Shift 按下：橡皮筋框选
                        let o = this.stage_origin.borrow();
                        if let Some((ox, oy)) = *o {
                            let (x, y) = (ev.position.x.as_f32() - ox, ev.position.y.as_f32() - oy);
                            this.marquee = Some(((x, y), (x, y)));
                            this.drag = Some(Drag::Marquee(ev.position));
                        }
                    } else {
                        this.drag = Some(Drag::Pan(ev.position));
                    }
                }
            }))
            .on_mouse_move(cx.listener(|this, ev: &MouseMoveEvent, _, cx| {
                match &mut this.drag {
                    Some(Drag::Node(ids, last)) => {
                        let dx = (ev.position.x - last.x).as_f32() / this.view.2;
                        let dy = (ev.position.y - last.y).as_f32() / this.view.2;
                        for &i in ids.iter() {
                            this.nodes[i].x += dx;
                            this.nodes[i].y += dy;
                        }
                        *last = ev.position;
                        cx.notify();
                    }
                    Some(Drag::Marquee(start)) => {
                        let o = this.stage_origin.borrow();
                        if let Some((ox, oy)) = *o {
                            let end = (ev.position.x.as_f32() - ox, ev.position.y.as_f32() - oy);
                            let start_local = (*start).x.as_f32() - ox;
                            let start_local_y = (*start).y.as_f32() - oy;
                            this.marquee = Some(((start_local, start_local_y), end));
                            cx.notify();
                        }
                    }
                    Some(Drag::Pan(last)) => {
                        this.view.0 += (ev.position.x - last.x).as_f32();
                        this.view.1 += (ev.position.y - last.y).as_f32();
                        *last = ev.position;
                        cx.notify();
                    }
                    None => {}
                }
            }))
            .on_mouse_up(MouseButton::Left, cx.listener(|this, _: &MouseUpEvent, _, cx| {
                let was_marquee = matches!(this.drag, Some(Drag::Marquee(_)));
                if was_marquee {
                    if let Some(((ax, ay), (bx, by))) = this.marquee.take() {
                        let (x0, x1) = (ax.min(bx), ax.max(bx));
                        let (y0, y1) = (ay.min(by), ay.max(by));
                        let hit: Vec<usize> = this
                            .nodes
                            .iter()
                            .enumerate()
                            .filter(|(_, n)| {
                                if this.reader && n.hidden {
                                    return false;
                                }
                                let nx = this.sx(n);
                                let ny = this.sy(n);
                                nx > x0 && nx < x1 && ny > y0 && ny < y1
                            })
                            .map(|(i, _)| i)
                            .collect();
                        if !hit.is_empty() {
                            this.sel = hit;
                        }
                    }
                    this.drag = None;
                    cx.notify();
                    return;
                }
                if this.drag.is_some() {
                    this.drag = None;
                    this.save_state();
                    cx.notify();
                }
            }))
            .on_mouse_down_out(cx.listener(|this, _, _, _| {
                this.drag = None;
                this.marquee = None;
            }));

        // 侧板：一阶影响半径
        let sel_title: Option<String> = match self.sel.len() {
            0 => None,
            1 => Some(format!("{}（{}）", self.nodes[self.sel[0]].label, self.nodes[self.sel[0]].id)),
            n => Some(format!("已选 {n} 个节点（成组可拖）")),
        };
        let rows = self.neighbor_rows();
        let side = div()
            .absolute()
            .right(theme::PAD_LG)
            .top(theme::PAD_LG)
            .w(px(250.))
            .p(theme::PAD_MD)
            .rounded(theme::RADIUS_CARD)
            .border_1()
            .border_color(theme::LINE())
            .bg(theme::PANEL())
            .child(
                div()
                    .text_size(theme::FONT_META)
                    .font_weight(FontWeight::SEMIBOLD)
                    .text_color(theme::TEXT())
                    .child(sel_title.unwrap_or_else(|| "选择一个节点".into())),
            )
            .child(div().pt(theme::GAP_MD).when(rows.is_empty(), |d| {
                d.child(
                    div()
                        .text_size(theme::FONT_LABEL)
                        .line_height(relative(theme::LINE_HEIGHT_BODY))
                        .text_color(theme::FAINT())
                        .child(if true { "实线=明线，虚线=暗线；黄=伏笔埋/收，绿=人物牵涉。点选看影响，Shift 点选多个可成组拖拽。" } else { "" }),
                )
            }).children(rows.into_iter().map(|(kind, name)| {
                div()
                    .flex()
                    .items_center()
                    .gap(theme::GAP_SM)
                    .py(theme::GAP_XS)
                    .border_b_1()
                    .border_color(theme::LINE())
                    .child(
                        Tag::custom(hsla(theme::BLUE_SOFT()), hsla(theme::BLUE()), hsla(theme::BLUE_BORDER()))
                            .outline()
                            .child(kind),
                    )
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .overflow_hidden()
                            .whitespace_nowrap()
                            .text_size(theme::FONT_LABEL)
                            .text_color(theme::DIM())
                            .child(name),
                    )
            })));

        // 工具条
        let bar = div()
            .absolute()
            .left(theme::PAD_LG)
            .top(theme::PAD_LG)
            .flex()
            .items_center()
            .gap(theme::GAP_SM)
            .child(
                Button::new("zoom-in").xsmall().label("＋")
                    .on_click(cx.listener(|this, _, _, cx| this.zoom(1.25, cx))),
            )
            .child(
                Button::new("zoom-out").xsmall().label("－")
                    .on_click(cx.listener(|this, _, _, cx| this.zoom(0.8, cx))),
            )
            .child(
                Button::new("zoom-fit").xsmall().label("⊡")
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.view = (0., 0., 1.);
                        this.save_state();
                        cx.notify();
                    })),
            )
            .child(
                Button::new("reader")
                    .xsmall()
                    .label(if self.reader { "读者视角" } else { "作者视角" })
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.reader = !this.reader;
                        this.save_state();
                        cx.notify();
                    })),
            )
            .child(
                Button::new("refresh").xsmall().label("刷新")
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.refresh();
                        cx.notify();
                    })),
            )
            .when(!self.sel.is_empty(), |d| {
                d.child(
                    Button::new("clear-sel")
                        .xsmall()
                        .label("清空选择")
                        .on_click(cx.listener(|this, _, _, cx| {
                            this.sel.clear();
                            cx.notify();
                        })),
                )
            });

        stage.child(side).child(bar)
    }
}

impl BasePanel for CanvasPanel {
    fn panel_name(&self) -> &'static str {
        "canvas-view"
    }
}

impl ComponentPanel for CanvasPanel {
    fn tab_name(&self, _: &App) -> Option<SharedString> {
        Some("画板".into())
    }

    fn title(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div()
            .text_size(theme::FONT_BODY)
            .font_weight(FontWeight::MEDIUM)
            .text_color(theme::TEXT())
            .child("剧情画板")
    }
}

// ═══════════════════════════ 预言书 ═══════════════════════════

#[derive(Debug, Clone)]
pub enum OracleEvent {
    Simulate { premise: String, fork: u32 },
    /// 晋升成功：宿主刷新收件箱/轨迹
    Promoted { worldline: String, proposal_id: String },
}

pub struct OraclePanel {
    focus_handle: FocusHandle,
    handle: Rc<ProjectHandle>,
    premise: Entity<InputState>,
    fork: Entity<InputState>,
    selected: Option<String>,
    message: Option<SharedString>,
}

impl OraclePanel {
    pub fn new(window: &mut Window, cx: &mut Context<Self>, handle: Rc<ProjectHandle>) -> Self {
        let premise = cx.new(|cx| {
            InputState::new(window, cx)
                .placeholder("如果……（一句假设，例：师兄在第15章没有死，而是以观灯人身份现身）")
        });
        let fork = cx.new(|cx| InputState::new(window, cx).placeholder("分歧章"));
        let selected = handle
            .worldlines()
            .into_iter()
            .find(|w| s(w, "id") != "α")
            .map(|w| s(&w, "id"));
        Self { focus_handle: cx.focus_handle(), handle, premise, fork, selected, message: None }
    }

    fn status_tag(status: &str) -> AnyElement {
        let (c, bg) = match status {
            "基准" => (theme::BLUE(), theme::BLUE_SOFT()),
            "推演中" => (theme::AMBER(), theme::AMBER_SOFT()),
            "已推演" => (theme::GREEN(), theme::GREEN_SOFT()),
            _ => (theme::FAINT(), theme::PANEL2()),
        };
        Tag::custom(hsla(bg), hsla(c), hsla(c))
            .outline()
            .child(SharedString::from(status.to_string()))
            .into_any_element()
    }
}

impl EventEmitter<PanelEvent> for OraclePanel {}
impl EventEmitter<OracleEvent> for OraclePanel {}

impl Focusable for OraclePanel {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus_handle.clone()
    }
}

impl Render for OraclePanel {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let lines = self.handle.worldlines();

        // —— 左列：世界线列表 ——
        let mut list: Vec<AnyElement> = Vec::new();
        for w in &lines {
            let id = s(w, "id");
            let is_alpha = id == "α";
            let name = s(w, "name");
            let premise = s(w, "premise");
            let status = s(w, "status");
            let n_diffs = w.get("diffs").and_then(|d| d.as_array()).map(|a| a.len()).unwrap_or(0);
            let n_nodes = w.get("nodes").and_then(|d| d.as_array()).map(|a| a.len()).unwrap_or(0);
            let on = !is_alpha && self.selected.as_deref() == Some(id.as_str());
            let click_id = id.clone();
            list.push(
                div()
                    .id(SharedString::from(format!("wl-{id}")))
                    .p(theme::PAD_SM)
                    .mb(theme::GAP_SM)
                    .rounded(theme::RADIUS_PANEL)
                    .border_1()
                    .border_color(if on { theme::ACCENT_BORDER() } else { theme::LINE() })
                    .bg(if on { theme::ACCENT_SOFT() } else { theme::PANEL2() })
                    .when(!is_alpha, |d| {
                        d.cursor_pointer().on_click(cx.listener(move |this, _, _, cx| {
                            this.selected = Some(click_id.clone());
                            this.message = None;
                            cx.notify();
                        }))
                    })
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap(theme::GAP_SM)
                            .child(
                                div()
                                    .font_family(theme::FAMILY_MONO)
                                    .text_size(theme::FONT_META)
                                    .font_weight(FontWeight::SEMIBOLD)
                                    .text_color(if is_alpha { theme::BLUE() } else { theme::ACCENT() })
                                    .child(SharedString::from(id.clone())),
                            )
                            .child(
                                div()
                                    .flex_1()
                                    .min_w_0()
                                    .overflow_hidden()
                                    .whitespace_nowrap()
                                    .text_size(theme::FONT_META)
                                    .font_weight(FontWeight::MEDIUM)
                                    .text_color(theme::TEXT())
                                    .child(SharedString::from(name)),
                            )
                            .child(Self::status_tag(&status)),
                    )
                    .child(
                        div()
                            .pt(theme::GAP_XS)
                            .text_size(theme::FONT_LABEL)
                            .line_height(relative(1.45))
                            .text_color(theme::DIM())
                            .child(SharedString::from(if is_alpha {
                                "当前事实基准 · 只读".to_string()
                            } else {
                                premise.chars().take(60).collect::<String>()
                            })),
                    )
                    .child(
                        div()
                            .text_size(theme::FONT_MICRO)
                            .font_family(theme::FAMILY_MONO)
                            .text_color(theme::FAINT())
                            .child(SharedString::from(format!("节点 {n_nodes} · 影响 {n_diffs}"))),
                    )
                    .into_any_element(),
            );
        }

        // —— 右列：选中线的影响清单 + 晋升 + 衍生 ——
        let sel_line = self
            .selected
            .clone()
            .and_then(|id| lines.iter().find(|w| s(w, "id") == id).cloned());
        let mut right: Vec<AnyElement> = Vec::new();
        if let Some(w) = &sel_line {
            let wid = s(w, "name");
            right.push(
                div()
                    .text_size(theme::FONT_META)
                    .font_weight(FontWeight::SEMIBOLD)
                    .text_color(theme::TEXT())
                    .child(SharedString::from(format!("「{wid}」影响清单 —— 晋升后才是提案，采纳后才改正典")))
                    .into_any_element(),
            );
            let diffs = w.get("diffs").and_then(|x| x.as_array()).cloned().unwrap_or_default();
            if diffs.is_empty() {
                right.push(
                    div().py(theme::PAD_MD).text_size(theme::FONT_LABEL).text_color(theme::FAINT())
                        .child("（还没有推演结果——从下方提出假设开线，AI 沿因果链走线后这里会亮起）")
                        .into_any_element(),
                );
            }
            for (idx, d) in diffs.into_iter().enumerate() {
                let target = s(&d, "target");
                let tref = s(&d, "targetRef");
                let before = if d.get("before").and_then(|b| b.as_str()).unwrap_or("—").is_empty() {
                    "—".to_string()
                } else {
                    s(&d, "before")
                };
                let after = s(&d, "after");
                let reason = s(&d, "reason");
                let sev = { let x = s(&d, "severity"); if x.is_empty() { "medium".into() } else { x } };
                let promoted = d.get("promoted").and_then(|p| p.as_bool()).unwrap_or(false);
                let (fg, bg) = if sev == "high" {
                    (theme::RED(), theme::RED_SOFT())
                } else {
                    (theme::AMBER(), theme::AMBER_SOFT())
                };
                let idx_u = idx;
                let promote_btn = Button::new(SharedString::from(format!("promote-{idx}")))
                    .xsmall()
                    .label("晋升为提案")
                    .on_click(cx.listener(move |this, _, _, cx| {
                        let wid = this.selected.clone().unwrap_or_default();
                        match this.handle.promote_diff(&wid, idx_u) {
                            Ok(pid) => {
                                this.message =
                                    Some(format!("已晋升 → 提案 {pid}（收件箱待裁决）").into());
                                cx.emit(OracleEvent::Promoted {
                                    worldline: wid.clone(),
                                    proposal_id: pid,
                                });
                            }
                            Err(e) => this.message = Some(format!("晋升失败：{e}").into()),
                        }
                        cx.notify();
                    }));
                right.push(
                    div()
                        .flex()
                        .items_start()
                        .gap(theme::GAP_MD)
                        .p(theme::PAD_SM)
                        .mb(theme::GAP_SM)
                        .rounded(theme::RADIUS_PANEL)
                        .border_1()
                        .border_color(theme::LINE())
                        .bg(theme::PANEL2())
                        .child(
                            Tag::custom(hsla(bg), hsla(fg), hsla(fg))
                                .outline()
                                .child(SharedString::from(sev)),
                        )
                        .child(
                            div()
                                .flex_1()
                                .min_w_0()
                                .child(
                                    div()
                                        .flex()
                                        .gap(theme::GAP_MD)
                                        .child(
                                            div()
                                                .flex_shrink_0()
                                                .font_family(theme::FAMILY_MONO)
                                                .text_size(theme::FONT_TAG)
                                                .text_color(theme::FAINT())
                                                .child(SharedString::from(format!("{target}·{tref}"))),
                                        )
                                        .child(
                                            div()
                                                .overflow_hidden()
                                                .whitespace_nowrap()
                                                .text_size(theme::FONT_TAG)
                                                .text_color(theme::DIM())
                                                .child(SharedString::from(format!(
                                                    "{} → {}",
                                                    before.chars().take(18).collect::<String>(),
                                                    after.chars().take(28).collect::<String>()
                                                ))),
                                        )
                                        .when(promoted, |x| {
                                            x.child(
                                                div()
                                                    .flex_shrink_0()
                                                    .text_size(theme::FONT_TAG)
                                                    .text_color(theme::GREEN())
                                                    .child("✓ 已晋升"),
                                            )
                                        }),
                                )
                                .child(
                                    div()
                                        .text_size(theme::FONT_MICRO)
                                        .line_height(relative(1.4))
                                        .text_color(theme::FAINT())
                                        .child(SharedString::from(reason)),
                                ),
                        )
                        .when(!promoted, move |d| d.child(promote_btn))
                        .into_any_element(),
                );
            }
            // 衍生实体（line 归属标）
            let dv = w.get("derived").cloned().unwrap_or_else(|| serde_json::json!({}));
            for key in ["characters", "canon", "clues"] {
                if let Some(arr) = dv.get(key).and_then(|c| c.as_array()) {
                    for x in arr.iter().filter_map(|v| v.as_str()) {
                        right.push(
                            div()
                                .flex()
                                .items_center()
                                .gap(theme::GAP_SM)
                                .py(theme::GAP_XS)
                                .child(
                                    Tag::custom(
                                        hsla(theme::PANEL()),
                                        hsla(theme::DIM()),
                                        hsla(theme::LINE()),
                                    )
                                    .outline()
                                    .child(match key {
                                        "characters" => "人物",
                                        "canon" => "设定",
                                        _ => "伏笔",
                                    }),
                                )
                                .child(
                                    div()
                                        .text_size(theme::FONT_LABEL)
                                        .text_color(theme::DIM())
                                        .child(SharedString::from(x.to_string())),
                                )
                                .into_any_element(),
                        );
                    }
                }
            }
        } else {
            right.push(
                div()
                    .pt(theme::PAD_XL)
                    .text_center()
                    .text_size(theme::FONT_LABEL)
                    .text_color(theme::FAINT())
                    .child("选一条世界线看影响清单；或从下方提出新假设。")
                    .into_any_element(),
            );
        }

        // —— 底部 composer ——
        let premise = self.premise.clone();
        let fork = self.fork.clone();
        let msg = self.message.clone();
        let composer = div()
            .flex()
            .flex_col()
            .gap(theme::GAP_XS)
            .p(theme::PAD_MD)
            .border_t_1()
            .border_color(theme::LINE())
            .bg(theme::PANEL())
            .when_some(msg, |d, m| {
                d.child(
                    div()
                        .text_size(theme::FONT_LABEL)
                        .text_color(theme::GREEN())
                        .child(m),
                )
            })
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(theme::GAP_SM)
                    .child(div().flex_1().min_w_0().child(Input::new(&premise)))
                    .child(div().w(px(64.)).child(Input::new(&fork)))
                    .child(
                        Button::new("oracle-run")
                            .primary()
                            .xsmall()
                            .label("开线推演")
                            .on_click(cx.listener(|this, _, _, cx| {
                                let p = this.premise.read(cx).value().trim().to_string();
                                if p.is_empty() {
                                    this.message = Some("先写一句假设（“如果……”）".into());
                                    cx.notify();
                                    return;
                                }
                                let f: u32 = this
                                    .fork
                                    .read(cx)
                                    .value()
                                    .trim()
                                    .parse()
                                    .unwrap_or(0);
                                cx.emit(OracleEvent::Simulate { premise: p, fork: f });
                                this.message = Some("已交给 AI 推演（右侧轨迹窗可见全程）".into());
                                cx.notify();
                            })),
                    ),
            );

        div()
            .size_full()
            .flex()
            .flex_col()
            .bg(theme::BG())
            .child(
                div()
                    .id("oracle-scroll")
                    .flex_1()
                    .min_h_0()
                    .flex()
                    .gap(theme::GAP_GROUP)
                    .p(theme::PAD_MD)
                    .overflow_y_scroll()
                    .child(div().w(px(280.)).flex_shrink_0().flex().flex_col().children(list))
                    .child(div().flex_1().min_w_0().flex().flex_col().gap(theme::GAP_XS).children(right)),
            )
            .child(composer)
    }
}

impl BasePanel for OraclePanel {
    fn panel_name(&self) -> &'static str {
        "oracle-view"
    }
}

impl ComponentPanel for OraclePanel {
    fn tab_name(&self, _: &App) -> Option<SharedString> {
        Some("预言书".into())
    }

    fn title(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div()
            .text_size(theme::FONT_BODY)
            .font_weight(FontWeight::MEDIUM)
            .text_color(theme::TEXT())
            .child("预言书 · 多重世界线")
    }
}
