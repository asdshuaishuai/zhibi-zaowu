//! 世界线：带类型的读写层 + 纯函数布局。
//!
//! 为什么不再直接操作 `serde_json::Value`：世界线是唯一同时被"作者手改"
//! 和"AI 推演"两种来源写入的结构，字段一多就容易拼错（`forkNode` 写成
//! `forkNodeId`），而拼错是静默的——画布上少一条枝，没人会报错。
//!
//! 这里只负责三件事，其余（晋升、提案、画布交互）留在原有路径上：
//! 1. **读写**：把 `worldlines.json` 反序列化成 `Line`，写回时保留未知字段
//!   （`extra`），不让新代码吃掉老数据。
//! 2. **血缘**：`parentId` / `forkNode` 表达"这条线从哪条线的哪个节点长出"，
//!   比只有一个 `forkedChapter` 更精确——同一章可能有两个节点。
//! 3. **布局**：线模式的车道顺序、树模式的子树宽度分配。纯函数，可测，
//!    同一份数据两次布局必须一模一样（否则每次重绘树都会抖）。

pub mod layout;
pub mod view;

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

/// 线的性质。决定时间轴上的线型与分组。
///
/// 这里最容易混的一点：`Base`（正典）与 `Fixed`（被扶正的 if）**不是**一回事。
/// - `Base` 是唯一那条「已经写进正文」的线（α）。
/// - `Fixed` 是作者从某条 if 里选定、正式收进正文后续的那条线。
///   它扶正之后 alpha 的固化边界会推到它的末尾，它自己留作"这一版是怎么来的"。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Kind {
    /// 正典基准（唯一，id 固定为 α）
    Base,
    /// 已扶正：从 if 线里被作者选定，成了正典的后续
    Fixed,
    /// 预测线：按大纲/成稿往前推演"会怎样"
    Forecast,
    /// 假设线：what-if
    #[default]
    WhatIf,
}

impl Kind {
    pub fn key(self) -> &'static str {
        match self {
            Kind::Base => "base",
            Kind::Fixed => "fixed",
            Kind::Forecast => "forecast",
            Kind::WhatIf => "what-if",
        }
    }

    pub fn from_key(s: &str) -> Self {
        match s {
            "base" => Kind::Base,
            "fixed" => Kind::Fixed,
            "forecast" => Kind::Forecast,
            _ => Kind::WhatIf,
        }
    }

    #[allow(dead_code)] // 界面按需取词；词表放在类型上，别散到各页
    pub fn label(self) -> &'static str {
        match self {
            Kind::Base => "正典",
            Kind::Fixed => "已固化",
            Kind::Forecast => "预测",
            Kind::WhatIf => "假设",
        }
    }
}

// 存盘用 key 字符串（"what-if"），而不是枚举判别式——
// 磁盘格式要能被人读懂、也要能被 AI 直接写。
impl Serialize for Kind {
    fn serialize<S: serde::Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        s.serialize_str(self.key())
    }
}

impl<'de> Deserialize<'de> for Kind {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let raw = String::deserialize(d)?;
        Ok(Kind::from_key(&raw))
    }
}

/// 节点在"这件事到底发生了没有"上的状态。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum NodeState {
    /// 计划中（还没写进正文）
    #[default]
    Planned,
    /// 已写进正文
    Written,
    /// 只在这个分支里存在（正典里没有）
    BranchOnly,
    /// 作者已采纳进正典，但正文还没写到那一章。
    ///
    /// 和 `Planned` 的区别是作者意图：`Planned` 是"这条 if 线里假设会这样"，
    /// `Adopted` 是"我决定了，这就是正典后续"。正文一旦写到那一章就转 `Written`。
    /// 没有这个状态的话，逐节点采纳之后界面上看不出到底承认了没有。
    Adopted,
}

impl NodeState {
    pub fn key(self) -> &'static str {
        match self {
            NodeState::Planned => "planned",
            NodeState::Written => "written",
            NodeState::BranchOnly => "branch-only",
            NodeState::Adopted => "adopted",
        }
    }

    pub fn from_key(s: &str) -> Self {
        match s {
            "written" => NodeState::Written,
            "branch-only" => NodeState::BranchOnly,
            "adopted" => NodeState::Adopted,
            _ => NodeState::Planned,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            NodeState::Planned => "计划",
            NodeState::Written => "已写",
            NodeState::BranchOnly => "仅此分支",
            NodeState::Adopted => "已采纳",
        }
    }

    /// 这个节点算不算已定的正典内容（扶正边界、固化段显示都按它判断）。
    pub fn is_canon(self) -> bool {
        matches!(self, NodeState::Written | NodeState::Adopted)
    }
}

impl Serialize for NodeState {
    fn serialize<S: serde::Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        s.serialize_str(self.key())
    }
}

impl<'de> Deserialize<'de> for NodeState {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let raw = String::deserialize(d)?;
        Ok(NodeState::from_key(&raw))
    }
}

/// 世界线上的一个节点（一个"发生了什么"）。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Node {
    pub id: String,
    /// storyline / event / character / clue / canon
    #[serde(default)]
    pub kind: String,
    #[serde(default)]
    pub label: String,
    /// 指向正典对象 id（可空）
    #[serde(default)]
    pub reference: String,
    /// 指向画布节点的 id（双向弱引用的一半）。
    ///
    /// 世界线节点是「时间轴上的一个关节」，画布节点是「关系网上的一个点」。
    /// 同一个剧情关节两边各有一份表示：世界线侧重"第几章发生了这件事"，
    /// 画布侧重"这件事跟谁相关"。这个字段把它们连起来，作者才能从
    /// 「第 12 章的观灯人现身」跳到「他跟沈砚是什么关系」。
    ///
    /// 弱引用的意思：画布节点被删了，这里不会崩，只是查不到关系。
    #[serde(default)]
    pub canvas_ref: String,
    /// 落在第几章。0 = 未排章。
    #[serde(default)]
    pub chapter: u32,
    #[serde(default)]
    pub note: String,
    #[serde(default)]
    pub state: NodeState,
    /// 画板自由坐标（用户拖动过才有意义）
    #[serde(default)]
    pub x: Option<f32>,
    #[serde(default)]
    pub y: Option<f32>,
    /// 未知字段一律保留，避免这一版丢掉别处写入的属性
    #[serde(flatten, default)]
    pub extra: BTreeMap<String, serde_json::Value>,
}

impl Node {
    pub fn new(id: &str, kind: &str, label: &str) -> Self {
        Self {
            id: id.to_string(),
            kind: kind.to_string(),
            label: label.to_string(),
            reference: String::new(),
            canvas_ref: String::new(),
            chapter: 0,
            note: String::new(),
            state: NodeState::default(),
            x: None,
            y: None,
            extra: BTreeMap::new(),
        }
    }

    /// 落在哪一章：优先用显式 `chapter`；没有就从 ref 里的 `ch-NNN` 猜。
    /// 猜不出来就是 0（画布会把它摆在"现在"之后，不当成已发生）。
    pub fn chapter_or_guess(&self) -> u32 {
        if self.chapter > 0 {
            return self.chapter;
        }
        guess_chapter(&self.reference)
            .or_else(|| guess_chapter(&self.label))
            .unwrap_or(0)
    }
}

/// 从任意字符串里认出 `ch-012` / `第12章` 两种写法。
pub(crate) fn guess_chapter(s: &str) -> Option<u32> {
    if let Some(rest) = s.split("ch-").nth(1) {
        let digits: String = rest.chars().take_while(|c| c.is_ascii_digit()).collect();
        if let Ok(n) = digits.parse::<u32>() {
            return Some(n);
        }
    }
    if let Some(pos) = s.find('第') {
        let digits: String = s[pos + '第'.len_utf8()..]
            .chars()
            .take_while(|c| c.is_ascii_digit())
            .collect();
        if let Ok(n) = digits.parse::<u32>() {
            return Some(n);
        }
    }
    None
}

/// 边。`hidden` = 暗线（读者视角不画）。
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct Edge {
    pub from: String,
    pub to: String,
    #[serde(default)]
    pub kind: String,
    #[serde(default)]
    pub hidden: bool,
    #[serde(flatten, default)]
    pub extra: BTreeMap<String, serde_json::Value>,
}

/// 对正典的一条影响。
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct Diff {
    #[serde(default)]
    pub target: String,
    #[serde(default)]
    pub target_ref: String,
    /// add | change | remove
    #[serde(default)]
    pub kind: String,
    #[serde(default)]
    pub before: String,
    #[serde(default)]
    pub after: String,
    #[serde(default)]
    pub reason: String,
    #[serde(default)]
    pub severity: String,
    #[serde(default)]
    pub promoted: bool,
    #[serde(default)]
    pub scheduled_chapter: Option<u32>,
    #[serde(flatten, default)]
    pub extra: BTreeMap<String, serde_json::Value>,
}

impl Diff {
    #[allow(dead_code)] // 预言书"撤销型影响"的高亮判断，UI 侧按需调用
    pub fn is_remove(&self) -> bool {
        self.kind == "remove"
    }
}

/// 一条世界线。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Line {
    pub id: String,
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub premise: String,
    #[serde(default)]
    pub kind: Kind,
    /// 从哪条线长出（α 没有）
    #[serde(default)]
    pub parent_id: String,
    /// 从父线的哪个节点长出（比 forkedChapter 精确）
    #[serde(default)]
    pub fork_node: String,
    /// 兼容字段：分叉发生在第几章
    #[serde(default)]
    pub forked_chapter: u32,
    /// 这条线**自己**固化到第几章（0 = 还没固化任何部分）。
    ///
    /// 对正典 α：等于最新成稿章——写出来的都固化了，之后都是开放的 if 空间。
    /// 对 if 线：等于「这条线自己认为已经确定的部分」；通常是它的分叉章
    /// （分叉点之前与父线共用，不必重复记），往后推多少算作者的自由。
    ///
    /// 关键：**它不是全局边界**。全局固化边界由 `fixed_through()` 算：
    /// 正典的边界 + 所有已扶正（Kind::Fixed）线的边界。
    #[serde(default)]
    pub fixed_through: u32,
    /// 这条线被扶正为正典后续时，写了哪条线（空 = 没扶正过）
    #[serde(default)]
    pub took_over: String,
    #[serde(default)]
    pub status: String,
    #[serde(default)]
    pub nodes: Vec<Node>,
    #[serde(default)]
    pub edges: Vec<Edge>,
    #[serde(default)]
    pub diffs: Vec<Diff>,
    #[serde(flatten, default)]
    pub extra: BTreeMap<String, serde_json::Value>,
}

impl Line {
    pub fn new(id: &str, name: &str, kind: Kind, parent: &str, fork_node: &str, chapter: u32) -> Self {
        Self {
            id: id.to_string(),
            name: name.to_string(),
            premise: String::new(),
            kind,
            parent_id: parent.to_string(),
            fork_node: fork_node.to_string(),
            forked_chapter: chapter,
            fixed_through: 0,
            took_over: String::new(),
            status: "推演中".into(),
            nodes: Vec::new(),
            edges: Vec::new(),
            diffs: Vec::new(),
            extra: BTreeMap::new(),
        }
    }

    /// 是不是正典基准。
    ///
    /// 两个判据都用：老数据里 α 没有 `kind` 字段，反序列化会落到
    /// `Kind::default()`（WhatIf），只按 kind 判断会把正典当成假设线。
    pub fn is_base(&self) -> bool {
        self.kind == Kind::Base || self.id == "α"
    }

    pub fn is_archived(&self) -> bool {
        self.status == "已归档"
    }

    /// 这条线的分叉章：优先 forkNode 所在章，其次 forkedChapter。
    pub fn fork_chapter(&self) -> u32 {
        if !self.fork_node.is_empty() {
            if let Some(n) = self.nodes.iter().find(|n| n.id == self.fork_node) {
                let ch = n.chapter_or_guess();
                if ch > 0 {
                    return ch;
                }
            }
        }
        self.forked_chapter
    }

    /// 这条线的最新章（含未排章节点忽略）。
    pub fn last_chapter(&self) -> u32 {
        self.nodes.iter().map(|n| n.chapter_or_guess()).max().unwrap_or(0)
    }

    fn node_mut(&mut self, id: &str) -> Option<&mut Node> {
        self.nodes.iter_mut().find(|n| n.id == id)
    }

    /// 往线上加节点。id 撞了就报错——静默覆盖会让作者以为加上了。
    pub fn add_node(&mut self, node: Node) -> Result<(), String> {
        if self.nodes.iter().any(|n| n.id == node.id) {
            return Err(format!("线{}已有节点 {}", self.id, node.id));
        }
        self.nodes.push(node);
        self.nodes.sort_by_key(|n| n.chapter_or_guess());
        Ok(())
    }

    pub fn remove_node(&mut self, id: &str) -> bool {
        let before = self.nodes.len();
        self.nodes.retain(|n| n.id != id);
        // 指向它的边一并清掉，不留悬空边
        self.edges.retain(|e| e.from != id && e.to != id);
        self.nodes.len() != before
    }

    pub fn update_node(&mut self, id: &str, patch: NodePatch) -> Result<(), String> {
        let line_id = self.id.clone();
        let node = self.node_mut(id).ok_or_else(|| format!("线{line_id}没有节点 {id}"))?;
        if let Some(v) = patch.label {
            node.label = v;
        }
        if let Some(v) = patch.chapter {
            node.chapter = v;
        }
        if let Some(v) = patch.note {
            node.note = v;
        }
        if let Some(v) = patch.state {
            node.state = v;
        }
        Ok(())
    }

    /// 这条线**实际上**固化到第几章。
    ///
    /// 取 `fixed_through` 与「最远节点章」的较大者：作者可能只标了边界没建节点
    /// （"我决定写到第 20 章了"），也可能建了节点忘了标边界。两者都算数，
    /// 不能让其中任何一种被另一种吃掉。
    pub fn fixed_boundary(&self) -> u32 {
        self.fixed_through.max(self.last_chapter())
    }

    /// 未晋升的影响条数（预言书左栏的角标）。
    pub fn pending_diffs(&self) -> usize {
        self.diffs.iter().filter(|d| !d.promoted).count()
    }
}

/// 节点局部更新。`None` = 不改这个字段。
#[derive(Debug, Clone, Default)]
pub struct NodePatch {
    pub label: Option<String>,
    pub chapter: Option<u32>,
    pub note: Option<String>,
    pub state: Option<NodeState>,
}

impl NodePatch {
    pub fn chapter(ch: u32) -> Self {
        Self { chapter: Some(ch), ..Default::default() }
    }
}

/// `worldlines.json` 的读写。
pub struct Store {
    root: PathBuf,
}

impl Store {
    pub fn new(book_root: &Path) -> Self {
        Self { root: book_root.to_path_buf() }
    }

    fn path(&self) -> PathBuf {
        self.root.join("worldlines.json")
    }

    /// 读全部线。坏 JSON 报错——世界线是作者手改过的文件，不能当空表覆盖。
    pub fn all(&self) -> Result<Vec<Line>, String> {
        let text = match std::fs::read_to_string(self.path()) {
            Ok(t) => t,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
            Err(e) => return Err(e.to_string()),
        };
        if text.trim().is_empty() {
            return Ok(Vec::new());
        }
        let mut lines: Vec<Line> =
            serde_json::from_str(&text).map_err(|e| format!("worldlines.json 解析失败：{e}"))?;
        // 老数据修复：α 在早期版本里没有 kind 字段（反序列化成 WhatIf）。
        // 这里在内存里纠正，不写盘——写盘会让"打开一本书"变成一次隐式写入，
        // 违背"只在作者动作时落库"。真正落盘等作者下一次改世界线。
        for l in &mut lines {
            if l.id == "α" && l.kind != Kind::Base {
                l.kind = Kind::Base;
            }
        }
        Ok(lines)
    }

    pub fn save(&self, lines: &[Line]) -> Result<(), String> {
        let json = serde_json::to_string_pretty(lines).map_err(|e| e.to_string())?;
        let tmp = self.path().with_extension("json.tmp");
        std::fs::write(&tmp, json).map_err(|e| e.to_string())?;
        std::fs::rename(&tmp, self.path()).map_err(|e| e.to_string())
    }

    /// 全局固化边界：剧情已经确定到第几章，之后全是开放的 if 空间。
    ///
    /// 三个来源取最大：
    /// 1. **已经写进正文的章数**（`chapters/` 里有 prose.md 的最大章号）。
    ///    这是最硬的事实——写出来了就是定了。⚠ 不能只看 worldlines.json：
    ///    实测一本写了 30 章的书，α 在 worldlines.json 里可能是 0 个节点，
    ///    只看那里会把边界算成 0，整条时间轴全被标成"开放的 if"（真的踩过）。
    /// 2. 正典自己的边界（作者可能预先标了"到第 N 章已定"）。
    /// 3. 所有已扶正（Kind::Fixed）线的边界——扶正后真正定下的内容在它们身上。
    ///
    /// 为什么要并集而不是只看正文：作者把 β 扶正后，正文还没写，
    /// 但剧情已经定了；不认 β 会让时间轴把已定的东西画成开放 if。
    pub fn fixed_boundary(&self, lines: &[Line]) -> u32 {
        let from_prose = self.written_chapters();
        lines
            .iter()
            .filter(|l| l.is_base() || l.kind == Kind::Fixed)
            .map(|l| l.fixed_boundary())
            .chain(std::iter::once(from_prose))
            .max()
            .unwrap_or(0)
    }

    /// 已经写进正文的最大章号（`chapters/ch-NNN/prose.md` 存在且非空）。
    ///
    /// 直接扫目录而不是读 project.json 的进度字段：进度是缓存的，
    /// 可能落后于磁盘；正文文件是事实。
    pub fn written_chapters(&self) -> u32 {
        let dir = self.root.join("chapters");
        let Ok(entries) = std::fs::read_dir(&dir) else {
            return 0;
        };
        let mut max_ch = 0;
        for e in entries.filter_map(|e| e.ok()) {
            let name = e.file_name().to_string_lossy().to_string();
            let Some(rest) = name.strip_prefix("ch-") else { continue };
            let Ok(n) = rest.parse::<u32>() else { continue };
            let prose = e.path().join("prose.md");
            // 空文件不算成稿：建了目录但还没写，不该把边界推过去
            if std::fs::metadata(&prose).map(|m| m.len() > 0).unwrap_or(false) {
                max_ch = max_ch.max(n);
            }
        }
        max_ch
    }

    /// 把一条 if 线扶正：它成为正典的后续，正典固化边界推到它的末尾。
    ///
    /// 这一步**不改正文**（正文只由作者落笔），改的是"哪条线代表已定剧情"：
    /// - 目标线 kind → Fixed，记 took_over 指向它自己（留痕：这一版从哪来）
    /// - 正典 α 的 fixed_through 推到目标线的边界
    /// - 原正典那一版不会被删：它仍是 Base，作者随时能看出"扶正前是什么样"
    ///
    /// 拒绝的情况：目标不存在、目标就是正典、目标已归档。
    pub fn fix_line(&self, id: &str) -> Result<Line, String> {
        let mut lines = self.all()?;
        let idx = lines
            .iter()
            .position(|l| l.id == id)
            .ok_or_else(|| format!("世界线 {id} 不存在"))?;
        if lines[idx].is_base() {
            return Err("正典本身不需要扶正".into());
        }
        if lines[idx].is_archived() {
            return Err(format!("线{id} 已归档，先恢复再扶正"));
        }
        let boundary = lines[idx].fixed_boundary();
        if boundary == 0 {
            return Err(format!("线{id} 一个节点都没有，没有可扶正的内容"));
        }
        let name = lines[idx].name.clone();
        lines[idx].kind = Kind::Fixed;
        lines[idx].status = "已定稿".into();
        if lines[idx].took_over.is_empty() {
            lines[idx].took_over = format!("{id}:{name}");
        }
        let fixed = lines[idx].clone();
        // 正典的固化边界往前推
        if let Some(base) = lines.iter_mut().find(|l| l.is_base()) {
            if base.fixed_through < boundary {
                base.fixed_through = boundary;
            }
        }
        self.save(&lines)?;
        Ok(fixed)
    }

    /// 所有引用了某画布节点的世界线节点（双向弱引用的另一半：反查）。
    ///
    /// 画布上点一个人物，要能看到"他在世界线的哪些章节里出现过"。
    /// 弱引用不保证双向都存在：世界线节点可能没填 canvas_ref（AI 推演的产物
    /// 通常没有），所以这里只返回确实填了的，不报错。
    pub fn nodes_referencing_canvas<'a>(&self, lines: &'a [Line], canvas_id: &str) -> Vec<(&'a Line, &'a Node)> {
        if canvas_id.is_empty() {
            return Vec::new();
        }
        lines
            .iter()
            .flat_map(|l| {
                l.nodes
                    .iter()
                    .filter(|n| n.canvas_ref == canvas_id)
                    .map(move |n| (l, n))
            })
            .collect()
    }

    /// 给世界线节点挂上画布引用（作者在时间轴上点"关联到画布节点"）。
    pub fn link_canvas(&self, line_id: &str, node_id: &str, canvas_id: &str) -> Result<(), String> {
        let mut lines = self.all()?;
        let line = lines
            .iter_mut()
            .find(|l| l.id == line_id)
            .ok_or_else(|| format!("世界线 {line_id} 不存在"))?;
        let node = line
            .nodes
            .iter_mut()
            .find(|n| n.id == node_id)
            .ok_or_else(|| format!("线{line_id}没有节点 {node_id}"))?;
        node.canvas_ref = canvas_id.to_string();
        self.save(&lines)
    }

    /// 给定章节，哪些线在这章「有话说」（用于时间轴按章聚合）。
    ///
    /// 三条规则：
    /// 1. 固化段（chapter ≤ 全局边界）：只有正典与已扶正线有内容，if 线不参与——
    ///    这段剧情已经定了，if 线在这里没有发言权。
    /// 2. 开放段（chapter > 边界）：正典没有内容（它还没写），所有 if 线按自己的
    ///    分叉点判断是否还在跑（分叉点早于该章）。
    /// 3. 分叉点本身那一章，父子线都算「有话说」：那是它们分开的地方。
    pub fn lines_at<'a>(&self, lines: &'a [Line], chapter: u32) -> Vec<&'a Line> {
        let boundary = self.fixed_boundary(lines);
        lines
            .iter()
            .filter(|l| {
                if l.is_archived() {
                    return false;
                }
                if chapter <= boundary {
                    // 固化段：正典 + 已扶正线
                    l.is_base() || l.kind == Kind::Fixed
                } else {
                    // 开放段：从边界之后还能继续跑的 if 线（含正典自身的规划）
                    l.is_base() || l.fork_chapter() <= chapter
                }
            })
            .collect()
    }

    /// 撤销扶正：目标线退回 if 线，正典边界回落到其余已扶正线的最大值。
    pub fn unfix_line(&self, id: &str) -> Result<(), String> {
        let mut lines = self.all()?;
        let idx = lines
            .iter()
            .position(|l| l.id == id)
            .ok_or_else(|| format!("世界线 {id} 不存在"))?;
        if lines[idx].is_base() {
            return Err("正典不能被撤销扶正".into());
        }
        lines[idx].kind = Kind::WhatIf;
        lines[idx].status = "推演中".into();
        // fixed_through 必须一起清：fixed_boundary() 取 max(fixed_through, last_chapter)，
        // 留着它等于没撤销（节点还在，边界照样算到那里）。
        lines[idx].fixed_through = 0;
        // 边界是并集算出来的，这里退回到剩下的最大值。
        //
        // 正典自己不能参与这个 max：它的 fixed_through 正是要被重算的那个值，
        // 拿它算等于"用结果推结果"（第一次撤销后永远退不回去）。
        // 正典自己的边界只来自它的节点，用 last_chapter()。
        let remaining = lines
            .iter()
            .filter(|l| l.id != id && l.kind == Kind::Fixed)
            .map(|l| l.fixed_boundary())
            .chain(
                lines
                    .iter()
                    .find(|l| l.is_base())
                    .map(|b| b.last_chapter()),
            )
            .max()
            .unwrap_or(0);
        if let Some(base) = lines.iter_mut().find(|l| l.is_base()) {
            base.fixed_through = remaining;
        }
        self.save(&lines)
    }

    pub fn find(&self, id: &str) -> Result<Option<Line>, String> {
        Ok(self.all()?.into_iter().find(|l| l.id == id))
    }

    /// 从某条线的某个节点分叉。
    ///
    /// 这是「从任意节点再分叉」的落点：父线必须存在、节点必须存在于父线，
    /// 这样 `parentId`/`forkNode` 才是可信的血缘，树布局才有依据。
    #[allow(dead_code)] // 精确到节点的分叉；当前 UI 走 fork_at_chapter
    pub fn fork(
        &self,
        parent_id: &str,
        fork_node: &str,
        new_id: &str,
        name: &str,
        premise: &str,
        kind: Kind,
    ) -> Result<Line, String> {
        let mut lines = self.all()?;
        if lines.iter().any(|l| l.id == new_id) {
            return Err(format!("世界线 {new_id} 已存在"));
        }
        let parent = lines
            .iter()
            .find(|l| l.id == parent_id)
            .ok_or_else(|| format!("父线 {parent_id} 不存在"))?;
        let node = parent
            .nodes
            .iter()
            .find(|n| n.id == fork_node)
            .ok_or_else(|| format!("线{parent_id}没有节点 {fork_node}"))?;
        let chapter = node.chapter_or_guess();
        let mut line = Line::new(new_id, name, kind, parent_id, fork_node, chapter);
        line.premise = premise.to_string();
        lines.push(line.clone());
        self.save(&lines)?;
        Ok(line)
    }

    /// 不指定节点分叉：取父线在第 `chapter` 章（或之前最近）的节点当分叉点。
    /// 作者只知道"从第 15 章分"，不该被迫先挑节点。
    pub fn fork_at_chapter(
        &self,
        parent_id: &str,
        chapter: u32,
        new_id: &str,
        name: &str,
        premise: &str,
        kind: Kind,
    ) -> Result<Line, String> {
        let lines = self.all()?;
        let parent = lines
            .iter()
            .find(|l| l.id == parent_id)
            .ok_or_else(|| format!("父线 {parent_id} 不存在"))?;
        let anchor = parent
            .nodes
            .iter()
            .filter(|n| n.chapter_or_guess() > 0 && n.chapter_or_guess() <= chapter)
            .max_by_key(|n| n.chapter_or_guess())
            .map(|n| n.id.clone())
            // 父线一个带章节点都没有：就用章节本身当锚，forkNode 留空
            .unwrap_or_default();
        let mut lines = lines;
        if lines.iter().any(|l| l.id == new_id) {
            return Err(format!("世界线 {new_id} 已存在"));
        }
        let mut line = Line::new(new_id, name, kind, parent_id, &anchor, chapter);
        line.premise = premise.to_string();
        lines.push(line.clone());
        self.save(&lines)?;
        Ok(line)
    }

    pub fn remove(&self, id: &str) -> Result<(), String> {
        let mut lines = self.all()?;
        if id == "α" {
            return Err("正典基准不能删除".into());
        }
        let len = lines.len();
        lines.retain(|l| l.id != id);
        if lines.len() == len {
            return Err(format!("世界线 {id} 不存在"));
        }
        // 子线不删，但父指针要断开——否则树布局会出现指向不存在父节点的枝
        for l in &mut lines {
            if l.parent_id == id {
                l.parent_id.clear();
                l.fork_node.clear();
            }
        }
        self.save(&lines)
    }

    pub fn add_node(&self, line_id: &str, node: Node) -> Result<(), String> {
        let mut lines = self.all()?;
        let line = lines
            .iter_mut()
            .find(|l| l.id == line_id)
            .ok_or_else(|| format!("世界线 {line_id} 不存在"))?;
        line.add_node(node)?;
        self.save(&lines)
    }

    /// 加一条边。节点必须都存在——悬空边会让画布画出一条指向虚空的线。
    pub fn add_edge(&self, line_id: &str, mut edge: Edge) -> Result<(), String> {
        let mut lines = self.all()?;
        let line = lines
            .iter_mut()
            .find(|l| l.id == line_id)
            .ok_or_else(|| format!("世界线 {line_id} 不存在"))?;
        if !line.nodes.iter().any(|n| n.id == edge.from) {
            return Err(format!("线{line_id}没有起点节点 {}", edge.from));
        }
        if !line.nodes.iter().any(|n| n.id == edge.to) {
            return Err(format!("线{line_id}没有终点节点 {}", edge.to));
        }
        // 同一条边不重复加（AI 可能重复推演同一段因果）
        if line
            .edges
            .iter()
            .any(|e| e.from == edge.from && e.to == edge.to && e.kind == edge.kind)
        {
            return Err("这条边已存在".into());
        }
        if edge.kind.is_empty() {
            edge.kind = "influence".into();
        }
        line.edges.push(edge);
        self.save(&lines)
    }

    /// 删一条边（按 from+to+kind 定位）。
    pub fn remove_edge(&self, line_id: &str, from: &str, to: &str, kind: &str) -> Result<(), String> {
        let mut lines = self.all()?;
        let line = lines
            .iter_mut()
            .find(|l| l.id == line_id)
            .ok_or_else(|| format!("世界线 {line_id} 不存在"))?;
        let before = line.edges.len();
        line.edges
            .retain(|e| !(e.from == from && e.to == to && e.kind == kind));
        if line.edges.len() == before {
            return Err("没有这条边".into());
        }
        self.save(&lines)
    }

    /// 改一条边的暗线标记（作者最常见的边操作：这条因果读者还不知道）。
    pub fn set_edge_hidden(
        &self,
        line_id: &str,
        from: &str,
        to: &str,
        kind: &str,
        hidden: bool,
    ) -> Result<(), String> {
        let mut lines = self.all()?;
        let line = lines
            .iter_mut()
            .find(|l| l.id == line_id)
            .ok_or_else(|| format!("世界线 {line_id} 不存在"))?;
        let edge = line
            .edges
            .iter_mut()
            .find(|e| e.from == from && e.to == to && e.kind == kind)
            .ok_or("没有这条边")?;
        edge.hidden = hidden;
        self.save(&lines)
    }

    /// 改线的名字与假设（作者手改；AI 推演写入的 premise 也走这里纠正）。
    pub fn update_line_meta(
        &self,
        line_id: &str,
        name: Option<String>,
        premise: Option<String>,
    ) -> Result<(), String> {
        let mut lines = self.all()?;
        let line = lines
            .iter_mut()
            .find(|l| l.id == line_id)
            .ok_or_else(|| format!("世界线 {line_id} 不存在"))?;
        if let Some(n) = name {
            if !n.trim().is_empty() {
                line.name = n.trim().to_string();
            }
        }
        if let Some(p) = premise {
            line.premise = p;
        }
        self.save(&lines)
    }

    pub fn remove_node(&self, line_id: &str, node_id: &str) -> Result<(), String> {
        let mut lines = self.all()?;
        let line = lines
            .iter_mut()
            .find(|l| l.id == line_id)
            .ok_or_else(|| format!("世界线 {line_id} 不存在"))?;
        if !line.remove_node(node_id) {
            return Err(format!("线{line_id}没有节点 {node_id}"));
        }
        // 别的线若从这个节点分叉，血缘断开
        for l in &mut lines {
            if l.parent_id == line_id && l.fork_node == node_id {
                l.fork_node.clear();
            }
        }
        self.save(&lines)
    }

    pub fn update_node(&self, line_id: &str, node_id: &str, patch: NodePatch) -> Result<(), String> {
        let mut lines = self.all()?;
        let line = lines
            .iter_mut()
            .find(|l| l.id == line_id)
            .ok_or_else(|| format!("世界线 {line_id} 不存在"))?;
        line.update_node(node_id, patch)?;
        self.save(&lines)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn book(tag: &str) -> PathBuf {
        let p = std::env::temp_dir().join(format!("zhibi-wl-{tag}"));
        let _ = std::fs::remove_dir_all(&p);
        std::fs::create_dir_all(&p).unwrap();
        p
    }

    fn seed(root: &Path) -> Store {
        let s = Store::new(root);
        let mut alpha = Line::new("α", "正典", Kind::Base, "", "", 0);
        alpha.status = "基准".into();
        let mut n1 = Node::new("n1", "event", "青锋出鞘");
        n1.chapter = 12;
        alpha.nodes.push(n1);
        let mut n2 = Node::new("n2", "event", "灯会初遇");
        n2.chapter = 16;
        alpha.nodes.push(n2);
        s.save(&[alpha]).unwrap();
        s
    }

    #[test]
    fn forks_from_a_specific_node_and_keeps_lineage() {
        let root = book("fork");
        let s = seed(&root);
        let line = s
            .fork("α", "n2", "β", "师兄没死", "如果师兄在第16章没死", Kind::WhatIf)
            .unwrap();
        assert_eq!(line.parent_id, "α");
        assert_eq!(line.fork_node, "n2");
        assert_eq!(line.forked_chapter, 16, "分叉章应取自节点所在章");
        // 存下来再读，血缘还在
        let back = s.find("β").unwrap().unwrap();
        assert_eq!(back.parent_id, "α");
        assert_eq!(back.fork_chapter(), 16);
    }

    #[test]
    fn fork_from_a_node_that_later_branches_again() {
        let root = book("fork2");
        let s = seed(&root);
        s.fork("α", "n2", "β", "β", "", Kind::WhatIf).unwrap();
        // β 上长一个节点，再从它分叉出 γ —— 树可以多层
        let mut bn = Node::new("β1", "event", "师兄现身");
        bn.chapter = 18;
        s.add_node("β", bn).unwrap();
        let g = s.fork("β", "β1", "γ", "γ", "", Kind::WhatIf).unwrap();
        assert_eq!(g.parent_id, "β");
        assert_eq!(g.forked_chapter, 18);
    }

    #[test]
    fn fork_at_chapter_picks_the_nearest_earlier_node() {
        let root = book("forkch");
        let s = seed(&root);
        // 第 14 章：最近的前序节点是第 12 章那个
        let line = s
            .fork_at_chapter("α", 14, "δ", "δ", "", Kind::Forecast)
            .unwrap();
        assert_eq!(line.fork_node, "n1");
        assert_eq!(line.forked_chapter, 14, "分叉章仍是作者说的 14 章");
    }

    #[test]
    fn rejects_forking_from_a_missing_node() {
        let root = book("forkbad");
        let s = seed(&root);
        assert!(s.fork("α", "nope", "β", "β", "", Kind::WhatIf).is_err());
        assert!(s.fork("nope", "n1", "β", "β", "", Kind::WhatIf).is_err());
        // 同名线不能重复
        s.fork("α", "n1", "β", "β", "", Kind::WhatIf).unwrap();
        assert!(s.fork("α", "n1", "β", "β", "", Kind::WhatIf).is_err());
    }

    #[test]
    fn removing_a_line_detaches_children_instead_of_orphaning_them() {
        let root = book("remove");
        let s = seed(&root);
        s.fork("α", "n2", "β", "β", "", Kind::WhatIf).unwrap();
        s.fork("β", "n2", "γ", "γ", "", Kind::WhatIf).ok();
        s.remove("β").unwrap();
        let all = s.all().unwrap();
        assert!(!all.iter().any(|l| l.id == "β"));
        assert!(s.remove("α").is_err(), "正典不能删");
    }

    #[test]
    fn removing_a_node_clears_edges_and_dependent_fork_points() {
        let root = book("rmnode");
        let s = seed(&root);
        s.fork("α", "n2", "β", "β", "", Kind::WhatIf).unwrap();
        s.remove_node("α", "n2").unwrap();
        let beta = s.find("β").unwrap().unwrap();
        assert!(beta.fork_node.is_empty(), "父节点没了，指向它的血缘要断");
        assert_eq!(beta.forked_chapter, 16, "章号仍保留，作者仍能看出从哪分");
    }

    #[test]
    fn fixed_boundary_is_the_union_of_base_and_fixed_lines() {
        let root = book("boundary");
        let s = seed(&root);
        // 正典到第 16 章
        assert_eq!(s.fixed_boundary(&s.all().unwrap()), 16);

        // 扶正 β（到第 18 章）→ 全局边界推到 18
        s.fork("α", "n2", "β", "β", "", Kind::WhatIf).unwrap();
        let mut bn = Node::new("b1", "event", "师兄现身");
        bn.chapter = 18;
        s.add_node("β", bn).unwrap();
        assert_eq!(s.fixed_boundary(&s.all().unwrap()), 16, "还没扶正，边界不动");

        s.fix_line("β").unwrap();
        let lines = s.all().unwrap();
        assert_eq!(s.fixed_boundary(&lines), 18, "扶正后边界应推到 β 的末尾");
        assert_eq!(s.find("β").unwrap().unwrap().kind, Kind::Fixed);
    }

    #[test]
    fn fixing_a_line_keeps_the_original_base_for_comparison() {
        let root = book("fixkeep");
        let s = seed(&root);
        s.fork("α", "n2", "β", "β", "", Kind::WhatIf).unwrap();
        let mut bn = Node::new("b1", "event", "师兄现身");
        bn.chapter = 20;
        s.add_node("β", bn).unwrap();
        s.fix_line("β").unwrap();

        let lines = s.all().unwrap();
        // 原正典不被删也不被改：作者要能看出"扶正前是什么样"
        assert!(lines.iter().any(|l| l.is_base()));
        assert_eq!(lines.iter().find(|l| l.is_base()).unwrap().nodes.len(), 2);
        // 但边界推过去了
        assert_eq!(lines.iter().find(|l| l.is_base()).unwrap().fixed_through, 20);
        // 扶正的那条留痕
        let beta = lines.iter().find(|l| l.id == "β").unwrap();
        assert!(beta.took_over.starts_with("β:"), "要记下这一版从哪来");
    }

    #[test]
    fn unfixing_rolls_the_boundary_back() {
        let root = book("unfix");
        let s = seed(&root);
        s.fork("α", "n2", "β", "β", "", Kind::WhatIf).unwrap();
        let mut bn = Node::new("b1", "event", "x");
        bn.chapter = 20;
        s.add_node("β", bn).unwrap();
        s.fix_line("β").unwrap();
        assert_eq!(s.fixed_boundary(&s.all().unwrap()), 20);

        s.unfix_line("β").unwrap();
        let lines = s.all().unwrap();
        assert_eq!(s.fixed_boundary(&lines), 16, "撤销后回落到正典自己的边界");
        assert_eq!(lines.iter().find(|l| l.id == "β").unwrap().kind, Kind::WhatIf);
    }

    #[test]
    fn fixing_rejects_base_archived_and_empty_lines() {
        let root = book("fixbad");
        let s = seed(&root);
        assert!(s.fix_line("α").is_err(), "正典不需要扶正");
        assert!(s.fix_line("nope").is_err());
        // 空线没有可扶正的内容
        s.fork("α", "n2", "β", "β", "", Kind::WhatIf).unwrap();
        assert!(s.fix_line("β").is_err(), "一个节点都没有的线不该能扶正");
        // 归档线要先恢复
        let mut bn = Node::new("b1", "event", "x");
        bn.chapter = 18;
        s.add_node("β", bn).unwrap();
        let mut lines = s.all().unwrap();
        lines.iter_mut().find(|l| l.id == "β").unwrap().status = "已归档".into();
        s.save(&lines).unwrap();
        assert!(s.fix_line("β").is_err(), "归档线不能直接扶正");
    }

    #[test]
    fn fixed_segment_only_shows_base_and_fixed_lines() {
        let root = book("linesat");
        let s = seed(&root);
        s.fork("α", "n2", "β", "β", "", Kind::WhatIf).unwrap();
        let mut bn = Node::new("b1", "event", "x");
        bn.chapter = 20;
        s.add_node("β", bn).unwrap();
        let lines = s.all().unwrap();

        // 第 10 章（固化段内）：β 还没分叉，不该出现
        let at10: Vec<&str> = s.lines_at(&lines, 10).iter().map(|l| l.id.as_str()).collect();
        assert_eq!(at10, vec!["α"], "固化段只有正典，if 线无权参与");

        // 第 18 章 > 边界 16，属开放段：β 已分叉，应该出现
        let at18: Vec<&str> = s.lines_at(&lines, 18).iter().map(|l| l.id.as_str()).collect();
        assert!(at18.contains(&"β"), "开放段里已分叉的 if 线要有位置");

        // 扶正 β 后边界推到 20：第 18 章落进固化段，
        // 此时 β 出现是因为它成了 Fixed（已定剧情），不是因为它"已分叉"
        s.fix_line("β").unwrap();
        let lines = s.all().unwrap();
        assert_eq!(s.fixed_boundary(&lines), 20);
        let at18b: Vec<&str> = s.lines_at(&lines, 18).iter().map(|l| l.id.as_str()).collect();
        assert!(at18b.contains(&"β"), "扶正后它就是已定剧情的一部分");
    }

    #[test]
    fn open_segment_shows_if_lines_that_have_already_forked() {
        let root = book("open");
        let s = seed(&root);
        s.fork("α", "n2", "β", "β", "", Kind::WhatIf).unwrap(); // 分叉于 16
        let lines = s.all().unwrap();
        // 边界是 16。第 20 章是开放段：β 已经分叉了，应该在
        let at20: Vec<&str> = s.lines_at(&lines, 20).iter().map(|l| l.id.as_str()).collect();
        assert!(at20.contains(&"β"), "开放段里已分叉的 if 线要有位置");
        assert!(at20.contains(&"α"), "正典在开放段仍有规划位");
    }

    #[test]
    fn archived_lines_never_appear_on_the_timeline() {
        let root = book("arch");
        let s = seed(&root);
        s.fork("α", "n2", "β", "β", "", Kind::WhatIf).unwrap();
        let mut lines = s.all().unwrap();
        lines.iter_mut().find(|l| l.id == "β").unwrap().status = "已归档".into();
        s.save(&lines).unwrap();
        let lines = s.all().unwrap();
        let at20: Vec<&str> = s.lines_at(&lines, 20).iter().map(|l| l.id.as_str()).collect();
        assert!(!at20.contains(&"β"), "归档线不该占时间轴");
    }

    #[test]
    fn fixed_boundary_counts_written_prose() {
        let root = book("prose-boundary");
        // 写了两章正文（第三章只有空目录 = 没写完）
        for (n, body) in [(1u32, "第一章正文"), (2, "第二章正文"), (3, "")] {
            let d = root.join("chapters").join(format!("ch-{n:03}"));
            std::fs::create_dir_all(&d).unwrap();
            std::fs::write(d.join("prose.md"), body).unwrap();
        }
        let s = Store::new(&root);
        let alpha = {
            let mut a = Line::new("α", "正典", Kind::Base, "", "", 0);
            a.status = "基准".into();
            a
        };
        s.save(&[alpha]).unwrap();

        // 正文写到第 2 章 → 固化边界必须是 2，不能因为 α 没有节点就是 0
        assert_eq!(s.written_chapters(), 2, "空 prose 不算成稿");
        assert_eq!(s.fixed_boundary(&s.all().unwrap()), 2);
    }

    #[test]
    fn legacy_alpha_without_kind_is_treated_as_base() {
        let root = book("legacy-alpha");
        // 老数据：α 没有 kind 字段
        let raw = serde_json::json!([{
            "id": "α", "name": "正典", "status": "基准",
            "nodes": [], "edges": [], "diffs": []
        }]);
        std::fs::write(root.join("worldlines.json"), raw.to_string()).unwrap();
        let s = Store::new(&root);
        let lines = s.all().unwrap();
        assert_eq!(lines[0].kind, Kind::Base, "缺 kind 的 α 必须当正典");
        assert!(lines[0].is_base());
        // 不能因为 kind 缺失就被当成可扶正的 if 线
        assert!(s.fix_line("α").is_err());
    }

    #[test]
    fn edges_need_both_endpoints_to_exist() {
        let root = book("edge");
        let s = seed(&root);
        let ok = Edge { from: "n1".into(), to: "n2".into(), kind: "influence".into(), ..Default::default() };
        s.add_edge("α", ok).unwrap();
        assert_eq!(s.all().unwrap()[0].edges.len(), 1);

        // 端点不存在 → 拒绝（悬空边会在画布上指向虚空）
        let bad = Edge { from: "n1".into(), to: "nope".into(), kind: "influence".into(), ..Default::default() };
        assert!(s.add_edge("α", bad).is_err());
        // 重复边 → 拒绝（AI 重复推演同一段因果）
        let dup = Edge { from: "n1".into(), to: "n2".into(), kind: "influence".into(), ..Default::default() };
        assert!(s.add_edge("α", dup).is_err());
    }

    #[test]
    fn edge_hidden_toggle_and_removal() {
        let root = book("edgehide");
        let s = seed(&root);
        s.add_edge("α", Edge { from: "n1".into(), to: "n2".into(), kind: "plant".into(), ..Default::default() })
            .unwrap();
        // 默认明线
        assert!(!s.all().unwrap()[0].edges[0].hidden);
        s.set_edge_hidden("α", "n1", "n2", "plant", true).unwrap();
        assert!(s.all().unwrap()[0].edges[0].hidden, "暗线标记要落盘");

        s.remove_edge("α", "n1", "n2", "plant").unwrap();
        assert!(s.all().unwrap()[0].edges.is_empty());
        assert!(s.remove_edge("α", "n1", "n2", "plant").is_err(), "删不存在的边要报错");
    }

    #[test]
    fn updating_line_meta_keeps_blank_name_from_wiping_it() {
        let root = book("linemeta");
        let s = seed(&root);
        s.fork("α", "n2", "β", "原名", "", Kind::WhatIf).unwrap();
        s.update_line_meta("β", Some("新名".into()), Some("如果师兄没死".into()))
            .unwrap();
        let l = s.find("β").unwrap().unwrap();
        assert_eq!(l.name, "新名");
        assert_eq!(l.premise, "如果师兄没死");

        // 空名字不该把原名抹掉（表单清空是误操作，不是意图）
        s.update_line_meta("β", Some("   ".into()), None).unwrap();
        assert_eq!(s.find("β").unwrap().unwrap().name, "新名");
    }

    #[test]
    fn subtree_keeps_only_descendants() {
        let root = book("subtree");
        let s = seed(&root);
        s.fork("α", "n2", "β", "β", "", Kind::WhatIf).unwrap();
        let mut bn = Node::new("b1", "event", "x");
        bn.chapter = 18;
        s.add_node("β", bn).unwrap();
        s.fork("β", "b1", "γ", "γ", "", Kind::WhatIf).unwrap();
        s.fork("α", "n1", "δ", "δ", "", Kind::WhatIf).unwrap();
        let lines = s.all().unwrap();

        // 钻取 β：只剩 β 与 γ（δ 是 α 的另一个孩子，不该进来）
        let sub = crate::worldline::layout::subtree(&lines, "β");
        let ids: Vec<&str> = sub.iter().map(|l| l.id.as_str()).collect();
        assert_eq!(ids.len(), 2);
        assert!(ids.contains(&"β") && ids.contains(&"γ"));
        assert!(!ids.contains(&"δ"));
        // 钻取根要变成新树根，槽位才会从 0 重排
        assert!(sub.iter().find(|l| l.id == "β").unwrap().parent_id.is_empty());
    }

    #[test]
    fn canvas_link_is_bidirectional_and_weak() {
        let root = book("canvaslink");
        let s = seed(&root);
        // 世界线节点挂到画布节点 C01
        s.link_canvas("α", "n1", "C01").unwrap();
        let lines = s.all().unwrap();
        // 正向：世界线节点记住了画布 id
        assert_eq!(
            lines.iter().find(|l| l.is_base()).unwrap().nodes[0].canvas_ref,
            "C01"
        );
        // 反向：从画布 id 能查回所有引用它的世界线节点
        let hits = s.nodes_referencing_canvas(&lines, "C01");
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].0.id, "α");
        assert_eq!(hits[0].1.id, "n1");

        // 弱引用的意思：画布那边删了，这里不崩、不清理，只是查不到
        let hits2 = s.nodes_referencing_canvas(&lines, "已删除的画布节点");
        assert!(hits2.is_empty(), "查不到就返回空，不报错");

        // 解绑
        s.link_canvas("α", "n1", "").unwrap();
        assert!(s
            .nodes_referencing_canvas(&s.all().unwrap(), "C01")
            .is_empty());
    }

    #[test]
    fn linking_rejects_missing_line_or_node() {
        let root = book("canvasbad");
        let s = seed(&root);
        assert!(s.link_canvas("nope", "n1", "C01").is_err());
        assert!(s.link_canvas("α", "nope", "C01").is_err());
    }

    #[test]
    fn unknown_fields_survive_a_rewrite() {
        let root = book("extra");
        let s = seed(&root);
        // 模拟别处（AI/老版本）写进来的未知字段
        let raw = serde_json::json!([{
            "id": "α", "name": "正典", "kind": "base", "status": "基准",
            "derived": { "characters": ["沈砚"], "canon": [] },
            "futureField": 42,
            "nodes": [{ "id": "n1", "kind": "event", "label": "x", "chapter": 3, "weight": 0.7 }],
            "edges": [], "diffs": []
        }]);
        std::fs::write(root.join("worldlines.json"), raw.to_string()).unwrap();
        let mut lines = s.all().unwrap();
        assert_eq!(lines[0].extra.get("futureField"), Some(&serde_json::json!(42)));
        assert_eq!(lines[0].nodes[0].extra.get("weight"), Some(&serde_json::json!(0.7)));
        // 改一下再存，未知字段不能丢
        lines[0].nodes[0].chapter = 9;
        s.save(&lines).unwrap();
        let back = s.all().unwrap();
        assert_eq!(back[0].extra.get("futureField"), Some(&serde_json::json!(42)));
        assert_eq!(back[0].nodes[0].extra.get("weight"), Some(&serde_json::json!(0.7)));
        assert_eq!(back[0].nodes[0].chapter, 9);
    }

    #[test]
    fn node_chapter_falls_back_to_reference_or_label() {
        let mut n = Node::new("n", "event", "第7章那场雨");
        assert_eq!(n.chapter_or_guess(), 7);
        let mut n2 = Node::new("n2", "clue", "刻痕");
        n2.reference = "ch-023".into();
        assert_eq!(n2.chapter_or_guess(), 23);
        // 显式 chapter 优先
        n.chapter = 99;
        assert_eq!(n.chapter_or_guess(), 99);
    }

    #[test]
    fn corrupt_file_is_reported_not_silently_emptied() {
        let root = book("corrupt");
        std::fs::write(root.join("worldlines.json"), "{ 坏").unwrap();
        let s = Store::new(&root);
        assert!(s.all().is_err(), "坏文件必须报错，不能返回空表");
    }

    #[test]
    fn adding_a_duplicate_node_id_is_rejected() {
        let root = book("dupnode");
        let s = seed(&root);
        let dup = Node::new("n1", "event", "又一个 n1");
        assert!(s.add_node("α", dup).is_err());
    }
}
