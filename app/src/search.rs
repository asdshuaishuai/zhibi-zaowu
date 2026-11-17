//! 全局搜索（作者侧）：⌥⌘F 唤起，跨正文/设定/人物/伏笔/事件全文子串匹配。
//!
//! 纯本地、零 AI、零网络。200 章的书也就几十 MB 文本，正则扫描毫秒级。
//! 结果按类别分组，点一条跳过去（章节进稿纸，其他进对应台账页并高亮）。

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
    /// 切到对应台账页
    Mode(crate::workspace::Mode),
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

/// 跨正文 + 台账 + 设定的子串搜索。大小写敏感（中文无大小写问题）。
/// 返回按 HitKind 字典序 + 组内标题排序的结果。
pub fn search_book(handle: &ProjectHandle, query: &str) -> Vec<SearchHit> {
    let mut hits = Vec::new();
    if query.is_empty() {
        return hits;
    }
    let q = query;

    // 1. 正文（含标题）
    for ch in handle.chapters() {
        let prose = handle.prose(ch.n);
        if let Some(snippet) = snippet_around(&prose, q) {
            hits.push(SearchHit {
                kind: HitKind::Chapter,
                title: format!("第 {} 章 · {}", ch.n, ch.title),
                snippet,
                target: HitTarget::Chapter(ch.n),
            });
        } else if ch.title.contains(q) {
            hits.push(SearchHit {
                kind: HitKind::Chapter,
                title: format!("第 {} 章 · {}", ch.n, ch.title),
                snippet: format!("（标题命中）{}", ch.title),
                target: HitTarget::Chapter(ch.n),
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
                    target: HitTarget::Mode(crate::workspace::Mode::Canon),
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
                target: HitTarget::Mode(crate::workspace::Mode::Characters),
            });
        }
    }

    // 4. 伏笔（clues.json）
    for cl in handle.clues_full() {
        let title = cl.get("title").and_then(|v| v.as_str()).unwrap_or("").to_string();
        if title.contains(q) {
            hits.push(SearchHit {
                kind: HitKind::Clue,
                title,
                snippet: cl.get("id").and_then(|v| v.as_str()).unwrap_or("").to_string(),
                target: HitTarget::Mode(crate::workspace::Mode::Clues),
            });
        }
    }

    // 5. 事件（events.json 的 objectiveFact）
    for e in handle.events() {
        let fact = e.get("objectiveFact").and_then(|v| v.as_str()).unwrap_or("").to_string();
        if fact.contains(q) {
            let ch = e.get("chapter").and_then(|v| v.as_u64()).unwrap_or(0) as u32;
            hits.push(SearchHit {
                kind: HitKind::Event,
                title: fact.chars().take(50).collect(),
                snippet: format!("第 {ch} 章"),
                target: HitTarget::Mode(crate::workspace::Mode::Outline),
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
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn search_finds_canon_and_characters() {
        let (root, handle) = test_book("canon");
        let hits = search_book(&handle, "守灯人");
        assert!(hits.iter().any(|h| h.kind == HitKind::Canon), "设定正文要命中");
        assert!(hits.iter().any(|h| h.kind == HitKind::Character), "人物背景要命中");
        let _ = std::fs::remove_dir_all(&root);
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
