//! 台账 JSON 的结构校验：采纳提案 / AI 改过之后，落库的内容必须还是台账。
//!
//! ## 为什么需要这一层
//!
//! 我们所有的写路径都是 `serde_json::Value` 直通：提案 payload 里是什么就写什么。
//! 这在字段这个层面很灵活（AI 加个新字段不必改代码），但代价是**没有任何地方
//! 拦住结构错误**——`id` 写成数字、`chapter` 写成 "第12章"、条目从对象退化成
//! 一个字符串、两条记录撞同一个 id。这些都不会让程序崩，只会让页面某天突然
//! 少半条数据，或者改一条把另一条改掉。
//!
//! ## 校验的边界
//!
//! 只查**结构契约**（是不是数组、每条是不是对象、id 有没有、id 重不重、
//! 已知字段的类型对不对），不查业务规则（不会判断"这个伏笔该不该埋"）。
//! 未知字段一律放行：AI 与后续版本都会往条目里加东西，拦住等于把演进路径堵死。
//!
//! ## 失败就整档回滚
//!
//! 一个文件里有一条非法，就把整个文件退回到写之前——而不是跳过那一条继续写。
//! 理由：台账是作者的眼睛，出现"部分写成功"的状态时他看到的是一份既不是旧版
//! 也不完整的第三份东西，比直接失败更难收拾。

use std::path::Path;

use crate::journal::{Journal, UndoOutcome};

/// 受结构校验保护的文件。不在这个清单里的（正文、设定 markdown、canvas 布局）
/// 有自己的格式，不走台账规则。
pub const LEDGER_FILES: &[&str] = &[
    "storylines.json",
    "events.json",
    "stages.json",
    "clues.json",
    "worldlines.json",
    "characters.json",
    "relations.json",
];

/// 一个文件当下的结构问题。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Problem {
    pub path: String,
    pub issues: Vec<String>,
}

impl Problem {
    /// 给人看的一句话（轨迹窗、改动记录页都用它）。
    pub fn summary(&self) -> String {
        let head: Vec<&str> = self.issues.iter().take(3).map(|s| s.as_str()).collect();
        let more = if self.issues.len() > 3 {
            format!("（另有 {} 处）", self.issues.len() - 3)
        } else {
            String::new()
        };
        format!("{} 结构不合法：{}{}", self.path, head.join("；"), more)
    }
}

/// 校验一个台账文件的**文本**。空文本视为合法（文件不存在 / 尚未创建）。
///
/// 返回 `Err` 而不是 bool：作者要知道"第 3 条的 id 是数字"，不是只知道"文件坏了"。
pub fn validate_text(rel_path: &str, raw: &str) -> Result<(), Vec<String>> {
    if raw.trim().is_empty() {
        return Ok(());
    }
    let value: serde_json::Value = match serde_json::from_str(raw) {
        Ok(v) => v,
        Err(e) => return Err(vec![format!("不是合法 JSON（{e}）")]),
    };
    let Some(items) = value.as_array() else {
        return Err(vec!["顶层不是数组".into()]);
    };
    let mut issues = Vec::new();
    let mut seen: std::collections::BTreeMap<&str, usize> = std::collections::BTreeMap::new();

    for (i, item) in items.iter().enumerate() {
        let n = i + 1;
        let Some(obj) = item.as_object() else {
            issues.push(format!("第 {n} 条不是对象"));
            continue;
        };
        match obj.get("id").and_then(|v| v.as_str()) {
            Some(id) if !id.trim().is_empty() => {
                if let Some(prev) = seen.insert(id, n) {
                    issues.push(format!("第 {n} 条的 id「{id}」与第 {prev} 条重复"));
                }
            }
            Some(_) => issues.push(format!("第 {n} 条的 id 是空串")),
            None => issues.push(format!("第 {n} 条缺 id（或 id 不是字符串）")),
        }
        check_shape(rel_path, obj, n, &mut issues);
    }
    if issues.is_empty() {
        Ok(())
    } else {
        Err(issues)
    }
}

/// 各文件特有的字段契约。
///
/// 只查那些**一旦错了会静默出事**的字段：
/// - 章号写成字符串 → 排序/过滤全失效，事件在排期板上消失
/// - 世界线的 nodes 退化成非数组 → 时间轴整条线空白
/// - 关系的 from/to 为空 → 画布画不出边，但列表里还留着这条
fn check_shape(
    rel_path: &str,
    obj: &serde_json::Map<String, serde_json::Value>,
    n: usize,
    issues: &mut Vec<String>,
) {
    let non_empty_str = |key: &str| -> bool {
        obj.get(key)
            .and_then(|v| v.as_str())
            .map(|s| !s.trim().is_empty())
            .unwrap_or(false)
    };
    let chapter_ok = |key: &str| -> bool {
        match obj.get(key) {
            None => true,
            Some(v) => v.as_u64().is_some() || v.is_null(),
        }
    };

    match rel_path {
        "events.json" => {
            if !non_empty_str("objectiveFact") {
                issues.push(format!("第 {n} 条缺 objectiveFact（事件内容）"));
            }
            if !chapter_ok("chapter") {
                issues.push(format!("第 {n} 条的 chapter 不是数字"));
            }
        }
        "clues.json" => {
            if !non_empty_str("title") {
                issues.push(format!("第 {n} 条缺 title（伏笔名）"));
            }
            if !chapter_ok("plantedChapter") {
                issues.push(format!("第 {n} 条的 plantedChapter 不是数字"));
            }
        }
        "stages.json" => {
            for key in ["chapterStart", "chapterEnd"] {
                if !chapter_ok(key) {
                    issues.push(format!("第 {n} 条的 {key} 不是数字"));
                }
            }
        }
        "storylines.json" => {
            if !non_empty_str("name") {
                issues.push(format!("第 {n} 条缺 name（故事线名）"));
            }
        }
        "characters.json" => {
            if !non_empty_str("name") {
                issues.push(format!("第 {n} 条缺 name（人物名）"));
            }
        }
        "relations.json" => {
            for key in ["from", "to"] {
                if !non_empty_str(key) {
                    issues.push(format!("第 {n} 条的 {key} 为空"));
                }
            }
            match obj.get("intervals") {
                None => {}
                Some(v) if v.is_array() => {
                    if v.as_array().map(|a| a.is_empty()).unwrap_or(false) {
                        issues.push(format!("第 {n} 条没有任何关系区间"));
                    }
                }
                Some(_) => issues.push(format!("第 {n} 条的 intervals 不是数组")),
            }
        }
        "worldlines.json" => {
            for key in ["nodes", "edges", "diffs"] {
                if let Some(v) = obj.get(key) {
                    if !v.is_array() {
                        issues.push(format!("第 {n} 条的 {key} 不是数组"));
                    }
                }
            }
            if let Some(nodes) = obj.get("nodes").and_then(|v| v.as_array()) {
                let mut ids = std::collections::BTreeSet::new();
                for node in nodes {
                    let Some(no) = node.as_object() else {
                        issues.push(format!("第 {n} 条线里有个节点不是对象"));
                        continue;
                    };
                    let Some(nid) = no.get("id").and_then(|v| v.as_str()) else {
                        issues.push(format!("第 {n} 条线里有节点缺 id"));
                        continue;
                    };
                    if !ids.insert(nid.to_string()) {
                        issues.push(format!("第 {n} 条线里节点 id「{nid}」重复"));
                    }
                    if let Some(ch) = no.get("chapter") {
                        if ch.as_u64().is_none() && !ch.is_null() {
                            issues.push(format!("线内节点「{nid}」的 chapter 不是数字"));
                        }
                    }
                }
            }
        }
        _ => {}
    }
}

/// 读一个台账文件并校验（不存在 → 合法）。
pub fn validate_file(root: &Path, rel_path: &str) -> Result<(), Vec<String>> {
    match std::fs::read_to_string(root.join(rel_path)) {
        Ok(text) => validate_text(rel_path, &text),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(e) => Err(vec![format!("读不出来：{e}")]),
    }
}

/// 一次写入前的内存快照：只保受保护的文件，校验不过就整档还原。
pub struct Guard {
    saved: Vec<(String, Option<String>)>,
}

impl Guard {
    pub fn capture(root: &Path) -> Self {
        let saved = LEDGER_FILES
            .iter()
            .map(|f| {
                let content = std::fs::read_to_string(root.join(f)).ok();
                ((*f).to_string(), content)
            })
            .collect();
        Self { saved }
    }

    /// 校验所有变过的文件；有非法就全部还原并返回问题清单。
    ///
    /// 还原**所有**变过的文件而不只是坏的那个：一次采纳可能连着改两个台账，
    /// 只回滚坏的那份会留下一个"改了一半"的中间状态。
    pub fn verify(&self, root: &Path) -> Result<(), Vec<Problem>> {
        let mut problems = Vec::new();
        let mut dirty: Vec<&(String, Option<String>)> = Vec::new();
        for entry in &self.saved {
            let now = std::fs::read_to_string(root.join(&entry.0)).ok();
            if now == entry.1 {
                continue;
            }
            dirty.push(entry);
            if let Some(text) = &now {
                if let Err(issues) = validate_text(&entry.0, text) {
                    problems.push(Problem { path: entry.0.clone(), issues });
                }
            }
        }
        if problems.is_empty() {
            return Ok(());
        }
        for (path, before) in dirty {
            let target = root.join(path);
            match before {
                Some(text) => {
                    let _ = crate::store::write_atomic(&target, text);
                }
                None => {
                    let _ = std::fs::remove_file(&target);
                }
            }
        }
        Err(problems)
    }
}

/// AI 改过的台账文件：校验不合法就按改动记录回滚。
///
/// 与 `Guard` 的区别是**时机**：`Guard` 守的是"我们自己的写路径"（采纳提案），
/// 这里守的是"AI 绕过工具直接写文件"——那条路没有 Java 门禁，事后才知道。
/// 回滚走 journal 的 undo 语义，所以作者之后手改过的文件会报冲突、不会被覆盖。
pub fn validate_and_rollback_ai_edits(root: &Path, limit: usize) -> Vec<Problem> {
    let journal = Journal::new(root);
    let mut checked: std::collections::BTreeSet<String> = std::collections::BTreeSet::new();
    let mut out = Vec::new();
    for change in journal.all().into_iter().take(limit) {
        if !matches!(change.actor, crate::journal::Actor::Ai { .. }) {
            continue;
        }
        if !LEDGER_FILES.contains(&change.path.as_str()) {
            continue;
        }
        // 同一文件这个回合被改过多次时只看最后一次
        if !checked.insert(change.path.clone()) {
            continue;
        }
        let issues = match validate_file(root, &change.path) {
            Ok(()) => continue,
            Err(issues) => issues,
        };
        let rolled = match journal.undo(root, &change.id) {
            Ok(UndoOutcome::Restored) | Ok(UndoOutcome::Removed) => {
                // 回滚之后再校验一次：前像本身也可能已经坏了，那就只报告不再动
                let still = validate_file(root, &change.path).err().unwrap_or_default();
                if still.is_empty() {
                    "已回滚".to_string()
                } else {
                    format!("回滚后仍不合法（{}）", still.join("；"))
                }
            }
            // 冲突时不动它：作者之后手改过这个文件，回滚会把他刚写的东西带走
            Ok(UndoOutcome::Conflict { .. }) => {
                "未回滚：作者之后改过这个文件（冲突，请先确认那一版怎么写）".to_string()
            }
            Err(e) => format!("未回滚：{e}"),
        };
        let mut issues = issues;
        issues.push(rolled);
        out.push(Problem { path: change.path, issues });
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tmp(tag: &str) -> std::path::PathBuf {
        let p = std::env::temp_dir().join(format!("zhibi-schema-{tag}"));
        let _ = std::fs::remove_dir_all(&p);
        std::fs::create_dir_all(&p).unwrap();
        p
    }

    #[test]
    fn well_formed_ledger_passes() {
        let ok = r#"[{"id":"L01","name":"主线"},{"id":"L02","name":"支线"}]"#;
        assert!(validate_text("storylines.json", ok).is_ok());
        // 未知字段放行：AI 与后续版本都会往里加东西
        let extra = r#"[{"id":"L01","name":"主线","whatever":{"x":1}}]"#;
        assert!(validate_text("storylines.json", extra).is_ok());
    }

    #[test]
    fn duplicate_ids_are_reported_with_both_positions() {
        let bad = r#"[{"id":"L01","name":"甲"},{"id":"L01","name":"乙"}]"#;
        let issues = validate_text("storylines.json", bad).unwrap_err();
        assert_eq!(issues.len(), 1);
        assert!(issues[0].contains("第 2 条"), "{issues:?}");
        assert!(issues[0].contains("第 1 条"), "要指出和谁撞了：{issues:?}");
    }

    #[test]
    fn chapter_as_string_is_caught() {
        // "第12章" 这种写法会让排期板把它当 0，事件凭空消失
        let bad = r#"[{"id":"E01","objectiveFact":"夜探","chapter":"第12章"}]"#;
        let issues = validate_text("events.json", bad).unwrap_err();
        assert!(issues.iter().any(|s| s.contains("chapter")), "{issues:?}");
    }

    #[test]
    fn empty_top_level_and_bad_json_are_caught() {
        assert!(validate_text("events.json", "").is_ok(), "文件不存在是合法的");
        let issues = validate_text("events.json", "null").unwrap_err();
        assert!(issues[0].contains("数组"), "{issues:?}");
        let issues = validate_text("events.json", "[{").unwrap_err();
        assert!(issues[0].contains("JSON"), "{issues:?}");
    }

    #[test]
    fn worldline_with_broken_nodes_is_caught() {
        let bad = r#"[{"id":"β","name":"假设","nodes":"nope"}]"#;
        let issues = validate_text("worldlines.json", bad).unwrap_err();
        assert!(issues.iter().any(|s| s.contains("nodes")), "{issues:?}");

        let dup = r#"[{"id":"β","name":"假设","nodes":[{"id":"n1"},{"id":"n1"}]}]"#;
        let issues = validate_text("worldlines.json", dup).unwrap_err();
        assert!(issues.iter().any(|s| s.contains("重复")), "{issues:?}");
    }

    #[test]
    fn relation_without_endpoints_is_caught() {
        let bad = r#"[{"id":"r1","from":"沈砚","to":"","kind":"师承","intervals":[]}]"#;
        let issues = validate_text("relations.json", bad).unwrap_err();
        assert!(issues.iter().any(|s| s.contains("to")), "{issues:?}");
        assert!(issues.iter().any(|s| s.contains("区间")), "{issues:?}");
    }

    #[test]
    fn guard_rolls_back_every_changed_file_not_just_the_broken_one() {
        let root = tmp("guard");
        std::fs::write(root.join("storylines.json"), r#"[{"id":"L01","name":"主线"}]"#).unwrap();
        std::fs::write(root.join("events.json"), r#"[{"id":"E01","objectiveFact":"旧","chapter":1}]"#).unwrap();

        let guard = Guard::capture(&root);
        // 一次操作同时改两个文件：一个合法、一个把 chapter 写成字符串
        std::fs::write(root.join("storylines.json"), r#"[{"id":"L01","name":"主线改"}]"#).unwrap();
        std::fs::write(root.join("events.json"), r#"[{"id":"E01","objectiveFact":"新","chapter":"十二"}]"#).unwrap();

        let problems = guard.verify(&root).unwrap_err();
        assert_eq!(problems.len(), 1);
        assert_eq!(problems[0].path, "events.json");

        // 两份都回到操作前，不留"改了一半"
        let sl = std::fs::read_to_string(root.join("storylines.json")).unwrap();
        assert!(sl.contains("主线") && !sl.contains("主线改"), "合法文件也要还原：{sl}");
        let ev = std::fs::read_to_string(root.join("events.json")).unwrap();
        assert!(ev.contains("旧"), "{ev}");
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn guard_deletes_a_file_that_did_not_exist_before() {
        let root = tmp("guard-new");
        let guard = Guard::capture(&root);
        std::fs::write(root.join("clues.json"), r#"[{"id":"F01"}]"#).unwrap();
        assert!(guard.verify(&root).is_err());
        assert!(!root.join("clues.json").exists(), "原本没有这个文件，还原就是删掉");
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn guard_leaves_valid_edits_alone() {
        let root = tmp("guard-ok");
        std::fs::write(root.join("events.json"), r#"[{"id":"E01","objectiveFact":"旧","chapter":1}]"#).unwrap();
        let guard = Guard::capture(&root);
        std::fs::write(root.join("events.json"), r#"[{"id":"E02","objectiveFact":"新","chapter":2}]"#).unwrap();
        assert!(guard.verify(&root).is_ok());
        assert!(std::fs::read_to_string(root.join("events.json")).unwrap().contains("新"));
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn ai_smuggled_broken_ledger_is_rolled_back() {
        let root = tmp("ai-rollback");
        let journal = Journal::new(&root);
        let before = r#"[{"id":"E01","objectiveFact":"旧","chapter":1}]"#;
        std::fs::write(root.join("events.json"), before).unwrap();
        // 模拟 AI 绕过工具直接写文件：先留日志，再写坏
        let after = r#"[{"id":"E01","objectiveFact":"新","chapter":"十二"}]"#;
        journal
            .record(
                &root,
                "events.json",
                before,
                after,
                &crate::journal::Actor::Ai { mode: "cowrite".into(), agent: "reviewer".into() },
                "edit",
                "turn9",
                "",
            )
            .unwrap();
        std::fs::write(root.join("events.json"), after).unwrap();

        let problems = validate_and_rollback_ai_edits(&root, 50);
        assert_eq!(problems.len(), 1, "{problems:?}");
        assert!(problems[0].issues.iter().any(|s| s.contains("已回滚")), "{problems:?}");
        let now = std::fs::read_to_string(root.join("events.json")).unwrap();
        assert!(now.contains("旧"), "要真的回到 AI 改之前：{now}");
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn ai_edit_after_author_handfix_reports_conflict_and_keeps_it() {
        let root = tmp("ai-conflict");
        let journal = Journal::new(&root);
        let before = r#"[{"id":"E01","objectiveFact":"旧","chapter":1}]"#;
        std::fs::write(root.join("events.json"), before).unwrap();
        let ai_after = r#"[{"id":"E01","objectiveFact":"AI","chapter":"十二"}]"#;
        journal
            .record(
                &root,
                "events.json",
                before,
                ai_after,
                &crate::journal::Actor::Ai { mode: "cowrite".into(), agent: String::new() },
                "edit",
                "turn9",
                "",
            )
            .unwrap();
        // AI 写完，作者又手改了一版（仍不合法，但这是作者的东西）
        let author = r#"[{"id":"E01","objectiveFact":"作者手改","chapter":"待定"}]"#;
        std::fs::write(root.join("events.json"), author).unwrap();

        let problems = validate_and_rollback_ai_edits(&root, 50);
        assert_eq!(problems.len(), 1);
        assert!(problems[0].issues.iter().any(|s| s.contains("冲突")), "{problems:?}");
        assert!(
            std::fs::read_to_string(root.join("events.json")).unwrap().contains("作者手改"),
            "作者的改动不能被静默覆盖"
        );
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn valid_ai_edit_is_not_touched() {
        let root = tmp("ai-ok");
        let journal = Journal::new(&root);
        let before = r#"[{"id":"E01","objectiveFact":"旧","chapter":1}]"#;
        std::fs::write(root.join("events.json"), before).unwrap();
        let after = r#"[{"id":"E01","objectiveFact":"新","chapter":2}]"#;
        journal
            .record(
                &root,
                "events.json",
                before,
                after,
                &crate::journal::Actor::Ai { mode: "cowrite".into(), agent: String::new() },
                "edit",
                "turn9",
                "",
            )
            .unwrap();
        std::fs::write(root.join("events.json"), after).unwrap();
        assert!(validate_and_rollback_ai_edits(&root, 50).is_empty());
        assert!(std::fs::read_to_string(root.join("events.json")).unwrap().contains("新"));
        let _ = std::fs::remove_dir_all(&root);
    }
}
