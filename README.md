# 执笔造物

**人写正文，AI 理账。作者是创世主，AI 是工具与权柄。**

执笔造物是一个跨端 AI 小说 / 剧本创作工作台。正文永远由作者落笔；AI 负责读全书、
理账、提案——所有产出都要经过作者裁决才可能进入正典。它把设定、伏笔、时间线、
人物关系、事件与转折织成一张可交互的网，把每一种「如果」铺成平行的世界线。

> 前身是 macOS SwiftUI 单端的「执笔」。这一代是彻底重写：DeepSeek Harness
> agent 底座 + GPUI 原生前端，`.zhibi` 项目目录双向兼容。

## 界面

- **书架**：书卡（五种封皮构图按书名哈希）、每日目标、新建 / 导入。
- **工作台**：章节树 · 稿纸编辑器 · 右侧 Dock（轨迹 / 改动记录 / 收件箱 / 对话）。
- **剧情画板**：七维视图（总览 / 设定 / 人物 / 关系 / 时间 / 事件 / 转折），
  节点可拖、边可读、暗线虚线，选中即显一阶影响半径；关系维有章节滑块。
- **世界线**：剧情的时间轴。横轴 = 真实章节；**已固化的剧情是实线，之后全是
  开放的 if 空间（虚线）**；每条 if 线从它的分叉章长出。页内两个看法：时间轴 /
  对比（两条线共同祖先之后差在哪）。可在此改节点章号、从任意处再分叉、
  把某条 if **扶正**为正典后续（边界推过去，原正典保留用于对照）。
- **画布**：关系网。七维（总览/设定/人物/关系/时间/事件/转折），节点是人物、
  设定、道具、事件，边是人物关系与剧情关联。选中节点还能看到它**在世界线上的
  落点**（这条卡在第几章、属于哪条线）——两侧靠双向弱引用连着。
- **预言书**：if 工坊。选中一条线看推演卷轴（按章分段的因果链）与影响面
  （待晋升 / 已晋升 / 高风险 / 同门竞争），逐条或批量晋升为提案。
- **排期板**：章 × 幕的工作排期，事件 / 伏笔卡跨列拖拽即改期；
  幕带、空章预警、伏笔静默逾期、断线预警全部由确定性规则算出。
- **人物档案**：左列表右档案，外貌 / 性格 / 动机 / 背景 / 目标 / 缺陷 / 口吻 /
  状态 / 弧线，字段可锁定，可合并、可增删别名，旧 facts 无损迁移。
- **关系网**：一条关系有多个**章节区间**（起止章 + 读者是否已知），
  所以「第 5-19 章师徒，第 20 章后仇敌」是可表达的。
- **提案收件箱**：按类型渲染、依据可溯源、逐条采纳 / 拒收（理由回灌），
  决策账本可回看。
- **改动记录**：作者与 AI 的每一次落库都在这里，可单条撤销或整批撤销；
  文件之后又被改过会提示冲突而不是覆盖。
- **设置**：账号余额、模型与推理强度、通用设置、三个 Agent 模式、
  本书专属 Agent、写作设置。

## 铁律

1. **AI 不越权**：辅助档只能提案；共写档可直写台账；只有代笔档 + 作者逐章授权
   才能直写正文。这条分界线由领域层判断（沙箱只能分「能写/不能写」）。
2. **作者签收**：采纳前自动快照；同题保护、同 id 合并、改名撞名跳过。
3. **一切可撤销**：每次落库先存前像，改动日志支持单条与整批撤销，含冲突检测。
4. **全程可见**：工具调用、参数、产出条数、失败原因都显示在轨迹窗，可回放。
5. **AI 不能自批**：采纳接口有宿主令牌门禁，AI 侧拿不到令牌。

## 技术栈

| 层 | 选型 |
|---|---|
| 前端 | Rust + GPUI（基于 gpui-kit 组件库：Dock / Table / Bubble / Tag / Empty …） |
| Agent 底座 | DeepSeek Harness（DSH）`--profile sdk` 子进程，NDJSON JSON-RPC over stdio |
| 能力包 | `novel-pack`：Cordis 插件，30 个工具（查询 / 提案 / 卡片 / 世界线 / 采纳）+ 16 个技能 |
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
├── novel-pack/          # DSH 能力包（Cordis 插件，30 工具 + 16 技能）
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
├── site/                # 官网（静态单页）
└── docs/                # DESIGN.md / ACCEPTANCE.md / SPIKE-M0.md
```

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
scripts/test-all.sh     # Rust（104 项）+ novel-pack（4 项）
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
