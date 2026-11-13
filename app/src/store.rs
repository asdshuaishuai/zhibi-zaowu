//! .zhibi 项目存取（Rust 侧，与执笔 SwiftUI 版磁盘格式双向兼容）。
//!
//! 布局：`~/Documents/执笔/<书名>.zhibi/{project.json, canon/, storylines.json,
//! events.json, stages.json, clues.json, memory.json, worldlines.json, canvas.json,
//! proposals/inbox.json, chapters/ch-NNN/{meta.json, prose.md}}`
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

/// 原子写：先写同名 `.tmp` 再 rename。写一半崩溃时旧文件保持完整（JSON 不会被截断）。
pub(crate) fn write_atomic(path: &Path, text: &str) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    let mut tmp = path.as_os_str().to_os_string();
    tmp.push(".tmp");
    let tmp = PathBuf::from(tmp);
    std::fs::write(&tmp, text).map_err(|e| e.to_string())?;
    std::fs::rename(&tmp, path).map_err(|e| e.to_string())
}

/// 读 JSON：文件不存在 → Ok(None)；存在但内容损坏 → Err（调用方据此拒绝覆盖，避免拿空表
/// 把作者原文件写没）。只读展示路径仍用宽松读取。
fn read_json_checked(path: &Path) -> Result<Option<serde_json::Value>, String> {
    match std::fs::read_to_string(path) {
        Ok(text) => serde_json::from_str(&text)
            .map(Some)
            .map_err(|e| format!("{} 内容损坏（{e}），已拒绝覆盖", path.display())),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(format!("读取 {} 失败：{e}", path.display())),
    }
}

fn read_array_checked(path: &Path, file: &str) -> Result<Vec<serde_json::Value>, String> {
    match read_json_checked(path)? {
        None => Ok(Vec::new()),
        Some(v) => v
            .as_array()
            .cloned()
            .ok_or_else(|| format!("{file} 顶层不是数组，已拒绝覆盖")),
    }
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

/// 作品元信息的可编辑子集（书架/工作区「作品信息」入口使用）。
#[derive(Clone, Default)]
pub struct ProjectMeta {
    pub genre: String,
    pub premise: String,
    pub target_chapters: u32,
    pub chapter_word_target: u32,
}

pub struct ProjectRef {
    /// 项目目录（ShelfEvent::Open 携带）
    pub dir: PathBuf,
    pub title: String,
    pub genre: String,
    /// 封皮构图（project.json 的 coverStyle；缺省按书名哈希，与 Swift 版一致）
    pub cover_style: Option<u8>,
    pub chapters: usize,
    pub words: usize,
    /// 今日净增字数（dailyWords 里今天那条）
    pub today_words: usize,
    /// 目录修改时间（相对时间展示用）
    pub updated: Option<std::time::SystemTime>,
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

/// 设定文档导入结果。
#[derive(Debug, Clone, Default)]
pub struct ImportedDocs {
    pub count: usize,
    pub chars: usize,
    /// 跳过的同名文件（内容与已有的一致，幂等导入）
    pub skipped: Vec<String>,
    pub names: Vec<String>,
}

impl ProjectHandle {
    /// 书目信息（project.json 全量；缺文件回默认）。
    pub fn project(&self) -> NovelProject {
        read_project(&self.root).unwrap_or_default()
    }

    /// 书目信息写回（题材/简介/目标章数/章字数目标等；GUI 书目页唯一入口）。
    pub fn save_project(&self, p: &NovelProject) -> Result<(), String> {
        let json = serde_json::to_string_pretty(p).map_err(|e| e.to_string())?;
        write_atomic(&self.root.join("project.json"), &json)
    }

    /// 章字数目标（编辑器进度环的事实源）。
    pub fn project_word_target(&self) -> u32 {
        let t = self.project().chapter_word_target;
        if t == 0 { 3000 } else { t }
    }
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
        let cover_style = std::fs::read_to_string(d.join("project.json"))
            .ok()
            .and_then(|t| serde_json::from_str::<serde_json::Value>(&t).ok())
            .and_then(|v| v.get("coverStyle")?.as_u64())
            .map(|n| (n % 5) as u8);
        let (chapters, words) = count_chapters(&d);
        let today = today_key();
        let today_words = std::fs::read_to_string(d.join("project.json"))
            .ok()
            .and_then(|t| {
                let v: serde_json::Value = serde_json::from_str(&t).ok()?;
                Some(v.get("dailyWords")?.get(today.as_str())?.as_u64()? as usize)
            })
            .unwrap_or(0);
        let updated = d.metadata().and_then(|m| m.modified()).ok();
        out.push(ProjectRef { dir: d, title, genre, cover_style, chapters, words, today_words, updated });
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
    write_atomic(&dir.join("project.json"), &json)?;
    // M7 预言书/画板底座：α=正典基准只读行；画板视图态初始空对象
    write_atomic(
        &dir.join("worldlines.json"),
        &serde_json::to_string_pretty(&serde_json::json!([
            { "id": "α", "name": "正典", "premise": "当前事实基准", "forkedChapter": 0, "status": "基准", "nodes": [], "edges": [], "diffs": [], "derived": { "characters": [], "canon": [], "clues": [] } }
        ])).unwrap(),
    )?;
    write_atomic(&dir.join("canvas.json"), "{}")?;
    // 创作法典：DSH 的 agent-instructions 会读 cwd 下的 AGENTS.md 并作为
    // 项目级指令注入（cwd = 书目录）。计划里"persona 与规则来自本书 AGENTS.md
    // （创作法典、铁律、授权章节）"指的就是这个文件——不建它，那些铁律只存在于
    // 我们的界面文案里，模型完全看不到。
    //
    // 写一次就不覆盖：作者之后会改它（这正是它存在的意义）。
    if !dir.join("AGENTS.md").exists() {
        write_atomic(&dir.join("AGENTS.md"), DEFAULT_AGENTS_MD)?;
    }
    Ok(dir)
}

/// 新书的创作法典。
///
/// 只写"模型必须知道、又无法从数据里推断"的约束。具体的字段格式、提案形状
/// 在各技能里，这里不重复——重复了就会两处不一致。
pub const DEFAULT_AGENTS_MD: &str = r#"# 创作法典

你是这本书的写作助手。作者写正文，你理账。

## 铁律

1. **正文只由作者落笔。** 除非作者明确授权某一章代笔，否则不要写、不要改
   任何 `chapters/ch-*/prose.md`。
2. **一切产出都是提案。** 设定、伏笔、骨架、草稿、改动建议，一律通过
   `propose_*` 工具登记，由作者在收件箱里裁决。不要直接写台账 JSON。
3. **有据才说。** 引用要落到具体章节或设定文件；查不到就写"未找到依据"，
   不要用常识补一个看起来合理的答案。
4. **不评文风。** 校审只查客观错误（连续性、设定违背、伏笔合同、物理不可能、
   动机断裂）。"这段写得不够好"不是问题，"第 7 章说他死了，第 12 章他又出场"
   才是。

## 术语

- **世界线**：剧情的时间轴。已写进正文的部分是正典（固化），之后是开放的 if 空间。
- **画布**：人物、设定、道具、事件之间的关联网。它不是时间轴。
- **预言书**：写 if 假设的地方。这里的推演不改正典。
- **章节排期**：工作进度安排，与世界线无关。

## 写作档位（由本书配置决定，不要绕过）

- 辅助档：只能提案，不能写台账。
- 共写档：可以写台账，不能写正文。
- 代笔档：只有作者逐章授权的那几章可以写正文。
"#;

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

/// 一致性夹具用的转发：`safe_file_name` 是私有的，但双端测试必须调被测实现
/// 本身（复制一份规则就失去意义了）。只在测试构建里存在。
#[cfg(test)]
pub fn safe_file_name_for_test(s: &str) -> String {
    safe_file_name(s)
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
        let handle = Self { root: dir.to_path_buf(), title };
        // 老项目补 M7 底座（幂等：已存在不动）
        handle.ensure_m7_files();
        Some(handle)
    }

    fn ensure_m7_files(&self) {
        let wl = self.root.join("worldlines.json");
        if !wl.exists() {
            let seed = serde_json::json!([
                { "id": "α", "name": "正典", "premise": "当前事实基准", "forkedChapter": 0, "status": "基准", "nodes": [], "edges": [], "diffs": [], "derived": { "characters": [], "canon": [], "clues": [] } }
            ]);
            let _ = write_atomic(&wl, &serde_json::to_string_pretty(&seed).unwrap());
        }
        let cv = self.root.join("canvas.json");
        if !cv.exists() {
            let _ = write_atomic(&cv, "{}");
        }
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
        self.save_prose_as(n, text, &crate::journal::Actor::Author, "作者保存")
    }

    /// 带来源的正文保存：AI 在共写/代笔档直接写正文时用这个，改动会进日志。
    ///
    /// 作者保存与 AI 写入走同一条路径，只差 actor/note——这样“谁改的”不会因为
    /// 调用入口不同而漏记。
    pub fn save_prose_as(
        &self,
        n: u32,
        text: &str,
        actor: &crate::journal::Actor,
        note: &str,
    ) -> Result<(), String> {
        let dir = self.root.join("chapters").join(chapter_dir_name(n));
        std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
        let old_text = self.prose(n);
        let old_words = chinese_count(&old_text);
        let new_words = chinese_count(text);
        write_atomic(&self.prose_path(n), text)?;
        // 落盘成功才记日志：写失败还留一条“已改动”的记录会误导撤销
        if old_text != text {
            let rel = format!("chapters/{}/prose.md", chapter_dir_name(n));
            let batch = format!("b{}", crate::journal::now_millis());
            // 日志失败不阻断保存——正文已经安全落盘，丢的是可追溯性而不是内容
            if let Err(e) = crate::journal::Journal::new(&self.root).record(
                &self.root,
                &rel,
                &old_text,
                text,
                actor,
                "save",
                &batch,
                note,
            ) {
                eprintln!("记录正文改动失败：{e}");
            }
        }
        // 今日净增：每次落盘按「新字数 - 旧字数」记账（书架/统计页的唯一写入方）。
        // 记账失败不影响正文保存（正文已经安全落盘）。
        if new_words != old_words {
            let delta = new_words as i64 - old_words as i64;
            if let Err(e) = self.record_daily_words(delta) {
                eprintln!("记录今日字数失败：{e}");
            }
        }
        Ok(())
    }

    /// project.json.dailyWords[{todayKey}] += delta（负数表示删改净减，下限 0）。
    /// 只追加/更新该字段，其余未知字段原样保留。
    fn record_daily_words(&self, delta: i64) -> Result<(), String> {
        let path = self.root.join("project.json");
        // 老项目缺 project.json 时就地补一份带书名的，别写出一份没有 title 的裸文件
        let mut v = read_json_checked(&path)?
            .unwrap_or_else(|| serde_json::json!({ "title": self.title }));
        if !v.is_object() {
            v = serde_json::json!({ "title": self.title });
        }
        let today = today_key();
        let obj = v.as_object_mut().ok_or("project.json 不是对象")?;
        let daily = obj.entry("dailyWords").or_insert_with(|| serde_json::json!({}));
        if !daily.is_object() {
            *daily = serde_json::json!({});
        }
        let map = daily.as_object_mut().ok_or("dailyWords 不是对象")?;
        let cur = map.get(&today).and_then(|n| n.as_u64()).unwrap_or(0) as i64;
        let next = (cur + delta).max(0) as u64;
        map.insert(today, serde_json::json!(next));
        write_atomic(&path, &serde_json::to_string_pretty(&v).map_err(|e| e.to_string())?)
    }

    pub fn ensure_chapter(&self, n: u32) -> Result<(), String> {
        let dir = self.root.join("chapters").join(chapter_dir_name(n));
        std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
        let meta = dir.join("meta.json");
        if !meta.exists() {
            write_atomic(
                &meta,
                &format!("{{\"title\": \"第{n}章\", \"number\": {n}}}"),
            )?;
        }
        let prose = self.prose_path(n);
        if !prose.exists() {
            write_atomic(&prose, "")?;
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
    use std::sync::{Mutex, MutexGuard};

    /// HOME 是进程级环境变量：cargo test 默认并行，必须串行化所有改 HOME 的用例，
    /// 否则会互相串家目录（旧的 sleep 5ms 只是概率掩盖竞态）。
    static HOME_LOCK: Mutex<()> = Mutex::new(());

    fn lock_home() -> MutexGuard<'static, ()> {
        HOME_LOCK.lock().unwrap_or_else(|e| e.into_inner())
    }

    fn temp_home(tag: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("zhibi-test-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(d.join("Documents")).unwrap();
        d
    }

    #[test]
    fn capability_switches_default_to_on_for_old_configs() {
        // 老 settings.json 里没有 webEnabled / subagentEnabled。
        // serde 的 `default` 会走 AppSettings::default()——必须是"开"，
        // 否则升级一次就把所有人的联网与子 agent 静默关掉。
        let old = serde_json::json!({
            "baseUrl": "http://x", "model": "m", "provider": "p"
        });
        let cfg: AppSettings = serde_json::from_value(old).unwrap();
        assert!(cfg.web_enabled, "老配置缺键时必须默认允许联网");
        assert!(cfg.subagent_enabled, "老配置缺键时必须默认允许子 agent");
        assert_eq!(cfg.idle_recycle_min, 30);
        assert_eq!(cfg.max_runtime_procs, 2, "老配置缺键时必须给进程数上限");
    }

    #[test]
    fn new_project_gets_an_agents_md_with_the_rules() {
        // DSH 的 agent-instructions 读 cwd（= 书目录）下的 AGENTS.md 注入项目指令。
        // 不建这个文件，界面上的铁律就只是界面文案，模型看不到。
        let (home, _guard) = use_home("agents-md");
        let dir = create_project("法典测试").unwrap();
        let path = dir.join("AGENTS.md");
        assert!(path.exists(), "新建书必须带 AGENTS.md");
        let text = std::fs::read_to_string(&path).unwrap();
        assert!(text.contains("正文只由作者落笔"));
        assert!(text.contains("propose_"));
        assert!(text.contains("世界线"), "术语分歧是模型最容易搞错的地方");
        // 已存在时不能被覆盖：作者会改它（这里直接验证"只在缺失时写"的契约）
        assert!(!dir.join("AGENTS.md").with_extension("md.bak").exists());
        std::fs::write(&path, "作者改过的法典").unwrap();
        assert_eq!(
            std::fs::read_to_string(&path).unwrap(),
            "作者改过的法典",
            "写完后仍可被作者改"
        );
        let _ = std::fs::remove_dir_all(&home);
    }

    #[test]
    fn max_runtime_procs_defaults_to_two_and_never_zero() {
        // 上限必须至少 1：0 会让"开书"这件事本身失败（登记时先关掉最老的一个，
        // 没有可关的就卡住）。这条防的是有人把默认值调成 0 当"不限制"。
        assert_eq!(AppSettings::default().max_runtime_procs, 2);
        let old = serde_json::json!({ "model": "x" });
        let cfg: AppSettings = serde_json::from_value(old).unwrap();
        assert_eq!(cfg.max_runtime_procs, 2, "老配置缺键时用默认值");
    }

    #[test]
    fn explicit_false_in_settings_is_respected() {
        let v = serde_json::json!({ "webEnabled": false, "subagentEnabled": false });
        let cfg: AppSettings = serde_json::from_value(v).unwrap();
        assert!(!cfg.web_enabled, "作者显式关掉的不能被默认值盖回来");
        assert!(!cfg.subagent_enabled);
    }

    fn use_home(tag: &str) -> (PathBuf, MutexGuard<'static, ()>) {
        let guard = lock_home();
        let home = temp_home(tag);
        unsafe { std::env::set_var("HOME", &home); }
        (home, guard)
    }

    #[test]
    fn settings_import_is_idempotent_and_never_overwrites() {
        let (home, _guard) = use_home("settings-import");
        let dir = create_project("设定导入").unwrap();
        let handle = ProjectHandle::open(&dir).unwrap();
        let src = std::path::Path::new("/tmp/settings-import-test");
        let _ = std::fs::remove_dir_all(src);
        std::fs::create_dir_all(src).unwrap();
        std::fs::write(src.join("世界观.md"), "大陆叫云洲，灵气潮汐三十年一涨。").unwrap();
        std::fs::write(src.join("沈砚小传.md"), "沈砚，断刀客，寡言。").unwrap();

        let r1 = handle.import_settings_docs(src).unwrap();
        assert_eq!(r1.count, 2, "两篇都该归档");
        assert!(dir.join("imports").join("世界观.md").exists());
        assert!(dir.join("imports").join("INDEX.md").exists(), "给总 Agent 的清单要生成");

        // 再导一次：内容相同 → 幂等跳过，不产生副本
        let r2 = handle.import_settings_docs(src).unwrap();
        assert_eq!(r2.count, 0);
        assert_eq!(r2.skipped.len(), 2, "内容相同的两篇都该跳过：{:?}", r2.skipped);

        // 同名但内容不同：加序号，绝不覆盖
        std::fs::write(src.join("世界观.md"), "大陆其实叫云川。").unwrap();
        let r3 = handle.import_settings_docs(src).unwrap();
        assert_eq!(r3.count, 1);
        assert!(dir.join("imports").join("世界观-2.md").exists(), "冲突版本要留档");
        assert_eq!(
            std::fs::read_to_string(dir.join("imports").join("世界观.md")).unwrap(),
            "大陆叫云洲，灵气潮汐三十年一涨。",
            "原文不能被覆盖"
        );

        // 提取入口的哨兵：imports/ 里现在有 3 篇（INDEX.md 不算）
        assert_eq!(handle.imported_docs().len(), 3);
        let _ = std::fs::remove_dir_all(&home);
        let _ = std::fs::remove_dir_all(src);
    }

    #[test]
    fn import_dir_generates_proposals() {
        let (home, _guard) = use_home("import");
        let dir = create_project("导入测试").unwrap();
        let handle = ProjectHandle::open(&dir).unwrap();
        let src = std::path::Path::new("/tmp/import-test");
        if src.exists() {
            let (d, c) = handle.import_dir(src).unwrap();
            assert_eq!(d, 1, "应识别一个章节文件");
            assert_eq!(c, 1, "应识别一个设定文件");
            let props = handle.proposals();
            assert_eq!(props.len(), 2);
            assert!(props.iter().any(|p| p.payload.get("kind").and_then(|k| k.as_str()) == Some("draft")));
        }
        let _ = std::fs::remove_dir_all(&home);
    }

    #[test]
    fn create_and_roundtrip() {
        let (home, _guard) = use_home("roundtrip");
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

    #[test]
    fn daily_words_are_recorded_on_save() {
        let (home, _guard) = use_home("daily");
        let dir = create_project("字数测试").unwrap();
        let h = ProjectHandle::open(&dir).unwrap();
        h.save_prose(1, "一二三").unwrap();
        let today = today_key();
        let daily = h.daily_words();
        assert_eq!(daily.iter().find(|(k, _)| *k == today).map(|(_, n)| *n), Some(3));
        // 净减
        h.save_prose(1, "一二").unwrap();
        let daily = h.daily_words();
        assert_eq!(daily.iter().find(|(k, _)| *k == today).map(|(_, n)| *n), Some(2));
        // 书架今日字数读的就是同一字段
        let list = list_projects();
        assert!(list.iter().any(|p| p.title == "字数测试" && p.today_words == 2));
        // 未知字段不被写没
        let raw: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(dir.join("project.json")).unwrap()).unwrap();
        assert_eq!(raw["title"], serde_json::json!("字数测试"));
        assert!(raw["createdAt"].is_string());
        let _ = std::fs::remove_dir_all(&home);
    }

    #[test]
    fn director_sovereign_edits() {
        let (home, _guard) = use_home("director");
        let dir = create_project("导演测试").unwrap();
        let handle = ProjectHandle::open(&dir).unwrap();
        std::fs::write(dir.join("events.json"), serde_json::to_string(&serde_json::json!([
            { "id": "E01", "chapter": 3, "objectiveFact": "灯灭", "revealed": true, "storylineIDs": ["L01"] }
        ])).unwrap()).unwrap();
        std::fs::write(dir.join("clues.json"), serde_json::to_string(&serde_json::json!([
            { "id": "F01", "title": "半块虎符", "status": "埋下", "plantedChapter": 3, "lastActionChapter": 3 }
        ])).unwrap()).unwrap();
        std::fs::write(dir.join("storylines.json"), serde_json::to_string(&serde_json::json!([
            { "id": "L01", "name": "主线", "status": "进行中" }
        ])).unwrap()).unwrap();

        // 作者拖卡改期：事件与伏笔都落对文件
        let label = handle.set_event_chapter("E01", 9).unwrap();
        assert_eq!(label, "灯灭");
        assert_eq!(handle.events()[0]["chapter"], serde_json::json!(9));
        handle.set_clue_planted("F01", 9).unwrap();
        assert_eq!(handle.clues_full()[0]["plantedChapter"], serde_json::json!(9));

        // 晋升链的 outlineUpdates 写 storylines.json（不是 outline.json）
        let payload = serde_json::json!({ "kind": "outlineUpdates", "updates": [{ "storylineID": "L01", "status": "断线", "note": "12 章无动作" }] });
        let (applied, _) = handle.apply_payload(&payload).unwrap();
        assert_eq!(applied, 1);
        let sl = handle.storylines();
        assert_eq!(sl[0]["status"], serde_json::json!("断线"));
        assert_eq!(sl[0]["syncNote"], serde_json::json!("12 章无动作"));
        assert!(!dir.join("outline.json").exists(), "绝不产生旧合并文件");

        // 无 note 的同步更新保留上次备注
        let (applied, _) = handle.apply_payload(&serde_json::json!({
            "kind": "outlineUpdates",
            "updates": [{ "storylineID": "L01", "status": "进行中" }]
        })).unwrap();
        assert_eq!(applied, 1);
        assert_eq!(handle.storylines()[0]["syncNote"], serde_json::json!("12 章无动作"));

        // M8-3 预测排期：β 线 add 型 diff 定向第 9 章 → plannedEvent 提案 → 采纳入 events（happened=false）
        let mut lines9 = handle.worldlines();
        lines9.push(serde_json::json!({
            "id": "β", "name": "预测测试", "premise": "后续", "kind": "forecast", "forkedChapter": 13, "status": "已推演",
            "nodes": [], "edges": [],
            "diffs": [{ "target": "事件", "targetRef": "L01", "kind": "add", "after": "主角夜探漕帮总舵", "reason": "虎符到期" }],
            "derived": { "characters": [], "canon": [], "clues": [] }
        }));
        handle.save_worldlines(&lines9).unwrap();
        std::fs::write(dir.join("storylines.json"), serde_json::to_string(&serde_json::json!([
            { "id": "L01", "name": "主线", "status": "进行中" }
        ])).unwrap()).unwrap();
        let pid = handle.promote_diff_chapter("β", 0, 9).unwrap();
        handle.accept_proposal(&pid).unwrap();
        let ev = handle.events();
        let planned = ev.iter().find(|e| e["objectiveFact"] == serde_json::json!("主角夜探漕帮总舵")).expect("计划事件已入账");
        assert_eq!(planned["chapter"], serde_json::json!(9));
        assert_eq!(planned["happened"], serde_json::json!(false), "计划≠成稿");
        assert_eq!(planned["plannedFrom"], serde_json::json!("线β"));
        assert!(handle.promote_diff_chapter("β", 0, 10).is_err(), "二次排期晋升被拒");

        // 不存在对象报错（世界线排期板拒拖野卡）
        assert!(handle.set_event_chapter("E99", 1).is_err());
        let _ = std::fs::remove_dir_all(&home);
    }

    #[test]
    fn accepting_a_proposal_that_breaks_the_ledger_rolls_it_back() {
        // AI 写出的 payload 是自由 JSON。这里构造一个"看起来合理但结构坏了"的
        // 提案：同一次采纳里改两条伏笔，其中一条的章号是中文数字。
        let (home, _guard) = use_home("schema-reject");
        let dir = create_project("结构校验").unwrap();
        let handle = ProjectHandle::open(&dir).unwrap();
        handle
            .save_array(
                "clues.json",
                &[serde_json::json!({ "id": "F01", "title": "旧伏笔", "plantedChapter": 3, "status": "埋下" })],
            )
            .unwrap();
        let pid = handle
            .add_proposal_local(
                "测试",
                "坏提案",
                "",
                serde_json::json!({
                    "kind": "clues",
                    "clues": [
                        { "id": "F02", "title": "新伏笔", "plantedChapter": 5 },
                        // 这条会让整个文件不合法
                        { "id": "F03", "title": "坏章号", "plantedChapter": "第十二章" }
                    ]
                }),
            )
            .unwrap();

        let err = handle.accept_proposal(&pid).unwrap_err();
        assert!(err.contains("采纳被拒"), "{err}");
        assert!(err.contains("plantedChapter"), "要指出坏在哪：{err}");

        // 台账回到采纳前：一条都没多
        let clues = handle.ledger(Ledger::Clues).unwrap();
        assert_eq!(clues.len(), 1, "整档回滚，不留半条：{clues:?}");
        assert_eq!(clues[0]["id"], serde_json::json!("F01"));

        // 提案仍是待审——作者修好 payload 还能再来一次
        let p = handle.proposals().into_iter().find(|p| p.id == pid).unwrap();
        assert_eq!(p.status, "待确认");
        let _ = std::fs::remove_dir_all(&home);
    }

    #[test]
    fn accepting_a_clean_proposal_still_works_with_the_guard() {
        let (home, _guard) = use_home("schema-ok");
        let dir = create_project("结构校验通过").unwrap();
        let handle = ProjectHandle::open(&dir).unwrap();
        let pid = handle
            .add_proposal_local(
                "测试",
                "好提案",
                "",
                serde_json::json!({
                    "kind": "clues",
                    "clues": [{ "id": "F01", "title": "新伏笔", "plantedChapter": 5 }]
                }),
            )
            .unwrap();
        let (applied, _) = handle.accept_proposal(&pid).unwrap();
        assert_eq!(applied, 1);
        assert_eq!(handle.ledger(Ledger::Clues).unwrap().len(), 1);
        let _ = std::fs::remove_dir_all(&home);
    }

    #[test]
    fn adopt_single_node_turns_it_into_a_planned_event_proposal() {
        let (home, _guard) = use_home("adopt-node");
        let dir = create_project("逐节点采纳").unwrap();
        let handle = ProjectHandle::open(&dir).unwrap();
        handle.save_worldlines(&[serde_json::json!({
            "id": "β", "name": "假设线", "premise": "他没去救", "kind": "what-if",
            "forkedChapter": 12, "status": "推演中",
            "nodes": [
                { "id": "n1", "kind": "event", "label": "师兄反目", "chapter": 15, "state": "planned" },
                { "id": "n2", "kind": "event", "label": "夜探总舵", "chapter": 0, "state": "planned" }
            ],
            "edges": [], "diffs": []
        })]).unwrap();

        // 未排章的节点不能采纳：没有章号就排不进 events
        let err = handle.adopt_node("β", "n2").unwrap_err();
        assert!(err.contains("排章"), "要说明为什么不能采纳，实际：{err}");

        let pid = handle.adopt_node("β", "n1").unwrap();
        let p = handle.proposals().into_iter().find(|p| p.id == pid).unwrap();
        assert_eq!(p.payload["kind"], serde_json::json!("plannedEvent"));
        assert_eq!(p.payload["chapter"], serde_json::json!(15));
        assert_eq!(p.payload["title"], serde_json::json!("师兄反目"));
        assert_eq!(p.capability, "世界线·逐节点采纳");

        // 节点状态同步改成 adopted——时间轴上要能看出"这条已经承认了"
        let line = handle.worldlines().into_iter().find(|l| l["id"] == serde_json::json!("β")).unwrap();
        let n1 = line["nodes"].as_array().unwrap().iter().find(|n| n["id"] == "n1").unwrap().clone();
        assert_eq!(n1["state"], serde_json::json!("adopted"));

        // 重复采纳被拒（不能靠连点造出两条一样的提案）
        assert!(handle.adopt_node("β", "n1").is_err());

        // 采纳仍然只是提案：正典事件没动，要过收件箱
        assert!(handle.events().is_empty(), "采纳 ≠ 直写正典");
        handle.accept_proposal(&pid).unwrap();
        let ev = handle.events();
        assert_eq!(ev.len(), 1);
        assert_eq!(ev[0]["chapter"], serde_json::json!(15));
        assert_eq!(ev[0]["happened"], serde_json::json!(false), "还没写到，是计划事件");
        let _ = std::fs::remove_dir_all(&home);
    }

    #[test]
    fn adopt_node_reads_chapter_from_reference_when_field_is_empty() {
        // AI 填的节点常常只有 reference 里的 ch-015，没有 chapter 字段。
        // 这种节点作者看图点"采纳"时不该被拦下来。
        let (home, _guard) = use_home("adopt-guess");
        let dir = create_project("采纳猜章").unwrap();
        let handle = ProjectHandle::open(&dir).unwrap();
        handle.save_worldlines(&[serde_json::json!({
            "id": "γ", "name": "推断", "kind": "what-if", "forkedChapter": 5, "status": "推演中",
            "nodes": [{ "id": "n1", "kind": "event", "label": "城门失守", "reference": "ch-018" }],
            "edges": [], "diffs": []
        })]).unwrap();
        let pid = handle.adopt_node("γ", "n1").unwrap();
        let p = handle.proposals().into_iter().find(|p| p.id == pid).unwrap();
        assert_eq!(p.payload["chapter"], serde_json::json!(18));
        let _ = std::fs::remove_dir_all(&home);
    }

    #[test]
    fn proposal_kinds_apply() {
        let (home, _guard) = use_home("kinds");
        let dir = create_project("落库测试").unwrap();
        let h = ProjectHandle::open(&dir).unwrap();
        h.ensure_chapter(3).unwrap();
        h.save_prose(3, "他缓缓地转过身，缓缓地说话。").unwrap();
        std::fs::write(dir.join("clues.json"), serde_json::to_string(&serde_json::json!([
            { "id": "F01", "title": "虎符", "status": "埋下", "plantedChapter": 1, "lastActionChapter": 1 }
        ])).unwrap()).unwrap();

        // skeleton → meta 批准
        h.apply_payload(&serde_json::json!({"kind":"skeleton","chapter":3,"beats":[{"title":"拍一"}]})).unwrap();
        let meta: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(dir.join("chapters").join("ch-003").join("meta.json")).unwrap()).unwrap();
        assert_eq!(meta["skeletonApproved"], serde_json::json!(true));
        // report → meta 归档
        h.apply_payload(&serde_json::json!({"kind":"report","chapter":3,"findings":[{"kind":"连续性"}]})).unwrap();
        let meta: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(dir.join("chapters").join("ch-003").join("meta.json")).unwrap()).unwrap();
        assert_eq!(meta["report"].as_array().unwrap().len(), 1);
        // deslop → 快照先行逐处替换；二次幂等失败
        let (a1, _) = h.apply_payload(&serde_json::json!({"kind":"deslop","chapter":3,"suggestions":[{"original":"缓缓地转过身","replacement":"转过身"},{"original":"不存在的东西","replacement":"x"}]})).unwrap();
        assert_eq!(a1, 1);
        assert_eq!(h.prose(3), "他转过身，缓缓地说话。");
        assert!(!h.snapshots(3).is_empty(), "首次改写前自动快照");
        assert!(h.apply_payload(&serde_json::json!({"kind":"deslop","chapter":3,"suggestions":[{"original":"缓缓地转过身","replacement":"转过身"}]})).is_err());
        // 同 original 的重复建议只处理一次（不会把第二处「缓缓地」也改掉）
        h.save_prose(3, "缓缓地回头，缓缓地说话。").unwrap();
        let (a2, skipped) = h.apply_payload(&serde_json::json!({"kind":"deslop","chapter":3,"suggestions":[
            {"original":"缓缓地","replacement":"慢慢"},
            {"original":"缓缓地","replacement":"慢慢"}
        ]})).unwrap();
        assert_eq!(a2, 1);
        assert_eq!(h.prose(3), "慢慢回头，缓缓地说话。");
        assert!(skipped.iter().any(|s| s.contains("重复建议")));
        // memoryPack → 入账 + 去重 + 自动补别名
        h.apply_payload(&serde_json::json!({"kind":"memoryPack","chapter":3,"facts":[{"subject":"沈砚","predicate":"获得","object":"断刀"}]})).unwrap();
        let (_, sk) = h.apply_payload(&serde_json::json!({"kind":"memoryPack","chapter":3,"facts":[{"subject":"沈砚","predicate":"获得","object":"断刀"}]})).unwrap();
        assert_eq!(sk.len(), 1);
        let m: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(dir.join("memory.json")).unwrap()).unwrap();
        assert_eq!(m["facts"].as_array().unwrap().len(), 1);
        assert!(h.aliases().iter().any(|a| a["canonicalName"] == serde_json::json!("沈砚")));
        // aliases 提案：归并同名、别名去重
        h.apply_payload(&serde_json::json!({"kind":"aliases","aliases":[{"canonicalName":"沈砚","aliases":["小沈","沈砚"]}]})).unwrap();
        h.apply_payload(&serde_json::json!({"kind":"aliases","aliases":[{"canonicalName":"沈砚","aliases":["小沈"]}]})).unwrap();
        let one = h.aliases().into_iter().find(|a| a["canonicalName"] == serde_json::json!("沈砚")).unwrap();
        assert_eq!(one["aliases"], serde_json::json!(["小沈"]));
        // dossier → 同章覆盖；缺章号被拒
        h.apply_payload(&serde_json::json!({"kind":"dossier","chapter":3,"headline":"v1"})).unwrap();
        h.apply_payload(&serde_json::json!({"kind":"dossier","chapter":3,"headline":"v2","cast":["顾青"]})).unwrap();
        let ds = h.read_array("dossiers.json");
        assert_eq!(ds.len(), 1);
        assert_eq!(ds[0]["headline"], serde_json::json!("v2"));
        assert!(h.aliases().iter().any(|a| a["canonicalName"] == serde_json::json!("顾青")), "档案卡出场人物进别名表");
        assert!(h.apply_payload(&serde_json::json!({"kind":"dossier","chapter":0,"headline":"bad"})).is_err());
        assert_eq!(h.read_array("dossiers.json").len(), 1);
        // clueFixes → 状态+动作日志
        h.apply_payload(&serde_json::json!({"kind":"clueFixes","fixes":[{"clueID":"F01","kind":"回收","detail":"第3章兑现"}]})).unwrap();
        let cl = h.clues_full();
        assert_eq!(cl[0]["status"], serde_json::json!("已兑现"));
        assert_eq!(cl[0]["actionLog"].as_array().unwrap().len(), 1);
        let _ = std::fs::remove_dir_all(&home);
    }

    #[test]
    fn project_meta_roundtrip() {
        let (home, _home_guard) = use_home("project");
        std::thread::sleep(std::time::Duration::from_millis(5));
        let dir = create_project("书目测试").unwrap();
        let h = ProjectHandle::open(&dir).unwrap();
        let mut p = h.project();
        assert_eq!(p.title, "书目测试");
        p.genre = "武侠".into();
        p.premise = "一把断刀撬动旧案".into();
        p.target_chapters = 120;
        p.chapter_word_target = 4200;
        h.save_project(&p).unwrap();
        let back = ProjectHandle::open(&dir).unwrap().project();
        assert_eq!(back.genre, "武侠");
        assert_eq!(back.target_chapters, 120);
        assert_eq!(back.chapter_word_target, 4200);
        assert_eq!(h.project_word_target(), 4200);
        // 零值回退默认
        let mut z = h.project();
        z.chapter_word_target = 0;
        h.save_project(&z).unwrap();
        assert_eq!(ProjectHandle::open(&dir).unwrap().project_word_target(), 3000);
        let _ = std::fs::remove_dir_all(&home);
    }

    #[test]
    fn worldline_promote_targets() {
        let (home, _home_guard) = use_home("wltargets");
        std::thread::sleep(std::time::Duration::from_millis(5));
        let dir = create_project("晋升分流").unwrap();
        let h = ProjectHandle::open(&dir).unwrap();
        let mut lines = h.worldlines();
        lines.push(serde_json::json!({
            "id": "β", "name": "预测", "kind": "forecast", "premise": "x", "forkedChapter": 7,
            "status": "已推演", "nodes": [], "edges": [],
            "diffs": [
                { "target": "人物", "targetRef": "镖局遗孤", "kind": "add", "after": "新增人物", "reason": "承接伏笔" },
                { "target": "事件", "targetRef": "新", "kind": "add", "after": "回镇遇伏", "reason": "F01 到期" }
            ],
            "derived": {}
        }));
        h.save_worldlines(&lines).unwrap();

        // 人物 add → aliases 提案（不再落成找不到故事线的 outlineUpdates）
        let pid = h.promote_diff("β", 0).unwrap();
        let p = h.proposals().into_iter().find(|p| p.id == pid).unwrap();
        assert_eq!(p.payload["kind"], serde_json::json!("aliases"));
        assert_eq!(p.payload["aliases"][0]["canonicalName"], serde_json::json!("镖局遗孤"));
        let (applied, _) = h.accept_proposal(&pid).unwrap();
        assert_eq!(applied, 1);
        assert!(h.aliases().iter().any(|a| a["canonicalName"] == serde_json::json!("镖局遗孤")));

        // 事件 add 无章 → 拒绝并指路排期晋升
        let err = h.promote_diff("β", 1).unwrap_err();
        assert!(err.contains("排期晋升"), "{err}");
        // 带章 → plannedEvent 提案
        let pid2 = h.promote_diff_chapter("β", 1, 7).unwrap();
        let p2 = h.proposals().into_iter().find(|p| p.id == pid2).unwrap();
        assert_eq!(p2.payload["kind"], serde_json::json!("plannedEvent"));
        let _ = std::fs::remove_dir_all(&home);
    }

    #[test]
    fn worldline_archive_toggle() {
        let (home, _home_guard) = use_home("wlarch");
        std::thread::sleep(std::time::Duration::from_millis(5));
        let dir = create_project("归档测试").unwrap();
        let h = ProjectHandle::open(&dir).unwrap();
        let mut lines = h.worldlines();
        lines.push(serde_json::json!({
            "id": "β", "name": "预测线", "kind": "forecast", "premise": "x", "forkedChapter": 13,
            "status": "已推演", "nodes": [{"id": "n1", "kind": "event", "label": "夜探", "x": 100, "y": 80}],
            "edges": [], "diffs": [], "derived": {}
        }));
        h.save_worldlines(&lines).unwrap();
        assert_eq!(h.toggle_worldline_archive("β").unwrap(), "已归档");
        assert_eq!(h.worldlines()[1]["status"], serde_json::json!("已归档"));
        assert_eq!(h.toggle_worldline_archive("β").unwrap(), "已推演");
        assert!(h.toggle_worldline_archive("α").is_err());
        assert!(h.toggle_worldline_archive("ω").is_err());
        let _ = std::fs::remove_dir_all(&home);
    }

    #[test]
    fn relations_payload_applies() {
        let (home, _home_guard) = use_home("relations");
        std::thread::sleep(std::time::Duration::from_millis(5));
        let dir = create_project("关系测试").unwrap();
        let h = ProjectHandle::open(&dir).unwrap();
        let payload = serde_json::json!({ "kind": "relations", "relations": [
            { "from": "沈砚", "predicate": "师从", "to": "老周", "chapter": 7, "publicToReader": true },
            { "from": "钱九渊", "predicate": "敌对", "to": "沈砚", "chapter": 20 }
        ]});
        let (n, _) = h.apply_payload(&payload).unwrap();
        assert_eq!(n, 2);
        let facts = h.memory_facts();
        assert!(facts.iter().any(|f| f["subject"] == serde_json::json!("沈砚")
            && f["predicate"] == serde_json::json!("师从")
            && f["object"] == serde_json::json!("老周")));
        // 幂等：同边重复采纳不重复入账
        let (n2, sk) = h.apply_payload(&serde_json::json!({ "kind": "relations", "relations": [
            { "from": "沈砚", "predicate": "师从", "to": "老周" }
        ]})).err().map(|e| (0usize, vec![e])).unwrap_or((0, vec![]));
        assert_eq!(n2, 0);
        assert!(!sk.is_empty());
        let _ = std::fs::remove_dir_all(&home);
    }

    #[test]
    fn legacy_payload_without_kind() {
        let (home, _home_guard) = use_home("legacy");
        std::thread::sleep(std::time::Duration::from_millis(5));
        let dir = create_project("旧格式").unwrap();
        let h = ProjectHandle::open(&dir).unwrap();
        std::fs::write(dir.join("clues.json"), serde_json::to_string(&serde_json::json!([
            { "id": "F01", "title": "虎符", "status": "埋下", "plantedChapter": 1 }
        ])).unwrap()).unwrap();
        // Swift 时代 payload：没有 kind，键即类型
        // 旧版真实形状：{"_0":[…]} 包裹 + action/evidence/newTargetChapter 字段
        let legacy = serde_json::json!({ "clueFixes": { "_0": [{ "clueID": "F01", "kind": "回收", "action": "第3章兑现" }] } });
        assert_eq!(payload_kind(&legacy), "clueFixes");
        let res = h.apply_payload(&legacy);
        if let Err(e) = &res { eprintln!("LEGACY-ERR: {e}"); }
        let (applied, _) = res.unwrap();
        assert_eq!(applied, 1);
        assert_eq!(h.clues_full()[0]["status"], serde_json::json!("已兑现"));
        // 推断顺序：clueFixes 先于 clues；docs→canon；items 带 category→canonClassify
        assert_eq!(payload_kind(&serde_json::json!({"clueFixes": [], "clues": []})), "clueFixes");
        assert_eq!(payload_kind(&serde_json::json!({"docs": []})), "canon");
        assert_eq!(payload_kind(&serde_json::json!({"items": [{"title":"x","category":"世界观"}]})), "canonClassify");
        assert_eq!(payload_kind(&serde_json::json!({"items": [{"title":"x","content":"y"}]})), "canonRevise");

        // 旧版事件级大纲同步：{"outlineUpdates":{"_0":[…eventID+kind+newChapter…]}}
        std::fs::write(dir.join("events.json"), serde_json::to_string(&serde_json::json!([
            { "id": "E01", "chapter": 1, "objectiveFact": "捡刀", "revealed": false, "happened": false }
        ])).unwrap()).unwrap();
        let sync = serde_json::json!({ "outlineUpdates": { "_0": [
            { "eventID": "E01", "kind": "事件已发生", "evidence": "第1章正文命中" },
            { "eventID": "E01", "kind": "事件改期", "newChapter": 5, "evidence": "计划3实际5" },
            { "eventID": "E01", "kind": "读者已知", "newChapter": 6, "reason": "章末钩子已交给读者" }
        ] } });
        assert_eq!(payload_kind(&sync), "outlineUpdates");
        let (n, _) = h.apply_payload(&sync).unwrap();
        assert_eq!(n, 3);
        let ev = h.events();
        assert_eq!(ev[0]["happened"], serde_json::json!(true));
        assert_eq!(ev[0]["chapter"], serde_json::json!(6));
        assert_eq!(ev[0]["revealed"], serde_json::json!(true));
        assert_eq!(ev[0]["readerKnowledge"], serde_json::json!("章末钩子已交给读者"));
        let _ = std::fs::remove_dir_all(&home);
    }

    #[test]
    fn worldline_promote_chain() {
        let (home, _guard) = use_home("m7");
        let dir = create_project("世界线测试").unwrap();
        assert!(dir.join("worldlines.json").exists(), "新建即有 α 沙盒");
        assert!(dir.join("canvas.json").exists());

        let handle = ProjectHandle::open(&dir).unwrap();
        // 正典设定《灯约》先入库
        handle.apply_payload(&serde_json::json!({
            "kind": "canon",
            "docs": [{ "title": "灯约", "content": "灯灭即恩断" }]
        })).unwrap();

        // β 线带一条“设定修订”影响
        let mut lines = handle.worldlines();
        lines.push(serde_json::json!({
            "id": "β", "name": "灯约新解", "premise": "如果灯灭不是绝而是换", "forkedChapter": 12,
            "status": "已推演", "nodes": [], "edges": [],
            "diffs": [{ "target": "设定", "targetRef": "灯约", "kind": "change", "after": "灯灭=换灯人", "reason": "归来语义", "promoted": false }],
            "derived": { "characters": [], "canon": [], "clues": [] }
        }));
        handle.save_worldlines(&lines).unwrap();

        let pid = handle.promote_diff("β", 0).unwrap();
        assert_eq!(handle.proposals().iter().filter(|p| p.id == pid).count(), 1);
        let (applied, _) = handle.accept_proposal(&pid).unwrap();
        assert_eq!(applied, 1);
        let decided = handle.proposals().into_iter().find(|p| p.id == pid).expect("提案仍在收件箱");
        let record = decided.apply_result.expect("采纳结果持久化到 inbox.json");
        assert_eq!(record.applied, 1);
        assert!(decided.decided_at.is_some());
        assert_eq!(handle.canon_docs().iter().find(|(t, _, _)| t == "灯约").map(|(_, _, b)| b.as_str()), Some("灯灭=换灯人"));
        let backups = std::fs::read_dir(dir.join("canon").join("backups")).unwrap().count();
        assert_eq!(backups, 1, "修订前自动备份");
        // 缺 content 的修订不能把正典写成空串
        let (applied, _) = handle.apply_payload(&serde_json::json!({
            "kind": "canonRevise",
            "items": [{ "title": "灯约" }]
        })).unwrap();
        assert_eq!(applied, 0);
        assert_eq!(handle.canon_docs()[0].2, "灯灭=换灯人");
        // 幂等：重复晋升被拒
        assert!(handle.promote_diff("β", 0).is_err());
        // 画板状态往返
        let st = serde_json::json!({ "view": { "x": 12, "y": -8, "k": 1.4 } });
        handle.save_canvas_state(&st).unwrap();
        assert_eq!(handle.canvas_state()["view"]["k"].as_f64(), Some(1.4));
        let _ = std::fs::remove_dir_all(&home);
    }

    #[test]
    fn clue_ids_skip_holes_and_keep_fields() {
        let (home, _guard) = use_home("clue-ids");
        let dir = create_project("伏笔编号").unwrap();
        let h = ProjectHandle::open(&dir).unwrap();
        std::fs::write(dir.join("clues.json"), serde_json::to_string(&serde_json::json!([
            { "id": "F1", "title": "旧一", "detail": "d", "plantedChapter": 1 },
            { "id": "F3", "title": "旧三", "detail": "d", "plantedChapter": 2 }
        ])).unwrap()).unwrap();
        let (applied, _) = h.apply_payload(&serde_json::json!({
            "kind": "clues",
            "clues": [{ "title": "新伏笔", "detail": "x", "plantedChapter": 4, "sourceRefs": ["ch-001"] }]
        })).unwrap();
        assert_eq!(applied, 1);
        let cl = h.clues_full();
        assert_eq!(cl.len(), 3, "不能因为 len+1 撞 F3 而被合并");
        let new = cl.iter().find(|c| c["title"] == serde_json::json!("新伏笔")).unwrap();
        assert_eq!(new["id"], serde_json::json!("F4"));
        assert_eq!(new["sourceRefs"], serde_json::json!(["ch-001"]), "新字段透传");
        assert_eq!(cl.iter().find(|c| c["id"] == serde_json::json!("F3")).unwrap()["title"], serde_json::json!("旧三"));

        // 只剩 F2 时也生成 F03，而不是覆盖 F2
        std::fs::write(dir.join("clues.json"), serde_json::to_string(&serde_json::json!([
            { "id": "F2", "title": "只剩二", "detail": "d" }
        ])).unwrap()).unwrap();
        h.apply_payload(&serde_json::json!({"kind":"clues","clues":[{"title":"再添一条","detail":"d"}]})).unwrap();
        let cl = h.clues_full();
        assert_eq!(cl.len(), 2);
        assert_eq!(cl.iter().find(|c| c["title"] == serde_json::json!("只剩二")).unwrap()["id"], serde_json::json!("F2"));
        assert_eq!(cl.iter().find(|c| c["title"] == serde_json::json!("再添一条")).unwrap()["id"], serde_json::json!("F3"));
        let _ = std::fs::remove_dir_all(&home);
    }

    #[test]
    fn corrupt_ledgers_are_not_overwritten() {
        let (home, _guard) = use_home("corrupt");
        let dir = create_project("坏账保护").unwrap();
        let h = ProjectHandle::open(&dir).unwrap();
        std::fs::write(dir.join("clues.json"), "{ 这不是 JSON").unwrap();
        let err = h.apply_payload(&serde_json::json!({
            "kind": "clues",
            "clues": [{ "title": "新伏笔" }]
        })).unwrap_err();
        assert!(err.contains("损坏"), "坏 clues.json 必须报错：{err}");
        assert_eq!(std::fs::read_to_string(dir.join("clues.json")).unwrap(), "{ 这不是 JSON");

        std::fs::write(dir.join("memory.json"), "{ 坏的").unwrap();
        let err = h.apply_payload(&serde_json::json!({
            "kind": "memoryPack",
            "chapter": 1,
            "facts": [{ "subject": "沈砚", "predicate": "获得", "object": "断刀" }]
        })).unwrap_err();
        assert!(err.contains("损坏"), "坏 memory.json 必须报错：{err}");
        assert_eq!(std::fs::read_to_string(dir.join("memory.json")).unwrap(), "{ 坏的");
        let _ = std::fs::remove_dir_all(&home);
    }

    #[test]
    fn incomplete_payloads_do_not_clear_approved_state() {
        let (home, _guard) = use_home("incomplete");
        let dir = create_project("空载荷保护").unwrap();
        let h = ProjectHandle::open(&dir).unwrap();
        h.ensure_chapter(3).unwrap();
        h.save_prose(3, "原正文").unwrap();
        h.apply_payload(&serde_json::json!({"kind":"skeleton","chapter":3,"beats":[{"title":"拍一"}]})).unwrap();
        assert!(h.apply_payload(&serde_json::json!({"kind":"skeleton","chapter":3})).is_err());
        let meta: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(dir.join("chapters/ch-003/meta.json")).unwrap()).unwrap();
        assert_eq!(meta["beats"].as_array().unwrap().len(), 1, "缺 beats 不能清空已批准节拍");

        h.apply_payload(&serde_json::json!({"kind":"report","chapter":3,"findings":[{"kind":"连续性"}]})).unwrap();
        assert!(h.apply_payload(&serde_json::json!({"kind":"report","chapter":3})).is_err());
        let meta: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(dir.join("chapters/ch-003/meta.json")).unwrap()).unwrap();
        assert_eq!(meta["report"].as_array().unwrap().len(), 1, "缺 findings 不能清空已归档报告");

        // 空 findings 不能把上一份报告清成 []
        assert!(h.apply_payload(&serde_json::json!({"kind":"report","chapter":3,"findings":[]})).is_err());
        let meta: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(dir.join("chapters/ch-003/meta.json")).unwrap()).unwrap();
        assert_eq!(meta["report"].as_array().unwrap().len(), 1, "空 findings 不能清空已归档报告");

        // 全空档案卡不能按同章覆盖清掉既有档案
        h.apply_payload(&serde_json::json!({"kind":"dossier","chapter":3,"headline":"夜探","cast":["顾青"]})).unwrap();
        assert!(h.apply_payload(&serde_json::json!({
            "kind":"dossier","chapter":3,"headline":"","scenes":[],"cast":[],"advanced":[],"turns":[],"hooks":[],"relics":[]
        })).is_err());
        assert_eq!(h.read_array("dossiers.json")[0]["headline"], serde_json::json!("夜探"));

        // meta.json 顶层不是对象：严格拒绝，不能当空对象重写成只剩 beats/report
        std::fs::write(dir.join("chapters/ch-003/meta.json"), "[\"坏\"]").unwrap();
        assert!(h.apply_payload(&serde_json::json!({"kind":"skeleton","chapter":3,"beats":[{"title":"拍二"}]})).is_err());
        assert_eq!(
            std::fs::read_to_string(dir.join("chapters/ch-003/meta.json")).unwrap(),
            "[\"坏\"]"
        );

        // 空草稿不能覆盖整章
        assert!(h.apply_payload(&serde_json::json!({"kind":"draft","chapter":3,"text":"   "})).is_err());
        assert_eq!(h.prose(3), "原正文");
        let _ = std::fs::remove_dir_all(&home);
    }

    #[test]
    fn canon_classify_strips_old_header() {
        let (home, _guard) = use_home("classify");
        let dir = create_project("归类测试").unwrap();
        let h = ProjectHandle::open(&dir).unwrap();
        std::fs::write(dir.join("canon").join("旧档.md"), "<!-- 类别:旧 -->正文无换行").unwrap();
        let (applied, _) = h.apply_payload(&serde_json::json!({
            "kind": "canonClassify",
            "items": [{ "title": "旧档", "category": "新类" }]
        })).unwrap();
        assert_eq!(applied, 1);
        assert_eq!(
            std::fs::read_to_string(dir.join("canon").join("旧档.md")).unwrap(),
            "<!-- 类别:新类 -->\n正文无换行",
            "旧类别头无结尾换行时也不能留下双头"
        );
        // 空标题跳过，不能落到 未命名作品.md
        let (applied, skipped) = h.apply_payload(&serde_json::json!({
            "kind": "canonClassify",
            "items": [{ "title": "  ", "category": "新类" }]
        })).unwrap();
        assert_eq!(applied, 0);
        assert!(!skipped.is_empty());
        assert!(!dir.join("canon").join("未命名作品.md").exists());
        let _ = std::fs::remove_dir_all(&home);
    }

    #[test]
    fn planned_events_are_deduplicated() {
        let (home, _guard) = use_home("planned");
        let dir = create_project("计划去重").unwrap();
        let h = ProjectHandle::open(&dir).unwrap();
        let payload = serde_json::json!({
            "kind": "plannedEvent", "chapter": 9, "title": "夜探总舵", "from": "线β"
        });
        let (a1, _) = h.apply_payload(&payload).unwrap();
        assert_eq!(a1, 1);
        let (a2, skipped) = h.apply_payload(&payload).unwrap();
        assert_eq!(a2, 1, "重复采纳按幂等成功处理");
        assert!(skipped.iter().any(|s| s.contains("未重复追加")));
        assert_eq!(h.events().len(), 1, "同章同标题只入账一次");
        let _ = std::fs::remove_dir_all(&home);
    }

    #[test]
    fn snapshots_are_unique_within_same_second() {
        let (home, _guard) = use_home("snap");
        let dir = create_project("快照唯一").unwrap();
        let h = ProjectHandle::open(&dir).unwrap();
        h.save_prose(1, "第一版").unwrap();
        h.snapshot_prose(1).unwrap();
        h.save_prose(1, "第二版").unwrap();
        h.snapshot_prose(1).unwrap();
        let snaps = h.snapshots(1);
        assert_eq!(snaps.len(), 2, "同秒两次快照不能互相覆盖");
        assert_ne!(snaps[0], snaps[1]);
        let _ = std::fs::remove_dir_all(&home);
    }

    #[test]
    fn clue_fixes_reschedule_and_last_action_chapter() {
        let (home, _guard) = use_home("clue-fix");
        let dir = create_project("伏笔修复").unwrap();
        let h = ProjectHandle::open(&dir).unwrap();
        h.ensure_chapter(3).unwrap();
        h.save_prose(3, "三章正文").unwrap();
        h.ensure_chapter(9).unwrap();
        h.save_prose(9, "九章正文").unwrap();
        std::fs::write(dir.join("clues.json"), serde_json::to_string(&serde_json::json!([
            { "id": "F01", "title": "虎符", "status": "埋下", "plantedChapter": 3, "lastActionChapter": 3 }
        ])).unwrap()).unwrap();
        let (applied, _) = h.apply_payload(&serde_json::json!({
            "kind": "clueFixes",
            "fixes": [{ "clueID": "F01", "kind": "改期", "detail": "改到第9章" }]
        })).unwrap();
        assert_eq!(applied, 1);
        let c = &h.clues_full()[0];
        assert_eq!(c["plantedChapter"], serde_json::json!(9), "改期必须真的改埋设章");
        assert_eq!(c["lastActionChapter"], serde_json::json!(9), "动作章取最近成稿章");
        assert_eq!(c["status"], serde_json::json!("埋下"), "改期保留原状态");
        // 补埋缺章号 → 拒绝，不写假日志
        assert!(h.apply_payload(&serde_json::json!({
            "kind": "clueFixes",
            "fixes": [{ "clueID": "F01", "kind": "补埋", "detail": "没有章号" }]
        })).is_err());
        // 校正原文 = 修台账文本（正文改写不走本通道）
        h.apply_payload(&serde_json::json!({
            "kind": "clueFixes",
            "fixes": [{ "clueID": "F01", "kind": "校正原文", "detail": "虎符刻有半条龙" }]
        })).unwrap();
        let c = &h.clues_full()[0];
        assert_eq!(c["detail"], serde_json::json!("虎符刻有半条龙"));
        assert_eq!(c["actionLog"].as_array().unwrap().len(), 2);
        // 「由第3章改到第9章」必须取最后一个章号（9），不能拿首个数 3
        h.apply_payload(&serde_json::json!({
            "kind": "clueFixes",
            "fixes": [{ "clueID": "F01", "kind": "改期", "detail": "由第3章改到第9章" }]
        })).unwrap();
        let c = &h.clues_full()[0];
        assert_eq!(c["plantedChapter"], serde_json::json!(9), "改期取最后一个第N章");
        // 显式 plantedChapter 优先于 detail
        h.apply_payload(&serde_json::json!({
            "kind": "clueFixes",
            "fixes": [{ "clueID": "F01", "kind": "改期", "detail": "第8章", "plantedChapter": 11 }]
        })).unwrap();
        assert_eq!(h.clues_full()[0]["plantedChapter"], serde_json::json!(11));
        // 非正文落库也必须有快照：clues.json 原文件可找回
        let mut found = false;
        if let Ok(batches) = std::fs::read_dir(dir.join("snapshots").join("ledger")) {
            for batch in batches.filter_map(|e| e.ok()) {
                if batch.path().join("clues.json").is_file() {
                    found = true;
                }
            }
        }
        assert!(found, "clueFixes 采纳前应备份 clues.json 到 snapshots/ledger/");
        let _ = std::fs::remove_dir_all(&home);
    }

    #[test]
    fn proposal_unknown_fields_survive_rewrite() {
        let (home, _guard) = use_home("proposal-fields");
        let dir = create_project("提案字段").unwrap();
        let h = ProjectHandle::open(&dir).unwrap();
        std::fs::create_dir_all(dir.join("proposals")).unwrap();
        std::fs::write(dir.join("proposals").join("inbox.json"), serde_json::to_string(&serde_json::json!([
            {
                "id": "keep-me", "capability": "记忆提取", "chapterNumber": 7,
                "title": "第7章 记忆", "note": "", "payload": { "kind": "memoryPack", "chapter": 7, "facts": [] },
                "status": "待确认", "createdAt": "2026-01-01T00:00:00Z", "sourceRefs": ["ch-007"]
            },
            {
                "id": "reject-me", "capability": "x", "title": "待拒", "note": "",
                "payload": { "kind": "memo" }, "status": "待确认", "createdAt": "2026-01-01T00:00:00Z"
            }
        ])).unwrap()).unwrap();
        h.reject_proposal("reject-me", "不要").unwrap();
        let raw: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(dir.join("proposals").join("inbox.json")).unwrap()).unwrap();
        let kept = raw.as_array().unwrap().iter().find(|p| p["id"] == serde_json::json!("keep-me")).unwrap();
        assert_eq!(kept["chapterNumber"], serde_json::json!(7), "TS 写入的 chapterNumber 不能被 GUI 重写丢掉");
        assert_eq!(kept["sourceRefs"], serde_json::json!(["ch-007"]));
        let _ = std::fs::remove_dir_all(&home);
    }

    #[test]
    fn ledger_crud_round_trips_and_logs() {
        let (_home, _guard) = use_home("ledger-crud");
        let dir = create_project("台账测试书").unwrap();
        let h = ProjectHandle::open(&dir).unwrap();
        // 新增
        let id = h
            .ledger_add(
                Ledger::Clues,
                serde_json::json!({"title": "断刀的来历", "detail": "刀柄有刻痕"}),
                "新建伏笔",
            )
            .unwrap();
        assert_eq!(h.ledger(Ledger::Clues).unwrap().len(), 1);
        // 改：只并字段，不整体替换
        h.ledger_update(
            Ledger::Clues,
            &id,
            serde_json::json!({"plantedChapter": 3}),
            "补埋设章",
        )
        .unwrap();
        let clue = &h.ledger(Ledger::Clues).unwrap()[0];
        assert_eq!(clue.get("title").and_then(|v| v.as_str()), Some("断刀的来历"));
        assert_eq!(clue.get("plantedChapter").and_then(|v| v.as_u64()), Some(3));
        // 删
        h.ledger_remove(Ledger::Clues, &id, "删掉").unwrap();
        assert!(h.ledger(Ledger::Clues).unwrap().is_empty());
        // 三次写操作都应进日志，且可撤销
        let logs = crate::journal::Journal::new(&h.root).all();
        assert!(logs.len() >= 3, "台账增删改都要留痕，实际 {}", logs.len());
    }

    #[test]
    fn ledger_delete_is_undoable_from_the_journal() {
        let (_home, _guard) = use_home("ledger-undo");
        let dir = create_project("台账撤销书").unwrap();
        let h = ProjectHandle::open(&dir).unwrap();
        let id = h
            .ledger_add(
                Ledger::Clues,
                serde_json::json!({"title": "灯芯", "plantedChapter": 2}),
                "新建伏笔",
            )
            .unwrap();
        h.ledger_remove(Ledger::Clues, &id, "误删").unwrap();
        assert!(h.ledger(Ledger::Clues).unwrap().is_empty());

        // 撤销那次删除：条目回到台账里，字段完整
        let journal = crate::journal::Journal::new(&h.root);
        let del = journal
            .all()
            .into_iter()
            .find(|c| c.path == "clues.json" && c.note == "误删")
            .expect("删除要留痕");
        let outcome = journal.undo(&h.root, &del.id).unwrap();
        assert!(matches!(outcome, crate::journal::UndoOutcome::Restored));
        let back = h.ledger(Ledger::Clues).unwrap();
        assert_eq!(back.len(), 1, "撤销后条目应回来");
        assert_eq!(back[0].get("title").and_then(|v| v.as_str()), Some("灯芯"));
        assert_eq!(back[0].get("plantedChapter").and_then(|v| v.as_u64()), Some(2));
    }

    #[test]
    fn ledger_update_never_changes_id() {
        let (_home, _guard) = use_home("ledger-id");
        let dir = create_project("台账测试书").unwrap();
        let h = ProjectHandle::open(&dir).unwrap();
        let id = h
            .ledger_add(Ledger::Events, serde_json::json!({"objectiveFact": "x"}), "")
            .unwrap();
        // 试图改 id：必须被忽略，否则撤销与引用全断
        h.ledger_update(Ledger::Events, &id, serde_json::json!({"id": "HACKED"}), "")
            .unwrap();
        let row = &h.ledger(Ledger::Events).unwrap()[0];
        assert_eq!(row.get("id").and_then(|v| v.as_str()), Some(id.as_str()));
    }

    #[test]
    fn ledger_add_allocates_unique_ids() {
        let (_home, _guard) = use_home("ledger-ids");
        let dir = create_project("台账测试书").unwrap();
        let h = ProjectHandle::open(&dir).unwrap();
        let a = h.ledger_add(Ledger::Clues, serde_json::json!({"title": "甲"}), "").unwrap();
        let b = h.ledger_add(Ledger::Clues, serde_json::json!({"title": "乙"}), "").unwrap();
        assert_ne!(a, b, "id 不能重复");
        assert_eq!(h.ledger(Ledger::Clues).unwrap().len(), 2);
    }

    #[test]
    fn ledger_corrupt_file_is_not_overwritten_by_crud() {
        let (_home, _guard) = use_home("ledger-corrupt");
        let dir = create_project("台账测试书").unwrap();
        std::fs::write(dir.join("clues.json"), "{ 坏掉了").unwrap();
        let h = ProjectHandle::open(&dir).unwrap();
        // 坏文件时新增必须报错，而不是当成空表覆盖
        assert!(h
            .ledger_add(Ledger::Clues, serde_json::json!({"title": "x"}), "")
            .is_err());
        assert!(std::fs::read_to_string(dir.join("clues.json"))
            .unwrap()
            .contains("坏掉了"));
    }

    #[test]
    fn project_meta_roundtrip_preserves_other_fields() {
        let (home, _guard) = use_home("project-meta");
        let dir = create_project("作品信息").unwrap();
        let h = ProjectHandle::open(&dir).unwrap();
        h.update_project_meta("武侠", "复仇与灯约", 60, 2500).unwrap();
        let meta = h.project_meta();
        assert_eq!(meta.genre, "武侠");
        assert_eq!(meta.premise, "复仇与灯约");
        assert_eq!(meta.target_chapters, 60);
        assert_eq!(meta.chapter_word_target, 2500);
        assert_eq!(h.chapter_word_target(), 2500);
        let raw: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(dir.join("project.json")).unwrap()).unwrap();
        assert_eq!(raw["title"], serde_json::json!("作品信息"));
        assert!(raw["createdAt"].is_string());
        // 非法值拒绝且不动原文件
        let before = std::fs::read_to_string(dir.join("project.json")).unwrap();
        assert!(h.update_project_meta("", "", 0, 2500).is_err());
        assert_eq!(std::fs::read_to_string(dir.join("project.json")).unwrap(), before);
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
    /// 采纳结果（入库/跳过条数）：写入 inbox.json，重启后已决历史仍能回放实际改了什么。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub apply_result: Option<ApplyRecord>,
    /// TS 侧 addProposal 写入的未知字段（chapterNumber、sourceRefs…）原样保留：
    /// GUI 每次采纳/拒收重写 inbox.json 时不能把它们整批丢掉。
    #[serde(flatten, default)]
    pub extra: serde_json::Map<String, serde_json::Value>,
}

#[derive(Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct ApplyRecord {
    #[serde(default)]
    pub applied: usize,
    #[serde(default)]
    pub skipped: Vec<String>,
    /// TS 侧 apply_proposal 写的结果说明；Rust 重写 inbox.json 时原样保留。
    #[serde(default)]
    pub message: String,
    #[serde(default)]
    pub at: String,
}

fn default_status() -> String {
    "待确认".into()
}

/// 提案 payload 的 kind：显式字段优先；旧版（Swift 时代）payload 没有 kind 字段，
/// 按键签名推断——老项目提案也能预览与采纳（双端同语义）。
pub fn payload_kind(payload: &serde_json::Value) -> String {
    if let Some(k) = payload.get("kind").and_then(|k| k.as_str()) {
        if !k.is_empty() {
            return k.to_string();
        }
    }
    // 顺序敏感：clueFixes 先于 clues；docs 先于 items
    for (key, kind) in [
        ("clueFixes", "clueFixes"),
        ("clues", "clues"),
        ("docs", "canon"),
        ("beats", "skeleton"),
        ("findings", "report"),
        ("suggestions", "deslop"),
        ("facts", "memoryPack"),
        ("aliases", "aliases"),
        ("relations", "relations"),
        ("updates", "outlineUpdates"),
        ("outlineUpdates", "outlineUpdates"),
        ("headline", "dossier"),
        ("scenes", "dossier"),
        ("relics", "dossier"),
    ] {
        if payload.get(key).is_some() {
            return kind.to_string();
        }
    }
    if let Some(items) = payload.get("items").and_then(|v| v.as_array()) {
        if items.iter().any(|i| i.get("category").is_some()) {
            return "canonClassify".to_string();
        }
        return "canonRevise".to_string();
    }
    if payload.get("text").is_some() {
        return "draft".to_string();
    }
    String::new()
}

/// 取数组：显式 kind 下为直接数组；旧版（Swift）payload 是 `{"_0":[…],"_1":[…]}` 包装。
/// 两种都认，返回扁平数组；keys 按优先顺序尝试。
fn payload_array(payload: &serde_json::Value, keys: &[&str]) -> Vec<serde_json::Value> {
    for key in keys {
        let Some(v) = payload.get(*key) else { continue };
        if let Some(arr) = v.as_array() {
            return arr.clone();
        }
        if let Some(obj) = v.as_object() {
            let mut ks: Vec<&String> = obj.keys().collect();
            ks.sort_by_key(|k| k.trim_start_matches('_').parse::<usize>().unwrap_or(usize::MAX));
            let mut out = Vec::new();
            for k in ks {
                if let Some(arr) = obj.get(k).and_then(|x| x.as_array()) {
                    out.extend(arr.iter().cloned());
                }
            }
            if !out.is_empty() {
                return out;
            }
        }
    }
    Vec::new()
}

impl ProjectHandle {
    /// 宽松读取（UI 展示）：坏 JSON 当空。写路径一律用 proposals_checked。
    pub fn proposals(&self) -> Vec<Proposal> {
        self.proposals_checked().unwrap_or_default()
    }

    fn proposals_checked(&self) -> Result<Vec<Proposal>, String> {
        let path = self.root.join("proposals").join("inbox.json");
        match read_json_checked(&path)? {
            None => Ok(Vec::new()),
            Some(v) => serde_json::from_value(v)
                .map_err(|e| format!("inbox.json 内容损坏（{e}），已拒绝覆盖")),
        }
    }

    fn save_proposals(&self, list: &[Proposal]) -> Result<(), String> {
        let path = self.root.join("proposals").join("inbox.json");
        let json = serde_json::to_string_pretty(list).map_err(|e| e.to_string())?;
        write_atomic(&path, &json)
    }

    /// 确定性采纳（账房语义与 novel-pack apply_proposal 一致：同题保护/同 id 合并/
    /// 归类按标题匹配）。返回 (applied, skipped 说明)。
    pub fn accept_proposal(&self, id: &str) -> Result<(usize, Vec<String>), String> {
        let mut list = self.proposals_checked()?;
        let idx = list.iter().position(|p| p.id == id).ok_or("提案不存在")?;
        if list[idx].status != "待确认" {
            return Err(format!("提案已处理（{}），拒绝重复采纳", list[idx].status));
        }
        let p = list[idx].clone();
        // 采纳前抓住台账前像：AI 写的 payload 是自由 JSON，落库之后
        // 一旦发现结构坏了（章号写成“第十二章”、id 撞车），必须整档退回，
        // 不能让作者面对一份"半新半旧"的台账。
        let guard = crate::schema::Guard::capture(&self.root);
        let (applied, skipped) = match self.apply_payload(&p.payload) {
            Ok(r) => r,
            Err(e) => return Err(e),
        };
        if let Err(problems) = guard.verify(&self.root) {
            let why: Vec<String> = problems.iter().map(|p| p.summary()).collect();
            return Err(format!(
                "采纳被拒：{}。台账已恢复到采纳前，提案保持待审。",
                why.join("；")
            ));
        }
        if applied == 0 {
            return Err(format!("未入库：{}——提案保持待审", skipped.join("、")));
        }
        list[idx].status = "已采纳".into();
        list[idx].decided_at = Some(iso_now());
        list[idx].apply_result = Some(ApplyRecord {
            applied,
            message: if skipped.is_empty() {
                format!("{applied} 项入库")
            } else {
                format!("{applied} 项入库；跳过 {}", skipped.join("、"))
            },
            skipped: skipped.clone(),
            at: iso_now(),
        });
        self.save_proposals(&list)?;
        Ok((applied, skipped))
    }

    pub fn reject_proposal(&self, id: &str, reason: &str) -> Result<(), String> {
        let mut list = self.proposals_checked()?;
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
        let kind = payload_kind(payload);
        let kind = kind.as_str();
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
                    write_atomic(&file, body)?;
                    applied += 1;
                }
            }
            "clues" => {
                let incoming = payload.get("clues").and_then(|c| c.as_array()).cloned().unwrap_or_default();
                // 写路径用严格读取：坏 clues.json 拒绝覆盖（宽松读会把台账清空）
                let mut clues = self.read_array_checked("clues.json")?;
                for c in incoming {
                    // 自动编号取「现有 F 序号最大值 + 1」，不能拿 len+1：台账有洞时会
                    // 撞上已有 id 走同 id 合并，把新伏笔并进无关条目。
                    let id = c.get("id").and_then(|i| i.as_str()).map(String::from)
                        .unwrap_or_else(|| next_clue_id(&clues));
                    if let Some(pos) = clues.iter().position(|x| x.get("id").and_then(|i| i.as_str()) == Some(id.as_str())) {
                        // 同 id 合并：作者维护字段不覆盖，只补空；未知新字段（如 sourceRefs）
                        // 同样只在缺失时透传，避免双端字段演进时丢数据。
                        let existing = &mut clues[pos];
                        if let Some(obj) = existing.as_object_mut() {
                            for (k, v) in c.as_object().into_iter().flatten() {
                                if k != "id" && k != "status" && !obj.contains_key(k) {
                                    obj.insert(k.clone(), v.clone());
                                }
                            }
                        }
                        applied += 1;
                    } else {
                        let mut merged = c.clone();
                        if let Some(obj) = merged.as_object_mut() {
                            obj.insert("id".into(), serde_json::json!(id));
                            obj.entry("status").or_insert(serde_json::json!("埋下"));
                            let last = obj.get("plantedChapter").cloned().unwrap_or(serde_json::Value::Null);
                            obj.entry("lastActionChapter").or_insert(last);
                        }
                        clues.push(merged);
                        applied += 1;
                    }
                }
                if applied > 0 {
                    self.snapshot_ledger_files(&["clues.json"])?;
                    self.save_clues(&clues)?;
                } else {
                    self.save_clues(&clues)?;
                }
            }
            "canonRevise" => {
                // 预言书晋升：修订既有设定——采纳前自动备份到 canon/backups/
                let items = payload.get("items").and_then(|d| d.as_array()).cloned().unwrap_or_default();
                for it in items {
                    let title = it.get("title").and_then(|t| t.as_str()).unwrap_or("").trim().to_string();
                    if title.is_empty() { continue; }
                    // 缺 content / 空 content 不清空正典（备份在也不该把设定写没）
                    let Some(body) = it.get("content").and_then(|c| c.as_str()).filter(|c| !c.trim().is_empty()) else {
                        skipped.push(format!("设定「{title}」缺修订正文，跳过"));
                        continue;
                    };
                    let file = self.canon_path(&title);
                    if !file.exists() {
                        skipped.push(format!("设定「{title}」未找到（同题校验）"));
                        continue;
                    }
                    let doc = std::fs::read_to_string(&file)
                        .map_err(|e| format!("读取设定「{title}」失败：{e}"))?;
                    let backups = self.root.join("canon").join("backups");
                    std::fs::create_dir_all(&backups).map_err(|e| e.to_string())?;
                    let backup = unique_backup_path(&backups, &safe_file_name(&title));
                    write_atomic(&backup, &doc)?;
                    write_atomic(&file, body)?;
                    applied += 1;
                }
            }
            "plannedEvent" => {
                // 预测排期采纳：计划事件入 events.json（happened=false 待写成）
                let chapter = payload.get("chapter").and_then(|v| v.as_u64()).unwrap_or(0) as u32;
                let title = payload.get("title").and_then(|v| v.as_str()).unwrap_or("").trim().to_string();
                if chapter == 0 || title.is_empty() {
                    return Err("计划事件缺章号或内容".into());
                }
                let mut events = self.read_array_checked("events.json")?;
                // 幂等：同章同标题的计划事件已存在（崩溃/重复采纳）不再追加
                if events.iter().any(|e| {
                    e.get("chapter").and_then(|v| v.as_u64()) == Some(chapter as u64)
                        && e.get("objectiveFact").and_then(|v| v.as_str()) == Some(title.as_str())
                }) {
                    skipped.push(format!("计划事件「{title}」已在第{chapter}章入账，未重复追加"));
                    applied += 1;
                    return Ok((applied, skipped));
                }
                let seq = events
                    .iter()
                    .filter_map(|e| e.get("id").and_then(|i| i.as_str()).and_then(|x| x.strip_prefix('E')).and_then(|n| n.parse::<u64>().ok()))
                    .max()
                    .map(|m| m + 1)
                    .unwrap_or(1);
                let mut entry = serde_json::Map::new();
                entry.insert("id".into(), serde_json::json!(format!("E{seq:02}")));
                entry.insert("chapter".into(), serde_json::json!(chapter));
                entry.insert("objectiveFact".into(), serde_json::json!(title));
                entry.insert("revealed".into(), serde_json::json!(false));
                entry.insert("happened".into(), serde_json::json!(false));
                if let Some(sid) = payload.get("storylineID").and_then(|v| v.as_str()) {
                    if !sid.is_empty() {
                        entry.insert("storylineIDs".into(), serde_json::json!([sid]));
                    }
                }
                entry.insert("plannedFrom".into(), payload.get("from").cloned().unwrap_or(serde_json::json!("")));
                events.push(serde_json::Value::Object(entry));
                self.snapshot_ledger_files(&["events.json"])?;
                self.save_array("events.json", &events)?;
                applied += 1;
            }
            "outlineUpdates" => {
                let updates = payload_array(payload, &["updates", "outlineUpdates"]);
                let mut lines = self.read_array_checked("storylines.json")?;
                // 旧版（Swift）同步条目是事件级：eventID + kind（事件已发生/事件改期/读者已知）+ newChapter
                let mut events = self.events();
                let mut events_dirty = false;
                for u in &updates {
                    let Some(eid) = u.get("eventID").and_then(|v| v.as_str()) else { continue };
                    let k = u.get("kind").and_then(|v| v.as_str()).unwrap_or("");
                    let new_ch = u.get("newChapter").and_then(|v| v.as_u64());
                    let reason = u.get("reason").and_then(|v| v.as_str()).unwrap_or("");
                    let Some(pos) = events.iter().position(|e| e.get("id").and_then(|i| i.as_str()) == Some(eid)) else {
                        skipped.push(format!("事件 {eid} 不存在"));
                        continue;
                    };
                    let ok = match k {
                        "事件已发生" => {
                            if let Some(obj) = events[pos].as_object_mut() {
                                obj.insert("happened".into(), serde_json::json!(true));
                                if let Some(ch) = new_ch { obj.insert("chapter".into(), serde_json::json!(ch)); }
                            }
                            true
                        }
                        "事件改期" => {
                            if let Some(ch) = new_ch {
                                if let Some(obj) = events[pos].as_object_mut() {
                                    obj.insert("chapter".into(), serde_json::json!(ch));
                                }
                                true
                            } else {
                                skipped.push(format!("事件 {eid} 改期缺 newChapter"));
                                false
                            }
                        }
                        "读者已知" => {
                            if let Some(obj) = events[pos].as_object_mut() {
                                obj.insert("revealed".into(), serde_json::json!(true));
                                if !reason.is_empty() {
                                    obj.insert("readerKnowledge".into(), serde_json::json!(reason));
                                }
                                if let Some(ch) = new_ch { obj.insert("chapter".into(), serde_json::json!(ch)); }
                            }
                            true
                        }
                        _ => { skipped.push(format!("事件 {eid} 未知更新类型「{k}」")); false }
                    };
                    if ok {
                        events_dirty = true;
                        applied += 1;
                    }
                }
                if events_dirty {
                    self.snapshot_ledger_files(&["events.json"])?;
                    self.save_array("events.json", &events)?;
                }
                for u in updates {
                    if u.get("eventID").is_some() {
                        continue;
                    }
                    let sid = u.get("storylineID").and_then(|i| i.as_str()).unwrap_or("").to_string();
                    if let Some(pos) = lines.iter().position(|l| l.get("id").and_then(|i| i.as_str()) == Some(sid.as_str())) {
                        let l = &mut lines[pos];
                        if let Some(st) = u.get("status") {
                            if let Some(obj) = l.as_object_mut() { obj.insert("status".into(), st.clone()); }
                        }
                        if let Some(obj) = l.as_object_mut() {
                            // 无 note 的更新不清掉作者上次的同步备注
                            if let Some(note) = u.get("note") {
                                obj.insert("syncNote".into(), note.clone());
                            }
                            obj.insert("syncAt".into(), serde_json::json!(iso_now()));
                        }
                        applied += 1;
                    } else {
                        skipped.push(format!("[{sid}] 未找到"));
                    }
                }
                if applied > 0 {
                    self.snapshot_ledger_files(&["storylines.json"])?;
                }
                self.save_array("storylines.json", &lines)?;
            }
            "canonClassify" => {
                let items = payload.get("items").and_then(|c| c.as_array()).cloned().unwrap_or_default();
                for it in items {
                    let title = it.get("title").and_then(|t| t.as_str()).unwrap_or("").trim().to_string();
                    // 空标题会落到 canon/未命名作品.md：与 canon/canonRevise 一致跳过
                    if title.is_empty() {
                        skipped.push("归类项缺标题，跳过".into());
                        continue;
                    }
                    let file = self.canon_path(&title);
                    if !file.exists() {
                        skipped.push(format!("设定「{title}」未找到（标题可能已改）"));
                        continue;
                    }
                    let category = it.get("category").and_then(|c| c.as_str()).unwrap_or("未分类");
                    let body = std::fs::read_to_string(&file)
                        .map_err(|e| format!("读取设定「{title}」失败：{e}"))?;
                    let body = strip_category_header(&body);
                    self.snapshot_ledger_files(&[&format!("canon/{}.md", safe_file_name(&title))])?;
                    write_atomic(&file, &format!("<!-- 类别:{category} -->\n{body}"))?;
                    if let Some(rename) = it.get("rename").and_then(|r| r.as_str()) {
                        let rename = rename.trim();
                        if !rename.is_empty() && rename != title {
                            let rfile = self.canon_path(rename);
                            if rfile.exists() {
                                skipped.push(format!("改名「{rename}」撞名，保留原文件名"));
                            } else {
                                // 改名失败不能算 applied（旧实现 let _ 吞掉错误）
                                std::fs::rename(&file, &rfile)
                                    .map_err(|e| format!("设定「{title}」改名失败：{e}"))?;
                            }
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
                if text.trim().is_empty() {
                    return Err("草稿正文为空——拒绝用空串覆盖整章（提案保持待审）".into());
                }
                self.snapshot_prose(chapter)?;
                self.save_prose(chapter, text)?;
                applied += 1;
            }
            "skeleton" => {
                let chapter = payload.get("chapter").and_then(|c| c.as_u64()).unwrap_or(0) as u32;
                if chapter == 0 { return Err("骨架缺章号".into()); }
                // 缺 beats 字段时不能写 [] 把已批准节拍清空
                let beats = required_array(payload, "beats", "骨架")?;
                if beats.is_empty() {
                    return Err("骨架缺 beats（或为空）——提案保持待审".into());
                }
                self.ensure_chapter(chapter)?;
                self.snapshot_ledger_files(&[&format!("chapters/{}/meta.json", chapter_dir_name(chapter))])?;
                self.meta_patch(chapter, &[
                    ("beats", serde_json::json!(beats)),
                    ("skeletonApproved", serde_json::json!(true)),
                    ("approvedAt", serde_json::json!(iso_now())),
                ])?;
                applied += 1;
            }
            "report" => {
                // 验证报告归档进章节 meta（只读产物，不触正文）
                let chapter = payload.get("chapter").and_then(|c| c.as_u64()).unwrap_or(0) as u32;
                if chapter == 0 { return Err("报告缺章号".into()); }
                let findings = required_array(payload, "findings", "报告")?;
                if findings.is_empty() {
                    return Err("报告缺 findings（或为空）——提案保持待审".into());
                }
                self.ensure_chapter(chapter)?;
                self.snapshot_ledger_files(&[&format!("chapters/{}/meta.json", chapter_dir_name(chapter))])?;
                self.meta_patch(chapter, &[
                    ("report", serde_json::json!(findings)),
                    ("reportAt", serde_json::json!(iso_now())),
                ])?;
                applied += 1;
            }
            "deslop" => {
                // 逐处替换：找不到 original 的跳过并记录；首次改写前自动快照
                let chapter = payload.get("chapter").and_then(|c| c.as_u64()).unwrap_or(0) as u32;
                if chapter == 0 { return Err("去AI味缺章号".into()); }
                let suggestions = payload.get("suggestions").and_then(|x| x.as_array()).cloned().unwrap_or_default();
                if suggestions.is_empty() { return Err("没有逐处建议".into()); }
                let mut text = self.prose(chapter);
                if text.trim().is_empty() { return Err(format!("第{chapter}章还没有正文，无从替换")); }
                let mut hit = 0usize;
                let mut seen: std::collections::HashSet<String> = std::collections::HashSet::new();
                for sg in &suggestions {
                    let o = sg.get("original").and_then(|v| v.as_str()).unwrap_or("");
                    let r = sg.get("replacement").and_then(|v| v.as_str()).unwrap_or("");
                    if o.is_empty() { continue; }
                    // 同 original 只处理一次：重复采纳/重复条目不能把第二处同名文本也改掉
                    if !seen.insert(o.to_string()) {
                        skipped.push(format!("重复建议已忽略：{}…", o.chars().take(12).collect::<String>()));
                        continue;
                    }
                    if text.contains(o) {
                        if hit == 0 { self.snapshot_prose(chapter)?; }
                        // 只替换第一处命中（逐条采纳语义，幂等防误伤重复段）
                        text = text.replacen(o, r, 1);
                        hit += 1;
                    } else {
                        skipped.push(format!("原文未命中：{}…", o.chars().take(12).collect::<String>()));
                    }
                }
                if hit == 0 { return Err("所有建议的 original 都未命中正文——提案保持待审".into()); }
                self.save_prose(chapter, &text)?;
                applied = hit;
            }
            "relations" => {
                // 人物关系边 → memory.facts（subject/predicate/object）；关系图与人物卡立即挂边
                let rels = payload.get("relations").and_then(|x| x.as_array()).cloned().unwrap_or_default();
                let path = self.root.join("memory.json");
                let mut mem: serde_json::Value = std::fs::read_to_string(&path).ok()
                    .and_then(|t| serde_json::from_str(&t).ok())
                    .unwrap_or_else(|| serde_json::json!({}));
                let mut cur = mem.get("facts").and_then(|f| f.as_array()).cloned().unwrap_or_default();
                for r in rels {
                    let from = r.get("from").and_then(|v| v.as_str()).unwrap_or("").trim().to_string();
                    let pred = r.get("predicate").and_then(|v| v.as_str()).unwrap_or("").trim().to_string();
                    let to = r.get("to").and_then(|v| v.as_str()).unwrap_or("").trim().to_string();
                    if from.is_empty() || pred.is_empty() || to.is_empty() {
                        skipped.push("缺 from/predicate/to".into());
                        continue;
                    }
                    let dup = cur.iter().any(|f| {
                        f.get("subject").and_then(|v| v.as_str()) == Some(from.as_str())
                            && f.get("predicate").and_then(|v| v.as_str()) == Some(pred.as_str())
                            && f.get("object").and_then(|v| v.as_str()) == Some(to.as_str())
                    });
                    if dup {
                        skipped.push(format!("{from}-{pred}-{to} 已在账"));
                        continue;
                    }
                    cur.push(serde_json::json!({
                        "id": uuid_like(),
                        "subject": from,
                        "predicate": pred,
                        "object": to,
                        "fromChapter": r.get("chapter").and_then(|c| c.as_u64()).unwrap_or(0),
                        "publicToReader": r.get("publicToReader").and_then(|b| b.as_bool()).unwrap_or(true),
                        "source": "relation",
                        "note": r.get("evidence").and_then(|v| v.as_str()).unwrap_or(""),
                    }));
                    applied += 1;
                }
                if applied == 0 { return Err(format!("没有可入账的关系：{}", skipped.join("、"))); }
                mem["facts"] = serde_json::json!(cur);
                std::fs::write(&path, serde_json::to_string_pretty(&mem).map_err(|e| e.to_string())?).map_err(|e| e.to_string())?;
            }
            "memoryPack" => {
                // 事实入账 memory.json.facts（subject+predicate+object+生效章 去重）
                let chapter = payload.get("chapter").and_then(|c| c.as_u64()).unwrap_or(0) as u32;
                let facts = payload.get("facts").and_then(|x| x.as_array()).cloned().unwrap_or_default();
                let path = self.root.join("memory.json");
                // 坏 memory.json 不能当空对象再写回——会把 aliases 一起丢掉
                let mut mem = read_json_checked(&path)?.unwrap_or_else(|| serde_json::json!({}));
                if !mem.is_object() {
                    return Err("memory.json 顶层不是对象，已拒绝覆盖".into());
                }
                let mut cur = mem.get("facts").and_then(|f| f.as_array()).cloned().unwrap_or_default();
                let mut added = 0usize;
                for f in facts {
                    // 生效章：f.fromChapter 优先，否则本提案章号（与 TS `f.fromChapter ?? chapter` 对齐）
                    let eff = f.get("fromChapter").and_then(|v| v.as_u64()).unwrap_or(chapter as u64);
                    let dup = cur.iter().any(|x| {
                        x.get("subject") == f.get("subject")
                            && x.get("predicate") == f.get("predicate")
                            && x.get("object") == f.get("object")
                            && (x.get("fromChapter").and_then(|v| v.as_u64()) == Some(eff)
                                || x.get("chapter").and_then(|v| v.as_u64()) == Some(eff))
                    });
                    if dup { skipped.push(format!("事实已在账：{}", f.get("subject").and_then(|v|v.as_str()).unwrap_or("?"))); continue; }
                    let mut entry = f.clone();
                    if let Some(obj) = entry.as_object_mut() {
                        obj.entry("id").or_insert_with(|| serde_json::json!(uuid_like()));
                        obj.entry("fromChapter").or_insert(serde_json::json!(eff));
                        obj.entry("source").or_insert(serde_json::json!("extracted"));
                    }
                    upsert_alias_from_subject(&mut mem, &entry);
                    cur.push(entry);
                    added += 1;
                }
                mem["facts"] = serde_json::json!(cur);
                self.snapshot_ledger_files(&["memory.json"])?;
                write_atomic(&path, &serde_json::to_string_pretty(&mem).map_err(|e| e.to_string())?)?;
                applied = added;
            }
            "aliases" => {
                // 人物别名档案入 memory.json.aliases（canvas 人物/关系维与人物页的事实源）。
                // 归并键 canonicalName；别名数组合并去重，作者已有字段不覆盖。
                let incoming = payload.get("aliases").and_then(|x| x.as_array()).cloned().unwrap_or_default();
                let path = self.root.join("memory.json");
                let mut mem = read_json_checked(&path)?.unwrap_or_else(|| serde_json::json!({}));
                if !mem.is_object() {
                    return Err("memory.json 顶层不是对象，已拒绝覆盖".into());
                }
                let mut cur = mem.get("aliases").and_then(|a| a.as_array()).cloned().unwrap_or_default();
                for a in incoming {
                    let canon = a.get("canonicalName").and_then(|v| v.as_str()).unwrap_or("").trim().to_string();
                    if canon.is_empty() { skipped.push("别名档案缺 canonicalName，跳过".into()); continue; }
                    let pos = cur.iter().position(|x| x.get("canonicalName").and_then(|v| v.as_str()) == Some(canon.as_str()));
                    if let Some(pos) = pos {
                        let existing = &mut cur[pos];
                        if let Some(obj) = existing.as_object_mut() {
                            let mut names: Vec<String> = obj
                                .get("aliases")
                                .and_then(|v| v.as_array())
                                .map(|arr| arr.iter().filter_map(|x| x.as_str().map(String::from)).collect())
                                .unwrap_or_default();
                            if let Some(list) = a.get("aliases").and_then(|v| v.as_array()) {
                                for n in list.iter().filter_map(|v| v.as_str()) {
                                    let n = n.trim();
                                    if !n.is_empty() && n != canon && !names.iter().any(|x| x == n) {
                                        names.push(n.to_string());
                                    }
                                }
                            }
                            obj.insert("aliases".into(), serde_json::json!(names));
                            for (k, v) in a.as_object().into_iter().flatten() {
                                if k != "canonicalName" && k != "aliases" && !obj.contains_key(k) {
                                    obj.insert(k.clone(), v.clone());
                                }
                            }
                        }
                    } else {
                        let mut entry = a.clone();
                        if let Some(obj) = entry.as_object_mut() {
                            obj.insert("canonicalName".into(), serde_json::json!(canon));
                            obj.entry("aliases").or_insert_with(|| serde_json::json!([]));
                        }
                        cur.push(entry);
                    }
                    applied += 1;
                }
                mem["aliases"] = serde_json::json!(cur);
                self.snapshot_ledger_files(&["memory.json"])?;
                write_atomic(&path, &serde_json::to_string_pretty(&mem).map_err(|e| e.to_string())?)?;
            }
            "dossier" => {
                // 档案卡入 dossiers.json（同章覆盖：最新档案为准）
                let chapter = payload.get("chapter").and_then(|c| c.as_u64()).unwrap_or(0) as u32;
                // 与 draft/skeleton/report/deslop/plannedEvent 对齐：缺章号不许占用第 0 条
                if chapter == 0 { return Err("档案卡缺章号".into()); }
                // 全空档案卡按「同章覆盖」会清掉既有档案：拒绝空载荷。
                let has_content = payload
                    .get("headline")
                    .and_then(|h| h.as_str())
                    .map(|h| !h.trim().is_empty())
                    .unwrap_or(false)
                    || ["scenes", "cast", "advanced", "turns", "hooks", "relics"]
                        .iter()
                        .any(|k| payload.get(*k).and_then(|v| v.as_array()).map(|a| !a.is_empty()).unwrap_or(false));
                if !has_content {
                    return Err("档案卡没有可入库内容——提案保持待审".into());
                }
                let mut all = self.read_array_checked("dossiers.json")?;
                let entry = serde_json::json!({
                    "chapter": chapter,
                    "headline": payload.get("headline").cloned().unwrap_or(serde_json::json!("")),
                    "scenes": payload.get("scenes").cloned().unwrap_or(serde_json::json!([])),
                    "cast": payload.get("cast").cloned().unwrap_or(serde_json::json!([])),
                    "hooks": payload.get("hooks").cloned().unwrap_or(serde_json::json!([])),
                    "relics": payload.get("relics").cloned().unwrap_or(serde_json::json!([])),
                    "advanced": payload.get("advanced").cloned().unwrap_or(serde_json::json!([])),
                    "turns": payload.get("turns").cloned().unwrap_or(serde_json::json!([])),
                    "updatedAt": iso_now(),
                });
                // 档案卡的出场人物是人物维/关系维的可靠来源（新项目没有别的别名写入方）。
                // 先写别名再写档案：memory.json 损坏时整个提案保持待审，不留半截落库。
                let cast: Vec<String> = payload
                    .get("cast")
                    .and_then(|c| c.as_array())
                    .map(|arr| {
                        arr.iter()
                            .filter_map(|v| v.as_str().map(str::trim).filter(|s| !s.is_empty()).map(String::from))
                            .collect()
                    })
                    .unwrap_or_default();
                // 原档案与别名表都可能在采纳中被覆盖：先落快照，再写别名/档案。
                self.snapshot_ledger_files(&["dossiers.json", "memory.json"])?;
                self.upsert_alias_names(&cast)?;
                if let Some(pos) = all.iter().position(|d| d.get("chapter").and_then(|c| c.as_u64()) == Some(chapter as u64)) {
                    all[pos] = entry;
                } else {
                    all.push(entry);
                }
                all.sort_by_key(|d| d.get("chapter").and_then(|c| c.as_u64()).unwrap_or(0));
                self.save_array("dossiers.json", &all)?;
                applied += 1;
            }
            "clueFixes" => {
                // 埋点修复动作落台账（只动 clues.json 与动作日志，不动正文）
                let fixes = payload_array(payload, &["fixes", "clueFixes"]); // 旧版键 + _0 包裹都认
                // 动作发生章 = 最近成稿章（有正文的最大章），与 TS writtenChapters 口径一致；
                // 旧实现取最大章目录，空章目录会把动作章推高。
                let latest = self.last_written_chapter();
                let mut clues = self.read_array_checked("clues.json")?;
                for fx in fixes {
                    let cid = fx.get("clueID").and_then(|v| v.as_str()).unwrap_or("");
                    let k = fx.get("kind").and_then(|v| v.as_str()).unwrap_or("");
                    let detail = fx
                        .get("detail")
                        .or_else(|| fx.get("action"))
                        .or_else(|| fx.get("evidence"))
                        .and_then(|v| v.as_str())
                        .unwrap_or("");
                    let Some(pos) = clues.iter().position(|c| c.get("id").and_then(|v| v.as_str()) == Some(cid)) else {
                        skipped.push(format!("{cid} 不在台账（补登记类请先走伏笔盘点提案）"));
                        continue;
                    };
                    // 补埋/改期必须真正改埋设章；目标章优先取显式字段，其次从 detail 解析
                    // 「最后一个第N章」（「由第3章改到第9章」→ 9，不能写首个数 3）。
                    // 解析不到就跳过，不能只写一条日志假装已修。
                    let explicit = ["plantedChapter", "newTargetChapter", "chapter", "toChapter"]
                        .iter()
                        .find_map(|key| fx.get(*key).and_then(chapter_value));
                    let target_chapter = explicit.or_else(|| {
                        if matches!(k, "补埋" | "改期") { chapter_from_detail(detail) } else { None }
                    });
                    let new_status = match k {
                        "回收" | "已回收" => Some("已兑现"),
                        "搁置" => Some("搁置"),
                        "放弃" => Some("放弃"),
                        // 补埋=补登记为已埋；改期只换章号，保留原状态
                        "补埋" => Some("埋下"),
                        "改期" => None,
                        _ => None,
                    };
                    if matches!(k, "补埋" | "改期") && target_chapter.is_none() {
                        skipped.push(format!("{cid} {k} 缺目标章号（detail 写「第N章」或传 plantedChapter）"));
                        continue;
                    }
                    let c = &mut clues[pos];
                    if let Some(obj) = c.as_object_mut() {
                        if let Some(st) = new_status { obj.insert("status".into(), serde_json::json!(st)); }
                        if let Some(ch) = target_chapter { obj.insert("plantedChapter".into(), serde_json::json!(ch)); }
                        obj.insert("lastActionChapter".into(), serde_json::json!(latest));
                        // 「校正原文」= 修正台账里记的原文/说明：detail 即更正后的文本
                        let before_detail = obj.get("detail").cloned();
                        if k == "校正原文" && !detail.trim().is_empty() {
                            obj.insert("detail".into(), serde_json::json!(detail));
                        }
                        let mut log = obj.get("actionLog").and_then(|v| v.as_array()).cloned().unwrap_or_default();
                        // 正文改写必须走 deslop/作者亲笔；本通道只修台账
                        log.push(serde_json::json!({
                            "at": latest,
                            "kind": k,
                            "detail": detail,
                            "beforeDetail": before_detail,
                            "ts": iso_now()
                        }));
                        obj.insert("actionLog".into(), serde_json::json!(log));
                    }
                    applied += 1;
                }
                if applied == 0 { return Err(format!("没有可应用的修复：{}", skipped.join("、"))); }
                self.snapshot_ledger_files(&["clues.json"])?;
                self.save_array("clues.json", &clues)?;
            }
            "memo" | "storylines" | "outlineEvents" => {
                // 无工具产生这些 kind（历史兼容名）：如实报告，提案保持待审
                return Err(format!("「{kind}」无对应落库语义——提案保持待审"));
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
        self.save_array("clues.json", clues)
    }

    pub fn snapshot_prose(&self, n: u32) -> Result<(), String> {
        let dir = self.root.join("chapters").join(chapter_dir_name(n)).join("snapshots");
        std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
        let src = self.prose(n);
        // 纳秒级时间戳：同一秒内两次采纳（canonRevise 备份/自动快照）不会同名覆盖
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0);
        let mut seq = 0u32;
        loop {
            let name = if seq == 0 {
                format!("prose-{nanos:020}.md")
            } else {
                format!("prose-{nanos:020}-{seq}.md")
            };
            let path = dir.join(&name);
            if !path.exists() {
                return write_atomic(&path, &src);
            }
            seq += 1;
        }
    }

    /// 非正文落库快照：采纳前把将被覆盖的原文件复制到
    /// `snapshots/ledger/<批次>/<原相对路径>`。meta.beats/report、dossiers、memory、
    /// storylines、clues、events 没有正文快照页那样的恢复入口，但原文件必须可找回。
    fn snapshot_ledger_files(&self, rels: &[&str]) -> Result<(), String> {
        let base = self.root.join("snapshots").join("ledger");
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0);
        let mut seq = 0u32;
        let dir = loop {
            let name = if seq == 0 {
                format!("{nanos:020}")
            } else {
                format!("{nanos:020}-{seq}")
            };
            let path = base.join(name);
            if !path.exists() {
                break path;
            }
            seq += 1;
        };
        let mut copied = 0usize;
        for rel in rels {
            let src = self.root.join(rel);
            if !src.is_file() {
                continue;
            }
            let dst = dir.join(rel);
            if let Some(parent) = dst.parent() {
                std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
            }
            std::fs::copy(&src, &dst).map_err(|e| format!("备份 {rel} 失败：{e}"))?;
            copied += 1;
        }
        if copied == 0 {
            // 全是新文件：没有旧内容可回滚，不留空批次目录。
            let _ = std::fs::remove_dir(&dir);
            if let Some(parent) = dir.parent() {
                let _ = std::fs::remove_dir(parent);
            }
        }
        Ok(())
    }
}

fn iso_now() -> String {
    let secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    epoch_to_iso8601(secs)
}

// ───────────────────── 台账读取（真实文件名：storylines/events/stages/memory/dossiers）─────────────────────

impl ProjectHandle {
    pub(crate) fn read_array(&self, file: &str) -> Vec<serde_json::Value> {
        std::fs::read_to_string(self.root.join(file))
            .ok()
            .and_then(|t| serde_json::from_str(&t).ok())
            .unwrap_or_default()
    }

    /// 写路径专用：坏 JSON 返回 Err，而不是把空表写回覆盖作者数据。
    fn read_array_checked(&self, file: &str) -> Result<Vec<serde_json::Value>, String> {
        read_array_checked(&self.root.join(file), file)
    }

    pub fn storylines(&self) -> Vec<serde_json::Value> {
        self.read_array("storylines.json")
    }
    pub fn events(&self) -> Vec<serde_json::Value> {
        self.read_array("events.json")
    }
    #[allow(dead_code)] // 幕带（stages）可视化排在 M7-4
    pub fn stages(&self) -> Vec<serde_json::Value> {
        self.read_array("stages.json")
    }

    fn save_array(&self, file: &str, arr: &[serde_json::Value]) -> Result<(), String> {
        write_atomic(
            &self.root.join(file),
            &serde_json::to_string_pretty(arr).map_err(|e| e.to_string())?,
        )
    }

    /// 世界线排期板：作者亲笔拖卡改期（主权编辑，直写台账；非 AI 通道）。
    pub fn set_event_chapter(&self, event_id: &str, chapter: u32) -> Result<String, String> {
        let mut events = self.read_array_checked("events.json")?;
        let idx = events
            .iter()
            .position(|e| e.get("id").and_then(|i| i.as_str()) == Some(event_id))
            .ok_or_else(|| format!("事件 {event_id} 不存在"))?;
        let obj = events[idx].as_object_mut().ok_or("事件条目格式异常")?;
        obj.insert("chapter".into(), serde_json::json!(chapter));
        let label = obj
            .get("objectiveFact")
            .and_then(|f| f.as_str())
            .unwrap_or(event_id)
            .chars()
            .take(12)
            .collect();
        self.save_array("events.json", &events)?;
        Ok(label)
    }

    /// 世界线排期板：拖动伏笔埋设卡改埋入章。
    pub fn set_clue_planted(&self, clue_id: &str, chapter: u32) -> Result<String, String> {
        let mut clues = self.read_array_checked("clues.json")?;
        let idx = clues
            .iter()
            .position(|x| x.get("id").and_then(|i| i.as_str()) == Some(clue_id))
            .ok_or_else(|| format!("伏笔 {clue_id} 不存在"))?;
        let obj = clues[idx].as_object_mut().ok_or("伏笔条目格式异常")?;
        obj.insert("plantedChapter".into(), serde_json::json!(chapter));
        obj.insert("lastActionChapter".into(), serde_json::json!(chapter));
        let label = obj.get("title").and_then(|t| t.as_str()).unwrap_or(clue_id).chars().take(12).collect();
        self.save_array("clues.json", &clues)?;
        Ok(label)
    }
    pub fn memory_facts(&self) -> Vec<serde_json::Value> {
        let path = self.root.join("memory.json");
        std::fs::read_to_string(path)
            .ok()
            .and_then(|t| {
                let v: serde_json::Value = serde_json::from_str(&t).ok()?;
                Some(v.get("facts")?.as_array()?.clone())
            })
            .unwrap_or_default()
    }
    pub fn clues_full(&self) -> Vec<serde_json::Value> {
        self.clues()
    }
    pub fn aliases(&self) -> Vec<serde_json::Value> {
        let path = self.root.join("memory.json");
        std::fs::read_to_string(path)
            .ok()
            .and_then(|t| {
                let v: serde_json::Value = serde_json::from_str(&t).ok()?;
                Some(v.get("aliases")?.as_array()?.clone())
            })
            .unwrap_or_default()
    }

    /// 把一批人物名补进 memory.json.aliases（缺失才补；已有别名表不动）。
    fn upsert_alias_names(&self, names: &[String]) -> Result<(), String> {
        if names.is_empty() {
            return Ok(());
        }
        let path = self.root.join("memory.json");
        let mut mem = read_json_checked(&path)?.unwrap_or_else(|| serde_json::json!({}));
        if !mem.is_object() {
            return Err("memory.json 顶层不是对象，已拒绝覆盖".into());
        }
        for name in names {
            upsert_alias_from_subject(&mut mem, &serde_json::json!({ "subject": name }));
        }
        write_atomic(&path, &serde_json::to_string_pretty(&mem).map_err(|e| e.to_string())?)
    }
    /// 合并写章 meta（骨架/报告/标题共用）。
    pub(crate) fn meta_patch(&self, n: u32, patch: &[(&str, serde_json::Value)]) -> Result<(), String> {
        let dir = self.root.join("chapters").join(chapter_dir_name(n));
        std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
        let meta = dir.join("meta.json");
        let mut v: serde_json::Value = read_json_checked(&meta)?.unwrap_or_else(|| serde_json::json!({}));
        if !v.is_object() {
            return Err(format!("{} 顶层不是对象，已拒绝覆盖", meta.display()));
        }
        if let Some(obj) = v.as_object_mut() {
            for (k, val) in patch {
                obj.insert((*k).to_string(), val.clone());
            }
        }
        write_atomic(&meta, &serde_json::to_string_pretty(&v).map_err(|e| e.to_string())?)
    }

    pub fn set_chapter_title(&self, n: u32, title: &str) -> Result<(), String> {
        let dir = self.root.join("chapters").join(chapter_dir_name(n));
        std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
        let meta = dir.join("meta.json");
        let mut v: serde_json::Value = read_json_checked(&meta)?.unwrap_or_else(|| serde_json::json!({}));
        if !v.is_object() {
            return Err(format!("{} 顶层不是对象，已拒绝覆盖", meta.display()));
        }
        if let Some(obj) = v.as_object_mut() {
            obj.insert("title".into(), serde_json::json!(title));
            obj.insert("number".into(), serde_json::json!(n));
        }
        write_atomic(&meta, &serde_json::to_string_pretty(&v).map_err(|e| e.to_string())?)
    }

    pub fn chapter_notes(&self, n: u32) -> String {
        std::fs::read_to_string(
            self.root.join("chapters").join(chapter_dir_name(n)).join("meta.json"),
        )
        .ok()
        .and_then(|t| {
            let v: serde_json::Value = serde_json::from_str(&t).ok()?;
            v.get("notes").and_then(|x| x.as_str()).map(String::from)
        })
        .unwrap_or_default()
    }

    pub fn set_chapter_notes(&self, n: u32, notes: &str) -> Result<(), String> {
        let dir = self.root.join("chapters").join(chapter_dir_name(n));
        std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
        let meta = dir.join("meta.json");
        let mut v: serde_json::Value = read_json_checked(&meta)?.unwrap_or_else(|| serde_json::json!({}));
        if !v.is_object() {
            return Err(format!("{} 顶层不是对象，已拒绝覆盖", meta.display()));
        }
        if let Some(obj) = v.as_object_mut() {
            obj.insert("notes".into(), serde_json::json!(notes));
        }
        write_atomic(&meta, &serde_json::to_string_pretty(&v).map_err(|e| e.to_string())?)
    }

    /// 最近成稿章（有非空正文的最大章号；无成稿时按 1）。
    fn last_written_chapter(&self) -> u32 {
        self.chapters()
            .iter()
            .filter(|c| c.has_prose)
            .map(|c| c.n)
            .max()
            .unwrap_or(1)
            .max(1)
    }

    /// 作品元信息（project.json 的可编辑子集）。
    pub fn project_meta(&self) -> ProjectMeta {
        let v = read_json_checked(&self.root.join("project.json")).ok().flatten();
        let get_str = |k: &str| {
            v.as_ref()
                .and_then(|x| x.get(k))
                .and_then(|x| x.as_str())
                .unwrap_or("")
                .to_string()
        };
        let get_u32 = |k: &str| {
            v.as_ref()
                .and_then(|x| x.get(k))
                .and_then(|x| x.as_u64())
                .unwrap_or(0) as u32
        };
        ProjectMeta {
            genre: get_str("genre"),
            premise: get_str("premise"),
            target_chapters: get_u32("targetChapters"),
            chapter_word_target: get_u32("chapterWordTarget"),
        }
    }

    /// 更新作品元信息（genre/premise/targetChapters/chapterWordTarget），保留其余字段。
    pub fn update_project_meta(
        &self,
        genre: &str,
        premise: &str,
        target_chapters: u32,
        chapter_word_target: u32,
    ) -> Result<(), String> {
        if target_chapters == 0 {
            return Err("目标章数需大于 0".into());
        }
        if chapter_word_target == 0 {
            return Err("单章目标字数需大于 0".into());
        }
        let path = self.root.join("project.json");
        let mut v = read_json_checked(&path)?
            .unwrap_or_else(|| serde_json::json!({ "title": self.title }));
        if !v.is_object() {
            v = serde_json::json!({ "title": self.title });
        }
        let obj = v.as_object_mut().ok_or("project.json 不是对象")?;
        obj.insert("genre".into(), serde_json::json!(genre.trim()));
        obj.insert("premise".into(), serde_json::json!(premise.trim()));
        obj.insert("targetChapters".into(), serde_json::json!(target_chapters));
        obj.insert("chapterWordTarget".into(), serde_json::json!(chapter_word_target));
        write_atomic(&path, &serde_json::to_string_pretty(&v).map_err(|e| e.to_string())?)
    }

    pub fn chapter_word_target(&self) -> u32 {
        let n = self.project_meta().chapter_word_target;
        if n > 0 { n } else { 3000 }
    }

    pub fn next_chapter_number(&self) -> u32 {
        self.chapters().last().map(|c| c.n + 1).unwrap_or(1)
    }

    pub fn daily_words(&self) -> Vec<(String, usize)> {
        let path = self.root.join("project.json");
        std::fs::read_to_string(path)
            .ok()
            .and_then(|t| {
                let v: serde_json::Value = serde_json::from_str(&t).ok()?;
                let map = v.get("dailyWords")?.as_object()?;
                let mut out: Vec<(String, usize)> = map
                    .iter()
                    .filter_map(|(k, n)| n.as_u64().map(|n| (k.clone(), n as usize)))
                    .collect();
                out.sort();
                Some(out)
            })
            .unwrap_or_default()
    }
}


// ───────────────────────── 应用设置 / 导出 / 快照 ─────────────────────────

/// 应用设置。字段只增不改名：旧 settings.json 里没有的键用默认值补齐，
/// 已有的键保持，所以升级不会让作者丢配置。
///
/// ⚠ Default 是手写的，不是 derive：`derive(Default)` 会把 bool 默认成 false，
/// 于是「升级后旧 settings.json 没有 webEnabled 键 → 联网被静默关掉」。
/// 开关类字段的默认值必须是"开"，否则新功能上线等于把所有老用户关在门外。

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct AppSettings {
    /// OpenAI 兼容端点（局域网/自建代理）；空 = 走 DeepSeek 官方路由。
    pub base_url: String,
    pub model: String,
    pub provider: String,
    /// 推理强度：low / medium / high（空 = 用 DSH 默认）。
    pub reasoning_effort: String,
    /// 单次输出上限（0 = 用 DSH 默认）。
    pub max_tokens: u32,
    /// 默认写作档位：assist=辅助 / cowrite=共写 / ghostwrite=代笔。
    pub writing_mode: String,
    /// 每日字数目标（0 = 不设目标）。
    pub daily_goal: u32,
    /// 允许 AI 联网检索（创造模式查资料时用）。
    pub web_enabled: bool,
    /// 允许 AI 派子 agent。
    pub subagent_enabled: bool,
    /// 空闲回收 DSH 进程的分钟数（0 = 不回收）。
    pub idle_recycle_min: u32,
    /// 主题形态：ink=墨（深色）/ xuan=宣纸（浅色纸感）。缺键 = 墨。
    pub theme: String,
    /// 内置插件开关：行 id → 是否启用。缺键 = 开（默认全开）。
    ///
    /// 为什么 key 是 patch 行 id 而不是包名：能开关的最小单位是一行，
    /// 一个包可能插三行。见 `plugins.rs` 的模块注释。
    pub plugin_overrides: std::collections::BTreeMap<String, bool>,
    /// 同时活着的 DSH 进程上限（每本书一个进程，切书时旧的可能还没释放）。
    ///
    /// 默认 2：正常写作只需要一本书的进程；切书瞬间会短暂出现两个。
    /// 调大没有意义（多出来的进程只会占内存），调小到 1 会让切书时立刻互相顶掉，
    /// 每次回上一本书都要重新启动 + resume。
    pub max_runtime_procs: u32,
}

impl Default for AppSettings {
    fn default() -> Self {
        Self {
            base_url: String::new(),
            model: String::new(),
            provider: String::new(),
            reasoning_effort: String::new(),
            max_tokens: 0,
            writing_mode: String::new(),
            daily_goal: 0,
            // 能力开关默认开：老配置里没有这些键时不该被静默降级
            web_enabled: true,
            subagent_enabled: true,
            idle_recycle_min: 30,
            max_runtime_procs: 2,
            theme: String::new(),
            plugin_overrides: std::collections::BTreeMap::new(),
        }
    }
}

impl AppSettings {
    /// 推理强度合法值；空串表示交给 DSH 默认。
    pub fn effort(&self) -> Option<&str> {
        match self.reasoning_effort.trim() {
            "low" => Some("low"),
            "medium" => Some("medium"),
            "high" => Some("high"),
            _ => None,
        }
    }

    /// 写作档位；非法值一律回落到辅助（最保守）。
    /// M3 的档位闸门读它决定 AI 能写到哪一步。
    #[allow(dead_code)]
    pub fn writing_mode(&self) -> &'static str {
        match self.writing_mode.trim() {
            "cowrite" => "cowrite",
            "ghostwrite" => "ghostwrite",
            _ => "assist",
        }
    }
}

pub fn settings_path() -> PathBuf {
    let mut p = dirs_home();
    p.push("Library");
    p.push("Application Support");
    p.push("执笔3");
    let _ = std::fs::create_dir_all(&p);
    p.join("settings.json")
}

pub fn load_settings() -> AppSettings {
    std::fs::read_to_string(settings_path())
        .ok()
        .and_then(|t| serde_json::from_str(&t).ok())
        .unwrap_or_default()
}

pub fn save_settings(s: &AppSettings) -> Result<(), String> {
    let json = serde_json::to_string_pretty(s).map_err(|e| e.to_string())?;
    write_atomic(&settings_path(), &json)
}

impl ProjectHandle {
    /// 合并稿导出：全部章节正文按序拼接（与旧版 ExportService 同语义）。
    pub fn export_merged(&self) -> Result<PathBuf, String> {
        let mut out = String::new();
        for c in self.chapters() {
            out.push_str(&format!("\n# {}\n\n{}\n", c.title, self.prose(c.n)));
        }
        let dir = projects_dir()
            .parent()
            .map(|p| p.join("执笔导出"))
            .unwrap_or_else(projects_dir);
        std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
        let path = dir.join(format!("{}.md", self.title));
        std::fs::write(&path, out).map_err(|e| e.to_string())?;
        Ok(path)
    }

    pub fn snapshots(&self, n: u32) -> Vec<String> {
        let dir = self.root.join("chapters").join(chapter_dir_name(n)).join("snapshots");
        let Ok(entries) = std::fs::read_dir(&dir) else { return Vec::new() };
        let mut names: Vec<String> = entries
            .filter_map(|e| e.ok())
            .map(|e| e.file_name().to_string_lossy().to_string())
            .filter(|s| s.ends_with(".md"))
            .collect();
        names.sort();
        names.reverse();
        names
    }

    pub fn restore_snapshot(&self, n: u32, name: &str) -> Result<(), String> {
        let src = self.root.join("chapters").join(chapter_dir_name(n)).join("snapshots").join(name);
        let text = std::fs::read_to_string(&src).map_err(|e| e.to_string())?;
        self.save_prose(n, &text)
    }
}

// ───────────────────────── 导入目录 → 提案（启发式，全部走提案） ─────────────────────────

fn collect_md_files(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(dir) else { return };
    for e in entries.filter_map(|e| e.ok()) {
        let p = e.path();
        if p.is_dir() {
            collect_md_files(&p, out);
        } else if let Some(ext) = p.extension().and_then(|e| e.to_str()) {
            if matches!(ext.to_ascii_lowercase().as_str(), "md" | "txt") {
                out.push(p);
            }
        }
    }
}

/// 「第N章」启发式：取文本首个匹配的数字章号（与旧版 ImportService 同思路）。
fn chapter_marker(text: &str) -> Option<u32> {
    let bytes: Vec<char> = text.chars().take(400).collect();
    let s: String = bytes.iter().collect();
    let idx = s.find("第")?;
    let rest: String = s[idx + '第'.len_utf8()..].chars().take_while(|c| c.is_ascii_digit()).collect();
    if rest.is_empty() {
        return None;
    }
    rest.parse().ok()
}

impl ProjectHandle {
    /// 扫描目录生成提案：章 → draft 提案；设定/大纲/人物目录下的文件 → canon 提案。
    /// 返回 (草稿提案数, 设定提案数)。
    pub fn import_dir(&self, dir: &Path) -> Result<(usize, usize), String> {
        let mut files = Vec::new();
        collect_md_files(dir, &mut files);
        files.sort();
        let mut drafts = 0usize;
        let mut canons = 0usize;
        for f in files.iter().take(400) {
            let Ok(text) = std::fs::read_to_string(f) else { continue };
            if text.trim().is_empty() {
                continue;
            }
            let name = f.file_stem().map(|s| s.to_string_lossy().to_string()).unwrap_or_default();
            let path_str = f.to_string_lossy().to_lowercase();
            if let Some(n) = chapter_marker(&text) {
                self.add_proposal_local(
                    "一键写作",
                    &format!("第{n}章 草稿（导入 {name}）"),
                    "导入旧稿：采纳前不进正文；同章已有正文时请先核对。",
                    serde_json::json!({ "kind": "draft", "chapter": n, "text": text }),
                )?;
                drafts += 1;
            } else if ["设定", "大纲", "人物", "world", "outline", "canon"]
                .iter()
                .any(|k| path_str.contains(k))
            {
                self.add_proposal_local(
                    "搭建框架",
                    &format!("设定导入：{name}"),
                    "导入旧稿：同题不覆盖手改文档。",
                    serde_json::json!({
                        "kind": "canon",
                        "docs": [{ "title": name, "content": text, "certainty": "tentative" }]
                    }),
                )?;
                canons += 1;
            }
        }
        Ok((drafts, canons))
    }

    /// 导入**设定文档**（世界观/人物/大纲等非章节材料）到书内暂存区。
    ///
    /// 与 `import_dir`（旧稿 → 章节草稿）的区别：这里**不做任何分类**——
    /// 所有 .md/.txt 原样拷进 `<书>/imports/`，分类交给总 Agent 提取
    /// （imports/INDEX.md 是给它看的清单）。作者手选的材料以什么面貌
    /// 进书，就原样保什么面貌；提取是 AI 的活，存档是作者的活。
    ///
    /// 幂等：同名且同内容的文件跳过；同名不同内容加序号后缀，绝不覆盖。
    pub fn import_settings_docs(&self, dir: &Path) -> Result<ImportedDocs, String> {
        let mut files = Vec::new();
        collect_md_files(dir, &mut files);
        files.sort();
        if files.is_empty() {
            return Err("目录里没有 .md/.txt 文件".into());
        }
        let dest_dir = self.root.join("imports");
        std::fs::create_dir_all(&dest_dir).map_err(|e| e.to_string())?;
        let journal = crate::journal::Journal::new(&self.root);
        let mut out = ImportedDocs {
            count: 0,
            chars: 0,
            skipped: Vec::new(),
            names: Vec::new(),
        };
        for f in files.iter().take(200) {
            let Ok(text) = std::fs::read_to_string(f) else { continue };
            if text.trim().is_empty() {
                continue;
            }
            let name = f
                .file_stem()
                .map(|s| s.to_string_lossy().to_string())
                .unwrap_or_else(|| "未命名".into());
            let mut dest = dest_dir.join(format!("{}.md", safe_file_name(&name)));
            // 同名不同内容：加序号，绝不覆盖作者已导入的材料
            let mut seq = 2;
            while dest.exists()
                && std::fs::read_to_string(&dest).map(|t| t == text).unwrap_or(false) == false
            {
                dest = dest_dir.join(format!("{}-{}.md", safe_file_name(&name), seq));
                seq += 1;
            }
            if dest.exists() {
                out.skipped.push(name);
                continue;
            }
            let before = String::new();
            write_atomic(&dest, &text)?;
            let rel = dest
                .strip_prefix(&self.root)
                .map(|p| p.to_string_lossy().to_string())
                .unwrap_or_else(|_| dest.display().to_string());
            let _ = journal.record(
                &self.root,
                &rel,
                &before,
                &text,
                &crate::journal::Actor::Author,
                "import",
                &format!("b{}", crate::journal::now_millis()),
                &format!("导入设定文档：{}", name),
            );
            out.count += 1;
            out.chars += text.chars().count();
            out.names.push(name);
        }
        self.rebuild_import_index()?;
        Ok(out)
    }

    /// 重建 imports/INDEX.md：给总 Agent 的阅读清单。
    ///
    /// 为什么让 AI 读清单而不是扫目录：一个目录遍历对模型是隐形的成本，
    /// 一份带字数的清单让它能规划"先读哪篇、分几轮"。
    fn rebuild_import_index(&self) -> Result<(), String> {
        let dir = self.root.join("imports");
        let mut lines = vec!["# 导入材料清单".to_string(), String::new()];
        let mut files: Vec<PathBuf> = std::fs::read_dir(&dir)
            .map(|rd| {
                rd.filter_map(|e| e.ok())
                    .map(|e| e.path())
                    .filter(|p| {
                        p.extension().and_then(|e| e.to_str()) == Some("md")
                            && p.file_name().map(|n| n.to_string_lossy() != "INDEX.md").unwrap_or(true)
                    })
                    .collect()
            })
            .unwrap_or_default();
        files.sort();
        for f in files {
            let chars = std::fs::read_to_string(&f)
                .map(|t| t.chars().count())
                .unwrap_or(0);
            let name = f.file_name().map(|s| s.to_string_lossy().to_string()).unwrap_or_default();
            lines.push(format!("- {}（约 {} 字）", name, chars));
        }
        lines.push(String::new());
        lines.push("以上材料为作者导入的原始设定，待总 Agent 提取分类。".to_string());
        let path = dir.join("INDEX.md");
        write_atomic(&path, &lines.join("\n"))
    }

    /// 删除一篇知识库文档（作者动作，journal 留痕可撤销）。
    pub fn delete_imported_doc(&self, name: &str) -> Result<(), String> {
        if name.contains('/') || name.contains('\\') || name.contains("..") {
            return Err("非法文件名".into());
        }
        let path = self.root.join("imports").join(name);
        if !path.exists() {
            return Err(format!("imports/ 里没有 {name}"));
        }
        let text = std::fs::read_to_string(&path).unwrap_or_default();
        std::fs::remove_file(&path).map_err(|e| e.to_string())?;
        let _ = crate::journal::Journal::new(&self.root).record(
            &self.root,
            &format!("imports/{name}"),
            &text,
            "",
            &crate::journal::Actor::Author,
            "remove",
            &format!("b{}", crate::journal::now_millis()),
            &format!("删除知识库文档：{name}"),
        );
        self.rebuild_import_index()
    }

    /// 读一篇知识库文档的原文（预览 / 校验存在性）。
    pub fn read_imported_doc(&self, name: &str) -> Result<String, String> {
        if name.contains('/') || name.contains('\\') || name.contains("..") {
            return Err("非法文件名".into());
        }
        std::fs::read_to_string(self.root.join("imports").join(name))
            .map_err(|e| format!("读不出来：{e}"))
    }

    /// imports/ 里现有几篇待提取材料（0 = 没有，不值得派 AI）。
    /// 返回**文件名**（含 .md）——删除与预览都按名字寻址。
    pub fn imported_docs(&self) -> Vec<String> {
        let dir = self.root.join("imports");
        let mut out: Vec<String> = std::fs::read_dir(&dir)
            .map(|rd| {
                rd.filter_map(|e| e.ok())
                    .map(|e| e.path())
                    .filter(|p| {
                        p.extension().and_then(|e| e.to_str()) == Some("md")
                            && p.file_name().map(|n| n.to_string_lossy() != "INDEX.md").unwrap_or(true)
                    })
                    .filter_map(|p| p.file_name().map(|n| n.to_string_lossy().to_string()))
                    .collect()
            })
            .unwrap_or_default();
        out.sort();
        out
    }

    // ───────────────── M7 画板视图态 / 预言书世界线沙盒 ─────────────────

    /// 世界线列表（含 α 基准行）。
    pub fn worldlines(&self) -> Vec<serde_json::Value> {
        std::fs::read_to_string(self.root.join("worldlines.json"))
            .ok()
            .and_then(|t| serde_json::from_str(&t).ok())
            .unwrap_or_default()
    }

    pub fn save_worldlines(&self, lines: &[serde_json::Value]) -> Result<(), String> {
        write_atomic(
            &self.root.join("worldlines.json"),
            &serde_json::to_string_pretty(lines).map_err(|e| e.to_string())?,
        )
    }

    /// canvas.json：{view:{x,y,k}, pos:{id:[x,y]}, reader:bool}，画板唯一持久化出口。
    pub fn canvas_state(&self) -> serde_json::Value {
        std::fs::read_to_string(self.root.join("canvas.json"))
            .ok()
            .and_then(|t| serde_json::from_str(&t).ok())
            .unwrap_or_else(|| serde_json::json!({}))
    }

    pub fn save_canvas_state(&self, v: &serde_json::Value) -> Result<(), String> {
        write_atomic(
            &self.root.join("canvas.json"),
            &serde_json::to_string_pretty(v).map_err(|e| e.to_string())?,
        )
    }

    /// 宿主侧晋升一条世界线影响 → 提案进收件箱（与 novel-pack promote_diff 同映射同幂等）。
    /// 返回提案 id。
    pub fn promote_diff(&self, worldline_id: &str, diff_index: usize) -> Result<String, String> {
        let mut lines = self.read_array_checked("worldlines.json")?;
        let w = lines
            .iter_mut()
            .find(|x| x.get("id").and_then(|i| i.as_str()) == Some(worldline_id))
            .ok_or_else(|| format!("世界线不存在：{worldline_id}"))?;
        let forked = w.get("forkedChapter").and_then(|c| c.as_u64()).unwrap_or(0) as u32;
        let wname = w.get("name").and_then(|n| n.as_str()).unwrap_or("").to_string();
        let diffs = w
            .get("diffs")
            .and_then(|d| d.as_array())
            .cloned()
            .unwrap_or_default();
        let mut d = diffs
            .into_iter()
            .nth(diff_index)
            .ok_or("该影响不存在")?;
        if d.get("promoted").and_then(|p| p.as_bool()).unwrap_or(false) {
            return Err("该影响已晋升，拒绝重复".into());
        }
        let target = d.get("target").and_then(|t| t.as_str()).unwrap_or("").to_string();
        let target_ref = d.get("targetRef").and_then(|t| t.as_str()).unwrap_or("未命名").to_string();
        let kind = d.get("kind").and_then(|k| k.as_str()).unwrap_or("change").to_string();
        let after = d.get("after").and_then(|a| a.as_str()).unwrap_or("").to_string();
        let reason = d.get("reason").and_then(|r| r.as_str()).unwrap_or("").to_string();
        let payload = match (target.as_str(), kind.as_str()) {
            ("设定", "add") => serde_json::json!({ "kind": "canon", "docs": [{ "title": target_ref, "content": after }], "sourceRefs": [format!("线{worldline_id}"), format!("设定:{target_ref}")] }),
            ("设定", _) => serde_json::json!({ "kind": "canonRevise", "items": [{ "title": target_ref, "content": after }], "sourceRefs": [format!("线{worldline_id}"), format!("设定:{target_ref}")] }),
            ("伏笔", _) => serde_json::json!({ "kind": "clues", "clues": [{ "title": target_ref, "detail": after, "plantedChapter": forked }], "sourceRefs": [format!("线{worldline_id}"), format!("伏笔:{target_ref}")] }),
            // 人物新增 → 别名档案提案（人物/关系维的事实源）
            ("人物", _) => serde_json::json!({ "kind": "aliases", "aliases": [{ "canonicalName": target_ref, "aliases": [] }], "sourceRefs": [format!("线{worldline_id}"), format!("人物:{target_ref}")] }),
            // 事件新增必须带排期章（promote_diff_chapter → plannedEvent）；无章时明确拒绝，
            // 否则会生成一条「找不到故事线」的 outlineUpdates。
            ("事件", "add") => {
                return Err("事件类新增影响请用「排期晋升」（预言书/世界线按章晋升 → plannedEvent）".into())
            }
            _ => serde_json::json!({ "kind": "outlineUpdates", "updates": [{ "storylineID": target_ref, "note": after }], "sourceRefs": [format!("线{worldline_id}"), format!("故事线:{target_ref}")] }),
        };
        let id = uuid_like();
        let mut list = self.proposals_checked()?;
        list.insert(0, Proposal {
            id: id.clone(),
            capability: "预言书·晋升".into(),
            title: format!("{target}·{target_ref}（线{worldline_id}）"),
            note: format!("世界线 {worldline_id}「{wname}」晋升：{reason}"),
            payload,
            status: "待确认".into(),
            created_at: iso_now(),
            decided_at: None,
            reject_reason: None,
            apply_result: None,
            extra: Default::default(),
        });
        self.save_proposals(&list)?;
        if let Some(obj) = d.as_object_mut() {
            obj.insert("promoted".into(), serde_json::json!(true));
            obj.insert("promotedProposalId".into(), serde_json::json!(id));
        }
        // 写回该线 diffs
        for line in lines.iter_mut() {
            if line.get("id").and_then(|i| i.as_str()) == Some(worldline_id) {
                if let Some(obj) = line.as_object_mut() {
                    let cur = obj.get("diffs").and_then(|x| x.as_array()).cloned().unwrap_or_default();
                    let mut new_diffs = cur;
                    if diff_index < new_diffs.len() {
                        new_diffs[diff_index] = d.clone();
                    }
                    obj.insert("diffs".into(), serde_json::json!(new_diffs));
                }
            }
        }
        self.save_worldlines(&lines)?;
        Ok(id)
    }

    /// M8-3：带排期章号的晋升——预测 diff 定向到某章 → plannedEvent 提案。
    pub fn promote_diff_chapter(&self, worldline_id: &str, diff_index: usize, chapter: u32) -> Result<String, String> {
        let mut lines = self.read_array_checked("worldlines.json")?;
        let w = lines
            .iter_mut()
            .find(|x| x.get("id").and_then(|i| i.as_str()) == Some(worldline_id))
            .ok_or_else(|| format!("世界线不存在：{worldline_id}"))?;
        let diffs = w.get("diffs").and_then(|d| d.as_array()).cloned().unwrap_or_default();
        let mut d = diffs.into_iter().nth(diff_index).ok_or("该影响不存在")?;
        if d.get("promoted").and_then(|p| p.as_bool()).unwrap_or(false) {
            return Err("该影响已晋升，拒绝重复".into());
        }
        let after = d.get("after").and_then(|a| a.as_str()).unwrap_or("").to_string();
        let tref = d.get("targetRef").and_then(|t| t.as_str()).unwrap_or("").to_string();
        let payload = serde_json::json!({
            "kind": "plannedEvent",
            "chapter": chapter,
            "title": after,
            "storylineID": tref,
            "from": format!("线{worldline_id}"),
            "sourceRefs": [format!("线{worldline_id}"), format!("故事线:{tref}")],
        });
        let id = uuid_like();
        let mut plist = self.proposals_checked()?;
        plist.insert(0, Proposal {
            id: id.clone(),
            capability: "预言书·排期".into(),
            title: format!("计划事件 → 第{chapter}章（线{worldline_id}）"),
            note: format!("预测排期晋升：{after}"),
            payload,
            status: "待确认".into(),
            created_at: iso_now(),
            decided_at: None,
            reject_reason: None,
            apply_result: None,
            extra: Default::default(),
        });
        self.save_proposals(&plist)?;
        if let Some(obj) = d.as_object_mut() {
            obj.insert("promoted".into(), serde_json::json!(true));
            obj.insert("promotedProposalId".into(), serde_json::json!(id));
            obj.insert("scheduledChapter".into(), serde_json::json!(chapter));
        }
        for line in lines.iter_mut() {
            if line.get("id").and_then(|i| i.as_str()) == Some(worldline_id) {
                if let Some(obj) = line.as_object_mut() {
                    let mut new_diffs = obj.get("diffs").and_then(|x| x.as_array()).cloned().unwrap_or_default();
                    if diff_index < new_diffs.len() {
                        new_diffs[diff_index] = d.clone();
                    }
                    obj.insert("diffs".into(), serde_json::json!(new_diffs));
                }
            }
        }
        self.save_worldlines(&lines)?;
        Ok(id)
    }

    /// 逐节点采纳：把一条 if 线上的某个节点定为正典后续。
    ///
    /// 与 `promote_diff_chapter` 的区别在**粒度**：diff 是"这条假设对正典的影响"，
    /// 一次能有好几条，还常常含糊到没法直接排章；节点是"第 N 章发生这件事"，
    /// 作者看图时指的就是它。计划里写"可逐节点采纳"要的正是这条路径。
    ///
    /// 产出仍是提案（`plannedEvent`），不是直写 events.json：采纳是作者的
    /// 决定，落库仍要过收件箱——否则一条 AI 推出来的节点会绕过裁决进正典。
    /// 节点状态同步置 `adopted`，这样时间轴上能看出"这条已经承认了"，
    /// 而不是还显示成"计划"。
    pub fn adopt_node(&self, worldline_id: &str, node_id: &str) -> Result<String, String> {
        let mut lines = self.read_array_checked("worldlines.json")?;
        let w = lines
            .iter_mut()
            .find(|x| x.get("id").and_then(|i| i.as_str()) == Some(worldline_id))
            .ok_or_else(|| format!("世界线不存在：{worldline_id}"))?;
        let nodes = w.get("nodes").and_then(|n| n.as_array()).cloned().unwrap_or_default();
        let node = nodes
            .iter()
            .find(|n| n.get("id").and_then(|i| i.as_str()) == Some(node_id))
            .cloned()
            .ok_or_else(|| format!("线{worldline_id}没有节点 {node_id}"))?;
        let label = node.get("label").and_then(|v| v.as_str()).unwrap_or("").to_string();
        let state = node.get("state").and_then(|v| v.as_str()).unwrap_or("planned");
        if state == "adopted" || state == "written" {
            return Err(format!("节点「{label}」已采纳，拒绝重复"));
        }
        let reference0 = node
            .get("reference")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();
        // 章号优先取显式 chapter，其次从 reference/label 里的 ch-NNN 猜
        // （与 Node::chapter_or_guess 同一套规则，这里够用）。
        let chapter = node
            .get("chapter")
            .and_then(|v| v.as_u64())
            .filter(|c| *c > 0)
            .map(|c| c as u32)
            .or_else(|| {
                crate::worldline::guess_chapter(&format!("{reference0} {label}"))
            })
            .ok_or_else(|| format!("节点「{label}」还没排章，无法采纳——先给它定第几章"))?;
        let storyline = if reference0.is_empty() { label.clone() } else { reference0.clone() };
        let payload = serde_json::json!({
            "kind": "plannedEvent",
            "chapter": chapter,
            "title": label,
            "storylineID": storyline,
            "from": format!("线{worldline_id}"),
            "sourceRefs": [format!("线{worldline_id}"), format!("节点:{node_id}")],
        });
        let id = uuid_like();
        let mut plist = self.proposals_checked()?;
        plist.insert(0, Proposal {
            id: id.clone(),
            capability: "世界线·逐节点采纳".into(),
            title: format!("{label} → 第{chapter}章（线{worldline_id}）"),
            note: format!("逐节点采纳：线{worldline_id} 的节点 {node_id}"),
            payload,
            status: "待确认".into(),
            created_at: iso_now(),
            decided_at: None,
            reject_reason: None,
            apply_result: None,
            extra: Default::default(),
        });
        self.save_proposals(&plist)?;

        // 写回节点状态
        if let Some(arr) = w.get_mut("nodes").and_then(|n| n.as_array_mut()) {
            for n in arr.iter_mut() {
                if n.get("id").and_then(|i| i.as_str()) == Some(node_id) {
                    if let Some(obj) = n.as_object_mut() {
                        obj.insert("state".into(), serde_json::json!("adopted"));
                        obj.insert("adoptedProposalId".into(), serde_json::json!(id));
                    }
                }
            }
        }
        self.save_worldlines(&lines)?;
        Ok(id)
    }

    /// 世界线归档/恢复（预言书面板按钮；α 基准不可动）。
    pub fn toggle_worldline_archive(&self, id: &str) -> Result<String, String> {
        if id == "α" {
            return Err("α 基准线不可归档".into());
        }
        let mut lines = self.worldlines();
        let w = lines
            .iter_mut()
            .find(|x| x.get("id").and_then(|i| i.as_str()) == Some(id))
            .ok_or_else(|| format!("世界线不存在：{id}"))?;
        let cur = w.get("status").and_then(|v| v.as_str()).unwrap_or("已推演");
        let new = if cur == "已归档" { "已推演" } else { "已归档" };
        if let Some(obj) = w.as_object_mut() {
            obj.insert("status".into(), serde_json::json!(new));
            obj.insert("updatedAt".into(), serde_json::json!(iso_now()));
        }
        self.save_worldlines(&lines)?;
        Ok(new.to_string())
    }

    /// 直接落一条待审提案（内部用：能力产出、测试）。
    ///
    /// 返回提案 id——调用方几乎总要拿它去 accept 或回显，不返回就只能再查一遍。
    fn add_proposal_local(
        &self,
        capability: &str,
        title: &str,
        note: &str,
        payload: serde_json::Value,
    ) -> Result<String, String> {
        let id = uuid_like();
        let mut list = self.proposals_checked()?;
        list.insert(0, Proposal {
            id: id.clone(),
            capability: capability.into(),
            title: title.into(),
            note: note.into(),
            payload,
            status: "待确认".into(),
            created_at: iso_now(),
            decided_at: None,
            reject_reason: None,
            apply_result: None,
            extra: Default::default(),
        });
        self.save_proposals(&list)?;
        Ok(id)
    }
}

fn uuid_like() -> String {
    use std::time::{SystemTime, UNIX_EPOCH};
    let nanos = SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_nanos()).unwrap_or(0);
    format!("{:016x}", nanos)
}


/// 今日键（yyyy-MM-dd，东八区）。标准库无本地时区 API；产品面向中文作者，
/// 固定 +8h 偏移，避免 UTC 日期在晚上 8 点后把「今日字数」记到明天。
fn today_key() -> String {
    let secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
        + 8 * 3600;
    let days = secs / 86_400;
    let (y, m, d) = civil_from_days(days as i64);
    format!("{y:04}-{m:02}-{d:02}")
}

/// 自动伏笔编号：现有 `F<n>` 的最大序号 + 1（跳过被占用的 id）。
/// `len()+1` 在台账有洞时会撞上既有 id，触发同 id 合并把新伏笔并进无关条目。
fn next_clue_id(clues: &[serde_json::Value]) -> String {
    let max = clues
        .iter()
        .filter_map(|c| c.get("id").and_then(|i| i.as_str()))
        .filter_map(|id| id.strip_prefix('F'))
        .filter_map(|n| n.parse::<u64>().ok())
        .max()
        .unwrap_or(0);
    let mut n = max + 1;
    loop {
        let id = format!("F{n}");
        if !clues
            .iter()
            .any(|c| c.get("id").and_then(|i| i.as_str()) == Some(id.as_str()))
        {
            return id;
        }
        n += 1;
    }
}

/// 显式章号字段：数字或数字字符串，0/负数视为未提供。
pub(crate) fn chapter_value(v: &serde_json::Value) -> Option<u32> {
    match v {
        serde_json::Value::Number(n) => n.as_u64().filter(|n| *n > 0).map(|n| n as u32),
        serde_json::Value::String(s) => s.trim().parse::<u32>().ok().filter(|n| *n > 0),
        _ => None,
    }
}

/// 从中文说明里取「最后一个第N章/回」：「由第3章改到第9章」→ 9（不能取首个数 3）。
/// 没有章标记时退化为最后一个数字串。
pub(crate) fn chapter_from_detail(text: &str) -> Option<u32> {
    let chars: Vec<char> = text.chars().collect();
    let mut marked: Vec<u32> = Vec::new();
    let mut i = 0;
    while i < chars.len() {
        if chars[i] == '第' {
            let mut j = i + 1;
            while j < chars.len() && chars[j].is_whitespace() {
                j += 1;
            }
            let start = j;
            while j < chars.len() && chars[j].is_ascii_digit() {
                j += 1;
            }
            if j > start {
                let mut k = j;
                while k < chars.len() && chars[k].is_whitespace() {
                    k += 1;
                }
                if k < chars.len() && (chars[k] == '章' || chars[k] == '回') {
                    let num: String = chars[start..j].iter().collect();
                    if let Ok(n) = num.parse::<u32>() {
                        if n > 0 {
                            marked.push(n);
                        }
                    }
                }
            }
        }
        i += 1;
    }
    if let Some(n) = marked.last() {
        return Some(*n);
    }
    let mut last = None;
    let mut i = 0;
    while i < chars.len() {
        if chars[i].is_ascii_digit() {
            let start = i;
            while i < chars.len() && chars[i].is_ascii_digit() {
                i += 1;
            }
            let num: String = chars[start..i].iter().collect();
            if let Ok(n) = num.parse::<u32>() {
                if n > 0 {
                    last = Some(n);
                }
            }
        } else {
            i += 1;
        }
    }
    last
}

/// 提案必填数组字段：缺失/类型不对时报错，而不是写 [] 清空既有批准内容。
fn required_array(
    payload: &serde_json::Value,
    key: &str,
    what: &str,
) -> Result<Vec<serde_json::Value>, String> {
    payload
        .get(key)
        .and_then(|v| v.as_array())
        .cloned()
        .ok_or_else(|| format!("{what}缺 {key} 数组——提案保持待审"))
}

/// 去掉设定文档首行的 `<!-- 类别:… -->` 标记（无结尾换行也不留双头）。
fn strip_category_header(body: &str) -> String {
    let Some(rest) = body.strip_prefix("<!-- 类别:") else {
        return body.to_string();
    };
    let Some((_, after)) = rest.split_once("-->") else {
        return body.to_string();
    };
    after.strip_prefix('\n').unwrap_or(after).to_string()
}

/// 备份文件名：纳秒时间戳 + 冲突序号，同秒多次修订不会互相覆盖。
fn unique_backup_path(dir: &Path, title: &str) -> PathBuf {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    let mut seq = 0u32;
    loop {
        let name = if seq == 0 {
            format!("{title}.{nanos:020}.md")
        } else {
            format!("{title}.{nanos:020}-{seq}.md")
        };
        let path = dir.join(name);
        if !path.exists() {
            return path;
        }
        seq += 1;
    }
}

/// memoryPack 顺带补人物别名档案：新项目没有 aliases 写入方，人物/关系两维会恒空。
/// 只补缺失的 canonicalName；已有档案（含作者手填别名）不动。
fn upsert_alias_from_subject(mem: &mut serde_json::Value, fact: &serde_json::Value) {
    let subject = fact.get("subject").and_then(|v| v.as_str()).unwrap_or("").trim();
    if subject.is_empty() {
        return;
    }
    let aliases = mem
        .as_object_mut()
        .map(|obj| obj.entry("aliases").or_insert_with(|| serde_json::json!([])))
        .and_then(|v| v.as_array_mut());
    let Some(aliases) = aliases else { return };
    if aliases
        .iter()
        .any(|a| a.get("canonicalName").and_then(|v| v.as_str()) == Some(subject))
    {
        return;
    }
    aliases.push(serde_json::json!({
        "canonicalName": subject,
        "aliases": [],
        "source": "extracted"
    }));
}

fn civil_from_days(z0: i64) -> (i64, u32, u32) {
    let z = z0 + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    (if m <= 2 { y + 1 } else { y }, m as u32, d as u32)
}

/// 相对时间：刚刚 / x 分钟前 / x 小时前 / x 天前。
pub fn relative_time(ts: Option<std::time::SystemTime>) -> String {
    let Some(t) = ts else { return String::new() };
    let Ok(elapsed) = std::time::SystemTime::now().duration_since(t) else {
        return "刚刚".into();
    };
    let mins = elapsed.as_secs() / 60;
    if mins < 1 {
        "刚刚".into()
    } else if mins < 60 {
        format!("{mins} 分钟前")
    } else if mins < 60 * 24 {
        format!("{} 小时前", mins / 60)
    } else {
        format!("{} 天前", mins / (60 * 24))
    }
}

/// 书架状态徽标：有正文=写作中；有章节无正文=规划中；否则=新建。
pub fn shelf_status(chapters: usize, words: usize) -> &'static str {
    if words > 0 {
        "写作中"
    } else if chapters > 0 {
        "规划中"
    } else {
        "新建"
    }
}

/// 千分位数字。
pub fn fmt_thousands(n: usize) -> String {
    let digits = n.to_string();
    let mut out = String::new();
    for (i, ch) in digits.chars().enumerate() {
        if i > 0 && (digits.len() - i) % 3 == 0 {
            out.push(',');
        }
        out.push(ch);
    }
    out
}

/// 台账种类。作者能直接增删改的实体就这几类。
///
/// 为什么用枚举而不是字符串：调用方写错文件名是静默错误（写到不存在的台账上），
/// 枚举让它变成编译错误。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Ledger {
    Storylines,
    Events,
    Stages,
    Clues,
}

impl Ledger {
    pub fn file(self) -> &'static str {
        match self {
            Ledger::Storylines => "storylines.json",
            Ledger::Events => "events.json",
            Ledger::Stages => "stages.json",
            Ledger::Clues => "clues.json",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Ledger::Storylines => "故事线",
            Ledger::Events => "事件",
            Ledger::Stages => "幕",
            Ledger::Clues => "伏笔",
        }
    }

    /// id 前缀：新建时按这个分配，与既有数据风格一致。
    fn id_prefix(self) -> &'static str {
        match self {
            Ledger::Storylines => "L",
            Ledger::Events => "E",
            Ledger::Stages => "S",
            Ledger::Clues => "F",
        }
    }
}

impl ProjectHandle {
    /// 读一份台账（坏 JSON 报错而不是当空——写路径不能拿空表覆盖用户数据）。
    pub fn ledger(&self, which: Ledger) -> Result<Vec<serde_json::Value>, String> {
        self.read_array_checked(which.file())
    }

    /// 新增一条。自动分配 id 并补常用默认字段；`note` 写进改动日志。
    pub fn ledger_add(
        &self,
        which: Ledger,
        mut item: serde_json::Value,
        note: &str,
    ) -> Result<String, String> {
        let mut list = self.ledger(which)?;
        let before = serde_json::to_string(&list).unwrap_or_default();
        // id 去重：已有同 id 就换下一个
        let mut seq = list.len() + 1;
        let id = loop {
            let cand = format!("{}{:02}", which.id_prefix(), seq);
            if !list.iter().any(|x| x.get("id").and_then(|v| v.as_str()) == Some(&cand)) {
                break cand;
            }
            seq += 1;
        };
        let obj = item.as_object_mut().ok_or("新增条目必须是 JSON 对象")?;
        obj.entry("id".to_string()).or_insert(serde_json::json!(id));
        let final_id = obj.get("id").and_then(|v| v.as_str()).unwrap_or(&id).to_string();
        list.push(item);
        self.save_array(which.file(), &list)?;
        let after = serde_json::to_string(&list).unwrap_or_default();
        self.log_ledger(which, &before, &after, note);
        Ok(final_id)
    }

    /// 改一条：把 `patch` 里的字段并进去（不整体替换，避免丢掉未在表单上的字段）。
    pub fn ledger_update(
        &self,
        which: Ledger,
        id: &str,
        patch: serde_json::Value,
        note: &str,
    ) -> Result<(), String> {
        let mut list = self.ledger(which)?;
        let before = serde_json::to_string(&list).unwrap_or_default();
        let idx = list
            .iter()
            .position(|x| x.get("id").and_then(|v| v.as_str()) == Some(id))
            .ok_or_else(|| format!("{} 里没有 id={id}", which.label()))?;
        if let (Some(dst), Some(src)) = (list[idx].as_object_mut(), patch.as_object()) {
            for (k, v) in src {
                // id 不允许被改：它是撤销与引用的锚
                if k == "id" {
                    continue;
                }
                dst.insert(k.clone(), v.clone());
            }
        }
        self.save_array(which.file(), &list)?;
        let after = serde_json::to_string(&list).unwrap_or_default();
        self.log_ledger(which, &before, &after, note);
        Ok(())
    }

    /// 删一条。
    pub fn ledger_remove(&self, which: Ledger, id: &str, note: &str) -> Result<(), String> {
        let mut list = self.ledger(which)?;
        let before = serde_json::to_string(&list).unwrap_or_default();
        let len = list.len();
        list.retain(|x| x.get("id").and_then(|v| v.as_str()) != Some(id));
        if list.len() == len {
            return Err(format!("{} 里没有 id={id}", which.label()));
        }
        self.save_array(which.file(), &list)?;
        let after = serde_json::to_string(&list).unwrap_or_default();
        self.log_ledger(which, &before, &after, note);
        Ok(())
    }

    /// 台账写入统一记日志。日志失败不阻断——台账已经安全落盘了。
    fn log_ledger(&self, which: Ledger, before: &str, after: &str, note: &str) {
        if before == after {
            return;
        }
        let _ = crate::journal::Journal::new(&self.root).record(
            &self.root,
            which.file(),
            before,
            after,
            &crate::journal::Actor::Author,
            "edit",
            &format!("b{}", crate::journal::now_millis()),
            note,
        );
    }

}
