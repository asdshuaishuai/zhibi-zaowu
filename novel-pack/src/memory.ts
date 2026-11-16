// 记忆召回与推断（TS 侧）。与 app/src/memory.rs 是同一套规则的两份实现，
// 同读 tests/conformance/cases.json 的 memoryRecall / memoryInfer 族。
// 打分全用整数，避免两端浮点舍入漂移；排序用码位序（BMP 内与 Rust 字节序一致）。

export interface Fact {
  id: string
  subject: string
  predicate: string
  object: string
  chapter: number
  public: boolean
}

export interface AliasEntry {
  canonicalName?: string
  aliases?: string[]
}

export function factFrom(v: any, idx: number): Fact {
  return {
    id: typeof v?.id === 'string' ? v.id : `f${idx}`,
    subject: typeof v?.subject === 'string' ? v.subject : '',
    predicate: typeof v?.predicate === 'string' ? v.predicate : '',
    object: typeof v?.object === 'string' ? v.object : '',
    chapter: Number(v?.fromChapter ?? 0) || 0,
    public: v?.publicToReader !== false,
  }
}

export function aliasMap(aliases: AliasEntry[]): Array<[string, string]> {
  const pairs: Array<[string, string]> = []
  for (const a of aliases ?? []) {
    const canon = a?.canonicalName ?? ''
    if (!canon) continue
    for (const al of a?.aliases ?? []) {
      if (al && al !== canon) pairs.push([al, canon])
    }
  }
  pairs.sort((x, y) => (x[0] < y[0] ? -1 : x[0] > y[0] ? 1 : x[1] < y[1] ? -1 : x[1] > y[1] ? 1 : 0))
  const seen = new Set<string>()
  return pairs.filter(([al, c]) => {
    const k = al + '\u0000' + c
    if (seen.has(k)) return false
    seen.add(k)
    return true
  })
}

export function normalize(name: string, map: Array<[string, string]>): string {
  for (const [al, canon] of map) if (al === name) return canon
  return name
}

export interface RecallQuery {
  chapter?: number
  terms?: string[]
  limit?: number
}

export interface RecallHit {
  fact: Fact
  score: number
  reasons: string[]
}

const PROX_WINDOW = 20

export function recall(facts: any[], aliases: AliasEntry[], q: RecallQuery): RecallHit[] {
  const map = aliasMap(aliases)
  const parsed = (facts ?? []).map((v, i) => factFrom(v, i))
  const maxCh = Math.max(1, ...parsed.map((f) => f.chapter))
  const limit = q.limit && q.limit > 0 ? q.limit : 12

  const hits: RecallHit[] = parsed.map((f) => {
    const s = normalize(f.subject, map)
    const o = normalize(f.object, map)
    let score = 0
    const reasons: string[] = []
    for (const t of q.terms ?? []) {
      if (!t) continue
      const tn = normalize(t, map)
      if (tn === s || tn === o) {
        score += 300
        reasons.push('主宾命中')
      } else if (f.predicate.includes(t) || o.includes(t) || s.includes(t)) {
        score += 200
        reasons.push('谓宾包含')
      } else if (s.includes(tn) || o.includes(tn)) {
        score += 100
        reasons.push('别名包含')
      }
    }
    if (q.chapter != null) {
      const dist = Math.abs(f.chapter - q.chapter)
      const prox = Math.floor((200 * (PROX_WINDOW - Math.min(dist, PROX_WINDOW))) / PROX_WINDOW)
      score += prox
      if (prox > 0) reasons.push('章距近')
    }
    score += Math.floor((100 * f.chapter) / maxCh)
    return { fact: f, score, reasons }
  })

  const cmp = (a: RecallHit, b: RecallHit) =>
    b.score - a.score ||
    b.fact.chapter - a.fact.chapter ||
    (a.fact.subject < b.fact.subject ? -1 : a.fact.subject > b.fact.subject ? 1 : 0)
  hits.sort(cmp)

  const dedup: RecallHit[] = []
  const keyOf = (h: RecallHit) =>
    normalize(h.fact.subject, map) + '\u0000' + h.fact.predicate + '\u0000' + normalize(h.fact.object, map)
  const seen = new Set<string>()
  for (const h of hits) {
    const k = keyOf(h)
    if (seen.has(k)) continue
    seen.add(k)
    dedup.push(h)
  }
  dedup.sort(
    (a, b) =>
      cmp(a, b) ||
      (a.fact.predicate < b.fact.predicate ? -1 : a.fact.predicate > b.fact.predicate ? 1 : 0) ||
      (a.fact.object < b.fact.object ? -1 : a.fact.object > b.fact.object ? 1 : 0),
  )
  return dedup.slice(0, limit)
}

export interface Implied {
  subject: string
  predicate: string
  object: string
  rule: string
  basis: string[]
}
export interface Contradiction {
  subject: string
  predicate: string
  objects: string[]
  chapters: number[]
}
export interface Inference {
  implied: Implied[]
  contradictions: Contradiction[]
  stale: string[]
}

const SYMMETRIC = ['夫妻', '结义', '同门']
const SINGLE_VALUED = ['夫妻', '配偶', '生父', '生母']
const DURABLE = ['师从', '生父', '生母', '出生']
const STALE_WINDOW = 30

/** 推断规则——书级可配。`memory.json` 里可含 `inferRules` 块覆盖默认。 */
export interface InferRules {
  symmetric: string[]
  singleValued: string[]
  durable: string[]
  staleWindow: number
}

export function defaultInferRules(): InferRules {
  return {
    symmetric: [...SYMMETRIC],
    singleValued: [...SINGLE_VALUED],
    durable: [...DURABLE],
    staleWindow: STALE_WINDOW,
  }
}

/** 从 `memory.json` 的 `inferRules` 块读配置；缺块或字段缺失时用默认值补。 */
export function inferRulesFromMemoryJson(mem: Record<string, unknown>): InferRules {
  const r = defaultInferRules()
  const blk = mem['inferRules']
  if (!blk || typeof blk !== 'object' || Array.isArray(blk)) return r
  const o = blk as Record<string, unknown>
  const take = (key: string, target: string[]) => {
    if (Array.isArray(o[key])) {
      return (o[key] as unknown[]).filter((x): x is string => typeof x === 'string')
    }
    return target
  }
  r.symmetric = take('symmetric', r.symmetric)
  r.singleValued = take('singleValued', r.singleValued)
  r.durable = take('durable', r.durable)
  if (typeof o['staleWindow'] === 'number' && Number.isFinite(o['staleWindow'])) {
    r.staleWindow = o['staleWindow'] as number
  }
  return r
}

export function infer(facts: any[], aliases: AliasEntry[], currentChapter: number): Inference {
  return inferWithRules(facts, aliases, currentChapter, defaultInferRules())
}

export function inferWithRules(
  facts: any[],
  aliases: AliasEntry[],
  currentChapter: number,
  rules: InferRules,
): Inference {
  const map = aliasMap(aliases)
  const parsed = (facts ?? []).map((v, i) => factFrom(v, i))
  const out: Inference = { implied: [], contradictions: [], stale: [] }

  const masters = new Map<string, Fact[]>()
  for (const f of parsed) {
    if (f.predicate !== '师从') continue
    const o = normalize(f.object, map)
    if (!masters.has(o)) masters.set(o, [])
    masters.get(o)!.push(f)
  }
  for (const group of masters.values()) {
    for (let i = 0; i < group.length; i++) {
      for (let j = i + 1; j < group.length; j++) {
        const a = normalize(group[i].subject, map)
        const b = normalize(group[j].subject, map)
        if (a === b) continue
        const [x, y] = a < b ? [a, b] : [b, a]
        const basis = [group[i].id, group[j].id].sort()
        if (!out.implied.some((m) => m.subject === x && m.object === y && m.predicate === '同门')) {
          out.implied.push({ subject: x, predicate: '同门', object: y, rule: '同师推导', basis })
        }
      }
    }
  }

  for (const f of parsed) {
    if (!rules.symmetric.includes(f.predicate)) continue
    const a = normalize(f.subject, map)
    const b = normalize(f.object, map)
    if (a === b || !a || !b) continue
    const hasRev = parsed.some(
      (g) => g.predicate === f.predicate && normalize(g.subject, map) === b && normalize(g.object, map) === a,
    )
    if (!hasRev && !out.implied.some((m) => m.subject === b && m.object === a && m.predicate === f.predicate)) {
      out.implied.push({ subject: b, predicate: f.predicate, object: a, rule: '对称补全', basis: [f.id] })
    }
  }

  const groups = new Map<string, Fact[]>()
  for (const f of parsed) {
    if (!rules.singleValued.includes(f.predicate)) continue
    const s = normalize(f.subject, map)
    const k = s + '\u0000' + f.predicate
    if (!groups.has(k)) groups.set(k, [])
    groups.get(k)!.push(f)
  }
  for (const [k, group] of groups) {
    const [s, p] = k.split('\u0000')
    const objs = [...new Set(group.map((f) => normalize(f.object, map)))].sort()
    if (objs.length >= 2) {
      const chapters = [...new Set(group.map((f) => f.chapter))].sort((a, b) => a - b)
      out.contradictions.push({ subject: s, predicate: p, objects: objs, chapters })
    }
  }

  if (currentChapter > 0) {
    for (const f of parsed) {
      if (f.chapter > 0 && currentChapter - f.chapter > rules.staleWindow && !rules.durable.includes(f.predicate)) {
        out.stale.push(f.id)
      }
    }
  }

  out.implied.sort(
    (a, b) =>
      (a.subject < b.subject ? -1 : a.subject > b.subject ? 1 : 0) ||
      (a.predicate < b.predicate ? -1 : a.predicate > b.predicate ? 1 : 0) ||
      (a.object < b.object ? -1 : a.object > b.object ? 1 : 0),
  )
  out.contradictions.sort(
    (a, b) =>
      (a.subject < b.subject ? -1 : a.subject > b.subject ? 1 : 0) ||
      (a.predicate < b.predicate ? -1 : a.predicate > b.predicate ? 1 : 0),
  )
  out.stale = [...new Set(out.stale)].sort()
  return out
}