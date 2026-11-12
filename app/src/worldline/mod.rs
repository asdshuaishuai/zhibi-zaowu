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

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

/// 线的性质。决定画布上的线型与分组。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Kind {
    /// 正典基准（通常只有 α）
    Base,
    /// 已固定：作者认可、视同事实
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
}

impl NodeState {
    pub fn key(self) -> &'static str {
        match self {
            NodeState::Planned => "planned",
            NodeState::Written => "written",
            NodeState::BranchOnly => "branch-only",
        }
    }

    pub fn from_key(s: &str) -> Self {
        match s {
            "written" => NodeState::Written,
            "branch-only" => NodeState::BranchOnly,
            _ => NodeState::Planned,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            NodeState::Planned => "计划",
            NodeState::Written => "已写",
            NodeState::BranchOnly => "仅此分支",
        }
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
fn guess_chapter(s: &str) -> Option<u32> {
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
            status: "推演中".into(),
            nodes: Vec::new(),
            edges: Vec::new(),
            diffs: Vec::new(),
            extra: BTreeMap::new(),
        }
    }

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
        serde_json::from_str(&text).map_err(|e| format!("worldlines.json 解析失败：{e}"))
    }

    pub fn save(&self, lines: &[Line]) -> Result<(), String> {
        let json = serde_json::to_string_pretty(lines).map_err(|e| e.to_string())?;
        let tmp = self.path().with_extension("json.tmp");
        std::fs::write(&tmp, json).map_err(|e| e.to_string())?;
        std::fs::rename(&tmp, self.path()).map_err(|e| e.to_string())
    }

    pub fn find(&self, id: &str) -> Result<Option<Line>, String> {
        Ok(self.all()?.into_iter().find(|l| l.id == id))
    }

    /// 从某条线的某个节点分叉。
    ///
    /// 这是「从任意节点再分叉」的落点：父线必须存在、节点必须存在于父线，
    /// 这样 `parentId`/`forkNode` 才是可信的血缘，树布局才有依据。
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
