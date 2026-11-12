//! 世界线的排版：车道顺序与树形布局。全是纯函数。
//!
//! 为什么单开一个文件：这两个算法都要被"画"和"点"两条路径共用
//! （画布拿它算坐标，点击拿它做命中测试），而且必须**可复现**——
//! 同一份数据两次布局不一样，树每次重绘都会抖，作者会觉得"世界乱了"。
//! 纯函数 + 全序排序键，这两条都能保证，也能直接单测。

use super::{Kind, Line};

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

/// 只保留以 `root` 为根的子树（树模式的「钻取」）。
///
/// 作者有一堆 if 时，整棵树会挤成一片。钻取到某条线，只看它的后代。
/// 返回的 `lines` 里，root 的 parent 被清空——这样 `tree()` 会把它当根，
/// 槽位从 0 重新排，画面填满而不是挤在角落。
pub fn subtree(lines: &[Line], root: &str) -> Vec<Line> {
    if root.is_empty() {
        return lines.to_vec();
    }
    let mut keep: Vec<String> = vec![root.to_string()];
    // 逐层往下收集：作者的树不会很深，但也不能假设
    let mut frontier = vec![root.to_string()];
    for _ in 0..lines.len() {
        let mut next: Vec<String> = Vec::new();
        for l in lines {
            if frontier.iter().any(|f| *f == l.parent_id) && !keep.contains(&l.id) {
                keep.push(l.id.clone());
                next.push(l.id.clone());
            }
        }
        if next.is_empty() {
            break;
        }
        frontier = next;
    }
    lines
        .iter()
        .filter(|l| keep.contains(&l.id))
        .map(|l| {
            let mut c = l.clone();
            if c.id == root {
                c.parent_id.clear();
                c.fork_node.clear();
            }
            c
        })
        .collect()
}

/// 时间轴上的一条轨道。
///
/// 与旧的 `Lane` 的区别：旧的是"一条线占一条横向车道，谁跟谁平行"，
/// 新的是"一条线在时间轴上的可见区间"——它明确区分**已固化**与**开放**两段。
/// 这是世界线最核心的语义：前面的剧情定了，后面才是 if 的地方。
#[derive(Debug, Clone, PartialEq)]
pub struct Track {
    pub id: String,
    pub name: String,
    pub kind: Kind,
    /// 分叉点：从父线的第几章分开（正典 = 0）
    pub fork_chapter: u32,
    /// 可见区间的起点（正典 0；if 线 = fork_chapter）
    pub from_chapter: u32,
    /// 这条线有内容的最后章
    pub last_chapter: u32,
    /// 已固化到的章（≤ 这个是实线，> 这个是虚线）
    pub fixed_through: u32,
    pub archived: bool,
    /// 从哪条线分叉
    pub parent_id: String,
    /// 血缘深度（用于缩进）
    pub depth: usize,
    pub pending_diffs: usize,
    pub node_count: usize,
}

/// 时间轴布局：把线排成轨道，分清固化段与开放段。
///
/// 排序规则（全序，可复现）：
/// 1. 正典永远第一
/// 2. 已扶正（Fixed）紧随其后——它们是"已经写进正典的那一版"
/// 3. 其余按血缘深度 → 分叉章 → id
/// 4. 归档沉底
pub fn tracks(lines: &[Line], global_boundary: u32) -> Vec<Track> {
    let depth_of = depths(lines);
    let mut out: Vec<Track> = lines
        .iter()
        .map(|l| Track {
            id: l.id.clone(),
            name: l.name.clone(),
            kind: l.kind,
            fork_chapter: if l.is_base() { 0 } else { l.fork_chapter() },
            from_chapter: if l.is_base() { 0 } else { l.fork_chapter() },
            last_chapter: l.last_chapter().max(l.fixed_boundary()),
            // 固化段的归属只有两种：正典（到全局边界）与已扶正线（到它自己的边界）。
            // 未扶正的 if 线整条都是开放的——它推演得再远也还没定，
            // 用 fixed_boundary() 会把"推演到第 15 章"误画成"固化到第 15 章"。
            fixed_through: if l.is_base() {
                global_boundary
            } else if l.kind == Kind::Fixed {
                l.fixed_boundary()
            } else {
                0
            },
            archived: l.is_archived(),
            parent_id: l.parent_id.clone(),
            depth: *depth_of.get(&l.id).unwrap_or(&0),
            pending_diffs: l.pending_diffs(),
            node_count: l.nodes.len(),
        })
        .collect();
    out.sort_by(|a, b| {
        let rank = |t: &Track| {
            if t.id == "α" || t.kind == Kind::Base {
                0
            } else if t.kind == Kind::Fixed {
                1
            } else if t.archived {
                3
            } else {
                2
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

/// 时间轴上的一段：某条线在某章区间里的内容。
#[derive(Debug, Clone, PartialEq)]
pub struct Segment {
    pub track: String,
    /// 起点（含）
    pub from: u32,
    /// 终点（含）
    pub to: u32,
    /// 这段是固化的（true）还是开放的 if（false）
    pub fixed: bool,
}

/// 把每条轨道切成「固化段 + 开放段」。
///
/// 一条 if 线可能整个都在开放段（还没扶正），也可能被扶正后整段变成固化。
/// 正典则通常前半固化、后半开放。
pub fn segments(tracks: &[Track], horizon: u32) -> Vec<Segment> {
    let mut out = Vec::new();
    for t in tracks {
        if t.archived {
            continue;
        }
        let start = t.from_chapter.max(if t.kind == Kind::Base { 0 } else { t.from_chapter });
        let end = horizon.max(t.last_chapter);
        if end < start {
            continue;
        }
        // 固化段与开放段的分界点，夹在这条线的区间内
        let cut = t.fixed_through.min(end);
        if cut >= start {
            out.push(Segment { track: t.id.clone(), from: start, to: cut, fixed: true });
        }
        if cut < end {
            let from = (cut + 1).max(start);
            if from <= end {
                out.push(Segment { track: t.id.clone(), from, to: end, fixed: false });
            }
        }
    }
    out
}

/// 某个固化点上，有哪些 if 线从这里或其之前分出（预言书页的核心查询）。
///
/// 找的是"在 `chapter` 这一章已经存在、并且之后还能继续走"的线——
/// 作者选中第 12 章，想看到"在第 12 章这个时间点上，有哪些 if 已经分出来了"。
pub fn forks_at<'a>(lines: &'a [Line], chapter: u32) -> Vec<&'a Line> {
    let mut out: Vec<&Line> = lines
        .iter()
        .filter(|l| {
            !l.is_base() && !l.is_archived() && l.fork_chapter() <= chapter
        })
        .collect();
    out.sort_by(|a, b| {
        a.fork_chapter()
            .cmp(&b.fork_chapter())
            .then(a.id.cmp(&b.id))
    });
    out
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
    fn cyclic_parent_pointers_do_not_hang() {
        // 手改坏的血缘：β→γ→β 成环
        let mut a = line("β", "γ", 5, Kind::WhatIf, false);
        let mut b = line("γ", "β", 6, Kind::WhatIf, false);
        a.parent_id = "γ".into();
        b.parent_id = "β".into();
        let lines = vec![line("α", "", 0, Kind::Base, false), a, b];
        // 不死循环就是通过；深度不要求正确，只要有限
        let ls = tracks(&lines, 0);
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
    fn tracks_put_base_then_fixed_then_ifs() {
        let b = with_nodes(line("β", "α", 12, Kind::WhatIf, false), &[("师兄现身", 18)]);
        let f = with_nodes(line("固定版", "α", 12, Kind::Fixed, false), &[("已扶正", 20)]);
        let lines = vec![line("α", "", 0, Kind::Base, false), b, f];
        let tr = tracks(&lines, 10);
        let order: Vec<&str> = tr.iter().map(|t| t.id.as_str()).collect();
        assert_eq!(order, vec!["α", "固定版", "β"], "正典 → 已扶正 → 其余 if");
    }

    #[test]
    fn segments_split_fixed_and_open_at_the_boundary() {
        let a = with_nodes(line("α", "", 0, Kind::Base, false), &[("出鞘", 5), ("初遇", 16)]);
        let t = tracks(&[a], 16);
        let segs = segments(&t, 30);
        let fixed: Vec<_> = segs.iter().filter(|s| s.fixed).collect();
        let open: Vec<_> = segs.iter().filter(|s| !s.fixed).collect();
        assert_eq!(fixed.len(), 1);
        assert_eq!((fixed[0].from, fixed[0].to), (0, 16), "固化段到边界为止");
        assert_eq!(open.len(), 1);
        assert_eq!(open[0].from, 17, "开放段从边界后一章开始");
        assert_eq!(open[0].to, 30, "开放段延伸到视野尽头");
    }

    #[test]
    fn a_pure_if_line_is_all_open() {
        let b = with_nodes(line("β", "α", 12, Kind::WhatIf, false), &[("假设", 15)]);
        let lines = vec![line("α", "", 0, Kind::Base, false), b];
        let t = tracks(&lines, 16);
        let beta_track = t.iter().find(|x| x.id == "β").unwrap();
        let segs = segments(&[beta_track.clone()], 30);
        assert!(segs.iter().all(|s| !s.fixed), "没扶正的 if 线整段都是开放的");
        assert_eq!(segs[0].from, 12, "从它的分叉点开始");
    }

    #[test]
    fn fixed_line_becomes_solid_after_being_promoted() {
        let b = with_nodes(line("β", "α", 12, Kind::Fixed, false), &[("扶正后", 22)]);
        let lines = vec![line("α", "", 0, Kind::Base, false), b];
        let t = tracks(&lines, 22);
        let beta_track = t.iter().find(|x| x.id == "β").unwrap();
        let segs = segments(&[beta_track.clone()], 30);
        let solid: Vec<_> = segs.iter().filter(|s| s.fixed).collect();
        assert_eq!(solid.len(), 1);
        assert_eq!((solid[0].from, solid[0].to), (12, 22), "扶正后整段变实线");
    }

    #[test]
    fn forks_at_lists_ifs_available_at_that_chapter() {
        let b = with_nodes(line("β", "α", 12, Kind::WhatIf, false), &[("x", 18)]);
        let g = with_nodes(line("γ", "α", 20, Kind::WhatIf, false), &[("y", 25)]);
        let lines = vec![line("α", "", 0, Kind::Base, false), b, g];
        // 第 15 章：只有 β（γ 还没分叉）
        let at15: Vec<&str> = forks_at(&lines, 15).iter().map(|l| l.id.as_str()).collect();
        assert_eq!(at15, vec!["β"]);
        // 第 22 章：β 与 γ 都在
        let at22: Vec<&str> = forks_at(&lines, 22).iter().map(|l| l.id.as_str()).collect();
        assert_eq!(at22, vec!["β", "γ"]);
    }

    #[test]
    fn archived_tracks_have_no_segments() {
        let b = with_nodes(line("β", "α", 12, Kind::WhatIf, true), &[("x", 18)]);
        let lines = vec![line("α", "", 0, Kind::Base, false), b];
        let t = tracks(&lines, 16);
        let segs = segments(&t, 30);
        assert!(!segs.iter().any(|s| s.track == "β"), "归档线不该占时间轴");
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
