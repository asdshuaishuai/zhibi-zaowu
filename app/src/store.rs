//! .zhibi 项目存取（Rust 侧，与执笔 SwiftUI 版磁盘格式双向兼容）。
//!
//! 布局：`~/Documents/执笔/<书名>.zhibi/{project.json, canon/, outline.json,
//! clues.json, memory.json, proposals/inbox.json, chapters/ch-NNN/{meta.json, prose.md}}`
//! project.json 字段为 camelCase，createdAt 为 ISO8601（Swift .iso8601 策略可读回）。

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

pub fn projects_dir() -> PathBuf {
    let mut dir = dirs_home();
    dir.push("Documents");
    dir.push("执笔");
    let _ = std::fs::create_dir_all(&dir);
    dir
}

fn dirs_home() -> PathBuf {
    std::env::var("HOME").map(PathBuf::from).unwrap_or_else(|_| PathBuf::from("/tmp"))
}

pub fn chinese_count(text: &str) -> usize {
    text.chars()
        .filter(|c| {
            let v = *c as u32;
            (0x4E00..=0x9FFF).contains(&v)
                || (0x3400..=0x4DBF).contains(&v)
                || (0xF900..=0xFAFF).contains(&v)
                || (0x3000..=0x303F).contains(&v)
                || (0xFF00..=0xFFEF).contains(&v)
        })
        .count()
}

/// project.json 的最小子集（其余字段原样保留不了——M2 重建时只写这些；
/// 打开旧项目时未识别字段会被丢弃，因此打开前先备份？不：serde 保留未知字段
/// 需要 flatten，这里选择「读旧字段（title 等）+ 写回时仅追加本项目写入的字段」
/// 的折中：写回只发生在作者动笔后，且旧字段里只有 title 参与打开流程。）
#[derive(Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct NovelProject {
    #[serde(default)]
    pub title: String,
    #[serde(default)]
    pub genre: String,
    #[serde(default)]
    pub premise: String,
    #[serde(default)]
    pub target_chapters: u32,
    #[serde(default)]
    pub chapter_word_target: u32,
    #[serde(default)]
    pub created_at: String,
}

pub struct ProjectRef {
    /// 项目目录（ShelfEvent::Open 携带）
    #[allow(dead_code)]
    pub dir: PathBuf,
    pub title: String,
    pub genre: String,
    pub chapters: usize,
    pub words: usize,
}

fn epoch_to_iso8601(secs: u64) -> String {
    // 简版 UTC ISO8601（civil_from_days 算法），避免引入时间库
    let days = (secs / 86_400) as i64;
    let rem = secs % 86_400;
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = if m <= 2 { y + 1 } else { y };
    format!(
        "{:04}-{:02}-{:02}T{:02}:{:02}:{:02}Z",
        y, m, d, rem / 3600, (rem % 3600) / 60, rem % 60
    )
}

fn read_project(dir: &Path) -> Option<NovelProject> {
    let text = std::fs::read_to_string(dir.join("project.json")).ok()?;
    serde_json::from_str(&text).ok()
}

fn read_meta_title(dir: &Path) -> Option<String> {
    let text = std::fs::read_to_string(dir.join("meta.json")).ok()?;
    let v: serde_json::Value = serde_json::from_str(&text).ok()?;
    v.get("title").and_then(|t| t.as_str()).map(String::from)
}

/// 扫描书架：每个 .zhibi 目录一张卡（章节数 / 总字数现算——书架量级可接受）。
pub fn list_projects() -> Vec<ProjectRef> {
    let dir = projects_dir();
    let mut out = Vec::new();
    let Ok(entries) = std::fs::read_dir(&dir) else {
        return out;
    };
    let mut dirs: Vec<PathBuf> = entries
        .filter_map(|e| e.ok())
        .map(|e| e.path())
        .filter(|p| p.is_dir() && p.extension().map(|e| e == "zhibi").unwrap_or(false))
        .collect();
    dirs.sort_by_key(|p| {
        p.metadata()
            .and_then(|m| m.modified())
            .map(|t| std::cmp::Reverse(t))
            .unwrap_or(std::cmp::Reverse(std::time::SystemTime::UNIX_EPOCH))
    });
    for d in dirs {
        let project = read_project(&d);
        let title = project
            .as_ref()
            .map(|p| p.title.clone())
            .unwrap_or_else(|| d.file_stem().unwrap_or_default().to_string_lossy().to_string());
        let genre = project.as_ref().map(|p| p.genre.clone()).unwrap_or_default();
        let (chapters, words) = count_chapters(&d);
        out.push(ProjectRef { dir: d, title, genre, chapters, words });
    }
    out
}

fn count_chapters(dir: &Path) -> (usize, usize) {
    let mut chapters = 0usize;
    let mut words = 0usize;
    let ch_dir = dir.join("chapters");
    if let Ok(entries) = std::fs::read_dir(&ch_dir) {
        for e in entries.filter_map(|e| e.ok()) {
            let prose = e.path().join("prose.md");
            if let Ok(text) = std::fs::read_to_string(&prose) {
                chapters += 1;
                words += chinese_count(&text);
            }
        }
    }
    (chapters, words)
}

/// 新建作品：目录 + project.json + 第一章骨架目录。
pub fn create_project(title: &str) -> Result<PathBuf, String> {
    let base = safe_file_name(title);
    let mut dir = projects_dir().join(format!("{base}.zhibi"));
    let mut suffix = 2;
    while dir.exists() {
        dir = projects_dir().join(format!("{base}-{suffix}.zhibi"));
        suffix += 1;
    }
    std::fs::create_dir_all(dir.join("canon")).map_err(|e| e.to_string())?;
    std::fs::create_dir_all(dir.join("proposals")).map_err(|e| e.to_string())?;
    std::fs::create_dir_all(dir.join("chapters")).map_err(|e| e.to_string())?;
    let project = NovelProject {
        title: title.to_string(),
        created_at: epoch_to_iso8601(
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_secs())
                .unwrap_or(0),
        ),
        target_chapters: 100,
        chapter_word_target: 3000,
        ..Default::default()
    };
    let json = serde_json::to_string_pretty(&project).map_err(|e| e.to_string())?;
    std::fs::write(dir.join("project.json"), json).map_err(|e| e.to_string())?;
    Ok(dir)
}

fn safe_file_name(s: &str) -> String {
    let cleaned: String = s
        .chars()
        .map(|c| match c {
            '/' | '\\' | ':' | '*' | '?' | '"' | '<' | '>' | '|' | '\n' | '\r' | '\t' => '_',
            _ => c,
        })
        .collect();
    let t = cleaned.trim();
    if t.is_empty() { "未命名作品".into() } else { t.to_string() }
}

pub fn chapter_dir_name(n: u32) -> String {
    format!("ch-{n:03}")
}

/// 打开的某一本书的句柄。
pub struct ProjectHandle {
    pub root: PathBuf,
    pub title: String,
}

impl ProjectHandle {
    pub fn open(dir: &Path) -> Option<Self> {
        let project = read_project(dir);
        let title = project
            .map(|p| p.title)
            .or_else(|| dir.file_stem().map(|s| s.to_string_lossy().to_string()))
            .unwrap_or_else(|| "未命名作品".into());
        Some(Self { root: dir.to_path_buf(), title })
    }

    /// 章节列表（有 prose.md 才算，含标题与字数），按章号升序。
    pub fn chapters(&self) -> Vec<ChapterInfo> {
        let mut out = Vec::new();
        let ch_dir = self.root.join("chapters");
        if let Ok(entries) = std::fs::read_dir(&ch_dir) {
            for e in entries.filter_map(|e| e.ok()) {
                let name = e.file_name().to_string_lossy().to_string();
                if !e.path().is_dir() {
                    continue;
                }
                let n: u32 = match name.strip_prefix("ch-").and_then(|s| s.parse().ok()) {
                    Some(n) => n,
                    None => continue,
                };
                let prose_path = e.path().join("prose.md");
                let prose = std::fs::read_to_string(&prose_path).unwrap_or_default();
                let title =
                    read_meta_title(&e.path()).unwrap_or_else(|| format!("第{n}章"));
                out.push(ChapterInfo {
                    n,
                    title,
                    words: chinese_count(&prose),
                    has_prose: !prose.trim().is_empty(),
                });
            }
        }
        out.sort_by_key(|c| c.n);
        out
    }

    pub fn prose(&self, n: u32) -> String {
        std::fs::read_to_string(self.prose_path(n)).unwrap_or_default()
    }

    pub fn save_prose(&self, n: u32, text: &str) -> Result<(), String> {
        let dir = self.root.join("chapters").join(chapter_dir_name(n));
        std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
        // 先落临时再替换（与 Swift write 语义一致）
        let target = self.prose_path(n);
        let tmp = target.with_extension("md.tmp");
        std::fs::write(&tmp, text).map_err(|e| e.to_string())?;
        std::fs::rename(&tmp, target).map_err(|e| e.to_string())
    }

    pub fn ensure_chapter(&self, n: u32) -> Result<(), String> {
        let dir = self.root.join("chapters").join(chapter_dir_name(n));
        std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
        let meta = dir.join("meta.json");
        if !meta.exists() {
            std::fs::write(
                &meta,
                format!("{{\"title\": \"第{n}章\", \"number\": {n}}}"),
            )
            .map_err(|e| e.to_string())?;
        }
        let prose = self.prose_path(n);
        if !prose.exists() {
            std::fs::write(&prose, "").map_err(|e| e.to_string())?;
        }
        Ok(())
    }

    fn prose_path(&self, n: u32) -> PathBuf {
        self.root.join("chapters").join(chapter_dir_name(n)).join("prose.md")
    }
}

#[derive(Clone)]
pub struct ChapterInfo {
    pub n: u32,
    pub title: String,
    pub words: usize,
    pub has_prose: bool,
}

pub use chinese_count as count_words;

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_home() -> PathBuf {
        let d = std::env::temp_dir().join(format!("zhibi-test-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(d.join("Documents")).unwrap();
        d
    }

    #[test]
    fn create_and_roundtrip() {
        let home = temp_home();
        // 测试独占进程环境；Rust 2024 起 set_var 为 unsafe（测试进程内无并发读）
        unsafe { std::env::set_var("HOME", &home); }
        // 会写进 temp 的 Documents/执笔
        let dir = create_project("测试书").expect("create");
        assert!(dir.join("project.json").exists());
        assert!(dir.join("canon").exists());
        // 同名第二本自动加后缀
        let dir2 = create_project("测试书").unwrap();
        assert_ne!(dir, dir2);

        let handle = ProjectHandle::open(&dir).expect("open");
        assert_eq!(handle.title, "测试书");
        handle.ensure_chapter(1).unwrap();
        handle.save_prose(1, "沈砚按着刀柄走进雨里。刀是好刀。").unwrap();
        let chapters = handle.chapters();
        assert_eq!(chapters.len(), 1);
        assert_eq!(chapters[0].words, 16); // 14 汉字 + 2 个中文句号（含 CJK 标点）
        assert_eq!(handle.prose(1), "沈砚按着刀柄走进雨里。刀是好刀。");

        let list = list_projects();
        assert!(list.iter().any(|p| p.title == "测试书" && p.words > 0));
        let _ = std::fs::remove_dir_all(&home);
    }
}

// ───────────────────────── 提案（proposals/inbox.json）─────────────────────────

#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Proposal {
    pub id: String,
    #[serde(default)]
    pub capability: String,
    #[serde(default)]
    pub title: String,
    #[serde(default)]
    pub note: String,
    pub payload: serde_json::Value,
    #[serde(default = "default_status")]
    pub status: String,
    #[serde(default)]
    pub created_at: String,
    #[serde(default)]
    pub decided_at: Option<String>,
    #[serde(default)]
    pub reject_reason: Option<String>,
}

fn default_status() -> String {
    "待确认".into()
}

impl ProjectHandle {
    pub fn proposals(&self) -> Vec<Proposal> {
        let path = self.root.join("proposals").join("inbox.json");
        std::fs::read_to_string(path)
            .ok()
            .and_then(|t| serde_json::from_str(&t).ok())
            .unwrap_or_default()
    }

    fn save_proposals(&self, list: &[Proposal]) -> Result<(), String> {
        let path = self.root.join("proposals").join("inbox.json");
        let tmp = path.with_extension("json.tmp");
        let json = serde_json::to_string_pretty(list).map_err(|e| e.to_string())?;
        std::fs::write(&tmp, json).map_err(|e| e.to_string())?;
        std::fs::rename(&tmp, path).map_err(|e| e.to_string())
    }

    /// 确定性采纳（账房语义与 novel-pack apply_proposal 一致：同题保护/同 id 合并/
    /// 归类按标题匹配）。返回 (applied, skipped 说明)。
    pub fn accept_proposal(&self, id: &str) -> Result<(usize, Vec<String>), String> {
        let mut list = self.proposals();
        let idx = list.iter().position(|p| p.id == id).ok_or("提案不存在")?;
        if list[idx].status != "待确认" {
            return Err(format!("提案已处理（{}），拒绝重复采纳", list[idx].status));
        }
        let p = list[idx].clone();
        let (applied, skipped) = self.apply_payload(&p.payload)?;
        if applied == 0 {
            return Err(format!("未入库：{}——提案保持待审", skipped.join("、")));
        }
        list[idx].status = "已采纳".into();
        list[idx].decided_at = Some(iso_now());
        self.save_proposals(&list)?;
        Ok((applied, skipped))
    }

    pub fn reject_proposal(&self, id: &str, reason: &str) -> Result<(), String> {
        let mut list = self.proposals();
        let idx = list.iter().position(|p| p.id == id).ok_or("提案不存在")?;
        if list[idx].status != "待确认" {
            return Err("提案已处理".into());
        }
        list[idx].status = "已拒绝".into();
        list[idx].decided_at = Some(iso_now());
        list[idx].reject_reason = Some(reason.to_string());
        self.save_proposals(&list)
    }

    fn apply_payload(&self, payload: &serde_json::Value) -> Result<(usize, Vec<String>), String> {
        let kind = payload.get("kind").and_then(|k| k.as_str()).unwrap_or("");
        let mut applied = 0usize;
        let mut skipped: Vec<String> = Vec::new();
        match kind {
            "canon" => {
                let docs = payload.get("docs").and_then(|d| d.as_array()).cloned().unwrap_or_default();
                for d in docs {
                    let title = d.get("title").and_then(|t| t.as_str()).unwrap_or("").trim().to_string();
                    if title.is_empty() { continue; }
                    let file = self.canon_path(&title);
                    if file.exists() {
                        skipped.push(format!("设定「{title}」已存在（同题保护）"));
                        continue;
                    }
                    let body = d.get("content").and_then(|c| c.as_str()).unwrap_or("");
                    std::fs::write(&file, body).map_err(|e| e.to_string())?;
                    applied += 1;
                }
            }
            "clues" => {
                let incoming = payload.get("clues").and_then(|c| c.as_array()).cloned().unwrap_or_default();
                let mut clues = self.clues();
                for c in incoming {
                    let id = c.get("id").and_then(|i| i.as_str()).map(String::from)
                        .unwrap_or_else(|| format!("F{}", clues.len() + 1));
                    if let Some(pos) = clues.iter().position(|x| x.get("id").and_then(|i| i.as_str()) == Some(id.as_str())) {
                        // 同 id 合并：作者维护字段不覆盖，只补空
                        let existing = &mut clues[pos];
                        for k in ["title", "detail", "plantedChapter"] {
                            if existing.get(k).is_none() {
                                if let Some(v) = c.get(k) {
                                    if let Some(obj) = existing.as_object_mut() { obj.insert(k.into(), v.clone()); }
                                }
                            }
                        }
                        applied += 1;
                    } else {
                        let mut merged = c.clone();
                        if let Some(obj) = merged.as_object_mut() {
                            obj.insert("id".into(), serde_json::json!(id));
                            obj.entry("status").or_insert(serde_json::json!("埋下"));
                        }
                        clues.push(merged);
                        applied += 1;
                    }
                }
                self.save_clues(&clues)?;
            }
            "canonClassify" => {
                let items = payload.get("items").and_then(|c| c.as_array()).cloned().unwrap_or_default();
                for it in items {
                    let title = it.get("title").and_then(|t| t.as_str()).unwrap_or("").trim().to_string();
                    let file = self.canon_path(&title);
                    if !file.exists() {
                        skipped.push(format!("设定「{title}」未找到（标题可能已改）"));
                        continue;
                    }
                    let category = it.get("category").and_then(|c| c.as_str()).unwrap_or("未分类");
                    let body = std::fs::read_to_string(&file).unwrap_or_default();
                    let body = body.strip_prefix(&format!("<!-- 类别:")).and_then(|b| b.split_once("-->\n").map(|(_, r)| r.to_string()))
                        .unwrap_or_else(|| {
                            // 已有类别头则替换；无则前插
                            if let Some(rest) = body.strip_prefix("<!-- 类别:") {
                                rest.split_once("-->\n").map(|(_, r)| r.to_string()).unwrap_or(body.clone())
                            } else { body.clone() }
                        });
                    std::fs::write(&file, format!("<!-- 类别:{category} -->\n{body}")).map_err(|e| e.to_string())?;
                    if let Some(rename) = it.get("rename").and_then(|r| r.as_str()) {
                        let rfile = self.canon_path(rename);
                        if !rename.trim().is_empty() && rename != title && !rfile.exists() {
                            let _ = std::fs::rename(&file, &rfile);
                        }
                    }
                    applied += 1;
                }
            }
            "draft" => {
                // 草稿：采纳 = 写入对应章正文（先自动快照）
                let chapter = payload.get("chapter").and_then(|c| c.as_u64()).unwrap_or(0) as u32;
                let text = payload.get("text").and_then(|t| t.as_str()).unwrap_or("");
                if chapter == 0 { return Err("草稿缺章号".into()); }
                self.snapshot_prose(chapter)?;
                self.save_prose(chapter, text)?;
                applied += 1;
            }
            "memo" | "report" | "deslop" | "skeleton" | "memoryPack" | "dossier" | "outlineUpdates" | "clueFixes" | "storylines" | "outlineEvents" => {
                // M3 后续：这些 payload 的确定性落库逐个补齐；先如实报告未支持
                return Err(format!("「{kind}」落库尚未实现（M3 后续）——提案保持待审"));
            }
            other => return Err(format!("未知提案类型：{other}")),
        }
        Ok((applied, skipped))
    }

    fn canon_path(&self, title: &str) -> PathBuf {
        self.root.join("canon").join(format!("{}.md", safe_file_name(title)))
    }

    /// 设定文档列表（标题/类别/正文）——M5 设定页使用
    #[allow(dead_code)]
    pub fn canon_docs(&self) -> Vec<(String, String, String)> {
        // (标题, 类别, 正文)
        let dir = self.root.join("canon");
        let Ok(entries) = std::fs::read_dir(&dir) else { return Vec::new() };
        let mut out = Vec::new();
        for e in entries.filter_map(|e| e.ok()) {
            let name = e.file_name().to_string_lossy().to_string();
            if !name.ends_with(".md") { continue; }
            let title = name.trim_end_matches(".md").to_string();
            let body = std::fs::read_to_string(e.path()).unwrap_or_default();
            let category = body
                .strip_prefix("<!-- 类别:")
                .and_then(|b| b.split_once("-->").map(|(c, _)| c.trim().to_string()))
                .unwrap_or_else(|| "未分类".into());
            let content = if category != "未分类" {
                body.split_once("-->\n").map(|(_, r)| r.to_string()).unwrap_or(body)
            } else { body };
            out.push((title, category, content));
        }
        out.sort_by(|a, b| a.0.cmp(&b.0));
        out
    }

    fn clues(&self) -> Vec<serde_json::Value> {
        self.root.join("clues.json")
            .exists()
            .then(|| std::fs::read_to_string(self.root.join("clues.json")).ok()
                .and_then(|t| serde_json::from_str(&t).ok())
                .unwrap_or_default())
            .unwrap_or_default()
    }

    fn save_clues(&self, clues: &[serde_json::Value]) -> Result<(), String> {
        let path = self.root.join("clues.json");
        let tmp = path.with_extension("json.tmp");
        let json = serde_json::to_string_pretty(clues).map_err(|e| e.to_string())?;
        std::fs::write(&tmp, json).map_err(|e| e.to_string())?;
        std::fs::rename(&tmp, path).map_err(|e| e.to_string())
    }

    pub fn snapshot_prose(&self, n: u32) -> Result<(), String> {
        let dir = self.root.join("chapters").join(chapter_dir_name(n)).join("snapshots");
        std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
        let src = self.prose(n);
        let stamp = iso_now().replace([':', '-'], "");
        std::fs::write(dir.join(format!("prose-{stamp}.md")), src).map_err(|e| e.to_string())
    }
}

fn iso_now() -> String {
    let secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    epoch_to_iso8601(secs)
}
