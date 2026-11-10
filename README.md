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
│       ├── editor.rs      # 稿纸编辑器（选区浮条轮询/进度/速记/分支卡）
│       ├── canvas.rs      # 剧情画板 + 预言书（M7/M8 结构视图，七维织网+世界线双态）
│       ├── director.rs    # 导演台排期板（M7-3 + 预测候选）
│       ├── bookinfo.rs    # 书目信息（题材/简介/目标章数/章字数目标）
│       ├── trajectory.rs  # 右侧轨迹面板（对话式时间线）
│       └── dsh/           # DSH sdk 子进程客户端（NDJSON JSON-RPC over stdio）
├── scripts/
│   ├── dev.sh             # 一键起 GUI（自动 nvm 24）
│   ├── dsh-smoke.sh       # 无 GUI 协议冒烟（node 引导 + initialize/session 事件流）
│   ├── build_app.sh       # macOS .app 打包
│   ├── build_app_linux.sh # Linux AppImage/tar.gz（原生机执行）
│   └── build_app_windows.ps1 # Windows portable/NSIS（原生机执行）
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

## 里程碑与当前实现状态

- [x] **M1** 骨架可跑：GPUI 三栏 + DSH sdk 子进程连通 + 轨迹实时滚动
- [x] **M2** 项目与编辑器：书架独立首页（与内容分层）；`.zhibi` 真读写（旧项目直接打开）、
      真章节树（切章先存盘）、新建作品/新建章、CJK 字数口径与旧版一致、磁盘往返单测。
      稿纸编辑器已精修三轮：居中 680pt 纸面栏、实时字数/目标进度条/对白占比、
      1.5s 防抖自动保存（带时间戳）、章标题就地编辑写回 meta.json、速记行（喂「让AI记一笔」）、
      空章引导占位、专注模式（⌘⇧F）、⌘S 保存、⌘⇧B 要走向
- [x] **M3** 提案闭环：novel-pack **17 工具**真实 runtime 注册验证；收件箱 GUI（就地采纳/拒收+理由回灌）；
      确定性采纳落库（同题保护/同 id 合并/改名撞名跳过/草稿快照先行）；编辑器 6 个 AI 按钮 + 任务书库；
      状态条；patch 自动挂载
- [x] **M4** 彩云式共写（第一、二片）：续写一句/给走向 → 轨迹文本气泡「采纳到正文」；
      选区创作（拉深/扩写/缩写/改写/去AI味）→ SelectionDraft 气泡「替换选区/追加」双落点，快照先行。
      （编辑器装饰级幽灵内联文本 = 待做，当前用轨迹气泡+一键采纳达成同等语义）
- [x] **M5** 台账页：设定（类别分组+归类）/ 大纲·时间线（真相|读者已知+同步/审查）/ 伏笔台账（+盘点）/
      记忆中枢（事实账本+记一笔）/ 人物卡（别名归一）/ 写作统计（14 天柱状+提案账本）/
      记忆图谱（列表视图）/ 导入旧稿（第N章启发式 → 提案制，单测覆盖）/ 快照页（恢复）/ 导出合并稿
- [x] **M6** 打包：`scripts/build_app.sh` 出 macOS `.app`（Info.plist/icns/ad-hoc 签名，release 19MB 验证）；
      `scripts/build_app_linux.sh` Linux 原生构建 → AppImage（appimagetool/linuxdeploy，缺则 tar.gz 回退）；
      GPUI 图形栈需目标机原生编译（无法从 macOS 交叉），Windows 同理待 Win 主机 runner。自动更新待补
- [x] **M7** 结构视图三件套（无限画板/导演台/预言书）：
      画板=节点图（故事线/事件/人物/伏笔/设定五类卡，`canvas()`+`PathBuilder` 贝塞尔边，
      暗线虚线紫/伏笔琥珀/人物牵涉绿；拖拽/平移/±缩放/橡皮筋框选成组（prepaint 原点换算）/
      一阶影响半径侧板/读者视角隐线；视图态 `canvas.json` 持久化）；
      导演台=8 章窗口排期板（事件/伏笔卡跨列拖拽**作者亲笔直写台账**、幕带卷名、
      确定性合同预警、列级「AI 核对」→ director_task）；
      预言书=世界线沙盒（α 基准/β… 推演只写 `worldlines.json`；影响清单逐条晋升→收件箱；
      `canonRevise` 采纳前自动备份 `canon/backups/`；衍生实体 `line:β` 归属标；
      假设 composer→`oracle_task` 进同一条 DSH 会话，轨迹窗全程可见）。
      novel-pack 23 工具（M7 五工具 + propose_aliases 在真实《断刀记验证》台账验证）；19 Rust 单测全绿

已知边界（如实，2026-11-10 复审计）：
- 「选区创作」浮条**已解决**：库无 `SelectionChanged` 事件，但 `selected_text()` 可读 →
  220ms 轮询比对，有选区才浮现（鼠标划选与 Shift+方向键通吃）。
- 局域网 OpenAI 兼容路由（`zhibi-lan`）已写进 patch 并条件挂载，但其适配器依赖
  `@deepseek-ai/dsh-environment` 尚未发布——发布后自动点亮；当前真实模型回环走
  `deepseek-official`（需 DEEPSEEK_API_KEY）。
- **提案落库现已全 kind 覆盖**（双端同语义 + 行为测试）：canon/clues/canonClassify/draft/
  canonRevise/outlineUpdates + 本轮补齐的 **skeleton**（meta 批准+节拍）/**report**（章 meta 归档）/
  **deslop**（逐处替换，未命中跳过记因，首次改写前自动快照）/**memoryPack**（事实入账去重）/
  **dossier**（档案卡同章覆盖）/**clueFixes**（台账状态+动作日志，不动正文）。
  仅剩 memo/storylines/outlineEvents 三个历史名——无工具会产生，如实报「无对应语义」。
- 装饰级**内联幽灵文本**（Tab 采纳那种）复审计后仍是库边界：gpui_base 内嵌 input 虽带
  `RangeDecorationCollection` 类型，但 TextareaState 未向调用方暴露 setter 入口——
  当前用轨迹气泡+一键采纳达成同等语义；竖排封面题字同（无 writing-mode）。
