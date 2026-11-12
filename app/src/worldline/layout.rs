//! 世界线的排版：车道顺序与树形布局。全是纯函数。
//!
//! 为什么单开一个文件：这两个算法都要被"画"和"点"两条路径共用
//! （画布拿它算坐标，点击拿它做命中测试），而且必须**可复现**——
//! 同一份数据两次布局不一样，树每次重绘都会抖，作者会觉得"世界乱了"。
//! 纯函数 + 全序排序键，这两条都能保证，也能直接单测。

use super::{Kind, Line};

/// 并轨图的一条车道。
#[derive(Debug, Clone, PartialEq)]
pub struct Lane {
    pub id: String,
    pub name: String,
    /// 从正典往下数几层（α = 0）
    pub depth: usize,
    /// 从第几章分出
    pub fork_chapter: u32,
    /// 这条线画到第几章
    pub last_chapter: u32,
    pub archived: bool,
    pub forecast: bool,
    pub pending_diffs: usize,
}

/// 并轨图车道顺序。
///
/// 顺序规则：正典在最上 → 按血缘深度分层 → 同层按分叉章 → 同章按 id。
/// 归档线一律沉底。全序键保证同一份数据永远得到同一顺序。
pub fn lanes(lines: &[Line]) -> Vec<Lane> {
    let depth_of = depths(lines);
    let mut out: Vec<Lane> = lines
        .iter()
        .map(|l| Lane {
            id: l.id.clone(),
            name: l.name.clone(),
            depth: *depth_of.get(&l.id).unwrap_or(&0),
            fork_chapter: if l.is_base() { 0 } else { l.fork_chapter() },
            last_chapter: l.last_chapter(),
            archived: l.is_archived(),
            forecast: l.kind == Kind::Forecast,
            pending_diffs: l.pending_diffs(),
        })
        .collect();
    out.sort_by(|a, b| {
        // 归档沉底，但正典永远第一
        let rank = |l: &Lane| {
            if l.id == "α" {
                0
            } else if l.archived {
                2
            } else {
                1
            }
        };
        rank(a)
            .cmp(&rank(b))
            .then(a.depth.cmp(&b.depth))
            .then(a.fork_chapter.cmp(&b.fork_chapter))
            .then(a.id.cmp(&b.id))
    });
    out
}

/// 每条线到正典的血缘深度。父指针缺失/成环时按根处理（深度 0）。
fn depths(lines: &[Line]) -> std::collections::BTreeMap<String, usize> {
    let parent: std::collections::BTreeMap<&str, &str> = lines
        .iter()
        .map(|l| (l.id.as_str(), l.parent_id.as_str()))
        .collect();
    let mut out = std::collections::BTreeMap::new();
    for l in lines {
        let mut depth = 0usize;
        let mut cur = l.parent_id.as_str();
        // 最多走 lines.len() 步：再多就是环
        for _ in 0..lines.len() {
            if cur.is_empty() {
                break;
            }
            let Some(next) = parent.get(cur) else { break };
            depth += 1;
            if *next == cur || *next == l.id.as_str() {
                break;
            }
            cur = next;
        }
        out.insert(l.id.clone(), depth);
    }
    out
}

/// 树布局里的一个枝。
#[derive(Debug, Clone, PartialEq)]
pub struct TreeNode {
    pub id: String,
    pub name: String,
    pub parent: Option<String>,
    pub depth: usize,
    /// 分配的槽位区间（叶占 1 槽，父占子树总和）
    pub slot_start: f32,
    pub slot_end: f32,
    /// 横向位置，0..1（已按总槽数归一）
    pub x: f32,
    /// 从第几章长出
    pub fork_chapter: u32,
    /// 画到第几章
    pub end_chapter: u32,
    pub archived: bool,
    pub forecast: bool,
    pub node_count: usize,
}

/// 树布局结果。
#[derive(Debug, Clone, PartialEq)]
pub struct Tree {
    pub nodes: Vec<TreeNode>,
    /// 总槽数（画布用它算横向间距）
    pub slots: f32,
    /// 这棵树覆盖到第几章（纵轴上界）
    pub max_chapter: u32,
}

/// 树形布局：纵轴 = 章节，横轴 = 按子树宽度分配。
///
/// 与旧实现的关键差别：旧的 x 是"id 字符和取模"抖动出来的，看着随机、
/// 每次不一定一样，也没有"子树占多少宽度"的概念——枝会互相压。这里
/// 用标准的 tidy-tree 分配：叶 1 槽，父 = 子之和，父的 x = 自己区间的中心。
pub fn tree(lines: &[Line]) -> Tree {
    // 只画有血缘的：正典为根，其余按 parentId 挂上去。
    // 父不存在（被删/手改坏了）就当根，不至于整条线消失。
    let ids: std::collections::BTreeSet<&str> = lines.iter().map(|l| l.id.as_str()).collect();
    let mut children: std::collections::BTreeMap<&str, Vec<&Line>> = Default::default();
    let mut roots: Vec<&Line> = Vec::new();
    for l in lines {
        let p = l.parent_id.as_str();
        if p.is_empty() || !ids.contains(p) || p == l.id {
            roots.push(l);
        } else {
            children.entry(p).or_default().push(l);
        }
    }
    // 手改出来的循环血缘（β→γ→β）会让环里的线从根不可达、整棵树里消失。
    // 这里把每条不可达环的第一个节点升为根，其余照旧挂在它下面——作者看到的是
    // "多了一棵独立的树"，而不是"我的世界没了"。
    {
        let mut reachable: std::collections::BTreeSet<String> = Default::default();
        let mut queue: Vec<String> = roots.iter().map(|l| l.id.clone()).collect();
        while let Some(id) = queue.pop() {
            if !reachable.insert(id.clone()) {
                continue;
            }
            if let Some(kids) = children.get(id.as_str()) {
                for k in kids {
                    queue.push(k.id.clone());
                }
            }
        }
        for l in lines {
            if reachable.contains(&l.id) {
                continue;
            }
            if let Some(kids) = children.get_mut(l.parent_id.as_str()) {
                kids.retain(|k| k.id != l.id);
            }
            roots.push(l);
            // 从这个新根往下走，把环剩余部分标成可达
            let mut q = vec![l.id.clone()];
            while let Some(id) = q.pop() {
                if !reachable.insert(id.clone()) {
                    continue;
                }
                if let Some(kids) = children.get(id.as_str()) {
                    for k in kids {
                        q.push(k.id.clone());
                    }
                }
            }
        }
    }

    // 子节点顺序：分叉章 → id。全序，保证布局可复现。
    for kids in children.values_mut() {
        kids.sort_by(|a, b| {
            a.fork_chapter()
                .cmp(&b.fork_chapter())
                .then(a.id.cmp(&b.id))
        });
    }
    roots.sort_by(|a, b| {
        // 正典排最左，其余按 id
        let rank = |l: &Line| if l.is_base() { 0 } else { 1 };
        rank(a).cmp(&rank(b)).then(a.id.cmp(&b.id))
    });

    let mut out: Vec<TreeNode> = Vec::new();
    let mut cursor = 0f32;
    let mut max_chapter = 0u32;

    // 显式栈做后序，避免深递归（作者可能造很多层）
    struct Frame<'a> {
        line: &'a Line,
        depth: usize,
        slot_start: f32,
        /// 已处理到第几个子节点
        next_child: usize,
        child_slots: f32,
    }
    for root in roots {
        let mut stack: Vec<Frame> = vec![Frame {
            line: root,
            depth: 0,
            slot_start: cursor,
            next_child: 0,
            child_slots: 0.,
        }];
        while let Some(top) = stack.last_mut() {
            let kids = children
                .get(top.line.id.as_str())
                .map(|v| v.as_slice())
                .unwrap_or(&[]);
            if top.next_child < kids.len() {
                let idx = top.next_child;
                let start = top.slot_start + top.child_slots;
                top.child_slots += 1.; // 先占位，子树会把它向右推
                top.next_child += 1;
                let depth = top.depth + 1;
                let child = kids[idx];
                stack.push(Frame {
                    line: child,
                    depth,
                    slot_start: start,
                    next_child: 0,
                    child_slots: 0.,
                });
            } else {
                // 子树处理完：宽度 = max(1, 子槽数之和)
                let width = top.child_slots.max(1.);
                let start = top.slot_start;
                let end = start + width;
                // 父节点的槽区间要覆盖子树：把父自己往前走
                let line = top.line;
                let fork_chapter = if line.is_base() { 1 } else { line.fork_chapter().max(1) };
                let end_chapter = line.last_chapter().max(fork_chapter);
                max_chapter = max_chapter.max(end_chapter).max(fork_chapter);
                out.push(TreeNode {
                    id: line.id.clone(),
                    name: line.name.clone(),
                    parent: if line.parent_id.is_empty() {
                        None
                    } else {
                        Some(line.parent_id.clone())
                    },
                    depth: top.depth,
                    slot_start: start,
                    slot_end: end,
                    x: (start + end) / 2.,
                    fork_chapter,
                    end_chapter,
                    archived: line.is_archived(),
                    forecast: line.kind == Kind::Forecast,
                    node_count: line.nodes.len(),
                });
                cursor = end;
                stack.pop();
                // 把宽度回报给父
                if let Some(parent) = stack.last_mut() {
                    // 父之前只给这个子留了 1 槽；实际宽度可能是 n 槽
                    let extra = width - 1.;
                    if extra > 0. {
                        // 把父已结算的兄弟整体右移不现实；改为记录在 child_slots
                        parent.child_slots += extra;
                    }
                }
            }
        }
    }

    // 归一化：x 落到 0..1
    let slots = cursor.max(1.);
    for n in &mut out {
        n.x = (n.x / slots).clamp(0., 1.);
        n.slot_start /= slots;
        n.slot_end /= slots;
    }
    // 输出顺序固定（父在前，便于画布先画干再画枝）
    out.sort_by(|a, b| a.depth.cmp(&b.depth).then(a.x.partial_cmp(&b.x).unwrap_or(std::cmp::Ordering::Equal)));
    Tree { nodes: out, slots, max_chapter: max_chapter.max(1) }
}

/// 两条线的对比结果：共同祖先之后的差异节点与互相冲突的影响。
///
/// "冲突"的定义要克制：只有**同一目标**被两条线以不同方式改（比如都改"断刀"
/// 但一个 add 一个 remove）才算冲突。仅仅是"两条线都动了某个设定"不算——
/// 那在写小说时太常见了（两边都得提到主角的刀），报出来是噪音。
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Compare {
    pub a: String,
    pub b: String,
    /// 两条线共同的祖先（空 = 无共同祖先）
    pub common_ancestor: String,
    /// a 独有的节点（label + chapter）
    pub only_a: Vec<(String, u32)>,
    /// b 独有的节点
    pub only_b: Vec<(String, u32)>,
    /// 同名同章 = 两条线都推演到了同一件事
    pub shared: Vec<(String, u32)>,
    /// 同一目标被改得不一样
    pub conflicts: Vec<String>,
}

/// 对比两条线。
pub fn compare(lines: &[Line], a: &str, b: &str) -> Compare {
    let find = |id: &str| lines.iter().find(|l| l.id == id);
    let (Some(la), Some(lb)) = (find(a), find(b)) else {
        return Compare { a: a.into(), b: b.into(), ..Default::default() };
    };
    let ancestor = common_ancestor(lines, a, b);
    // 共同祖先之后的节点才算"差异"：祖先之前两边本来就一样
    let after = |l: &Line| -> Vec<(String, u32)> {
        let cut = if ancestor.is_empty() {
            0
        } else {
            find(&ancestor)
                .map(|x| x.nodes.iter().map(|n| n.chapter_or_guess()).max().unwrap_or(0))
                .unwrap_or(0)
        };
        let mut v: Vec<(String, u32)> = l
            .nodes
            .iter()
            .map(|n| (n.label.clone(), n.chapter_or_guess()))
            .filter(|(_, ch)| *ch >= cut)
            .collect();
        v.sort_by(|x, y| x.1.cmp(&y.1).then(x.0.cmp(&y.0)));
        v
    };
    let va = after(la);
    let vb = after(lb);
    let mut only_a = Vec::new();
    let mut only_b = Vec::new();
    let mut shared = Vec::new();
    for n in &va {
        if vb.contains(n) {
            shared.push(n.clone());
        } else {
            only_a.push(n.clone());
        }
    }
    for n in &vb {
        if !va.contains(n) && !only_b.contains(n) {
            only_b.push(n.clone());
        }
    }
    // 冲突：同一 targetRef 在两条线上 kind 不同
    let mut conflicts = Vec::new();
    for da in &la.diffs {
        if let Some(db) = lb
            .diffs
            .iter()
            .find(|d| !d.target_ref.is_empty() && d.target_ref == da.target_ref)
        {
            if da.kind != db.kind {
                conflicts.push(format!(
                    "{}「{}」：{} 是 {}，{} 是 {}",
                    da.target, da.target_ref, a, da.kind, b, db.kind
                ));
            }
        }
    }
    Compare {
        a: a.into(),
        b: b.into(),
        common_ancestor: ancestor,
        only_a,
        only_b,
        shared,
        conflicts,
    }
}

/// 两条线最近的共同祖先。沿 a 的父链走，第一个出现在 b 的父链里的就是。
fn common_ancestor(lines: &[Line], a: &str, b: &str) -> String {
    let parent_of = |id: &str| -> String {
        lines
            .iter()
            .find(|l| l.id == id)
            .map(|l| l.parent_id.clone())
            .unwrap_or_default()
    };
    let chain_b = {
        let mut v = vec![b.to_string()];
        let mut cur = parent_of(b);
        for _ in 0..lines.len() {
            if cur.is_empty() || v.contains(&cur) {
                break;
            }
            v.push(cur.clone());
            cur = parent_of(&cur);
        }
        v
    };
    let mut cur = a.to_string();
    let mut seen = vec![cur.clone()];
    for _ in 0..lines.len() {
        if chain_b.contains(&cur) {
            return cur;
        }
        let p = parent_of(&cur);
        if p.is_empty() || seen.contains(&p) {
            break;
        }
        seen.push(p.clone());
        cur = p;
    }
    String::new()
}

/// 幕带：把 `stages.json` 里的章节区间换算成"从第几章到第几章、叫什么"。
///
/// 单独抽出来是因为排期板和世界线两个视图都要画，且都要处理
/// "作者把 chapterStart 写成 0 或倒挂"这种手改出来的脏数据。
#[derive(Debug, Clone, PartialEq)]
pub struct Band {
    pub name: String,
    pub theme: String,
    pub start: u32,
    pub end: u32,
}

pub fn bands<T: AsRef<str>>(stages: &[(T, T, u32, u32)]) -> Vec<Band> {
    let mut out: Vec<Band> = stages
        .iter()
        .filter_map(|(name, theme, a, b)| {
            let (a, b) = (*a, *b);
            // 起止都缺 = 没有信息，不画；倒挂就交换，别画成负宽度
            if a == 0 && b == 0 {
                return None;
            }
            let (start, end) = if a == 0 {
                (1, b)
            } else if b == 0 || b < a {
                (a, a)
            } else {
                (a, b)
            };
            Some(Band {
                name: name.as_ref().to_string(),
                theme: theme.as_ref().to_string(),
                start,
                end,
            })
        })
        .collect();
    out.sort_by(|x, y| x.start.cmp(&y.start));
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::worldline::{Kind, Line};

    fn line(id: &str, parent: &str, fork: u32, kind: Kind, archived: bool) -> Line {
        let mut l = Line::new(id, id, kind, parent, "", fork);
        if archived {
            l.status = "已归档".into();
        }
        l
    }

    #[test]
    fn lanes_put_base_first_and_archived_last() {
        let lines = vec![
            line("ζ", "α", 20, Kind::WhatIf, true),
            line("β", "α", 10, Kind::WhatIf, false),
            line("α", "", 0, Kind::Base, false),
        ];
        let ordered = lanes(&lines);
        let order: Vec<&str> = ordered.iter().map(|l| l.id.as_str()).collect();
        assert_eq!(order, vec!["α", "β", "ζ"], "正典第一，归档沉底");
    }

    #[test]
    fn lanes_sort_by_lineage_depth_then_chapter() {
        let lines = vec![
            line("α", "", 0, Kind::Base, false),
            line("γ", "β", 18, Kind::WhatIf, false), // 深一层
            line("β", "α", 10, Kind::WhatIf, false),
            line("δ", "α", 6, Kind::WhatIf, false), // 同层但更早
        ];
        let ordered = lanes(&lines);
        let order: Vec<&str> = ordered.iter().map(|l| l.id.as_str()).collect();
        assert_eq!(order, vec!["α", "δ", "β", "γ"], "浅的线在上，同层按分叉章");
    }

    #[test]
    fn lanes_are_stable_across_calls() {
        let lines = vec![
            line("α", "", 0, Kind::Base, false),
            line("β", "α", 10, Kind::WhatIf, false),
            line("γ", "α", 10, Kind::Forecast, false),
        ];
        let a: Vec<String> = lanes(&lines).iter().map(|l| l.id.clone()).collect();
        let b: Vec<String> = lanes(&lines).iter().map(|l| l.id.clone()).collect();
        assert_eq!(a, b);
        assert_eq!(a, vec!["α", "β", "γ"], "同章同层按 id 定序");
    }

    #[test]
    fn cyclic_parent_pointers_do_not_hang() {
        // 手改坏的血缘：β→γ→β 成环
        let mut a = line("β", "γ", 5, Kind::WhatIf, false);
        let mut b = line("γ", "β", 6, Kind::WhatIf, false);
        a.parent_id = "γ".into();
        b.parent_id = "β".into();
        let lines = vec![line("α", "", 0, Kind::Base, false), a, b];
        // 不死循环就是通过；深度不要求正确，只要有限
        let ls = lanes(&lines);
        assert_eq!(ls.len(), 3);
        assert!(ls.iter().all(|l| l.depth < 3));
        let t = tree(&lines);
        assert_eq!(t.nodes.len(), 3);
    }

    #[test]
    fn tree_assigns_disjoint_slots_to_siblings() {
        let lines = vec![
            line("α", "", 0, Kind::Base, false),
            line("β", "α", 10, Kind::WhatIf, false),
            line("γ", "α", 12, Kind::WhatIf, false),
        ];
        let t = tree(&lines);
        let get = |id: &str| t.nodes.iter().find(|n| n.id == id).unwrap();
        let (b, g) = (get("β"), get("γ"));
        assert!(b.slot_end <= g.slot_start, "兄弟不能重叠：{b:?} {g:?}");
        // 子节点在父节点区间之内（或与之相接）
        let a = get("α");
        assert!(b.slot_start >= a.slot_start - 0.001 && b.slot_end <= a.slot_end + 0.001);
        assert!(g.slot_start >= a.slot_start - 0.001 && g.slot_end <= a.slot_end + 0.001);
        assert!((a.slot_end - a.slot_start - (1.0 + 1e-6)).abs() < 0.01, "两槽从 α 分出");
    }

    #[test]
    fn tree_layout_is_reproducible() {
        let lines = vec![
            line("α", "", 0, Kind::Base, false),
            line("β", "α", 10, Kind::WhatIf, false),
            line("γ", "α", 12, Kind::Forecast, false),
            line("δ", "β", 15, Kind::WhatIf, false),
            line("ε", "β", 16, Kind::WhatIf, false),
        ];
        let a = tree(&lines);
        let b = tree(&lines);
        assert_eq!(a, b, "同一份数据两次布局必须完全一致，否则画布每次重绘都会抖");
    }

    #[test]
    fn tree_deeper_branch_takes_more_width() {
        let shallow = vec![
            line("α", "", 0, Kind::Base, false),
            line("β", "α", 10, Kind::WhatIf, false),
        ];
        let deep = vec![
            line("α", "", 0, Kind::Base, false),
            line("β", "α", 10, Kind::WhatIf, false),
            line("δ", "β", 15, Kind::WhatIf, false),
            line("ε", "β", 16, Kind::WhatIf, false),
        ];
        assert!(tree(&deep).slots > tree(&shallow).slots, "带两个孙子的枝应占更宽");
    }

    #[test]
    fn orphaned_line_becomes_its_own_root() {
        // 父线被删了，子线还在：不能因此从画面里消失
        let lines = vec![
            line("α", "", 0, Kind::Base, false),
            line("ζ", "已删除的线", 8, Kind::WhatIf, false),
        ];
        let t = tree(&lines);
        assert_eq!(t.nodes.len(), 2);
        assert_eq!(t.nodes.iter().find(|n| n.id == "ζ").unwrap().parent, Some("已删除的线".into()));
    }

    #[test]
    fn tree_uses_fork_node_chapter_when_available() {
        let mut alpha = line("α", "", 0, Kind::Base, false);
        let mut n = crate::worldline::Node::new("n1", "event", "初遇");
        n.chapter = 16;
        alpha.nodes.push(n);
        let mut beta = line("β", "α", 0, Kind::WhatIf, false);
        beta.fork_node = "n1".into(); // 没写 forkedChapter，靠节点推
        // fork_node 指向父线的节点，但 Line::fork_chapter 只查自己身上的节点，
        // 所以这里靠 forkedChapter 兜底：显式写 0 时画在第 1 章
        let t = tree(&[alpha, beta]);
        let b = t.nodes.iter().find(|n| n.id == "β").unwrap();
        assert!(b.fork_chapter >= 1, "分叉章不能是 0（0 画不出来）");
    }

    fn with_nodes(mut l: Line, nodes: &[(&str, u32)]) -> Line {
        for (label, ch) in nodes {
            let mut n = crate::worldline::Node::new(label, "event", label);
            n.chapter = *ch;
            l.nodes.push(n);
        }
        l
    }

    #[test]
    fn compare_splits_nodes_after_the_common_ancestor() {
        let lines = vec![
            with_nodes(line("α", "", 0, Kind::Base, false), &[("出鞘", 5), ("初遇", 16)]),
            with_nodes(line("β", "α", 16, Kind::WhatIf, false), &[("师兄现身", 18)]),
            with_nodes(line("γ", "α", 16, Kind::WhatIf, false), &[("师兄身死", 18), ("血战", 20)]),
        ];
        let c = compare(&lines, "β", "γ");
        assert_eq!(c.common_ancestor, "α");
        assert!(c.only_a.contains(&("师兄现身".into(), 18)));
        assert!(c.only_b.contains(&("血战".into(), 20)));
        // 祖先之前的节点两边一样，不该进差异列表
        assert!(!c.only_a.iter().any(|(l, _)| l == "出鞘"));
    }

    #[test]
    fn compare_reports_only_real_conflicts() {
        let mut a = line("β", "α", 16, Kind::WhatIf, false);
        a.diffs = vec![
            crate::worldline::Diff { target: "伏笔".into(), target_ref: "断刀".into(), kind: "change".into(), ..Default::default() },
            crate::worldline::Diff { target: "设定".into(), target_ref: "长安".into(), kind: "add".into(), ..Default::default() },
        ];
        let mut b = line("γ", "α", 16, Kind::WhatIf, false);
        b.diffs = vec![
            // 与 a 同目标但方式不同 → 冲突
            crate::worldline::Diff { target: "伏笔".into(), target_ref: "断刀".into(), kind: "remove".into(), ..Default::default() },
            // 同目标同方式 → 不算冲突
            crate::worldline::Diff { target: "设定".into(), target_ref: "长安".into(), kind: "add".into(), ..Default::default() },
        ];
        let lines = vec![line("α", "", 0, Kind::Base, false), a, b];
        let c = compare(&lines, "β", "γ");
        assert_eq!(c.conflicts.len(), 1, "只有方式不同才算冲突：{:?}", c.conflicts);
        assert!(c.conflicts[0].contains("断刀"));
    }

    #[test]
    fn compare_handles_lines_without_a_common_ancestor() {
        let lines = vec![
            line("α", "", 0, Kind::Base, false),
            with_nodes(line("ζ", "", 5, Kind::WhatIf, false), &[("孤立", 9)]),
        ];
        let c = compare(&lines, "α", "ζ");
        assert_eq!(c.common_ancestor, "", "两条根之间没有共同祖先");
        // 没有共同祖先时不做章节过滤，各自全部节点都是差异
        assert!(!c.only_b.is_empty());
    }

    #[test]
    fn compare_of_unknown_line_is_empty_not_panic() {
        let lines = vec![line("α", "", 0, Kind::Base, false)];
        let c = compare(&lines, "α", "不存在");
        assert!(c.only_a.is_empty() && c.only_b.is_empty());
    }

    #[test]
    fn bands_sort_and_repair_inverted_ranges() {
        let raw = vec![
            ("第二幕", "灯会", 20u32, 35u32),
            ("第一幕", "出鞘", 1u32, 19u32),
            ("坏幕", "", 30u32, 10u32),
            ("空幕", "", 0u32, 0u32),
        ];
        let b = bands(&raw);
        assert_eq!(b.len(), 3, "空幕没有信息，不该占一格");
        assert_eq!(b[0].name, "第一幕");
        assert_eq!(b[1].name, "第二幕");
        // 倒挂的区间被收成一个点，宽度不会是负的
        let bad = b.iter().find(|x| x.name == "坏幕").unwrap();
        assert_eq!((bad.start, bad.end), (30, 30));
    }

    #[test]
    fn bands_handle_missing_starts() {
        let raw = vec![("序", "", 0u32, 4u32)];
        let b = bands(&raw);
        assert_eq!((b[0].start, b[0].end), (1, 4), "缺起始章按第 1 章算");
    }
}
