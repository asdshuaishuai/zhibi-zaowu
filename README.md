# 执笔 3.0（跨端超级重构）

**人执笔，AI 研墨** —— 还是执笔：一次彻底的大重构。macOS SwiftUI 单端实现 →
**DeepSeek Harness（DSH）agent 底座 + GPUI Kit 跨端前端**（macOS / Windows / Linux）。
执笔的全部能力 1:1 复刻进新底座，并以彩云小梦式人机共写（幽灵续写 / 分支卡 / 选区拉深）
补上「写的时候 AI 就在旁边」的体验。铁律不变：**AI 的一切产出只能以提案形态存在，
作者采纳前不落盘**。老项目（`../zhibi`，SwiftUI）保留不动，`.zhibi` 项目目录双向兼容。

完整设计：[docs/DESIGN.md](docs/DESIGN.md)（含 UI/UX 原型说明与 M1–M6 里程碑）。

## 结构

```
zhibi-next/
├── app/           # GPUI 前端（Rust）：三栏工作台 + 右侧 Agent 轨迹 Dock
│   └── src/
│       ├── main.rs        # 窗口启动
│       ├── workspace.rs   # 工作台装配 + DSH 事件分发
│       ├── editor.rs      # 中央编辑器面板（M1 占位稿 + 发话输入条）
│       ├── trajectory.rs  # 右侧轨迹面板（对话式时间线）
│       └── dsh/           # DSH sdk 子进程客户端（NDJSON JSON-RPC over stdio）
├── scripts/
│   ├── dev.sh             # 一键起 GUI（自动 nvm 24）
│   └── dsh-smoke.sh       # 无 GUI 协议冒烟（initialize → session/query）
└── docs/
```

## 环境要求

- Rust（cargo，含 macOS/Win/Linux 图形栈依赖）
- **Node >= 24.9**（dsh CLI 引擎要求：`import.meta.main` 等）+ 全局包：
  ```sh
  npm i -g @deepseek-ai/dsh
  ```
- 模型（M1 冒烟）：`export DEEPSEEK_API_KEY=…`（deepseek-official 路由）；
  自定义 OpenAI 兼容端点（局域网等）在 M3 由 novel-pack 挂适配器后可用
  `ZHIBI_PROVIDER` / `ZHIBI_MODEL` 覆盖。

## 运行

```sh
scripts/dev.sh        # 起 GUI：发一句话，右侧轨迹实时滚动 session.event 流
scripts/dsh-smoke.sh  # 无 GUI 冒烟：只打印方法名（RESPONSE / session.event / …）
cargo build           # 仅构建
```

## 架构速记

- **前端不实现 agent 循环**：每次 AI 任务 = 复用一个 DSH session，发 prompt，
  收 `session.event` 全量事件流；会话日志 append-only，轨迹可回放、跨启动保留。
- **novel-pack**（M3 起，Cordis 插件包 / dsh.bundle）：执笔的 30+ `propose_*` 提案
  工具 + 确定性扫描（骨架闸门/连贯性/AI味/对账）+ 创作法典 + 项目存取（`.zhibi`）。
  随应用内置分发，保留 `dsh plugin add` 独立安装能力。
- **协议边界**：`packages/sdk/protocol` 的 wire types 是唯一契约
  （initialize / session.query / session.event / session.status / approval…）。
  M1 不启用 approvalRequests 与 userQuestions（runtime 进程内应答）。

## 里程碑

- [x] **M1** 骨架可跑：GPUI 三栏 + DSH sdk 子进程连通 + 轨迹实时滚动
- [ ] **M2** 项目与编辑器：`.zhibi` 读写 + 稿纸编辑器 + 章节树/书架
- [ ] **M3** 提案闭环：novel-pack 首批工具 + 收件箱 + 就地采纳 + 状态条
- [ ] **M4** 彩云式共写：幽灵续写 + 分支卡 + 选区改写（diff + 快照）
- [ ] **M5** 全能力复刻：审查/对账/去AI味/记忆/图谱/导入导出/统计
- [ ] **M6** 跨端打包：Win/Linux + 自动更新
