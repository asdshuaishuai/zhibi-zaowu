# 执笔造物

**人写正文，AI 理账。作者是创世主，AI 是工具与权柄。**

执笔造物是一个跨端 AI 小说 / 剧本创作工作台。正文永远由作者落笔；AI 负责读全书、
理账、提案——所有产出都要经过作者裁决才可能进入正典。它把设定、伏笔、时间线、
人物关系、事件与转折织成一张可交互的网，把每一种「如果」铺成平行的世界线。

> 前身是 macOS SwiftUI 单端的「执笔」。这一代是彻底重写：DeepSeek Harness
> agent 底座 + GPUI 原生前端，`.zhibi` 项目目录双向兼容。

## 界面

- **书架**：书卡（五种封皮构图按书名哈希）、每日目标、新建 / 导入。
- **工作台**：章节树 · 稿纸编辑器 · 右侧 Dock（轨迹 / 提案 / 对话）。
- **剧情画板**：七维视图（总览 / 设定 / 人物 / 关系 / 时间 / 事件 / 转折），
  节点可拖、边可读、暗线虚线，选中即显一阶影响半径。
- **世界线（排期板）**：八章窗口排期，事件 / 伏笔卡跨列拖拽即改期；
  幕带、空章预警、伏笔静默逾期、断线预警全部由确定性规则算出。
- **预言书**：α 正典基线 + 预测线 / 假设线；并轨图按章节展开平行时间线；
  影响清单逐条晋升为提案，采纳才动正典，修订前自动备份。
- **人物卡**：别名归一 + 事实账 + 首次出场 + 关系边，卡牌式呈现。
- **提案收件箱**：按类型渲染、依据可溯源、逐条采纳 / 拒收（理由回灌），
  决策账本可回看。

## 铁律

1. **AI 只提案**：设定、伏笔、骨架、草稿、关系、大纲更新全部以提案登记，
   不存在任何直接写正文的工具。
2. **作者签收**：采纳前自动快照；同题保护、同 id 合并、改名撞名跳过；
   一切可回滚。
3. **全程可见**：工具调用、参数、产出条数、失败原因都显示在轨迹窗，可回放。
4. **AI 不能自批**：采纳接口有宿主令牌门禁，AI 侧拿不到令牌。

## 技术栈

| 层 | 选型 |
|---|---|
| 前端 | Rust + GPUI（基于 gpui-kit 组件库：Dock / Table / Bubble / Tag / Empty …） |
| Agent 底座 | DeepSeek Harness（DSH）`--profile sdk` 子进程，NDJSON JSON-RPC over stdio |
| 能力包 | `novel-pack`：Cordis 插件，24 个工具（查询 / 提案 / 世界线 / 采纳） |
| 数据 | `.zhibi` 目录（JSON + Markdown），与旧版执笔双向兼容 |
| 运行时 | 内置 Node + DSH（随应用分发，无需全局 npm；见下文） |

## 目录

```
zhibi-next/
├── app/                 # GPUI 前端（Rust）
│   └── src/             # workspace / editor / canvas / director / shelf / inbox / dsh…
├── novel-pack/          # DSH 能力包（Cordis 插件，24 工具）
├── scripts/
│   ├── dev.sh           # 开发启动
│   ├── dsh-smoke.sh     # 协议冒烟
│   ├── vendor_runtime.sh# 一次性 vendor Node + DSH 到 .vendor/
│   ├── build_app.sh     # macOS .app（内含 runtime）
│   ├── build_app_linux.sh
│   └── build_app_windows.ps1
├── design/              # 交互原型（HTML，与实现同源）
├── site/                # 官网（静态单页）
└── docs/                # DESIGN.md / ACCEPTANCE.md
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
新增文件：`worldlines.json`（预言书沙盒）、`canvas.json`（画板视图态）。

## 许可

MIT。DSH 为 DeepSeek 官方开源项目（MIT），本仓库以子进程方式使用，未修改其源码。
