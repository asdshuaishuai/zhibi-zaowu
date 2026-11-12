# M0 Spike 报告：DSH 基础运行环境

日期：2026-11-12　DSH：`@deepseek-ai/dsh@0.2.2-rc.1`　Node：24.22.2（macOS arm64）
实测代码在 `runtime/spike/`；运行环境在 `runtime/zhibi-runtime/`。所有测试用临时 `DSH_HOME`（`/tmp/zhibi-spike-*`），
账号凭据按用户授权从 `~/.dsh/.credentials.yaml` 复制到临时目录，测完应删除。

## 结论总表

| # | 问题 | 结果 | 证据 |
|---|---|---|---|
| 1 | 自建 `zhibi-runtime` 包（只装 `dsh-*` 本体，不装 web/桌面端 bundle）能否启动 sdk | **通过** | `node lib/bin.js --profile sdk --help` 正常；`initialize` 返回 `deepseek-harness-sdk-runtime` |
| 2 | 体积 | **274 MB**（`node_modules`，184 个 `@deepseek-ai` 包；原整包 758 MB） | `du -sh`；npm 自己把重复 zod 去重，未做任何手工裁剪 |
| 3 | 官方 `lib/*.js` 原样复制是否足够 | **通过** | 6 个文件原样拷贝，`INSTALL_ANCHOR` 指向 `lib/../package.json` |
| 4 | 缺少 web-app / acp-app 等包时是否报错 | **不报错** | 启动 stderr 为空 |
| 5 | `deepseek-account` 路由用复制来的账号凭据能否真实对话 | **通过**（`deepseek-v4-flash` 回复“你好”，`turn/end completed`） | `runtime/spike/rpc.mjs` |
| 6 | host 平面 PTC（`tool-executor-native` 关、`tool-executor-ptc` 开） | **通过** | 主 agent 只暴露 `run_code`，系统提示含完整 SDK 声明 |
| 7 | host 平面挂 `agent-preset-registry` + `tool-subagent(allowedAgentPresets)` | **通过，无启动顺序问题** | SDK 声明里出现 `agent_preset?: "reviewer"`；子会话事件 `agent-preset/selected {reviewer}` |
| 8 | PTC 主 agent 经 `subagent` 指挥 preset 子 agent | **通过** | 主 agent 在 `run_code` 里 `tools.subagent({agent_preset:"reviewer"})`，子 agent 用 `send_message` 回传，主 agent 转述 |
| 9 | **子 agent 工具范围**：preset 的 `tool-fs {write:false}` 是否能限制子 agent | **不能**（风险成立） | 子 agent 列出的工具含 `bash/write/edit`，并且真的创建了文件、执行了 `echo hi` |
| 10 | 缓解：preset 内放一个 `tools.restrict({deny})` 的插件 | **有效** | 子 agent 工具从 31 个降到 26 个，不含 `bash/write/edit/plugin_manager/workflow`；`write` 未被调用、磁盘无新文件 |
| 11 | `tools.restrict` 对未知工具名 | **整体抛错** | `names unknown global tools "schedule_create", "schedule_edit"`；必须只写真实存在的全局工具名，且需用 try/catch 兜底 |

## 对计划的修正

1. **“子 agent 继承 host 工具超集”确认为真**，且 preset 里 `tool-fs` 的开关无效（那是 preset 自己的行，host 行仍然可见）。
   → 需要“只读专属 Agent”（校审、人物、关系、世界线分析等）时，必须在 preset 里加一个**极小的限制插件**
   （调用 `ctx.tools.restrict({deny})`，约 20 行，不改 DSH 任何代码）。该插件应放进 `novel-pack`，
   而不是每个 preset 各写一份。deny 名单从 `ctx.tools` 的已知全局工具里取交集，避免因名单过期而整体失败。
2. 计划里“`inject` 覆盖”这一项**不需要**：registry 与 `tool-subagent` 在 sdk 里可正常共存。
3. PTC 主 + native 子的执行器组合**可用**：子 preset 里放 `tool-executor-native` 后，子 agent 直接调原生工具。
4. `sessionId` 在同一个 `DSH_HOME` 内必须唯一（重复会报 `session "x" already exists`），客户端需用唯一 id 或走 `session/resume`。
5. 体积：274 MB 已满足“不激进裁剪”原则，不再做目录去重等处理。

## 未验证（后续里程碑再测）

- `tool-cordis` / `plugin_manager` 在 sdk 里安装 preset bundle 并写入 `$DSH_HOME/profiles/sdk/cordis.patch.yml` 的完整链路
  （已确认两者都在 sdk 里挂载；`config-editor`/`plugin-manager` 写的是 `profileContext.patchPath`；`plugin_manager` 的包安装需要 `pnpm` 在 PATH）。
- 账号 loopback 登录（本次只验证了复用已有凭据）。
- 余额接口。
- `session/resume` 恢复后的上下文完整性。
- 每书一个进程的内存占用。

## 补充实测（plugin_manager / 创造模式持久化）

| # | 问题 | 结果 | 证据 |
|---|---|---|---|
| 12 | `plugin_manager` 在 read-only/workspace-write 下能否安装 bundle | **被拒** | `Error: sandbox escalation to "danger-full-access" requires approval, but no approval channel is available` |
| 13 | 开启 `initialize.approvalRequests=true` 并由客户端回 `{outcome:"allowed-once"}` | **放行** | 收到 `approval/request {toolName:"plugin_manager", reason:"escalate sandbox to danger-full-access…"}`，应答后继续执行 |
| 14 | 安装本地 bundle 的落点 | **profile 目录** | 返回 `application:"restart-required", restartReason:"no-hmr"`；`$DSH_HOME/profiles/sdk/package.json` 的 `dsh.profile.bundles` 追加了 `@local/dsh-spike-preset`，`dependencies` 记 `link:/tmp/spike-bundle/review-preset` |
| 15 | 重启后 preset 是否生效 | **生效** | 新进程 `--profile sdk --dump-config` 输出里出现 `id: preset-spike` 行 |

### 由此确定的两条实现要点

1. **创造模式的产物不必我们序列化**：让 DSH 官方的 `plugin_manager` 写 profile，我们只需
   - 在 GUI 里把 `approval/request` 渲染成审批卡（默认允许并提示“这会永久修改本书的 Agent 配置”）；
   - 安装完成后提示重启该书的 DSH 进程（`no-hmr`）；
   - 重启后读 `profiles/sdk/package.json` 的 `bundles` 来展示“本书专属 Agent 已启用”。
2. **每书一个 `DSH_HOME` 就是隔离边界**：bundle 写在本书的 profile 里，不会影响其他书。

## M1 实机验证（Rust 生成的 patch 直接喂给 DSH）

| # | 项 | 结果 |
|---|---|---|
| 16 | `DshClient::mode_patch` 生成的托管模式 patch 能被 DSH 加载 | **通过**，`--dump-config` 里 8 个 `preset-*` 行全部就位 |
| 17 | 主 agent 按 `agent_preset` 派活儿到指定专属 Agent | **通过**，`agent-preset/selected {relations}` |
| 18 | `novel-pack/src/guard.ts` 在 preset 里生效 | **通过**，`relations` 子 agent 自报可见工具 26 个，不含 `write`/`bash` |
| 19 | 生成的 patch 内容不变时不重写文件 | **通过**（单元测试 `patch_is_stable_across_calls`） |
| 20 | 每书 DSH_HOME 稳定且互相隔离 | **通过**（单元测试 `home_dir_is_isolated_per_book_and_stable`；用进程内缓存防 `HOME` 漂移） |

`cargo test` 30 项全绿（新增 6 项：模式 patch 内容、guard 覆盖、稳定性、home 隔离）。

## M1 收尾验证

| # | 项 | 结果 |
|---|---|---|
| 21 | `session/resume` 跨进程接回历史 | **通过**：进程 A 让模型记住「青锋」，进程 B resume 后追问，答「青锋」 |
| 22 | `offer_branches` 结构化卡片工具 | **通过**：模型真实调用，返回 3 个走向名（冒雨直闯长街 / 石桥遇旧识 / 走出十步回头） |
| 23 | novel-pack 工具总数 | 24 → **27**（+offer_branches/offer_rewrite/offer_continuation） |
| 24 | 前端文本猜意图 | **已清除**：`【A】` 解析、`pending_sel` 全部删除；改写采纳改为校验原文仍在 |

sdk 协议没有 `session/abort`，所以「停止」= 杀进程 + 下次发送时 resume（已实现并实测可行）。

## M3 实机验证（作者主权底座）

| # | 项 | 结果 |
|---|---|---|
| 25 | 辅助档调 `write_ledger` | **被拒**，提示改用 propose_* |
| 26 | 共写档调 `write_ledger` | **成功**，clues.json 真实落盘 |
| 27 | 共写档调 `write_prose` | **被拒**，提示用 offer_rewrite |
| 28 | 代笔档 + 授权第 2 章 → 写第 2 章 | **成功**，`chapters/ch-002/prose.md` 落盘 |
| 29 | 代笔档 → 写第 3 章（未授权） | **被拒**，理由列出已授权章节 |
| 30 | 切档位是否要重启运行时 | **不需要**：档位写 `zhibi-auth.json`，novel-pack 每次调用重读 |

工具数 27 → 30（+get_write_mode / write_ledger / write_prose）。
