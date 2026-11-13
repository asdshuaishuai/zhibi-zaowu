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

/// 关系维的 Agent 按钮。
///
/// 用 Button（不是自绘 div）：这一排是**动作**，不是数据，得有点击反馈、
/// 有禁用态。侧板里其他自绘的 chip 都是状态显示，不能混。
fn agent_btn(id: SharedString, label: &'static str) -> Button {
    Button::new(id).xsmall().label(label)
}

/// 关系粗分类 → 时间轴条色。
///
/// 与画布边的配色同一套语义（红=敌对、蓝=师承、绿=利益……），
/// 作者在画布上认熟一套颜色之后，侧板不该换一套。
fn rel_color(kind: &str) -> gpui_kit::Rgba {
    match kind {
        "敌对" => theme::RED(),
        "亲缘" => theme::AMBER(),
        "师承" => theme::BLUE(),
        "情爱" => theme::ACCENT(),
        "利益" => theme::GREEN(),
        _ => theme::DIM(),
    }
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
}

const DIMS: [(Dim, &'static str); 7] = [
    (Dim::Overview, "总览"),
    (Dim::Canon, "设定"),
    (Dim::Cast, "人物"),
    (Dim::Relations, "关系"),
    (Dim::Timeline, "时间"),
    (Dim::Events, "事件"),
    (Dim::Turns, "转折"),
];

enum Drag {
    Node(Vec<usize>, Point<Pixels>),
    Pan(Point<Pixels>),
    Marquee(Point<Pixels>),
}

const NODE_W: f32 = 150.;
const NODE_H: f32 = 76.;

/// 基因图每层的径向间距（与 `graph_layout::RING_GAP` 必须一致，
/// 否则同心圆画在两组节点之间，看着像错位）。
const RING_R: f32 = 210.;

/// 画布布局的版本号。
///
/// 换布局算法时 +1：`canvas.json` 里存着每个节点上次被拖到的位置，
/// 算法一变这些坐标就成了"新图上的旧点"，必须整体作废一次。
const CANVAS_LAYOUT_VERSION: u64 = 3;

/// 侧板宽度（关系演变的时间轴要按这个宽度换算像素，所以只能有一个真值）。
const SIDE_W: f32 = 250.;
const SIDE_PAD: f32 = 12.;
/// 关系演变条的轨道宽度 = 侧板宽度减去左右内边距。
const EVO_TRACK_W: f32 = SIDE_W - 2. * SIDE_PAD;
/// 演变条高度：只有一条 6px 色条（段标签已移到行首的简略章号里）。
const EVO_TRACK_H: f32 = 6.;

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
    /// 展开了详情的关系 id（一次只展开一条：侧板窄，展开两条就看不到别的了）
    evo_open: Option<String>,
    /// 基因图：主角节点下标与孤儿节点下标。
    ///
    /// 画布要知道"哪些节点还没接上主线"，才能在图下方画那块孤儿区。
    /// 这两个值由 `refresh()` 的布局算出，不落盘——它们是布局结果，不是数据。
    graph_center: Option<usize>,
    graph_orphans: Vec<usize>,
    /// 基因图的包围盒（布局算出来的，用于"适应窗口"）
    graph_size: (f32, f32),
    /// 每个节点离主角几跳（None = 孤儿）；画同心圆与分环着色用
    graph_rings: Vec<Option<u32>>,
    /// 舞台的像素尺寸（prepaint 每帧写回）。
    ///
    /// 为什么需要它：适应窗口要同时知道"图多大"和"窗口多大"，
    /// 后者只有在实际布局之后才知道。GPUI 里能拿到它的地方只有 paint 回调。
    stage_size: Rc<RefCell<Option<(f32, f32)>>>,
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
            evo_open: None,
            graph_center: None,
            graph_orphans: Vec::new(),
            graph_size: (0., 0.),
            graph_rings: Vec::new(),
            stage_size: Rc::new(RefCell::new(None)),
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
        };
        let state = self.handle.canvas_state();
        // 布局版本：变了就说明这一版的位置/视角都是旧算法的，全部作废。
        // 视角必须一起作废——只换坐标不换视角时，新图会开在旧视角的某个角落，
        // 看起来就是"打开是一片空白"。
        let layout_version = state
            .get("layoutVersion")
            .and_then(|v| v.as_u64())
            .unwrap_or(0);
        let keep_saved = layout_version >= CANVAS_LAYOUT_VERSION;
        if let Some(v) = state.get("view").filter(|_| keep_saved) {
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
        let want_events = !matches!(self.dim, Dim::Canon | Dim::Cast | Dim::Relations);
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

        // 基因图布局（总览/人物/事件/设定/转折）。
        //
        // 关系维有自己的一套（按章节区间的位置关系），时间维是顺序链，
        // 都不走基因图——它们表达的根本不是"离主角多远"。
        if !matches!(self.dim, Dim::Relations | Dim::Timeline) {
            let roster = crate::characters::Roster::new(&self.handle.root);
            let people = roster.all();
            let meta: Vec<crate::graph_layout::Meta> = nodes
                .iter()
                .map(|n| {
                    let role = if n.kind == "character" {
                        people
                            .iter()
                            .find(|c| c.matches(&n.label))
                            .map(|c| c.role.as_str())
                            .unwrap_or("")
                    } else {
                        ""
                    };
                    crate::graph_layout::Meta { kind: n.kind, label: &n.label, role }
                })
                .collect();
            let pairs: Vec<(usize, usize)> = edges.iter().map(|e| (e.from, e.to)).collect();
            let laid = crate::graph_layout::gene_map(&meta, &pairs);
            for (i, p) in laid.pos.iter().enumerate() {
                if let Some((x, y)) = p {
                    nodes[i].x = *x;
                    nodes[i].y = *y;
                }
            }
            self.graph_center = laid.center;
            self.graph_orphans = laid.orphans.clone();
            self.graph_size = laid.size;
            self.graph_rings = laid.ring.clone();
            if std::env::var("ZHIBI_DEBUG_LAYOUT").is_ok() {
                eprintln!(
                    "gene_map: n={} center={:?} orphans={} size={:?} first3={:?}",
                    nodes.len(),
                    laid.center,
                    laid.orphans.len(),
                    laid.size,
                    nodes.iter().take(3).map(|n| (n.id.clone(), n.x, n.y)).collect::<Vec<_>>()
                );
            }
        }

        // 恢复持久化坐标（dim:id 键）。
        //
        // 布局算法换代时旧坐标必须作废：`layoutVersion` 变了就整体丢弃一次，
        // 否则作者看到的是旧算法的位置叠在新算法的边上（比不对齐还糟）。
        if let Some(pos) = state.get("pos").and_then(|p| p.as_object()).filter(|_| keep_saved) {
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
        // 布局换代（或第一次看这一维）时，把整张图收进视野。
        // 判断依据是"存的坐标是否可用"：不可用说明刚重排过，旧视角没有意义。
        if !keep_saved && !matches!(self.dim, Dim::Relations | Dim::Timeline) {
            self.fit_to_content();
        }
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
            "layoutVersion": CANVAS_LAYOUT_VERSION,
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
    /// 选中节点在世界线上的落点（双向弱引用的反查）。
    ///
    /// 画布节点是"关系网上的点"，世界线节点是"时间轴上的关节"。同一个剧情
    /// 关节两边各有一份表示，`worldline::Node.canvas_ref` 指回这里。
    /// 找到就把"线X · 第N章 · 标题"列在侧板上——作者点一个人物，
    /// 能立刻看到"他在哪几条线的哪一章出现过"。
    fn worldline_hits(&self) -> Vec<(String, u32, String)> {
        let Some(&sel) = self.sel.last() else { return Vec::new() };
        let cid = &self.nodes[sel].id;
        let Ok(lines) = crate::worldline::Store::new(&self.handle.root).all() else {
            return Vec::new();
        };
        let store = crate::worldline::Store::new(&self.handle.root);
        store
            .nodes_referencing_canvas(&lines, cid)
            .into_iter()
            .map(|(l, n)| (l.id.clone(), n.chapter_or_guess(), n.label.clone()))
            .collect()
    }

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

    /// 外部跳转：记下要聚焦的世界线。
    ///
    /// 画布不再有"预言维"——世界线是时间轴视图，在「世界线」页看，
    /// 画布只画关系网。这里保留一个"我从哪条线过来"的痕迹，
    /// 供侧板显示"这条节点属于世界线 X 的第 N 章"的反向引用。
    pub fn focus_worldline(&mut self, id: String) {
        self.oracle_line = Some(id);
        self.refresh();
    }

    /// 让整张图落进视野（"适应窗口"）。
    ///
    /// 基因图在长书里能到 3000×4000（几十个节点、好几环），默认视图只显示左上角，
    /// 打开就像空的。所以布局一变（新书、切维、换算法）就自动适应一次；
    /// 作者手动缩放/平移之后不再打扰他。
    fn fit_to_content(&mut self) {
        let (gw, gh) = self.graph_size;
        if gw <= 1. || gh <= 1. {
            return;
        }
        // 窗口尺寸：优先用上一次 paint 量到的真实值；还没画过就按常见窗口估
        let (vw, vh) = self
            .stage_size
            .borrow()
            .map(|(w, h)| (w - 40., h - 40.))
            .filter(|(w, h)| *w > 120. && *h > 120.)
            .unwrap_or((1180., 900.));
        // 缩放上下限：太小看不清卡片，太大等于没适应
        // 下限 0.5：再小卡片上的名字就只剩一两个字，等于看不见。
        // 图大于窗口时宁可只看一部分，也不要缩成一团糊。
        let k = (vw / gw).min(vh / gh).clamp(0.45, 1.0);
        // 居中：图的中心对到窗口中心
        let x = (vw - gw * k) / 2.;
        let y = (vh - gh * k) / 2.;
        self.view = (x, y, k);
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

    /// 一行关系演变：名字 + 简略章号 + 一条按章定位的轨道。
    ///
    /// 侧板只有 226px，塞不下"第 5 章到第 19 章 师徒；第 20 章起 仇敌"这种全量描述。
    /// 所以这里只给**简略章号**（`5–19 → 20–`）+ 一条色条：作者在这一屏要判断的是
    /// "哪段、断没断、现在处在哪一段"。
    ///
    /// 两条"更细"的出路，都不在这块小面板里铺开：
    /// - 点名字 → 跳人物卡（外貌、动机、口吻、弧线的所在地）
    /// - 点章号 → 就地展开每一段的标签、读者可见性与依据
    fn evolution_row(
        &self,
        me: &str,
        row: &crate::relations::EvolutionRow,
        cx: &mut Context<Self>,
    ) -> gpui_kit::AnyElement {
        let horizon = self.last_chapter.max(1) as f32;
        // 章号 → 轨道内的像素左偏移。第 1 章贴左边，末章贴右边。
        let at = |ch: u32| -> f32 {
            let c = if ch == 0 { 1 } else { ch };
            ((c.saturating_sub(1) as f32 / horizon).clamp(0.0, 1.0)) * EVO_TRACK_W
        };
        let color = rel_color(&row.kind);

        let mut track = div()
            .relative()
            .h(px(EVO_TRACK_H))
            .w(px(EVO_TRACK_W))
            .rounded(px(3.))
            .bg(theme::BAR_TRACK());
        for (from, to, _label, public) in &row.spans {
            let x0 = at(*from);
            // to=0 表示至今：画到轨道右端（末章），而不是画成零宽
            let x1 = if *to == 0 { EVO_TRACK_W } else { at(*to) };
            let w = (x1 - x0).max(2.);
            let mut seg = div()
                .absolute()
                .left(px(x0))
                .top(px(0.))
                .h(px(EVO_TRACK_H))
                .w(px(w))
                .rounded(px(3.))
                .bg(hsla(color).opacity(if *public { 0.95 } else { 0.42 }));
            // 暗线（读者此刻还不知道）：描个虚边，和画布上的紫虚线同一套语义
            if !*public {
                seg = seg.border_1().border_color(hsla(theme::PURPLE()).opacity(0.9));
            }
            track = track.child(seg);
        }
        // "现在"的竖线：滑块所在章。它在时间轴上的位置就是判断"此刻关系"的依据。
        track = track.child(
            div()
                .absolute()
                .left(px(at(self.chapter)))
                .top(px(-1.))
                .w(px(1.))
                .h(px(EVO_TRACK_H + 2.))
                .bg(hsla(theme::ACCENT()).opacity(0.8)),
        );

        let rid = row.rid.clone();
        let other = row.other.clone();
        let open = self.evo_open.as_deref() == Some(rid.as_str());

        // 展开态：每一段一行（标签 + 章号 + 暗/明），比轨道多出的就是这些字
        let mut detail = div().flex().flex_col().gap(px(1.)).pt(px(2.)).pl(px(2.));
        for (from, to, label, public) in &row.spans {
            detail = detail.child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(5.))
                    .child(
                        div()
                            .w(px(46.))
                            .flex_shrink_0()
                            .font_family(theme::FAMILY_MONO)
                            .text_size(px(9.))
                            .text_color(theme::FAINT())
                            .child(SharedString::from(
                                crate::relations::EvolutionRow::span_text(*from, *to),
                            )),
                    )
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .text_ellipsis()
                            .text_size(px(9.))
                            .text_color(theme::DIM())
                            .child(SharedString::from(if label.is_empty() {
                                "（无标签）".to_string()
                            } else {
                                label.clone()
                            })),
                    )
                    .child(
                        div()
                            .flex_shrink_0()
                            .text_size(px(9.))
                            .text_color(if *public {
                                theme::FAINT()
                            } else {
                                theme::PURPLE()
                            })
                            .child(if *public { "明" } else { "暗" }),
                    ),
            );
        }

        let row_el = div()
            .flex()
            .flex_col()
            .gap(px(1.))
            .py(px(2.))
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(4.))
                    // 名字：跳到完整档案。关系面板只给脉络，不复述档案——
                    // 226px 里塞不下，塞了也只会变成两处不一致的第二份数据。
                    .child(
                        div()
                            .id(SharedString::from(format!("rel-who-{rid}")))
                            .flex_shrink_0()
                            .max_w(px(88.))
                            .overflow_hidden()
                            .whitespace_nowrap()
                            .text_ellipsis()
                            .cursor_pointer()
                            .text_size(theme::FONT_MICRO)
                            .text_color(theme::TEXT())
                            .hover(|d| d.text_color(theme::ACCENT()))
                            .child(SharedString::from(other.clone()))
                            .on_click(cx.listener(move |_this, _, _, cx| {
                                cx.emit(CanvasEvent::OpenCharacter { name: other.clone() });
                            })),
                    )
                    // 章号：点一下展开每一段；再点收起（侧板窄，只展开一条）
                    .child(
                        div()
                            .id(SharedString::from(format!("rel-open-{rid}")))
                            .flex_1()
                            .min_w_0()
                            .cursor_pointer()
                            .font_family(theme::FAMILY_MONO)
                            .text_size(px(9.))
                            .text_color(if row.has_gap { theme::AMBER() } else { theme::FAINT() })
                            .text_ellipsis()
                            .child(SharedString::from(format!(
                                "{}{}",
                                row.brief(),
                                if row.has_gap { " ⚠" } else { "" }
                            )))
                            .on_click(cx.listener({
                                let rid = rid.clone();
                                move |this, _, _, cx| {
                                    this.evo_open = if this.evo_open.as_deref() == Some(rid.as_str())
                                    {
                                        None
                                    } else {
                                        Some(rid.clone())
                                    };
                                    cx.notify();
                                }
                            })),
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
                                let me = me.to_string();
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
                                                    &format!("删除关系（{me}）"),
                                                );
                                        }
                                        Err(e) => eprintln!("删除关系失败：{e}"),
                                    }
                                    this.refresh();
                                    cx.notify();
                                }
                            })),
                    ),
            )
            .child(track)
            .when(open, |d| d.child(detail));
        row_el.into_any_element()
    }
}

impl EventEmitter<PanelEvent> for CanvasPanel {}
impl EventEmitter<CanvasEvent> for CanvasPanel {}

/// 画布发出去的事件。
///
/// 关系维的 Agent 入口只发事件、不自己调 DSH：派活的唯一入口在 workspace，
/// 轨迹窗、忙碌保护、模式判断都在那边。面板自己发指令会绕过这三样。
#[derive(Debug, Clone)]
pub enum CanvasEvent {
    /// 派活给某个专属 Agent（`target` = None 表示模块级一键能力）
    Delegate {
        agent: &'static str,
        key: &'static str,
        target: Option<String>,
    },
    /// 打开某个人物的完整档案卡。
    ///
    /// 关系面板刻意只给脉络：一行关系占 226px，写不下外貌、动机、弧线。
    /// 想读全的那份就点名字跳过去——而不是把侧板撑成第二个档案页。
    OpenCharacter { name: String },
}

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
                // 选中某点：与它相连的亮起来，其余压到几乎看不见——
                // 这正是一阶影响半径该有的样子，也让密集图能读。
                Some(f) if e.from == f || e.to == f => 0.95,
                Some(_) => 0.08,
                // 没选中：整体压淡。63 个节点近两百条边，0.55 就是一张毛线团；
                // 压到 0.22 之后节点卡与层次重新成为主体，边退成背景纹理。
                None => 0.22,
            };
            segs.push((self.sx(a), self.sy(a), self.sx(b), self.sy(b), e.kind, e.hidden, alpha));
        }

        let marquee_now = self.marquee;
        let origin_cell = self.stage_origin.clone();
        let size_cell = self.stage_size.clone();
        // 孤儿区与主角：基因图的两块"额外语义"，paint 里要画出来
        let orphan_zone: Option<(f32, f32, f32, f32)> = if self.graph_orphans.is_empty() {
            None
        } else {
            let mut x0 = f32::MAX;
            let mut y0 = f32::MAX;
            let mut x1 = f32::MIN;
            let mut y1 = f32::MIN;
            for &i in &self.graph_orphans {
                if let Some(n) = self.nodes.get(i) {
                    x0 = x0.min(n.x);
                    y0 = y0.min(n.y);
                    x1 = x1.max(n.x + NODE_W);
                    y1 = y1.max(n.y + NODE_H);
                }
            }
            (x0 != f32::MAX).then_some((x0, y0, x1, y1))
        };
        let center_i = self.graph_center;
        // 主角中心与最大环数：画同心圆用（圆以节点中心为心）
        let ring_center: Option<(f32, f32)> = center_i
            .and_then(|i| self.nodes.get(i))
            .map(|n| (n.x + NODE_W / 2., n.y + NODE_H / 2.));
        let ring_count = self
            .graph_rings
            .iter()
            .filter_map(|r| *r)
            .max()
            .unwrap_or(0)
            .min(6);
        // 基因图用直线画边：径向布局里"水平控制点的贝塞尔"会绕出一堆回环，
        // 几十条叠起来就是一团乱麻。直线至少让人看清"谁连着谁"。
        // 时间维是顺序链，直线也一样更好读。
        let view_x = self.view.0;
        let view_y = self.view.1;
        let view_k = self.view.2;
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
                        *size_cell.borrow_mut() = Some((
                            bounds.size.width.as_f32(),
                            bounds.size.height.as_f32(),
                        ));
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
                        // 基因图的层环：以主角为心的几道同心圆。
                        // 没有它就是一盘散点——作者看不出"这层是直接相关的人、
                        // 那层是隔一层的"。环是虚线，退在节点与边之后。
                        if let Some((cx, cy)) = ring_center {
                            let k = view_k;
                            let bo = bounds.origin;
                            let c = point(
                                px(bo.x.as_f32() + (cx + view_x) * k),
                                px(bo.y.as_f32() + (cy + view_y) * k),
                            );
                            // 四段贝塞尔近似圆，0.5523 是四分之一圆的经典系数
                            for r in 1..=ring_count {
                                let rr = r as f32 * RING_R * k;
                                let kk = rr * 0.5523;
                                let mut circ = PathBuilder::stroke(px(1.));
                                circ = circ.dash_array(&[px(2.), px(7.)]);
                                circ.move_to(point(c.x + px(rr), c.y));
                                circ.cubic_bezier_to(point(c.x, c.y + px(rr)), point(c.x + px(rr), c.y + px(kk)), point(c.x + px(kk), c.y + px(rr)));
                                circ.cubic_bezier_to(point(c.x - px(rr), c.y), point(c.x - px(kk), c.y + px(rr)), point(c.x - px(rr), c.y + px(kk)));
                                circ.cubic_bezier_to(point(c.x, c.y - px(rr)), point(c.x - px(rr), c.y - px(kk)), point(c.x - px(kk), c.y - px(rr)));
                                circ.cubic_bezier_to(point(c.x + px(rr), c.y), point(c.x + px(kk), c.y - px(rr)), point(c.x + px(rr), c.y - px(kk)));
                                if let Ok(path) = circ.build() {
                                    window.paint_path(path, hsla(theme::LINE()).opacity(0.55));
                                }
                            }
                        }
                        // 孤儿区：主图下方的一块虚线围栏 + 标题。
                        // 画在最底层（边之前），免得盖住连线。
                        if let Some((x0, y0, x1, y1)) = orphan_zone {
                            let pad = 26.;
                            let k = view_k;
                            let bo = bounds.origin;
                            let r = |x: f32, y: f32| {
                                point(
                                    px(bo.x.as_f32() + (x + view_x) * k),
                                    px(bo.y.as_f32() + (y + view_y) * k),
                                )
                            };
                            let mut box_p = PathBuilder::stroke(px(1.));
                            box_p = box_p.dash_array(&[px(6.), px(6.)]);
                            box_p.move_to(r(x0 - pad, y0 - pad));
                            box_p.line_to(r(x1 + pad, y0 - pad));
                            box_p.line_to(r(x1 + pad, y1 + pad));
                            box_p.line_to(r(x0 - pad, y1 + pad));
                            box_p.close();
                            if let Ok(path) = box_p.build() {
                                window.paint_path(path, hsla(theme::LINE()).opacity(0.9));
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
                            b.line_to(end);
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
                } else if center_i == Some(i) {
                    // 主角：整张图的原点，边框要和别的不一样，否则"为什么它在中"看不出来
                    theme::AMBER()
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
                        .when(center_i == Some(i), |d| {
                            // 主角标记：作者一眼能看出"这是圆心"
                            d.child(
                                div()
                                    .text_size(px(9.))
                                    .text_color(theme::AMBER())
                                    .child("◎"),
                            )
                        })
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
        // 世界线上的落点（反查）：点人物/事件节点，能看到它在哪几条线的哪一章
        let wl_hits = self.worldline_hits();
        let side = div()
            .absolute()
            .right(theme::PAD_LG)
            .top(theme::PAD_LG)
            .w(px(SIDE_W))
            .p(px(SIDE_PAD))
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
            // 世界线落点：这张卡在时间轴上出现的位置（双向弱引用的反查）。
            // 没有关联就不占地方——大多数节点没被世界线引用过，空面板是噪音。
            .when(!wl_hits.is_empty(), |d| {
                d.child(
                    div()
                        .pt(theme::GAP_SM)
                        .pb(px(2.))
                        .text_size(theme::FONT_MICRO)
                        .font_weight(FontWeight::SEMIBOLD)
                        .text_color(theme::BLUE())
                        .child(SharedString::from(format!("世界线落点 · {}", wl_hits.len()))),
                )
                .children(wl_hits.iter().map(|(lid, ch, label)| {
                    div()
                        .flex()
                        .items_center()
                        .gap(px(4.))
                        .py(px(1.))
                        .child(
                            div()
                                .font_family(theme::FAMILY_MONO)
                                .text_size(theme::FONT_MICRO)
                                .text_color(theme::ACCENT())
                                .child(SharedString::from(lid.clone())),
                        )
                        .child(
                            div()
                                .font_family(theme::FAMILY_MONO)
                                .text_size(theme::FONT_MICRO)
                                .text_color(theme::FAINT())
                                .child(SharedString::from(if *ch > 0 {
                                    format!("第{ch}章")
                                } else {
                                    "未排章".into()
                                })),
                        )
                        .child(
                            div()
                                .flex_1()
                                .min_w_0()
                                .text_ellipsis()
                                .text_size(theme::FONT_MICRO)
                                .text_color(theme::DIM())
                                .child(SharedString::from(label.clone())),
                        )
                }))
            })
            // 关系维：选中一个人时，给出「与谁、什么关系、从第几章起」的就地编辑，
            // 以及该人参与的关系史（含已结束的区间）与删除入口。
            .when(self.dim == Dim::Relations && self.sel.len() == 1, |d| {
                let me = self.nodes[self.sel[0]].label.clone();
                let web = crate::relations::Web::new(&self.handle.root);
                let rows = crate::relations::evolution(&web, &me, self.last_chapter);
                let mut hist = div().flex().flex_col().gap(px(3.)).pt(theme::GAP_SM);
                if rows.is_empty() {
                    hist = hist.child(
                        div()
                            .text_size(theme::FONT_MICRO)
                            .text_color(theme::FAINT())
                            .child("还没有关系记录"),
                    );
                }
                for row in rows.iter().take(8) {
                    hist = hist.child(self.evolution_row(&me, row, cx));
                }
                d.child(sec_label_mini("关系演变"))
                    .child(
                        div()
                            .text_size(theme::FONT_MICRO)
                            .text_color(theme::FAINT())
                            .child(SharedString::from(format!(
                                "横轴第 1-{} 章，竖线是现在（第{}章）",
                                self.last_chapter.max(1),
                                self.chapter
                            ))),
                    )
                    .child(hist)
                    // 关系维的 Agent 入口。两种：
                    // 模块级「全量梳理」= 从正文重建整张网；
                    // 指定对象「这个人跟谁有关」= 只捋选中的这个人的关系。
                    // 都必须在按钮文字里说清作用范围，作者才知道点下去会改多少东西。
                    .children(crate::capabilities::agent("relations").map(|a| {
                        let mut bar = div().flex().flex_wrap().gap(px(4.)).pt(theme::GAP_SM);
                        for cap in a.bulk {
                            bar = bar.child(agent_btn(
                                SharedString::from(format!("rel-agent-{}", cap.key)),
                                cap.label,
                            )
                            .on_click(cx.listener(move |_this, _, _, cx| {
                                cx.emit(CanvasEvent::Delegate {
                                    agent: "relations",
                                    key: cap.key,
                                    target: None,
                                });
                            })));
                        }
                        for op in a.targeted {
                            // 只有 Chapter / Character 这类"带对象"的操作在这个面板上有意义；
                            // 面板拿不到的对象类型（线、节点、台账条目）不显示，
                            // 免得点下去指令里写个空对象。
                            let desc = match op.target_kind {
                                crate::capabilities::TargetKind::Character => Some(format!(
                                    "人物：{me}（画布关系维选中）"
                                )),
                                crate::capabilities::TargetKind::Chapter => {
                                    Some(format!("第{}章", self.chapter))
                                }
                                _ => None,
                            };
                            let Some(desc) = desc else { continue };
                            bar = bar.child(agent_btn(
                                SharedString::from(format!("rel-agent-{}", op.key)),
                                op.label,
                            )
                            .on_click(cx.listener(move |_this, _, _, cx| {
                                cx.emit(CanvasEvent::Delegate {
                                    agent: "relations",
                                    key: op.key,
                                    target: Some(desc.clone()),
                                });
                            })));
                        }
                        bar.into_any_element()
                    }))
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
            .when(!self.graph_orphans.is_empty(), |d| {
                // 孤儿区说明：告诉作者这块是什么、为什么在这儿
                d.child(
                    div()
                        .pt(theme::GAP_SM)
                        .text_size(theme::FONT_MICRO)
                        .text_color(theme::FAINT())
                        .child(SharedString::from(format!(
                            "虚线框里是 {} 个还没接上主角的节点（孤儿区）",
                            self.graph_orphans.len()
                        ))),
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
                        // ⊡ = 适应窗口（不是"回到原点"）：作者要的是"让我看全"
                        this.fit_to_content();
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

        stage.child(side).child(bar).child(tabs)
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

    /// 预测续写（M8）：fork 自动=最新成稿下一章，AI 按大纲/成稿向前织
    Forecast,
    /// 晋升成功：宿主刷新收件箱/轨迹
    Promoted { worldline: String, proposal_id: String },
    /// 在画布上查看该世界线（切到预言维）
    ViewOnCanvas { worldline: String },
}
pub struct OraclePanel {
    focus_handle: FocusHandle,
    handle: Rc<ProjectHandle>,
    premise: Entity<InputState>,
    fork: Entity<InputState>,
    selected: Option<String>,
    message: Option<SharedString>,
    /// 线模式选中的车道（与 selected 分开：一条线可以"选中看影响"但车道另外聚焦）
    lane: Option<String>,
    /// 车道里点开的节点（详情抽屉用）
    node: Option<String>,
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
            lane: None,
            node: None,
        }
    }

    /// 选中某条世界线（世界线页联动过来）。推演/晋升都针对它。
    pub fn select_line(&mut self, id: String, cx: &mut Context<Self>) {
        self.selected = Some(id.clone());
        self.lane = Some(id);
        self.node = None;
        self.message = None;
        cx.notify();
    }

    /// 外部跳转：直接切到预言维并选中某条世界线（预言书「在画布查看」）。    /// 当前活跃车道：作者在预言书里点中的那条线；没点过就是正典 α。
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
                    // 迷你走势：一条 60px 宽的小横线，节点按章落点。
                    // 不做成图例也不是装饰——一眼看出"这条线推到第几章、密不密"，
                    // 比"节点 7"这种数字有用。
                    .when(!is_alpha, |d| {
                        let mut chs: Vec<u32> = w
                            .get("nodes")
                            .and_then(|n| n.as_array())
                            .map(|arr| {
                                arr.iter()
                                    .map(|n| n.get("chapter").and_then(|c| c.as_u64()).unwrap_or(0) as u32)
                                    .filter(|c| *c > 0)
                                    .collect()
                            })
                            .unwrap_or_default();
                        chs.sort_unstable();
                        let span = chs.last().copied().unwrap_or(0).max(1);
                        d.child(
                            div().h(px(10.)).w_full().relative().mt(px(2.)).children(
                                chs.iter().map(|c| {
                                    div()
                                        .absolute()
                                        .left(relative((*c as f32 / span as f32).min(1.)))
                                        .top(px(3.))
                                        .w(px(3.))
                                        .h(px(3.))
                                        .rounded(px(1.5))
                                        .bg(theme::GREEN())
                                }),
                            ),
                        )
                    })
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap(theme::GAP_SM)
                            .when(!is_alpha, |d| {
                                // 分叉章优先取 forkNode 所在章（与画布一致），
                                // 否则卡片说第 16 章、画布画在第 12 章，作者会怀疑数据坏了。
                                let fork_ch = w
                                    .get("forkNode")
                                    .and_then(|n| n.as_str())
                                    .and_then(|fnode| {
                                        w.get("nodes")
                                            .and_then(|nx| nx.as_array())
                                            .and_then(|arr| {
                                                arr.iter()
                                                    .find(|n| s(n, "id") == fnode)
                                                    .map(|n| {
                                                        n.get("chapter")
                                                            .and_then(|c| c.as_u64())
                                                            .unwrap_or(0)
                                                    })
                                            })
                                    })
                                    .unwrap_or_else(|| {
                                        w.get("forkedChapter").and_then(|v| v.as_u64()).unwrap_or(0)
                                    });
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
                                "「{wid}」推演卷轴 —— 沿因果链走，每条影响晋升后才成提案"
                            ))),
                    )
                    .child(
                        Button::new(SharedString::from(format!("see-{wide}")))
                            .xsmall()
                            // 这个按钮现在跳到「世界线」页（画布不再有预言维），
                            // 所以文字也跟着改——按钮写着"在画布查看"却跳到别处，
                            // 点一次就会怀疑自己点错了。
                            .label("看时间轴")
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

            // —— 因果链卷轴：把这条线的节点按章串起来，每章一段 ——
            //
            // 为什么先画节点再画影响：作者读推演时先想知道"这条线都发生了什么"，
            // 再关心"这些事会改动正典的什么"。节点是叙事，diffs 是账。
            {
                let mut nodes: Vec<(u32, String, String, bool)> = w
                    .get("nodes")
                    .and_then(|n| n.as_array())
                    .map(|arr| {
                        arr.iter()
                            .map(|n| {
                                (
                                    n.get("chapter").and_then(|c| c.as_u64()).unwrap_or(0) as u32,
                                    s(n, "label"),
                                    s(n, "note"),
                                    s(n, "state") == "written",
                                )
                            })
                            .collect()
                    })
                    .unwrap_or_default();
                nodes.sort_by_key(|(ch, _, _, _)| *ch);
                if !nodes.is_empty() {
                    right.push(
                        div()
                            .pb(theme::GAP_XS)
                            .text_size(theme::FONT_TAG)
                            .font_weight(FontWeight::SEMIBOLD)
                            .text_color(theme::BLUE())
                            .child(SharedString::from(format!("推演卷轴 · {} 节点", nodes.len())))
                            .into_any_element(),
                    );
                    let mut last_ch = u32::MAX;
                    for (ch, label, note, written) in nodes {
                        // 章号变化时打一个章节分隔条，形成"卷轴分段"
                        if ch != last_ch {
                            last_ch = ch;
                            right.push(
                                div()
                                    .pt(theme::GAP_SM)
                                    .pb(theme::GAP_XS)
                                    .flex()
                                    .items_center()
                                    .gap(theme::GAP_SM)
                                    .child(
                                        div()
                                            .font_family(theme::FAMILY_MONO)
                                            .text_size(theme::FONT_MICRO)
                                            .text_color(theme::FAINT())
                                            .child(SharedString::from(if ch > 0 {
                                                format!("第{ch}章")
                                            } else {
                                                "未排章".into()
                                            })),
                                    )
                                    .child(
                                        div().flex_1().h(px(1.)).bg(theme::LINE_SOFT()),
                                    )
                                    .into_any_element(),
                            );
                        }
                        right.push(
                            div()
                                .flex()
                                .items_start()
                                .gap(theme::GAP_SM)
                                .py(px(1.))
                                .child(
                                    // 竖线把同一章下的节点串成一段
                                    div()
                                        .w(px(1.))
                                        .h(px(14.))
                                        .flex_shrink_0()
                                        .bg(if written { theme::GREEN() } else { theme::LINE() }),
                                )
                                .child(
                                    div()
                                        .flex_1()
                                        .min_w_0()
                                        .text_size(theme::FONT_LABEL)
                                        .text_color(if written { theme::TEXT() } else { theme::DIM() })
                                        .child(SharedString::from(label)),
                                )
                                .when(!note.is_empty(), |d| {
                                    d.child(
                                        div()
                                            .max_w(px(180.))
                                            .text_size(theme::FONT_MICRO)
                                            .text_color(theme::FAINT())
                                            .child(SharedString::from(
                                                note.chars().take(40).collect::<String>(),
                                            )),
                                    )
                                })
                                .into_any_element(),
                        );
                    }
                }
            }

            if !diffs.is_empty() {
                right.push(
                    div()
                        .pt(theme::GAP_MD)
                        .pb(theme::GAP_XS)
                        .text_size(theme::FONT_TAG)
                        .font_weight(FontWeight::SEMIBOLD)
                        .text_color(theme::AMBER())
                        .child(SharedString::from(format!("对正典的影响 · {} 条", diffs.len())))
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
            // —— 影响面板：波及面 + 批量操作 ——
            //
            // 单条晋升在每条影响上；这里给"这一整条线现在什么状态"的俯瞰，
            // 以及"全都升"——作者推演完通常是整条线一起拿定主意，
            // 逐条点 20 次不如一次批量。
            {
                let diffs_all = w.get("diffs").and_then(|x| x.as_array()).cloned().unwrap_or_default();
                let total = diffs_all.len();
                let promoted = diffs_all
                    .iter()
                    .filter(|d| d.get("promoted").and_then(|p| p.as_bool()).unwrap_or(false))
                    .count();
                let pending: Vec<usize> = diffs_all
                    .iter()
                    .enumerate()
                    .filter(|(_, d)| !d.get("promoted").and_then(|p| p.as_bool()).unwrap_or(false))
                    .map(|(i, _)| i)
                    .collect();
                let mut high = 0;
                for d in &diffs_all {
                    if s(d, "severity") == "high" {
                        high += 1;
                    }
                }
                // 波及几条线：同一 parentId 的兄弟线数（"这条假设和谁在同一祖先下竞争"）
                let parent_id = s(w, "parentId");
                let siblings = lines
                    .iter()
                    .filter(|x| {
                        let xid = s(x, "id");
                        xid != s(w, "id") && s(x, "parentId") == parent_id && !parent_id.is_empty()
                    })
                    .count();
                right.push(
                    div()
                        .mt(theme::GAP_MD)
                        .p(theme::PAD_SM)
                        .rounded(theme::RADIUS_PANEL)
                        .border_1()
                        .border_color(theme::LINE())
                        .bg(theme::PANEL())
                        .flex()
                        .flex_col()
                        .gap(theme::GAP_XS)
                        .child(
                            div()
                                .flex()
                                .items_center()
                                .gap(theme::GAP_MD)
                                .child(
                                    div()
                                        .text_size(theme::FONT_TAG)
                                        .font_weight(FontWeight::SEMIBOLD)
                                        .text_color(theme::TEXT())
                                        .child("影响面"),
                                )
                                .child(
                                    div()
                                        .font_family(theme::FAMILY_MONO)
                                        .text_size(theme::FONT_MICRO)
                                        .text_color(theme::FAINT())
                                        .child(SharedString::from(format!(
                                            "共 {total} 条 · 待晋升 {} · 已晋升 {promoted} · 高风险 {high} · 同门竞争 {siblings} 条线",
                                            pending.len()
                                        ))),
                                )
                                .child(div().flex_1())
                                .when(!pending.is_empty(), |d| {
                                    let wid2 = s(w, "id");
                                    let mut pending2 = pending.clone();
                                    pending2.sort_unstable();
                                    let pending3 = pending2.clone();
                                    d.child(
                                        Button::new(SharedString::from(format!("promote-all-{wid2}")))
                                            .xsmall()
                                            .label(format!("全部晋升 {} 条", pending.len()))
                                            .on_click(cx.listener(move |this, _, _, cx| {
                                                let wid = this.selected.clone().unwrap_or_default();
                                                // 倒序推：promote_diff 目前是原地标记（不重排数组），
                                                // 正序也能对；但倒序在任何"以后会删元素"的改法下都安全。
                                                let mut ok = 0usize;
                                                let mut errs: Vec<String> = Vec::new();
                                                for i in pending3.clone().into_iter().rev() {
                                                    match this.handle.promote_diff(&wid, i) {
                                                        Ok(_) => ok += 1,
                                                        Err(e) => errs.push(e),
                                                    }
                                                }
                                                this.message = Some(if errs.is_empty() {
                                                    format!("已批量晋升 {ok} 条 → 收件箱待裁决").into()
                                                } else {
                                                    format!("晋升 {ok} 条，失败 {} 条：{}", errs.len(), errs.join("；"))
                                                        .into()
                                                });
                                                cx.emit(OracleEvent::Promoted {
                                                    worldline: wid,
                                                    proposal_id: format!("batch×{ok}"),
                                                });
                                                cx.notify();
                                            })),
                                    )
                                }),
                        )
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
                        b.cubic_bezier_to(p(x + k, y + r), p(x, y + r), p(x + r, y + k));
                        b.cubic_bezier_to(p(x - r, y + k), p(x - r, y), p(x - k, y + r));
                        b.cubic_bezier_to(p(x - k, y - r), p(x, y - r), p(x - r, y - k));
                        b.cubic_bezier_to(p(x + r, y - k), p(x + r, y), p(x + k, y - r));
                        b
                    };
                    let leaf = |bx: f32, by: f32, ang: f32, len: f32| {
                        let (dx, dy) = (ang.cos(), ang.sin());
                        let (nx, ny) = (-dy, dx);
                        let hw = len * 0.30;
                        let mut b = PathBuilder::fill();
                        b.move_to(p(bx, by));
                        b.cubic_bezier_to(p(bx + dx * len, by + dy * len), p(bx + dx * len * 0.30 + nx * hw, by + dy * len * 0.30 + ny * hw), p(
                                bx + dx * len * 0.82 + nx * hw * 0.65,
                                by + dy * len * 0.82 + ny * hw * 0.65,
                            ));
                        b.cubic_bezier_to(p(bx, by), p(
                                bx + dx * len * 0.82 - nx * hw * 0.65,
                                by + dy * len * 0.82 - ny * hw * 0.65,
                            ), p(bx + dx * len * 0.30 - nx * hw, by + dy * len * 0.30 - ny * hw));
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
                        r.cubic_bezier_to(p(trunk_x + off * 2.2, TH - 4.), p(trunk_x + sd * 3., base_y + 8.), p(trunk_x + off * 1.2, TH - 14.));
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
                        bp.cubic_bezier_to(p(b.c2.0, b.c2.1), p(b.tip.0, b.tip.1), p(b.c1.0, b.c1.1));
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
                            tp.cubic_bezier_to(p(cx, cy - 3.), p(ex, ey), p(cx, cy));
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

        // 三视图（并轨 / 世界树 / 对比）已经搬到「世界线」页——
        // 那一页是时间轴（横轴=章节、固化段实线、开放段虚线），
        // 这一页（预言书）只做一件事：选中一条 if 线，推演它，看影响，决定扶不扶正。

        // 页头。
        //
        // 这里**不再**放「并轨 / 世界树 / 对比」三视图——那三个按钮属于「世界线」页，
        // 留在预言书里会让作者以为这是同一个页面（也真的有人这么问过：
        // "章节排期为什么也叫世界线"）。预言书只做一件事：
        // 选中一条 if，推演它、看影响、决定扶不扶正。
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
                    .child("预言书"),
            )
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .text_size(theme::FONT_MICRO)
                    .text_color(theme::FAINT())
                    .child("if 工坊：选中一条假设 → 推演卷轴 → 逐条晋升或整条扶正；正典只由作者改"),
            );

        // —— 对比视图：两条线并排 ——
        // 预言书的两栏：左 = 这条线（及同分叉点的兄弟线），中 = 推演卷轴与影响面。
        let body: gpui_kit::AnyElement = div()
            .flex_1()
            .min_h_0()
            .flex()
            .gap(theme::GAP_GROUP)
            .p(theme::PAD_MD)
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
            .into_any_element();

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
            .child("预言书")
    }
}
