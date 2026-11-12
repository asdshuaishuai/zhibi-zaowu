//! 关系网：人物之间“从第几章到第几章、是什么关系”。
//!
//! 为什么要独立的 `relations.json`：原来关系是 `memory.json` 里的 facts 三元组
//! （subject/predicate/object + fromChapter）。它有三个硬伤：
//!
//! 1. **没有终点**。师徒关系在第 20 章决裂之后，三元组仍然显示“师徒”。
//! 2. **同三元组无法演变**。去重键只看 subject/predicate/object，所以“同一个人
//!    第 5 章和第 30 章的关系不同”根本存不进去。
//! 3. **读者可见性没被用上**。facts 有 publicToReader，但画布关系维从来没读过。
//!
//! 新模型：一条关系有多个**区间**（interval），每段有自己的起止章与可见性。
//! 这样“第 5-19 章是师徒，第 20 章后是仇敌”是可表达的，章节滑块也能正确显示
//! “此时此刻他们是什么关系”。
//!
//! 兼容：旧的 facts 里 `source=relation` 或能归一成人名的三元组，迁移时转成
//! 单区间关系（fromChapter 起、无终点），原 facts 不动。

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

/// 一段关系成立的时间区间。
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", default)]
pub struct Interval {
    /// 从第几章开始（含）。0 表示“一开始就有”。
    pub from_chapter: u32,
    /// 到第几章结束（不含）。0 或缺失 = 至今仍成立。
    pub to_chapter: u32,
    /// 这一段的谓词：师徒/敌对/夫妻…
    pub label: String,
    /// 读者此刻是否已知（暗线 = false）。
    pub public_to_reader: bool,
    /// 依据：哪一章哪一段。
    #[serde(default)]
    pub evidence: String,
}

impl Interval {
    /// 第 `chapter` 章时这段关系是否成立。
    pub fn covers(&self, chapter: u32) -> bool {
        let started = self.from_chapter == 0 || chapter >= self.from_chapter;
        let not_ended = self.to_chapter == 0 || chapter < self.to_chapter;
        started && not_ended
    }

    /// 区间合法性：终点不能早于起点（0 表示开放，不算非法）。
    pub fn is_valid(&self) -> bool {
        self.to_chapter == 0 || self.to_chapter > self.from_chapter
    }
}

/// 两人（或一人与一物/一势力）之间的一段关系史。
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", default)]
pub struct Relation {
    pub id: String,
    /// 主体（本名优先）。
    pub from: String,
    /// 客体。
    pub to: String,
    /// 分段：按 fromChapter 升序。
    #[serde(default)]
    pub intervals: Vec<Interval>,
    /// 关系是否有方向（师徒有，同乡无）。
    #[serde(default)]
    pub directed: bool,
    /// 关系类型（粗分类，用于着色）：亲缘/师承/敌对/情爱/利益/其他。
    #[serde(default)]
    pub kind: String,
    #[serde(default)]
    pub note: String,
}

impl Relation {
    pub fn new(from: &str, to: &str, kind: &str) -> Self {
        Self {
            id: format!("rel-{}", unique_suffix()),
            from: from.to_string(),
            to: to.to_string(),
            kind: kind.to_string(),
            directed: true,
            ..Default::default()
        }
    }

    /// 第 `chapter` 章时生效的区间（可能有多段同时成立，如“师门”与“敌对”并存）。
    pub fn active_at(&self, chapter: u32) -> Vec<&Interval> {
        self.intervals.iter().filter(|i| i.covers(chapter)).collect()
    }

    /// 排序并丢掉非法/空区间。
    pub fn normalize(&mut self) {
        self.intervals.retain(|i| i.is_valid() && !i.label.trim().is_empty());
        self.intervals.sort_by_key(|i| i.from_chapter);
    }

    /// 当前（最新章节）的关系标签。测试与后续「关系演变视图」用；
    /// 画布侧直接读 active_at 以便同时画出并存的区间。
    #[allow(dead_code)]
    pub fn current_label(&self, chapter: u32) -> String {
        let active = self.active_at(chapter);
        active
            .iter()
            .map(|i| i.label.clone())
            .collect::<Vec<_>>()
            .join(" / ")
    }
}

fn unique_suffix() -> String {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    format!("{nanos:x}")
}

/// `relations.json` 读写。
pub struct Web {
    root: PathBuf,
}

impl Web {
    pub fn new(book_root: &Path) -> Self {
        Self {
            root: book_root.to_path_buf(),
        }
    }

    fn path(&self) -> PathBuf {
        self.root.join("relations.json")
    }

    pub fn all(&self) -> Vec<Relation> {
        std::fs::read_to_string(self.path())
            .ok()
            .and_then(|t| serde_json::from_str::<Vec<Relation>>(&t).ok())
            .unwrap_or_default()
    }

    pub fn save(&self, list: &[Relation]) -> Result<(), String> {
        let json = serde_json::to_string_pretty(list).map_err(|e| e.to_string())?;
        let tmp = self.path().with_extension("tmp");
        std::fs::write(&tmp, json).map_err(|e| e.to_string())?;
        std::fs::rename(&tmp, self.path()).map_err(|e| e.to_string())
    }

    /// 整条关系替换。手工编辑表单目前走 add_interval（追加区间），
    /// 整条替换留给后续「编辑已有区间」的场景。
    #[allow(dead_code)]
    pub fn upsert(&self, mut rel: Relation) -> Result<(), String> {
        rel.normalize();
        let mut list = self.all();
        match list.iter().position(|r| r.id == rel.id) {
            Some(i) => list[i] = rel,
            None => list.push(rel),
        }
        self.save(&list)
    }

    pub fn remove(&self, id: &str) -> Result<(), String> {
        let mut list = self.all();
        list.retain(|r| r.id != id);
        self.save(&list)
    }

    /// 某章生效的全部关系（画布关系维与章节滑块读它）。
    pub fn at_chapter(&self, chapter: u32) -> Vec<Relation> {
        self.all()
            .into_iter()
            .filter(|r| !r.active_at(chapter).is_empty())
            .collect()
    }

    /// 给某人加一段关系区间。同一对人物已存在关系时往里追加区间，
    /// 而不是新建一条——这样“师徒 → 决裂”是同一段关系史的两段。
    pub fn add_interval(
        &self,
        from: &str,
        to: &str,
        kind: &str,
        interval: Interval,
    ) -> Result<String, String> {
        if !interval.is_valid() {
            return Err("区间非法：结束章不能早于起始章".into());
        }
        if interval.label.trim().is_empty() {
            return Err("关系标签不能为空".into());
        }
        let mut list = self.all();
        // 找到同一对（有方向时按方向匹配，无向时两个方向都算）
        let found = list.iter().position(|r| {
            if r.directed {
                r.from == from && r.to == to
            } else {
                (r.from == from && r.to == to) || (r.from == to && r.to == from)
            }
        });
        if let Some(i) = found {
            let id = list[i].id.clone();
            list[i].intervals.push(interval);
            list[i].normalize();
            self.save(&list)?;
            Ok(id)
        } else {
            let mut rel = Relation::new(from, to, kind);
            rel.intervals.push(interval);
            rel.normalize();
            let id = rel.id.clone();
            list.push(rel);
            self.save(&list)?;
            Ok(id)
        }
    }

    /// 从 memory.json 的 facts 迁移。
    ///
    /// 只迁移**看起来是人名**的宾语，否则“获得·断刀”会被造出一条人物关系。
    /// 规则：subject 一律建；object 只有谓词属于关系词表、或已在已知人物集合里才建。
    /// 原 facts 不动，迁移幂等（已存在的同对同标签不重复加）。
    pub fn migrate_from_facts(
        &self,
        facts: &[serde_json::Value],
        known_people: &[String],
    ) -> usize {
        const RELATIONAL: &[&str] = &[
            "师从", "师父", "徒弟", "父子", "父女", "母子", "母女", "兄弟", "姐妹", "夫妻",
            "敌对", "仇敌", "同乡", "共谋", "暗恋", "主仆", "盟友", "部下", "上司", "相识",
            "结义", "背叛", "信任", "利用", "喜欢", "恨", "救过", "杀过", "暗杀",
        ];
        let mut list = self.all();
        let mut added = 0usize;
        for f in facts {
            let Some(subject) = f.get("subject").and_then(|v| v.as_str()) else {
                continue;
            };
            let Some(predicate) = f.get("predicate").and_then(|v| v.as_str()) else {
                continue;
            };
            let Some(object) = f.get("object").and_then(|v| v.as_str()) else {
                continue;
            };
            let is_relation = RELATIONAL.contains(&predicate);
            let known_object = known_people.iter().any(|k| k == object);
            if !is_relation && !known_object {
                continue;
            }
            let chapter = f
                .get("fromChapter")
                .and_then(|c| c.as_u64())
                .unwrap_or(0) as u32;
            let public = f
                .get("publicToReader")
                .and_then(|p| p.as_bool())
                .unwrap_or(true);
            // 同一对 + 同一标签 + 同一起点已存在就跳过（幂等）
            let dup = list.iter().any(|r| {
                r.from == subject
                    && r.to == object
                    && r.intervals
                        .iter()
                        .any(|i| i.label == predicate && i.from_chapter == chapter)
            });
            if dup {
                continue;
            }
            match list
                .iter()
                .position(|r| r.from == subject && r.to == object)
            {
                Some(i) => list[i].intervals.push(Interval {
                    from_chapter: chapter,
                    to_chapter: 0,
                    label: predicate.to_string(),
                    public_to_reader: public,
                    evidence: String::new(),
                }),
                None => {
                    let kind = classify(predicate);
                    let mut rel = Relation::new(subject, object, &kind);
                    rel.intervals.push(Interval {
                        from_chapter: chapter,
                        to_chapter: 0,
                        label: predicate.to_string(),
                        public_to_reader: public,
                        evidence: String::new(),
                    });
                    list.push(rel);
                }
            }
            added += 1;
        }
        if added > 0 {
            for r in &mut list {
                r.normalize();
            }
            let _ = self.save(&list);
        }
        added
    }
}

/// 关系演变视图的一行：某段区间在时间轴上的位置。
///
/// 为什么单独算而不是直接读 intervals：作者要看到的是"这条关系在整条时间轴上
/// 占哪一段、中间有没有断档"。裸 intervals 告诉不了这个——两段之间隔了 10 章
/// 的空白，在列表里看不出来，在时间轴上是个明显的缺口。
#[derive(Debug, Clone, PartialEq)]
pub struct EvolutionRow {
    /// 关系 id（删的时候要用）
    pub rid: String,
    /// 对方的名字
    pub other: String,
    /// 关系粗分类（着色用）
    pub kind: String,
    /// 每一段：起、止（0 = 至今）、标签、读者是否已知
    pub spans: Vec<(u32, u32, String, bool)>,
    /// 这条关系第一次出现的章
    pub first: u32,
    /// 最后一次仍在生效的章（0 = 至今）
    pub last: u32,
    /// 中间是否有断档（两段之间空了至少一章）
    pub has_gap: bool,
}

impl EvolutionRow {
    /// 一段区间的简略章节写法：`5–19` / `20–`（至今）。
    ///
    /// 侧板只有 226px 宽，"第 5 章到第 19 章"这种写法两段就折行了；
    /// 作者在时间轴上要的是相对位置，章号越短越好读。
    pub fn span_text(from: u32, to: u32) -> String {
        let a = if from == 0 { 1 } else { from };
        if to == 0 {
            format!("{a}–")
        } else {
            format!("{a}–{to}")
        }
    }

    /// 整条关系的简略写法：`5–19 → 20–`。
    ///
    /// 用 `→` 而不是逗号：它表达的是"先这样、后来那样"，逗号读起来像并列。
    pub fn brief(&self) -> String {
        self.spans
            .iter()
            .map(|(a, b, _, _)| Self::span_text(*a, *b))
            .collect::<Vec<_>>()
            .join(" → ")
    }
}

/// 某个人的关系演变（按"第一次出现"排序）。
///
/// `horizon` 是最新成稿章：开放的区间（to=0）画到 horizon。
pub fn evolution(web: &Web, person: &str, horizon: u32) -> Vec<EvolutionRow> {
    let mut out: Vec<EvolutionRow> = web
        .all()
        .into_iter()
        .filter(|r| r.from == person || r.to == person)
        .map(|r| {
            let other = if r.from == person { r.to.clone() } else { r.from.clone() };
            let mut spans: Vec<(u32, u32, String, bool)> = r
                .intervals
                .iter()
                .map(|i| (i.from_chapter, i.to_chapter, i.label.clone(), i.public_to_reader))
                .collect();
            spans.sort_by_key(|(a, _, _, _)| *a);
            let first = spans.iter().map(|(a, _, _, _)| *a).min().unwrap_or(0);
            let last = spans
                .iter()
                .map(|(_, b, _, _)| *b)
                .filter(|b| *b > 0)
                .max()
                .unwrap_or(0);
            // 断档：相邻两段之间，前一段有明确终点、后一段起点明显更晚
            let has_gap = spans.windows(2).any(|w| {
                let end_prev = w[0].1;
                end_prev > 0 && w[1].0 > end_prev
            });
            EvolutionRow { rid: r.id.clone(), other, kind: r.kind.clone(), spans, first, last, has_gap }
        })
        .collect();
    out.sort_by(|a, b| a.first.cmp(&b.first).then(a.other.cmp(&b.other)));
    let _ = horizon;
    out
}

/// 谓词 → 粗分类（用于边着色）。
pub fn classify(predicate: &str) -> String {
    const KIN: &[&str] = &[
        "父子", "父女", "母子", "母女", "兄弟", "姐妹", "夫妻", "亲族", "叔", "舅", "姨",
    ];
    const MENTOR: &[&str] = &["师从", "师父", "徒弟", "门下", "传人", "授业"];
    const HOSTILE: &[&str] = &["敌对", "仇敌", "背叛", "杀过", "暗杀", "宿敌", "追杀"];
    const LOVE: &[&str] = &["暗恋", "喜欢", "爱慕", "夫妻", "情人"];
    const INTEREST: &[&str] = &["利用", "共谋", "交易", "主仆", "部下", "上司", "盟友"];
    if KIN.iter().any(|k| predicate.contains(k)) {
        "亲缘".into()
    } else if MENTOR.iter().any(|k| predicate.contains(k)) {
        "师承".into()
    } else if HOSTILE.iter().any(|k| predicate.contains(k)) {
        "敌对".into()
    } else if LOVE.iter().any(|k| predicate.contains(k)) {
        "情爱".into()
    } else if INTEREST.iter().any(|k| predicate.contains(k)) {
        "利益".into()
    } else {
        "其他".into()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn book(tag: &str) -> PathBuf {
        let p = std::env::temp_dir().join(format!("zhibi-rel-{tag}"));
        let _ = std::fs::remove_dir_all(&p);
        std::fs::create_dir_all(&p).unwrap();
        p
    }

    #[test]
    fn interval_covers_chapter_range() {
        let i = Interval {
            from_chapter: 5,
            to_chapter: 20,
            label: "师徒".into(),
            ..Default::default()
        };
        assert!(!i.covers(4), "第 4 章还没拜师");
        assert!(i.covers(5));
        assert!(i.covers(19));
        assert!(!i.covers(20), "第 20 章已决裂");
        // 0 = 开放终点
        let open = Interval { from_chapter: 5, to_chapter: 0, ..Default::default() };
        assert!(open.covers(999));
    }

    #[test]
    fn same_pair_can_evolve_across_chapters() {
        let root = book("evolve");
        let w = Web::new(&root);
        // 第 5 章拜师、第 20 章决裂——同一对人物，两段
        w.add_interval("沈砚", "老周", "师承", Interval {
            from_chapter: 5, to_chapter: 20, label: "师徒".into(),
            public_to_reader: true, evidence: String::new(),
        }).unwrap();
        w.add_interval("沈砚", "老周", "敌对", Interval {
            from_chapter: 20, to_chapter: 0, label: "仇敌".into(),
            public_to_reader: false, evidence: String::new(),
        }).unwrap();

        assert_eq!(w.all().len(), 1, "同一对人物只有一条关系史");
        // 第 10 章：师徒
        let at10 = w.at_chapter(10);
        assert_eq!(at10[0].current_label(10), "师徒");
        // 第 25 章：仇敌
        let at25 = w.at_chapter(25);
        assert_eq!(at25[0].current_label(25), "仇敌");
        // 第 2 章：还没有关系
        assert!(w.at_chapter(2).is_empty());
        // 第 25 章这段是暗线，读者不可见
        assert!(!at25[0].active_at(25)[0].public_to_reader);
    }

    #[test]
    fn rejects_invalid_interval() {
        let root = book("invalid");
        let w = Web::new(&root);
        let bad = Interval {
            from_chapter: 20, to_chapter: 5, label: "师徒".into(), ..Default::default()
        };
        assert!(w.add_interval("A", "B", "师承", bad).is_err(), "终点早于起点应拒绝");
        let empty = Interval { from_chapter: 1, to_chapter: 0, label: "  ".into(), ..Default::default() };
        assert!(w.add_interval("A", "B", "师承", empty).is_err(), "空标签应拒绝");
    }

    #[test]
    fn migration_skips_objects_that_are_not_people() {
        let root = book("migrate");
        let w = Web::new(&root);
        let facts = vec![
            serde_json::json!({"subject":"沈砚","predicate":"获得","object":"断刀","fromChapter":1}),
            serde_json::json!({"subject":"沈砚","predicate":"师从","object":"老周","fromChapter":5}),
        ];
        let added = w.migrate_from_facts(&facts, &[]);
        assert_eq!(added, 1, "只迁移真正的两人关系");
        let all = w.all();
        assert_eq!(all.len(), 1);
        assert_eq!(all[0].from, "沈砚");
        assert_eq!(all[0].to, "老周");
        assert_eq!(all[0].kind, "师承", "谓词应被归类用于着色");
    }

    #[test]
    fn migration_is_idempotent() {
        let root = book("idem");
        let w = Web::new(&root);
        let facts = vec![
            serde_json::json!({"subject":"沈砚","predicate":"师从","object":"老周","fromChapter":5}),
        ];
        assert_eq!(w.migrate_from_facts(&facts, &[]), 1);
        assert_eq!(w.migrate_from_facts(&facts, &[]), 0, "重复迁移不该再增");
    }

    #[test]
    fn brief_uses_short_chapter_ranges() {
        // 侧板只有 226px 宽："第 5 章到第 19 章"两段就折行。
        assert_eq!(EvolutionRow::span_text(5, 19), "5–19");
        assert_eq!(EvolutionRow::span_text(20, 0), "20–", "至今写成开口区间");
        assert_eq!(EvolutionRow::span_text(0, 0), "1–", "起章 0 当成第 1 章");
        let row = EvolutionRow {
            rid: "r1".into(),
            other: "老周".into(),
            kind: "师承".into(),
            spans: vec![
                (5, 19, "师徒".into(), true),
                (20, 0, "仇敌".into(), false),
            ],
            first: 5,
            last: 0,
            has_gap: false,
        };
        assert_eq!(row.brief(), "5–19 → 20–");
    }

    #[test]
    fn evolution_marks_a_gap_between_two_spans() {
        let root = book("evolve-gap");
        let w = Web::new(&root);
        // 第 5-10 章师徒，第 10-20 章断了，第 20 章起仇敌
        w.add_interval("沈砚", "老周", "师承", Interval {
            from_chapter: 5, to_chapter: 10, label: "师徒".into(),
            public_to_reader: true, evidence: String::new(),
        }).unwrap();
        w.add_interval("沈砚", "老周", "敌对", Interval {
            from_chapter: 20, to_chapter: 0, label: "仇敌".into(),
            public_to_reader: false, evidence: String::new(),
        }).unwrap();

        let rows = evolution(&w, "沈砚", 40);
        assert_eq!(rows.len(), 1);
        let r = &rows[0];
        assert_eq!(r.other, "老周");
        assert_eq!(r.spans.len(), 2);
        assert_eq!(r.first, 5);
        assert!(r.has_gap, "第 10-20 章之间有 10 章空白，必须标出来");
        // 暗线标记要带出来（画布上要画紫虚线）
        assert!(!r.spans[1].3, "仇敌那一段是读者未知");
    }

    #[test]
    fn evolution_without_gap_when_spans_touch() {
        let root = book("evolve-touch");
        let w = Web::new(&root);
        w.add_interval("A", "B", "师承", Interval {
            from_chapter: 1, to_chapter: 10, label: "师徒".into(),
            public_to_reader: true, evidence: String::new(),
        }).unwrap();
        w.add_interval("A", "B", "敌对", Interval {
            from_chapter: 10, to_chapter: 0, label: "仇敌".into(),
            public_to_reader: true, evidence: String::new(),
        }).unwrap();
        let rows = evolution(&w, "A", 30);
        assert!(!rows[0].has_gap, "首尾相接不算断档");
    }

    #[test]
    fn evolution_lists_both_directions_of_a_relation() {
        let root = book("evolve-both");
        let w = Web::new(&root);
        w.add_interval("甲", "乙", "其他", Interval {
            from_chapter: 1, to_chapter: 0, label: "相识".into(),
            public_to_reader: true, evidence: String::new(),
        }).unwrap();
        // 从两边的角度都能看到这条关系
        assert_eq!(evolution(&w, "甲", 10).len(), 1);
        assert_eq!(evolution(&w, "乙", 10).len(), 1);
        assert_eq!(evolution(&w, "丙", 10).len(), 0);
    }

    #[test]
    fn evolution_sorts_by_first_appearance() {
        let root = book("evolve-order");
        let w = Web::new(&root);
        w.add_interval("我", "后认识", "其他", Interval {
            from_chapter: 20, to_chapter: 0, label: "相识".into(),
            public_to_reader: true, evidence: String::new(),
        }).unwrap();
        w.add_interval("我", "先认识", "其他", Interval {
            from_chapter: 3, to_chapter: 0, label: "相识".into(),
            public_to_reader: true, evidence: String::new(),
        }).unwrap();
        let rows = evolution(&w, "我", 30);
        assert_eq!(rows[0].other, "先认识", "先出现的排前面");
        assert_eq!(rows[1].other, "后认识");
    }

    #[test]
    fn classify_maps_predicates_to_color_groups() {
        assert_eq!(classify("师从"), "师承");
        assert_eq!(classify("敌对"), "敌对");
        assert_eq!(classify("父子"), "亲缘");
        assert_eq!(classify("暗恋"), "情爱");
        assert_eq!(classify("利用"), "利益");
        assert_eq!(classify("路过"), "其他");
    }
}
