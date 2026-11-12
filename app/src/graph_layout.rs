//! 剧情画布的布局：以主角为中心的"基因图"。
//!
//! ## 为什么不是网格/自由摆放
//!
//! 画布上有几百个节点（人物、事件、伏笔、设定），一次性平铺成网格时，
//! 作者看不出"哪些是主线、哪些还飘在外面"——所有节点都长得一样，位置也不表达
//! 任何含义。而这张图画的是**剧情**，剧情天然是围绕主角长出来的：
//! 主角的动机牵出事件，事件牵出人物，人物再牵出伏笔。
//!
//! 所以布局按"离主角几步"分层：
//! - 第 0 层：主角本人，正中
//! - 第 1 层：直接跟主角有关的人/事（一环）
//! - 第 2 层：隔一层推进的（二环）
//! - 再往外：越远越接近"背景设定"
//!
//! ## 孤儿节点
//!
//! **连不上主角的节点不参与分层**——它们被收到图下方的"孤儿区"。
//! 这不是丢弃：一个"城防图"设定、一个还没登场的人物，本来就和当前主线无关。
//! 把它们混进环里会稀释主线；单独放一块，作者一眼能看到
//! "这些还没接上，要么补关系、要么就是背景"。
//!
//! 孤儿判定用连通性而不是距离阈值：隔 5 层但连着主线的仍然算主线的一部分。

/// 布局需要的节点信息（画布侧从台账拼出来）。
pub struct Meta<'a> {
    pub kind: &'a str,
    /// 显示名（用于稳定排序）
    pub label: &'a str,
    /// 人物角色（"主角" / "反派" …；非人物给空串）
    pub role: &'a str,
}

/// 布局结果：每个节点的左上角坐标（与画布 NODE_W/NODE_H 同一坐标系）。
#[derive(Debug, Clone, PartialEq)]
pub struct Layout {
    /// 与输入等长；`None` = 没排进去（正常不会出现，保留以防调用方漏项）
    pub pos: Vec<Option<(f32, f32)>>,
    /// 主角节点的下标（找不到就是 None）
    pub center: Option<usize>,
    /// 每个节点离主角几跳（`None` = 连不上 = 孤儿）
    pub ring: Vec<Option<u32>>,
    /// 孤儿节点下标（按名字排序，位置稳定）
    pub orphans: Vec<usize>,
    /// 整张图的包围盒（用于"适应窗口"）
    pub size: (f32, f32),
}

const NODE_W: f32 = 150.;
const NODE_H: f32 = 76.;
/// 每层之间的最小径向间距
const RING_GAP: f32 = 210.;
/// 同一环上两个节点沿弧线的最小间距（避免卡片重叠）
const ARC_GAP: f32 = 170.;
/// 孤儿区与主图之间的空隙
const ORPHAN_GAP: f32 = 150.;
/// 孤儿区内节点间距
const ORPHAN_STEP_X: f32 = 200.;
const ORPHAN_STEP_Y: f32 = NODE_H + 40.;

/// 计算基因图布局。
///
/// 纯函数：同样的输入永远得到同样的输出（节点顺序按名字定序，不依赖 HashMap
/// 的遍历顺序）。这条对画布很重要——每次刷新都跳一下位置的话，作者没法用手记住
/// "刚才那个伏笔在哪儿"。
pub fn gene_map(meta: &[Meta], edges: &[(usize, usize)]) -> Layout {
    let n = meta.len();
    let mut pos: Vec<Option<(f32, f32)>> = vec![None; n];
    let mut ring: Vec<Option<u32>> = vec![None; n];
    if n == 0 {
        return Layout { pos, center: None, ring, orphans: vec![], size: (0., 0.) };
    }

    // 邻接表（无向）
    let mut adj: Vec<Vec<usize>> = vec![Vec::new(); n];
    for &(a, b) in edges {
        if a < n && b < n && a != b {
            adj[a].push(b);
            adj[b].push(a);
        }
    }
    for list in &mut adj {
        list.sort_unstable();
        list.dedup();
    }

    // 主角：先看角色名，再看人物里度数最大的那个
    let center = meta
        .iter()
        .position(|m| m.kind == "character" && m.role.contains("主角"))
        .or_else(|| {
            (0..n)
                .filter(|i| meta[*i].kind == "character")
                .max_by_key(|i| adj[*i].len())
        });

    let Some(center) = center else {
        // 一个人物都没有：退化成"按类型分组的同心环"没有意义，
        // 直接按名字排成网格，至少是稳定可读的。
        return grid_fallback(meta, pos, ring);
    };

    // BFS 分层
    let mut dist: Vec<u32> = vec![u32::MAX; n];
    dist[center] = 0;
    let mut queue = std::collections::VecDeque::new();
    queue.push_back(center);
    while let Some(u) = queue.pop_front() {
        for &v in &adj[u] {
            if dist[v] == u32::MAX {
                dist[v] = dist[u] + 1;
                queue.push_back(v);
            }
        }
    }

    // 按层收人；同层内按 (类型, 名字) 排序，保证位置稳定
    let max_ring = dist.iter().filter(|d| **d != u32::MAX).copied().max().unwrap_or(0);
    let mut by_ring: Vec<Vec<usize>> = vec![Vec::new(); max_ring as usize + 1];
    let mut orphans: Vec<usize> = Vec::new();
    for i in 0..n {
        if dist[i] == u32::MAX {
            orphans.push(i);
            continue;
        }
        by_ring[dist[i] as usize].push(i);
    }
    for list in &mut by_ring {
        list.sort_by(|a, b| {
            meta[*a]
                .kind
                .cmp(meta[*b].kind)
                .then(meta[*a].label.cmp(meta[*b].label))
        });
    }
    orphans.sort_by(|a, b| meta[*a].label.cmp(meta[*b].label));

    // 边界用一个元组记 (min_x, min_y, max_x, max_y)。
    // 不用闭包是因为闭包会一直持有可变借用，后面就读不了 max_y 去算孤儿区基准线。
    let mut bounds = (f32::MAX, f32::MAX, f32::MIN, f32::MIN);

    // 中心
    put(&mut pos, &mut ring, &mut bounds, center, -NODE_W / 2., -NODE_H / 2., Some(0));
    // 各环：按类型分扇区铺开。
    //
    // 为什么不是均匀铺开：所有节点等角距排一圈时，人物会夹在事件中间，
    // 连线横穿整张图，看起来就是一团乱麻。按类型分扇区后，"人物在东南、
    // 事件在西南、伏笔在北"是固定的，同类连线只在本扇区内走，交叉少一大半——
    // 也才有了"基因图"那种可读的条带感。
    for (r, list) in by_ring.iter().enumerate() {
        if r == 0 || list.is_empty() {
            continue;
        }
        let k = list.len() as f32;
        let radius = (r as f32 * RING_GAP).max(k * ARC_GAP / (2. * std::f32::consts::PI));
        // 按类型分组（保持组内已按名字排好的顺序）
        let mut sectors: Vec<(&str, Vec<usize>)> = Vec::new();
        for &idx in list {
            let kind = meta[idx].kind;
            match sectors.iter_mut().find(|(k2, _)| *k2 == kind) {
                Some((_, v)) => v.push(idx),
                None => sectors.push((kind, vec![idx])),
            }
        }
        // 类型扇区的顺序固定：人物 → 事件 → 伏笔 → 设定 → 其它
        sectors.sort_by_key(|(kind, _)| kind_rank(kind));
        let total = k.max(1.);
        let mut cursor = -std::f32::consts::FRAC_PI_2;
        for (_, items) in &sectors {
            // 扇区宽度正比于条数；再留一点缝，免得两个扇区贴在一起看着是一坨
            let share = items.len() as f32 / total;
            let arc = 2. * std::f32::consts::PI * share;
            let n = items.len() as f32;
            for (i, &idx) in items.iter().enumerate() {
                let angle = cursor + arc * ((i as f32 + 0.5) / n);
                let cx = angle.cos() * radius;
                let cy = angle.sin() * radius;
                put(&mut pos, &mut ring, &mut bounds, idx, cx - NODE_W / 2., cy - NODE_H / 2., Some(r as u32));
            }
            cursor += arc;
        }
    }

    // 孤儿区：主图下方，按行铺开
    let base_y = bounds.3 + ORPHAN_GAP;
    if !orphans.is_empty() {
        // 每行最多 8 个：再多就换行，免得整张图被拉得极宽
        let per_row = 8usize;
        let cols = orphans.len().min(per_row);
        let row_width = cols as f32 * ORPHAN_STEP_X;
        let start_x = -row_width / 2.;
        for (i, &idx) in orphans.iter().enumerate() {
            let row = i / per_row;
            let col = i % per_row;
            put(
                &mut pos,
                &mut ring,
                &mut bounds,
                idx,
                start_x + col as f32 * ORPHAN_STEP_X,
                base_y + row as f32 * ORPHAN_STEP_Y,
                None,
            );
        }
    }

    // 平移到左上角 (0,0)：画布坐标从 0 起，视图偏移才好算
    let (min_x, min_y, max_x, max_y) = bounds;
    for p in pos.iter_mut().flatten() {
        p.0 -= min_x;
        p.1 -= min_y;
    }
    let size = (max_x - min_x, max_y - min_y);
    Layout { pos, center: Some(center), ring, orphans, size }
}

/// 类型在扇区里的固定次序。
///
/// 顺序本身有意义：人物先（剧情由人推动）、事件次之、伏笔再外、设定最外。
/// 固定次序保证同一本书每次打开图的"地貌"一样，作者能靠位置记忆找东西。
fn kind_rank(kind: &str) -> u8 {
    match kind {
        "character" => 0,
        "event" => 1,
        "clue" => 2,
        "storyline" => 3,
        "canon" => 4,
        _ => 5,
    }
}

/// 放一个节点并扩边界。
///
/// 独立成函数（而不是闭包）：闭包会持有 `pos`/`ring`/`bounds` 的可变借用直到
/// 作用域结束，后面就算只是**读** `bounds.3` 也会被拒。
fn put(
    pos: &mut [Option<(f32, f32)>],
    ring: &mut [Option<u32>],
    bounds: &mut (f32, f32, f32, f32),
    i: usize,
    x: f32,
    y: f32,
    r: Option<u32>,
) {
    pos[i] = Some((x, y));
    ring[i] = r;
    bounds.0 = bounds.0.min(x);
    bounds.1 = bounds.1.min(y);
    bounds.2 = bounds.2.max(x + NODE_W);
    bounds.3 = bounds.3.max(y + NODE_H);
}

/// 没有人物时的兜底：按类型分组的网格。
fn grid_fallback(
    meta: &[Meta],
    mut pos: Vec<Option<(f32, f32)>>,
    mut ring: Vec<Option<u32>>,
) -> Layout {
    let mut order: Vec<usize> = (0..meta.len()).collect();
    order.sort_by(|a, b| {
        meta[*a]
            .kind
            .cmp(meta[*b].kind)
            .then(meta[*a].label.cmp(meta[*b].label))
    });
    let cols = 5usize;
    let mut max_x = 0f32;
    let mut max_y = 0f32;
    for (i, &idx) in order.iter().enumerate() {
        let x = (i % cols) as f32 * (NODE_W + 60.);
        let y = (i / cols) as f32 * (NODE_H + 80.);
        pos[idx] = Some((x, y));
        ring[idx] = None;
        max_x = max_x.max(x + NODE_W);
        max_y = max_y.max(y + NODE_H);
    }
    Layout {
        pos,
        center: None,
        ring,
        orphans: order,
        size: (max_x, max_y),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn m(kind: &'static str, label: &'static str, role: &'static str) -> Meta<'static> {
        Meta { kind, label, role }
    }

    /// 主角-师父-仇敌 三点两线 + 一个孤儿设定。
    fn sample() -> (Vec<Meta<'static>>, Vec<(usize, usize)>) {
        (
            vec![
                m("character", "沈砚", "主角"),
                m("character", "老周", "师父"),
                m("character", "柳七", "仇敌"),
                m("canon", "城防图", ""),
            ],
            vec![(0, 1), (1, 2)],
        )
    }

    #[test]
    fn protagonist_sits_at_the_centre() {
        let (meta, edges) = sample();
        let l = gene_map(&meta, &edges);
        assert_eq!(l.center, Some(0));
        assert_eq!(l.ring[0], Some(0));
        // 判"居中"不能比对绝对坐标（整张图会平移到左上角），
        // 要比相对关系：所有第一环的节点离主角的距离应该彼此接近且都那么大。
        let (cx, cy) = centre_of(&l, 0);
        let r1: Vec<f32> = (0..meta.len())
            .filter(|i| l.ring[*i] == Some(1))
            .map(|i| {
                let (x, y) = centre_of(&l, i);
                ((x - cx).powi(2) + (y - cy).powi(2)).sqrt()
            })
            .collect();
        assert!(!r1.is_empty());
        let radius = r1[0];
        for r in &r1 {
            assert!((r - radius).abs() < 1., "第一环应在同一个半径上：{r1:?}");
        }
        assert!(radius > 100., "半径太小就读不出层次：{radius}");
    }

    /// 节点中心（pos 是左上角）。
    fn centre_of(l: &Layout, i: usize) -> (f32, f32) {
        let (x, y) = l.pos[i].unwrap();
        (x + NODE_W / 2., y + NODE_H / 2.)
    }

    #[test]
    fn distance_becomes_ring() {
        let (meta, edges) = sample();
        let l = gene_map(&meta, &edges);
        assert_eq!(l.ring[1], Some(1), "直接相关的人在第一环");
        assert_eq!(l.ring[2], Some(2), "隔一层的人在第二环");
    }

    #[test]
    fn nodes_not_connected_to_the_protagonist_are_orphans() {
        let (meta, edges) = sample();
        let l = gene_map(&meta, &edges);
        assert_eq!(l.orphans, vec![3], "城防图跟主线没关系，进孤儿区");
        assert_eq!(l.ring[3], None);
        // 孤儿必须在主图下方，不能混进环里
        let (_, cy) = l.pos[0].unwrap();
        let (_, oy) = l.pos[3].unwrap();
        assert!(oy > cy + 200., "孤儿要明显在主图下方：{oy} vs {cy}");
    }

    #[test]
    fn an_event_chain_keeps_the_ring_growing() {
        // 主角 → 事件A → 人物B → 伏笔C：三层，各自在 1/2/3 环
        let meta = vec![
            m("character", "主角", "主角"),
            m("event", "A", ""),
            m("character", "B", ""),
            m("clue", "C", ""),
        ];
        let edges = vec![(0, 1), (1, 2), (2, 3)];
        let l = gene_map(&meta, &edges);
        assert_eq!(l.ring[1], Some(1));
        assert_eq!(l.ring[2], Some(2));
        assert_eq!(l.ring[3], Some(3));
        assert!(l.orphans.is_empty());
    }

    #[test]
    fn same_kind_nodes_stay_in_one_sector() {
        // 一圈里有 2 人物 + 2 事件：同类应该连着排，而不是穿插。
        // 穿插会让人物-事件连线横穿整张图，图就变成一团麻。
        let meta = vec![
            m("character", "主角", "主角"),
            m("character", "甲", ""),
            m("character", "乙", ""),
            m("event", "E1", ""),
            m("event", "E2", ""),
        ];
        let edges = vec![(0, 1), (0, 2), (0, 3), (0, 4)];
        let l = gene_map(&meta, &edges);
        // 用相对主角的极角判断：甲、乙应相邻，E1、E2 应相邻
        let (cx, cy) = centre_of(&l, 0);
        let ang = |i: usize| {
            let (x, y) = centre_of(&l, i);
            (y - cy).atan2(x - cx)
        };
        let chars: Vec<f32> = vec![ang(1), ang(2)];
        let evs: Vec<f32> = vec![ang(3), ang(4)];
        // 两组各自的夹角应该小于 90°（同扇区内），不能铺满一圈
        let spread = |v: &[f32]| {
            let mut a = v.to_vec();
            a.sort_by(|x, y| x.partial_cmp(y).unwrap());
            let mut d = a[1] - a[0];
            if d > std::f32::consts::PI { d = 2. * std::f32::consts::PI - d; }
            d
        };
        // 每类分到的扇区宽 = 该类条数 / 总数 × 2π（这里是 π）。
        // 只要实际夹角没超出自己的扇区宽度，就说明没有和别的类穿插。
        assert!(spread(&chars) <= std::f32::consts::PI, "人物应聚在自己的扇区里");
        assert!(spread(&evs) <= std::f32::consts::PI, "事件应聚在自己的扇区里");
        // 更硬的一条：两类不能有重叠的角度区间
        let mut all = vec![(chars[0], "c"), (chars[1], "c"), (evs[0], "e"), (evs[1], "e")];
        all.sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap());
        let flips = all.windows(2).filter(|w| w[0].1 != w[1].1).count();
        assert!(flips <= 2, "不同类型必须成段出现，实际交界 {flips} 次");
    }

    #[test]
    fn layout_is_deterministic() {
        let (meta, edges) = sample();
        let a = gene_map(&meta, &edges);
        let b = gene_map(&meta, &edges);
        assert_eq!(a.pos, b.pos, "同样的数据两次布局必须完全一致");
    }

    #[test]
    fn ring_order_does_not_depend_on_input_order() {
        // 名字排序保证位置稳定：台账里条目顺序变了，图不该跟着抖
        let meta = vec![
            m("character", "主角", "主角"),
            m("character", "乙", ""),
            m("character", "甲", ""),
        ];
        let e1 = vec![(0, 1), (0, 2)];
        let l1 = gene_map(&meta, &e1);
        // 交换乙/甲的位置（相当于台账顺序变了）
        let meta2 = vec![
            m("character", "主角", "主角"),
            m("character", "甲", ""),
            m("character", "乙", ""),
        ];
        let e2 = vec![(0, 1), (0, 2)];
        let l2 = gene_map(&meta2, &e2);
        // 甲在两份输入里的下标不同，但坐标应该一致
        assert_eq!(l1.pos[2], l2.pos[1], "甲的位置应相同");
        assert_eq!(l1.pos[1], l2.pos[2], "乙的位置应相同");
    }

    #[test]
    fn no_protagonist_falls_back_to_a_grid() {
        let meta = vec![m("canon", "设定甲", ""), m("canon", "设定乙", "")];
        let l = gene_map(&meta, &[]);
        assert_eq!(l.center, None);
        assert_eq!(l.orphans.len(), 2, "没有人物时全部按网格摆，不做分层");
        assert!(l.pos.iter().all(|p| p.is_some()));
    }

    #[test]
    fn empty_input_is_safe() {
        let l = gene_map(&[], &[]);
        assert_eq!(l.size, (0., 0.));
        assert!(l.center.is_none());
    }

    #[test]
    fn a_dense_ring_gets_a_larger_radius() {
        // 一个主角带 12 个直接相关的人：环半径要撑到不重叠
        let mut meta = vec![m("character", "主角", "主角")];
        let mut edges = Vec::new();
        for i in 0..12 {
            meta.push(m("character", Box::leak(format!("n{i:02}").into_boxed_str()), ""));
            edges.push((0, i + 1));
        }
        let l = gene_map(&meta, &edges);
        // 圆周上对称位置的 x 会相同（cos 对称），所以要比二维距离而不是比 x
        let mut min_gap = f32::MAX;
        for i in 1..=12 {
            for j in (i + 1)..=12 {
                let (ax, ay) = centre_of(&l, i);
                let (bx, by) = centre_of(&l, j);
                let d = ((ax - bx).powi(2) + (ay - by).powi(2)).sqrt();
                min_gap = min_gap.min(d);
            }
        }
        assert!(min_gap > 60., "同环节点不能叠在一起，最近两个相距 {min_gap}");
    }
}

#[cfg(test)]
mod probe_real {
    use super::*;
    #[test]
    fn probe_book_overview() {
        let root = std::path::Path::new("/Users/kelthas/Documents/执笔/断刀记验证.zhibi");
        if !root.exists() { return; }
        let roster = crate::characters::Roster::new(root);
        let people = roster.all();
        eprintln!("characters: {}", people.len());
        for c in people.iter().take(5) { eprintln!("  {} role={:?} minor={}", c.name, c.role, c.minor); }
    }
}
