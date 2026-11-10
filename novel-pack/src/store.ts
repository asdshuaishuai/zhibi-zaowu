// .zhibi 项目存取（与执笔 SwiftUI 版的磁盘格式逐字节兼容）：
//   <root>/
//   ├── project.json          # 书目（title/genre/premise/targetChapters…）
//   ├── canon/<safe-title>.md # 设定（Markdown，文件名即标题的安全形式）
//   ├── outline.json          # { storylines, events, stages }
//   ├── clues.json            # 伏笔台账数组
//   ├── memory.json           # { facts, characterAliases }
//   ├── worldlines.json       # 预言书沙盒（M7）：推演只写这里，晋升才进提案
//   ├── canvas.json           # 画板视图态（节点位置/平移缩放/读者视角）
//   ├── proposals/inbox.json  # AI 提案流水（前端/AI 双端共用，铁律落点）
//   └── chapters/ch-NNN/{meta.json, prose.md}
//
// M3-zero 范围：通用 JSON 读写 + 提案登记/采纳/列表。写全部走“读改写”原子替换。

import { mkdir, readFile, readdir, rename, writeFile } from 'node:fs/promises'
import { existsSync } from 'node:fs'
import { dirname, join } from 'node:path'

export function safeFileName(s: string): string {
  // 与 Rust safe_file_name 对齐：空/全非法标题统一落到「未命名作品」
  return s.replace(/[/\\:*?"<>|\n\r\t]/g, '_').trim() || '未命名作品'
}

export function chapterDirName(n: number): string {
  return `ch-${String(n).padStart(3, '0')}`
}

/** 提案流水条目（字段名与执笔 Swift 版 AIProposal 一致，camelCase） */
export interface Proposal {
  id: string
  capability: string
  chapterNumber?: number
  title: string
  note: string
  payload: { kind: string; [k: string]: unknown }
  status: '待确认' | '已采纳' | '已拒绝'
  createdAt: string
  decidedAt?: string
  rejectReason?: string
  /** 采纳结果快照：已决历史重启后仍能回放实际入库/跳过情况（Rust/TS 双端字段兼容）。 */
  applyResult?: {
    applied: number
    message?: string
    skipped?: string[]
    at?: string
  }
}

export class NovelStore {
  readonly root: string
  /** 提案箱读改写串行化：并发 propose 不能互相覆盖。 */
  private proposalWriteTail: Promise<void> = Promise.resolve()

  constructor(root: string) {
    this.root = root
  }

  private serializeProposalWrite<T>(fn: () => Promise<T>): Promise<T> {
    const run = this.proposalWriteTail.then(fn)
    this.proposalWriteTail = run.then(
      () => undefined,
      () => undefined,
    )
    return run
  }

  private path(...parts: string[]): string {
    return join(this.root, ...parts)
  }

  async ensureDirs(): Promise<void> {
    for (const d of ['canon', 'proposals', 'chapters']) {
      await mkdir(this.path(d), { recursive: true })
    }
    // 预言书沙盒：α 正典基准行（与执笔 app 端 create/open 同语义，幂等）
    const wl = this.path('worldlines.json')
    if (!existsSync(wl)) {
      await writeFile(
        wl,
        JSON.stringify(
          [
            { id: 'α', name: '正典', premise: '当前事实基准', forkedChapter: 0, status: '基准', nodes: [], edges: [], diffs: [], derived: { characters: [], canon: [], clues: [] } },
          ],
          null,
          2,
        ),
        'utf8',
      )
    }
  }

  async readJSON<T>(rel: string, fallback: T): Promise<T> {
    const p = this.path(rel)
    if (!existsSync(p)) return fallback
    try {
      return JSON.parse(await readFile(p, 'utf8')) as T
    } catch (err) {
      // 坏 JSON 隔离：用户数据宁可空出来也不要崩
      return fallback
    }
  }

  /** 写路径严格读：文件不存在返回 undefined；坏 JSON 抛错，避免拿空值覆盖用户台账。 */
  async readJSONChecked<T>(rel: string): Promise<T | undefined> {
    const p = this.path(rel)
    if (!existsSync(p)) return undefined
    return JSON.parse(await readFile(p, 'utf8')) as T
  }

  async writeJSON(rel: string, value: unknown): Promise<void> {
    await this.ensureDirs()
    const p = this.path(rel)
    await mkdir(dirname(p), { recursive: true }) // 任意嵌套父目录（snapshots/ 等）
    const tmp = p + '.tmp'
    await writeFile(tmp, JSON.stringify(value, null, 2), 'utf8')
    await rename(tmp, p)
  }

  async readText(rel: string): Promise<string | null> {
    const p = this.path(rel)
    return existsSync(p) ? readFile(p, 'utf8') : null
  }

  async writeText(rel: string, text: string): Promise<void> {
    await this.ensureDirs()
    const p = this.path(rel)
    await mkdir(dirname(p), { recursive: true })
    const tmp = p + '.tmp'
    await writeFile(tmp, text, 'utf8')
    await rename(tmp, p)
  }

  // —— 章节 ——

  async chapterProse(n: number): Promise<string | null> {
    return this.readText(join('chapters', chapterDirName(n), 'prose.md'))
  }

  async chapterTitle(n: number): Promise<string | null> {
    const meta = await this.readJSON<Record<string, unknown>>(
      join('chapters', chapterDirName(n), 'meta.json'),
      {},
    )
    const title = meta['title']
    return typeof title === 'string' ? title : null
  }

  async writtenChapters(): Promise<number[]> {
    const dir = this.path('chapters')
    if (!existsSync(dir)) return []
    const out: number[] = []
    for (const name of await readdir(dir)) {
      const m = /^ch-(\d{3,})$/.exec(name)
      if (!m) continue
      // 与 Rust last_written_chapter 同口径：空 prose.md 不算成稿。
      const prose = await readFile(join(dir, name, 'prose.md'), 'utf8').catch(() => null)
      if (prose !== null && prose.trim() !== '') out.push(Number(m[1]))
    }
    return out.sort((a, b) => a - b)
  }

  // —— 设定 ——

  async canonTitles(): Promise<string[]> {
    const dir = this.path('canon')
    if (!existsSync(dir)) return []
    return (await readdir(dir))
      .filter((f) => f.endsWith('.md'))
      .map((f) => f.slice(0, -3))
  }

  async canonDoc(title: string): Promise<string | null> {
    // 空标题不是合法查询：不能落到「未命名作品.md」上
    const name = title.trim()
    if (!name) return null
    return this.readText(join('canon', `${safeFileName(name)}.md`))
  }

  /** 同目录改名（设定归类规范化），目标存在与否由调用方先判。 */
  async moveText(fromRel: string, toRel: string): Promise<void> {
    const from = this.path(fromRel)
    const to = this.path(toRel)
    await mkdir(dirname(to), { recursive: true })
    await rename(from, to)
  }

  // —— 台账 ——

  async clues(): Promise<unknown[]> {
    return this.readJSON<unknown[]>('clues.json', [])
  }

  async outline(): Promise<Record<string, unknown>> {
    // 真实磁盘布局：storylines.json / events.json / stages.json 三文件分储
    //（outline.json 为旧兼容路径，存在则合并兜底）
    const legacy = await this.readJSON<Record<string, unknown>>('outline.json', {})
    const storylines = await this.readJSON<unknown[]>('storylines.json', (Array.isArray(legacy['storylines']) ? legacy['storylines'] : []))
    const events = await this.readJSON<unknown[]>('events.json', (Array.isArray(legacy['events']) ? legacy['events'] : []))
    const stages = await this.readJSON<unknown[]>('stages.json', (Array.isArray(legacy['stages']) ? legacy['stages'] : []))
    return { storylines, events, stages }
  }

  async saveStorylines(lines: unknown[]): Promise<void> {
    await this.writeJSON('storylines.json', lines)
  }

  // —— 预言书沙盒（M7）——

  async worldlines(): Promise<any[]> {
    return this.readJSON<any[]>('worldlines.json', [])
  }

  async saveWorldlines(lines: any[]): Promise<void> {
    await this.writeJSON('worldlines.json', lines)
  }

  // —— 提案流水 ——

  /** 只读展示：宽松读取，坏 JSON 当空。写路径一律用 proposalsChecked。 */
  async proposals(): Promise<Proposal[]> {
    return this.readJSON<Proposal[]>(join('proposals', 'inbox.json'), [])
  }

  /** 写路径严格读：坏 JSON / 顶层不是数组直接拒绝，避免把整本提案历史覆盖成空表。 */
  async proposalsChecked(): Promise<Proposal[]> {
    const v = await this.readJSONChecked<unknown>(join('proposals', 'inbox.json'))
    if (v === undefined) return []
    if (!Array.isArray(v)) throw new Error('proposals/inbox.json 顶层不是数组，已拒绝覆盖')
    return v as Proposal[]
  }

  async addProposal(p: Omit<Proposal, 'id' | 'status' | 'createdAt'>): Promise<Proposal> {
    return this.serializeProposalWrite(async () => {
      const list = await this.proposalsChecked()
      const full: Proposal = {
        ...p,
        id: crypto.randomUUID(),
        status: '待确认',
        createdAt: new Date().toISOString(),
      }
      list.unshift(full)
      await this.writeJSON(join('proposals', 'inbox.json'), list)
      return full
    })
  }

  async updateProposal(id: string, patch: Partial<Proposal>): Promise<Proposal | null> {
    return this.serializeProposalWrite(async () => {
      const list = await this.proposalsChecked()
      const idx = list.findIndex((p) => p.id === id)
      if (idx < 0) return null
      list[idx] = { ...list[idx], ...patch, decidedAt: new Date().toISOString() }
      await this.writeJSON(join('proposals', 'inbox.json'), list)
      return list[idx]
    })
  }
}
