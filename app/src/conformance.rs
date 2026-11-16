//! 双端一致性：同一份夹具，Rust 与 novel-pack 各跑一遍。
//!
//! 为什么需要：客户端（Rust）与能力包（TypeScript）**各写一遍**同样的规则——
//! 文件名安全化、章目录命名、写作档位闸门、关系区间判定。两边任意一侧改了
//! 而另一侧没改，症状是"界面说能写、AI 说不能写"这类最难复现的 bug。
//!
//! 做法：`tests/conformance/cases.json` 是唯一事实源。这边读它断言 Rust 实现，
//! 那边（`novel-pack/test/conformance.test.mjs`）读同一个文件断言 TS 实现。
//! **任一侧改了规则而没改夹具，两侧测试会同时失败**——这才是它起作用的方式。

#![cfg(test)]

use serde_json::Value;

fn cases() -> Value {
    // 编译期定位：CARGO_MANIFEST_DIR 是 app/，仓库根是它的上一级
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .join("tests")
        .join("conformance")
        .join("cases.json");
    let text = std::fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("读不到夹具 {}：{e}", path.display()));
    serde_json::from_str(&text).expect("夹具不是合法 JSON")
}

#[test]
fn safe_file_name_matches_the_shared_fixture() {
    let c = cases();
    let arr = c["safeFileName"].as_array().expect("缺 safeFileName");
    assert!(!arr.is_empty());
    for item in arr {
        let input = item["in"].as_str().expect("in 必须是字符串");
        let expected = item["out"].as_str().expect("out 必须是字符串");
        // 用被测实现，不是复制一份规则
        let got = crate::store::safe_file_name_for_test(input);
        assert_eq!(
            got, expected,
            "safeFileName({input:?}) = {got:?}，夹具期望 {expected:?}"
        );
    }
}

#[test]
fn chapter_dir_name_matches_the_shared_fixture() {
    let c = cases();
    let arr = c["chapterDirName"].as_array().expect("缺 chapterDirName");
    for item in arr {
        let n = item["n"].as_u64().expect("n 必须是数字") as u32;
        let expected = item["out"].as_str().unwrap();
        assert_eq!(crate::store::chapter_dir_name(n), expected, "章目录命名漂了：{n}");
    }
}

#[test]
fn write_policy_matches_the_shared_fixture() {
    let c = cases();
    let arr = c["writePolicy"].as_array().expect("缺 writePolicy");
    for item in arr {
        let mode = item["mode"].as_str().unwrap();
        let chapter = item["chapter"].as_u64().unwrap() as u32;
        let ledger_ok = item["ledger"].as_bool().unwrap();
        let prose_ok = item["prose"].as_bool().unwrap();
        let why = item["why"].as_str().unwrap_or("");
        let ghost: Vec<u32> = item["ghostwriteChapters"]
            .as_array()
            .map(|a| a.iter().filter_map(|v| v.as_u64()).map(|n| n as u32).collect())
            .unwrap_or_default();

        let policy = crate::conformance_helpers::WritePolicy { mode, ghost };
        assert_eq!(
            policy.can_write_ledger(),
            ledger_ok,
            "档位 {mode} 的台账权限与夹具不符（{why}）"
        );
        assert_eq!(
            policy.can_write_prose(chapter),
            prose_ok,
            "档位 {mode} 第 {chapter} 章的正文权限与夹具不符（{why}）"
        );
    }
}

#[test]
fn relation_interval_covers_matches_the_shared_fixture() {
    let c = cases();
    let arr = c["intervalCovers"].as_array().expect("缺 intervalCovers");
    for item in arr {
        let from = item["from"].as_u64().unwrap() as u32;
        let to = item["to"].as_u64().unwrap() as u32;
        let chapter = item["chapter"].as_u64().unwrap() as u32;
        let expected = item["covers"].as_bool().unwrap();
        let why = item["why"].as_str().unwrap_or("");
        let iv = crate::relations::Interval {
            from_chapter: from,
            to_chapter: to,
            label: "x".into(),
            public_to_reader: true,
            evidence: String::new(),
            extra: Default::default(),
        };
        assert_eq!(
            iv.covers(chapter),
            expected,
            "[{from},{to}) 是否覆盖第 {chapter} 章与夹具不符（{why}）"
        );
        // 合法性判定也要与 TS 侧一致：终点早于起点是坏数据，0 表示开放不算坏。
        // （TS 侧 test/conformance.test.mjs 的「区间合法性判定」测同一件事。）
        assert_eq!(
            iv.is_valid(),
            to == 0 || to > from,
            "[{from},{to}) 的合法性判定与夹具语义不符"
        );
    }
}

#[test]
fn memory_recall_matches_the_shared_fixture() {
    let c = cases();
    let arr = c["memoryRecall"].as_array().expect("缺 memoryRecall");
    assert!(!arr.is_empty());
    for item in arr {
        let why = item["why"].as_str().unwrap_or("");
        let facts = item["facts"].as_array().expect("facts 必须是数组").clone();
        let aliases = item["aliases"].as_array().cloned().unwrap_or_default();
        let q = crate::memory::RecallQuery {
            chapter: item.get("chapter").and_then(|v| v.as_u64()).map(|n| n as u32),
            terms: item["terms"]
                .as_array()
                .map(|a| a.iter().filter_map(|v| v.as_str().map(|s| s.to_string())).collect())
                .unwrap_or_default(),
            limit: item["limit"].as_u64().unwrap_or(12) as usize,
        };
        let hits = crate::memory::recall(&facts, &aliases, &q);
        let got: Vec<String> = hits.iter().map(|h| h.fact.id.clone()).collect();
        let expect: Vec<String> = item["expectIds"]
            .as_array()
            .expect("expectIds")
            .iter()
            .filter_map(|v| v.as_str().map(|s| s.to_string()))
            .collect();
        assert_eq!(got, expect, "召回排序与夹具不符（{why}）");
        if let Some(top) = item.get("expectTopScore").and_then(|v| v.as_i64()) {
            assert_eq!(hits[0].score, top, "召回顶分与夹具不符（{why}）");
        }
    }
}

#[test]
fn memory_infer_matches_the_shared_fixture() {
    let c = cases();
    let arr = c["memoryInfer"].as_array().expect("缺 memoryInfer");
    assert!(!arr.is_empty());
    for item in arr {
        let why = item["why"].as_str().unwrap_or("");
        let facts = item["facts"].as_array().expect("facts").clone();
        let aliases = item["aliases"].as_array().cloned().unwrap_or_default();
        let cur = item["currentChapter"].as_u64().unwrap_or(0) as u32;
        // 共享夹具测的是**默认规则**——书级 inferRules 可配性由 store 侧测试覆盖。
        let inf = crate::memory::infer_with_rules(
            &facts,
            &aliases,
            cur,
            &crate::memory::InferRules::default(),
        );

        let got: Vec<(String, String, String)> = inf
            .implied
            .iter()
            .map(|m| (m.subject.clone(), m.predicate.clone(), m.object.clone()))
            .collect();
        let expect: Vec<(String, String, String)> = item["expectImplied"]
            .as_array()
            .expect("expectImplied")
            .iter()
            .map(|v| {
                (
                    v["subject"].as_str().unwrap().to_string(),
                    v["predicate"].as_str().unwrap().to_string(),
                    v["object"].as_str().unwrap().to_string(),
                )
            })
            .collect();
        assert_eq!(got, expect, "推导关系与夹具不符（{why}）");

        let gotc: Vec<(String, String, Vec<String>)> = inf
            .contradictions
            .iter()
            .map(|x| (x.subject.clone(), x.predicate.clone(), x.objects.clone()))
            .collect();
        let expectc: Vec<(String, String, Vec<String>)> = item["expectContradictions"]
            .as_array()
            .expect("expectContradictions")
            .iter()
            .map(|v| {
                (
                    v["subject"].as_str().unwrap().to_string(),
                    v["predicate"].as_str().unwrap().to_string(),
                    v["objects"].as_array().unwrap().iter().filter_map(|o| o.as_str().map(|s| s.to_string())).collect(),
                )
            })
            .collect();
        assert_eq!(gotc, expectc, "冲突检出与夹具不符（{why}）");

        let expects: Vec<String> = item["expectStale"]
            .as_array()
            .expect("expectStale")
            .iter()
            .filter_map(|v| v.as_str().map(|s| s.to_string()))
            .collect();
        assert_eq!(inf.stale, expects, "陈旧待复核与夹具不符（{why}）");
    }
}
