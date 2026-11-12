//! 改动日志：谁在什么时候改了哪个文件、改前是什么、能不能撤。
//!
//! 为什么需要它：本书的写入有三个来源——作者手写、AI 提案被采纳、AI 在共写/代笔档
//! 直接改文件。后两者一旦写下去，作者就无从追溯“这一版是谁改的、能不能退回去”。
//! 日志把每次落盘变成一条可回滚的记录，这是“作者是创世主”这条铁律的工程落点。
//!
//! 设计取舍：
//! - **追加写 JSONL**，不覆盖。日志本身坏了也只是少一条历史，不会让书打不开。
//! - **前像存在日志旁边**（`journal/blobs/<hash>`），按内容寻址：同一份内容只存一次，
//!   反复保存同一章不会把磁盘撑爆。
//! - **撤销靠前像**，不靠反向 diff。后者要保证 diff/undiff 完全对称，前者是直接把
//!   文件写回原字节，语义简单得多。
//! - **冲突检测**：撤销前比对当前内容是否仍是“那次改动之后的样子”。作者在那之后
//!   又改过，就不是简单回退而是丢掉新内容，必须让他决定。

use std::collections::BTreeMap;
use std::io::Write;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

/// 谁改的。`Author` 是作者手写，`Ai` 带档位与 Agent 名（共写档由 relations 专属 Agent 改的会标出来）。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Actor {
    /// 作者本人（编辑器保存、拖卡改期、点采纳等）。
    Author,
    /// AI 直接写入（仅共写/代笔档）。
    Ai {
        /// 档位：cowrite / ghostwrite。
        mode: String,
        /// 执行写入的 Agent，如 "relations"；主 Agent 为空串。
        #[serde(default)]
        agent: String,
    },
}

impl Actor {
    pub fn label(&self) -> String {
        match self {
            Actor::Author => "作者".into(),
            Actor::Ai { mode, agent } => {
                let m = match mode.as_str() {
                    "cowrite" => "共写",
                    "ghostwrite" => "代笔",
                    other => other,
                };
                if agent.is_empty() {
                    format!("AI（{m}）")
                } else {
                    format!("AI（{m}·{agent}）")
                }
            }
        }
    }
}

/// 一条改动记录。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Change {
    /// 记录 id，撤销时按它定位。
    pub id: String,
    /// 毫秒时间戳（本地显示时换算）。
    pub ts: u64,
    pub actor: Actor,
    /// 相对书目录的路径，如 "chapters/ch-012/prose.md"。
    pub path: String,
    /// 改动类型：save / adopt / edit / remove。
    pub op: String,
    /// 改动后的内容哈希（用于冲突检测）。
    pub after_hash: String,
    /// 改动前的内容哈希；空串表示原来文件不存在（撤销 = 删除）。
    pub before_hash: String,
    /// 同一次操作（一个回合、一次采纳）的批次号，便于整批撤销。
    pub batch: String,
    /// 一句话说明改了什么，给人看。
    #[serde(default)]
    pub note: String,
}

/// 撤销的三种结果——调用方必须区分，不能把冲突当成功。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum UndoOutcome {
    /// 成功写回前像。
    Restored,
    /// 原来没有这个文件，已删除。
    Removed,
    /// 文件在那之后又被改过：拒绝撤销，需要作者决定。
    Conflict { expected: String, actual: String },
}

/// 改动日志（挂在书目录下的 `journal/`）。
pub struct Journal {
    root: PathBuf,
}

impl Journal {
    pub fn new(book_root: &Path) -> Self {
        Self {
            root: book_root.join("journal"),
        }
    }

    fn log_path(&self) -> PathBuf {
        self.root.join("changes.jsonl")
    }

    fn blob_dir(&self) -> PathBuf {
        self.root.join("blobs")
    }

    fn ensure(&self) -> Result<(), String> {
        std::fs::create_dir_all(self.blob_dir()).map_err(|e| e.to_string())
    }

    /// 内容寻址存一份前像，返回哈希。内容是空或文件不存在时返回空串（表示“原来没有”）。
    fn store_blob(&self, content: &str) -> Result<String, String> {
        if content.is_empty() {
            return Ok(String::new());
        }
        let hash = content_hash(content);
        let path = self.blob_dir().join(&hash);
        if !path.exists() {
            self.ensure()?;
            // 原子写：写一半崩溃不会留下半个 blob 被当成完整前像
            let tmp = self.blob_dir().join(format!("{hash}.tmp"));
            std::fs::write(&tmp, content).map_err(|e| e.to_string())?;
            std::fs::rename(&tmp, &path).map_err(|e| e.to_string())?;
        }
        Ok(hash)
    }

    fn read_blob(&self, hash: &str) -> Result<Option<String>, String> {
        if hash.is_empty() {
            return Ok(None);
        }
        let path = self.blob_dir().join(hash);
        if !path.exists() {
            return Ok(None);
        }
        std::fs::read_to_string(&path).map(Some).map_err(|e| e.to_string())
    }

    /// 记一条改动。`before` 是改动前的完整内容（新文件给空串）。
    ///
    /// 这是所有落盘的统一入口：作者保存、AI 写入、采纳提案都走它，
    /// 所以“改了什么都可追溯”不依赖调用方自觉。
    pub fn record(
        &self,
        book_root: &Path,
        rel_path: &str,
        before: &str,
        after: &str,
        actor: &Actor,
        op: &str,
        batch: &str,
        note: &str,
    ) -> Result<Change, String> {
        self.ensure()?;
        // 内容没变就不记：反复自动保存会灌满日志，反而查不到真改动
        if before == after {
            return Err("内容未变，未记录".into());
        }
        let before_hash = self.store_blob(before)?;
        let after_hash = content_hash(after);
        let change = Change {
            id: format!("c{}", uuid_like()),
            ts: now_millis(),
            actor: actor.clone(),
            path: rel_path.to_string(),
            op: op.to_string(),
            after_hash,
            before_hash,
            batch: batch.to_string(),
            note: note.to_string(),
        };
        let mut line = serde_json::to_string(&change).map_err(|e| e.to_string())?;
        line.push('\n');
        let mut f = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(self.log_path())
            .map_err(|e| e.to_string())?;
        f.write_all(line.as_bytes()).map_err(|e| e.to_string())?;
        let _ = book_root;
        Ok(change)
    }

    /// 读全部改动（新的在前）。坏行跳过——日志坏了不该让整个功能不可用。
    pub fn all(&self) -> Vec<Change> {
        let Ok(text) = std::fs::read_to_string(self.log_path()) else {
            return Vec::new();
        };
        let mut out: Vec<Change> = text
            .lines()
            .filter(|l| !l.trim().is_empty())
            .filter_map(|l| serde_json::from_str::<Change>(l).ok())
            .collect();
        out.reverse();
        out
    }

    /// 撤销一条改动。
    ///
    /// 先比对 `after_hash`：当前文件已经不是“那次改动之后的样子”，说明作者之后又改过，
    /// 此时回退会静默丢掉新内容，所以返回 `Conflict` 让上层问作者。
    pub fn undo(&self, book_root: &Path, change_id: &str) -> Result<UndoOutcome, String> {
        let change = self
            .all()
            .into_iter()
            .find(|c| c.id == change_id)
            .ok_or_else(|| format!("找不到改动记录 {change_id}"))?;
        let target = book_root.join(&change.path);
        let current = std::fs::read_to_string(&target).unwrap_or_default();
        let actual = if current.is_empty() {
            String::new()
        } else {
            content_hash(&current)
        };
        // 空串含义：那一刻文件不存在。当前也空 → 没有冲突。
        if actual != change.after_hash {
            return Ok(UndoOutcome::Conflict {
                expected: change.after_hash,
                actual,
            });
        }
        match self.read_blob(&change.before_hash)? {
            Some(before) => {
                if let Some(parent) = target.parent() {
                    std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
                }
                // 撤销本身也要原子：写一半崩掉会毁掉正文
                let tmp = target.with_extension("undo.tmp");
                std::fs::write(&tmp, &before).map_err(|e| e.to_string())?;
                std::fs::rename(&tmp, &target).map_err(|e| e.to_string())?;
                Ok(UndoOutcome::Restored)
            }
            // 前像为空 = 那次改动是新建文件；撤销就是删掉它
            None => {
                if target.exists() {
                    std::fs::remove_file(&target).map_err(|e| e.to_string())?;
                }
                Ok(UndoOutcome::Removed)
            }
        }
    }

    /// 按批次撤销（一个回合 / 一次采纳）。返回每条的结果——部分失败要如实报出。
    pub fn undo_batch(
        &self,
        book_root: &Path,
        batch: &str,
    ) -> Vec<(Change, Result<UndoOutcome, String>)> {
        let changes: Vec<Change> = self
            .all()
            .into_iter()
            .filter(|c| c.batch == batch)
            .collect();
        // all() 已经是「新的在前」，直接按这个顺序撤：
        // 同一文件同批次多次改动时，必须先撤最后一次，否则当前内容对不上前一次的前像。
        changes
            .into_iter()
            .map(|c| {
                let r = self.undo(book_root, &c.id);
                (c, r)
            })
            .collect()
    }

    /// 按实体/章节筛最近记录。改动记录页目前用 `groups` 按回合聚合，
    /// 保留按路径筛的能力给后续“只看某个人物的改动”。
    #[allow(dead_code)]
    pub fn recent(&self, path_contains: &str, limit: usize) -> Vec<Change> {
        self.all()
            .into_iter()
            .filter(|c| path_contains.is_empty() || c.path.contains(path_contains))
            .take(limit)
            .collect()
    }

    /// 按批次聚合，给“一次会话改了什么”的视图。
    pub fn groups(&self, limit: usize) -> BTreeMap<String, Vec<Change>> {
        let mut out: BTreeMap<String, Vec<Change>> = BTreeMap::new();
        for c in self.all().into_iter().take(limit * 8) {
            out.entry(c.batch.clone()).or_default().push(c);
        }
        out
    }
}

/// 内容哈希：用标准库的 DefaultHasher。这里只用于“是不是同一份内容”，
/// 不是安全用途，不需要密码学强度，也就不引额外依赖。
pub fn content_hash(s: &str) -> String {
    use std::hash::{Hash, Hasher};
    let mut h = std::collections::hash_map::DefaultHasher::new();
    s.hash(&mut h);
    format!("{:016x}", h.finish())
}

pub fn now_millis() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

fn uuid_like() -> String {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    let pid = std::process::id();
    format!("{nanos:x}{pid:x}")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tmp_book(tag: &str) -> PathBuf {
        let p = std::env::temp_dir().join(format!("zhibi-journal-{tag}"));
        let _ = std::fs::remove_dir_all(&p);
        std::fs::create_dir_all(&p).unwrap();
        p
    }

    fn write(root: &Path, rel: &str, text: &str) {
        let p = root.join(rel);
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(p, text).unwrap();
    }

    #[test]
    fn records_and_restores_previous_content() {
        let root = tmp_book("restore");
        let j = Journal::new(&root);
        write(&root, "chapters/ch-001/prose.md", "旧正文");
        j.record(
            &root,
            "chapters/ch-001/prose.md",
            "旧正文",
            "新正文",
            &Actor::Ai { mode: "cowrite".into(), agent: "reviewer".into() },
            "edit",
            "b1",
            "AI 改了一句",
        )
        .unwrap();
        write(&root, "chapters/ch-001/prose.md", "新正文");

        let logs = j.all();
        assert_eq!(logs.len(), 1);
        assert!(logs[0].actor.label().contains("共写"));
        assert!(logs[0].actor.label().contains("reviewer"));

        let outcome = j.undo(&root, &logs[0].id).unwrap();
        assert_eq!(outcome, UndoOutcome::Restored);
        assert_eq!(
            std::fs::read_to_string(root.join("chapters/ch-001/prose.md")).unwrap(),
            "旧正文",
            "撤销必须逐字写回前像"
        );
    }

    #[test]
    fn undo_refuses_when_author_edited_afterwards() {
        let root = tmp_book("conflict");
        let j = Journal::new(&root);
        write(&root, "a.md", "v1");
        let c = j
            .record(&root, "a.md", "v1", "v2", &Actor::Author, "save", "b", "")
            .unwrap();
        // AI 写完后作者又手改了一版
        write(&root, "a.md", "v3 作者的改动");
        let outcome = j.undo(&root, &c.id).unwrap();
        assert!(
            matches!(outcome, UndoOutcome::Conflict { .. }),
            "作者之后改过就必须报冲突，不能静默覆盖"
        );
        assert_eq!(
            std::fs::read_to_string(root.join("a.md")).unwrap(),
            "v3 作者的改动",
            "冲突时文件不能被改动"
        );
    }

    #[test]
    fn undo_of_new_file_deletes_it() {
        let root = tmp_book("newfile");
        let j = Journal::new(&root);
        let c = j
            .record(&root, "characters.json", "", "[]", &Actor::Author, "save", "b", "")
            .unwrap();
        write(&root, "characters.json", "[]");
        assert_eq!(j.undo(&root, &c.id).unwrap(), UndoOutcome::Removed);
        assert!(!root.join("characters.json").exists());
    }

    #[test]
    fn identical_content_is_not_logged() {
        let root = tmp_book("nochange");
        let j = Journal::new(&root);
        // 自动保存每次都触发：内容没变就不该灌日志
        assert!(j.record(&root, "a.md", "same", "same", &Actor::Author, "save", "b", "").is_err());
        assert!(j.all().is_empty());
    }

    #[test]
    fn batch_undo_walks_backwards() {
        let root = tmp_book("batch");
        let j = Journal::new(&root);
        write(&root, "a.md", "v1");
        j.record(&root, "a.md", "v1", "v2", &Actor::Ai { mode: "cowrite".into(), agent: String::new() }, "edit", "turn1", "").unwrap();
        write(&root, "a.md", "v2");
        j.record(&root, "a.md", "v2", "v3", &Actor::Ai { mode: "cowrite".into(), agent: String::new() }, "edit", "turn1", "").unwrap();
        write(&root, "a.md", "v3");

        let results = j.undo_batch(&root, "turn1");
        assert_eq!(results.len(), 2);
        assert!(results.iter().all(|(_, r)| matches!(r, Ok(UndoOutcome::Restored))));
        assert_eq!(
            std::fs::read_to_string(root.join("a.md")).unwrap(),
            "v1",
            "整批撤销后应回到该回合之前的状态"
        );
    }

    #[test]
    fn blobs_are_content_addressed_and_deduped() {
        let root = tmp_book("dedupe");
        let j = Journal::new(&root);
        let big = "同一段内容".repeat(500);
        write(&root, "a.md", &big);
        j.record(&root, "a.md", &big, "x", &Actor::Author, "save", "b1", "").unwrap();
        write(&root, "b.md", &big);
        j.record(&root, "b.md", &big, "y", &Actor::Author, "save", "b2", "").unwrap();
        let blobs: Vec<_> = std::fs::read_dir(root.join("journal/blobs")).unwrap().collect();
        assert_eq!(blobs.len(), 1, "相同内容只应存一份前像");
    }
}

/// 正文里“有 AI 参与”的段落标记。
///
/// 存在 `chapters/ch-NNN/ai-marks.json`，与正文分开：正文是作者的作品，不该被
/// 元数据污染；标记只在编辑器左缘画墨点，不影响导出。
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct AiMark {
    /// 该段在正文里的起始字符偏移。
    pub start: usize,
    /// 结束偏移（不含）。
    pub end: usize,
    /// 对应改动记录的 id，可回跳到当时的轨迹。
    pub change_id: String,
    /// 谁写的：cowrite / ghostwrite。
    pub mode: String,
}

impl Journal {
    fn marks_path(&self, book_root: &Path, chapter: u32) -> PathBuf {
        book_root
            .join("chapters")
            .join(crate::store::chapter_dir_name(chapter))
            .join("ai-marks.json")
    }

    /// 读某章的 AI 参与标记。文件不存在或坏了都当空——标记丢了不该让正文打不开。
    pub fn marks(&self, book_root: &Path, chapter: u32) -> Vec<AiMark> {
        std::fs::read_to_string(self.marks_path(book_root, chapter))
            .ok()
            .and_then(|t| serde_json::from_str(&t).ok())
            .unwrap_or_default()
    }

    /// 记一段 AI 参与的区间。
    ///
    /// 目前 AI 直写正文发生在 novel-pack（独立进程），由 `write_prose` 自己写
    /// ai-marks.json；这个 Rust 侧入口留给「宿主代 AI 写入」的路径（如未来批量代笔）。
    #[allow(dead_code)]
    pub fn mark(
        &self,
        book_root: &Path,
        chapter: u32,
        mark: AiMark,
    ) -> Result<(), String> {
        let path = self.marks_path(book_root, chapter);
        let mut marks = self.marks(book_root, chapter);
        // 同一区间重复标记（比如重试）只留一条
        if !marks.iter().any(|m| m.start == mark.start && m.end == mark.end) {
            marks.push(mark);
            marks.sort_by_key(|m| m.start);
            if let Some(parent) = path.parent() {
                std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
            }
            let json = serde_json::to_string_pretty(&marks).map_err(|e| e.to_string())?;
            std::fs::write(&path, json).map_err(|e| e.to_string())?;
        }
        Ok(())
    }

    /// 清掉某章的全部标记（作者整章重写后调用）。
    #[allow(dead_code)]
    pub fn clear_marks(&self, book_root: &Path, chapter: u32) -> Result<(), String> {
        let path = self.marks_path(book_root, chapter);
        if path.exists() {
            std::fs::remove_file(path).map_err(|e| e.to_string())?;
        }
        Ok(())
    }
}

/// 写作档位：决定 AI 能写到哪一步。默认最保守。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WritingMode {
    /// 辅助：AI 只能提提案，任何落库都要作者点采纳。
    Assist,
    /// 共写：AI 可直接改台账与设定，但正文只给建议（走 offer_rewrite/续写）。
    Cowrite,
    /// 代笔：AI 可写正文，但仅限作者授权的章节范围。
    Ghostwrite,
}

impl WritingMode {
    pub fn from_key(k: &str) -> Self {
        match k.trim() {
            "cowrite" => WritingMode::Cowrite,
            "ghostwrite" => WritingMode::Ghostwrite,
            _ => WritingMode::Assist,
        }
    }

    pub fn key(self) -> &'static str {
        match self {
            WritingMode::Assist => "assist",
            WritingMode::Cowrite => "cowrite",
            WritingMode::Ghostwrite => "ghostwrite",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            WritingMode::Assist => "辅助",
            WritingMode::Cowrite => "共写",
            WritingMode::Ghostwrite => "代笔",
        }
    }

    /// 作者对某章的代笔授权范围。
    ///
    /// 存在 `zhibi-auth.json`（不在 DSH 沙箱可写范围外也要读得到，所以放书目录）；
    /// 授权是显式的：没授权就是空，代笔档也写不了正文。
    /// 权限判断的实际执行方是 novel-pack（它读同一份 zhibi-auth.json）。
    /// 这里保留一份等价实现供界面预判与测试。
    #[allow(dead_code)]
    pub fn allows_chapter(self, book_root: &Path, chapter: u32) -> bool {
        if self != WritingMode::Ghostwrite {
            return false;
        }
        let path = book_root.join("zhibi-auth.json");
        let Ok(text) = std::fs::read_to_string(path) else {
            return false;
        };
        let Ok(v) = serde_json::from_str::<serde_json::Value>(&text) else {
            return false;
        };
        v.get("ghostwriteChapters")
            .and_then(|c| c.as_array())
            .map(|arr| arr.iter().any(|x| x.as_u64() == Some(chapter as u64)))
            .unwrap_or(false)
    }
}

#[cfg(test)]
mod mode_tests {
    use super::*;

    fn book(tag: &str) -> PathBuf {
        let p = std::env::temp_dir().join(format!("zhibi-mode-{tag}"));
        let _ = std::fs::remove_dir_all(&p);
        std::fs::create_dir_all(&p).unwrap();
        p
    }

    #[test]
    fn assist_and_cowrite_never_write_prose() {
        let root = book("perm-assist");
        std::fs::write(
            root.join("zhibi-auth.json"),
            r#"{"ghostwriteChapters":[3]}"#,
        )
        .unwrap();
        // 即使授权了第 3 章，辅助/共写档也不能写正文
        assert!(!WritingMode::Assist.allows_chapter(&root, 3));
        assert!(!WritingMode::Cowrite.allows_chapter(&root, 3));
    }

    #[test]
    fn ghostwrite_needs_explicit_chapter_authorization() {
        let root = book("perm-ghost");
        // 没授权文件：代笔档什么都写不了
        assert!(!WritingMode::Ghostwrite.allows_chapter(&root, 3));
        std::fs::write(root.join("zhibi-auth.json"), r#"{"ghostwriteChapters":[3]}"#).unwrap();
        assert!(WritingMode::Ghostwrite.allows_chapter(&root, 3));
        // 未授权的章节仍然不许写
        assert!(!WritingMode::Ghostwrite.allows_chapter(&root, 4));
    }

    #[test]
    fn marks_round_trip_and_dedupe_same_range() {
        let root = book("marks");
        let j = Journal::new(&root);
        let m = AiMark { start: 10, end: 20, change_id: "c1".into(), mode: "cowrite".into() };
        j.mark(&root, 2, m.clone()).unwrap();
        j.mark(&root, 2, m.clone()).unwrap();
        let got = j.marks(&root, 2);
        assert_eq!(got.len(), 1, "同一区间不该重复标记");
        assert_eq!(got[0], m);
    }

    #[test]
    fn corrupt_marks_file_does_not_break_prose() {
        let root = book("marks-corrupt");
        let dir = root.join("chapters/ch-001");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("ai-marks.json"), "{ 坏 json").unwrap();
        // 坏文件当空读，不能 panic
        assert!(Journal::new(&root).marks(&root, 1).is_empty());
    }
}

/// 书级的写作授权：档位 + 代笔章节。
///
/// 放在书目录的 `zhibi-auth.json`：宿主切档位时写它，novel-pack 每次工具调用重读，
/// 所以切换立即生效、不必重启 AI 运行时（重启会丢正在跑的回合）。
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct BookAuth {
    /// assist / cowrite / ghostwrite
    pub writing_mode: String,
    /// 作者显式授权“代笔”的章节号
    pub ghostwrite_chapters: Vec<u32>,
}

impl BookAuth {
    pub fn load(book_root: &Path) -> Self {
        std::fs::read_to_string(book_root.join("zhibi-auth.json"))
            .ok()
            .and_then(|t| serde_json::from_str(&t).ok())
            .unwrap_or_default()
    }

    pub fn save(&self, book_root: &Path) -> Result<(), String> {
        let json = serde_json::to_string_pretty(self).map_err(|e| e.to_string())?;
        let path = book_root.join("zhibi-auth.json");
        let tmp = path.with_extension("tmp");
        std::fs::write(&tmp, json).map_err(|e| e.to_string())?;
        std::fs::rename(&tmp, &path).map_err(|e| e.to_string())
    }

    pub fn mode(&self) -> WritingMode {
        WritingMode::from_key(&self.writing_mode)
    }

    /// 切换档位。切回辅助时清掉代笔授权——降级要清权限，不能留着“以后再用”。
    pub fn set_mode(&mut self, mode: WritingMode) {
        self.writing_mode = mode.key().to_string();
        if mode != WritingMode::Ghostwrite {
            self.ghostwrite_chapters.clear();
        }
    }

    /// 授权/取消某章的代笔。
    pub fn toggle_chapter(&mut self, chapter: u32) {
        if let Some(pos) = self.ghostwrite_chapters.iter().position(|c| *c == chapter) {
            self.ghostwrite_chapters.remove(pos);
        } else {
            self.ghostwrite_chapters.push(chapter);
        }
        self.ghostwrite_chapters.sort_unstable();
    }
}

#[cfg(test)]
mod auth_tests {
    use super::*;

    fn book(tag: &str) -> PathBuf {
        let p = std::env::temp_dir().join(format!("zhibi-auth-{tag}"));
        let _ = std::fs::remove_dir_all(&p);
        std::fs::create_dir_all(&p).unwrap();
        p
    }

    #[test]
    fn auth_round_trips_through_disk() {
        let root = book("roundtrip");
        let mut a = BookAuth::default();
        a.set_mode(WritingMode::Ghostwrite);
        a.toggle_chapter(3);
        a.toggle_chapter(5);
        a.save(&root).unwrap();

        let loaded = BookAuth::load(&root);
        assert_eq!(loaded.mode(), WritingMode::Ghostwrite);
        assert_eq!(loaded.ghostwrite_chapters, vec![3, 5]);
        // novel-pack 读同一份文件，所以两边判断必须一致
        assert!(WritingMode::Ghostwrite.allows_chapter(&root, 3));
    }

    #[test]
    fn downgrading_clears_ghostwrite_authorization() {
        let mut a = BookAuth::default();
        a.set_mode(WritingMode::Ghostwrite);
        a.toggle_chapter(7);
        // 降回共写：授权必须一起清掉，否则以后再升代笔会意外获得旧权限
        a.set_mode(WritingMode::Cowrite);
        assert!(a.ghostwrite_chapters.is_empty());
    }

    #[test]
    fn toggling_a_chapter_twice_removes_it() {
        let mut a = BookAuth::default();
        a.toggle_chapter(2);
        assert_eq!(a.ghostwrite_chapters, vec![2]);
        a.toggle_chapter(2);
        assert!(a.ghostwrite_chapters.is_empty());
    }
}
