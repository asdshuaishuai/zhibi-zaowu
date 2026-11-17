//! 全局搜索（作者侧）：⌥⌘F 唤起，跨正文/设定/人物/伏笔/事件全文子串匹配。
//!
//! 纯本地、零 AI、零网络，采用子串匹配。
//! 结果按类别分组，点一条定位章节、设定文档或台账条目。

use std::path::PathBuf;
use std::rc::Rc;

use gpui_kit::component::button::Button;
use gpui_kit::component::dock::{BasePanel, Panel as ComponentPanel, PanelEvent};
use gpui_kit::component::input::{Input, InputEvent, InputState};
use gpui_kit::component::Sizable;
use gpui_kit::{
    App, AppContext as _, Context, Entity, EventEmitter, FocusHandle, Focusable, FontWeight,
    InteractiveElement as _, IntoElement, ParentElement, Render, SharedString,
    StatefulInteractiveElement as _, Styled, Window, div,
};

use crate::store::ProjectHandle;
use crate::theme;

/// 一条搜索结果。
#[derive(Clone, Debug, PartialEq)]
pub struct SearchHit {
    /// 类别（分组用）
    pub kind: HitKind,
    /// 主行：「第 12 章 · 断刀」「沈砚」「半块虎符」
    pub title: String,
    /// 上下文片段（命中词前后各截 24 字，命中词**加粗**渲染由 UI 负责）
    pub snippet: String,
    /// 跳转目标
    pub target: HitTarget,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum HitKind {
    Chapter,
    Canon,
    Character,
    Clue,
    Event,
}

impl HitKind {
    pub fn label(self) -> &'static str {
        match self {
            Self::Chapter => "章节",
            Self::Canon => "设定",
            Self::Character => "人物",
            Self::Clue => "伏笔",
            Self::Event => "事件",
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub enum HitTarget {
    /// 打开第 N 章稿纸
    Chapter(u32),
    Canon(String),
    Character(String),
    Clue(String),
    Event(String),
}

/// 搜出来的批次（一次 query 的全部命中，按类别分组）。
pub struct SearchResults {
    pub query: String,
    pub hits: Vec<SearchHit>,
    pub elapsed_ms: u64,
}

impl EventEmitter<PanelEvent> for SearchPanel {}
impl EventEmitter<SearchEvent> for SearchPanel {}

#[derive(Debug)]
pub enum SearchEvent {
    Jump(HitTarget),
    Close,
}

pub struct SearchPanel {
    focus_handle: FocusHandle,
    handle: Rc<ProjectHandle>,
    input: Entity<InputState>,
    results: Option<SearchResults>,
}

impl SearchPanel {
    pub fn new(window: &mut Window, cx: &mut Context<Self>, handle: Rc<ProjectHandle>) -> Self {
        let input = cx.new(|cx| {
            InputState::new(window, cx).placeholder("搜正文 / 设定 / 人物 / 伏笔 / 事件…")
        });
        cx.subscribe(&input, |this, _, ev: &InputEvent, cx| {
            if matches!(ev, InputEvent::Change) {
                this.run_search(cx);
            }
        })
        .detach();
        Self {
            focus_handle: cx.focus_handle(),
            handle,
            input,
            results: None,
        }
    }

    /// 唤起搜索页时把工作区焦点移进搜索框（⌥⌘F 之后打开就能敲字）。
    pub fn input_focus_handle(&self, cx: &Context<Self>) -> FocusHandle {
        self.input.read(cx).focus_handle(cx)
    }

    fn run_search(&mut self, cx: &mut Context<Self>) {
        let q = self.input.read(cx).value().trim().to_string();
        if q.len() < 2 {
            self.results = None;
            cx.notify();
            return;
        }
        let started = std::time::Instant::now();
        let hits = search_book(&self.handle, &q);
        self.results = Some(SearchResults {
            query: q,
            hits,
            elapsed_ms: started.elapsed().as_millis() as u64,
        });
        cx.notify();
    }
}

impl Focusable for SearchPanel {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus_handle.clone()
    }
}

/// 全书正文语料缓存，**按书目录分键**。
///
/// 为什么要分键：这是进程级静态缓存，而一个进程里可能先后打开多本书
/// （书架切书、测试并行跑不同 fixture）。只存一份会让 A 书的内容被 B 书命中，
/// 表现为"搜到了别的书里的句子"——比慢严重得多。
///
/// 版本用章节代数号（写路径递增），不用逐文件 mtime：后者每轮要 300 次 stat，
/// 每次按键都搜时仍是可见开销。代数号只在保存/建章/改标题时变，时机完全一致。
type Corpus = Vec<(u32, String, String)>;

fn corpus_cache() -> &'static std::sync::Mutex<std::collections::HashMap<PathBuf, (u64, Corpus)>> {
    static CACHE: std::sync::OnceLock<
        std::sync::Mutex<std::collections::HashMap<PathBuf, (u64, Corpus)>>,
    > = std::sync::OnceLock::new();
    CACHE.get_or_init(|| std::sync::Mutex::new(std::collections::HashMap::new()))
}

/// 取全书正文（章号, 标题, 正文）。命中缓存时不读盘、不 stat。
fn prose_corpus(handle: &ProjectHandle) -> Corpus {
    let sig = crate::store::chapters_epoch();
    if let Ok(guard) = corpus_cache().lock()
        && let Some((cached_sig, corpus)) = guard.get(&handle.root)
        && *cached_sig == sig
    {
        return corpus.clone();
    }
    let corpus: Corpus = handle
        .chapters()
        .into_iter()
        .map(|ch| (ch.n, ch.title, handle.prose(ch.n)))
        .collect();
    if let Ok(mut guard) = corpus_cache().lock() {
        guard.insert(handle.root.clone(), (sig, corpus.clone()));
    }
    corpus
}

/// 跨正文 + 台账 + 设定的子串搜索。大小写敏感（中文无大小写问题）。
/// 返回按 HitKind 字典序 + 组内标题排序的结果。
pub fn search_book(handle: &ProjectHandle, query: &str) -> Vec<SearchHit> {
    let mut hits = Vec::new();
    if query.is_empty() {
        return hits;
    }
    let q = query;

    // 1. 正文（含标题）
    for (n, title, prose) in prose_corpus(handle) {
        if let Some(snippet) = snippet_around(&prose, q) {
            hits.push(SearchHit {
                kind: HitKind::Chapter,
                title: format!("第 {n} 章 · {title}"),
                snippet,
                target: HitTarget::Chapter(n),
            });
        } else if title.contains(q) {
            hits.push(SearchHit {
                kind: HitKind::Chapter,
                title: format!("第 {n} 章 · {title}"),
                snippet: format!("（标题命中）{title}"),
                target: HitTarget::Chapter(n),
            });
        }
    }

    // 2. 设定（canon/*.md）
    for entry in std::fs::read_dir(handle.root.join("canon")).into_iter().flatten().filter_map(|e| e.ok()) {
        let path = entry.path();
        if path.extension().and_then(|s| s.to_str()) != Some("md") {
            continue;
        }
        let title = path.file_stem().and_then(|s| s.to_str()).unwrap_or("").to_string();
        if let Ok(text) = std::fs::read_to_string(&path)
            && let Some(snippet) = snippet_around(&text, q).or_else(|| {
                if title.contains(q) { Some(format!("（标题命中）{title}")) } else { None }
            }) {
                hits.push(SearchHit {
                    kind: HitKind::Canon,
                    title: title.clone(),
                    snippet,
                    target: HitTarget::Canon(title.clone()),
                });
        }
    }

    // 3. 人物档案（characters.json 的所有字符串字段）
    for c in crate::characters::Roster::new(&handle.root).all() {
        let hit = c.name.contains(q)
            || c.background.contains(q)
            || c.personality.contains(q)
            || c.appearance.contains(q)
            || c.voice.contains(q)
            || c.motivation.contains(q)
            || c.notes.contains(q);
        if hit {
            hits.push(SearchHit {
                kind: HitKind::Character,
                title: c.name.clone(),
                snippet: c.background.chars().take(80).collect(),
                target: HitTarget::Character(if c.id.is_empty() { c.name.clone() } else { c.id.clone() }),
            });
        }
    }

    // 4. 伏笔（clues.json）
    for cl in handle.clues_full() {
        let title = cl.get("title").and_then(|v| v.as_str()).unwrap_or("").to_string();
        if title.contains(q) {
            let id = cl.get("id").and_then(|v| v.as_str()).unwrap_or("").to_string();
            if id.is_empty() { continue; }
            hits.push(SearchHit {
                kind: HitKind::Clue,
                title,
                snippet: id.clone(),
                target: HitTarget::Clue(id),
            });
        }
    }

    // 5. 事件（events.json 的 objectiveFact）
    for e in handle.events() {
        let fact = e.get("objectiveFact").and_then(|v| v.as_str()).unwrap_or("").to_string();
        if fact.contains(q) {
            let id = e.get("id").and_then(|v| v.as_str()).unwrap_or("").to_string();
            if id.is_empty() { continue; }
            let ch = e.get("chapter").and_then(|v| v.as_u64()).unwrap_or(0) as u32;
            hits.push(SearchHit {
                kind: HitKind::Event,
                title: fact.chars().take(50).collect(),
                snippet: format!("第 {ch} 章 · {id}"),
                target: HitTarget::Event(id),
            });
        }
    }

    // 排序：类别分组（Chapter → Canon → Character → Clue → Event），组内按标题
    hits.sort_by(|a, b| a.kind.cmp(&b.kind).then(a.title.cmp(&b.title)));
    hits
}

/// 命中词前后各截 24 字。返回 None 表示没命中。
fn snippet_around(text: &str, q: &str) -> Option<String> {
    let pos = text.find(q)?;
    let start = text[..pos].chars().rev().take(24).collect::<String>().chars().rev().collect::<String>();
    let end: String = text[pos + q.len()..].chars().take(24).collect();
    Some(format!("…{start}【{q}】{end}…"))
}

impl Render for SearchPanel {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let mut body = div()
            .flex()
            .flex_col()
            .gap(theme::GAP_MD)
            .w_full()
            .p(theme::PAD_LG)
            .bg(theme::BG());

        // 搜索框
        body = body.child(
            div()
                .flex()
                .items_center()
                .gap(theme::GAP_SM)
                .child(div().flex_1().min_w_0().child(
                    Input::new(&self.input)
                        .bg(theme::PANEL())
                        .border_1()
                        .border_color(theme::ACCENT_BORDER())
                        .rounded(theme::RADIUS_BTN)
                        .text_size(theme::FONT_BODY)
                        .text_color(theme::TEXT()),
                ))
                .child(
                    Button::new("search-close")
                        .label("关闭")
                        .xsmall()
                        .on_click(cx.listener(|_, _, _, cx| {
                            cx.emit(SearchEvent::Close);
                        })),
                ),
        );

        // 结果
        match &self.results {
            None => {
                body = body.child(
                    div()
                        .text_size(theme::FONT_META)
                        .text_color(theme::FAINT())
                        .child("输入 2 个以上字符开始搜——正文 / 设定 / 人物 / 伏笔 / 事件都搜"),
                );
            }
            Some(res) if res.hits.is_empty() => {
                body = body.child(
                    div()
                        .text_size(theme::FONT_META)
                        .text_color(theme::DIM())
                        .child(format!("没有命中「{}」", res.query)),
                );
            }
            Some(res) => {
                body = body.child(
                    div()
                        .text_size(theme::FONT_TAG)
                        .text_color(theme::FAINT())
                        .child(format!("{} 条命中 · {} ms", res.hits.len(), res.elapsed_ms)),
                );
                let mut last_kind: Option<HitKind> = None;
                for (i, hit) in res.hits.iter().enumerate() {
                    if last_kind != Some(hit.kind) {
                        last_kind = Some(hit.kind);
                        body = body.child(
                            div()
                                .text_size(theme::FONT_LABEL)
                                .text_color(theme::FAINT())
                                .pt(theme::GAP_SM)
                                .child(hit.kind.label()),
                        );
                    }
                    let target = hit.target.clone();
                    body = body.child(
                        div()
                            .id(SharedString::from(format!("search-hit-{i}")))
                            .flex()
                            .flex_col()
                            .gap(theme::GAP_XS)
                            .p(theme::PAD_MD)
                            .rounded(theme::RADIUS_BTN)
                            .bg(theme::PANEL())
                            .cursor_pointer()
                            .hover(|s| s.bg(theme::ACCENT_SOFT()))
                            .on_click(cx.listener(move |_, _, _, cx| {
                                cx.emit(SearchEvent::Jump(target.clone()));
                            }))
                            .child(
                                div()
                                    .text_size(theme::FONT_BODY)
                                    .font_weight(FontWeight::MEDIUM)
                                    .text_color(theme::TEXT())
                                    .child(SharedString::from(hit.title.clone())),
                            )
                            .child(
                                div()
                                    .text_size(theme::FONT_META)
                                    .text_color(theme::DIM())
                                    .child(SharedString::from(hit.snippet.clone())),
                            ),
                    );
                }
            }
        }

        body
    }
}

impl BasePanel for SearchPanel {
    fn panel_name(&self) -> &'static str {
        "search-view"
    }
}

impl ComponentPanel for SearchPanel {
    fn tab_name(&self, _: &App) -> Option<SharedString> {
        Some("搜索".into())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn test_book(tag: &str) -> (PathBuf, ProjectHandle) {
        let base = std::env::temp_dir().join(format!("zhibi-search-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&base);
        let root = base.join("测试书.zhibi");
        std::fs::create_dir_all(root.join("chapters/ch-001")).unwrap();
        std::fs::create_dir_all(root.join("chapters/ch-002")).unwrap();
        std::fs::create_dir_all(root.join("canon")).unwrap();
        std::fs::create_dir_all(root.join("proposals")).unwrap();
        std::fs::write(
            root.join("project.json"),
            serde_json::to_string(&serde_json::json!({
                "title": "测试书", "createdAt": "2026-11-16T00:00:00Z",
                "targetChapters": 100, "chapterWordTarget": 3000
            })).unwrap(),
        ).unwrap();
        std::fs::write(root.join("chapters/ch-001/prose.md"), "沈砚按着断刀走进雨里。").unwrap();
        std::fs::write(root.join("chapters/ch-002/prose.md"), "顾青在灯下拆信。").unwrap();
        std::fs::write(root.join("canon/灯约.md"), "# 灯约\n\n每年上元夜，守灯人必须点亮全部一百零八盏灯。").unwrap();
        std::fs::write(
            root.join("characters.json"),
            serde_json::to_string(&serde_json::json!([
                { "name": "沈砚", "background": "镖局遗孤，沉默寡言" },
                { "name": "顾青", "background": "守灯人" }
            ])).unwrap(),
        ).unwrap();
        std::fs::write(
            root.join("clues.json"),
            serde_json::to_string(&serde_json::json!([
                { "id": "F01", "title": "断刀来历", "status": "埋下", "plantedChapter": 1 }
            ])).unwrap(),
        ).unwrap();
        std::fs::write(
            root.join("events.json"),
            serde_json::to_string(&serde_json::json!([
                { "id": "E01", "chapter": 1, "objectiveFact": "沈砚在雨夜拔出了断刀" }
            ])).unwrap(),
        ).unwrap();
        let handle = ProjectHandle::open(&root).unwrap();
        (root, handle)
    }

    #[test]
    fn search_covers_all_five_kinds() {
        let (root, handle) = test_book("five");
        let hits = search_book(&handle, "断刀");
        let kinds: std::collections::HashSet<_> = hits.iter().map(|h| h.kind).collect();
        assert!(kinds.contains(&HitKind::Chapter), "正文要命中");
        assert!(kinds.contains(&HitKind::Clue), "伏笔要命中");
        assert!(kinds.contains(&HitKind::Event), "事件要命中");
        assert_eq!(hits.len(), 3);
        // 组序：Chapter 在最前
        assert_eq!(hits[0].kind, HitKind::Chapter);
        assert_eq!(hits[0].target, HitTarget::Chapter(1));
        assert!(hits.iter().any(|h| h.target == HitTarget::Clue("F01".into())));
        assert!(hits.iter().any(|h| h.target == HitTarget::Event("E01".into())));
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn search_finds_canon_and_characters() {
        let (root, handle) = test_book("canon");
        let hits = search_book(&handle, "守灯人");
        assert!(hits.iter().any(|h| h.target == HitTarget::Canon("灯约".into())), "设定要定位到文档");
        assert!(hits.iter().any(|h| matches!(&h.target, HitTarget::Character(id) if !id.is_empty())), "人物要定位到稳定 id");
        let _ = std::fs::remove_dir_all(&root);
    }

    /// 搜索语料缓存必须在保存后失效，否则刚写的字搜不到。
    #[test]
    fn search_corpus_cache_follows_saves() {
        let (root, handle) = test_book("corpus");
        assert!(search_book(&handle, "断刀").iter().any(|h| h.kind == HitKind::Chapter));
        // 再搜一次（命中缓存路径）
        assert!(search_book(&handle, "断刀").iter().any(|h| h.kind == HitKind::Chapter));

        handle.save_prose(2, "顾青在灯下拆信，信里写着断刀的下落。").unwrap();
        let hits = search_book(&handle, "断刀");
        assert!(
            hits.iter().any(|h| h.kind == HitKind::Chapter && h.target == HitTarget::Chapter(2)),
            "新写入的正文必须立刻可搜到（缓存要随保存失效）"
        );
        let _ = std::fs::remove_dir_all(&root);
    }

    /// 语料缓存必须按书隔离：否则同一进程里打开第二本书会搜到第一本的正文。
    ///
    /// 这条是真实缺陷的回归——缓存曾是全局单例，全量并行测试时才暴露。
    #[test]
    fn corpus_cache_is_isolated_per_book() {
        let (root_a, a) = test_book("iso-a");
        let (root_b, b) = test_book("iso-b");
        // B 书没有"断刀"，但有它自己的独有词
        b.save_prose(1, "顾青在雪里数更漏。").unwrap();

        assert!(search_book(&a, "断刀").iter().any(|h| h.kind == HitKind::Chapter));
        let hits_b = search_book(&b, "断刀");
        assert!(
            !hits_b.iter().any(|h| matches!(h.target, HitTarget::Chapter(_))),
            "B 书搜到了 A 书的正文——语料缓存没有按书分键"
        );
        assert!(search_book(&b, "更漏").iter().any(|h| h.kind == HitKind::Chapter), "B 书自己的正文要能搜到");

        let _ = std::fs::remove_dir_all(&root_a);
        let _ = std::fs::remove_dir_all(&root_b);
    }

    #[test]
    fn search_book_does_what_it_says() {
        let (root, handle) = test_book("short");
        // search_book 本身不做长度过滤（那是 UI 层 run_search 的事）——
        // 单字照样搜，空串才不搜。这里钉住 search_book 的字面契约。
        assert!(!search_book(&handle, "断").is_empty(), "单字也能搜出东西（UI 层才做 <2 过滤）");
        assert!(search_book(&handle, "").is_empty(), "空串不搜");
        let _ = std::fs::remove_dir_all(&root);
    }
}
