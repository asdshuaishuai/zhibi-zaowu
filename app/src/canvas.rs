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
    /// 计划事件（happened=false，来自计划排期/预测线）：虚线框 + 计划标
    planned: bool,
    x: f32,
    y: f32,
}

#[derive(Clone)]
pub struct CEdge {
    from: usize,
    to: usize,
    kind: &'static str, // influence|plant|payoff|involve|conflict
    hidden: bool,
    /// 动态边标签（如关系谓词「师徒」）；空则用 kind 词
    label: String,
}

#[derive(Clone, Copy, PartialEq)]
pub enum Dim {
    Overview,
    Canon,
    Cast,
    Relations,
    Timeline,
    Events,
    Turns,
    Oracle,
}

const DIMS: [(Dim, &'static str); 8] = [
    (Dim::Overview, "总览"),
    (Dim::Canon, "设定"),
    (Dim::Cast, "人物"),
    (Dim::Relations, "关系"),
    (Dim::Timeline, "时间"),
    (Dim::Events, "事件"),
    (Dim::Turns, "转折"),
    (Dim::Oracle, "预言"),
];

enum Drag {
    Node(Vec<usize>, Point<Pixels>),
    Pan(Point<Pixels>),
    Marquee(Point<Pixels>),
}

const NODE_W: f32 = 150.;
const NODE_H: f32 = 76.;

pub struct CanvasPanel {
    focus_handle: FocusHandle,
    dim: Dim,
    handle: Rc<ProjectHandle>,
    nodes: Vec<CNode>,
    edges: Vec<CEdge>,
    view: (f32, f32, f32), // x, y, k
    sel: Vec<usize>,
    drag: Option<Drag>,
    /// 预言维选中的世界线 id（None=最新非归档线）
    oracle_line: Option<String>,
    /// 橡皮筋（窗口坐标；paint 时直接画，窗口首帧未写回 stage 原点也不影响）
    marquee: Option<((f32, f32), (f32, f32))>,
    /// canvas prepaint 每帧写回的 stage 窗口原点 → 事件坐标可换算
    stage_origin: Rc<RefCell<Option<(f32, f32)>>>,
    /// 节点按下已消费本次点击：Shift 取消选中时 drag=None，不能让事件继续冒泡去开框选
    absorb_click: bool,
    reader: bool,
    /// 「此刻看第几章」——关系维按它过滤区间，画的是“这一章时他们的关系”。
    chapter: u32,
    /// 最新成稿章（滑块的右端）。
    last_chapter: u32,
    /// 关系编辑表单：对方人名 / 关系标签 / 起始章（终点留空=至今）
    rel_to: Entity<InputState>,
    rel_label: Entity<InputState>,
    rel_from: Entity<InputState>,
}

/// 侧板里的小节标题（比 sec_title 更紧凑，面板已经很小）。
fn sec_label_mini(text: &'static str) -> gpui_kit::AnyElement {
    div()
        .pt(theme::GAP_SM)
        .text_size(theme::FONT_MICRO)
        .font_weight(FontWeight::SEMIBOLD)
        .text_color(theme::FAINT())
        .child(text)
        .into_any_element()
}

fn s(v: &serde_json::Value, key: &str) -> String {
    v.get(key).and_then(|x| x.as_str()).unwrap_or("").to_string()
}

impl CanvasPanel {
    pub fn new(window: &mut Window, cx: &mut Context<Self>, handle: Rc<ProjectHandle>) -> Self {
        let rel_to = cx.new(|c| InputState::new(window, c).placeholder("对方人名"));
        let rel_label = cx.new(|c| InputState::new(window, c).placeholder("关系：师从/敌对/结义…"));
        let rel_from = cx.new(|c| InputState::new(window, c).placeholder("起始章"));
        let mut this = Self {
            focus_handle: cx.focus_handle(),
            dim: Dim::Overview,
            handle,
            nodes: Vec::new(),
            edges: Vec::new(),
            view: (0., 0., 1.),
            sel: Vec::new(),
            drag: None,
            oracle_line: None,
            marquee: None,
            stage_origin: Rc::new(RefCell::new(None)),
            absorb_click: false,
            reader: false,
            chapter: 1,
            last_chapter: 1,
            rel_to,
            rel_label,
            rel_from,
        };
        this.refresh();
        this
    }

    /// 正典 → 节点/边 重建（按当前维度）；视图态从 canvas.json 恢复（pos 键 dim:id）。
    pub fn refresh(&mut self) {
        let dimkey = || match self.dim {
            Dim::Overview => "ov",
            Dim::Canon => "ca",
            Dim::Cast => "ch",
            Dim::Relations => "rl",
            Dim::Timeline => "tl",
            Dim::Events => "ev",
            Dim::Turns => "tr",
            Dim::Oracle => "or",
        };
        let state = self.handle.canvas_state();
        if let Some(v) = state.get("view") {
            self.view = (
                v.get("x").and_then(|x| x.as_f64()).unwrap_or(0.) as f32,
                v.get("y").and_then(|x| x.as_f64()).unwrap_or(0.) as f32,
                v.get("k").and_then(|x| x.as_f64()).unwrap_or(1.) as f32,
            );
        }
        self.reader = state.get("reader").and_then(|r| r.as_bool()).unwrap_or(false);
        // 滑块上限 = 最新成稿章（不含未来）；关系维按 chapter 过滤区间
        self.last_chapter = self.handle.next_chapter_number().saturating_sub(1).max(1);
        if self.chapter == 0 {
            self.chapter = 1;
        }
        if self.chapter > self.last_chapter {
            self.chapter = self.last_chapter;
        }

        let mut nodes: Vec<CNode> = Vec::new();
        let mut edges: Vec<CEdge> = Vec::new();
        let ev_all = self.handle.events();
        let facts = self.handle.memory_facts();
        let aliases_all = self.handle.aliases();

        // 别名归一表：所有名字形态 → canonicalName
        let mut alias_map: Vec<(String, String)> = Vec::new();
        for a in &aliases_all {
            let canon = s(a, "canonicalName");
            if canon.is_empty() {
                continue;
            }
            alias_map.push((canon.clone(), canon.clone()));
            if let Some(arr) = a.get("aliases").and_then(|v| v.as_array()) {
                for x in arr.iter().filter_map(|v| v.as_str()) {
                    alias_map.push((x.to_string(), canon.clone()));
                }
            }
        }
        let resolve = |name: &str| -> Option<String> {
            alias_map.iter().find(|(k, _)| k == name).map(|(_, v)| v.clone())
        };

        let col = |kind: &str| match kind {
            "storyline" => 60.,
            "event" => 420.,
            "character" => 800.,
            "clue" => 1120.,
            "canon" => 1420.,
            _ => 900.,
        };
        let mut counts = [0usize; 6];
        let push_col = |nodes: &mut Vec<CNode>, counts: &mut [usize; 6], kind: &'static str, id: String, label: String, meta: String, hidden: bool| {
            let ci = match kind {
                "storyline" => 0,
                "event" => 1,
                "character" => 2,
                "clue" => 3,
                "canon" => 4,
                _ => 5,
            };
            let y = 60. + counts[ci] as f32 * (NODE_H + 92.);
            counts[ci] += 1;
            nodes.push(CNode { id, kind, label, meta, hidden, planned: false, x: col(kind), y });
        };

        let want_lines = matches!(self.dim, Dim::Overview | Dim::Events);
        let want_events = !matches!(self.dim, Dim::Canon | Dim::Cast | Dim::Relations | Dim::Oracle);
        let want_chars = matches!(self.dim, Dim::Overview | Dim::Cast | Dim::Relations);
        let want_clues = matches!(self.dim, Dim::Overview);
        let want_canon = matches!(self.dim, Dim::Overview | Dim::Canon);

        if want_lines {
            for l in self.handle.storylines() {
                push_col(&mut nodes, &mut counts, "storyline", s(&l, "id"), s(&l, "name"), s(&l, "status"), false);
            }
        }
        if want_events {
            for e in &ev_all {
                let revealed = e.get("revealed").and_then(|r| r.as_bool()).unwrap_or(true);
                let fact = s(e, "objectiveFact");
                let chap = e.get("chapter").and_then(|c| c.as_u64()).unwrap_or(0);
                push_col(
                    &mut nodes,
                    &mut counts,
                    "event",
                    s(e, "id"),
                    fact.chars().take(14).collect(),
                    format!("第{chap}章 · {}", if revealed { "读者已知" } else { "未揭示·暗线" }),
                    !revealed,
                );
            }
        }
        if want_chars {
            for a in &aliases_all {
                let canon = s(a, "canonicalName");
                let aka = a
                    .get("aliases")
                    .and_then(|v| v.as_array())
                    .map(|arr| arr.iter().filter_map(|x| x.as_str()).collect::<Vec<_>>().join("、"))
                    .unwrap_or_default();
                push_col(&mut nodes, &mut counts, "character", format!("A:{canon}"), canon, aka, false);
            }
        }
        if want_clues {
            for c in self.handle.clues_full() {
                let planted = c.get("plantedChapter").and_then(|v| v.as_u64()).unwrap_or(0);
                push_col(&mut nodes, &mut counts, "clue", s(&c, "id"), s(&c, "title"), format!("{} · 埋第{planted}章", s(&c, "status")), false);
            }
        }
        if want_canon {
            for (title, category, _) in self.handle.canon_docs() {
                push_col(&mut nodes, &mut counts, "canon", format!("W:{title}"), title, category, false);
            }
        }

        // 事件/伏笔数量多时单列太高、连线成束：总览维拆双列（事件两列、伏笔两列），
        // 高度减半、边捆散开——参照常见分层图做法。
        if matches!(self.dim, Dim::Overview) {
            let mut ei = 0usize;
            let mut ci = 0usize;
            for n in nodes.iter_mut() {
                if n.kind == "event" {
                    n.x = 420. + (ei % 2) as f32 * 310.;
                    n.y = 80. + (ei / 2) as f32 * (NODE_H + 92.);
                    ei += 1;
                } else if n.kind == "clue" {
                    n.x = 1400. + (ci % 2) as f32 * 280.;
                    n.y = 80. + (ci / 2) as f32 * (NODE_H + 92.);
                    ci += 1;
                } else if n.kind == "canon" {
                    n.x += 360.;
                }
            }
        }

        // —— 分维边与专布局 ——
        match self.dim {
            Dim::Overview | Dim::Events | Dim::Cast => {
                // 线→事件
                for e in &ev_all {
                    let eid = s(e, "id");
                    let revealed = e.get("revealed").and_then(|r| r.as_bool()).unwrap_or(true);
                    if let Some(sids) = e.get("storylineIDs").and_then(|x| x.as_array()).cloned() {
                        for sid in sids.iter().filter_map(|x| x.as_str()) {
                            if let (Some(fi), Some(ti)) = (
                                nodes.iter().position(|n| n.id == sid),
                                nodes.iter().position(|n| n.id == eid),
                            ) {
                                edges.push(CEdge { from: fi, to: ti, kind: "influence", hidden: !revealed, label: String::new() });
                            }
                        }
                    }
                }
                if matches!(self.dim, Dim::Overview) {
                    // 伏笔埋/收
                    for c in self.handle.clues_full() {
                        let cid = s(&c, "id");
                        let planted = c.get("plantedChapter").and_then(|v| v.as_u64());
                        let last = c.get("lastActionChapter").and_then(|v| v.as_u64());
                        let status = s(&c, "status");
                        for e in &ev_all {
                            let ch = e.get("chapter").and_then(|v| v.as_u64());
                            let eid = s(e, "id");
                            if let (Some(ci), Some(ei)) = (
                                nodes.iter().position(|n| n.id == cid),
                                nodes.iter().position(|n| n.id == eid),
                            ) {
                                if planted.is_some() && planted == ch {
                                    edges.push(CEdge { from: ci, to: ei, kind: "plant", hidden: false, label: String::new() });
                                }
                                if status == "已兑现" && last.is_some() && last == ch {
                                    edges.push(CEdge { from: ei, to: ci, kind: "payoff", hidden: false, label: String::new() });
                                }
                            }
                        }
                    }
                }
                // 人物↔事件（facts subject×chapter）
                if matches!(self.dim, Dim::Overview | Dim::Cast) {
                    for f in &facts {
                        let subj = s(f, "subject");
                        let Some(canon) = resolve(&subj) else { continue };
                        let ch = f.get("fromChapter").and_then(|v| v.as_u64());
                        let Some(ci) = nodes.iter().position(|n| n.kind == "character" && n.label == canon) else { continue };
                        for e in &ev_all {
                            if e.get("chapter").and_then(|v| v.as_u64()) == ch {
                                if let Some(ei) = nodes.iter().position(|n| n.id == s(e, "id")) {
                                    let has = edges.iter().any(|x| (x.from == ci && x.to == ei) || (x.from == ei && x.to == ci));
                                    let revealed = e.get("revealed").and_then(|r| r.as_bool()).unwrap_or(true);
                                    if !has {
                                        edges.push(CEdge {
                                            from: ci,
                                            to: ei,
                                            kind: "involve",
                                            hidden: !revealed,
                                            label: s(f, "predicate"),
                                        });
                                    }
                                }
                            }
                        }
                    }
                }
            }
            Dim::Canon => {
                // 设定网格 + facts.object 命中标题 → 人物牵设定
                let titles: Vec<String> = nodes.iter().filter(|n| n.kind == "canon").map(|n| n.label.clone()).collect();
                for f in &facts {
                    let obj = s(f, "object");
                    let Some(hit) = titles.iter().find(|t| !t.is_empty() && (obj == **t || obj.contains(t.as_str()) || t.contains(&obj))).cloned() else { continue };
                    let Some(wi) = nodes.iter().position(|n| n.kind == "canon" && n.label == hit) else { continue };
                    let subj_canon = resolve(&s(f, "subject"));
                    let from = subj_canon
                        .as_ref()
                        .and_then(|c| nodes.iter().position(|n| n.kind == "canon" && n.label == *c));
                    // 主体不是设定则连「事件锚」跳过；网内只留设定↔设定（人名不建节点）
                    if let Some(fi) = from {
                        if fi != wi {
                            let has = edges.iter().any(|x| (x.from == fi && x.to == wi) || (x.from == wi && x.to == fi));
                            if !has {
                                edges.push(CEdge { from: fi, to: wi, kind: "influence", hidden: false, label: s(f, "predicate") });
                            }
                        }
                    }
                }
                // 网格布局（4 列）
                let mut ci = 0usize;
                for n in nodes.iter_mut() {
                    n.x = 80. + (ci % 3) as f32 * 300.;
                    n.y = 80. + (ci / 3) as f32 * (NODE_H + 92.);
                    ci += 1;
                }
            }
            Dim::Relations => {
                // 读 relations.json 的区间模型，按「当前查看章」过滤。
                //
                // 为什么不再从 facts 推：facts 只有起点、没有终点，也存不下“同一人
                // 不同章不同关系”。章节滑块的意义就是“此刻他们是什么关系”。
                let chapter = self.chapter;
                let web = crate::relations::Web::new(&self.handle.root);
                let mut live = web.at_chapter(chapter);
                // 首次使用：没有 relations.json 但有 facts 时迁移一次（不覆盖已有）
                if live.is_empty() && web.all().is_empty() {
                    let people: Vec<String> = nodes
                        .iter()
                        .filter(|n| n.kind == "character")
                        .map(|n| n.label.clone())
                        .collect();
                    let _ = web.migrate_from_facts(&facts, &people);
                    live = web.at_chapter(chapter);
                }
                for rel in &live {
                    let (Some(fi), Some(ti)) = (
                        nodes
                            .iter()
                            .position(|n| n.kind == "character" && n.label == rel.from),
                        nodes
                            .iter()
                            .position(|n| n.kind == "character" && n.label == rel.to),
                    ) else {
                        continue;
                    };
                    if fi == ti {
                        continue;
                    }
                    // 一段区间一条边：同期并存的多段关系（如“师门”+“敌对”）都要画出来
                    for iv in rel.active_at(chapter) {
                        let hidden = !iv.public_to_reader;
                        let kind = match rel.kind.as_str() {
                            "敌对" => "hostile",
                            "亲缘" | "情爱" => "kin",
                            "师承" => "mentor",
                            _ => "influence",
                        };
                        let dup = edges
                            .iter()
                            .any(|e| e.from == fi && e.to == ti && e.label == iv.label);
                        if !dup {
                            edges.push(CEdge {
                                from: fi,
                                to: ti,
                                kind,
                                hidden,
                                label: iv.label.clone(),
                            });
                        }
                    }
                }
                // 中心辐射：出度最大者居中，其余圆周。
                // 节点为空时 n_count=0，下面的循环一次都不跑（旧实现 max(1) 会索引 nodes[0] panic）。
                let n_count = nodes.len();
                let mut best = 0usize;
                let mut best_deg = -1isize;
                for i in 0..n_count {
                    let deg = edges.iter().filter(|e| e.from == i || e.to == i).count() as isize;
                    if deg > best_deg {
                        best_deg = deg;
                        best = i;
                    }
                }
                let cx = 520f32;
                let cy = 420f32;
                let mut ring = 0usize;
                for i in 0..n_count {
                    if i == best {
                        nodes[i].x = cx - NODE_W / 2.;
                        nodes[i].y = cy - NODE_H / 2.;
                    } else {
                        let step = std::f32::consts::TAU / (n_count - 1).max(2) as f32;
                        let ang = step * ring as f32 - std::f32::consts::FRAC_PI_2;
                        nodes[i].x = cx + 330. * ang.cos() - NODE_W / 2.;
                        nodes[i].y = cy + 280. * ang.sin() - NODE_H / 2.;
                        ring += 1;
                    }
                }
            }
            Dim::Timeline => {
                // 章序时间线：横向折行 + 顺序链
                nodes.sort_by(|a, b| {
                    let ca = ev_all.iter().find(|e| s(e, "id") == a.id).and_then(|e| e.get("chapter")).and_then(|v| v.as_u64()).unwrap_or(0);
                    let cb = ev_all.iter().find(|e| s(e, "id") == b.id).and_then(|e| e.get("chapter")).and_then(|v| v.as_u64()).unwrap_or(0);
                    ca.cmp(&cb)
                });
                for (i, n) in nodes.iter_mut().enumerate() {
                    n.x = 60. + (i % 5) as f32 * 250.;
                    n.y = 80. + (i / 5) as f32 * (NODE_H + 96.);
                }
                for i in 0..nodes.len().saturating_sub(1) {
                    edges.push(CEdge { from: i, to: i + 1, kind: "influence", hidden: false, label: String::new() });
                }
            }
            Dim::Oracle => {
                // 世界线沙盒：渲染一条预测/假设线的节点与边（AI simulate 写入的 nodes/edges）
                let lines = self.handle.worldlines();
                let pick = self.oracle_line.clone();
                let w = lines
                    .iter()
                    .find(|w| {
                        s(w, "id") != "α"
                            && s(w, "status") != "已归档"
                            && pick.as_deref().map(|p| s(w, "id") == p).unwrap_or(true)
                    })
                    .or_else(|| lines.iter().find(|w| s(w, "id") != "α"));
                if let Some(w) = w {
                    let lid = s(w, "id");
                    let nn = w.get("nodes").and_then(|v| v.as_array()).cloned().unwrap_or_default();
                    let has_xy = nn.iter().any(|n| {
                        n.get("x").and_then(|v| v.as_f64()).unwrap_or(0.).abs() > 1.
                            || n.get("y").and_then(|v| v.as_f64()).unwrap_or(0.).abs() > 1.
                    });
                    for nj in nn.iter() {
                        let raw_kind = s(nj, "kind");
                        let kind: &'static str = match raw_kind.as_str() {
                            "storyline" => "storyline",
                            "character" => "character",
                            "clue" => "clue",
                            "canon" => "canon",
                            _ => "event",
                        };
                        let (x, y) = if has_xy {
                            (
                                nj.get("x").and_then(|v| v.as_f64()).unwrap_or(0.) as f32,
                                nj.get("y").and_then(|v| v.as_f64()).unwrap_or(0.) as f32,
                            )
                        } else {
                            let ci = match kind {
                                "storyline" => 0,
                                "event" => 1,
                                "character" => 2,
                                "clue" => 3,
                                _ => 4,
                            };
                            let idx_in_col = nodes.iter().filter(|n: &&CNode| n.kind == kind).count();
                            let _ = ci;
                            (col("event") + idx_in_col as f32 * 210., 80. + (idx_in_col % 3) as f32 * 150.)
                        };
                        nodes.push(CNode {
                            id: s(nj, "id"),
                            kind,
                            label: s(nj, "label").chars().take(16).collect(),
                            meta: format!("线{lid} · {}", if raw_kind.is_empty() { "event".to_string() } else { raw_kind }),
                            hidden: false,
                            planned: true, // 预测/假设本身就是「未定」——统一虚线
                            x,
                            y,
                        });
                    }
                    let en = w.get("edges").and_then(|v| v.as_array()).cloned().unwrap_or_default();
                    for ej in en {
                        let fid = s(&ej, "from");
                        let tid = s(&ej, "to");
                        if let (Some(fi), Some(ti)) = (
                            nodes.iter().position(|n| n.id == fid),
                            nodes.iter().position(|n| n.id == tid),
                        ) {
                            edges.push(CEdge {
                                from: fi,
                                to: ti,
                                kind: "influence",
                                hidden: ej.get("hidden").and_then(|v| v.as_bool()).unwrap_or(false),
                                label: s(&ej, "kind"),
                            });
                        }
                    }
                }
            }
            Dim::Turns => {
                // 事件主循环已建列；此处只加档案卡拐点节点与红边
                let dossiers = self.handle.read_array("dossiers.json");
                let mut twist_i = 0usize;
                for d in &dossiers {
                    let chap = d.get("chapter").and_then(|v| v.as_u64()).unwrap_or(0);
                    let turns = d.get("turns").and_then(|v| v.as_array()).cloned().unwrap_or_default();
                    for (ti, t) in turns.iter().enumerate() {
                        let Some(txt) = t.as_str() else { continue };
                        let node_i = nodes.len();
                        nodes.push(CNode {
                            id: format!("D{chap}T{ti}"),
                            kind: "twist",
                            label: txt.chars().take(16).collect(),
                            meta: format!("第{chap}章 拐点"),
                            hidden: false,
                            planned: false,
                            x: 480.,
                            y: 80. + twist_i as f32 * (NODE_H + 84.),
                        });
                        twist_i += 1;
                        // 连该章事件（含该章所有事件）
                        for e in &ev_all {
                            if e.get("chapter").and_then(|v| v.as_u64()) != Some(chap) {
                                continue;
                            }
                            let eid = s(e, "id");
                            if let Some(ei) = nodes.iter().position(|n| n.kind == "event" && n.id == eid) {
                                edges.push(CEdge { from: node_i, to: ei, kind: "conflict", hidden: false, label: "转折于".into() });
                            }
                        }
                    }
                }
            }
        }

        // 计划事件标记（happened=false，来自 plannedEvent 采纳或预测排期）：
        // 虚线框 + 计划标，与已发生事件区分
        for n in nodes.iter_mut() {
            if n.kind != "event" {
                continue;
            }
            if let Some(ev) = ev_all.iter().find(|e| s(e, "id") == n.id) {
                if !ev.get("happened").and_then(|v| v.as_bool()).unwrap_or(true) {
                    n.planned = true;
                    let line = s(ev, "plannedFrom");
                    n.meta = if line.is_empty() {
                        format!("计划 · {}", n.meta)
                    } else {
                        format!("计划({line}) · {}", n.meta)
                    };
                }
            }
        }

        // 恢复持久化坐标（dim:id 键）
        if let Some(pos) = state.get("pos").and_then(|p| p.as_object()) {
            let dk = dimkey();
            for n in nodes.iter_mut() {
                let key = format!("{dk}:{}", n.id);
                if let Some(xy) = pos.get(&key).and_then(|v| v.as_array()) {
                    if xy.len() == 2 {
                        n.x = xy[0].as_f64().unwrap_or(n.x as f64) as f32;
                        n.y = xy[1].as_f64().unwrap_or(n.y as f64) as f32;
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
        let dimkey = match self.dim {
            Dim::Overview => "ov",
            Dim::Canon => "ca",
            Dim::Cast => "ch",
            Dim::Relations => "rl",
            Dim::Timeline => "tl",
            Dim::Events => "ev",
            Dim::Turns => "tr",
            Dim::Oracle => "or",
        };
        let mut pos = serde_json::Map::new();
        // 合并旧值：只保留「其他维度」的 dim:id 键。旧版本留下的裸 id 键、已删除节点的
        // 本维键都在这里清掉，否则 canvas.json 会随改名/删节点无限膨胀。
        let known_dims = ["ov", "ca", "ch", "rl", "tl", "ev", "tr"];
        if let Some(old) = self.handle.canvas_state().get("pos").and_then(|p| p.as_object()) {
            for (k, v) in old {
                if let Some((prefix, _)) = k.split_once(':') {
                    if prefix != dimkey && known_dims.contains(&prefix) {
                        pos.insert(k.clone(), v.clone());
                    }
                }
            }
        }
        for n in &self.nodes {
            pos.insert(format!("{dimkey}:{}", n.id), serde_json::json!([n.x, n.y]));
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
            let kind = if !e.label.is_empty() {
                e.label.clone()
            } else {
                match e.kind {
                    "plant" => "埋设".to_string(),
                    "payoff" => "兑现".to_string(),
                    "involve" => "牵涉".to_string(),
                    "conflict" => "转折".to_string(),
                    _ => "影响".to_string(),
                }
            };
            out.push((format!("{kind}{dir}{}", if e.hidden { "·暗" } else { "" }), name));
        }
        out.truncate(12);
        out
    }

    /// 外部跳转：直接切到预言维并选中某条世界线（预言书「在画布查看」）。
    pub fn show_line(&mut self, id: String) {
        self.dim = Dim::Oracle;
        self.oracle_line = Some(id);
        self.refresh();
    }

    /// 切换画布维度：先存当前维位置再重建（pos 键按维隔离；view 是全局视图，跨维保留——
    /// 旧实现在这里重置 view 后立刻被 refresh 从 canvas.json 覆盖，重置是死代码）。
    fn set_dim(&mut self, d: Dim, cx: &mut Context<Self>) {
        if d == self.dim {
            return;
        }
        self.save_state();
        self.dim = d;
        self.refresh();
        cx.notify();
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
        // 选中节点时只亮与它相关的边，其余压到 28%（降噪：密集图的可读性关键）
        let focus = self.sel.last().copied();
        let mut segs: Vec<(f32, f32, f32, f32, &'static str, bool, f32)> = Vec::new();
        for e in &self.edges {
            if reader && e.hidden {
                continue;
            }
            let a = &self.nodes[e.from];
            let b = &self.nodes[e.to];
            let alpha = match focus {
                Some(f) if e.from == f || e.to == f => 0.95,
                Some(_) => 0.28,
                None => 0.55,
            };
            segs.push((self.sx(a), self.sy(a), self.sx(b), self.sy(b), e.kind, e.hidden, alpha));
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
                            // marquee 存窗口坐标，直接画（不依赖 stage 原点是否已写回）
                            let mut r = PathBuilder::stroke(px(1.2));
                            r.move_to(point(px(ax), px(ay)));
                            r.line_to(point(px(bx), px(ay)));
                            r.line_to(point(px(bx), px(by)));
                            r.line_to(point(px(ax), px(by)));
                            r.line_to(point(px(ax), px(ay)));
                            if let Ok(path) = r.build() {
                                window.paint_path(path, hsla(theme::ACCENT()));
                            }
                        }
                        for (x1, y1, x2, y2, kind, hidden, alpha) in &segs {
                            let color = if *hidden {
                                theme::PURPLE()
                            } else {
                                match *kind {
                                    "plant" | "payoff" => theme::AMBER(),
                                    "involve" => theme::GREEN(),
                                    _ => theme::DIM(),
                                }
                            };
                            let mut b = PathBuilder::stroke(px(1.15));
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
                                window.paint_path(path, hsla(color).opacity(*alpha));
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
                "twist" => theme::RED(),
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
                .border_color(if sel {
                    theme::ACCENT_BORDER()
                } else if n.planned {
                    theme::AMBER()
                } else {
                    theme::LINE()
                })
                .when(n.planned && !sel, |d| d.border_dashed())
                .bg(if sel { theme::ACCENT_SOFT() } else { theme::PANEL() })
                .on_mouse_down(
                    MouseButton::Left,
                    cx.listener(move |this, ev: &MouseDownEvent, _, cx| {
                        // 节点消费本次按下：即使下面把 drag 置 None（Shift 取消选中），
                        // 事件冒泡到舞台时也不能再开橡皮筋。
                        this.absorb_click = true;
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
                // 节点按下处理器先执行（bubble）：已消费的点击不再进入舞台手势
                if this.absorb_click {
                    this.absorb_click = false;
                    return;
                }
                if this.drag.is_none() {
                    if ev.modifiers.shift {
                        // 空白 Shift 按下：橡皮筋框选（窗口坐标，首帧不依赖 stage_origin）
                        let x = ev.position.x.as_f32();
                        let y = ev.position.y.as_f32();
                        this.marquee = Some(((x, y), (x, y)));
                        this.drag = Some(Drag::Marquee(ev.position));
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
                            if let Some(n) = this.nodes.get_mut(i) {
                                n.x += dx;
                                n.y += dy;
                            }
                        }
                        *last = ev.position;
                        cx.notify();
                    }
                    Some(Drag::Marquee(start)) => {
                        let end = (ev.position.x.as_f32(), ev.position.y.as_f32());
                        let start = (start.x.as_f32(), start.y.as_f32());
                        this.marquee = Some((start, end));
                        cx.notify();
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
                        // marquee 是窗口坐标；节点中心是 stage 局部坐标，比较前补上 stage 原点
                        let (ox, oy) = (*this.stage_origin.borrow()).unwrap_or((0., 0.));
                        let (x0, x1) = (ax.min(bx), ax.max(bx));
                        let (y0, y1) = (ay.min(by), ay.max(by));
                        this.sel = this
                            .nodes
                            .iter()
                            .enumerate()
                            .filter(|(_, n)| {
                                if this.reader && n.hidden {
                                    return false;
                                }
                                let nx = this.sx(n) + ox;
                                let ny = this.sy(n) + oy;
                                nx > x0 && nx < x1 && ny > y0 && ny < y1
                            })
                            .map(|(i, _)| i)
                            .collect();
                    }
                    this.drag = None;
                    cx.notify();
                    return;
                }
                this.absorb_click = false;
                if this.drag.is_some() {
                    this.drag = None;
                    this.save_state();
                    cx.notify();
                }
            }))
            .on_mouse_down_out(cx.listener(|this, _, _, _| {
                // 指针拖出窗口：清干净拖拽态；平移/缩放已经改过 view，必须落盘
                let had_gesture = this.drag.is_some();
                this.drag = None;
                this.absorb_click = false;
                this.marquee = None;
                if had_gesture {
                    this.save_state();
                }
            }))
            .on_mouse_up_out(MouseButton::Left, cx.listener(|this, _, _, cx| {
                // 在舞台外松手（拖出窗口/侧板）：同样要结束手势并落盘
                if this.drag.is_some() || this.marquee.is_some() {
                    this.drag = None;
                    this.marquee = None;
                    this.absorb_click = false;
                    this.save_state();
                    cx.notify();
                }
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
            // 关系维：选中一个人时，给出「与谁、什么关系、从第几章起」的就地编辑，
            // 以及该人参与的关系史（含已结束的区间）与删除入口。
            .when(self.dim == Dim::Relations && self.sel.len() == 1, |d| {
                let me = self.nodes[self.sel[0]].label.clone();
                let web = crate::relations::Web::new(&self.handle.root);
                let mine: Vec<crate::relations::Relation> = web
                    .all()
                    .into_iter()
                    .filter(|r| r.from == me || r.to == me)
                    .collect();
                let mut hist = div().flex().flex_col().gap(px(2.)).pt(theme::GAP_SM);
                if mine.is_empty() {
                    hist = hist.child(
                        div()
                            .text_size(theme::FONT_MICRO)
                            .text_color(theme::FAINT())
                            .child("还没有关系记录"),
                    );
                }
                for r in mine.iter().take(8) {
                    let other = if r.from == me { r.to.clone() } else { r.from.clone() };
                    let span = r
                        .intervals
                        .iter()
                        .map(|i| {
                            let end = if i.to_chapter == 0 {
                                "今".to_string()
                            } else {
                                i.to_chapter.to_string()
                            };
                            format!("{}·{}-{}", i.label, i.from_chapter, end)
                        })
                        .collect::<Vec<_>>()
                        .join("；");
                    let rid = r.id.clone();
                    hist = hist.child(
                        div()
                            .flex()
                            .items_center()
                            .gap(px(4.))
                            .child(
                                div()
                                    .flex_1()
                                    .min_w_0()
                                    .text_size(theme::FONT_MICRO)
                                    .text_color(theme::DIM())
                                    .text_ellipsis()
                                    .child(SharedString::from(format!("{other}：{span}"))),
                            )
                            .child(
                                div()
                                    .id(SharedString::from(format!("rel-del-{rid}")))
                                    .cursor_pointer()
                                    .text_size(theme::FONT_MICRO)
                                    .text_color(theme::FAINT())
                                    .child("×")
                                    .on_click(cx.listener({
                                        let rid = rid.clone();
                                        move |this, _, _, cx| {
                                            let web = crate::relations::Web::new(&this.handle.root);
                                            // 先留前像再删：撤销要能逐字写回原来的关系表
                                            let before = std::fs::read_to_string(
                                                this.handle.root.join("relations.json"),
                                            )
                                            .unwrap_or_default();
                                            match web.remove(&rid) {
                                                Ok(()) => {
                                                    let after = std::fs::read_to_string(
                                                        this.handle.root.join("relations.json"),
                                                    )
                                                    .unwrap_or_default();
                                                    let _ = crate::journal::Journal::new(&this.handle.root)
                                                        .record(
                                                            &this.handle.root,
                                                            "relations.json",
                                                            &before,
                                                            &after,
                                                            &crate::journal::Actor::Author,
                                                            "remove",
                                                            &format!("b{}", crate::journal::now_millis()),
                                                            "删除关系",
                                                        );
                                                }
                                                Err(e) => eprintln!("删除关系失败：{e}"),
                                            }
                                            this.refresh();
                                            cx.notify();
                                        }
                                    })),
                            ),
                    );
                }
                d.child(sec_label_mini("关系史"))
                    .child(hist)
                    .child(sec_label_mini("新增关系"))
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .gap(px(4.))
                            .child(div().child(Input::new(&self.rel_to)))
                            .child(div().child(Input::new(&self.rel_label)))
                            .child(div().child(Input::new(&self.rel_from)))
                            .child(
                                div()
                                    .id("rel-add")
                                    .mt(px(2.))
                                    .px(px(8.))
                                    .py(px(2.))
                                    .rounded(px(5.))
                                    .bg(theme::ACCENT_SOFT())
                                    .border_1()
                                    .border_color(theme::ACCENT_BORDER())
                                    .cursor_pointer()
                                    .child(
                                        div()
                                            .text_size(theme::FONT_MICRO)
                                            .text_color(theme::ACCENT())
                                            .child("建立关系"),
                                    )
                                    .on_click(cx.listener(|this, _, _, cx| {
                                        let to = this.rel_to.read(cx).value().trim().to_string();
                                        let label = this.rel_label.read(cx).value().trim().to_string();
                                        let from: u32 = this
                                            .rel_from
                                            .read(cx)
                                            .value()
                                            .trim()
                                            .parse()
                                            .unwrap_or(0);
                                        let me = match this.sel.last() {
                                            Some(&i) => this.nodes[i].label.clone(),
                                            None => return,
                                        };
                                        if to.is_empty() || label.is_empty() {
                                            return;
                                        }
                                        let web = crate::relations::Web::new(&this.handle.root);
                                        let before = std::fs::read_to_string(
                                            this.handle.root.join("relations.json"),
                                        )
                                        .unwrap_or_default();
                                        let iv = crate::relations::Interval {
                                            from_chapter: from,
                                            to_chapter: 0,
                                            label: label.clone(),
                                            // 作者亲手建的关系默认读者可见；暗线由 AI 提案或后续编辑标记
                                            public_to_reader: true,
                                            evidence: String::new(),
                                        };
                                        let kind = crate::relations::classify(&label);
                                        match web.add_interval(&me, &to, &kind, iv) {
                                            Ok(_) => {
                                                let after = std::fs::read_to_string(
                                                    this.handle.root.join("relations.json"),
                                                )
                                                .unwrap_or_default();
                                                let _ = crate::journal::Journal::new(&this.handle.root)
                                                    .record(
                                                        &this.handle.root,
                                                        "relations.json",
                                                        &before,
                                                        &after,
                                                        &crate::journal::Actor::Author,
                                                        "edit",
                                                        &format!("b{}", crate::journal::now_millis()),
                                                        "新增关系",
                                                    );
                                            }
                                            Err(e) => eprintln!("建立关系失败：{e}"),
                                        }
                                        this.refresh();
                                        cx.notify();
                                    })),
                            ),
                    )
            })
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
            // 章节滑块：只在关系维出现。关系会随剧情演变，
            // “此刻他们是什么关系”必须能按章回看。
            .when(self.dim == Dim::Relations, |d| {
                let ch = self.chapter;
                let last = self.last_chapter;
                d.child(
                    div()
                        .flex()
                        .items_center()
                        .gap(px(6.))
                        .px(px(8.))
                        .py(px(2.))
                        .rounded(px(5.))
                        .bg(theme::PANEL2())
                        .border_1()
                        .border_color(theme::LINE())
                        .child(
                            div()
                                .id("rel-ch-prev")
                                .cursor_pointer()
                                .text_size(theme::FONT_MICRO)
                                .text_color(if ch > 1 { theme::TEXT() } else { theme::FAINT() })
                                .child("◀")
                                .on_click(cx.listener(|this, _, _, cx| {
                                    if this.chapter > 1 {
                                        this.chapter -= 1;
                                        this.refresh();
                                        cx.notify();
                                    }
                                })),
                        )
                        .child(
                            div()
                                .font_family(theme::FAMILY_MONO)
                                .text_size(theme::FONT_MICRO)
                                .text_color(theme::ACCENT())
                                .child(SharedString::from(format!("第{ch}/{last}章"))),
                        )
                        .child(
                            div()
                                .id("rel-ch-next")
                                .cursor_pointer()
                                .text_size(theme::FONT_MICRO)
                                .text_color(if ch < last { theme::TEXT() } else { theme::FAINT() })
                                .child("▶")
                                .on_click(cx.listener(|this, _, _, cx| {
                                    if this.chapter < this.last_chapter {
                                        this.chapter += 1;
                                        this.refresh();
                                        cx.notify();
                                    }
                                })),
                        )
                        .child(
                            div()
                                .id("rel-ch-now")
                                .cursor_pointer()
                                .ml(px(2.))
                                .text_size(theme::FONT_MICRO)
                                .text_color(theme::FAINT())
                                .child("现在")
                                .on_click(cx.listener(|this, _, _, cx| {
                                    this.chapter = this.last_chapter;
                                    this.refresh();
                                    cx.notify();
                                })),
                        ),
                )
            })
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

        // 维度页签（织网镜头切换）
        let tabs = div()
            .absolute()
            .left(theme::PAD_LG)
            .bottom(theme::PAD_LG)
            .flex()
            .items_center()
            .gap(theme::GAP_XS)
            .p(theme::GAP_XS)
            .rounded(theme::RADIUS_CARD)
            .border_1()
            .border_color(theme::LINE())
            .bg(theme::PANEL())
            .children(DIMS.iter().map(|(d, name)| {
                let on = self.dim == *d;
                let dd = *d;
                let b = Button::new(SharedString::from(format!("dim-{name}"))).xsmall().label(*name);
                let b = if on { b.primary() } else { b };
                b.on_click(cx.listener(move |this, _, _, cx| this.set_dim(dd, cx)))
            }));

        // 预言维：世界线选择条（非 α 线逐条切换）
        let mut oracle_switch: Option<gpui_kit::AnyElement> = None;
        if self.dim == Dim::Oracle {
            let lines: Vec<String> = self
                .handle
                .worldlines()
                .iter()
                .filter(|w| s(w, "id") != "α" && s(w, "status") != "已归档")
                .map(|w| s(w, "id"))
                .collect();
            let cur = self.oracle_line.clone();
            let mut row: Vec<gpui_kit::AnyElement> = vec![
                div()
                    .text_size(theme::FONT_TAG)
                    .text_color(theme::FAINT())
                    .child("世界线：")
                    .into_any_element(),
            ];
            if lines.is_empty() {
                row.push(
                    div()
                        .text_size(theme::FONT_TAG)
                        .text_color(theme::FAINT())
                        .child("（还没有预测/假设线——去「预言书」提假设或预测续写）")
                        .into_any_element(),
                );
            }
            for lid in lines {
                let on = cur.as_deref().map(|c| c == lid).unwrap_or(false);
                let pick = lid.clone();
                let b = Button::new(SharedString::from(format!("wl-pick-{lid}")))
                    .xsmall()
                    .label(SharedString::from(lid.clone()));
                let b = if on { b.primary() } else { b };
                row.push(
                    b.on_click(cx.listener(move |this, _, _, cx| {
                        this.oracle_line = Some(pick.clone());
                        this.refresh();
                        cx.notify();
                    }))
                    .into_any_element(),
                );
            }
            oracle_switch = Some(
                div()
                    .absolute()
                    .left(theme::PAD_LG)
                    .top(px(52.))
                    .flex()
                    .items_center()
                    .gap(theme::GAP_XS)
                    .p(theme::GAP_XS)
                    .rounded(theme::RADIUS_CARD)
                    .border_1()
                    .border_color(theme::LINE())
                    .bg(theme::PANEL())
                    .children(row)
                    .into_any_element(),
            );
        }

        let stage = match oracle_switch {
            Some(sw) => stage.child(side).child(bar).child(tabs).child(sw),
            None => stage.child(side).child(bar).child(tabs),
        };
        stage
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
    /// 从（线, 节点）分叉出一条新线
    ForkFrom { line: String, node: String, name: String },
    /// 节点字段改一条（章号 / 标题 / 备注 / 状态）
    NodePatch {
        line: String,
        node: String,
        patch: crate::worldline::NodePatch,
    },
    /// 删一个节点
    NodeRemove { line: String, node: String },
    /// 新增一个节点
    NodeAdd {
        line: String,
        kind: String,
        label: String,
        chapter: u32,
    },
    /// 预测续写（M8）：fork 自动=最新成稿下一章，AI 按大纲/成稿向前织
    Forecast,
    /// 晋升成功：宿主刷新收件箱/轨迹
    Promoted { worldline: String, proposal_id: String },
    /// 在画布上查看该世界线（切到预言维）
    ViewOnCanvas { worldline: String },
}

/// 车道里某条线指向的父线 id（没有就是空串）。
fn line_parent_id(lines: &[crate::worldline::Line], id: &str) -> String {
    lines
        .iter()
        .find(|l| l.id == id)
        .map(|l| l.parent_id.clone())
        .unwrap_or_default()
}

/// 线模式的节点详情抽屉：作者点车道上的节点时，就地看它、改它、从它分叉。
///
/// 做成一个函数而不是内联进 render 的原因：render 已经太长，而这块
/// 逻辑（选中节点 → 找它 → 拼表单）和画布坐标没有关系。
fn lane_detail(
    cx: &mut Context<OraclePanel>,
    lines: &[crate::worldline::Line],
    lane: &Option<String>,
    handle: &std::rc::Rc<ProjectHandle>,
) -> Option<gpui_kit::AnyElement> {
    let lid = lane.as_ref()?;
    let line = lines.iter().find(|l| &l.id == lid)?;
    let mut body = div().flex().flex_col().gap(px(3.));
    let mut nodes = line.nodes.clone();
    nodes.sort_by_key(|n| n.chapter_or_guess());
    if nodes.is_empty() {
        body = body.child(
            div()
                .text_size(theme::FONT_MICRO)
                .text_color(theme::FAINT())
                .child("这条线还没有节点。推演一次，或者让主 Agent 派 worldline 子 Agent。"),
        );
    }
    for n in nodes.iter().take(14) {
        let ch = n.chapter_or_guess();
        let nid = n.id.clone();
        let lid = line.id.clone();
        let patch_ch = nid.clone();
        let patch_lid = lid.clone();
        let del_n = nid.clone();
        let del_l = lid.clone();
        let fork_l = lid.clone();
        let fork_n = nid.clone();
        let fork_name: String = format!("{}·{}", line.name, n.label)
            .chars()
            .take(24)
            .collect();
        body = body.child(
            div()
                .flex()
                .items_center()
                .gap(theme::GAP_SM)
                .py(px(1.))
                .child(
                    // 章号：点一下 +1（改排章是最常做的事，同 M6 台账的就地微调）
                    div()
                        .id(SharedString::from(format!("wn-ch-{lid}-{nid}")))
                        .w(px(46.))
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
                        .on_click(cx.listener(move |_this, _, _, cx| {
                            cx.emit(OracleEvent::NodePatch {
                                line: patch_lid.clone(),
                                node: patch_ch.clone(),
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
                    div()
                        .text_size(px(9.))
                        .text_color(if n.state == crate::worldline::NodeState::Written {
                            theme::GREEN()
                        } else {
                            theme::FAINT()
                        })
                        .child(n.state.label()),
                )
                .child(
                    // 从此节点再分叉：树可以一层层长下去
                    div()
                        .id(SharedString::from(format!("wn-fork-{lid}-{nid}")))
                        .flex_shrink_0()
                        .cursor_pointer()
                        .text_size(theme::FONT_MICRO)
                        .text_color(theme::FAINT())
                        .child("⤴")
                        .on_click(cx.listener(move |_this, _, _, cx| {
                            cx.emit(OracleEvent::ForkFrom {
                                line: fork_l.clone(),
                                node: fork_n.clone(),
                                name: fork_name.clone(),
                            });
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
                        .on_click(cx.listener(move |_this, _, _, cx| {
                            cx.emit(OracleEvent::NodeRemove {
                                line: del_l.clone(),
                                node: del_n.clone(),
                            });
                        })),
                ),
        );
    }
    let total = line.nodes.len();
    let _ = handle;
    Some(
        div()
        .absolute()
        .left(px(52.))
        .bottom(px(20.))
        .w(px(360.))
        .p(theme::PAD_SM)
        .rounded(theme::RADIUS_PANEL)
        .border_1()
        .border_color(theme::LINE())
        .bg(theme::PANEL())
        .child(
            div()
                .flex()
                .items_center()
                .gap(theme::GAP_SM)
                .child(
                    div()
                        .font_family(theme::FAMILY_MONO)
                        .text_size(theme::FONT_META)
                        .text_color(theme::ACCENT())
                        .child(SharedString::from(line.id.clone())),
                )
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .text_size(theme::FONT_META)
                        .text_color(theme::TEXT())
                        .child(SharedString::from(line.name.clone())),
                )
                .child(
                    div()
                        .text_size(theme::FONT_MICRO)
                        .text_color(theme::FAINT())
                        .child(SharedString::from(format!(
                            "{} · {total} 节点{fork_note}{diff_note}",
                            line.kind.label(),
                            fork_note = if line.fork_node.is_empty() {
                                String::new()
                            } else {
                                format!(" · 自分叉于 {}", line.fork_node)
                            },
                            // 撤销型影响（kind=remove）在正典里是"删掉某个设定"，
                            // 比 add/change 更需要作者留意，单独点出来。
                            diff_note = if line.diffs.iter().any(|d| d.is_remove()) {
                                " · 含撤销型影响".to_string()
                            } else {
                                String::new()
                            }
                        ))),
                ),
        )
        .child(
            div()
                .mt(theme::GAP_XS)
                .flex()
                .items_center()
                .gap(theme::GAP_SM)
                .child(
                    div()
                        .id("wn-add")
                        .cursor_pointer()
                        .text_size(theme::FONT_MICRO)
                        .text_color(theme::ACCENT())
                        .child("＋ 加节点")
                        .on_click(cx.listener({
                            let lid = line.id.clone();
                            let next_ch = nodes.iter().map(|n| n.chapter_or_guess()).max().unwrap_or(0) + 1;
                            move |_this, _, _, cx| {
                                cx.emit(OracleEvent::NodeAdd {
                                    line: lid.clone(),
                                    kind: "event".into(),
                                    label: "新节点".into(),
                                    chapter: next_ch,
                                });
                            }
                        })),
                )
                .child(
                    div()
                        .text_size(theme::FONT_MICRO)
                        .text_color(theme::FAINT())
                        .child("「⤴」从此节点再分叉；「×」删节点；点章号 +1"),
                ),
        )
        .child(body)
        .into_any_element(),
    )
}

/// 世界线的两种看法：并轨图（横轴=章节的平行线）与世界树（纵向生长的枝）。
/// 两种并存，默认并轨（旧形态不改），世界树是新增视图。
#[derive(Clone, Copy, PartialEq)]
enum WlView {
    Lanes,
    Tree,
    /// 两条线并排对比：共同祖先之后的差异与冲突
    Compare,
}

pub struct OraclePanel {
    focus_handle: FocusHandle,
    handle: Rc<ProjectHandle>,
    premise: Entity<InputState>,
    fork: Entity<InputState>,
    selected: Option<String>,
    message: Option<SharedString>,
    view: WlView,
    /// 线模式选中的车道（与 selected 分开：一条线可以"选中看影响"但车道另外聚焦）
    lane: Option<String>,
    /// 车道里点开的节点（详情抽屉用）
    node: Option<String>,
    /// 对比模式的另一条线（lane 是左边那条）
    compare_with: Option<String>,
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
        Self {
            focus_handle: cx.focus_handle(),
            handle,
            premise,
            fork,
            selected,
            message: None,
            view: WlView::Lanes,
            lane: None,
            node: None,
            compare_with: None,
        }
    }

    /// 当前活跃车道：作者在预言书里点中的那条线；没点过就是正典 α。
    ///
    /// 宿主用它决定"假设推演"从谁身上分叉——这是「从任意节点再分叉」的
    /// 父线来源。返回 None 表示连 α 都读不到（书还没初始化），
    /// 调用方应退化成"只把分叉章交给 AI"。
    pub fn active_lane_root(&self) -> Option<String> {
        if let Some(l) = &self.lane {
            return Some(l.clone());
        }
        let lines = crate::worldline::Store::new(&self.handle.root).all().ok()?;
        lines
            .iter()
            .find(|l| l.is_base())
            .map(|l| l.id.clone())
            .or_else(|| lines.first().map(|l| l.id.clone()))
    }

    /// 宿主改完世界线后的统一反馈：更新选中车道并提示结果。
    ///
    /// 外部只能这样改面板状态（字段是私有的），这样"提示 + 重绘"不会漏掉一个。
    pub fn after_line_change(
        &mut self,
        lane: Option<String>,
        message: impl Into<SharedString>,
        cx: &mut Context<Self>,
    ) {
        if let Some(l) = lane {
            self.lane = Some(l);
            self.node = None;
        }
        self.message = Some(message.into());
        cx.notify();
    }

    /// 选中车道最后一个节点所在章：作为"假设推演"的默认分叉章。
    /// 没选线、或线里没节点时返回 0（交给宿主取最新成稿下一章）。
    fn lane_fork_hint(&self) -> u32 {
        let Some(lid) = self.lane.as_ref() else { return 0 };
        let Ok(lines) = crate::worldline::Store::new(&self.handle.root).all() else {
            return 0;
        };
        lines
            .iter()
            .find(|l| &l.id == lid)
            .map(|l| l.nodes.iter().map(|n| n.chapter_or_guess()).max().unwrap_or(0))
            .unwrap_or(0)
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
        let mut lines = self.handle.worldlines();
        // M8 双态分组：α 固化基准 → 预测线（往哪织）→ 假设线（如果改）
        lines.sort_by_key(|w| {
            let id = s(w, "id");
            if id == "α" {
                0
            } else if s(w, "status") == "已归档" {
                3
            } else if s(w, "kind") == "forecast" {
                1
            } else {
                2
            }
        });

        // —— 左列：世界线列表（分组） ——
        let mut list: Vec<AnyElement> = Vec::new();
        let mut sect_now = String::new();
        for w in &lines {
            let id = s(w, "id");
            let is_alpha = id == "α";
            if !is_alpha {
                let kind0 = s(w, "kind");
                let archived0 = s(w, "status") == "已归档";
                let (sect, color) = if archived0 {
                    ("已归档 · 只读封存（可恢复继续推演）", theme::FAINT())
                } else if kind0 == "forecast" {
                    ("预测线 · 往哪织（按大纲/成稿向前推演）", theme::GREEN())
                } else {
                    ("假设线 · 如果改（what-if 推演）", theme::AMBER())
                };
                if sect_now != sect {
                    sect_now = sect.to_string();
                    list.push(
                        div()
                            .pt(theme::GAP_MD)
                            .pb(theme::GAP_XS)
                            .text_size(theme::FONT_TAG)
                            .font_weight(FontWeight::SEMIBOLD)
                            .text_color(color)
                            .child(SharedString::from(sect))
                            .into_any_element(),
                    );
                }
            }
            let name = s(w, "name");
            let premise = s(w, "premise");
            let status = s(w, "status");
            let n_diffs = w.get("diffs").and_then(|d| d.as_array()).map(|a| a.len()).unwrap_or(0);
            let n_promoted = w
                .get("diffs")
                .and_then(|d| d.as_array())
                .map(|a| a.iter().filter(|x| x.get("promoted").and_then(|p| p.as_bool()).unwrap_or(false)).count())
                .unwrap_or(0);
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
                                let happened = self.handle.events().iter().filter(|e| e.get("happened").and_then(|v| v.as_bool()).unwrap_or(true)).count();
                                let written = self.handle.chapters().iter().filter(|c| c.has_prose).count();
                                let done = self.handle.clues_full().iter().filter(|c| s(c, "status") == "已兑现").count();
                                let allc = self.handle.clues_full().len();
                                format!("固化 · 成稿 {written} 章 · 定事 {happened} · 伏笔 {done}/{allc}")
                            } else {
                                premise.chars().take(60).collect::<String>()
                            })),
                    )
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap(theme::GAP_SM)
                            .when(!is_alpha, |d| {
                                let fork_ch = w.get("forkedChapter").and_then(|v| v.as_u64()).unwrap_or(0);
                                d.child(
                                    div()
                                        .flex_1()
                                        .text_size(theme::FONT_MICRO)
                                        .font_family(theme::FAMILY_MONO)
                                        .text_color(theme::FAINT())
                                        .child(SharedString::from(format!(
                                            "fork 第{fork_ch}章 · 节点 {n_nodes} · 影响 {n_diffs} · 已晋升 {n_promoted}/{n_diffs}"
                                        ))),
                                )
                            })
                            .when(is_alpha, |d| {
                                d.child(
                                    div()
                                        .flex_1()
                                        .text_size(theme::FONT_MICRO)
                                        .font_family(theme::FAMILY_MONO)
                                        .text_color(theme::FAINT())
                                        .child(SharedString::from(format!("节点 {n_nodes} · 影响 {n_diffs}"))),
                                )
                            })
                            .when(!is_alpha, |d| {
                                let arch_id = id.clone();
                                let del_id = id.clone();
                                let archived_now = s(w, "status") == "已归档";
                                d.child(
                                    Button::new(SharedString::from(format!("arch-{id}")))
                                        .xsmall()
                                        .label(if archived_now { "恢复" } else { "归档" })
                                        .on_click(cx.listener(move |this, _, _, cx| {
                                            match this.handle.toggle_worldline_archive(&arch_id) {
                                                Ok(st) => this.message = Some(format!("线{arch_id} → {st}").into()),
                                                Err(e) => this.message = Some(format!("归档失败：{e}").into()),
                                            }
                                            cx.notify();
                                        })),
                                )
                                .child(
                                    // 删线：归档是"封存可恢复"，删是"真的不要了"。
                                    // 子线不删，只断开父指针（Store::remove 负责）。
                                    div()
                                        .id(SharedString::from(format!("wl-del-{id}")))
                                        .cursor_pointer()
                                        .px(px(3.))
                                        .text_size(theme::FONT_MICRO)
                                        .text_color(theme::FAINT())
                                        .child("×")
                                        .on_click(cx.listener(move |this, _, _, cx| {
                                            let store = crate::worldline::Store::new(&this.handle.root);
                                            match store.remove(&del_id) {
                                                Ok(()) => {
                                                    this.selected = None;
                                                    this.lane = None;
                                                    this.message =
                                                        Some(format!("已删除线{del_id}（子线保留，父指针已断开）").into());
                                                }
                                                Err(e) => {
                                                    this.message = Some(format!("删除失败：{e}").into())
                                                }
                                            }
                                            cx.notify();
                                        })),
                                )
                            }),
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
            let wid_btn = s(w, "id");
            let wide = wid_btn.clone();
            right.push(
                div()
                    .flex()
                    .items_center()
                    .gap(theme::GAP_MD)
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .text_size(theme::FONT_META)
                            .font_weight(FontWeight::SEMIBOLD)
                            .text_color(theme::TEXT())
                            .child(SharedString::from(format!(
                                "「{wid}」影响清单 —— 晋升后才是提案，采纳后才改正典"
                            ))),
                    )
                    .child(
                        Button::new(SharedString::from(format!("see-{wide}")))
                            .xsmall()
                            .label("在画布查看")
                            .on_click(cx.listener(move |_, _, _, cx| {
                                cx.emit(OracleEvent::ViewOnCanvas { worldline: wide.clone() });
                            })),
                    )
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
            // 按 target 分组（事件→故事线→设定→人物→伏笔→其他），与原顺序无关
            let mut by_group: Vec<(String, Vec<(usize, serde_json::Value)>)> = Vec::new();
            for (idx, d) in diffs.into_iter().enumerate() {
                let t = s(&d, "target");
                let key = if t.is_empty() { "其他".to_string() } else { t };
                if let Some(g) = by_group.iter_mut().find(|(k, _)| *k == key) {
                    g.1.push((idx, d));
                } else {
                    by_group.push((key, vec![(idx, d)]));
                }
            }
            by_group.sort_by_key(|(k, _)| match k.as_str() {
                "事件" => 0usize,
                "故事线" => 1,
                "设定" => 2,
                "人物" => 3,
                "伏笔" => 4,
                _ => 5,
            });
            for (gi, (group, items)) in by_group.into_iter().enumerate() {
                right.push(
                    div()
                        .pt(if gi == 0 { theme::PAD_XS } else { theme::PAD_MD })
                        .pb(theme::PAD_XS)
                        .flex()
                        .items_center()
                        .gap(theme::GAP_SM)
                        .child(
                            div()
                                .text_size(theme::FONT_TAG)
                                .font_weight(FontWeight::SEMIBOLD)
                                .text_color(theme::DIM())
                                .child(SharedString::from(format!("{group} · {} 条", items.len()))),
                        )
                        .into_any_element(),
                );
                for (idx, d) in items {
                let target = group.clone();
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
                // 事件类新增：预告的是「后续要发生的事」——按分歧章做排期晋升（plannedEvent），
                // 不走 outlineUpdates（那条路要找故事线 id，必然落空）。
                let event_add = target == "事件" && s(&d, "kind") == "add";
                let fork_ch = w.get("forkedChapter").and_then(|v| v.as_u64()).unwrap_or(0) as u32;
                let btn_label = if event_add { "晋升排期" } else { "晋升为提案" };
                let promote_btn = Button::new(SharedString::from(format!("promote-{idx}")))
                    .xsmall()
                    .label(btn_label)
                    .on_click(cx.listener(move |this, _, _, cx| {
                        let wid = this.selected.clone().unwrap_or_default();
                        let res = if event_add {
                            if fork_ch == 0 {
                                Err("该线没有分歧章（forkedChapter=0），无法排期晋升".to_string())
                            } else {
                                this.handle.promote_diff_chapter(&wid, idx_u, fork_ch)
                            }
                        } else {
                            this.handle.promote_diff(&wid, idx_u)
                        };
                        match res {
                            Ok(pid) => {
                                this.message = Some(if event_add {
                                    format!("已排期晋升（第{fork_ch}章）→ 提案 {pid}（收件箱待裁决）").into()
                                } else {
                                    format!("已晋升 → 提案 {pid}（收件箱待裁决）").into()
                                });
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
                                                .child(SharedString::from(format!(
                                                    "{target}·{tref}{}",
                                                    d.get("scheduledChapter")
                                                        .and_then(|v| v.as_u64())
                                                        .map(|ch| format!(" → 第{ch}章"))
                                                        .unwrap_or_default()
                                                ))),
                                        )
                                        .child(
                                            div()
                                                .overflow_hidden()
                                                .whitespace_nowrap()
                                                .text_size(theme::FONT_TAG)
                                                .child(
                                                    div()
                                                        .flex()
                                                        .gap(px(4.))
                                                        .child(
                                                            div()
                                                                .text_color(theme::RED())
                                                                .child(SharedString::from(
                                                                    before.chars().take(18).collect::<String>(),
                                                                )),
                                                        )
                                                        .child(
                                                            div()
                                                                .text_color(theme::FAINT())
                                                                .child("→"),
                                                        )
                                                        .child(
                                                            div()
                                                                .text_color(theme::GREEN())
                                                                .child(SharedString::from(
                                                                    after.chars().take(28).collect::<String>(),
                                                                )),
                                                        ),
                                                ),
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
                            .label("假设推演")
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
                                // 有选中车道时，分叉章默认取该车道最后一个节点所在章，
                                // 作者不用再手查"我分到第几章"。
                                let fork = if f == 0 { this.lane_fork_hint() } else { f };
                                cx.emit(OracleEvent::Simulate { premise: p, fork });
                                this.message = Some("已交给 AI 推演（右侧轨迹窗可见全程）".into());
                                cx.notify();
                            })),
                    )
                    .child(
                        Button::new("oracle-forecast")
                            .xsmall()
                            .label("预测续写")
                            .on_click(cx.listener(|this, _, _, cx| {
                                cx.emit(OracleEvent::Forecast);
                                this.message = Some("预测线已交给 AI：fork 自动取最新成稿下一章（轨迹窗可见）".into());
                                cx.notify();
                            })),
                    ),
            );

        // —— 世界树：干 = 此世 α（纵向章节轴，根在第 1 章），枝 = 自分歧章长出的平行世界。
        //    干居中、枝左右交替；每条枝再分小枝结叶 —— 树冠的读感靠「枝→小枝→叶」三层。
        //    枝上的果 = 该世界已推演的节点；叶 = 新长出的世界；枯枝（虚线、下垂）= 归档世界。
        //    几何全部按 320×480 定值算 —— 画布与覆盖层（章号、可点的世界徽）共用同一组坐标。
        const TW: f32 = 320.;
        const TH: f32 = 480.;
        let mut worlds: Vec<(String, u32, usize, bool, bool, bool)> = Vec::new();
        for w in &lines {
            let id = s(w, "id");
            if id == "α" {
                continue;
            }
            let on = self.selected.as_deref() == Some(id.as_str());
            worlds.push((
                id,
                w.get("forkedChapter").and_then(|v| v.as_u64()).unwrap_or(0) as u32,
                w.get("nodes").and_then(|v| v.as_array()).map(|a| a.len()).unwrap_or(0),
                on,
                s(w, "kind") == "forecast",
                s(w, "status") == "已归档",
            ));
        }
        // 活枝按分叉章排序（早的在下），枯枝收在最上面
        worlds.sort_by_key(|w| (w.5, w.1));

        let now_ch = self.handle.next_chapter_number().saturating_sub(1).max(1);
        // 树布局：纵轴 = 章节，横轴按子树宽度分配（layout::tree 是纯函数，
        // 同一份数据两次结果一致，画布重绘不会抖）。
        let typed_tree: Vec<crate::worldline::Line> = lines
            .iter()
            .filter_map(|v| serde_json::from_value(v.clone()).ok())
            .collect();
        let laid = crate::worldline::layout::tree(&typed_tree);
        let max_ch = laid
            .max_chapter
            .max(now_ch)
            .max(worlds.iter().map(|w| w.1).max().unwrap_or(0))
            .max(10);
        // 树占满整个 320 宽：左右各留 24 给标签
        let tree_l = 24.0f32;
        let tree_r = TW - 24.;
        let trunk_x = (tree_l + tree_r) / 2.;
        let top_y = 42.0f32;
        let base_y = TH - 40.;
        let ch_y = move |ch: u32| base_y - (ch.min(max_ch) as f32 / max_ch as f32) * (base_y - top_y);

        #[derive(Clone)]
        struct Branch {
            id: String,
            fork_ch: u32,
            fork_y: f32,
            tip: (f32, f32),
            c1: (f32, f32),
            c2: (f32, f32),
            side: f32,
            nodes: usize,
            on: bool,
            forecast: bool,
            archived: bool,
        }
        let mut branches: Vec<Branch> = Vec::with_capacity(worlds.len());
        for (id, fork, nn, on, forecast, archived) in worlds.iter() {
            // 几何全部来自 layout::tree，不再用 id 字符和当随机源：
            // - 分叉章 fork_chapter 已优先取 forkNode 所在章
            // - 左右侧由它在树里占的槽位决定（左半 → 左侧）
            // - 伸出距离由子树宽度决定（子孙多的枝占更宽），枝不会互相压
            let laid_node = laid.nodes.iter().find(|n| &n.id == id);
            let slot_mid = laid_node.map(|n| (n.slot_start + n.slot_end) / 2.).unwrap_or(0.5);
            let subtree = laid_node.map(|n| n.slot_end - n.slot_start).unwrap_or(0.5);
            let side = if *archived || slot_mid < 0.5 { -1.0f32 } else { 1.0 };
            let max_reach = if side > 0. { 150. } else { 104. };
            // 子树越宽 → 越往外伸；深度越大 → 越往两侧散
            let reach_w = 0.20 + subtree * 0.85;
            let mut rise = 38. + 22. * (subtree * 3.).min(3.) + (*nn).min(6) as f32 * 2.2;
            let mut reach = (40. + 150. * reach_w).min(max_reach);
            if *archived {
                // 枯枝：向下垂，不再长
                rise = -10. - subtree * 8.;
            }
            let fork_y = ch_y((*fork).max(1));
            let avail = fork_y - top_y - 18.;
            if rise > 0. && rise > avail {
                let sfa = avail / rise;
                rise *= sfa;
                reach *= sfa;
            }
            let tip = (trunk_x + side * reach, fork_y - rise);
            branches.push(Branch {
                id: id.clone(),
                fork_ch: *fork,
                fork_y,
                tip,
                c1: (trunk_x + side * reach * 0.05, fork_y - rise * 0.58),
                c2: (tip.0 - side * reach * 0.45, tip.1 - rise * 0.05),
                side,
                nodes: *nn,
                on: *on,
                forecast: *forecast,
                archived: *archived,
            });
        }

        let mut tree = div().relative().w(px(TW)).h(px(TH)).flex_shrink_0().child({
            let branches = branches.clone();
            canvas(
                |_, _, _| {},
                move |bounds, _, window, _| {
                    let ox = bounds.origin.x.as_f32();
                    let oy = bounds.origin.y.as_f32();
                    let p = |x: f32, y: f32| point(px(ox + x), px(oy + y));
                    let blue = theme::BLUE();
                    let faint = theme::FAINT();
                    let green = theme::GREEN();
                    let amber = theme::AMBER();
                    let accent = theme::ACCENT();
                    let line_c = theme::LINE();
                    let dot = |x: f32, y: f32, r: f32| {
                        let k = 0.5523 * r;
                        let mut b = PathBuilder::fill();
                        b.move_to(p(x + r, y));
                        b.cubic_bezier_to(p(x + r, y + k), p(x + k, y + r), p(x, y + r));
                        b.cubic_bezier_to(p(x - k, y + r), p(x - r, y + k), p(x - r, y));
                        b.cubic_bezier_to(p(x - r, y - k), p(x - k, y - r), p(x, y - r));
                        b.cubic_bezier_to(p(x + k, y - r), p(x + r, y - k), p(x + r, y));
                        b
                    };
                    let leaf = |bx: f32, by: f32, ang: f32, len: f32| {
                        let (dx, dy) = (ang.cos(), ang.sin());
                        let (nx, ny) = (-dy, dx);
                        let hw = len * 0.30;
                        let mut b = PathBuilder::fill();
                        b.move_to(p(bx, by));
                        b.cubic_bezier_to(
                            p(bx + dx * len * 0.30 + nx * hw, by + dy * len * 0.30 + ny * hw),
                            p(
                                bx + dx * len * 0.82 + nx * hw * 0.65,
                                by + dy * len * 0.82 + ny * hw * 0.65,
                            ),
                            p(bx + dx * len, by + dy * len),
                        );
                        b.cubic_bezier_to(
                            p(
                                bx + dx * len * 0.82 - nx * hw * 0.65,
                                by + dy * len * 0.82 - ny * hw * 0.65,
                            ),
                            p(bx + dx * len * 0.30 - nx * hw, by + dy * len * 0.30 - ny * hw),
                            p(bx, by),
                        );
                        b
                    };
                    let cubic = |p0: (f32, f32), c1: (f32, f32), c2: (f32, f32), p1: (f32, f32), t: f32| {
                        let u = 1. - t;
                        (
                            u * u * u * p0.0
                                + 3. * u * u * t * c1.0
                                + 3. * u * t * t * c2.0
                                + t * t * t * p1.0,
                            u * u * u * p0.1
                                + 3. * u * u * t * c1.1
                                + 3. * u * t * t * c2.1
                                + t * t * t * p1.1,
                        )
                    };
                    // 根（第 1 章之前的世界，向土里散开）
                    for sd in [-2.0f32, -1., 0., 1., 2.] {
                        let off = sd * 13.;
                        let a = 0.40 - sd.abs() * 0.09;
                        let mut r = PathBuilder::stroke(px(1.0));
                        r.move_to(p(trunk_x, base_y));
                        r.cubic_bezier_to(
                            p(trunk_x + sd * 3., base_y + 8.),
                            p(trunk_x + off * 1.2, TH - 14.),
                            p(trunk_x + off * 2.2, TH - 4.),
                        );
                        if let Ok(path) = r.build() {
                            window.paint_path(path, hsla(blue).opacity(a));
                        }
                    }
                    // 干：四段收梢（3.3 → 1.9）
                    for (y0, y1, sw) in [
                        (base_y, base_y - 150., 3.3),
                        (base_y - 150., base_y - 290., 2.8),
                        (base_y - 290., base_y - 370., 2.3),
                        (base_y - 370., top_y, 1.9),
                    ] {
                        let mut t = PathBuilder::stroke(px(sw));
                        t.move_to(p(trunk_x, y0));
                        t.line_to(p(trunk_x, y1));
                        if let Ok(path) = t.build() {
                            window.paint_path(path, hsla(blue));
                        }
                    }
                    let mut t2 = PathBuilder::stroke(px(1.0));
                    t2.move_to(p(trunk_x + 4.2, base_y - 8.));
                    t2.line_to(p(trunk_x + 4.2, top_y + 10.));
                    if let Ok(path) = t2.build() {
                        window.paint_path(path, hsla(blue).opacity(0.18));
                    }
                    // 章节刻度（每 5 章一格；章号写在分叉徽处）
                    let mut ch = 5;
                    while ch <= max_ch {
                        let y = ch_y(ch);
                        let mut tk = PathBuilder::stroke(px(1.0));
                        tk.move_to(p(trunk_x - 3., y));
                        tk.line_to(p(trunk_x + 3., y));
                        if let Ok(path) = tk.build() {
                            window.paint_path(path, hsla(line_c).opacity(0.70));
                        }
                        ch += 5;
                    }
                    // 枝 → 小枝 → 叶
                    for b in branches.iter() {
                        let c = if b.archived {
                            faint
                        } else if b.forecast {
                            green
                        } else {
                            amber
                        };
                        let c = if b.on { accent } else { c };
                        let mut bp = PathBuilder::stroke(px(if b.on {
                            2.4
                        } else if b.archived {
                            1.0
                        } else {
                            1.7
                        }));
                        if b.archived {
                            bp = bp.dash_array(&[px(3.), px(3.)]);
                        }
                        bp.move_to(p(trunk_x, b.fork_y));
                        bp.cubic_bezier_to(p(b.c1.0, b.c1.1), p(b.c2.0, b.c2.1), p(b.tip.0, b.tip.1));
                        if let Ok(path) = bp.build() {
                            window.paint_path(
                                path,
                                hsla(c).opacity(if b.archived {
                                    0.42
                                } else if b.on {
                                    1.
                                } else {
                                    0.92
                                }),
                            );
                        }
                        if let Ok(path) = dot(trunk_x, b.fork_y, 2.2).build() {
                            window.paint_path(path, hsla(c));
                        }
                        if b.archived {
                            // 枯枝：两截下垂的断杈
                            for (t0, ang, len) in [(0.55f32, 0.55f32, 12.0f32), (0.85, 0.95, 9.)] {
                                let (bx, by) = cubic((trunk_x, b.fork_y), b.c1, b.c2, b.tip, t0);
                                let mut d = PathBuilder::stroke(px(0.9));
                                d.move_to(p(bx, by));
                                d.line_to(p(bx + ang.cos() * len, by + ang.sin() * len));
                                if let Ok(path) = d.build() {
                                    window.paint_path(path, hsla(c).opacity(0.42));
                                }
                            }
                            continue;
                        }
                        // 果：已推演节点，沿枝由内向外
                        let m = b.nodes.min(6);
                        for k in 0..m {
                            let t0 = if m > 1 {
                                0.30 + 0.62 * (k as f32 / (m as f32 - 1.))
                            } else {
                                0.55
                            };
                            let (fx, fy) = cubic((trunk_x, b.fork_y), b.c1, b.c2, b.tip, t0);
                            if let Ok(path) = dot(fx, fy, 1.6).build() {
                                window.paint_path(path, hsla(c).opacity(0.50));
                            }
                        }
                        // 小枝：沿切线上下扇，左右自动对称；枝梢长叶
                        for (t0, da, len) in [
                            (0.40f32, -0.85f32, 18.0f32),
                            (0.55, -0.45, 23.),
                            (0.70, -0.05, 25.),
                            (0.84, 0.45, 21.),
                            (0.95, -1.15, 15.),
                        ] {
                            let (bx, by) = cubic((trunk_x, b.fork_y), b.c1, b.c2, b.tip, t0);
                            let (qx, qy) =
                                cubic((trunk_x, b.fork_y), b.c1, b.c2, b.tip, (t0 + 0.01).min(1.));
                            let tang = (qy - by).atan2(qx - bx);
                            let ang = tang + da;
                            let (ex, ey) = (bx + ang.cos() * len, by + ang.sin() * len);
                            let (cx, cy) =
                                (bx + ang.cos() * len * 0.45, by + ang.sin() * len * 0.45 - 4.);
                            let mut tp = PathBuilder::stroke(px(if b.on { 1.15 } else { 0.95 }));
                            tp.move_to(p(bx, by));
                            tp.cubic_bezier_to(p(cx, cy), p(cx, cy - 3.), p(ex, ey));
                            if let Ok(path) = tp.build() {
                                window.paint_path(path, hsla(c).opacity(if b.on { 0.80 } else { 0.66 }));
                            }
                            for (la, ll) in [
                                (ang - 0.55, 9.),
                                (ang - 0.22, 13.),
                                (ang + 0.14, 12.),
                                (ang + 0.45, 9.),
                            ] {
                                if let Ok(path) = leaf(ex, ey, la, ll).build() {
                                    window.paint_path(path, hsla(c).opacity(if b.on { 0.46 } else { 0.34 }));
                                }
                            }
                            let (mx, my) = ((bx + ex) / 2., (by + ey) / 2.);
                            for (la, ll) in [(ang - 1.0, 8.), (ang + 0.9, 8.)] {
                                if let Ok(path) = leaf(mx, my, la, ll).build() {
                                    window.paint_path(path, hsla(c).opacity(if b.on { 0.38 } else { 0.28 }));
                                }
                            }
                        }
                        // 枝端叶
                        for (la, ll) in [(-2.1f32, 12.0f32), (-1.55, 16.), (-1.0, 14.), (-0.55, 11.)] {
                            if let Ok(path) = leaf(b.tip.0, b.tip.1, la, ll).build() {
                                window.paint_path(path, hsla(c).opacity(if b.on { 0.46 } else { 0.34 }));
                            }
                        }
                    }
                    // 树梢 = 此世此刻：一簇叶 + 两点果
                    if let Ok(path) = dot(trunk_x, top_y, 2.6).build() {
                        window.paint_path(path, hsla(blue));
                    }
                    for (ang, len) in [
                        (-2.75f32, 12.0f32),
                        (-2.35, 16.),
                        (-2.0, 19.),
                        (-1.65, 15.),
                        (-1.3, 13.),
                        (-0.95, 15.),
                        (-0.6, 12.),
                        (-0.3, 10.),
                    ] {
                        if let Ok(path) = leaf(trunk_x, top_y, ang, len).build() {
                            window.paint_path(path, hsla(blue).opacity(0.38));
                        }
                    }
                    for (dx, dy) in [(-9.0f32, -6.0f32), (8., -8.), (13., 2.)] {
                        if let Ok(path) = dot(trunk_x + dx, top_y + dy, 1.6).build() {
                            window.paint_path(path, hsla(blue).opacity(0.65));
                        }
                    }
                },
            )
            .absolute()
            .size_full()
        });
        // 树梢标注
        tree = tree.child(
            div()
                .absolute()
                .left(px(trunk_x - 44.))
                .top(px(top_y - 40.))
                .w(px(88.))
                .text_center()
                .text_size(theme::FONT_MICRO)
                .text_color(theme::BLUE())
                .child("现在"),
        );
        // 分叉徽（世界 + 章号）：点一下切到那个世界
        for b in &branches {
            let c = if b.archived {
                theme::FAINT()
            } else if b.forecast {
                theme::GREEN()
            } else {
                theme::AMBER()
            };
            let c = if b.on { theme::ACCENT() } else { c };
            let pick = b.id.clone();
            let label = b.id.clone();
            let chip_x = trunk_x + b.side * 10. - 7.5;
            let ch_label = format!("第{}章", b.fork_ch.max(1));
            let ch_y_lab = if b.fork_y < top_y + 110. {
                b.fork_y + 13.
            } else {
                b.fork_y - 15.
            };
            let ch_left = if b.side > 0. { chip_x + 18. } else { chip_x - 43. };
            tree = tree.child(
                div()
                    .id(SharedString::from(format!("wfork-{}", b.id)))
                    .absolute()
                    .left(px(chip_x))
                    .top(px(b.fork_y - 7.5))
                    .w(px(15.))
                    .h(px(15.))
                    .rounded(px(7.5))
                    .border_1()
                    .border_color(c)
                    .bg(if b.on { theme::ACCENT_SOFT() } else { theme::BG() })
                    .flex()
                    .items_center()
                    .justify_center()
                    .cursor_pointer()
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.selected = Some(pick.clone());
                        this.message = None;
                        cx.notify();
                    }))
                    .child(
                        div()
                            .text_size(px(8.5))
                            .font_family(theme::FAMILY_MONO)
                            .text_color(c)
                            .child(SharedString::from(label)),
                    ),
            );
            tree = tree.child(
                div()
                    .absolute()
                    .left(px(ch_left))
                    .top(px(ch_y_lab))
                    .w(px(40.))
                    .when(b.side > 0., |d| d.text_left())
                    .when(b.side < 0., |d| d.text_right())
                    .text_size(px(8.5))
                    .font_family(theme::FAMILY_MONO)
                    .text_color(theme::FAINT())
                    .child(SharedString::from(ch_label)),
            );
        }
        if branches.is_empty() {
            tree = tree.child(
                div()
                    .absolute()
                    .left(px(trunk_x + 16.))
                    .top(px(TH / 2. - 34.))
                    .w(px(140.))
                    .text_size(theme::FONT_LABEL)
                    .line_height(relative(1.55))
                    .text_color(theme::FAINT())
                    .child("此世还没有分叉。在下方写一句「如果……」，世界会从那一章长出新枝。"),
            );
        }
        let mut tree_col: Vec<AnyElement> = vec![
            div()
                .flex()
                .items_baseline()
                .gap(theme::GAP_SM)
                .child(
                    div()
                        .text_size(theme::FONT_META)
                        .font_weight(FontWeight::SEMIBOLD)
                        .text_color(theme::BLUE())
                        .child("世界树"),
                )
                .child(
                    div()
                        .text_size(theme::FONT_MICRO)
                        .text_color(theme::FAINT())
                        .child(SharedString::from(format!(
                            "干 = 此世 α · 枝 {} · 叶果 = 已推演",
                            branches.len()
                        ))),
                )
                .into_any_element(),
            tree.into_any_element(),
            div()
                .w(px(TW))
                .text_size(theme::FONT_MICRO)
                .line_height(relative(1.5))
                .text_color(theme::FAINT())
                .child("根 = 第 1 章，梢 = 现在；每一次「如果」都在某一章分出一枝。枝上的果是那世界已推演的事，虚线枯枝是归档的世界。")
                .into_any_element(),
        ];
        // 选中世界的名字（枝梢放不下，就在树下点名）
        if let Some(wid) = worlds.iter().find(|w| w.3).map(|w| w.0.clone()) {
            if let Some(line) = lines.iter().find(|x| s(x, "id") == wid) {
                tree_col.insert(
                    2,
                    div()
                        .w(px(TW))
                        .text_size(theme::FONT_MICRO)
                        .text_color(theme::ACCENT())
                        .child(SharedString::from(format!(
                            "▸ 世界{wid} · {}",
                            s(line, "name")
                        )))
                        .into_any_element(),
                );
            }
        }

        // —— 线模式（横轴 = 真实章节）——
        //
        // 三条老问题在这里一并解决：
        // 1. 旧版横轴只有"从 1 到 max"一段，看不出幕、也看不出"现在"在哪。
        //    现在叠上 stages.json 的幕带，并在最新成稿章画一条"现在"标线，
        //    左侧实线（已发生）、右侧虚线（还没写）。
        // 2. 旧版每条线只是"从 fork 章弯出来再横着画"，节点一个都不落。
        //    现在节点按自己的 chapter 落在车道上，一眼看得出这条线推到第几章。
        // 3. 旧版只能从 forkedChapter 分叉。现在 forkNode 指到具体节点，
        //    曲线从那个节点的位置弯出——同一章两个节点也能区分。
        let lanes_el: Option<gpui_kit::AnyElement> = if self.view == WlView::Lanes {
            // 画布这一层仍用 Value（AI 可能写进未知字段，转数字段会丢），
            // 但布局与血缘判断走带类型的那一层：字段名写错在那里是编译错误。
            let typed: Vec<crate::worldline::Line> = lines
                .iter()
                .filter_map(|v| serde_json::from_value(v.clone()).ok())
                .collect();
            let detail = lane_detail(cx, &typed, &self.lane, &self.handle);
            let lane_rows = crate::worldline::layout::lanes(&typed);
            let stage_raw: Vec<(String, String, u32, u32)> = self
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
            let bands = crate::worldline::layout::bands(&stage_raw);
            // 现在 = 最新成稿章；一条都没写时是 0（标线不画）
            let now_ch = self
                .handle
                .chapters()
                .iter()
                .filter(|c| c.has_prose)
                .map(|c| c.n)
                .max()
                .unwrap_or(0);
            let max_ch = self
                .handle
                .next_chapter_number()
                .max(lane_rows.iter().map(|l| l.last_chapter).max().unwrap_or(0))
                .max(now_ch + 3)
                .max(12);
            // 车道高度固定 30：节点圆点直径 7，行距再小就挤成一条
            let row_h = 30.;
            let head_h = 46.;
            let lane_h = head_h + lane_rows.len().max(1) as f32 * row_h + 10.;

            let lane_snapshot = lane_rows.clone();
            let band_snapshot = bands.clone();
            let node_map: Vec<(String, Vec<(u32, String, bool)>)> = typed
                .iter()
                .map(|l| {
                    let mut ns: Vec<(u32, String, bool)> = l
                        .nodes
                        .iter()
                        .map(|n| {
                            (
                                n.chapter_or_guess(),
                                n.label.clone(),
                                n.state == crate::worldline::NodeState::Written,
                            )
                        })
                        .filter(|(ch, _, _)| *ch > 0)
                        .collect();
                    ns.sort_by_key(|(ch, _, _)| *ch);
                    (l.id.clone(), ns)
                })
                .collect();
            let selected_lane = self.lane.clone();

            let lanes_el: gpui_kit::AnyElement = {
                let alpha_color = theme::BLUE();
                let dim_color = theme::FAINT();
                let line_color = theme::LINE();
                let acc = theme::ACCENT();
                let green = theme::GREEN();
                let purple = theme::PURPLE();
                let panel = theme::PANEL();
                let bg = theme::BG();
                let rows = lane_snapshot.clone();
                let bands_s = band_snapshot.clone();
                let nodes = node_map.clone();
                let sel = selected_lane.clone();
                div()
                    .relative()
                    .w_full()
                    .h(px(lane_h))
                    .child(
                        canvas(
                            |_, _, _| {},
                            move |bounds, _, window, _| {
                                let w = bounds.size.width.as_f32();
                                let h = bounds.size.height.as_f32();
                                let ox = bounds.origin.x.as_f32();
                                let oy = bounds.origin.y.as_f32();
                                let pad_l = 52.0;
                                let pad_r = 20.0;
                                let track = (w - pad_l - pad_r).max(1.);
                                let x_of = |ch: u32| {
                                    pad_l + (ch as f32 / max_ch as f32).clamp(0., 1.) * track
                                };
                                let y_of = |i: usize| head_h + row_h * i as f32;
                                let p = |x: f32, y: f32| point(px(ox + x), px(oy + y));

                                // 幕带：交替底色 + 顶部卷名，作者一眼看出"这段是第一幕"
                                for (bi, b) in bands_s.iter().enumerate() {
                                    let x0 = x_of(b.start.saturating_sub(1));
                                    let x1 = x_of(b.end.min(max_ch));
                                    if x1 <= x0 {
                                        continue;
                                    }
                                    let mut rect = PathBuilder::fill();
                                    rect.move_to(p(x0, 0.));
                                    rect.line_to(p(x1, 0.));
                                    rect.line_to(p(x1, h));
                                    rect.line_to(p(x0, h));
                                    rect.close();
                                    if let Ok(path) = rect.build() {
                                        // 奇偶交替，靠明度区分而不是靠颜色堆砌
                                        let a = if bi % 2 == 0 { 0.055 } else { 0.025 };
                                        window.paint_path(path, hsla(purple).opacity(a));
                                    }
                                    let mut edge = PathBuilder::stroke(px(1.));
                                    edge.move_to(p(x0, 0.));
                                    edge.line_to(p(x0, h));
                                    if let Ok(path) = edge.build() {
                                        window.paint_path(path, hsla(purple).opacity(0.22));
                                    }
                                }

                                // 章节刻度：每 5 章一道，带章号
                                let mut ch = 5;
                                while ch <= max_ch {
                                    let x = x_of(ch);
                                    let mut t = PathBuilder::stroke(px(1.));
                                    t.move_to(p(x, head_h - 8.));
                                    t.line_to(p(x, h - 2.));
                                    if let Ok(path) = t.build() {
                                        window.paint_path(path, hsla(line_color).opacity(0.65));
                                    }
                                    ch += 5;
                                }

                                // "现在"标线：左侧已发生、右侧未写
                                if now_ch > 0 {
                                    let x = x_of(now_ch);
                                    let mut past = PathBuilder::stroke(px(1.4));
                                    past.move_to(p(x, head_h - 10.));
                                    past.line_to(p(x, h - 2.));
                                    if let Ok(path) = past.build() {
                                        window.paint_path(path, hsla(acc).opacity(0.55));
                                    }
                                    let mut future = PathBuilder::stroke(px(1.2));
                                    future = future.dash_array(&[px(4.), px(4.)]);
                                    future.move_to(p(x, head_h - 10.));
                                    future.line_to(p(x, h - 2.));
                                    if let Ok(path) = future.build() {
                                        window.paint_path(path, hsla(acc).opacity(0.28));
                                    }
                                }

                                // 车道
                                for (i, lane) in rows.iter().enumerate() {
                                    let y = y_of(i);
                                    let on = sel.as_deref() == Some(lane.id.as_str());
                                    let c = if lane.id == "α" {
                                        alpha_color
                                    } else if lane.archived {
                                        dim_color
                                    } else if lane.forecast {
                                        green
                                    } else {
                                        purple
                                    };
                                    let c = if on { acc } else { c };
                                    let op = if lane.archived { 0.42 } else if on { 1.0 } else { 0.85 };

                                    // 基线：整条车道画到底，作者能看出这条线覆盖到哪
                                    let base_end = if lane.last_chapter > 0 {
                                        x_of(lane.last_chapter)
                                    } else {
                                        x_of(lane.fork_chapter.max(1))
                                    };
                                    let mut b = PathBuilder::stroke(px(if on { 1.8 } else { 1.3 }));
                                    if lane.archived {
                                        b = b.dash_array(&[px(3.), px(3.)]);
                                    }
                                    b.move_to(p(x_of(0), y));
                                    b.line_to(p(base_end.max(x_of(0) + 4.), y));
                                    if on {
                                        // 选中线：淡底块，一眼找到
                                        let mut fill = PathBuilder::fill();
                                        fill.move_to(p(0., y - row_h / 2. + 2.));
                                        fill.line_to(p(w, y - row_h / 2. + 2.));
                                        fill.line_to(p(w, y + row_h / 2. - 2.));
                                        fill.line_to(p(0., y + row_h / 2. - 2.));
                                        fill.close();
                                        if let Ok(path) = fill.build() {
                                            window.paint_path(path, hsla(acc).opacity(0.06));
                                        }
                                    }
                                    if let Ok(path) = b.build() {
                                        window.paint_path(path, hsla(c).opacity(op));
                                    }
                                    if lane.archived {
                                        continue;
                                    }

                                    // 分叉：从父线（正典或另一条）的对应车道弯出
                                    if lane.id != "α" {
                                        let py = rows
                                            .iter()
                                            .position(|r| r.id == line_parent_id(&typed, &lane.id))
                                            .map(|pi| y_of(pi))
                                            .unwrap_or(y_of(0));
                                        let fx = x_of(lane.fork_chapter.max(1));
                                        let mut curve = PathBuilder::stroke(px(if on { 1.8 } else { 1.3 }));
                                        curve.move_to(p(fx, py));
                                        curve.cubic_bezier_to(
                                            p(fx + 10., py),
                                            p(fx + 10., y),
                                            p((fx + 22.).min(w - pad_r), y),
                                        );
                                        if let Ok(path) = curve.build() {
                                            window.paint_path(path, hsla(c).opacity(op));
                                        }
                                    }

                                    // 节点落点：已写的实心、计划中的空心
                                    if let Some((_, ns)) = nodes.iter().find(|(id, _)| id == &lane.id) {
                                        for (nch, _, written) in ns {
                                            let nx = x_of(*nch);
                                            let r = if on { 3.4 } else { 2.6 };
                                            if *written {
                                                let k = 0.5523 * r;
                                                let mut d = PathBuilder::fill();
                                                d.move_to(p(nx + r, y));
                                                d.cubic_bezier_to(p(nx + r, y + k), p(nx + k, y + r), p(nx, y + r));
                                                d.cubic_bezier_to(p(nx - k, y + r), p(nx - r, y + k), p(nx - r, y));
                                                d.cubic_bezier_to(p(nx - r, y - k), p(nx - k, y - r), p(nx, y - r));
                                                d.cubic_bezier_to(p(nx + k, y - r), p(nx + r, y - k), p(nx + r, y));
                                                if let Ok(path) = d.build() {
                                                    window.paint_path(path, hsla(c).opacity(if on { 0.95 } else { 0.72 }));
                                                }
                                            } else {
                                                let mut d = PathBuilder::stroke(px(1.1));
                                                d.move_to(p(nx + r, y));
                                                let k = 0.5523 * r;
                                        d.cubic_bezier_to(p(nx + r, y + k), p(nx + k, y + r), p(nx, y + r));
                                                d.cubic_bezier_to(p(nx - k, y + r), p(nx - r, y + k), p(nx - r, y));
                                                d.cubic_bezier_to(p(nx - r, y - k), p(nx - k, y - r), p(nx, y - r));
                                                d.cubic_bezier_to(p(nx + k, y - r), p(nx + r, y - k), p(nx + r, y));
                                                if let Ok(path) = d.build() {
                                                    window.paint_path(path, hsla(c).opacity(if on { 0.85 } else { 0.55 }));
                                                }
                                            }
                                        }
                                    }
                                }
                                let _ = (panel, bg);
                            },
                        )
                        .absolute()
                        .size_full(),
                    )
                    // 左侧车道名（等宽字体，与画布 y 对齐）
                    .child(
                        div()
                            .absolute()
                            .left(px(4.))
                            .top(px(head_h))
                            .flex()
                            .flex_col()
                            .children(lane_snapshot.iter().map(|lane| {
                                let on = self.lane.as_deref() == Some(lane.id.as_str());
                                let c = if lane.id == "α" {
                                    theme::BLUE()
                                } else if lane.archived {
                                    theme::FAINT()
                                } else if on {
                                    theme::ACCENT()
                                } else {
                                    theme::DIM()
                                };
                                let id = lane.id.clone();
                                div()
                                    .id(SharedString::from(format!("lane-sel-{}", lane.id)))
                                    .h(px(row_h))
                                    .flex()
                                    .items_center()
                                    .gap(px(3.))
                                    .cursor_pointer()
                                    .on_click(cx.listener(move |this, _, _, cx| {
                                        this.lane = Some(id.clone());
                                        this.node = None;
                                        this.message = None;
                                        cx.notify();
                                    }))
                                    .child(
                                        div()
                                            .font_family(theme::FAMILY_MONO)
                                            .text_size(theme::FONT_MICRO)
                                            .text_color(c)
                                            .child(SharedString::from(lane.id.clone())),
                                    )
                                    .when(lane.pending_diffs > 0, |d| {
                                        d.child(
                                            div()
                                                .text_size(px(8.))
                                                .font_family(theme::FAMILY_MONO)
                                                .text_color(theme::GREEN())
                                                .child(SharedString::from(format!(
                                                    "{}",
                                                    lane.pending_diffs
                                                ))),
                                        )
                                    })
                            })),
                    )
                    // 幕名（画在幕带顶上，跟着 x 位置走会更好，但 canvas 里排版文字成本高，
                    // 这里用相对定位容器按百分比摆放）
                    .child(
                        div()
                            .absolute()
                            .left(px(52.))
                            .right(px(20.))
                            .top(px(6.))
                            .h(px(18.))
                            .relative()
                            .children(band_snapshot.iter().map(|b| {
                                let total = max_ch.max(1) as f32;
                                let start = (b.start.saturating_sub(1)) as f32 / total;
                                let width = ((b.end.saturating_sub(b.start) + 1) as f32 / total)
                                    .clamp(0.02, 1.);
                                let name = b.name.clone();
                                let theme_s = b.theme.clone();
                                div()
                                    .absolute()
                                    .left(relative(start))
                                    .w(relative(width))
                                    .overflow_hidden()
                                    .whitespace_nowrap()
                                    .text_size(theme::FONT_MICRO)
                                    .text_color(theme::PURPLE())
                                    .child(SharedString::from(if theme_s.is_empty() {
                                        name
                                    } else {
                                        format!("{name} · {theme_s}")
                                    }))
                            })),
                    )
                    .child(
                        div()
                            .absolute()
                            .right(px(4.))
                            .bottom(px(2.))
                            .text_size(theme::FONT_MICRO)
                            .text_color(theme::FAINT())
                            .child(SharedString::from(format!(
                                "共 {max_ch} 章 · 现在第 {now_ch} 章 · 实心=已写 空心=计划"
                            ))),
                    )
                    .children(detail)
                    .into_any_element()
            };
            Some(lanes_el)
        } else {
            None
        };


        // 页头：并轨 / 世界树 双视图（旧形态默认，世界树是新增看法）
        let seg = |label: &'static str, on: bool, key: &'static str| {
            div()
                .id(key)
                .px(px(10.))
                .py(px(3.))
                .rounded(px(6.))
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
        };
        let head = div()
            .flex_shrink_0()
            .px(theme::PAD_LG)
            .pt(theme::PAD_MD)
            .flex()
            .items_center()
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
                    .child(match self.view {
                        WlView::Lanes => "并轨：横轴 = 章节，α 是贯穿基线；其他线自分歧章分叉、平行延伸",
                        WlView::Tree => "世界树：根 = 第 1 章，梢 = 现在；每一次「如果」都在某一章长出一枝",
                        WlView::Compare => "对比：选两条线，看共同祖先之后各自推演到哪、哪里互相冲突",
                    }),
            )
            .child(
                div()
                    .flex()
                    .gap(px(4.))
                    .child(seg("并轨", self.view == WlView::Lanes, "wl-view-lanes").on_click(
                        cx.listener(|this, _, _, cx| {
                            this.view = WlView::Lanes;
                            cx.notify();
                        }),
                    ))
                    .child(seg("世界树", self.view == WlView::Tree, "wl-view-tree").on_click(
                        cx.listener(|this, _, _, cx| {
                            this.view = WlView::Tree;
                            cx.notify();
                        }),
                    ))
                    .child(seg("对比", self.view == WlView::Compare, "wl-view-compare").on_click(
                        cx.listener(|this, _, _, cx| {
                            this.view = WlView::Compare;
                            // 进对比时默认拿"当前选中线 vs 正典"，作者立刻看到东西
                            if this.compare_with.is_none() {
                                this.compare_with = Some("α".into());
                            }
                            cx.notify();
                        }),
                    )),
            );

        // —— 对比视图：两条线并排 ——
        let compare_el: Option<gpui_kit::AnyElement> = if self.view == WlView::Compare {
            let typed_cmp: Vec<crate::worldline::Line> = lines
                .iter()
                .filter_map(|v| serde_json::from_value(v.clone()).ok())
                .collect();
            let left = self.lane.clone().or_else(|| {
                typed_cmp
                    .iter()
                    .find(|l| !l.is_base())
                    .map(|l| l.id.clone())
            });
            let right = self.compare_with.clone().unwrap_or_else(|| "α".into());
            let Some(left) = left else {
                return div()
                    .size_full()
                    .flex()
                    .items_center()
                    .justify_center()
                    .child(
                        div()
                            .text_size(theme::FONT_LABEL)
                            .text_color(theme::FAINT())
                            .child("还没有可对比的世界线——先在下方写一句「如果……」开一条。"),
                    );
            };
            let cmp = crate::worldline::layout::compare(&typed_cmp, &left, &right);
            let pickable = typed_cmp.clone();
            let seg2 = |label: String, on: bool, key: String| {
                div()
                    .id(SharedString::from(key))
                    .px(px(8.))
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
            let mut col_a = div().flex().flex_col().gap(px(2.));
            for (label, ch) in &cmp.only_a {
                col_a = col_a.child(
                    div()
                        .flex()
                        .gap(theme::GAP_SM)
                        .child(
                            div()
                                .w(px(40.))
                                .flex_shrink_0()
                                .font_family(theme::FAMILY_MONO)
                                .text_size(theme::FONT_MICRO)
                                .text_color(theme::ACCENT())
                                .child(SharedString::from(format!("第{ch}章"))),
                        )
                        .child(
                            div()
                                .text_size(theme::FONT_MICRO)
                                .text_color(theme::TEXT())
                                .child(SharedString::from(label.clone())),
                        ),
                );
            }
            let mut col_b = div().flex().flex_col().gap(px(2.));
            for (label, ch) in &cmp.only_b {
                col_b = col_b.child(
                    div()
                        .flex()
                        .gap(theme::GAP_SM)
                        .child(
                            div()
                                .w(px(40.))
                                .flex_shrink_0()
                                .font_family(theme::FAMILY_MONO)
                                .text_size(theme::FONT_MICRO)
                                .text_color(theme::BLUE())
                                .child(SharedString::from(format!("第{ch}章"))),
                        )
                        .child(
                            div()
                                .text_size(theme::FONT_MICRO)
                                .text_color(theme::TEXT())
                                .child(SharedString::from(label.clone())),
                        ),
                );
            }
            let mut conflicts_el = div().flex().flex_col().gap(px(2.));
            if cmp.conflicts.is_empty() {
                conflicts_el = conflicts_el.child(
                    div()
                        .text_size(theme::FONT_MICRO)
                        .text_color(theme::FAINT())
                        .child("两条线对同一目标的影响方式一致，没有冲突。"),
                );
            }
            for c in &cmp.conflicts {
                conflicts_el = conflicts_el.child(
                    div()
                        .text_size(theme::FONT_MICRO)
                        .text_color(theme::AMBER())
                        .child(SharedString::from(format!("⚠ {c}"))),
                );
            }
            Some(
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
                            .gap(theme::GAP_SM)
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
                                        this.lane = Some(id.clone());
                                        cx.notify();
                                    },
                                ))
                            }))
                            .child(
                                div()
                                    .px(px(6.))
                                    .text_size(theme::FONT_MICRO)
                                    .text_color(theme::FAINT())
                                    .child("vs"),
                            )
                            .children(pickable.iter().map(|l| {
                                let id = l.id.clone();
                                let on = id == right;
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
                    .child(
                        div()
                            .flex()
                            .gap(theme::GAP_GROUP)
                            .child(
                                div()
                                    .flex_1()
                                    .min_w_0()
                                    .flex()
                                    .flex_col()
                                    .gap(px(3.))
                                    .child(
                                        div()
                                            .text_size(theme::FONT_LABEL)
                                            .font_weight(FontWeight::SEMIBOLD)
                                            .text_color(theme::ACCENT())
                                            .child(SharedString::from(format!(
                                                "{} 独有 {}",
                                                cmp.a,
                                                cmp.only_a.len()
                                            ))),
                                    )
                                    .child(col_a),
                            )
                            .child(
                                div()
                                    .flex_1()
                                    .min_w_0()
                                    .flex()
                                    .flex_col()
                                    .gap(px(3.))
                                    .child(
                                        div()
                                            .text_size(theme::FONT_LABEL)
                                            .font_weight(FontWeight::SEMIBOLD)
                                            .text_color(theme::BLUE())
                                            .child(SharedString::from(format!(
                                                "{} 独有 {}",
                                                cmp.b,
                                                cmp.only_b.len()
                                            ))),
                                    )
                                    .child(col_b),
                            ),
                    )
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
                            .child(conflicts_el),
                    )
                    .into_any_element(),
            )
        } else {
            None
        };

        let body: gpui_kit::AnyElement = if let Some(cmp) = compare_el {
            cmp
        } else if let Some(lanes) = lanes_el {
            div()
                .flex_1()
                .min_h_0()
                .flex()
                .flex_col()
                .child(
                    div()
                        .flex_shrink_0()
                        .px(theme::PAD_LG)
                        .pt(theme::PAD_MD)
                        .child(lanes),
                )
                .child(
                    div()
                        .flex_1()
                        .min_h_0()
                        .flex()
                        .gap(theme::GAP_GROUP)
                        .p(theme::PAD_MD)
                        .child(
                            div()
                                .id("wl-list")
                                .w(px(280.))
                                .flex_shrink_0()
                                .min_h_0()
                                .flex()
                                .flex_col()
                                .overflow_y_scroll()
                                .children(list),
                        )
                        .child(
                            div()
                                .id("oracle-scroll")
                                .flex_1()
                                .min_w_0()
                                .min_h_0()
                                .flex()
                                .flex_col()
                                .gap(theme::GAP_XS)
                                .overflow_y_scroll()
                                .children(right),
                        ),
                )
                .into_any_element()
        } else {
            div()
                .flex_1()
                .min_h_0()
                .flex()
                .gap(theme::GAP_GROUP)
                .p(theme::PAD_MD)
                .child(
                    div()
                        .id("wl-tree")
                        .w(px(320.))
                        .flex_shrink_0()
                        .min_h_0()
                        .flex()
                        .flex_col()
                        .gap(theme::GAP_SM)
                        .overflow_y_scroll()
                        .children(tree_col),
                )
                .child(
                    div()
                        .id("wl-list")
                        .w(px(248.))
                        .flex_shrink_0()
                        .min_h_0()
                        .flex()
                        .flex_col()
                        .overflow_y_scroll()
                        .children(list),
                )
                .child(
                    div()
                        .id("oracle-scroll")
                        .flex_1()
                        .min_w_0()
                        .min_h_0()
                        .flex()
                        .flex_col()
                        .gap(theme::GAP_XS)
                        .overflow_y_scroll()
                        .children(right),
                )
                .into_any_element()
        };

        div()
            .size_full()
            .flex()
            .flex_col()
            .bg(theme::BG())
            .child(head)
            .child(body)
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
