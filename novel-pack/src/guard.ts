// 执笔的只读守卫：给某个 Agent（preset 里的子 agent）收窄可见工具。
//
// 为什么需要它：DSH 的 agent preset 只决定“这个 agent 自己挂哪些插件”，
// 但子 agent 会用 `tools.view()` 把 host 平面（主 agent 那一层）注册的全局工具
// 一并继承下来。所以 preset 里写 `tool-fs: { write: false }` 并不能阻止子 agent
// 调 host 的 write。Cordis 的 `ctx.tools.restrict({deny})` 才是按 scope 生效的掩码。
//
// 用法（在 agent-preset 的 plugins 里挂一行）：
//   - id: readonly-guard
//     name: '@zhibi/novel-pack/guard'
//     config:
//       deny: [bash, write, edit]
//
// 设计要点：
// - 只在“已知全局工具”范围内 deny，避免 DSH 升级后某个工具名消失导致整行报错。
//   spike 实测：`tools.restrict` 遇到未知名字会整体抛错，连合法的 deny 都不生效。
// - 不写死工具清单：每次应用时从 ctx.tools 读当前可见集合求交集。
// - 失败必须显式记日志，不能静默降级成“什么都没限制”。

export const name = 'zhibi-readonly-guard'
export const inject = ['tools']

/** 拼一个尽量不吵的日志出口：DSH 的 ctx.logger 与 console 都可能不存在于测试环境。 */
function warn(ctx, msg) {
  try {
    ctx.logger?.warn?.(msg)
  } catch {
    /* logger 不可用就退回 stderr */
    try {
      process.stderr.write(`[zhibi-guard] ${msg}\n`)
    } catch {
      /* 连 stderr 都没有就算了，不该因此崩掉 */
    }
  }
}

export function apply(ctx, config = {}) {
  const wanted = Array.isArray(config.deny) ? config.deny.filter((x) => typeof x === 'string') : []
  if (wanted.length === 0) return

  // 当前作用域可见的工具名（含 host 继承下来的全局工具）。
  let known = []
  try {
    known = (ctx.tools.schemas?.() ?? []).map((s) => s.name).filter((n) => typeof n === 'string')
  } catch (e) {
    warn(ctx, `读取工具清单失败，跳过收窄：${e && e.message}`)
    return
  }
  const knownSet = new Set(known)
  const deny = wanted.filter((n) => knownSet.has(n))
  const unknown = wanted.filter((n) => !knownSet.has(n))
  if (unknown.length > 0) {
    // 不致命：工具可能还没注册，或 DSH 升级后改名。如实记下，方便排查。
    warn(ctx, `忽略未知工具名：${unknown.join(', ')}`)
  }
  if (deny.length === 0) return

  try {
    const dispose = ctx.tools.restrict({ deny })
    ctx.effect(() => dispose)
  } catch (e) {
    warn(ctx, `收窄工具失败（${deny.join(',')}）：${e && e.message}`)
  }
}
