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
  return s.replace(/[/\\:*?"<>|\n\r\t]/g, '_').trim() || '未命名'
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
}

export class NovelStore {
  readonly root: string

  constructor(root: string) {
    this.root = root
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
      if (m && existsSync(join(dir, name, 'prose.md'))) {
        out.push(Number(m[1]))
      }
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
    return this.readText(join('canon', `${safeFileName(title)}.md`))
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

  async proposals(): Promise<Proposal[]> {
    return this.readJSON<Proposal[]>(join('proposals', 'inbox.json'), [])
  }

  async addProposal(p: Omit<Proposal, 'id' | 'status' | 'createdAt'>): Promise<Proposal> {
    const list = await this.proposals()
    const full: Proposal = {
      ...p,
      id: crypto.randomUUID(),
      status: '待确认',
      createdAt: new Date().toISOString(),
    }
    list.unshift(full)
    await this.writeJSON(join('proposals', 'inbox.json'), list)
    return full
  }

  async updateProposal(id: string, patch: Partial<Proposal>): Promise<Proposal | null> {
    const list = await this.proposals()
    const idx = list.findIndex((p) => p.id === id)
    if (idx < 0) return null
    list[idx] = { ...list[idx], ...patch, decidedAt: new Date().toISOString() }
    await this.writeJSON(join('proposals', 'inbox.json'), list)
    return list[idx]
  }
}
