---
name: zhibi-relations
description: 梳理人物关系网：谁与谁、什么关系、从第几章成立、是否读者已知；同一对人物可以有多段演变。
whenToUse: 作者要求理关系、补关系、或导入旧稿里的人物关系。
---

# 人物关系

## 模型：一条关系史，多段区间

同一对人物只有**一条** `relations.json` 记录，里面按章分区间：

```
沈砚—老周：{5–20 师徒，读者已知} → {20– 仇敌，暗线}
```

- **`fromChapter`**：这段关系**首次成立**的章。不知道就留 0，采纳时会按标签合并，不会凭空造出「从第 0 章起」。
- **`toChapter`**：这段关系**结束**的章，语义是 `[fromChapter, toChapter)`——第 `toChapter` 章起换成下一段。留 0 = 至今仍在。`toChapter` 必须大于 `fromChapter`。
- **`label`**：关系谓词（师徒/父女/宿敌…），画布按它归类着色；同一对被写多段时，颜色跟最后一段走。
- **`publicToReader`**：`false` = 暗线（作者知道、读者还不知道），关系维读者视角不画。
- **`directed`**：`true`（默认）表示有方向，A→B 与 B→A 是两条；`false` 表示无向，两个方向合并成一条。

作者会在关系维里补终点、改区间；你提的是新的一段，不是重写整条关系史。

## 落库边界

- **`relations.json` 是关系的事实源。** 关系维、别名归一、端点匹配都读它。
- 采纳 `propose_relations` 时会**双写** `memory.facts`（`from/to/label/chapter`），那是给总览维和旧数据兼容看的视图，不是第二事实源。两边的去重规则不同：`relations.json` 按「同一对 + 有向性」合并区间，`memory.facts` 按「主语+谓语+宾语+起始章」去重。
- 已在关系维里手改过的关系，不要再用提案去"纠正"，否则会多出一段；要改就由作者在关系维里改。
- 人物改名/删人时，`relations.rs` 的 `rename_person` / `remove_person` 会同步迁移或清理端点；**`memory.facts` 不会跟着改**——旧 facts 里可能残留旧名，这是兼容视图的已知边界，不要据此判断关系是否还在。

## 提取纪律

1. 先 `get_outline` + `get_canon` + `get_characters`，确认人物本名；别名先用 `propose_aliases` 归一，不要用绰号建边。
2. 只提**原文/设定里有依据**的关系。推断出来的给 `evidence` 写明依据。
3. 一对人物关系变了（师徒→仇敌）就再提一条同 pair 的新区间，带上 `fromChapter`（和已知的 `toChapter`）；不要试图改旧边。
4. 拿不准的关系不提；两条材料矛盾时都提，注明冲突。

## 落库

用 `propose_relations` 提案（进收件箱，作者裁决后才写 `relations.json` 与记忆事实）。

关系端点是人名：人物改名后系统会自动迁移，但**不要**用已废弃的旧名建新边。

