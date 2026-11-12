//! 世界线页：剧情的时间轴 + if 分支。
//!
//! 这一页回答一个问题：**"故事确定到哪儿了，之后有哪些可能？"**
//!
//! 与另外两页的分工（这三页最容易混，写在这里当下锚点）：
//! - **世界线（本页）**：时间轴。横轴 = 章节。已固化的剧情是实线，
//!   之后的开放段是虚线；每条 if 线从它的分叉章长出。回答"有哪些世界、从哪分叉"。
//! - **画布**：关系网。节点 = 人物/设定/道具/事件，边 = 人物关系与剧情关联。
//!   回答"这个关节跟谁有关"。via 双向弱引用可跳到本页的具体章节。
//! - **预言书**：if 工坊。选中一个固化点，在这里写假设、让 AI 推演、
//!   看影响清单、把某条 if 扶正。回答"这条假设要不要收进正典"。
//!
//! 为什么拆出来：旧的实现把这三件事全塞进 `OraclePanel`（"预言书"），
//! 结果"世界线"这个词在侧栏出现两次、而真正的世界线没有自己的页面。

use std::collections::BTreeMap;
use std::rc::Rc;

use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::dock::{BasePanel, Panel as ComponentPanel, PanelEvent};
use gpui_kit::component::Sizable;
use gpui_kit::prelude::FluentBuilder;
use gpui_kit::{
    App, Context, EventEmitter, FocusHandle, Focusable, FontWeight, InteractiveElement,
    IntoElement, ParentElement, PathBuilder, Render, SharedString, StatefulInteractiveElement,
    Styled, Window, canvas, div, point, px, relative,
};

use crate::store::ProjectHandle;

/// Rgba → Hsla（画布路径与 Tag 需要 Hsla）。
fn hsla(c: gpui_kit::Rgba) -> gpui_kit::Hsla {
    c.into()
}
use crate::theme;
use crate::worldline::layout::{bands, forks_at, segments, tracks, Band, Track};
use crate::worldline::{Kind, Line, Store};

/// 世界线页的事件。写操作全部交给宿主，页面不碰磁盘。
#[derive(Debug, Clone)]
pub enum WorldlineEvent {
    /// 选中某条线（联动右侧详情与预言书）
    Select(String),
    /// 从某条线的第 N 章分叉
    Fork { from: String, chapter: u32 },
    /// 把某条 if 线扶正为正典后续
    Fix(String),
    /// 撤销扶正
    Unfix(String),
    /// 归档 / 恢复
    Archive(String),
    /// 删除
    Remove(String),
    /// 跳到预言书，针对某条线推演
    Simulate(String),
    /// 改一个节点的字段（章号 / 标题 / 状态）
    NodePatch {
        line: String,
        node: String,
        patch: crate::worldline::NodePatch,
    },
    /// 删一个节点
    NodeRemove { line: String, node: String },
    /// 把世界线节点挂到画布节点（双向弱引用的一半）
    LinkCanvas {
        line: String,
        node: String,
        canvas: String,
    },
    /// 新增一个节点
    NodeAdd {
        line: String,
        kind: String,
        label: String,
        chapter: u32,
    },
}

pub struct WorldlinePanel {
    focus_handle: FocusHandle,
    handle: Rc<ProjectHandle>,
    selected: Option<String>,
    /// 时间轴的可见章节范围（None = 自动：0 到 max(边界, 最后节点) + 余量）
    window: Option<(u32, u32)>,
    message: Option<SharedString>,
    /// 「时间轴 / 对比」两个看法。时间轴回答"有哪些世界、从哪分叉"，
    /// 对比回答"两条线具体差在哪"。两者都属于世界线，不该分页。
    compare_with: Option<String>,
}

impl WorldlinePanel {
    pub fn new(cx: &mut Context<Self>, handle: Rc<ProjectHandle>) -> Self {
        Self {
            focus_handle: cx.focus_handle(),
            handle,
            selected: None,
            window: None,
            message: None,
            compare_with: None,
        }
    }

    pub fn set_message(&mut self, msg: impl Into<SharedString>, cx: &mut Context<Self>) {
        self.message = Some(msg.into());
        cx.notify();
    }

    pub fn select(&mut self, id: String, cx: &mut Context<Self>) {
        self.selected = Some(id);
        cx.notify();
    }

    /// 读全部线，坏文件当空（界面不能因为一份坏 JSON 整个白屏）。
    fn lines(&self) -> Vec<Line> {
        Store::new(&self.handle.root).all().unwrap_or_default()
    }

    fn stage_bands(&self) -> Vec<Band> {
        let raw: Vec<(String, String, u32, u32)> = self
            .handle
            .stages()
            .iter()
            .map(|st| {
                (
                    s(st, "name"),
                    s(st, "theme"),
                    st.get("chapterStart").and_then(|v| v.as_u64()).unwrap_or(0) as u32,
                    st.get("chapterEnd").and_then(|v| v.as_u64()).unwrap_or(0) as u32,
                )
            })
            .collect();
        bands(&raw)
    }

    /// 最新成稿章：作者"已经写到哪"。
    fn written_through(&self) -> u32 {
        self.handle
            .chapters()
            .iter()
            .filter(|c| c.has_prose)
            .map(|c| c.n)
            .max()
            .unwrap_or(0)
    }
}

fn s(v: &serde_json::Value, k: &str) -> String {
    v.get(k).and_then(|x| x.as_str()).unwrap_or("").to_string()
}

impl EventEmitter<PanelEvent> for WorldlinePanel {}
impl EventEmitter<WorldlineEvent> for WorldlinePanel {}

impl Focusable for WorldlinePanel {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus_handle.clone()
    }
}

impl Render for WorldlinePanel {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let lines = self.lines();
        let store = Store::new(&self.handle.root);
        let boundary = store.fixed_boundary(&lines);
        let written = self.written_through();

        // 视野：从 0 到「边界与最远节点取大」再留 6 章余量——作者要看见"前面还有路"
        let horizon = lines
            .iter()
            .map(|l| l.last_chapter())
            .max()
            .unwrap_or(0)
            .max(boundary)
            .max(written)
            + 6;
        let (from_ch, to_ch) = self.window.unwrap_or((0, horizon.max(12)));
        let span = (to_ch.saturating_sub(from_ch)).max(1);

        let tr = tracks(&lines, boundary);
        let segs = segments(&tr, to_ch);
        let band_list = self.stage_bands();

        // —— 左：轨道列表 ——
        let mut track_rows: Vec<gpui_kit::AnyElement> = Vec::new();
        for t in &tr {
            let on = self.selected.as_deref() == Some(t.id.as_str());
            let color = track_color(t);
            let id = t.id.clone();
            let pick = t.id.clone();
            track_rows.push(
                div()
                    .id(SharedString::from(format!("wltr-{}", t.id)))
                    .h(px(30.))
                    .flex()
                    .items_center()
                    .gap(px(4.))
                    .cursor_pointer()
                    .when(on, |d| d.bg(theme::ACCENT_SOFT()))
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.selected = Some(pick.clone());
                        cx.emit(WorldlineEvent::Select(pick.clone()));
                        cx.notify();
                    }))
                    .child(
                        div()
                            .w(px(28.))
                            .flex_shrink_0()
                            .font_family(theme::FAMILY_MONO)
                            .text_size(theme::FONT_MICRO)
                            .text_color(color)
                            .child(SharedString::from(id)),
                    )
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .text_ellipsis()
                            .text_size(theme::FONT_MICRO)
                            .text_color(if on { theme::TEXT() } else { theme::DIM() })
                            .child(SharedString::from(t.name.clone())),
                    )
                    .child(
                        div()
                            .text_size(px(9.))
                            .text_color(theme::FAINT())
                            .child(t.kind.label()),
                    )
                    .into_any_element(),
            );
        }

        // —— 右：时间轴画布 ——
        let segs_c = segs.clone();
        let bands_c = band_list.clone();
        let tr_c = tr.clone();
        let nodes_by_track: BTreeMap<String, Vec<(u32, String, bool)>> = lines
            .iter()
            .map(|l| {
                let mut ns: Vec<(u32, String, bool)> = l
                    .nodes
                    .iter()
                    .map(|n| {
                        (
                            n.chapter_or_guess(),
                            n.label.clone(),
                            n.state == crate::worldline::NodeState::Written,
                        )
                    })
                    .filter(|(ch, _, _)| *ch > 0)
                    .collect();
                ns.sort_by_key(|(ch, _, _)| *ch);
                (l.id.clone(), ns)
            })
            .collect();
        let nodes_c = nodes_by_track.clone();
        let sel = self.selected.clone();
        let row_h = 30.;
        let head_h = 48.;
        let canvas_h = head_h + tr_c.len().max(1) as f32 * row_h + 12.;

        let timeline = div()
            .relative()
            .flex_1()
            .min_w_0()
            .h(px(canvas_h))
            .child(
                canvas(
                    |_, _, _| {},
                    move |bounds, _, window, _| {
                        let w = bounds.size.width.as_f32();
                        let h = bounds.size.height.as_f32();
                        let ox = bounds.origin.x.as_f32();
                        let oy = bounds.origin.y.as_f32();
                        let p = |x: f32, y: f32| point(px(ox + x), px(oy + y));
                        let pad_r = 13.0;
                        let track_w = (w - pad_r).max(1.);
                        let x_of = |ch: u32| {
                            let rel = (ch.saturating_sub(from_ch)) as f32 / span as f32;
                            rel.clamp(0., 1.) * track_w
                        };
                        let y_of = |i: usize| head_h + row_h * i as f32;
                        let purple = theme::PURPLE();

                        // 幕带
                        for (bi, b) in bands_c.iter().enumerate() {
                            let x0 = x_of(b.start.saturating_sub(1));
                            let x1 = x_of(b.end);
                            if x1 <= x0 {
                                continue;
                            }
                            let mut r = PathBuilder::fill();
                            r.move_to(p(x0, 0.));
                            r.line_to(p(x1, 0.));
                            r.line_to(p(x1, h));
                            r.line_to(p(x0, h));
                            r.close();
                            if let Ok(path) = r.build() {
                                let a = if bi % 2 == 0 { 0.05 } else { 0.022 };
                                window.paint_path(path, hsla(purple).opacity(a));
                            }
                        }

                        // 章节刻度
                        for ch in (from_ch..=to_ch).step_by(5) {
                            if ch == 0 {
                                continue;
                            }
                            let x = x_of(ch);
                            let mut t = PathBuilder::stroke(px(1.));
                            t.move_to(p(x, head_h - 10.));
                            t.line_to(p(x, h - 2.));
                            if let Ok(path) = t.build() {
                                window.paint_path(path, hsla(theme::LINE()).opacity(0.6));
                            }
                        }

                        // 固化边界：整条竖线，最粗最实——这是这一页的主线索
                        if boundary > 0 {
                            let x = x_of(boundary);
                            let mut b1 = PathBuilder::stroke(px(2.));
                            b1.move_to(p(x, head_h - 14.));
                            b1.line_to(p(x, h - 2.));
                            if let Ok(path) = b1.build() {
                                window.paint_path(path, hsla(theme::ACCENT()).opacity(0.85));
                            }
                        }

                        // 各轨道的段
                        for (si, seg) in segs_c.iter().enumerate() {
                            let Some(ti) = tr_c.iter().position(|t| t.id == seg.track) else {
                                continue;
                            };
                            let y = y_of(ti);
                            let x0 = x_of(seg.from);
                            let x1 = x_of(seg.to);
                            if x1 <= x0 {
                                continue;
                            }
                            let color = track_color(&tr_c[ti]);
                            let on = sel.as_deref() == Some(seg.track.as_str());
                            let mut line_p = PathBuilder::stroke(px(if on {
                                2.4
                            } else if seg.fixed {
                                2.0
                            } else {
                                1.4
                            }));
                            if !seg.fixed {
                                line_p = line_p.dash_array(&[px(5.), px(4.)]);
                            }
                            line_p.move_to(p(x0, y));
                            line_p.line_to(p(x1, y));
                            if let Ok(path) = line_p.build() {
                                // 固化段实、开放段淡：一眼分出"已定"与"未定"
                                let op = if seg.fixed { 1.0 } else { 0.55 };
                                window.paint_path(path, hsla(color).opacity(op));
                            }
                            let _ = si;
                        }

                        // if 线的分叉曲线：从父轨道的分叉点弯到自己的轨道
                        for (ti, t) in tr_c.iter().enumerate() {
                            if t.parent_id.is_empty() || t.fork_chapter == 0 {
                                continue;
                            }
                            let Some(pi) = tr_c.iter().position(|x| x.id == t.parent_id) else {
                                continue;
                            };
                            let x = x_of(t.fork_chapter);
                            let color = track_color(t);
                            let mut c = PathBuilder::stroke(px(1.5));
                            c.move_to(p(x, y_of(pi)));
                            c.cubic_bezier_to(
                                p(x + 8., y_of(pi)),
                                p(x + 8., y_of(ti)),
                                p(x + 18., y_of(ti)),
                            );
                            if let Ok(path) = c.build() {
                                window.paint_path(path, hsla(color).opacity(0.9));
                            }
                        }

                        // 节点：实心 = 已写进正文，空心 = 仅计划/推演
                        for (ti, t) in tr_c.iter().enumerate() {
                            let y = y_of(ti);
                            let Some(ns) = nodes_c.get(&t.id) else { continue };
                            for (ch, _, written) in ns {
                                if *ch < from_ch || *ch > to_ch {
                                    continue;
                                }
                                let x = x_of(*ch);
                                let r = 3.2;
                                let k = 0.5523 * r;
                                if *written {
                                    let mut d = PathBuilder::fill();
                                    d.move_to(p(x + r, y));
                                    d.cubic_bezier_to(p(x + r, y + k), p(x + k, y + r), p(x, y + r));
                                    d.cubic_bezier_to(p(x - k, y + r), p(x - r, y + k), p(x - r, y));
                                    d.cubic_bezier_to(p(x - r, y - k), p(x - k, y - r), p(x, y - r));
                                    d.cubic_bezier_to(p(x + k, y - r), p(x + r, y - k), p(x + r, y));
                                    if let Ok(path) = d.build() {
                                        window.paint_path(path, hsla(theme::GREEN()));
                                    }
                                } else {
                                    let mut d = PathBuilder::stroke(px(1.1));
                                    d.move_to(p(x + r, y));
                                    d.cubic_bezier_to(p(x + r, y + k), p(x + k, y + r), p(x, y + r));
                                    d.cubic_bezier_to(p(x - k, y + r), p(x - r, y + k), p(x - r, y));
                                    d.cubic_bezier_to(p(x - r, y - k), p(x - k, y - r), p(x, y - r));
                                    d.cubic_bezier_to(p(x + k, y - r), p(x + r, y - k), p(x + r, y));
                                    if let Ok(path) = d.build() {
                                        window.paint_path(path, hsla(theme::FAINT()).opacity(0.8));
                                    }
                                }
                            }
                        }
                    },
                )
                .absolute()
                .size_full(),
            )
            // 幕名
            .child(
                div()
                    .absolute()
                    .left_0()
                    .right(px(13.))
                    .top(px(6.))
                    .h(px(16.))
                    .relative()
                    .children(band_list.iter().map(|b| {
                        let start = (b.start.saturating_sub(from_ch)) as f32 / span as f32;
                        let width = ((b.end.saturating_sub(b.start) + 1) as f32 / span as f32)
                            .clamp(0.03, 1.);
                        div()
                            .absolute()
                            .left(relative(start))
                            .w(relative(width))
                            .overflow_hidden()
                            .whitespace_nowrap()
                            .text_size(theme::FONT_MICRO)
                            .text_color(theme::PURPLE())
                            .child(SharedString::from(b.name.clone()))
                    })),
            )
            // 章号 + 固化边界标注
            .child(
                div()
                    .absolute()
                    .left_0()
                    .right(px(13.))
                    .top(px(24.))
                    .h(px(16.))
                    .relative()
                    .child(
                        div()
                            .absolute()
                            .left(relative(
                                (boundary.saturating_sub(from_ch)) as f32 / span as f32,
                            ))
                            .text_size(theme::FONT_MICRO)
                            .font_family(theme::FAMILY_MONO)
                            .text_color(theme::ACCENT())
                            .child(SharedString::from(if boundary > 0 {
                                format!("固化至 {boundary}")
                            } else {
                                "尚未固化".to_string()
                            })),
                    ),
            )
            .child(
                div()
                    .absolute()
                    .right(px(16.))
                    .bottom(px(2.))
                    .text_size(theme::FONT_MICRO)
                    .text_color(theme::FAINT())
                    .child(SharedString::from(format!(
                        "第 {from_ch}–{to_ch} 章 · 实线=已固化 虚线=开放的 if · ● 已写 ○ 计划"
                    ))),
            );

        // —— 右侧详情：选中线的操作 ——
        let mut detail = div().flex().flex_col().gap(theme::GAP_SM);
        let sel_line = self
            .selected
            .as_ref()
            .and_then(|id| lines.iter().find(|l| &l.id == id))
            .cloned();
        match &sel_line {
            Some(l) => {
                let kind_note = match l.kind {
                    Kind::Base => "正典：已经写进正文的那条线。它的固化边界就是「剧情确定到哪儿」。",
                    Kind::Fixed => "已扶正：作者从 if 里选定、正式收进正典后续的那一版。",
                    Kind::Forecast => "预测线：按大纲/成稿往前推演「接下来会怎样」。还没定。",
                    Kind::WhatIf => "假设线：如果某件事不一样会怎样。还没定。",
                };
                let forks_here = forks_at(&lines, l.fork_chapter());
                let lid = l.id.clone();
                let l2 = l.id.clone();
                let l3 = l.id.clone();
                let l4 = l.id.clone();
                let l5 = l.id.clone();
                detail = detail
                    .child(
                        div()
                            .text_size(theme::FONT_LABEL)
                            .font_weight(FontWeight::SEMIBOLD)
                            .text_color(theme::TEXT())
                            .child(SharedString::from(format!("{} · {}", l.id, l.name))),
                    )
                    .child(
                        div()
                            .text_size(theme::FONT_MICRO)
                            .line_height(relative(1.45))
                            .text_color(theme::DIM())
                            .child(SharedString::from(kind_note)),
                    )
                    .child(
                        div()
                            .text_size(theme::FONT_MICRO)
                            .font_family(theme::FAMILY_MONO)
                            .text_color(theme::FAINT())
                            .child(SharedString::from(format!(
                                "分叉于第{}章 · 节点 {} · 未晋升 {}",
                                l.fork_chapter(),
                                l.nodes.len(),
                                l.pending_diffs()
                            ))),
                    )
                    .child(
                        div()
                            .text_size(theme::FONT_MICRO)
                            .text_color(theme::FAINT())
                            .child(SharedString::from(format!(
                                "同一个分叉点上共有 {} 条线",
                                forks_here.len()
                            ))),
                    )
                    .child(
                        div()
                            .flex()
                            .flex_wrap()
                            .gap(theme::GAP_SM)
                            .child(
                                Button::new("wl-sim")
                                    .xsmall()
                                    .label("在预言书推演")
                                    .on_click(cx.listener(move |_this, _, _, cx| {
                                        cx.emit(WorldlineEvent::Simulate(lid.clone()));
                                    })),
                            )
                            .when(!l.is_base(), |d| {
                                d.child(
                                    Button::new("wl-fork")
                                        .xsmall()
                                        .label("从此处再分叉")
                                        .on_click(cx.listener(move |_this, _, _, cx| {
                                            cx.emit(WorldlineEvent::Fork {
                                                from: l2.clone(),
                                                chapter: 0,
                                            });
                                        })),
                                )
                            })
                            .when(!l.is_base() && l.kind != Kind::Fixed, |d| {
                                d.child(
                                    Button::new("wl-fix")
                                        .primary()
                                        .xsmall()
                                        .label("扶正为正典后续")
                                        .on_click(cx.listener(move |_this, _, _, cx| {
                                            cx.emit(WorldlineEvent::Fix(l3.clone()));
                                        })),
                                )
                            })
                            .when(l.kind == Kind::Fixed, |d| {
                                d.child(
                                    Button::new("wl-unfix")
                                        .xsmall()
                                        .label("撤销扶正")
                                        .on_click(cx.listener(move |_this, _, _, cx| {
                                            cx.emit(WorldlineEvent::Unfix(l4.clone()));
                                        })),
                                )
                            })
                            .when(!l.is_base(), |d| {
                                d.child(
                                    Button::new("wl-arch")
                                        .xsmall()
                                        .label(if l.is_archived() { "恢复" } else { "归档" })
                                        .on_click(cx.listener(move |_this, _, _, cx| {
                                            cx.emit(WorldlineEvent::Archive(l5.clone()));
                                        })),
                                )
                            })
                            .when(!l.is_base(), |d| {
                                // 删线：归档是"封存可恢复"，删是"真的不要了"。
                                // 子线不删，只断父指针（Store::remove 负责）。
                                let l6 = l.id.clone();
                                d.child(
                                    Button::new("wl-del")
                                        .xsmall()
                                        .label("删除")
                                        .on_click(cx.listener(move |_this, _, _, cx| {
                                            cx.emit(WorldlineEvent::Remove(l6.clone()));
                                        })),
                                )
                            }),
                    )
                    // 节点清单：时间轴上的关节，作者可以改章、删、加
                    .child(
                        div()
                            .pt(theme::GAP_SM)
                            .text_size(theme::FONT_MICRO)
                            .font_weight(FontWeight::SEMIBOLD)
                            .text_color(theme::DIM())
                            .child(SharedString::from(format!("节点 · {}", l.nodes.len()))),
                    )
                    .children({
                        let mut nodes = l.nodes.clone();
                        nodes.sort_by_key(|n| n.chapter_or_guess());
                        let lid = l.id.clone();
                        let next_ch = nodes
                            .iter()
                            .map(|n| n.chapter_or_guess())
                            .max()
                            .unwrap_or(boundary)
                            + 1;
                        let mut rows: Vec<gpui_kit::AnyElement> = nodes
                            .iter()
                            .take(20)
                            .map(|n| {
                                let ch = n.chapter_or_guess();
                                let nid = n.id.clone();
                                let l1 = lid.clone();
                                let l2 = lid.clone();
                                let n1 = nid.clone();
                                let n2 = nid.clone();
                                div()
                                    .flex()
                                    .items_center()
                                    .gap(px(3.))
                                    .py(px(1.))
                                    .child(
                                        // 点章号 +1：改排章是最常做的动作
                                        div()
                                            .id(SharedString::from(format!("wn-ch-{lid}-{nid}")))
                                            .w(px(42.))
                                            .flex_shrink_0()
                                            .cursor_pointer()
                                            .font_family(theme::FAMILY_MONO)
                                            .text_size(theme::FONT_MICRO)
                                            .text_color(theme::FAINT())
                                            .child(SharedString::from(if ch > 0 {
                                                format!("第{ch}章")
                                            } else {
                                                "排章".into()
                                            }))
                                            .on_click(cx.listener(move |_t, _, _, cx| {
                                                cx.emit(WorldlineEvent::NodePatch {
                                                    line: l1.clone(),
                                                    node: n1.clone(),
                                                    patch: crate::worldline::NodePatch::chapter(ch + 1),
                                                });
                                            })),
                                    )
                                    .child(
                                        div()
                                            .flex_1()
                                            .min_w_0()
                                            .text_ellipsis()
                                            .text_size(theme::FONT_MICRO)
                                            .text_color(theme::DIM())
                                            .child(SharedString::from(n.label.clone())),
                                    )
                                    .child(
                                        // 已写进正文 / 仅计划：这个区别决定它算不算"已定剧情"
                                        div()
                                            .flex_shrink_0()
                                            .text_size(px(9.))
                                            .text_color(if n.state
                                                == crate::worldline::NodeState::Written
                                            {
                                                theme::GREEN()
                                            } else {
                                                theme::FAINT()
                                            })
                                            .child(n.state.label()),
                                    )
                                    .child(
                                        // 双向弱引用：把这个时间轴关节挂到画布上的某个节点，
                                        // 之后在画布点那个人物就能反查"他在哪几条线的哪一章"。
                                        div()
                                            .id(SharedString::from(format!("wn-link-{lid}-{nid}")))
                                            .flex_shrink_0()
                                            .cursor_pointer()
                                            .px(px(2.))
                                            .text_size(theme::FONT_MICRO)
                                            .text_color(if n.canvas_ref.is_empty() {
                                                theme::FAINT()
                                            } else {
                                                theme::BLUE()
                                            })
                                            .child(SharedString::from(if n.canvas_ref.is_empty() {
                                                "关联"
                                            } else {
                                                "已关联"
                                            }))
                                            .on_click(cx.listener({
                                                let l3 = lid.clone();
                                                let n3 = nid.clone();
                                                let cur = n.canvas_ref.clone();
                                                move |_t, _, _, cx| {
                                                    // 就地点一下：有引用就清掉，没有就用自己的
                                                    // id 当默认（最常见的场景是两边同名同 id）。
                                                    cx.emit(WorldlineEvent::LinkCanvas {
                                                        line: l3.clone(),
                                                        node: n3.clone(),
                                                        canvas: if cur.is_empty() {
                                                            n3.clone()
                                                        } else {
                                                            String::new()
                                                        },
                                                    });
                                                }
                                            })),
                                    )
                                    .child(
                                        div()
                                            .id(SharedString::from(format!("wn-del-{lid}-{nid}")))
                                            .flex_shrink_0()
                                            .cursor_pointer()
                                            .text_size(theme::FONT_MICRO)
                                            .text_color(theme::FAINT())
                                            .child("×")
                                            .on_click(cx.listener(move |_t, _, _, cx| {
                                                cx.emit(WorldlineEvent::NodeRemove {
                                                    line: l2.clone(),
                                                    node: n2.clone(),
                                                });
                                            })),
                                    )
                                    .into_any_element()
                            })
                            .collect();
                        rows.push(
                            div()
                                .id("wn-add")
                                .cursor_pointer()
                                .pt(px(2.))
                                .text_size(theme::FONT_MICRO)
                                .text_color(theme::ACCENT())
                                .child("＋ 加节点")
                                .on_click(cx.listener(move |_t, _, _, cx| {
                                    cx.emit(WorldlineEvent::NodeAdd {
                                        line: lid.clone(),
                                        kind: "event".into(),
                                        label: "新节点".into(),
                                        chapter: next_ch,
                                    });
                                }))
                                .into_any_element(),
                        );
                        rows
                    });
            }
            None => {
                detail = detail.child(
                    div()
                        .text_size(theme::FONT_MICRO)
                        .text_color(theme::FAINT())
                        .child("选一条线看详情。实线是已经定下的剧情，虚线是后面还有的 if 空间。"),
                );
            }
        }

        // 对比视图：选两条线，看共同祖先之后各自差在哪。
        // 对比属于世界线的第二种看法（时间轴回答"有哪些世界"，对比回答"差在哪"），
        // 所以不另开页，用页头的两个小页签切。
        let compare_el: Option<gpui_kit::AnyElement> = self.compare_with.as_ref().map(|right| {
            let left = self
                .selected
                .clone()
                .or_else(|| lines.iter().find(|l| !l.is_base()).map(|l| l.id.clone()))
                .unwrap_or_else(|| "α".into());
            let cmp = crate::worldline::layout::compare(&lines, &left, right);
            let pickable = lines.clone();
            let right_id = right.clone();
            let seg2 = |label: String, on: bool, key: String| {
                div()
                    .id(SharedString::from(key))
                    .px(px(7.))
                    .py(px(2.))
                    .rounded(px(5.))
                    .border_1()
                    .border_color(if on { theme::ACCENT_BORDER() } else { theme::LINE() })
                    .bg(if on { theme::ACCENT_SOFT() } else { theme::PANEL2() })
                    .cursor_pointer()
                    .text_size(theme::FONT_MICRO)
                    .text_color(if on { theme::ACCENT() } else { theme::DIM() })
                    .child(SharedString::from(label))
            };
            let mut cols = div().flex().gap(theme::GAP_GROUP);
            for (title, list, color) in [
                (format!("{} 独有", cmp.a), cmp.only_a.clone(), theme::ACCENT()),
                (format!("{} 独有", cmp.b), cmp.only_b.clone(), theme::BLUE()),
            ] {
                let mut col = div().flex_1().min_w_0().flex().flex_col().gap(px(2.)).child(
                    div()
                        .text_size(theme::FONT_LABEL)
                        .font_weight(FontWeight::SEMIBOLD)
                        .text_color(color)
                        .child(SharedString::from(format!("{title} {}", list.len()))),
                );
                for (label, ch) in list {
                    col = col.child(
                        div()
                            .flex()
                            .gap(theme::GAP_SM)
                            .child(
                                div()
                                    .w(px(40.))
                                    .flex_shrink_0()
                                    .font_family(theme::FAMILY_MONO)
                                    .text_size(theme::FONT_MICRO)
                                    .text_color(theme::FAINT())
                                    .child(SharedString::from(format!("第{ch}章"))),
                            )
                            .child(
                                div()
                                    .text_size(theme::FONT_MICRO)
                                    .text_color(theme::TEXT())
                                    .child(SharedString::from(label)),
                            ),
                    );
                }
                cols = cols.child(col);
            }
            let mut conflict_el = div().flex().flex_col().gap(px(2.));
            if cmp.conflicts.is_empty() {
                conflict_el = conflict_el.child(
                    div()
                        .text_size(theme::FONT_MICRO)
                        .text_color(theme::FAINT())
                        .child("两条线对同一目标的影响方式一致，没有冲突。"),
                );
            }
            for c in &cmp.conflicts {
                conflict_el = conflict_el.child(
                    div()
                        .text_size(theme::FONT_MICRO)
                        .text_color(theme::AMBER())
                        .child(SharedString::from(format!("⚠ {c}"))),
                );
            }
            div()
                .flex_1()
                .min_h_0()
                .flex()
                .flex_col()
                .gap(theme::GAP_MD)
                .p(theme::PAD_MD)
                .child(
                    div()
                        .flex()
                        .items_center()
                        .flex_wrap()
                        .gap(px(4.))
                        .child(
                            div()
                                .text_size(theme::FONT_MICRO)
                                .text_color(theme::FAINT())
                                .child("左"),
                        )
                        .children(pickable.iter().map(|l| {
                            let id = l.id.clone();
                            let on = id == left;
                            seg2(id.clone(), on, format!("cmp-a-{id}")).on_click(cx.listener(
                                move |this, _, _, cx| {
                                    this.selected = Some(id.clone());
                                    cx.notify();
                                },
                            ))
                        }))
                        .child(
                            div()
                                .px(px(5.))
                                .text_size(theme::FONT_MICRO)
                                .text_color(theme::FAINT())
                                .child("vs"),
                        )
                        .children(pickable.iter().map(|l| {
                            let id = l.id.clone();
                            let on = id == right_id;
                            seg2(id.clone(), on, format!("cmp-b-{id}")).on_click(cx.listener(
                                move |this, _, _, cx| {
                                    this.compare_with = Some(id.clone());
                                    cx.notify();
                                },
                            ))
                        })),
                )
                .child(
                    div()
                        .text_size(theme::FONT_MICRO)
                        .text_color(theme::FAINT())
                        .child(SharedString::from(if cmp.common_ancestor.is_empty() {
                            "两条线没有共同祖先：各自推演的全部节点都算差异".to_string()
                        } else {
                            format!("共同祖先：{}（只比较它之后的推演）", cmp.common_ancestor)
                        })),
                )
                .child(cols)
                .child(
                    div()
                        .flex()
                        .flex_col()
                        .gap(px(3.))
                        .border_t_1()
                        .border_color(theme::LINE())
                        .pt(theme::GAP_SM)
                        .child(
                            div()
                                .text_size(theme::FONT_LABEL)
                                .font_weight(FontWeight::SEMIBOLD)
                                .text_color(theme::TEXT())
                                .child(SharedString::from(format!(
                                    "共同推演 {} 件 · 冲突 {} 处",
                                    cmp.shared.len(),
                                    cmp.conflicts.len()
                                ))),
                        )
                        .child(conflict_el),
                )
                .into_any_element()
        });

        div()
            .size_full()
            .flex()
            .flex_col()
            .bg(theme::BG())
            .child(
                div()
                    .flex_shrink_0()
                    .px(theme::PAD_LG)
                    .pt(theme::PAD_MD)
                    .flex()
                    .items_baseline()
                    .gap(theme::GAP_MD)
                    .child(
                        div()
                            .text_size(theme::FONT_META)
                            .font_weight(FontWeight::SEMIBOLD)
                            .text_color(theme::TEXT())
                            .child("世界线"),
                    )
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .text_size(theme::FONT_MICRO)
                            .text_color(theme::FAINT())
                            .child(SharedString::from({
                                let here = store.lines_at(&lines, boundary.max(1)).len();
                                format!(
                                    "剧情确定到第 {boundary} 章（成稿 {written} 章）；此后开放的 if，                                     当前位置有 {here} 条线在跑"
                                )
                            })),
                    )
                    .when_some(self.message.clone(), |d, m| {
                        d.child(
                            div()
                                .text_size(theme::FONT_MICRO)
                                .text_color(theme::GREEN())
                                .child(m),
                        )
                    })
                    // 时间轴 / 对比：都属于"世界线"这一页的两种看法
                    .child(
                        div()
                            .flex()
                            .gap(px(4.))
                            .child(
                                div()
                                    .id("wl-tab-time")
                                    .px(px(9.))
                                    .py(px(3.))
                                    .rounded(px(6.))
                                    .border_1()
                                    .border_color(if self.compare_with.is_none() {
                                        theme::ACCENT_BORDER()
                                    } else {
                                        theme::LINE()
                                    })
                                    .bg(if self.compare_with.is_none() {
                                        theme::ACCENT_SOFT()
                                    } else {
                                        theme::PANEL2()
                                    })
                                    .cursor_pointer()
                                    .text_size(theme::FONT_MICRO)
                                    .text_color(if self.compare_with.is_none() {
                                        theme::ACCENT()
                                    } else {
                                        theme::DIM()
                                    })
                                    .child("时间轴")
                                    .on_click(cx.listener(|this, _, _, cx| {
                                        this.compare_with = None;
                                        cx.notify();
                                    })),
                            )
                            .child(
                                div()
                                    .id("wl-tab-cmp")
                                    .px(px(9.))
                                    .py(px(3.))
                                    .rounded(px(6.))
                                    .border_1()
                                    .border_color(if self.compare_with.is_some() {
                                        theme::ACCENT_BORDER()
                                    } else {
                                        theme::LINE()
                                    })
                                    .bg(if self.compare_with.is_some() {
                                        theme::ACCENT_SOFT()
                                    } else {
                                        theme::PANEL2()
                                    })
                                    .cursor_pointer()
                                    .text_size(theme::FONT_MICRO)
                                    .text_color(if self.compare_with.is_some() {
                                        theme::ACCENT()
                                    } else {
                                        theme::DIM()
                                    })
                                    .child("对比")
                                    .on_click(cx.listener(|this, _, _, cx| {
                                        // 默认拿「选中线 vs 正典」，作者立刻看到东西
                                        if this.compare_with.is_none() {
                                            this.compare_with = Some("α".into());
                                        }
                                        cx.notify();
                                    })),
                            ),
                    ),
            )
            .when_some(compare_el, |d, cmp| d.child(cmp))
            .when(self.compare_with.is_none(), |d| {
                d.child(div()
                    .flex_1()
                    .min_h_0()
                    .flex()
                    .gap(theme::GAP_GROUP)
                    .p(theme::PAD_MD)
                    // 左：轨道名（与画布行高对齐）
                    .child(
                        div()
                            .w(px(160.))
                            .flex_shrink_0()
                            .flex()
                            .flex_col()
                            .child(div().h(px(head_h)))
                            .children(track_rows),
                    )
                    // 中：时间轴
                    .child(timeline)
                    // 右：详情
                    .child(
                        div()
                            .id("wl-detail")
                            .w(px(268.))
                            .flex_shrink_0()
                            .min_h_0()
                            .overflow_y_scroll()
                            .flex()
                            .flex_col()
                            .gap(theme::GAP_SM)
                            .p(theme::PAD_SM)
                            .rounded(theme::RADIUS_PANEL)
                            .border_1()
                            .border_color(theme::LINE())
                            .bg(theme::PANEL())
                            .child(detail),
                    ),
                )
            })
    }
}

impl BasePanel for WorldlinePanel {
    fn panel_name(&self) -> &'static str {
        "worldline-view"
    }
}

impl ComponentPanel for WorldlinePanel {
    fn tab_name(&self, _: &App) -> Option<SharedString> {
        Some("世界线".into())
    }

    fn title(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div()
            .text_size(theme::FONT_BODY)
            .font_weight(FontWeight::MEDIUM)
            .text_color(theme::TEXT())
            .child("世界线 · 时间轴")
    }
}

fn track_color(t: &Track) -> gpui_kit::Rgba {
    if t.kind == Kind::Base {
        theme::BLUE()
    } else if t.kind == Kind::Fixed {
        theme::GREEN()
    } else if t.archived {
        theme::FAINT()
    } else if t.kind == Kind::Forecast {
        theme::AMBER()
    } else {
        theme::PURPLE()
    }
}
