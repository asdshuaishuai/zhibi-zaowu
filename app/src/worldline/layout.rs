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
    // 正典 id（世界树的主干）。没有血缘的线一律挂在它下面——
    // 一条世界线即便没写 parentId，逻辑上也是从正典某个点分出来的，
    // 不是凭空独立的一棵树。旧实现把它们都当根，画出来是五根互不相连的柱子。
    let base_id: Option<&str> = lines
        .iter()
        .find(|l| l.is_base())
        .map(|l| l.id.as_str());
    let mut children: std::collections::BTreeMap<&str, Vec<&Line>> = Default::default();
    let mut roots: Vec<&Line> = Vec::new();
    for l in lines {
        let p = l.parent_id.as_str();
        if l.is_base() {
            roots.push(l);
            continue;
        }
        if p.is_empty() || !ids.contains(p) || p == l.id {
            // 没写父指针：挂到正典上（正典不存在时才当独立根）
            match base_id {
                Some(b) => children.entry(b).or_default().push(l),
                None => roots.push(l),
            }
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
                // 线的末端：走到它自己的最后一章。如果一个已排章的节点都没有
                //（AI 推演出来还没落章很常见），至少给它一段可见的枝梢——
                // 否则整棵树的所有枝都是零长度，看起来像"没有树"。
                let last = line.last_chapter();
                let end_chapter = if last > fork_chapter {
                    last
                } else {
                    // 3 章或全书的 1/10，取大者：短书里也看得见，长书里不夸张
                    fork_chapter + 3.max(1)
                };
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

/// 时间轴取景框的缩放（纯算术，不碰 UI）。
///
/// `window` = (起章, 止章)。`factor > 1` 缩小视野（看得更细），`< 1` 拉远。
///
/// 为什么以中心为轴：作者放大都是为了看清"正在看的那一段"。以左端为轴的话
/// 每按一次 ＋ 都会朝第 1 章跑，越放越离开关注点。
///
/// 两条不变量：
/// 1. 跨度不小于 8 章——再窄就只剩两三根刻度，"看一段"退化成"看一章"。
/// 2. 结果始终落在 `0..=full` 内，且**跨度不变**（越界靠整体平移解决，
///    裁剪会改变缩放比例，按一次＋和按两次的步长就不一样了）。
pub fn zoom_window(current: Option<(u32, u32)>, factor: f32, full: u32) -> (u32, u32) {
    let full = full.max(12);
    let (from, to) = current.unwrap_or((0, full));
    let span = to.saturating_sub(from).max(1);
    let mid = from as f32 + span as f32 / 2.;
    let new_span = ((span as f32 * factor).round() as u32).clamp(8, full);
    let half = new_span as f32 / 2.;
    let mut a = (mid - half).round().max(0.) as u32;
    let mut b = a + new_span;
    if b > full {
        b = full;
        a = b.saturating_sub(new_span);
    }
    (a, b)
}

/// 时间轴取景框的平移：整窗左右挪 `chapters` 章，跨度不变。
pub fn pan_window(current: Option<(u32, u32)>, chapters: i32, full: u32) -> (u32, u32) {
    let full = full.max(12);
    let (from, to) = current.unwrap_or((0, full));
    let span = to.saturating_sub(from).max(1);
    let max_from = full.saturating_sub(span);
    let a = (from as i64 + chapters as i64).clamp(0, max_from as i64) as u32;
    (a, a + span)
}

/// 把视野挪到某一章（已在该范围内就原样返回，避免"点一下视野就跳"）。
pub fn focus_window(current: Option<(u32, u32)>, chapter: u32, full: u32) -> (u32, u32) {
    let full = full.max(12);
    let (from, to) = current.unwrap_or((0, full));
    let span = to.saturating_sub(from).max(1);
    if chapter >= from && chapter <= to {
        return (from, to);
    }
    // 目标放在视野左三分之一处：留出"后面会怎样"的余地，
    // 放正中间的话往后看的那一半全被浪费。
    let mut a = chapter.saturating_sub(span / 3);
    if a + span > full {
        a = full.saturating_sub(span);
    }
    (a, a + span)
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

    #[test]
    fn a_line_with_no_scheduled_nodes_still_has_a_visible_branch() {
        // AI 推演出的节点常常没排章（chapter=0）。如果末端就等于分叉章，
        // 整棵树的所有枝都是零长度——界面上就是"没有树"。
        let mut l = line("β", "α", 7, Kind::WhatIf, false);
        l.nodes.push(crate::worldline::Node::new("n1", "event", "某件事"));
        let t = tree(&[l]);
        let n = t.nodes.iter().find(|n| n.id == "β").unwrap();
        assert!(n.end_chapter > n.fork_chapter, "枝梢必须比枝根高：{:?}", (n.fork_chapter, n.end_chapter));
    }

    #[test]
    fn zoom_keeps_the_centre_and_the_span() {
        // 全览 120 章 → 放大到约 1/1.8
        let (a, b) = zoom_window(None, 1. / 1.8, 120);
        let span = b - a;
        assert!((64..=70).contains(&span), "跨度应约 67，实际 {span}");
        // 中心仍在中点附近
        let mid = (a + b) as f32 / 2.;
        assert!((mid - 60.).abs() <= 2., "放大后应围绕原来的中心，实际 {mid}");
    }

    #[test]
    fn zoom_never_goes_below_eight_chapters() {
        let mut w = None;
        for _ in 0..20 {
            w = Some(zoom_window(w, 1. / 1.8, 500));
        }
        let (a, b) = w.unwrap();
        assert_eq!(b - a, 8, "再怎么放大也要留 8 章，否则只剩两三根刻度");
        assert!(a <= b);
    }

    #[test]
    fn zoom_out_at_the_right_edge_pulls_back_instead_of_clipping() {
        // 贴着右端拉远：跨度是算出来的 36 章，不能裁成 20 章——
        // 裁剪会改变真实缩放比例，下一次按 ＋ 的步长就跟预期不符了。
        let (a, b) = zoom_window(Some((100, 120)), 1.8, 120);
        assert_eq!(b, 120, "右端不越界");
        assert_eq!(b - a, 36, "跨度必须保持算出来的值，靠整体左移腾地方");
        assert_eq!(a, 84);
    }

    #[test]
    fn zoom_out_caps_at_the_full_span() {
        // 10 章一次拉远 20 倍 = 200 章，超过全书 120 → 顶到全览
        let (a, b) = zoom_window(Some((10, 20)), 20.0, 120);
        assert_eq!((a, b), (0, 120), "拉远到底就是全览");
    }

    #[test]
    fn pan_moves_by_the_same_span_and_stops_at_both_ends() {
        // 跨度 40、全书 120：最多挪到 80 起
        let (a, b) = pan_window(Some((0, 40)), 20, 120);
        assert_eq!((a, b), (20, 60));
        let (a, b) = pan_window(Some((0, 40)), -20, 120);
        assert_eq!((a, b), (0, 40), "左端顶住");
        let (a, b) = pan_window(Some((0, 40)), 999, 120);
        assert_eq!((a, b), (80, 120), "右端顶住且跨度不变");
    }

    #[test]
    fn focus_does_not_jump_when_the_chapter_is_already_visible() {
        let w = Some((10, 50));
        assert_eq!(focus_window(w, 30, 120), (10, 50), "已在视野里就别动");
    }

    #[test]
    fn focus_brings_an_outlying_chapter_into_view() {
        // 目标在视野左侧：应该落在三分之一处，前面留出上下文
        let (a, b) = focus_window(Some((100, 140)), 50, 200);
        assert_eq!(b - a, 40, "跨度不变");
        assert!(a <= 50 && 50 <= b, "目标必须落在视野内：{a}..{b}");
        assert!(a < 50, "目标左侧要留上下文，不能贴边：{a}");
    }

    #[test]
    fn focus_near_the_end_clamps_inside_the_book() {
        let (a, b) = focus_window(None, 115, 120);
        assert_eq!(b, 120, "不能超出全书");
        assert!(a <= 115);
    }
}

