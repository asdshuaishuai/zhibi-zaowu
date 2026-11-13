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
    // 力导向摆放（Fruchterman-Reingold）。
    //
    // 为什么不再按环铺：环上每个节点只跟圆心有关，节点之间互相不知道对方在哪，
    // 于是"人物在这边、他牵涉的事件在对面"，连线横穿整张图——就是一团乱麻。
    // FR 让相连的互相拉近、所有节点互相排斥，得到的是关系图的标准形态：
    // 抱团的抱团、孤立的散在外面，边短且少交叉。
    //
    // 主角钉在原点：作者要的是"围绕主角"，不能让力把它推到角落。
    // 初始摆放的名次：按 (类型, 名字) 排，与台账顺序无关
    let mut order_key: Vec<usize> = (0..n).collect();
    order_key.sort_by(|a, b| {
        kind_rank(meta[*a].kind)
            .cmp(&kind_rank(meta[*b].kind))
            .then(meta[*a].label.cmp(meta[*b].label))
    });
    let placed = force_layout(n, &adj, Some(center), &order_key);
    let (mut min_x, mut min_y) = (f32::MAX, f32::MAX);
    let (mut max_x, mut max_y) = (f32::MIN, f32::MIN);
    for (i, p) in placed.iter().enumerate() {
        pos[i] = Some((p.0 - NODE_W / 2., p.1 - NODE_H / 2.));
        ring[i] = if dist[i] == u32::MAX { None } else { Some(dist[i]) };
        min_x = min_x.min(p.0 - NODE_W / 2.);
        min_y = min_y.min(p.1 - NODE_H / 2.);
        max_x = max_x.max(p.0 + NODE_W / 2.);
        max_y = max_y.max(p.1 + NODE_H / 2.);
    }
    let mut bounds = (min_x, min_y, max_x, max_y);

    // 孤儿区：主图下方，按行铺开
    let base_y = bounds.3 + ORPHAN_GAP;
    if !orphans.is_empty() {
        let per_row = 8usize;
        let cols = orphans.len().min(per_row);
        let row_width = cols as f32 * ORPHAN_STEP_X;
        let start_x = min_x + (bounds.2 - min_x - row_width) / 2.;
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

    // 平移到左上角 (0,0)
    let (min_x, min_y, max_x, max_y) = bounds;
    for p in pos.iter_mut().flatten() {
        p.0 -= min_x;
        p.1 -= min_y;
    }
    let size = (max_x - min_x, max_y - min_y);
    Layout { pos, center: Some(center), ring, orphans, size }
}

/// 力导向布局（Fruchterman-Reingold）。
///
/// 参数按"卡片是 150×76 的矩形"来定：理想边长 `k` 取 210，
/// 这样相连的节点大致隔一张卡，不叠在一起也不至于隔着半屏。
///
/// 三条实现约定：
/// 1. **确定性**：初始位置固定摆在一个圆上，不用随机数——同一本书每次打开
///    图的"地貌"一样，作者才能靠位置记忆找东西。
/// 2. **主角钉住**：`pinned` 的节点不参与位移，永远在原点。
/// 3. **冷却**：每轮最大位移线性衰减，最后几轮只做微调，不会来回甩。
fn force_layout(
    n: usize,
    adj: &[Vec<usize>],
    pinned: Option<usize>,
    order_key: &[usize],
) -> Vec<(f32, f32)> {
    // 初始位置按"名字排序后的名次"摆，而不是数组下标。
    // 台账里条目的顺序会变（新增一条就把后面全挤一位），如果按下标摆，
    // 同一本书两次打开图的形状会不一样——作者没法靠位置记忆找东西。
    let mut pos: Vec<(f32, f32)> = vec![(0., 0.); n];
    for (rank, &i) in order_key.iter().enumerate() {
        // 黄金角螺旋：比正方形/圆环更均匀，不会一开始就出现"两堆"
        let a = rank as f32 * 2.399_963_2;
        let r = 140. * (rank as f32 + 0.5).sqrt();
        pos[i] = (a.cos() * r, a.sin() * r);
    }
    if n <= 1 {
        return pos;
    }
    // 理想边长。取 175 而不是更大的值：作者一屏要看到"整张关系网"才有意义，
    // 边长越大图越舒展、但也越难一屏看全（50 个节点时差一倍）。
    let k = 175.0f32;
    // FR 的边界框：节点被关在 W×H 里。**没有这条就必然发散**——
    // 每个节点受几十个邻居的斥力，合力常年顶在上限，位置一轮轮往外漂，
    // 实测能漂到 8 万像素宽（整张图变成几个孤点）。
    let frame = (n as f32).sqrt() * k * 1.2;
    let half = frame / 2.;
    let iterations = 420;
    // 位移上限从 k 线性降到 k*0.02
    for it in 0..iterations {
        let t = k * (1.0 - it as f32 / iterations as f32).max(0.02);
        let mut disp = vec![(0.0f32, 0.0f32); n];

        // 排斥：所有节点两两相斥（O(n²)，n 是几十到几百，够用）
        for i in 0..n {
            for j in (i + 1)..n {
                let dx = pos[i].0 - pos[j].0;
                let dy = pos[i].1 - pos[j].1;
                let d2 = (dx * dx + dy * dy).max(1.0);
                let d = d2.sqrt();
                let f = k * k / d;
                let ux = dx / d;
                let uy = dy / d;
                disp[i].0 += ux * f;
                disp[i].1 += uy * f;
                disp[j].0 -= ux * f;
                disp[j].1 -= uy * f;
            }
        }

        // 吸引：只在有边的节点之间
        for i in 0..n {
            for &j in &adj[i] {
                if j <= i {
                    continue;
                }
                let dx = pos[i].0 - pos[j].0;
                let dy = pos[i].1 - pos[j].1;
                let d = (dx * dx + dy * dy).sqrt().max(1.0);
                let f = d * d / k;
                let ux = dx / d;
                let uy = dy / d;
                disp[i].0 -= ux * f;
                disp[i].1 -= uy * f;
                disp[j].0 += ux * f;
                disp[j].1 += uy * f;
            }
        }

        // 位移（限制在 t 以内），钉住的节点不动、也不许出框
        for i in 0..n {
            if Some(i) == pinned {
                continue;
            }
            let (dx, dy) = disp[i];
            let d = (dx * dx + dy * dy).sqrt().max(1e-6);
            let s = d.min(t);
            pos[i].0 = (pos[i].0 + dx / d * s).clamp(-half, half);
            pos[i].1 = (pos[i].1 + dy / d * s).clamp(-half, half);
        }
    }

    // 去重叠：FR 只保证"大致分开"，卡片有实际尺寸，再推几轮确保不叠
    for _ in 0..60 {
        let mut moved = false;
        for i in 0..n {
            for j in (i + 1)..n {
                let dx = pos[j].0 - pos[i].0;
                let dy = pos[j].1 - pos[i].1;
                // 卡片的包围盒 + 间距
                let need_x = NODE_W + 26.;
                let need_y = NODE_H + 26.;
                if dx.abs() < need_x && dy.abs() < need_y {
                    // 沿重叠较小的轴推开，位移自然
                    let ox = need_x - dx.abs();
                    let oy = need_y - dy.abs();
                    let (sx, sy) = (if dx >= 0. { 1. } else { -1. }, if dy >= 0. { 1. } else { -1. });
                    if ox < oy {
                        let push = ox * 0.5 * sx;
                        if Some(i) != pinned { pos[i].0 -= push; }
                        if Some(j) != pinned { pos[j].0 += push; }
                    } else {
                        let push = oy * 0.5 * sy;
                        if Some(i) != pinned { pos[i].1 -= push; }
                        if Some(j) != pinned { pos[j].1 += push; }
                    }
                    moved = true;
                }
            }
        }
        if !moved {
            break;
        }
    }
    // 主角钉在原点，其余整体以它为中心（避免力把整图推歪）
    if let Some(c) = pinned {
        let (cx, cy) = pos[c];
        for p in pos.iter_mut() {
            p.0 -= cx;
            p.1 -= cy;
        }
    }
    pos
}

/// 类型在排序里的固定次序。
///
/// 只影响**初始摆放**与同分时的次序，不表达"哪类更重要"：
/// 人物先（剧情由人推动）、事件、伏笔、故事线、设定。
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
    fn connected_nodes_end_up_closer_than_unconnected_ones() {
        // 力导向的核心承诺：有边的一对平均比没边的近。
        // 这不只是"好不好看"——关系图要能读出"谁跟谁一伙"，
        // 靠的就是这个距离差。
        let meta = vec![
            m("character", "主角", "主角"),
            m("character", "甲", ""),
            m("character", "乙", ""),
            m("character", "丙", ""),
            m("character", "丁", ""),
            m("character", "戊", ""),
        ];
        // 甲、乙与主角相连；丙、丁、戊互不相连
        let edges = vec![(0, 1), (0, 2)];
        let l = gene_map(&meta, &edges);
        let dist = |a: usize, b: usize| {
            let (ax, ay) = centre_of(&l, a);
            let (bx, by) = centre_of(&l, b);
            ((ax - bx).powi(2) + (ay - by).powi(2)).sqrt()
        };
        let near = (dist(0, 1) + dist(0, 2)) / 2.;
        let far = (dist(3, 4) + dist(3, 5) + dist(4, 5)) / 3.;
        assert!(near < far, "相连的应更近：near={near:.0} far={far:.0}");
    }

    #[test]
    fn nodes_do_not_overlap() {
        // 卡片是 150×76 的矩形，任何两张都不能压在一起——
        // 压住就是"看不清关系"。
        let (meta, edges) = sample();
        let l = gene_map(&meta, &edges);
        let mut ms = f32::MAX;
        for i in 0..meta.len() {
            for j in (i + 1)..meta.len() {
                if l.ring[i].is_none() != l.ring[j].is_none() {
                    continue; // 主图与孤儿区分开检查
                }
                let (ax, ay) = centre_of(&l, i);
                let (bx, by) = centre_of(&l, j);
                ms = ms.min((ax - bx).abs().max((ay - by).abs()));
            }
        }
        assert!(ms > 40., "节点间距过小：{ms}");
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
