// 写作档位的权限判断：AI 到底能写到哪一步。
//
// 为什么放在插件里而不是只靠 DSH 的沙箱：沙箱只能区分“能写文件/不能写文件”，
// 分不出“能写台账但不能写正文”“只能写作者授权的章节”。这条分界线是产品铁律，
// 必须由领域层判断。
//
// 档位来自 `ZHIBI_WRITE_MODE` 环境变量，由宿主在每次启动运行时时写入。
// 每次工具调用都重读——这样作者切档位不必重启进程。

export type WritingMode = 'assist' | 'cowrite' | 'ghostwrite'

/**
 * 当前档位。
 *
 * 优先读书目录的 zhibi-auth.json（作者切档位时宿主写它，立即生效、不必重启运行时）；
 * 没有就用启动时的环境变量；两者都没有 = assist（最保守）。
 */
export function currentMode(root?: string): WritingMode {
  if (root) {
    try {
      const v = readAuth(root)
      const raw = typeof v?.writingMode === 'string' ? v.writingMode.trim() : ''
      if (raw === 'cowrite') return 'cowrite'
      if (raw === 'ghostwrite') return 'ghostwrite'
      if (raw === 'assist') return 'assist'
    } catch {
      // 文件坏了退回环境变量，不要让权限判断崩掉
    }
  }
  const env = (process.env.ZHIBI_WRITE_MODE ?? '').trim()
  if (env === 'cowrite') return 'cowrite'
  if (env === 'ghostwrite') return 'ghostwrite'
  return 'assist'
}

function readAuth(root: string): any {
  const fs = process.getBuiltinModule('node:fs')
  const path = process.getBuiltinModule('node:path')
  const text = fs.readFileSync(path.join(root, 'zhibi-auth.json'), 'utf8')
  return JSON.parse(text)
}

/**
 * 作者对哪些章节授权了“代笔”。
 *
 * 存在书目录的 zhibi-auth.json：`{ "ghostwriteChapters": [3, 4] }`。
 * 读不到或坏掉一律当“没授权”——授权必须是显式的。
 */
export function ghostwriteChapters(root: string): number[] {
  try {
    const arr = readAuth(root)?.ghostwriteChapters
    if (!Array.isArray(arr)) return []
    return arr.filter((n: unknown) => typeof n === 'number') as number[]
  } catch {
    return []
  }
}

export interface WritePermission {
  allowed: boolean
  /** 拒绝时给模型看的原因（要具体，模型才知道下一步该做什么）。 */
  reason?: string
}

/**
 * 能否直接写台账类文件（storylines/events/clues/canon/memory/characters/relations 等）。
 * 共写与代笔档可以，辅助档不行。
 */
export function canWriteLedger(root?: string): WritePermission {
  const mode = currentMode(root)
  if (mode === 'assist') {
    return {
      allowed: false,
      reason:
        '当前是「辅助」档：AI 不能直接改台账。请改用 propose_* 工具登记提案，由作者采纳。',
    }
  }
  return { allowed: true }
}

/**
 * 能否直接写某章正文。只有代笔档 + 作者显式授权的章节才可以。
 */
export function canWriteProse(root: string, chapter: number): WritePermission {
  const mode = currentMode(root)
  if (mode === 'assist') {
    return {
      allowed: false,
      reason: '当前是「辅助」档：正文只能由作者落笔。改写请用 offer_rewrite，由作者采纳。',
    }
  }
  if (mode === 'cowrite') {
    return {
      allowed: false,
      reason:
        '当前是「共写」档：正文仍由作者落笔（可用 offer_rewrite / offer_continuation 给建议）。要直接写正文需切到「代笔」档并由作者授权该章。',
    }
  }
  const allowed = ghostwriteChapters(root)
  if (!allowed.includes(chapter)) {
    return {
      allowed: false,
      reason: `第${chapter}章没有代笔授权（已授权：${
        allowed.length ? allowed.join('、') : '无'
      }）。请作者在书里授权该章，或改用 offer_rewrite 给建议。`,
    }
  }
  return { allowed: true }
}
