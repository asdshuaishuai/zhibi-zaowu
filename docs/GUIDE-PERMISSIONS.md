# 权限与安全边界

这份文档回答三件事：**AI 在什么档位能写什么、档位从哪里读、越界后怎么发现和撤回。**

## 1. 三个写作档位

| 档位 | 台账 JSON | 正文 `prose.md` | 典型用途 |
|---|---|---|---|
| 辅助 `assist` | 只能 `propose_*`，不能直写 | 不能写 | 默认；人写正文，AI 理账 |
| 共写 `cowrite` | 可 `write_ledger` 直写 | 不能写，只能给建议/续写 | 作者愿意让 AI 维护台账 |
| 代笔 `ghostwrite` | 可 `write_ledger` | 仅 `ghostwriteChapters` 里授权的章可 `write_prose` | 作者授权的整章代写 |

提案（`propose_*`）在任何档位都可用；采纳仍由作者在收件箱点。

## 2. 档位的事实源与优先级

```
书目录/zhibi-auth.json  >  设置页「默认写作档位」  >  assist 兜底
```

- **`zhibi-auth.json` 是唯一事实源**：`{"writingMode":"assist|cowrite|ghostwrite","ghostwriteChapters":[2,3]}`。
- 第一次打开一本书、且书目录没有这个文件时，应用会把设置页的默认档位写进去；之后两者一致。
- 设置页改「默认档位」只影响**还没有 `zhibi-auth.json` 的新书**；已有书的档位在底栏 chip 里切，立即写书内文件。
- novel-pack 每次工具调用都重读 `zhibi-auth.json`，切档不需要重启 AI 运行时。

> 排障：如果底栏显示「辅助」但 AI 仍能直写台账，先检查书目录有没有 `zhibi-auth.json`。这是旧版本可能出现的分叉；当前版本开书时会落盘，不应再出现。

## 3. 工具面边界

| 工具 | 谁能调 | 闸门 |
|---|---|---|
| `propose_*` | 所有档位 | 只登记提案，作者采纳后才落库 |
| `write_ledger` | 共写 / 代笔 | `canWriteLedger`；只允许 8 个台账 JSON，不含 `prose.md` / `canon/*.md` |
| `write_prose` | 代笔 + 逐章授权 | `canWriteProse`；章节必须在 `ghostwriteChapters` |
| `apply_proposal` | 宿主专用 | 需 `hostApplyToken`；AI 调用一律拒绝 |
| `get_*` / `offer_*` | 所有档位 | 只读 / 卡片，不落盘 |

六个专属 Agent 的 preset 默认 deny：

```
bash, write, edit, plugin_manager, workflow, write_ledger, write_prose
```

即专属 Agent 连直写工具也拿不到；它们只能出提案。主 Agent 仍按当前档位受 `write-policy` 约束。

> guard 缺失时 preset 会退回“继承全部工具”（只在能力包文件缺失时发生）。这不是正常状态；如果轨迹里提示 guard 未加载，先修安装而不是继续写作。

## 4. AI 直写如何被记录

AI 绕过 Rust 落库函数、直接用 `write_prose` / `write_ledger` / 原生 fs 写盘时：

1. 回合开始时应用对可写文件做一次快照（正文、章 meta、七个台账、memory/dossiers、canon）。
2. 回合结束时比对，把变化补记成 `Actor::Ai` 的改动记录；如果同一文件在本回合已有更晚的作者记录，则跳过（不误记）。
3. 「资料 → 改动记录」能看到这些条目并撤销。
4. 越界检查只认 `chapters/ch-NNN/prose.md`：改了没有代笔授权的正文会通报，可在改动记录撤回。

## 5. Agent 形态（与写作档位是两回事）

| 形态 | 子 Agent | 说明 |
|---|---|---|
| 托管 Managed | 可派六个专属 Agent | 默认；主 Agent 按 `agent_preset` 路由 |
| 极简 Minimal | 无子 Agent | 所有活主 Agent 自己干；系统提示不会注入派活路由表 |
| 创造 Creator | 可派 + Cordis 工具 | 用于造本书专属 Agent |

极简模式下点「知识库全量提取 / 拆分」不会静默退化：任务书仍要求分派，但 `tool-subagent` 已被关闭。需要分派时请切到托管。

## 6. 数据安全底线

- 采纳提案有前像快照与结构校验；写入失败或结构坏了会整档回滚，提案保持待审。
- `relations.json` / `characters.json` / `worldlines.json` 写路径严格读；坏文件拒绝覆盖。
- 正文采纳前自动快照，可在「快照」页恢复。
- 删除知识库文档会记 journal，可撤销。

## 相关文档

- [第一次写一章](GUIDE-WRITING.md)
- [工具与概念参考](REFERENCE.md)
