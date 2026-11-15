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
/// **只有一条线是"已固化"的：正典 `Base`（α）。** 它代表已经写进正文的剧情，
/// 由正文文件决定，不由任何预演动作决定。
///
/// 其余全部是**预演**（`Forecast` 往未来推、`WhatIf` 改过去看会怎样），
/// 它们只存在于预言书，是给作者将来写番外留的素材，**永远不会被扶正成正典**。
/// 旧版有一个 `Fixed`（已扶正）形态，那条路已经整条拆掉：它让"推演一下"
/// 就能把时间轴顶上第二条实线，和作者定的规则直接冲突。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Kind {
    /// 正典基准（唯一，id 固定为 α）：已经写进正文的那条线
    Base,
    /// 预测线：从固化点往未来推演（预演剧情，不进正典）
    Forecast,
    /// 假设线：what-if（预演改动，不进正典）
    #[default]
    WhatIf,
}

impl Kind {
    pub fn key(self) -> &'static str {
        match self {
            Kind::Base => "base",
            Kind::Forecast => "forecast",
            Kind::WhatIf => "what-if",
        }
    }

    pub fn from_key(s: &str) -> Self {
        match s {
            "base" => Kind::Base,
            "forecast" => Kind::Forecast,
            // 老数据里可能还有 "fixed"（旧版的"已扶正"）。它现在只能退成
            // 一条预演线——绝不能再被当成"已定剧情"的一员。
            _ => Kind::WhatIf,
        }
    }

    #[allow(dead_code)] // 界面按需取词；词表放在类型上，别散到各页
    pub fn label(self) -> &'static str {
        match self {
            Kind::Base => "正典",
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
    /// 旧版的「这条线自己固化到第几章」。**已不参与任何判断，只为读旧数据保留。**
    ///
    /// 现在的规则：固化边界只来自正文（`Store::fixed_boundary`）。
    /// 这条字段留着只是为了不改老文件的形状——反序列化进来、原样写回去，
    /// 从不算进时间轴的实段，也不改变"哪些线已定"。
    #[serde(default)]
    pub fixed_through: u32,
    /// 旧版的「被扶正时顶替了哪条线」。**已废弃，只为读旧数据保留。**
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

    /// 这条预演"动得有多狠"：所有影响里最严重的一档。
    ///
    /// 0 = 没写影响（或全是无关紧要的改动）、1 = low、2 = medium、3 = high。
    /// 作者挑未来时最先要比的就是这个：动到设定层（high）和加一场戏（medium）
    /// 是完全不同的代价，界面上必须能一眼分开。
    pub fn max_severity(&self) -> u8 {
        self.diffs
            .iter()
            .map(|d| match d.severity.as_str() {
                "high" => 3u8,
                "medium" => 2,
                "low" => 1,
                _ => 0,
            })
            .max()
            .unwrap_or(0)
    }

    /// 牵扯到的正典条目数（人物 + 线索 + 设定）。
    ///
    /// 来自推演结果里的 `derived` 三个清单。它回答"选这条会波及多大一片"，
    /// 和 `max_severity`（动得多狠）是两个维度：可以只改一处设定但波及全书。
    pub fn touched(&self) -> usize {
        let Some(d) = self.extra.get("derived").and_then(|v| v.as_object()) else {
            return 0;
        };
        ["characters", "clues", "canon"]
            .iter()
            .filter_map(|k| d.get(*k).and_then(|v| v.as_array()).map(|a| a.len()))
            .sum()
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
    /// **只有一个来源：已经写进正文的章数**（作者在书设置里手动标注的
    /// 「已定剧情到第 N 章」也应折算进来，见 `confirmed_through`）。
    ///
    /// 这条规则是被反复纠正过的，写死在这里免得再走回头路：
    /// - 已经固化的剧情**只会有一条线**。它由正文决定，不由哪条世界线决定。
    /// - if 线永远不会被扶正成正典。无论它推演到第几章、写得多么合作者心意，
    ///   都只是**预演**；作者真要采纳，就去写正文——写了正文，边界自然过去。
    /// - 所以这里**不看** `lines` 里任何一条线的 `fixedThrough`：
    ///   早先版本让已扶正线的边界顶高全局边界，结果是"推演一下，剧情就算定了"，
    ///   时间轴上出现了第二条实线，正是作者最反对的那种错。
    ///
    /// 参数 `lines` 保留是为了调用方签名稳定与未来可能的展示用途；
    /// 它**不参与**边界计算。
    pub fn fixed_boundary(&self, _lines: &[Line]) -> u32 {
        self.written_chapters().max(self.confirmed_through())
    }

    /// 作者在书设置里手动标注的「已定剧情到第 N 章」（`worldline.json` 的
    /// `confirmedThrough`）。用于「先定稿章目、正文还没写」的阶段。
    ///
    /// 它与 if 线的区别是**来源**：这是作者对**那唯一一条线**的声明，
    /// 不是某条预演线的成绩。读不到就当 0。
    pub fn confirmed_through(&self) -> u32 {
        let p = self.root.join("worldline.json");
        std::fs::read_to_string(p)
            .ok()
            .and_then(|t| serde_json::from_str::<serde_json::Value>(&t).ok())
            .and_then(|v| {
                v.get("confirmedThrough")
                    .and_then(|x| x.as_u64())
                    .or_else(|| v.get("confirmedThrough").and_then(|x| x.as_str()).and_then(|s| s.parse().ok()))
            })
            .unwrap_or(0) as u32
    }

    /// 写「已定剧情到第 N 章」。0 = 清掉，回到纯按正文判断。
    pub fn set_confirmed_through(&self, chapter: u32) -> Result<(), String> {
        let p = self.root.join("worldline.json");
        let mut v: serde_json::Value = std::fs::read_to_string(&p)
            .ok()
            .and_then(|t| serde_json::from_str(&t).ok())
            .unwrap_or_else(|| serde_json::json!({}));
        if !v.is_object() {
            v = serde_json::json!({});
        }
        if let Some(obj) = v.as_object_mut() {
            obj.insert("confirmedThrough".into(), serde_json::json!(chapter));
        }
        let json = serde_json::to_string_pretty(&v).map_err(|e| e.to_string())?;
        let tmp = p.with_extension("json.tmp");
        std::fs::write(&tmp, json).map_err(|e| e.to_string())?;
        std::fs::rename(&tmp, &p).map_err(|e| e.to_string())
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

    /// 「扶正 if 线」这个动作**已经不存在**。
    ///
    /// 保留这个函数只为让旧调用点编译失败时有个明确的落点（已全部摘除），
    /// 它永远返回错误——即使有人（包括 AI、插件、脚本）调进来，也扶不正。
    ///
    /// 规则：if 线是预演，不是候选正典。要让某个预演成真，只有一条路：
    /// **作者把这段剧情写进正文**。正文写到哪，固化边界就到哪（见 `fixed_boundary`）。
    #[allow(dead_code)] // 没有调用点是**故意的**：保留它只为"调进来也扶不正"这道保险
    pub fn fix_line(&self, _id: &str) -> Result<Line, String> {
        Err("if 线不能被扶正：世界线只做预演，固化剧情由正文决定。\
             要让某个预演成真，请把这段剧情写进正文。"
            .into())
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
    /// 1. 固化段（chapter ≤ 全局边界）：**只有正典**有内容，if 线不参与——
    ///    这段剧情已经写进正文了，预演在这里没有发言权（预演只在固化点之后）。
    /// 2. 开放段（chapter > 边界）：正典没有内容（它还没写），所有 if 线按自己的
    ///    分叉点判断是否还在跑（分叉点早于该章）。
    /// 3. 分叉点本身那一章，父子线都算「有话说」：那是它们分开的地方。
    #[allow(dead_code)] // 时间轴已改为按线渲染；这张"某章有谁在跑"的查询留给后续的章节联动
    pub fn lines_at<'a>(&self, lines: &'a [Line], chapter: u32) -> Vec<&'a Line> {
        let boundary = self.fixed_boundary(lines);
        lines
            .iter()
            .filter(|l| {
                if l.is_archived() {
                    return false;
                }
                if chapter <= boundary {
                    // 固化段：只有正典（if 线永不固化）
                    l.is_base()
                } else {
                    // 开放段：从边界之后还能继续跑的 if 线（含正典自身的规划）
                    l.is_base() || l.fork_chapter() <= chapter
                }
            })
            .collect()
    }

    /// 撤销扶正：已无此动作（见 `fix_line`）。永远返回错误。
    #[allow(dead_code)] // 同上：与 fix_line 成对，只为保险
    pub fn unfix_line(&self, _id: &str) -> Result<(), String> {
        Err("没有「扶正」就没有「撤销扶正」——固化剧情只随正文变化。".into())
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

/// 正文写到第 `chapter` 章之后，把所有「已采纳」且落在该章及之前的节点
/// 置为「已写」。
///
/// 为什么由正文推动：`Adopted` 是作者承认的"正典后续"，正文追上它才是既成事实。
/// 之前没有任何写入方，`Written` 永远不会出现——时间轴上"已写/已采纳/计划"
/// 三态里最关键的一态是死的。这个函数是那条缺失的写入路径。
///
/// 幂等；没有可改的节点时不写盘。返回改了几个节点。
pub fn mark_adopted_written(book_root: &Path, chapter: u32) -> Result<usize, String> {
    if chapter == 0 {
        return Ok(0);
    }
    let store = Store::new(book_root);
    let mut lines = store.all()?;
    let mut n = 0usize;
    for line in &mut lines {
        for node in &mut line.nodes {
            if node.state != NodeState::Adopted {
                continue;
            }
            let ch = node.chapter_or_guess();
            if ch > 0 && ch <= chapter {
                node.state = NodeState::Written;
                n += 1;
            }
        }
    }
    if n > 0 {
        store.save(&lines)?;
    }
    Ok(n)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 挑未来时要比的两个量：动得多狠、波及多广。
    /// 它们是纯函数，直接单测——界面上那套"粗细/端点"编码全靠这两个数。
    #[test]
    fn severity_and_touch_describe_how_heavy_a_line_is() {
        let mut l = Line::new("β", "断刀引路", Kind::Forecast, "α", "", 7);
        // 没有影响：0 狠、0 波及
        assert_eq!(l.max_severity(), 0);
        assert_eq!(l.touched(), 0);

        l.diffs = vec![
            Diff {
                severity: "low".into(),
                ..Default::default()
            },
            Diff {
                severity: "medium".into(),
                ..Default::default()
            },
        ];
        // 取最狠的一档，不是平均、也不是求和
        assert_eq!(l.max_severity(), 2);

        l.diffs.push(Diff {
            severity: "high".into(),
            ..Default::default()
        });
        assert_eq!(l.max_severity(), 3);

        // 未知档位当作 0，不让脏数据把线画粗
        l.diffs.push(Diff {
            severity: "critical".into(),
            ..Default::default()
        });
        assert_eq!(l.max_severity(), 3);
    }

    #[test]
    fn touched_counts_all_three_derived_lists() {
        let mut l = Line::new("β", "断刀引路", Kind::Forecast, "α", "", 7);
        l.extra.insert(
            "derived".into(),
            serde_json::json!({
                "characters": ["遗孤", "老周"],
                "clues": ["F02"],
                "canon": ["界字·第二层"],
            }),
        );
        assert_eq!(l.touched(), 4);
        // 缺清单 / 字段类型不对时不能 panic，直接算 0
        l.extra
            .insert("derived".into(), serde_json::json!({ "characters": "不是数组" }));
        assert_eq!(l.touched(), 0);
        l.extra.remove("derived");
        assert_eq!(l.touched(), 0);
    }

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

    /// 固化边界只由正文决定：推演一条线推得再远，边界也不动。
    #[test]
    fn boundary_comes_from_prose_only() {
        let root = book("boundary");
        let s = seed(&root);
        // seed 里 α 的节点落到第 16 章，但那不是正文——正文为空时边界是 0。
        assert_eq!(s.fixed_boundary(&s.all().unwrap()), 0, "没写正文就没有已定剧情");

        // 正文写到第 9 章：边界就是 9，跟 worldlines.json 里的节点无关
        let ch = root.join("chapters").join("ch-009");
        std::fs::create_dir_all(&ch).unwrap();
        std::fs::write(ch.join("prose.md"), "正文").unwrap();
        assert_eq!(s.fixed_boundary(&s.all().unwrap()), 9, "正文写到哪，边界就到哪");

        // 开一条推到第 18 章的预测线：边界不动
        s.fork("α", "n2", "β", "β", "", Kind::Forecast).unwrap();
        let mut bn = Node::new("b1", "event", "师兄现身");
        bn.chapter = 18;
        s.add_node("β", bn).unwrap();
        let lines = s.all().unwrap();
        assert_eq!(
            s.fixed_boundary(&lines),
            9,
            "预演推演到第 18 章也不能把固化边界顶上去"
        );
    }

    /// 作者手动标注的「已定剧情到第 N 章」也算数——但那是作者对唯一那条线的声明，
    /// 不是某条预演线的成绩。
    #[test]
    fn author_can_declare_confirmed_through_manually() {
        let root = book("confirmed");
        let s = seed(&root);
        assert_eq!(s.fixed_boundary(&[]), 0);
        s.set_confirmed_through(12).unwrap();
        assert_eq!(s.confirmed_through(), 12);
        assert_eq!(s.fixed_boundary(&[]), 12, "作者声明也要认");
        s.set_confirmed_through(0).unwrap();
        assert_eq!(s.fixed_boundary(&[]), 0, "清掉就回到纯按正文判断");
    }

    /// if 线永远扶不正——这是硬规则，写在函数体里而不是靠 UI 不放按钮。
    #[test]
    fn an_if_line_can_never_be_promoted() {
        let root = book("nofix");
        let s = seed(&root);
        s.fork("α", "n2", "β", "β", "", Kind::WhatIf).unwrap();
        let mut bn = Node::new("b1", "event", "师兄现身");
        bn.chapter = 20;
        s.add_node("β", bn).unwrap();

        assert!(s.fix_line("β").is_err(), "if 线不能被扶正");
        assert!(s.unfix_line("β").is_err(), "没有扶正就没有撤销扶正");
        // 线原样留着，仍是预演
        let beta = s.find("β").unwrap().unwrap();
        assert_eq!(beta.kind, Kind::WhatIf);
        assert_ne!(beta.status, "已定稿");
    }

    /// 旧数据里 kind="fixed" 的线，读进来必须降级成预演——
    /// 绝不能再被当成"已定剧情"的一员。
    #[test]
    fn legacy_fixed_lines_read_back_as_previews() {
        let root = book("legacy");
        let s = seed(&root);
        s.fork("α", "n2", "β", "β", "", Kind::WhatIf).unwrap();
        // 手改文件，模拟旧版写下的 "fixed"
        let mut lines = s.all().unwrap();
        {
            let beta = lines.iter_mut().find(|l| l.id == "β").unwrap();
            beta.extra.insert("kind".into(), serde_json::json!("fixed"));
            beta.fixed_through = 22;
        }
        let text = serde_json::to_string_pretty(
            &lines
                .iter()
                .map(|l| serde_json::to_value(l).unwrap())
                .collect::<Vec<_>>(),
        )
        .unwrap()
        .replace("\"kind\": \"what-if\"", "\"kind\": \"fixed\"");
        std::fs::write(root.join("worldlines.json"), text).unwrap();

        let back = s.all().unwrap();
        let beta = back.iter().find(|l| l.id == "β").unwrap();
        assert_ne!(beta.kind.key(), "fixed", "旧 fixed 必须降级");
        // 旧的 fixedThrough 也不再顶高边界
        assert_eq!(s.fixed_boundary(&back), 0, "旧扶正数据不能继续影响边界");
    }

    /// 固化段里只出现正典。预演线哪怕节点落在边界之前，也不能挤进"已定"那一段。
    #[test]
    fn fixed_segment_only_shows_the_base_line() {
        let root = book("linesat");
        let s = seed(&root);
        let ch = root.join("chapters").join("ch-016");
        std::fs::create_dir_all(&ch).unwrap();
        std::fs::write(ch.join("prose.md"), "正文").unwrap();

        s.fork("α", "n2", "β", "β", "", Kind::WhatIf).unwrap();
        let mut bn = Node::new("b1", "event", "x");
        bn.chapter = 20;
        s.add_node("β", bn).unwrap();
        let lines = s.all().unwrap();

        // 第 10 章（固化段内）：只有正典
        let at10: Vec<&str> = s.lines_at(&lines, 10).iter().map(|l| l.id.as_str()).collect();
        assert_eq!(at10, vec!["α"], "固化段只有正典，预演线无权参与");

        // 第 18 章 > 边界 16，属开放段：β 已分叉，应该出现
        let at18: Vec<&str> = s.lines_at(&lines, 18).iter().map(|l| l.id.as_str()).collect();
        assert!(at18.contains(&"β"), "开放段里已分叉的预演线要有位置");
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
    fn adopted_nodes_turn_written_when_prose_catches_up() {
        let root = book("written-sync");
        let s = seed(&root);
        // 第 12 章的节点已采纳；第 16 章的仍是计划
        s.update_node("α", "n1", NodePatch { state: Some(NodeState::Adopted), ..Default::default() }).unwrap();
        s.update_node("α", "n2", NodePatch { state: Some(NodeState::Adopted), ..Default::default() }).unwrap();

        // 正文只写到第 12 章：n1 → written，n2 仍是 adopted
        assert_eq!(mark_adopted_written(&root, 12).unwrap(), 1);
        let back = s.find("α").unwrap().unwrap();
        assert_eq!(back.nodes[0].state, NodeState::Written);
        assert_eq!(back.nodes[1].state, NodeState::Adopted, "正文还没写到第 16 章");

        // 幂等：再跑一次没有新增
        assert_eq!(mark_adopted_written(&root, 12).unwrap(), 0);
        // 写到第 16 章：n2 也转 written
        assert_eq!(mark_adopted_written(&root, 16).unwrap(), 1);
        assert_eq!(s.find("α").unwrap().unwrap().nodes[1].state, NodeState::Written);
    }

    #[test]
    fn planned_nodes_are_not_marked_written_by_prose() {
        // 作者写过了第 20 章，不代表"计划里但没采纳的节点"发生过——
        // 只有 Adopted 才随正文转 Written。
        let root = book("written-planned");
        let s = seed(&root);
        assert_eq!(mark_adopted_written(&root, 99).unwrap(), 0);
        assert_eq!(s.find("α").unwrap().unwrap().nodes[0].state, NodeState::Planned);
    }

    #[test]
    fn archived_preview_lines_do_not_push_the_global_boundary() {
        let root = book("archived-boundary");
        let s = seed(&root);
        let mut line = Line::new("β", "假设", Kind::Forecast, "α", "", 5);
        line.status = "已归档".into();
        let mut n = Node::new("b1", "event", "远未来");
        n.chapter = 90;
        line.nodes.push(n);
        let mut lines = s.all().unwrap();
        lines.push(line);
        assert_eq!(s.fixed_boundary(&lines), 0, "归档的预演线不该把边界顶到 90");
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
