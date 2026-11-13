# 执笔造物

**人写正文，AI 理账。作者是创世主，AI 是工具与权柄。**

执笔造物是一个跨端 AI 小说 / 剧本创作工作台。正文永远由作者落笔；AI 负责读全书、
理账、提案——所有产出都要经过作者裁决才可能进入正典。它把设定、伏笔、时间线、
人物关系、事件与转折织成一张可交互的网，把每一种「如果」铺成平行的世界线。

> 前身是 macOS SwiftUI 单端的「执笔」。这一代是彻底重写：DeepSeek Harness
> agent 底座 + GPUI 原生前端，`.zhibi` 项目目录双向兼容。

## 界面

侧栏按**写作 / 世界 / 资料 / 系统**四栏分组；章节不逐条平铺，而是收进独立的「章节」综合页。

- **书架**：书卡（五种封皮构图按书名哈希）、每日目标、新建 / 导入。
- **工作台**：左侧分组导航 · 稿纸编辑器 · 右侧 Dock（轨迹 / 改动记录 / 收件箱 / 对话）。
- **章节**：全书目录 + 进度总览，卡片显示每章字数、状态（未动笔 / 草稿 / 成稿）、
  骨架是否批准；右上「新建章」、卡片「继续写」直接进稿纸。
- **剧情画板**：七维视图（总览 / 设定 / 人物 / 关系 / 时间 / 事件 / 转折），
  节点可拖、边可读、暗线虚线，选中即显一阶影响半径；关系维有章节滑块，
  按 `relations.json` 的区间模型画「第 5–19 章师徒、第 20 章起仇敌」这种演变。
  总览是**力导向关系图**：相连的聚在一起，主角钉在中心，
  还没接上主线的收在图下方的**孤儿区**。没有“预言维”——世界线在「世界线」页看。
- **世界线**：剧情的时间轴。横轴 = 真实章节；**已固化的剧情是实线，之后全是
  开放的 if 空间（虚线）**；每条 if 线从它的分叉章长出。页内**两种看法**：
  **时间轴**（一根主干横贯，每条 if 自分叉章弯出；实线=已确定、虚线=开放的 if）/
  **对比**（两条线共同祖先之后差在哪）。可改节点章号、连/删因果边、标明线暗线、
  从任意处再分叉、把某条 if **扶正**为正典后续（边界推过去，原正典保留对照）。
  时间轴可**缩放平移**（长书里相邻两章只差几像素，全览看不出东西）；
  点轴上的任意节点浮出**抽屉**：备注、双向因果链、章号 +1、逐节点采纳、从这儿推演。
- **预言书**：if 工坊。选中一条线看推演卷轴（按章分段的因果链）与影响面
  （待晋升 / 已晋升 / 高风险 / 同门竞争），逐条或批量晋升为提案。
- **章节排期**：章 × 幕的工作排期，事件 / 伏笔卡跨列拖拽即改期；
  幕带、空章预警、伏笔静默逾期、断线预警全部由确定性规则算出。**与世界线无关**。
- **人物卡**：左列表右档案，外貌 / 性格 / 动机 / 背景 / 目标 / 缺陷 / 口吻 /
  状态 / 弧线，字段可锁定，可合并、可增删别名，旧 facts 无损迁移。
- **提案收件箱**：按类型渲染、依据可溯源、逐条采纳 / 拒收（理由回灌），
  决策账本可回看。
- **知识库**：老书的世界观/人物/大纲文档原样归档存放（幂等导入、冲突留档），
  逐篇「AI 拆分」或一键全量提取——总 Agent 通读后分派各专属 Agent 落成提案；
  矛盾不上秤，冲突两条都出留给作者裁决。
- **改动记录**：作者与 AI 的每一次落库都在这里，可单条撤销或整批撤销；
  文件之后又被改过会提示冲突而不是覆盖。
- **设置**：账号余额、模型与推理强度、通用设置、三个 Agent 模式、
  **内置插件开关**（关掉会说明失去什么）、**Agent 预设清单**、本书专属 Agent、
  运行时进程上限与闲置回收、写作设置。

## 铁律

1. **AI 不越权**：辅助档只能提案；共写档可直写台账；只有代笔档 + 作者逐章授权
   才能直写正文。这条分界线由领域层判断（沙箱只能分「能写/不能写」）。
2. **作者签收**：采纳前自动快照；同题保护、同 id 合并、改名撞名跳过。
3. **一切可撤销**：每次落库先存前像，改动日志支持单条与整批撤销，含冲突检测。
   采纳前校验台账结构，不合法的写入整档退回（不会留下改了一半的台账）。
4. **全程可见**：工具调用、参数、产出条数、失败原因都显示在轨迹窗，可回放。
5. **AI 不能自批**：采纳接口有宿主令牌门禁，AI 侧拿不到令牌。

## 技术栈

| 层 | 选型 |
|---|---|
| 前端 | Rust + GPUI（基于 gpui-kit 组件库：Dock / Table / Bubble / Tag / Empty …） |
| Agent 底座 | DeepSeek Harness（DSH）`--profile sdk` 子进程，NDJSON JSON-RPC over stdio |
| 能力包 | `novel-pack`：Cordis 插件，32 个工具（查询 / 提案 / 卡片 / 世界线 / 采纳 / 直写）+ 19 个技能 |
| 数据 | `.zhibi` 目录（JSON + Markdown），与旧版执笔双向兼容 |
| 运行时 | 内置 Node + DSH（随应用分发，无需全局 npm；见下文） |

### Agent 体系

每本打开的书一个 DSH 进程（`DSH_HOME` 与 cwd 都指向这本书），右侧对话是**主 Agent**，
它用 `subagent` 指挥 6 个内置专属 Agent（人物 / 关系 / 世界线 / 预言 / 大纲伏笔 / 校审）。
三种模式：

| 模式 | 主 Agent 形态 | 用途 |
|---|---|---|
| **托管**（默认） | PTC 执行器，可派全部专属 Agent | 写作托管 |
| **极简** | native 执行器、无 subagent、只带提案与卡片工具 | 提案与共创 |
| **创造** | 托管 + 官方 `tool-cordis` 与插件管理器 | 为本书造专属 Agent |

技能库（`novel-pack/skills/zhibi-*.md`）在启动时同步进本书 `DSH_HOME/skills`，
且**关掉默认技能根**——否则会把开发者全局安装的技能（flutter、cloudbase…）
全塞进这本书的上下文里。升级 DSH 时这一层不动。

## 目录

```
zhibi-next/
├── app/                 # GPUI 前端（Rust）
│   └── src/             # workspace / editor / canvas / director / shelf / inbox / dsh /
│                        # characters / relations / journal / worldline / account / prompts…
├── novel-pack/          # DSH 能力包（Cordis 插件，32 工具 + 19 技能）
│   ├── src/             # index.ts / store.ts / guard.ts / write-policy.ts
│   ├── skills/          # zhibi-*.md —— 写作能力的作业规程（AI 加载，作者可改）
│   └── test/            # 双端一致性（node --test）
├── runtime/zhibi-runtime# 封闭的 DSH sdk 运行环境（274MB，只装 sdk 闭包）
├── tests/conformance/   # 双端共享夹具（Rust 与 TS 读同一份）
├── scripts/
│   ├── dev.sh           # 开发启动
│   ├── test-all.sh      # 双端测试（cargo test + node --test）
│   ├── dsh-smoke.sh     # 协议冒烟
│   ├── vendor_runtime.sh# 一次性 vendor Node + DSH 到 .vendor/
│   ├── build_app.sh     # macOS .app（内含 runtime）
│   ├── build_app_linux.sh
│   └── build_app_windows.ps1
├── design/              # 交互原型（HTML，与实现同源）
└── docs/                # 作者指南 + DESIGN/ACCEPTANCE/REFERENCE + index.html（官网单页）
```

## 文档

作者向：

- [第一次写一章](docs/GUIDE-WRITING.md)：从新建章到正文落盘，骨架 → 草稿 → 采纳 → 撤销。
- [权限与安全边界](docs/GUIDE-PERMISSIONS.md)：三个写作档位、`zhibi-auth.json` 优先级、工具闸门、AI 直写如何入账。
- [世界线与剧情预演](docs/GUIDE-WORLDLINE.md)：正典 / 沙盒 / 固化边界、开线、推演、三条晋升路径。
- [知识库与召回边界](docs/GUIDE-KNOWLEDGE.md)：导入、拆分、分派，以及“写作时不会自动召回”这件事。
- [工具与概念参考](docs/REFERENCE.md)：32 个工具 / 19 个技能 / 7 个画布维度的现行口径。

设计向：

- [DESIGN.md](docs/DESIGN.md)：架构与模块边界。
- [ACCEPTANCE.md](docs/ACCEPTANCE.md)：验收清单。
- [SPIKE-M0.md](docs/SPIKE-M0.md)：技术预研记录。

## 构建与运行

### 开发态

```sh
# 1) 一次性准备运行时（Node + DSH 复制到 .vendor/，约 1GB，不入库）
scripts/vendor_runtime.sh

# 2) 起 GUI（自动优先用 .vendor 内嵌运行时）
scripts/dev.sh
```

若本机已全局安装 `@deepseek-ai/dsh`，也可直接用 `ZHIBI_DSH_BIN` 指定。

### 测试

```sh
scripts/test-all.sh     # Rust（209 项）+ novel-pack（6 项）
```

两端读同一份 `tests/conformance/cases.json`。改了任意一侧的规则而没改夹具，
两侧会同时失败——这是这份夹具存在的全部意义。

### 打包 macOS

```sh
scripts/build_app.sh        # → target/release/执笔造物.app（含 runtime）
open target/release/执笔造物.app
```

`.app` 已内置 Node 与 DSH，目标机器**无需安装 Node / npm**。应用内模型配置在
「设置」页：DeepSeek 官方 API Key，或任意 OpenAI 兼容端点（含局域网）。

### 跨端

Linux / Windows 使用同一 Rust 工程原生编译；`scripts/build_app_linux.sh`
与 `build_app_windows.ps1` 是各自打包脚本的骨架（GPUI 图形栈需目标机原生编译）。

## 数据兼容

打开旧版执笔的 `.zhibi` 项目目录即可，字段与文件布局保持一致：
`project.json`、`canon/*.md`、`storylines.json`、`events.json`、`stages.json`、
`clues.json`、`memory.json`、`proposals/inbox.json`、`chapters/ch-NNN/`。
新增文件：`worldlines.json`（预言书沙盒）、`canvas.json`（画板视图态）、
`characters.json`（人物实体）、`relations.json`（区间关系）、`journal/`（改动日志与
内容寻址前像）、`zhibi-auth.json`（写作档位与代笔授权）。

## 许可

MIT。DSH 为 DeepSeek 官方开源项目（MIT），本仓库以子进程方式使用，未修改其源码。
