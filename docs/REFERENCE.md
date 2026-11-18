# 工具与概念参考

**以代码为准。** 本页的工具/技能数量随版本变化；如果与 README 的旧数字冲突，信这一页。

当前版本：**34 个工具 / 19 个技能 / 7 个画布维度**。

## 1. 工具面（novel-pack）

### 1.1 只读

| 工具 | 返回 | 上限 |
|---|---|---|
| `get_chapter` | 单章正文 | 12000 字 |
| `get_clues` | 全部伏笔 | 无截断 |
| `get_outline` | 故事线/事件/阶段 + 档案卡 | 档案卡最近 12 章 |
| `get_canon` | 设定标题 + 内容 | 前 24 篇 × 1600 字 |
| `get_characters` | 人物档案 | 默认 30 / 上限 80 |
| `get_world_state` | 紧凑世界图 | 事件事实 80 字 |
| `recall_memory` | 按需召回事实（相关度+章距+新近度排序去重，别名归一） | 默认 12 条 |
| `infer_memory` | 推断报告（同门/对称推导、单值冲突、陈旧待复核） | 只读 |
| `list_proposals` | 提案列表 | 默认 50 |
| `get_write_mode` | 当前档位与授权章 | — |

### 1.2 提案（只登记，不落库）

| 工具 | 提案 kind | 采纳后写入 |
|---|---|---|
| `propose_canon` | `canon` | `canon/*.md`（同题保护） |
| `propose_clues` | `clues` | `clues.json`（同 id 合并） |
| `propose_relations` | `relations` | `relations.json` 区间 + `memory.facts` |
| `propose_character` | `character` | `characters.json`（只补空）+ `memory.aliases` |
| `propose_aliases` | `aliases` | `memory.aliases` |
| `propose_canon_classify` | `canonClassify` | 设定类别头（可选改名） |
| `propose_skeleton` | `skeleton` | `chapters/ch-NNN/meta.json`（含 `skeletonApproved`） |
| `propose_draft` | `draft` | `prose.md`（先快照；需骨架已批准） |
| `propose_validation` | `report` | `chapters/ch-NNN/meta.json` 的 report |
| `propose_deslop` | `deslop` | `prose.md` 逐处替换 |
| `propose_memory` | `memoryPack` | `memory.facts` |
| `propose_dossier` | `dossier` | `dossiers.json` + `memory.aliases` |
| `propose_clue_fixes` | `clueFixes` | `clues.json` 状态/埋设章/动作日志 |
| `propose_outline_updates` | `outlineUpdates` | `events.json` / `storylines.json` |
| `propose_worldline` | —（沙盒直写） | `worldlines.json` 新线 |
| `simulate_worldline` | —（沙盒直写） | 沙盒线 nodes/edges/diffs；α 拒绝 |
| `promote_diff` | 视目标而定 | 生成 canon/canonRevise/clues/aliases/outlineUpdates 提案 |
| `archive_worldline` | —（沙盒直写） | 归档/恢复；α 拒绝 |

### 1.3 卡片 / 宿主 / 直写

| 工具 | 用途 |
|---|---|
| `offer_branches` | 三个故事走向卡片（`label` + `text` + `mood` + `touches`） |
| `offer_rewrite` | 选区改写对比卡 |
| `offer_continuation` | 一句续写卡 |
| `apply_proposal` | **宿主专用**；需 `hostApplyToken`，AI 调用被拒 |
| `write_ledger` | 直写 8 个台账 JSON（共写/代笔档） |
| `write_prose` | 直写正文（代笔档 + 逐章授权） |

## 2. 技能（19）

| 技能 | 谁用 | 作用 |
|---|---|---|
| `zhibi-skeleton` | 主 Agent | 搭骨架 |
| `zhibi-draft` | 主 Agent | 按批准骨架出草稿 |
| `zhibi-continue` | 主 Agent | 续写一句 |
| `zhibi-branches` | 主 Agent | 三个走向 |
| `zhibi-rewrite` | 主 Agent | 选区改写 |
| `zhibi-deslop` | 主 Agent / 校审 | 去 AI 味 |
| `zhibi-validate` | 主 Agent / 校审 | 单章验收 |
| `zhibi-audit` | 校审 | 全书连贯性 |
| `zhibi-memory` | 主 Agent | 事实/档案卡记账 |
| `zhibi-dossier` | 主 Agent | 章节档案卡 |
| `zhibi-inventory` | 大纲伏笔 | 伏笔盘点 |
| `zhibi-outline-sync` | 大纲伏笔 | 大纲对账 |
| `zhibi-canon-classify` | 大纲伏笔 | 设定归类 |
| `zhibi-character` | 人物 | 人物档案补全/新建 |
| `zhibi-relations` | 关系 | 关系区间梳理 |
| `zhibi-oracle` | 世界线 | 假设推演 |
| `zhibi-forecast` | 预言 | 预测线 |
| `zhibi-director` | 主 Agent | 排期板核对 |
| `zhibi-import-extract` | 主 Agent | 知识库提取分派 |

六个专属 Agent 与技能绑定：人物→`zhibi-character`、关系→`zhibi-relations`、世界线→`zhibi-oracle`、预言→`zhibi-forecast`、大纲伏笔→`zhibi-outline-sync`/`zhibi-inventory`/`zhibi-canon-classify`、校审→`zhibi-audit`/`zhibi-validate`/`zhibi-deslop`。

## 3. 数据文件

| 文件 | 内容 |
|---|---|
| `project.json` | 书名/题材/目标章数/单章字数/每日字数 |
| `canon/*.md` | 设定文档（标题即文件名） |
| `storylines.json` | 故事线 |
| `events.json` | 事件（`chapter`、`objectiveFact`、`happened`、`revealed`） |
| `stages.json` | 阶段 |
| `clues.json` | 伏笔（`plantedChapter`、`status`、`actionLog`） |
| `characters.json` | 人物档案（`lockedFields`、`arc`、`coreStory`） |
| `relations.json` | 关系区间（`fromChapter`/`toChapter`/`label`/`publicToReader`/`directed`） |
| `memory.json` | `aliases`（画布人物源）+ `facts`（总览维兼容） |
| `dossiers.json` | 章节档案卡 |
| `worldlines.json` | 世界线、节点、边、影响清单 |
| `proposals/inbox.json` | 提案收件箱 |
| `chapters/ch-NNN/meta.json` | 章标题/备注/骨架/报告/`skeletonApproved` |
| `chapters/ch-NNN/prose.md` | 正文 |
| `chapters/ch-NNN/snapshots/` | 采纳前正文快照 |
| `chapters/ch-NNN/ai-marks.json` | AI 参与墨点 |
| `imports/` | 知识库原始材料 + `INDEX.md` |
| `journal/` | 改动日志 `changes.jsonl` + 前像 `blobs/` |
| `zhibi-auth.json` | 写作档位与代笔授权章节 |

## 4. 概念口径

- **章号**：1 基。`chapters/ch-001` 即第 1 章。空 `prose.md` 不算成稿。
- **关系区间**：`[fromChapter, toChapter)`；`0` 表示开放端点（起点=一开始，终点=至今）。`toChapter` 必须晚于 `fromChapter`。
- **关系方向**：`directed=true` 时 A→B 与 B→A 是两条；`false` 时两个方向合并。
- **公开性**：`publicToReader=false` 是暗线；关系维读者视角不画。
- **画布七维**：总览 / 设定 / 人物 / 关系 / 时间 / 事件 / 转折。没有“预言维”；世界线在「世界线」页看。
- **节点四态**：`planned`（计划）/ `adopted`（已采纳待写）/ `written`（已写进正文）/ `branch-only`（仅此分支）。
- **固化边界**：`max(非空正文最大章, worldline.json 的 confirmedThrough)`。
  预演线的节点与 `fixedThrough` **一律不参与**；if 线不可扶正（`fix_line` 永远报错）。
- **预言书**：if 线存根，与主线无关；保留分支供番外取用，不进正典。
- **事件缺省**：`happened=false`（计划）、`revealed=false`（暗线）。
- **提案状态**：`待确认` → `已采纳` / `已拒绝`；拒收理由会回灌给 AI。
- **档位优先级**：书内 `zhibi-auth.json` > 设置默认（仅首次开书）> `assist`。
- **记忆召回**：按需（不自动）。整数打分 = 相关度（每命中词 300/200/100）+ 章距（0..200，窗口 20 章）+ 新近度（0..100）；别名归一后按 `(主语,谓语,宾语)` 去重保留高分、同分留新章；稳定排序 分↓→章↓→主语→谓语→宾语，默认 12 条。Rust/TS 两端同夹具。
  - `chapter` 参数语义：**写作时锚"正在写的这一章"**（写作链路里 `zhibi-skeleton`/`zhibi-draft` 都这么传），不是上一章；**手动对账时锚"你关心的那一章"**（点记忆页召回框 / 直接调工具时）。
- **记忆推断**：只读报告。同门推导（共享师从）、对称补全（夫妻/结义/同门）、单值冲突（夫妻/配偶/生父/生母）、陈旧待复核。不自动落库。
  - **陈旧只标状态型谓语**（默认 `配偶`/`夫妻`/`位置`/`状态`/`身份`/`所在地`，生效章距今 >30 章）。历史事件（`获得`/`失去`/`去过`）**永远为真**，不会因为年头久被要求复核——早期实现反着来（除少数耐久谓语外一律标陈旧），在一本 118 章的书里把 61 条事实中的 38 条误报，提醒等于噪音。
  - **书级可配**：`memory.json` 加 `inferRules` 块（`symmetric`/`singleValued`/`volatile`/`staleWindow`）覆盖默认——修仙题材给 `volatile` 加 `境界`，职场题材加 `职位`。Rust/TS 双端同读同一份配置。
- **导演台↔世界线**：共享章号坐标。章抽屉「排期板看此章」跳导演台并定位；排期板列头「世界线」跳世界线并聚焦该章。

## 5. 相关指南

- [第一次写一章](GUIDE-WRITING.md)
- [权限与安全边界](GUIDE-PERMISSIONS.md)
- [世界线与剧情预演](GUIDE-WORLDLINE.md)
- [知识库与召回边界](GUIDE-KNOWLEDGE.md)
