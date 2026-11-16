//! 记忆召回与推断：把 `memory.json` 的事实账本从「平铺列表」变成「可召回、可推断」。
//!
//! 召回 = 给定章号 / 人物 / 关键词，按相关度 + 章距衰减 + 新近度排序去重；
//! 推断 = 从既有事实推导隐含关系、检出单值谓语冲突、标出陈旧待复核事实。
//! 两者都是**纯函数**，Rust（界面）与 novel-pack（AI 工具）各写一遍，
//! 同读 `tests/conformance/cases.json` 的 memoryRecall / memoryInfer 族。
//! 打分全用整数，避免两端浮点舍入漂移。

use serde_json::Value;

/// 一条事实的只读视图。
#[derive(Clone, Debug, PartialEq)]
pub struct Fact {
    pub id: String,
    pub subject: String,
    pub predicate: String,
    pub object: String,
    pub chapter: u32,
    pub public: bool,
}

pub fn fact_from(v: &Value, idx: usize) -> Fact {
    Fact {
        id: v
            .get("id")
            .and_then(|x| x.as_str())
            .map(|s| s.to_string())
            .unwrap_or_else(|| format!("f{idx}")),
        subject: v.get("subject").and_then(|x| x.as_str()).unwrap_or("").to_string(),
        predicate: v.get("predicate").and_then(|x| x.as_str()).unwrap_or("").to_string(),
        object: v.get("object").and_then(|x| x.as_str()).unwrap_or("").to_string(),
        chapter: v.get("fromChapter").and_then(|x| x.as_u64()).unwrap_or(0) as u32,
        public: v.get("publicToReader").and_then(|x| x.as_bool()).unwrap_or(true),
    }
}

/// 别名 → 本名。同一别名挂多个本名时取字典序最小，保证两端一致。
pub fn alias_map(aliases: &[Value]) -> Vec<(String, String)> {
    let mut pairs: Vec<(String, String)> = Vec::new();
    for a in aliases {
        let canon = a.get("canonicalName").and_then(|x| x.as_str()).unwrap_or("");
        if canon.is_empty() {
            continue;
        }
        if let Some(list) = a.get("aliases").and_then(|x| x.as_array()) {
            for al in list {
                let Some(s) = al.as_str() else { continue };
                if !s.is_empty() && s != canon {
                    pairs.push((s.to_string(), canon.to_string()));
                }
            }
        }
    }
    pairs.sort();
    pairs.dedup();
    pairs
}

pub fn normalize(name: &str, map: &[(String, String)]) -> String {
    for (al, canon) in map {
        if al == name {
            return canon.clone();
        }
    }
    name.to_string()
}

/// 召回查询。chapter 缺省表示不按章距加权；terms 为空表示只按新近度排。
#[derive(Clone, Debug, Default)]
pub struct RecallQuery {
    pub chapter: Option<u32>,
    pub terms: Vec<String>,
    pub limit: usize,
}

#[derive(Clone, Debug, PartialEq)]
pub struct RecallHit {
    pub fact: Fact,
    pub score: i64,
    pub reasons: Vec<String>,
}

const PROX_WINDOW: i64 = 20;

/// 整数打分：相关度(300/200/100 每词) + 章距(0..200) + 新近度(0..100)。
pub fn recall(facts: &[Value], aliases: &[Value], q: &RecallQuery) -> Vec<RecallHit> {
    let map = alias_map(aliases);
    let parsed: Vec<Fact> = facts.iter().enumerate().map(|(i, v)| fact_from(v, i)).collect();
    let max_ch = parsed.iter().map(|f| f.chapter).max().unwrap_or(0).max(1) as i64;
    let limit = if q.limit == 0 { 12 } else { q.limit };

    let mut hits: Vec<RecallHit> = Vec::new();
    for f in &parsed {
        let s = normalize(&f.subject, &map);
        let o = normalize(&f.object, &map);
        let mut score: i64 = 0;
        let mut reasons: Vec<String> = Vec::new();
        for t in &q.terms {
            if t.is_empty() {
                continue;
            }
            let tn = normalize(t, &map);
            if tn == s || tn == o {
                score += 300;
                reasons.push("主宾命中".into());
            } else if f.predicate.contains(t.as_str()) || o.contains(t.as_str()) || s.contains(t.as_str()) {
                score += 200;
                reasons.push("谓宾包含".into());
            } else if s.contains(tn.as_str()) || o.contains(tn.as_str()) {
                score += 100;
                reasons.push("别名包含".into());
            }
        }
        if let Some(ch) = q.chapter {
            let dist = (f.chapter as i64 - ch as i64).abs();
            let prox = 200 * (PROX_WINDOW - dist.min(PROX_WINDOW)) / PROX_WINDOW;
            score += prox;
            if prox > 0 {
                reasons.push("章距近".into());
            }
        }
        score += 100 * f.chapter as i64 / max_ch;
        hits.push(RecallHit { fact: f.clone(), score, reasons });
    }

    // 同 (主,谓,宾) 去重：留分高者，同分留章新者。
    // 用 Set 而不是 dedup 内层线性查找：这段在记忆页每帧都跑，账本上千条时
    // O(n²) 的重复归一化会把渲染拖住（TS 侧同逻辑用的就是 Set）。
    hits.sort_by(|a, b| {
        b.score
            .cmp(&a.score)
            .then(b.fact.chapter.cmp(&a.fact.chapter))
            .then(a.fact.subject.cmp(&b.fact.subject))
    });
    let mut seen: std::collections::HashSet<(String, String, String)> = std::collections::HashSet::new();
    let mut dedup: Vec<RecallHit> = Vec::new();
    for h in hits {
        let key = (
            normalize(&h.fact.subject, &map),
            h.fact.predicate.clone(),
            normalize(&h.fact.object, &map),
        );
        if !seen.insert(key) {
            continue;
        }
        dedup.push(h);
    }
    dedup.sort_by(|a, b| {
        b.score
            .cmp(&a.score)
            .then(b.fact.chapter.cmp(&a.fact.chapter))
            .then(a.fact.subject.cmp(&b.fact.subject))
            .then(a.fact.predicate.cmp(&b.fact.predicate))
            .then(a.fact.object.cmp(&b.fact.object))
    });
    dedup.truncate(limit);
    dedup
}

#[derive(Clone, Debug, PartialEq)]
pub struct Implied {
    pub subject: String,
    pub predicate: String,
    pub object: String,
    pub rule: String,
    pub basis: Vec<String>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Contradiction {
    pub subject: String,
    pub predicate: String,
    pub objects: Vec<String>,
    pub chapters: Vec<u32>,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Inference {
    pub implied: Vec<Implied>,
    pub contradictions: Vec<Contradiction>,
    pub stale: Vec<String>,
}

const SYMMETRIC: [&str; 3] = ["夫妻", "结义", "同门"];
const SINGLE_VALUED: [&str; 4] = ["夫妻", "配偶", "生父", "生母"];
const DURABLE: [&str; 4] = ["师从", "生父", "生母", "出生"];
const STALE_WINDOW: u32 = 30;

/// 推断规则——**书级可配**。`memory.json` 里可含 `inferRules` 块覆盖默认
/// （修仙题材加 `师徒`/`道侣`，西幻加 `封臣`/`血盟`）。两端读同一配置，语义一致。
#[derive(Clone, Debug, PartialEq)]
pub struct InferRules {
    /// 对称谓语（A→B 成立则补 B→A）
    pub symmetric: Vec<String>,
    /// 单值谓语（同一主语同一时刻只能指向一个宾语）
    pub single_valued: Vec<String>,
    /// 耐久谓语（不会陈旧，永不标待复核）
    pub durable: Vec<String>,
    /// 陈旧窗口（章）：事实生效章距今超过此值且非耐久 → 待复核
    pub stale_window: u32,
}

impl Default for InferRules {
    fn default() -> Self {
        Self {
            symmetric: SYMMETRIC.iter().map(|s| s.to_string()).collect(),
            single_valued: SINGLE_VALUED.iter().map(|s| s.to_string()).collect(),
            durable: DURABLE.iter().map(|s| s.to_string()).collect(),
            stale_window: STALE_WINDOW,
        }
    }
}

impl InferRules {
    /// 从 `memory.json` 的 `inferRules` 块读配置；缺块或字段缺失时用默认值补。
    /// 坏 JSON / 非对象一律给默认——推断不能因配置坏了就整页报错。
    pub fn from_memory_json(v: &Value) -> Self {
        let mut r = Self::default();
        let Some(blk) = v.get("inferRules").and_then(|x| x.as_object()) else {
            return r;
        };
        let take_strs = |key: &str, default: &mut Vec<String>| {
            if let Some(arr) = blk.get(key).and_then(|x| x.as_array()) {
                *default = arr.iter().filter_map(|s| s.as_str().map(String::from)).collect();
            }
        };
        take_strs("symmetric", &mut r.symmetric);
        take_strs("singleValued", &mut r.single_valued);
        take_strs("durable", &mut r.durable);
        if let Some(w) = blk.get("staleWindow").and_then(|x| x.as_u64()) {
            r.stale_window = w as u32;
        }
        r
    }
}

/// 推断：同门推导、对称补全、单值冲突、陈旧待复核。全部确定性、可复算。
/// 生产代码请用 [`infer_with_rules`] 传书级 `inferRules`；这是默认规则的便捷入口
/// （主要为测试与外部调用方保留，内部页面不直接调它）。
#[allow(dead_code)]
pub fn infer(facts: &[Value], aliases: &[Value], current_chapter: u32) -> Inference {
    infer_with_rules(facts, aliases, current_chapter, &InferRules::default())
}

/// 带规则的推断。调用方（记忆页 / 图谱 / `infer_memory`）从这里读 `memory.json` 的
/// `inferRules` 块传入，让书级题材可自定义哪些谓语参与推导/冲突/陈旧判定。
pub fn infer_with_rules(facts: &[Value], aliases: &[Value], current_chapter: u32, rules: &InferRules) -> Inference {
    let map = alias_map(aliases);
    let parsed: Vec<Fact> = facts.iter().enumerate().map(|(i, v)| fact_from(v, i)).collect();
    let mut out = Inference::default();

    // 同门：师从(A,C) 且 师从(B,C) → 同门(A,B)，只出一次（排序对）。
    let mut masters: Vec<(String, Vec<&Fact>)> = Vec::new();
    for f in &parsed {
        if f.predicate == "师从" {
            let o = normalize(&f.object, &map);
            if let Some(entry) = masters.iter_mut().find(|(k, _)| *k == o) {
                entry.1.push(f);
            } else {
                masters.push((o, vec![f]));
            }
        }
    }
    for (_, group) in &masters {
        for i in 0..group.len() {
            for j in (i + 1)..group.len() {
                let a = normalize(&group[i].subject, &map);
                let b = normalize(&group[j].subject, &map);
                if a == b {
                    continue;
                }
                let (x, y) = if a < b { (a, b) } else { (b, a) };
                let mut basis = vec![group[i].id.clone(), group[j].id.clone()];
                basis.sort();
                if !out.implied.iter().any(|m| m.subject == x && m.object == y && m.predicate == "同门") {
                    out.implied.push(Implied {
                        subject: x,
                        predicate: "同门".into(),
                        object: y,
                        rule: "同师推导".into(),
                        basis,
                    });
                }
            }
        }
    }

    // 对称补全：夫妻/结义/同门(A,B) 若没有显式 (B,A)，补一条。
    for f in &parsed {
        if !rules.symmetric.iter().any(|p| p == &f.predicate) {
            continue;
        }
        let a = normalize(&f.subject, &map);
        let b = normalize(&f.object, &map);
        if a == b || a.is_empty() || b.is_empty() {
            continue;
        }
        let has_rev = parsed.iter().any(|g| {
            g.predicate == f.predicate
                && normalize(&g.subject, &map) == b
                && normalize(&g.object, &map) == a
        });
        if !has_rev
            && !out
                .implied
                .iter()
                .any(|m| m.subject == b && m.object == a && m.predicate == f.predicate)
        {
            out.implied.push(Implied {
                subject: b,
                predicate: f.predicate.clone(),
                object: a,
                rule: "对称补全".into(),
                basis: vec![f.id.clone()],
            });
        }
    }

    // 单值谓语冲突：同 (主,谓) 出现多个不同宾语。
    let mut groups: Vec<(String, String, Vec<&Fact>)> = Vec::new();
    for f in &parsed {
        if !rules.single_valued.iter().any(|p| p == &f.predicate) {
            continue;
        }
        let s = normalize(&f.subject, &map);
        if let Some(entry) = groups.iter_mut().find(|(ks, kp, _)| *ks == s && *kp == f.predicate) {
            entry.2.push(f);
        } else {
            groups.push((s, f.predicate.clone(), vec![f]));
        }
    }
    for (s, p, group) in groups {
        let mut objs: Vec<String> = group.iter().map(|f| normalize(&f.object, &map)).collect();
        objs.sort();
        objs.dedup();
        if objs.len() >= 2 {
            let mut chapters: Vec<u32> = group.iter().map(|f| f.chapter).collect();
            chapters.sort();
            chapters.dedup();
            out.contradictions.push(Contradiction { subject: s, predicate: p, objects: objs, chapters });
        }
    }

    // 陈旧待复核：生效章远落后于当前边界、且谓语非持久型。
    if current_chapter > 0 {
        for f in &parsed {
            if f.chapter > 0
                && current_chapter.saturating_sub(f.chapter) > rules.stale_window
                && !rules.durable.iter().any(|p| p == &f.predicate)
            {
                out.stale.push(f.id.clone());
            }
        }
    }

    out.implied.sort_by(|a, b| {
        a.subject
            .cmp(&b.subject)
            .then(a.predicate.cmp(&b.predicate))
            .then(a.object.cmp(&b.object))
    });
    out.contradictions.sort_by(|a, b| a.subject.cmp(&b.subject).then(a.predicate.cmp(&b.predicate)));
    out.stale.sort();
    out.stale.dedup();
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn facts() -> Vec<Value> {
        vec![
            json!({"id":"a","subject":"沈砚","predicate":"师从","object":"老周","fromChapter":3}),
            json!({"id":"b","subject":"顾青","predicate":"师从","object":"老周","fromChapter":5}),
            json!({"id":"c","subject":"沈砚","predicate":"夫妻","object":"顾青","fromChapter":8}),
            json!({"id":"d","subject":"沈砚","predicate":"配偶","object":"阿绣","fromChapter":9}),
            json!({"id":"e","subject":"沈砚","predicate":"配偶","object":"顾青","fromChapter":10}),
            json!({"id":"f","subject":"沈砚","predicate":"获得","object":"断刀","fromChapter":2}),
        ]
    }

    #[test]
    fn recall_ranks_alias_hit_above_substring_and_dedups() {
        let aliases = vec![json!({"canonicalName":"沈砚","aliases":["砚哥"]})];
        let q = RecallQuery { chapter: Some(9), terms: vec!["砚哥".into()], limit: 10 };
        let hits = recall(&facts(), &aliases, &q);
        // 别名归一后主宾命中(300) 应排在仅包含(200/100)之前
        assert_eq!(hits[0].fact.subject, "沈砚");
        assert!(hits[0].score >= 300);
        // 配偶 两条同主谓不同宾不去重；同 (主,谓,宾) 才去重
        let dup = vec![
            json!({"id":"x","subject":"沈砚","predicate":"获得","object":"断刀","fromChapter":2}),
            json!({"id":"y","subject":"沈砚","predicate":"获得","object":"断刀","fromChapter":4}),
        ];
        let h2 = recall(&dup, &[], &RecallQuery { limit: 10, ..Default::default() });
        assert_eq!(h2.len(), 1);
        assert_eq!(h2[0].fact.id, "y"); // 同分留章新者
    }

    #[test]
    fn infer_derives_tongmen_symmetry_contradiction_stale() {
        let inf = infer(&facts(), &[], 40);
        assert!(inf.implied.iter().any(|m| m.predicate == "同门" && m.subject == "沈砚" && m.object == "顾青"));
        assert!(inf.implied.iter().any(|m| m.predicate == "夫妻" && m.subject == "顾青" && m.object == "沈砚"));
        let c = inf.contradictions.iter().find(|c| c.predicate == "配偶").unwrap();
        assert_eq!(c.objects, vec!["阿绣".to_string(), "顾青".to_string()]);
        // 获得(断刀) 生效第2章，边界40 → 差38>30 且非持久 → 待复核
        assert!(inf.stale.contains(&"f".to_string()));
        // 师从 是持久型，不标陈旧
        assert!(!inf.stale.contains(&"a".to_string()));
    }

    #[test]
    fn infer_rules_are_book_level_configurable() {
        // 修仙题材加 师徒/道侣：symmetric 加 师徒，singleValued 加 道侣。
        let cfg = json!({
            "inferRules": {
                "symmetric": ["夫妻", "结义", "同门", "师从"],
                "singleValued": ["夫妻", "配偶", "生父", "生母", "道侣"],
                "durable": ["师从", "生父", "生母", "出生"],
                "staleWindow": 20
            }
        });
        let r = InferRules::from_memory_json(&cfg);
        assert!(r.symmetric.contains(&"师从".to_string()), "symmetric 覆盖 师从（修仙题材可配）");
        assert!(r.single_valued.contains(&"道侣".to_string()));
        assert_eq!(r.stale_window, 20);

        // 只覆盖一部分字段：缺的用默认，覆盖的用配置
        let partial = json!({ "inferRules": { "staleWindow": 15 } });
        let r2 = InferRules::from_memory_json(&partial);
        assert_eq!(r2.stale_window, 15);
        assert!(r2.symmetric.contains(&"夫妻".to_string()), "symmetric 缺省回默认");
        assert!(r2.durable.contains(&"师从".to_string()), "durable 缺省回默认");

        // 坏 JSON / 无 inferRules 块 → 全默认，不 panic
        let empty = json!({});
        let r3 = InferRules::from_memory_json(&empty);
        assert_eq!(r3, InferRules::default());
        let bad = json!("不是对象");
        let r4 = InferRules::from_memory_json(&bad);
        assert_eq!(r4, InferRules::default());

        // 书级规则真正改变推断结果：symmetric 加 师徒 → 从 (A 师从 B) 补出 (B 师从 A)
        let facts = vec![
            json!({"id":"t1","subject":"沈砚","predicate":"师从","object":"老周","fromChapter":3}),
            json!({"id":"t2","subject":"顾青","predicate":"师从","object":"老周","fromChapter":5}),
        ];
        // 默认规则：师从 不在 SYMMETRIC，无对称补全
        let d = infer_with_rules(&facts, &[], 10, &InferRules::default());
        assert!(!d.implied.iter().any(|m| m.predicate == "师从" && m.object == "沈砚"));
        // 配置 symmetric 含 师从 → 补出 老周 → 沈砚
        let custom = InferRules::from_memory_json(&cfg);
        let c = infer_with_rules(&facts, &[], 10, &custom);
        assert!(
            c.implied.iter().any(|m| m.predicate == "师从" && m.subject == "老周" && m.object == "沈砚"),
            "symmetric 加 师徒 后应从 师从(A,B) 补出 师从(B,A)"
        );
    }
}