// 执笔能力包：以 DSH Cordis 插件形态提供 .zhibi 项目工具面。
//
// 铁律：AI 只产提案（propose_* → proposals/inbox.json），作者采纳（apply_proposal，
// 幂等保护）才落库；不存在任何直接写正文的工具。
//
// 零运行时依赖：工具定义按 dsh-tools 的 ToolDefinition 形状手写（parameters/output
// 为原生 JSON Schema，render 返回 text 块），不 import DSH 包——避免双实例服务漂移。
// 类型仅从 @deepseek-ai/cordis 以 type-only 引入（Node 24 类型剥离后无运行时代价）。

import { writeFileSync } from 'node:fs'
import type { Context } from '@deepseek-ai/cordis'
import { NovelStore, safeFileName, type Proposal } from './store.ts'

export const name = 'zhibi-novel-pack'

/** 依赖的 Cordis 服务：工具注册表（ctx.tools）。logger 为内建。 */
export const inject = ['tools']

// —— ToolDefinition（与 @deepseek-ai/dsh-tools 的形状一致的手写版）——

interface ToolDef {
  name: string
  description: string
  parameters: Record<string, unknown>
  output: {
    schema: Record<string, unknown>
    render: (args: unknown, value: unknown) => Array<{ type: string; text: string }>
  }
  execute: (args: any, exec: { signal: AbortSignal }) => Promise<unknown>
}

function textTool(
  name: string,
  description: string,
  parameters: Record<string, unknown>,
  run: (args: any) => Promise<string>,
): ToolDef {
  return {
    name,
    description,
    parameters,
    output: {
      schema: { type: 'string' },
      render: (_args, value) => [{ type: 'text', text: String(value) }],
    },
    execute: async (args) => run(args),
  }
}

const str = (description: string, required = true) => ({ type: 'string', description, required })
const num = (description: string, required = true) => ({ type: 'number', description, required })

// —— 能力包 ——

export function apply(ctx: Context, config: { root?: string } = {}): void {
  const root = config.root ?? process.env.ZHIBI_ROOT
  if (!root) {
    // 没配项目根：只提示，不崩（面板/冒烟阶段允许无项目运行）
    ctx.logger.info('zhibi-novel-pack 未配置项目根（config.root 或环境变量 ZHIBI_ROOT），工具将返回未打开项目')
    const store = new NovelStore('/nonexistent')
    registerTools(ctx, store, true)
    return
  }
  const store = new NovelStore(root)
  void store.ensureDirs()
  registerTools(ctx, store, false)
}

function registerTools(ctx: Context, store: NovelStore, closed: boolean): void {
  const guard = async (run: () => Promise<string>): Promise<string> => {
    if (closed) return '未打开执笔项目（配置 config.root 或 ZHIBI_ROOT）——只读与提案工具不可用。'
    try {
      return await run()
    } catch (err) {
      return `工具执行失败：${err instanceof Error ? err.message : String(err)}`
    }
  }

  const tools: ToolDef[] = [
    // —— 只读查询 ——

    textTool(
      'get_chapter',
      '读取某章正文（作者亲笔）。用于验证时对照原文取证。',
      { chapter: num('章节号') },
      (args) =>
        guard(async () => {
          const prose = await store.chapterProse(Number(args.chapter))
          if (prose === null || prose.trim() === '') return `第${args.chapter}章还没有正文。`
          return prose.slice(0, 12000)
        }),
    ),

    textTool(
      'get_clues',
      '读取伏笔台账全文。',
      {},
      () =>
        guard(async () => {
          const clues = await store.clues()
          if (clues.length === 0) return '台账为空。'
          return clues
            .map((c: any) =>
              [
                `[${c.id ?? '?'}]`,
                c.title ?? '?',
                c.status ?? '?',
                `埋于第${c.plantedChapter ?? '?'}章`,
                `最近动作第${c.lastActionChapter ?? '?'}章`,
                c.detail ?? '',
              ].join('｜'),
            )
            .join('\n')
        }),
    ),

    textTool(
      'get_outline',
      '读取故事线 + 时间线事件（含作者真相/读者已知）。',
      {},
      () =>
        guard(async () => {
          const o = await store.outline()
          const lines = Array.isArray(o['storylines']) ? (o['storylines'] as any[]) : []
          const events = Array.isArray(o['events']) ? (o['events'] as any[]) : []
          const linesText = lines
            .map((l) => `[${l.id}] ${l.name}（${l.kind ?? ''}${l.isThroughLine ? '·贯穿' : ''}）`)
            .join('\n')
          const eventsText = events
            .map(
              (e) =>
                `[${e.id}] 第${e.chapter}章 真相：${e.objectiveFact}｜读者已知：${e.revealed ? e.readerKnowledge : '未揭示'}`,
            )
            .join('\n')
          return (
            `故事线：\n${linesText || '（空）'}\n事件：\n${eventsText || '（空）'}`
          )
        }),
    ),

    textTool(
      'get_canon',
      '读取全部设定文档（标题 + 内容）。设定是作者主权：不得臆造，先读再写。',
      {},
      () =>
        guard(async () => {
          const titles = await store.canonTitles()
          if (titles.length === 0) return '设定库为空。'
          const parts: string[] = []
          for (const t of titles.slice(0, 24)) {
            const doc = await store.canonDoc(t)
            parts.push(`《${t}》\n${(doc ?? '').slice(0, 1600)}`)
          }
          return parts.join('\n\n')
        }),
    ),

    textTool(
      'list_proposals',
      '列出当前待作者裁决的提案（id/标题/能力）。',
      {},
      () =>
        guard(async () => {
          const list = await store.proposals()
          const pending = list.filter((p) => p.status === '待确认')
          if (pending.length === 0) return '没有待审提案。'
          return pending
            .map((p: Proposal) => `[${p.id}] ${p.title}（${p.capability}）`)
            .join('\n')
        }),
    ),

    // —— 提案（铁律落点：只登记，不落库） ——

    textTool(
      'propose_canon',
      '提交设定文档提案（世界观/势力人物/修炼体系/题材规则等背景框架）。每篇是 Markdown，可以含表格；每篇聚焦一个主题，合计不超过 6 篇。作者审阅采纳后才写入设定库。',
      {
        docs: {
          type: 'array',
          description: '设定文档数组',
          required: true,
          items: {
            type: 'object',
            properties: {
              title: str('文档标题'),
              content: str('Markdown 正文'),
              certainty: {
                type: 'string',
                description: 'canon=已定正典 tentative=暂定 blank=有意留白',
              },
            },
            required: ['title', 'content'],
          },
        },
      },
      (args) =>
        guard(async () => {
          const docs: any[] = Array.isArray(args.docs) ? args.docs : []
          if (docs.length === 0) return '没有可登记的文档。'
          const p = await store.addProposal({
            capability: '搭建框架',
            title: `背景设定框架（${docs.length} 篇）`,
            note: '设定主权在作者：采纳后写入设定库，同题不覆盖手改过的文档。',
            payload: { kind: 'canon', docs },
          })
          return `已登记设定提案 ${docs.length} 篇（${p.id}），等待作者审阅。`
        }),
    ),

    textTool(
      'propose_clues',
      '提交伏笔提案（埋点登记）。F 编号可省略（采纳时自动分配）；种下原文尽量摘自正文。',
      {
        clues: {
          type: 'array',
          description: '伏笔数组',
          required: true,
          items: {
            type: 'object',
            properties: {
              id: str('编号（可省略，自动分配 F 序号）', false),
              title: str('一句话标题'),
              detail: str('详情：埋的是什么、如何回收'),
              plantedChapter: num('埋设章节号'),
            },
            required: ['title', 'detail'],
          },
        },
      },
      (args) =>
        guard(async () => {
          const clues: any[] = Array.isArray(args.clues) ? args.clues : []
          if (clues.length === 0) return '没有可登记的伏笔。'
          const p = await store.addProposal({
            capability: '线索埋点台账',
            title: `伏笔盘点（${clues.length} 条）`,
            note: '采纳后入台账；同 id 合并，作者维护的字段不被覆盖。',
            payload: { kind: 'clues', clues },
          })
          return `已登记伏笔提案 ${clues.length} 条（${p.id}），等待作者审阅。`
        }),
    ),

    textTool(
      'propose_canon_classify',
      '提交设定归类提案：对每篇设定文档给出分类与可选的规范化改名。按现有标题精确匹配；改名撞名会被跳过。',
      {
        classifications: {
          type: 'array',
          description: '归类条目数组',
          required: true,
          items: {
            type: 'object',
            properties: {
              title: str('现有文档标题，逐字一致'),
              category: str('分类：世界观/势力人物/修炼体系/剧情大纲/题材规则/未分类'),
              rename: str('可选：规范化后的新标题', false),
              note: str('可选：理由', false),
            },
            required: ['title', 'category'],
          },
        },
      },
      (args) =>
        guard(async () => {
          const items: any[] = Array.isArray(args.classifications) ? args.classifications : []
          if (items.length === 0) return '没有可登记的归类。'
          const p = await store.addProposal({
            capability: '设定归类',
            title: `设定归类（${items.length} 篇）`,
            note: '采纳后设定页按类别分组展示。',
            payload: { kind: 'canonClassify', items },
          })
          return `已登记归类提案 ${items.length} 条（${p.id}），等待作者审阅。`
        }),
    ),

    // —— 确定性采纳（作者点“采纳”后由宿主调用；幂等保护） ——

    textTool(
      'apply_proposal',
      '【宿主专用】按作者裁决执行一条提案：canon 同题保护、clues 同 id 合并、归类按标题匹配。只有提案状态为“待确认”时执行。',
      { proposalId: str('提案 id') },
      (args) =>
        guard(async () => {
          const list = await store.proposals()
          const p = list.find((x) => x.id === String(args.proposalId))
          if (!p) return `提案不存在：${args.proposalId}`
          if (p.status !== '待确认') return `提案已处理（${p.status}），拒绝重复采纳。`
          const applied = await applyPayload(store, p)
          await store.updateProposal(p.id, { status: '已采纳' })
          return applied
        }),
    ),
  ]

  for (const t of tools) {
    ctx.tools.register(t)
  }
  // 冒烟标记：证明本插件被真实挂载且全部注册成功（stderr 无失败即已通过）
  try {
    writeFileSync('/tmp/np-loaded.json', JSON.stringify({ tools: tools.map((t) => t.name) }, null, 2))
  } catch {}
  ctx.logger.info(`zhibi-novel-pack 已注册 ${tools.length} 个工具（root=${store.root}）`)
}

// —— 提案落库（与执笔 Swift applyPayloadDetailed 语义一致） ——

async function applyPayload(store: NovelStore, p: Proposal): Promise<string> {
  const payload = p.payload ?? {}
  switch (payload.kind) {
    case 'canon': {
      const docs: any[] = Array.isArray(payload.docs) ? payload.docs : []
      let applied = 0
      const skipped: string[] = []
      for (const d of docs) {
        const title = safeFileName(String(d.title ?? '').trim())
        if (!title) continue
        if (await store.canonDoc(title)) {
          skipped.push(`「${d.title}」已存在（同题保护）`)
          continue
        }
        await store.writeText(joinCanon(title), String(d.content ?? ''))
        applied += 1
      }
      return `设定入库 ${applied}/${docs.length}${skipped.length ? '；跳过：' + skipped.join('、') : ''}`
    }
    case 'clues': {
      const incoming: any[] = Array.isArray(payload.clues) ? payload.clues : []
      const clues = await store.clues()
      let applied = 0
      for (const c of incoming) {
        const id = String(c.id ?? `F${clues.length + 1}`)
        const existingIdx = clues.findIndex((x: any) => x.id === id)
        if (existingIdx >= 0) {
          // 同 id 合并：台账字段以作者维护为准，只补空
          const existing: any = clues[existingIdx]
          for (const k of ['title', 'detail', 'plantedChapter']) {
            if (existing[k] === undefined && c[k] !== undefined) existing[k] = c[k]
          }
          applied += 1
        } else {
          clues.push({ status: '埋下', lastActionChapter: c.plantedChapter, ...c, id })
          applied += 1
        }
      }
      await store.writeJSON('clues.json', clues)
      return `伏笔入账 ${applied}/${incoming.length}`
    }
    case 'canonClassify': {
      const items: any[] = Array.isArray(payload.items) ? payload.items : []
      let applied = 0
      const skipped: string[] = []
      for (const item of items) {
        const title = safeFileName(String(item.title ?? '').trim())
        const doc = await store.canonDoc(title)
        if (doc === null) {
          skipped.push(`「${item.title}」未找到`)
          continue
        }
        // 分类写进文档首行 metadata 注释（M3-zero 简化格式；M2 迁 canon meta）
        const category = String(item.category ?? '未分类')
        let body = doc
        if (body.startsWith('<!-- 类别:')) {
          body = body.replace(/^<!-- 类别:.*?-->\n/, '')
        }
        await store.writeText(joinCanon(title), `<!-- 类别:${category} -->\n${body}`)
        applied += 1
      }
      return `归类写回 ${applied}/${items.length}${skipped.length ? '；跳过：' + skipped.join('、') : ''}`
    }
    default:
      return `未知提案类型：${payload.kind ?? '（空）'}`
  }
}

function joinCanon(title: string): string {
  return `canon/${safeFileName(title)}.md`
}
