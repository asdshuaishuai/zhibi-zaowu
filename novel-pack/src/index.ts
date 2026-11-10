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

    // —— M3 补齐：提示词引用的其余提案工具 ——

    textTool(
      'propose_skeleton',
      '提交章节骨架提案：4-6 节拍（建议字数/场景层/章尾钩子/伏笔合同）。骨架是写前契约，作者批准前不给写草稿。',
      {
        chapter: num('章节号'),
        beats: {
          type: 'array',
          description: '节拍数组',
          required: true,
          items: {
            type: 'object',
            properties: {
              title: str('节拍名'),
              summary: str('这拍发生什么'),
              suggestedWords: num('建议字数', false),
              hook: str('章尾钩子（挂具体物）', false),
            },
            required: ['title', 'summary'],
          },
        },
      },
      (args) =>
        guard(async () => {
          const beats = Array.isArray(args.beats) ? args.beats : [];
          if (beats.length === 0) return '没有可登记的节拍。';
          const p = await store.addProposal({
            capability: '章节骨架',
            chapterNumber: Number(args.chapter),
            title: `第${args.chapter}章 骨架（${beats.length} 拍）`,
            note: '批准后解锁写草稿；采纳前正文不动。',
            payload: { kind: 'skeleton', chapter: Number(args.chapter), beats },
          });
          return `已登记骨架提案（${p.id}），等待作者批准。`
        }),
    ),

    textTool(
      'propose_draft',
      '提交整章草稿提案。草稿只能以提案形态存在：作者采纳前不进正文，采纳时自动快照可回滚。',
      { chapter: num('章节号'), text: str('草稿全文（Markdown）') },
      (args) =>
        guard(async () => {
          const text = String(args.text ?? '');
          if (text.trim().length < 50) return '草稿太短，不登记。'
          const p = await store.addProposal({
            capability: '一键写作',
            chapterNumber: Number(args.chapter),
            title: `第${args.chapter}章 草稿 v1（${text.length} 字）`,
            note: '采纳 = 自动快照后写入正文；拒绝 = 正文一字不动。',
            payload: { kind: 'draft', chapter: Number(args.chapter), text },
          });
          return `已登记草稿提案（${p.id}），等待作者审阅。`
        }),
    ),

    textTool(
      'propose_validation',
      '提交一致性验证报告提案：只查客观错误（连续性/设定违背/伏笔合同/物理不可能/动机断裂），只报告不代改。',
      { chapter: num('章节号'), findings: { type: 'array', description: '发现数组（每条含类型/位置/证据/建议）', required: true, items: { type: 'object', properties: { kind: str('类型'), location: str('位置'), evidence: str('证据'), suggestion: str('建议') }, required: ['kind', 'evidence'] } } },
      (args) =>
        guard(async () => {
          const findings = Array.isArray(args.findings) ? args.findings : [];
          const p = await store.addProposal({
            capability: '一致性验证',
            chapterNumber: Number(args.chapter),
            title: `第${args.chapter}章 验证报告（${findings.length} 处）`,
            note: '报告只读；配套修复动作走伏笔/大纲提案。',
            payload: { kind: 'report', chapter: Number(args.chapter), findings },
          });
          return `已登记验证报告提案（${p.id}）。`
        }),
    ),

    textTool(
      'propose_deslop',
      '提交去AI味逐处建议提案：original/replacement/gate/reason，逐条采纳，首次采纳前自动快照。',
      { chapter: num('章节号'), suggestions: { type: 'array', required: true, description: '逐处建议', items: { type: 'object', properties: { original: str('原文片段'), replacement: str('建议替换'), reason: str('为什么') }, required: ['original', 'replacement'] } } },
      (args) =>
        guard(async () => {
          const suggestions = Array.isArray(args.suggestions) ? args.suggestions : [];
          const p = await store.addProposal({
            capability: '去AI味',
            chapterNumber: Number(args.chapter),
            title: `第${args.chapter}章 去AI味建议（${suggestions.length} 处）`,
            note: '逐条采纳；采纳前自动快照。',
            payload: { kind: 'deslop', chapter: Number(args.chapter), suggestions },
          });
          return `已登记去AI味提案 ${suggestions.length} 处（${p.id}）。`
        }),
    ),

    textTool(
      'propose_memory',
      '提交记忆提案：双时态事实三元组 + 章摘要 + 伏笔候选。作者确认后入记忆中枢。',
      { chapter: num('章节号'), facts: { type: 'array', required: true, description: '事实数组', items: { type: 'object', properties: { subject: str('主语'), predicate: str('谓语'), object: str('宾语'), publicToReader: { type: 'boolean', description: '读者是否已知' } }, required: ['subject', 'predicate', 'object'] } } },
      (args) =>
        guard(async () => {
          const facts = Array.isArray(args.facts) ? args.facts : [];
          const p = await store.addProposal({
            capability: '记忆提取',
            chapterNumber: Number(args.chapter),
            title: `第${args.chapter}章 记忆（${facts.length} 事实）`,
            note: '采纳后事实进记忆中枢，矛盾体检自动核对。',
            payload: { kind: 'memoryPack', chapter: Number(args.chapter), facts },
          });
          return `已登记记忆提案（${p.id}）。`
        }),
    ),

    textTool(
      'propose_dossier',
      '提交章节档案卡提案（长程记忆）：场景/出场/推进/拐点/悬念/关键细节。采纳后进跨全书检索库。',
      { chapter: num('章节号'), headline: str('一句话本章'), scenes: { type: 'array', items: { type: 'string' }, description: '场景', required: false }, cast: { type: 'array', items: { type: 'string' }, description: '出场', required: false }, hooks: { type: 'array', items: { type: 'string' }, description: '悬念', required: false }, relics: { type: 'array', items: { type: 'string' }, description: '检索锚点（关键物件/台词）', required: false } },
      (args) =>
        guard(async () => {
          const p = await store.addProposal({
            capability: '档案卡提取',
            chapterNumber: Number(args.chapter),
            title: `第${args.chapter}章 档案卡（长程记忆）`,
            note: '采纳后进跨全书检索记忆库。',
            payload: { kind: 'dossier', chapter: Number(args.chapter), headline: String(args.headline ?? ''), scenes: args.scenes ?? [], cast: args.cast ?? [], hooks: args.hooks ?? [], relics: args.relics ?? [] },
          });
          return `已登记档案卡提案（${p.id}）。`
        }),
    ),

    textTool(
      'propose_clue_fixes',
      '提交埋点修复提案：每条烂账配一个可一键采纳的修复动作（补埋/改期/回收/搁置/放弃/校正原文/补登记）。只改台账不动正文。',
      { fixes: { type: 'array', required: true, description: '修复动作数组', items: { type: 'object', properties: { clueID: str('伏笔编号'), kind: str('动作类型'), detail: str('说明') }, required: ['clueID', 'kind'] } } },
      (args) =>
        guard(async () => {
          const fixes = Array.isArray(args.fixes) ? args.fixes : [];
          const p = await store.addProposal({
            capability: '全书连贯性审查',
            title: `埋点修复（${fixes.length} 条）`,
            note: '只改台账与动作日志，不动正文。',
            payload: { kind: 'clueFixes', fixes },
          });
          return `已登记埋点修复提案 ${fixes.length} 条（${p.id}）。`
        }),
    ),

    textTool(
      'propose_outline_updates',
      '提交大纲更新提案：剧情线状态（健康/放缓/停滞/断线/待收束）与事件偏移的更新建议。采纳后大纲才是活文档。',
      { updates: { type: 'array', required: true, description: '更新数组', items: { type: 'object', properties: { storylineID: str('故事线编号'), status: str('新状态', false), note: str('建议说明') }, required: ['note'] } } },
      (args) =>
        guard(async () => {
          const updates = Array.isArray(args.updates) ? args.updates : [];
          const p = await store.addProposal({
            capability: '大纲同步',
            title: `大纲更新提案（${updates.length} 条）`,
            note: '采纳后故事线状态真实变化。',
            payload: { kind: 'outlineUpdates', updates },
          });
          return `已登记大纲更新提案（${p.id}）。`
        }),
    ),

    // —— M7 预言书：世界线沙盒（铁律延伸：推演只写沙盒，晋升才进提案箱） ——

    textTool(
      'get_world_state',
      '读取正典世界的紧凑图状态（故事线/事件/伏笔/设定/人物别名 + 现有世界线摘要）。画板节点与预言书推演都以此为唯一事实源；推演前先调它。',
      {},
      () =>
        guard(async () => {
          const o = await store.outline()
          const lines = Array.isArray(o['storylines']) ? (o['storylines'] as any[]) : []
          const events = Array.isArray(o['events']) ? (o['events'] as any[]) : []
          const clues = await store.clues()
          const titles = await store.canonTitles()
          const mem = await store.readJSON<Record<string, unknown>>('memory.json', {})
          const aliases = Array.isArray(mem['aliases']) ? (mem['aliases'] as any[]) : []
          const wls = await store.worldlines()
          return JSON.stringify(
            {
              storylines: lines.map((l) => ({ id: l.id, name: l.name, status: l.status ?? '', kind: l.kind ?? '', isThroughLine: !!l.isThroughLine })),
              events: events.map((e) => ({
                id: e.id,
                chapter: e.chapter,
                fact: String(e.objectiveFact ?? '').slice(0, 80),
                revealed: !!e.revealed,
                storylineIDs: e.storylineIDs ?? [],
              })),
              clues: clues.map((c: any) => ({ id: c.id, title: c.title, status: c.status, plantedChapter: c.plantedChapter, lastActionChapter: c.lastActionChapter })),
              canon: titles.map((t) => ({ title: t })),
              characters: aliases.map((a: any) => ({ canonical: a.canonicalName ?? '?', aka: (a.aliases ?? []).join('、') })),
              worldlines: wls.map((w) => ({ id: w.id, name: w.name, premise: w.premise, forkedChapter: w.forkedChapter, status: w.status, nodes: (w.nodes ?? []).length, diffs: (w.diffs ?? []).length, promoted: (w.diffs ?? []).filter((d: any) => d.promoted).length })),
            },
            null,
            1,
          )
        }),
    ),

    textTool(
      'propose_worldline',
      '开一条世界线（预言书沙盒）：记名称/假设/分歧章。只写 worldlines.json 沙盒，绝不触碰正典。id 自动取希腊字母序（α 为正典基准，不可创建）。',
      { name: str('世界线名，如「师兄未死」'), premise: str('一句假设（如果…）'), forkedChapter: num('分歧章号') },
      (args) =>
        guard(async () => {
          const wls = await store.worldlines()
          // 自愈：确保 α 基准行在（裸工具/未走 ensureDirs 时也不缺基准）
          if (!wls.some((w) => w.id === 'α')) {
            wls.unshift({ id: 'α', name: '正典', premise: '当前事实基准', forkedChapter: 0, status: '基准', nodes: [], edges: [], diffs: [], derived: { characters: [], canon: [], clues: [] } })
          }
          const greek = ['β', 'γ', 'δ', 'ε', 'ζ', 'η', 'θ', 'ι', 'κ', 'λ', 'μ', 'ν', 'ξ', 'ο', 'π']
          let id = ''
          for (const g of greek) {
            if (!wls.some((w) => w.id === g)) { id = g; break }
          }
          if (!id) return '世界线沙盒已满（15 条），先归档旧的。'
          const w = {
            id,
            name: String(args.name ?? '').trim() || '未命名',
            premise: String(args.premise ?? '').trim(),
            forkedChapter: Number(args.forkedChapter) || 0,
            status: '推演中',
            createdAt: new Date().toISOString(),
            nodes: [],
            edges: [],
            diffs: [],
            derived: { characters: [], canon: [], clues: [] },
          }
          wls.push(w)
          await store.saveWorldlines(wls)
          return `已开世界线 ${id}「${w.name}」（fork 第${w.forkedChapter}章）——沙盒已建，接下来用 simulate_worldline 写入推演结果。`
        }),
    ),

    textTool(
      'simulate_worldline',
      '把推演结果写入世界线沙盒（覆盖式）：节点/边/影响清单 diffs/衍生实体。铁律：diff 只描述「对正典的影响」，本身不改正典；晋升用 promote_diff。',
      {
        worldlineId: str('世界线 id，如 β'),
        nodes: { type: 'array', required: false, description: '推演节点', items: { type: 'object', properties: { id: str('节点id'), kind: str('storyline|event|character|clue|canon'), label: str('标题'), ref: str('指向正典对象id（可空）', false), x: num('画板横坐标', false), y: num('画板纵坐标', false) }, required: ['id', 'kind', 'label'] } },
        edges: { type: 'array', required: false, description: '推演边', items: { type: 'object', properties: { from: str('源节点id'), to: str('目标节点id'), kind: str('influence|plant|payoff|involve|conflict|hiddenline'), hidden: { type: 'boolean', description: '暗线（读者视角隐藏）', required: false } }, required: ['from', 'to', 'kind'] } },
        diffs: { type: 'array', required: true, description: '对正典的影响清单', items: { type: 'object', properties: { target: str('设定|伏笔|事件|故事线|人物'), targetRef: str('正典对象标识（标题/编号）'), kind: str('add|change'), before: str('原状（add 填「—」）'), after: str('推演后的新状态'), reason: str('为何连锁至此'), severity: str('high|medium|low', false) }, required: ['target', 'targetRef', 'kind', 'after'] } },
        derived: { type: 'object', required: false, description: '本线衍生（自动标 line 归属）', properties: { characters: { type: 'array', items: { type: 'string' }, required: false }, canon: { type: 'array', items: { type: 'string' }, required: false }, clues: { type: 'array', items: { type: 'string' }, required: false } } },
      },
      (args) =>
        guard(async () => {
          const wls = await store.worldlines()
          const w = wls.find((x) => x.id === String(args.worldlineId))
          if (!w) return `世界线不存在：${args.worldlineId}（先 propose_worldline）`
          w.nodes = Array.isArray(args.nodes) ? args.nodes : []
          w.edges = Array.isArray(args.edges) ? args.edges : []
          w.diffs = (Array.isArray(args.diffs) ? args.diffs : []).map((d: any) => ({ ...d, promoted: false, severity: d.severity ?? 'medium' }))
          const dv = args.derived ?? {}
          w.derived = {
            characters: (Array.isArray(dv.characters) ? dv.characters : []).map((s: string) => `${s}（line:${w.id}）`),
            canon: (Array.isArray(dv.canon) ? dv.canon : []).map((s: string) => `${s}（line:${w.id}）`),
            clues: (Array.isArray(dv.clues) ? dv.clues : []).map((s: string) => `${s}（line:${w.id}）`),
          }
          w.status = '已推演'
          w.updatedAt = new Date().toISOString()
          await store.saveWorldlines(wls)
          return `世界线 ${w.id} 推演入档：节点 ${w.nodes.length} · 边 ${w.edges.length} · 影响 ${w.diffs.length} · 衍生已标 line:${w.id}。正典未动；等作者逐条晋升。`
        }),
    ),

    textTool(
      'promote_diff',
      '晋升一条世界线影响为正式提案（进收件箱，作者裁决后才改正典）。映射：设定add→canon 提案；设定change→canonRevise 提案（采纳时自动备份原文）；伏笔→clues 提案；事件/故事线→outlineUpdates 提案。同一条只能晋升一次。',
      { worldlineId: str('世界线 id'), diffIndex: num('diffs 数组下标（从 0 起）') },
      (args) =>
        guard(async () => {
          const wls = await store.worldlines()
          const w = wls.find((x) => x.id === String(args.worldlineId))
          if (!w) return `世界线不存在：${args.worldlineId}`
          const idx = Number(args.diffIndex)
          const d = (w.diffs ?? [])[idx]
          if (!d) return `世界线 ${w.id} 第 ${idx} 条影响不存在`
          if (d.promoted) return `该影响已晋升（提案 ${d.promotedProposalId ?? ''}），拒绝重复。`
          const after = String(d.after ?? '')
          const reason = String(d.reason ?? '')
          let p
          switch (String(d.target)) {
            case '设定':
              if (d.kind === 'add') {
                p = await store.addProposal({ capability: '预言书·晋升', title: `新设定《${d.targetRef}》（线${w.id}）`, note: `世界线 ${w.id}「${w.name}」推演晋升：${reason}`, payload: { kind: 'canon', docs: [{ title: String(d.targetRef), content: after }] } })
              } else {
                p = await store.addProposal({ capability: '预言书·晋升', title: `修订设定《${d.targetRef}》（线${w.id}）`, note: `世界线 ${w.id} 晋升修订；采纳前自动备份原文。理由：${reason}`, payload: { kind: 'canonRevise', items: [{ title: String(d.targetRef), content: after }] } })
              }
              break
            case '伏笔':
              p = await store.addProposal({ capability: '预言书·晋升', title: `伏笔入账（线${w.id}）`, note: `晋升自世界线推演：${reason}`, payload: { kind: 'clues', clues: [{ title: String(d.targetRef), detail: after, plantedChapter: w.forkedChapter }] } })
              break
            default:
              p = await store.addProposal({ capability: '预言书·晋升', title: `大纲更新（线${w.id}）`, note: `晋升自世界线推演 ${d.target}·${d.targetRef}：${reason}`, payload: { kind: 'outlineUpdates', updates: [{ storylineID: String(d.targetRef), note: after }] } })
          }
          d.promoted = true
          d.promotedProposalId = p.id
          await store.saveWorldlines(wls)
          return `影响已晋升为提案 ${p.id}（收件箱待裁决）。正典仍未动。`
        }),
    ),

    textTool(
      'archive_worldline',
      '归档世界线（沙盒只读封存，可随时解档继续推演）。',
      { worldlineId: str('世界线 id') },
      (args) =>
        guard(async () => {
          const wls = await store.worldlines()
          const w = wls.find((x) => x.id === String(args.worldlineId))
          if (!w) return `世界线不存在：${args.worldlineId}`
          w.status = w.status === '已归档' ? '已推演' : '已归档'
          await store.saveWorldlines(wls)
          return `世界线 ${w.id} 现为「${w.status}」。`
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
    case 'canonRevise': {
      // 预言书晋升：修订既有设定——采纳前先把原文备份到 canon/backups/
      const items: any[] = Array.isArray(payload.items) ? payload.items : []
      let applied = 0
      const skipped: string[] = []
      for (const d of items) {
        const title = safeFileName(String(d.title ?? '').trim())
        if (!title) continue
        const doc = await store.canonDoc(title)
        if (doc === null) {
          skipped.push(`「${d.title}」未找到（同题校验）`)
          continue
        }
        const stamp = new Date().toISOString().replace(/[:.]/g, '-').slice(0, 19)
        await store.writeText(`canon/backups/${title}.${stamp}.md`, doc)
        await store.writeText(joinCanon(title), String(d.content ?? ''))
        applied += 1
      }
      return `设定修订 ${applied}/${items.length}${skipped.length ? '；跳过：' + skipped.join('、') : ''}（已自动备份）`
    }
    case 'outlineUpdates': {
      const updates: any[] = Array.isArray(payload.updates) ? payload.updates : []
      const o = await store.outline()
      const lines = Array.isArray(o['storylines']) ? (o['storylines'] as any[]) : []
      let applied = 0
      const skipped: string[] = []
      for (const u of updates) {
        const l = lines.find((x) => x.id === String(u.storylineID))
        if (!l) {
          skipped.push(`[${u.storylineID}] 未找到`)
          continue
        }
        if (u.status) l.status = String(u.status)
        l.syncNote = String(u.note ?? '')
        l.syncAt = new Date().toISOString()
        applied += 1
      }
      await store.saveStorylines(lines)
      return `大纲同步 ${applied}/${updates.length}${skipped.length ? '；跳过：' + skipped.join('、') : ''}`
    }
    default:
      return `未知提案类型：${payload.kind ?? '（空）'}`
  }
}

function joinCanon(title: string): string {
  return `canon/${safeFileName(title)}.md`
}
