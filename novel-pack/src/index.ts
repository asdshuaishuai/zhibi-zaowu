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
const sourceRefs = () => ({
  type: 'array',
  description: '依据来源：本提案引用的既有伏笔/设定/事实编号（如 F03 / 设定:灯约 / 沈砚·获得·断刀），无则留空',
  required: false,
  items: { type: 'string' },
})

// —— 能力包 ——

export function apply(
  ctx: Context,
  config: { root?: string; hostApply?: boolean; hostApplyToken?: string } = {},
): void {
  const root = config.root ?? process.env.ZHIBI_ROOT
  // apply_proposal 是作者裁决入口，默认不对外开放：宿主须显式开 hostApply（或
  // ZHIBI_HOST_APPLY=1）并给令牌，AI 侧拿不到令牌就无法自批。
  const hostApplyEnabled = config.hostApply ?? (process.env.ZHIBI_HOST_APPLY === '1')
  const hostApplyToken = hostApplyEnabled
    ? (config.hostApplyToken ?? process.env.ZHIBI_HOST_APPLY_TOKEN ?? '')
    : ''
  if (!root) {
    // 没配项目根：只提示，不崩（面板/冒烟阶段允许无项目运行）
    ctx.logger.info('zhibi-novel-pack 未配置项目根（config.root 或环境变量 ZHIBI_ROOT），工具将返回未打开项目')
    const store = new NovelStore('/nonexistent')
    registerTools(ctx, store, true, '')
    return
  }
  const store = new NovelStore(root)
  void store.ensureDirs()
  registerTools(ctx, store, false, hostApplyToken)
}

function registerTools(ctx: Context, store: NovelStore, closed: boolean, hostApplyToken: string): void {
  // 失败必须在协议层可见：抛出（不把异常伪装成正常文本结果），模型才能区分失败与结果。
  const guard = async (run: () => Promise<string>): Promise<string> => {
    if (closed) throw new Error('未打开执笔项目（配置 config.root 或 ZHIBI_ROOT）——工具不可用。')
    try {
      return await run()
    } catch (err) {
      if (err instanceof Error) throw err
      throw new Error(`工具执行失败：${String(err)}`)
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
      '读取故事线 + 时间线事件（含作者真相/读者已知）+ 最近档案卡（推进/拐点/悬念）。',
      {},
      () =>
        guard(async () => {
          const o = await store.outline()
          const lines = Array.isArray(o['storylines']) ? (o['storylines'] as any[]) : []
          const events = Array.isArray(o['events']) ? (o['events'] as any[]) : []
          const dossiers = await store.readJSON<any[]>('dossiers.json', [])
          const linesText = lines
            .map((l) => `[${l.id}] ${l.name}（${l.kind ?? ''}${l.isThroughLine ? '·贯穿' : ''}）`)
            .join('\n')
          const eventsText = events
            .map((e) => {
              const known =
                typeof e.readerKnowledge === 'string' && e.readerKnowledge.trim()
                  ? e.readerKnowledge
                  : e.revealed
                    ? '（已揭示，读者已知未填）'
                    : '未揭示'
              return `[${e.id}] 第${e.chapter ?? '?'}章 真相：${e.objectiveFact ?? '（空）'}｜读者已知：${known}`
            })
            .join('\n')
          const dossiersText = dossiers
            .slice(-12)
            .map((d) => {
              const bits = [`[第${d.chapter ?? '?'}章] ${d.headline ?? ''}`]
              if (Array.isArray(d.advanced) && d.advanced.length) bits.push(`推进：${d.advanced.join('；')}`)
              if (Array.isArray(d.turns) && d.turns.length) bits.push(`拐点：${d.turns.join('；')}`)
              if (Array.isArray(d.hooks) && d.hooks.length) bits.push(`悬念：${d.hooks.join('；')}`)
              return bits.join('｜')
            })
            .join('\n')
          return (
            `故事线：\n${linesText || '（空）'}\n事件：\n${eventsText || '（空）'}\n档案卡（最近 12 章）：\n${dossiersText || '（空）'}`
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
      '列出提案及状态。默认返回待审 + 已拒绝（含拒收理由，供修正后重提）；可按状态筛选。',
      {
        status: {
          type: 'string',
          description: 'pending=仅待审；rejected=仅已拒绝；all=全部；默认 pending+rejected',
          required: false,
        },
        limit: num('最多返回条数（默认 50）', false),
      },
      (args) =>
        guard(async () => {
          const list = await store.proposals()
          const mode = String(args.status ?? '').toLowerCase()
          let picked: Proposal[]
          if (mode === 'pending') picked = list.filter((p) => p.status === '待确认')
          else if (mode === 'rejected') picked = list.filter((p) => p.status === '已拒绝')
          else if (mode === 'all') picked = list
          else picked = list.filter((p) => p.status === '待确认' || p.status === '已拒绝')
          const limit = Math.max(1, Math.min(200, Number(args.limit) || 50))
          picked = picked.slice(0, limit)
          if (picked.length === 0) return mode === 'rejected' ? '没有被拒提案。' : '没有待审提案。'
          return picked
            .map((p: Proposal) => {
              const head = `[${p.id}] ${p.title}（${p.capability}·${p.payload?.kind ?? '?'}）｜${p.status}`
              const reason =
                p.status === '已拒绝' && p.rejectReason ? `｜拒收理由：${p.rejectReason}` : ''
              return head + reason
            })
            .join('\n')
        }),
    ),

    // —— 提案（铁律落点：只登记，不落库） ——

    textTool(
      'propose_canon',
      '提交设定文档提案（世界观/势力人物/修炼体系/题材规则等背景框架）。每篇是 Markdown，可以含表格；每篇聚焦一个主题，合计不超过 6 篇，每篇用 sourceRefs 列依据来源。作者审阅采纳后才写入设定库。',
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
              sourceRefs: sourceRefs(),
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
      '提交伏笔提案（埋点登记）。F 编号可省略（采纳时自动分配）；种下原文尽量摘自正文，每条用 sourceRefs 列依据来源。',
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
              sourceRefs: sourceRefs(),
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
      'propose_relations',
      '提交人物关系边提案：给定「谁—谓词—谁」（如 沈砚—师从—老周 / 钱九渊—敌对—沈砚），采纳后写入记忆事实，关系图与人物卡立即出现这条边。AI 可主动发现关系（读 get_outline/get_canon 后推断）或补全读者已知关系；作者可拒收、可继续用同一工具修正。',
      {
        relations: {
          type: 'array',
          description: '关系边数组',
          required: true,
          items: {
            type: 'object',
            properties: {
              from: str('主体（人名，本名优先）'),
              predicate: str('关系谓词：师从/父子/敌对/同乡/共谋/暗恋/主仆…（2-4 字）'),
              to: str('客体（人名）'),
              evidence: str('依据（哪章哪段；可为空）', false),
              chapter: num('首次成立章号（可空）', false),
              publicToReader: { type: 'boolean', description: '读者是否已知（暗线填 false）', required: false },
            },
            required: ['from', 'predicate', 'to'],
          },
        },
      },
      (args) =>
        guard(async () => {
          const relations: any[] = Array.isArray(args.relations) ? args.relations : []
          if (relations.length === 0) return '没有可登记的关系边。'
          const p = await store.addProposal({
            capability: '人物关系',
            title: `人物关系 ${relations.length} 条`,
            note: '采纳后入记忆事实（subject=from / predicate / object=to），关系图与人物卡立即挂边。',
            payload: { kind: 'relations', relations },
          })
          return `已登记人物关系提案 ${relations.length} 条（${p.id}），等待作者审阅。`
        }),
    ),

    textTool(
      'propose_aliases',
      '提交人物别名归一提案：同一人物的本名与各种称谓（小名/绰号/敬称）归并到 canonicalName。作者采纳后进 memory.json.aliases——人物卡、画布人物维/关系维以它为事实源；同名归并、别名去重，作者维护的字段不覆盖。',
      {
        aliases: {
          type: 'array',
          description: '别名档案数组',
          required: true,
          items: {
            type: 'object',
            properties: {
              canonicalName: str('本名（文档内统一指称）'),
              aliases: { type: 'array', items: { type: 'string' }, description: '别名列表（不含本名本身）', required: true },
              note: str('可选：依据（哪一章哪种称呼）', false),
            },
            required: ['canonicalName', 'aliases'],
          },
        },
      },
      (args) =>
        guard(async () => {
          const aliases: any[] = Array.isArray(args.aliases) ? args.aliases : []
          if (aliases.length === 0) return '没有可登记的别名档案。'
          const p = await store.addProposal({
            capability: '人物别名归一',
            title: `人物别名档案（${aliases.length} 人）`,
            note: '采纳后进 memory.json.aliases；同名归并、别名去重。',
            payload: { kind: 'aliases', aliases },
          })
          return `已登记别名提案 ${aliases.length} 人（${p.id}），等待作者审阅。`
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
        sourceRefs: sourceRefs(),
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
            payload: {
              kind: 'skeleton',
              chapter: Number(args.chapter),
              beats,
              sourceRefs: Array.isArray(args.sourceRefs) ? args.sourceRefs : [],
            },
          });
          return `已登记骨架提案（${p.id}），等待作者批准。`
        }),
    ),

    textTool(
      'propose_draft',
      '提交整章草稿提案。草稿只能以提案形态存在：作者采纳前不进正文，采纳时自动快照可回滚。',
      { chapter: num('章节号'), text: str('草稿全文（Markdown）'), sourceRefs: sourceRefs() },
      (args) =>
        guard(async () => {
          const text = String(args.text ?? '');
          if (text.trim().length < 50) return '草稿太短，不登记。'
          const p = await store.addProposal({
            capability: '一键写作',
            chapterNumber: Number(args.chapter),
            title: `第${args.chapter}章 草稿 v1（${text.length} 字）`,
            note: '采纳 = 自动快照后写入正文；拒绝 = 正文一字不动。',
            payload: {
              kind: 'draft',
              chapter: Number(args.chapter),
              text,
              sourceRefs: Array.isArray(args.sourceRefs) ? args.sourceRefs : [],
            },
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
          // 空报告不得登记：采纳会写 meta.report=[]，把上一份报告清空。
          if (findings.length === 0) return '没有可登记的发现，未登记报告。';
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
      '提交去AI味逐处建议提案：original/replacement/reason，逐条采纳，首次采纳前自动快照。',
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
      { chapter: num('章节号'), facts: { type: 'array', required: true, description: '事实数组', items: { type: 'object', properties: { subject: str('主语'), predicate: str('谓语'), object: str('宾语'), publicToReader: { type: 'boolean', description: '读者是否已知' } }, required: ['subject', 'predicate', 'object'] } }, sourceRefs: sourceRefs() },
      (args) =>
        guard(async () => {
          const facts = Array.isArray(args.facts) ? args.facts : [];
          const p = await store.addProposal({
            capability: '记忆提取',
            chapterNumber: Number(args.chapter),
            title: `第${args.chapter}章 记忆（${facts.length} 事实）`,
            note: '采纳后事实进记忆中枢，矛盾体检自动核对。',
            payload: {
              kind: 'memoryPack',
              chapter: Number(args.chapter),
              facts,
              sourceRefs: Array.isArray(args.sourceRefs) ? args.sourceRefs : [],
            },
          });
          return `已登记记忆提案（${p.id}）。`
        }),
    ),

    textTool(
      'propose_dossier',
      '提交章节档案卡提案（长程记忆）：场景/出场/推进（advanced）/拐点（turns）/悬念/关键细节。采纳后进跨全书检索库。',
      {
        chapter: num('章节号'),
        headline: str('一句话本章'),
        scenes: { type: 'array', items: { type: 'string' }, description: '场景', required: false },
        cast: { type: 'array', items: { type: 'string' }, description: '出场', required: false },
        advanced: {
          type: 'array',
          items: { type: 'string' },
          description: '推进：本章对主线/人物关系的实质推进（逐条一句话）',
          required: false,
        },
        turns: {
          type: 'array',
          items: { type: 'string' },
          description: '拐点：本章的反转/认知颠覆/关系断裂（逐条一句话，转折画布直接消费）',
          required: false,
        },
        hooks: { type: 'array', items: { type: 'string' }, description: '悬念', required: false },
        relics: { type: 'array', items: { type: 'string' }, description: '检索锚点（关键物件/台词）', required: false },
        sourceRefs: sourceRefs(),
      },
      (args) =>
        guard(async () => {
          const headline = String(args.headline ?? '').trim()
          const contentKeys = ['scenes', 'cast', 'advanced', 'turns', 'hooks', 'relics'] as const
          const hasContent =
            headline !== '' ||
            contentKeys.some((k) => Array.isArray(args[k]) && (args[k] as unknown[]).length > 0)
          if (!hasContent) return '档案卡没有可登记内容（headline 与六类条目均空）。'
          const p = await store.addProposal({
            capability: '档案卡提取',
            chapterNumber: Number(args.chapter),
            title: `第${args.chapter}章 档案卡（长程记忆）`,
            note: '采纳后进跨全书检索记忆库。',
            payload: {
              kind: 'dossier',
              chapter: Number(args.chapter),
              headline: String(args.headline ?? ''),
              scenes: Array.isArray(args.scenes) ? args.scenes : [],
              cast: Array.isArray(args.cast) ? args.cast : [],
              advanced: Array.isArray(args.advanced) ? args.advanced : [],
              turns: Array.isArray(args.turns) ? args.turns : [],
              hooks: Array.isArray(args.hooks) ? args.hooks : [],
              relics: Array.isArray(args.relics) ? args.relics : [],
              sourceRefs: Array.isArray(args.sourceRefs) ? args.sourceRefs : [],
            },
          });
          return `已登记档案卡提案（${p.id}）。`
        }),
    ),

    textTool(
      'propose_clue_fixes',
      '提交埋点修复提案：每条烂账配一个可一键采纳的修复动作（补埋/改期/回收/搁置/放弃/校正原文/补登记）。只改台账不动正文。',
      { fixes: { type: 'array', required: true, description: '修复动作数组', items: { type: 'object', properties: { clueID: str('伏笔编号'), kind: str('动作类型'), detail: str('说明；补埋/改期可写「由第3章改到第9章」，校正原文时为新原文'), plantedChapter: num('补埋/改期：目标埋设章号（优先于 detail 解析）', false), toChapter: num('改期：目标章号（plantedChapter 的别名）', false) }, required: ['clueID', 'kind'] } } },
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
              worldlines: wls.map((w) => ({ id: w.id, name: w.name, kind: w.kind ?? 'what-if', premise: w.premise, forkedChapter: w.forkedChapter, status: w.status, nodes: (w.nodes ?? []).length, diffs: (w.diffs ?? []).length, promoted: (w.diffs ?? []).filter((d: any) => d.promoted).length })),
            },
            null,
            1,
          )
        }),
    ),

    textTool(
      'propose_worldline',
      '开一条世界线（预言书沙盒）：记名称/假设/分歧章。只写 worldlines.json 沙盒，绝不触碰正典。id 自动取希腊字母序（α 为正典基准，不可创建）。',
      { name: str('世界线名，如「师兄未死」'), premise: str('假设或预测方向（如果…/后续…）'), forkedChapter: num('分歧章号（forecast 线可传 0=自动取最新成稿下一章）'), kind: { type: 'string', description: 'what-if=假设推演（默认）forecast=预测线（基于大纲/成稿向前织）', required: false } },
      (args) =>
        guard(async () => {
          const wls = await store.worldlines()
          // 自愈：确保 α 基准行在（裸工具/未走 ensureDirs 时也不缺基准）
          if (!wls.some((w) => w.id === 'α')) {
            wls.unshift({ id: 'α', name: '正典', premise: '当前事实基准', forkedChapter: 0, status: '基准', kind: 'base', nodes: [], edges: [], diffs: [], derived: { characters: [], canon: [], clues: [] } })
          }
          const kind = String(args.kind ?? 'what-if') === 'forecast' ? 'forecast' : 'what-if'
          let fork = Number(args.forkedChapter) || 0
          if (kind === 'forecast' && fork === 0) {
            const written = await store.writtenChapters()
            fork = (written.length ? written[written.length - 1] : 0) + 1
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
            kind,
            forkedChapter: fork,
            status: '推演中',
            createdAt: new Date().toISOString(),
            nodes: [],
            edges: [],
            diffs: [],
            derived: { characters: [], canon: [], clues: [] },
          }
          wls.push(w)
          await store.saveWorldlines(wls)
          return `已开${kind === 'forecast' ? '预测线' : '假设线'} ${id}「${w.name}」（fork 第${w.forkedChapter}章）——沙盒已建，接下来用 simulate_worldline 写入推演结果。`
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
          // 覆盖式推演不能把已晋升的 diff 洗回可晋升：按 target/targetRef/kind/after 认回旧条目。
          const promotedPool = (Array.isArray(w.diffs) ? w.diffs : []).filter((d: any) => d?.promoted)
          let kept = 0
          const diffKey = (d: any) =>
            [d?.target ?? '', d?.targetRef ?? '', d?.kind ?? '', d?.after ?? ''].map(String).join('|')
          w.diffs = (Array.isArray(args.diffs) ? args.diffs : []).map((d: any) => {
            const i = promotedPool.findIndex((old: any) => diffKey(old) === diffKey(d))
            if (i >= 0) {
              const old = promotedPool.splice(i, 1)[0]
              kept += 1
              return {
                ...d,
                promoted: true,
                promotedProposalId: old.promotedProposalId,
                scheduledChapter: old.scheduledChapter,
                severity: d.severity ?? old.severity ?? 'medium',
              }
            }
            return { ...d, promoted: false, severity: d.severity ?? 'medium' }
          })
          // 新推演不再列出已晋升条目时也必须保留：否则 promote_diff 的已晋升闸门
          // 会被覆盖式推演洗掉，同一条影响可再次晋升。
          for (const old of promotedPool) w.diffs.push({ ...old, promoted: true })
          const dv = args.derived ?? {}
          w.derived = {
            characters: (Array.isArray(dv.characters) ? dv.characters : []).map((s: string) => `${s}（line:${w.id}）`),
            canon: (Array.isArray(dv.canon) ? dv.canon : []).map((s: string) => `${s}（line:${w.id}）`),
            clues: (Array.isArray(dv.clues) ? dv.clues : []).map((s: string) => `${s}（line:${w.id}）`),
          }
          w.status = '已推演'
          w.updatedAt = new Date().toISOString()
          await store.saveWorldlines(wls)
          const keptNote = kept ? ` · 保留既晋升 ${kept} 条` : ''
          return `世界线 ${w.id} 推演入档：节点 ${w.nodes.length} · 边 ${w.edges.length} · 影响 ${w.diffs.length}${keptNote} · 衍生已标 line:${w.id}。正典未动；等作者逐条晋升。`
        }),
    ),

    textTool(
      'promote_diff',
      '晋升一条世界线影响为正式提案（进收件箱，作者裁决后才改正典）。映射：设定add→canon 提案；设定change→canonRevise 提案（采纳时自动备份原文）；伏笔→clues 提案；事件/故事线→outlineUpdates 提案。同一条只能晋升一次。',
      { worldlineId: str('世界线 id'), diffIndex: num('diffs 数组下标（从 0 起）'), targetChapter: num('可选：排期章号——事件/故事线 add 型 diff 带章晋升走 plannedEvent（计划事件）提案', false) },
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
          const schedChap = Number(args.targetChapter) || 0
          let p
          if (schedChap > 0 && (d.target === '事件' || d.target === '故事线') && d.kind === 'add') {
            // M8-3 排期晋升：计划事件提案（采纳才入 events.json，happened=false）
            p = await store.addProposal({ capability: '预言书·排期', title: `计划事件 → 第${schedChap}章（线${w.id}）`, note: `预测排期晋升：${after}`, payload: { kind: 'plannedEvent', chapter: schedChap, title: after, storylineID: String(d.targetRef ?? ''), from: `线${w.id}`, sourceRefs: [`线${w.id}`, `故事线:${d.targetRef}`] } })
            d.promoted = true
            d.scheduledChapter = schedChap
          } else
          switch (String(d.target)) {
            case '设定':
              if (d.kind === 'add') {
                p = await store.addProposal({ capability: '预言书·晋升', title: `新设定《${d.targetRef}》（线${w.id}）`, note: `世界线 ${w.id}「${w.name}」推演晋升：${reason}`, payload: { kind: 'canon', docs: [{ title: String(d.targetRef), content: after }], sourceRefs: [`线${w.id}`, `设定:${d.targetRef}`] } })
              } else {
                p = await store.addProposal({ capability: '预言书·晋升', title: `修订设定《${d.targetRef}》（线${w.id}）`, note: `世界线 ${w.id} 晋升修订；采纳前自动备份原文。理由：${reason}`, payload: { kind: 'canonRevise', items: [{ title: String(d.targetRef), content: after }], sourceRefs: [`线${w.id}`, `设定:${d.targetRef}`] } })
              }
              break
            case '伏笔':
              p = await store.addProposal({ capability: '预言书·晋升', title: `伏笔入账（线${w.id}）`, note: `晋升自世界线推演：${reason}`, payload: { kind: 'clues', clues: [{ title: String(d.targetRef), detail: after, plantedChapter: w.forkedChapter }], sourceRefs: [`线${w.id}`, `伏笔:${d.targetRef}`] } })
              break
            case '人物':
              // 人物新增 → 别名档案提案（人物/关系维事实源）
              p = await store.addProposal({ capability: '预言书·晋升', title: `人物档案（线${w.id}）`, note: `晋升自世界线推演：${reason}`, payload: { kind: 'aliases', aliases: [{ canonicalName: String(d.targetRef), aliases: [] }], sourceRefs: [`线${w.id}`, `人物:${d.targetRef}`] } })
              break
            case '事件':
              if (d.kind === 'add' && !(Number(args.targetChapter) > 0)) {
                return '事件类新增影响请用「排期晋升」（带 targetChapter → plannedEvent），避免生成找不到故事线的空洞更新。'
              }
              p = await store.addProposal({ capability: '预言书·晋升', title: `大纲更新（线${w.id}）`, note: `晋升自世界线推演 ${d.target}·${d.targetRef}：${reason}`, payload: { kind: 'outlineUpdates', updates: [{ storylineID: String(d.targetRef), note: after }], sourceRefs: [`线${w.id}`, `故事线:${d.targetRef}`] } })
              break
            default:
              p = await store.addProposal({ capability: '预言书·晋升', title: `大纲更新（线${w.id}）`, note: `晋升自世界线推演 ${d.target}·${d.targetRef}：${reason}`, payload: { kind: 'outlineUpdates', updates: [{ storylineID: String(d.targetRef), note: after }], sourceRefs: [`线${w.id}`, `故事线:${d.targetRef}`] } })
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
      '【宿主专用】按作者裁决执行一条提案：canon 同题保护、clues 同 id 合并、归类按标题匹配。默认关闭；仅当宿主以 hostApply + hostApplyToken 启动时可用，AI 调用一律拒绝。只有提案状态为“待确认”时执行。',
      { proposalId: str('提案 id'), approvalToken: str('宿主审批令牌（hostApplyToken / ZHIBI_HOST_APPLY_TOKEN）') },
      (args) =>
        guard(async () => {
          if (!hostApplyToken) {
            throw new Error('apply_proposal 为宿主专用：未启用 hostApply 或未配置 hostApplyToken，工具面拒绝执行（作者采纳走 GUI 收件箱）。')
          }
          if (String(args.approvalToken ?? '') !== hostApplyToken) {
            throw new Error('apply_proposal 为宿主专用：approvalToken 不匹配。')
          }
          const list = await store.proposalsChecked()
          const p = list.find((x) => x.id === String(args.proposalId))
          if (!p) throw new Error(`提案不存在：${args.proposalId}`)
          if (p.status !== '待确认') throw new Error(`提案已处理（${p.status}），拒绝重复采纳。`)
          const result = await applyPayload(store, p)
          if (result.applied === 0) {
            throw new Error(`未入库：${result.message}——提案保持待审`)
          }
          await store.updateProposal(p.id, {
            status: '已采纳',
            applyResult: { applied: result.applied, message: result.message },
          })
          return result.message
        }),
    ),
    // —— 轻交互卡片：结构化产出，GUI 直接读参数，不再靠解析模型文本 ——
    //
    // 为什么用工具而不是“让模型输出【A】【B】【C】”：前端原来靠
    // contains('【') && contains("A】") 猜这是不是分支卡，普通聊天里出现同样的字符
    // 就会被误判。工具参数是结构化的，不会误判，也自带 callId 能与那一轮轨迹对上。

    textTool(
      'offer_branches',
      '给作者 3 个故事走向候选。每项 80-150 字正文，并标注走向名、情绪标签与牵动的伏笔/设定。作者点选后由宿主并入正文——你不是在写正文。',
      {
        branches: {
          type: 'array',
          description: '走向数组（建议 3 个）',
          required: true,
          items: {
            type: 'object',
            properties: {
              label: str('一句话走向名，如「复仇线」'),
              mood: str('情绪标签，如 压抑/昂扬/温情', false),
              text: str('这个走向的正文（80-150 字）'),
              touches: {
                type: 'array',
                items: { type: 'string' },
                description: '牵动的伏笔编号或设定名',
                required: false,
              },
            },
            required: ['label', 'text'],
          },
        },
        chapter: num('所属章节号', false),
      },
      (args) =>
        guard(async () => {
          const branches = Array.isArray(args.branches) ? args.branches : []
          if (branches.length === 0) return '没有可展示的走向。'
          const names = branches.map((b: any) => b.label).join(' / ')
          return `已展示 ${branches.length} 个走向（${names}），等作者点选。`
        }),
    ),

    textTool(
      'offer_rewrite',
      '给作者一段改写建议。original 必须逐字引自正文，宿主会做原文校验——对不上就不替换。只提交这一处，不要顺手改别处。',
      {
        original: str('原文（逐字摘自正文）'),
        replacement: str('改写后的文本'),
        kind: {
          type: 'string',
          description: 'deepen=拉深 / expand=扩写 / condense=缩写 / rewrite=改写 / deslop=去AI味',
          required: false,
        },
        chapter: num('所属章节号', false),
      },
      (args) =>
        guard(async () => {
          const original = String(args.original ?? '')
          const replacement = String(args.replacement ?? '')
          if (!original.trim() || !replacement.trim()) return '原文或改写为空，未登记。'
          const kind = String(args.kind ?? 'rewrite')
          return `已给出改写建议（${kind}）：${original.length} 字 → ${replacement.length} 字，等作者采纳。`
        }),
    ),

    textTool(
      'offer_continuation',
      '给作者一句续写（紧接正文最后一段，不超过 60 字）。作者采纳才落盘；不要调用其他工具，不要解释。',
      {
        text: str('续写的那一句'),
        chapter: num('所属章节号', false),
      },
      (args) =>
        guard(async () => {
          const text = String(args.text ?? '')
          if (!text.trim()) return '续写为空，未展示。'
          return `已给出续写（${text.length} 字），等作者采纳。`
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

/** payload 的 kind：显式字段优先；旧版（Swift 时代）payload 无 kind，按键签名推断（与 Rust 同序）。 */
/** 取数组：直接数组或旧版 `{"_0":[…]}` 包裹；keys 按优先序（与 Rust payload_array 同语义）。 */
function payloadArray(payload: Record<string, unknown>, keys: string[]): any[] {
  for (const key of keys) {
    const v = payload[key]
    if (Array.isArray(v)) return v
    if (v && typeof v === 'object') {
      const obj = v as Record<string, unknown>
      const ks = Object.keys(obj).sort((a, b) => {
        const na = Number(a.replace(/^_/, '')), nb = Number(b.replace(/^_/, ''))
        return (Number.isNaN(na) ? Number.MAX_SAFE_INTEGER : na) - (Number.isNaN(nb) ? Number.MAX_SAFE_INTEGER : nb)
      })
      const out: any[] = []
      for (const k of ks) {
        const arr = obj[k]
        if (Array.isArray(arr)) out.push(...arr)
      }
      if (out.length > 0) return out
    }
  }
  return []
}

function payloadKind(payload: Record<string, unknown>): string {
  const explicit = payload['kind']
  if (typeof explicit === 'string' && explicit.length > 0) return explicit
  const order: Array<[string, string]> = [
    ['clueFixes', 'clueFixes'],
    ['clues', 'clues'],
    ['docs', 'canon'],
    ['beats', 'skeleton'],
    ['findings', 'report'],
    ['suggestions', 'deslop'],
    ['facts', 'memoryPack'],
    ['aliases', 'aliases'],
    ['relations', 'relations'],
    ['updates', 'outlineUpdates'],
    ['outlineUpdates', 'outlineUpdates'],
    ['headline', 'dossier'],
    ['scenes', 'dossier'],
    ['relics', 'dossier'],
  ]
  for (const [k, kind] of order) {
    if (payload[k] !== undefined) return kind
  }
  if (Array.isArray(payload['items'])) {
    const items = payload['items'] as any[]
    return items.some((i) => i && typeof i === 'object' && 'category' in i) ? 'canonClassify' : 'canonRevise'
  }
  if (payload['text'] !== undefined) return 'draft'
  return ''
}

type ApplyResult = { applied: number; message: string }

/** 写路径严格读：坏 JSON 或类型不符直接拒绝，避免把用户台账覆盖成空表。 */
async function readArrayChecked(store: NovelStore, rel: string): Promise<any[]> {
  const v = await store.readJSONChecked<unknown>(rel)
  if (v === undefined) return []
  if (!Array.isArray(v)) throw new Error(`${rel} 顶层不是数组，已拒绝覆盖`)
  return v
}

async function readObjectChecked(store: NovelStore, rel: string): Promise<Record<string, unknown>> {
  const v = await store.readJSONChecked<unknown>(rel)
  if (v === undefined) return {}
  if (v === null || typeof v !== 'object' || Array.isArray(v)) throw new Error(`${rel} 顶层不是对象，已拒绝覆盖`)
  return v as Record<string, unknown>
}

async function applyPayload(store: NovelStore, p: Proposal): Promise<ApplyResult> {
  const payload = p.payload ?? {}
  switch (payloadKind(payload)) {
    case 'canon': {
      const docs: any[] = Array.isArray(payload.docs) ? payload.docs : []
      if (docs.length === 0) return { applied: 0, message: '没有可入库的设定文档' }
      let applied = 0
      const skipped: string[] = []
      for (const d of docs) {
        const rawTitle = String(d.title ?? '').trim()
        if (!rawTitle) continue
        const title = safeFileName(rawTitle)
        if ((await store.canonDoc(title)) !== null) {
          skipped.push(`「${rawTitle}」已存在（同题保护）`)
          continue
        }
        await store.writeText(joinCanon(title), String(d.content ?? ''))
        applied += 1
      }
      return { applied, message: `设定入库 ${applied}/${docs.length}${skipped.length ? '；跳过：' + skipped.join('、') : ''}` }
    }
    case 'clues': {
      const incoming: any[] = Array.isArray(payload.clues) ? payload.clues : []
      if (incoming.length === 0) return { applied: 0, message: '没有可入账的伏笔' }
      const clues = await readArrayChecked(store, 'clues.json')
      // 自动编号取现有 F 序号最大值 +1（不能 len+1：台账有洞会撞 id 走合并）
      // 与 Rust next_clue_id 同形：F1/F2/F10，不补前导零（同一台账不能混出 F04）。
      const nextClueId = () =>
        `F${(clues as any[]).reduce((m: number, x: any) => {
          const n = Number(String(x?.id ?? '').replace(/^F/, ''))
          return Number.isFinite(n) && n > m ? n : m
        }, 0) + 1}`
      let applied = 0
      for (const c of incoming) {
        const id = String(c.id ?? nextClueId())
        const existingIdx = clues.findIndex((x: any) => x.id === id)
        if (existingIdx >= 0) {
          // 同 id 合并：作者维护字段不覆盖；未知新字段（如 sourceRefs）只在缺失时透传
          const existing: any = clues[existingIdx]
          for (const [k, v] of Object.entries(c)) {
            if (k !== 'id' && k !== 'status' && existing[k] === undefined) existing[k] = v
          }
          applied += 1
        } else {
          clues.push({ status: '埋下', lastActionChapter: c.plantedChapter ?? null, ...c, id })
          applied += 1
        }
      }
      if (applied > 0) {
        await snapshotLedger(store, ['clues.json'])
        await store.writeJSON('clues.json', clues)
      }
      return { applied, message: `伏笔入账 ${applied}/${incoming.length}` }
    }
    case 'canonClassify': {
      const items: any[] = Array.isArray(payload.items) ? payload.items : []
      if (items.length === 0) return { applied: 0, message: '没有可归类的文档' }
      let applied = 0
      let renamed = 0
      const skipped: string[] = []
      for (const item of items) {
        const rawTitle = String(item.title ?? '').trim()
        if (!rawTitle) {
          skipped.push('标题为空')
          continue
        }
        const title = safeFileName(rawTitle)
        const doc = await store.canonDoc(title)
        if (doc === null) {
          skipped.push(`「${rawTitle}」未找到`)
          continue
        }
        // 分类写进文档首行 metadata 注释（M3-zero 简化格式；M2 迁 canon meta）
        const category = String(item.category ?? '未分类')
        let body = doc
        if (body.startsWith('<!-- 类别:')) {
          body = body.replace(/^<!-- 类别:.*?-->\n/, '')
        }
        await snapshotLedger(store, [joinCanon(title)])
        await store.writeText(joinCanon(title), `<!-- 类别:${category} -->\n${body}`)
        // 与 Rust 对齐：rename 也执行（同名或目标已存在则只应用分类，不改名）
        const rawRename = String(item.rename ?? '').trim()
        if (rawRename) {
          const renamedTitle = safeFileName(rawRename)
          if (renamedTitle !== title) {
            if ((await store.canonDoc(renamedTitle)) === null) {
              await store.moveText(joinCanon(title), joinCanon(renamedTitle))
              renamed += 1
            } else {
              skipped.push(`「${rawRename}」改名撞名，保留原文件名`)
            }
          }
        }
        applied += 1
      }
      return {
        applied,
        message: `归类写回 ${applied}/${items.length}${renamed ? `（改名 ${renamed} 篇）` : ''}${skipped.length ? '；跳过：' + skipped.join('、') : ''}`,
      }
    }
    case 'canonRevise': {
      // 预言书晋升：修订既有设定——采纳前先把原文备份到 canon/backups/
      const items: any[] = Array.isArray(payload.items) ? payload.items : []
      if (items.length === 0) return { applied: 0, message: '没有可修订的设定' }
      let applied = 0
      const skipped: string[] = []
      for (const d of items) {
        const rawTitle = String(d.title ?? '').trim()
        if (!rawTitle) continue
        const title = safeFileName(rawTitle)
        const content = String(d.content ?? '')
        if (!content.trim()) {
          skipped.push(`「${rawTitle}」缺修订正文，跳过`)
          continue
        }
        const doc = await store.canonDoc(title)
        if (doc === null) {
          skipped.push(`「${rawTitle}」未找到（同题校验）`)
          continue
        }
        const stamp = new Date().toISOString().replace(/[:.]/g, '-')
        await store.writeText(`canon/backups/${title}.${stamp}.md`, doc)
        await store.writeText(joinCanon(title), content)
        applied += 1
      }
      return { applied, message: `设定修订 ${applied}/${items.length}${skipped.length ? '；跳过：' + skipped.join('、') : ''}（已自动备份）` }
    }
    case 'plannedEvent': {
      const chapter = Number(payload.chapter ?? 0)
      const title = String(payload.title ?? '').trim()
      if (!chapter || !title) return { applied: 0, message: '计划事件缺章号或内容' }
      const events = await readArrayChecked(store, 'events.json')
      // 幂等：同章同标题的计划事件已存在（崩溃/重复采纳）不再追加
      if (events.some((e) => Number(e.chapter) === chapter && String(e.objectiveFact ?? '') === title)) {
        return { applied: 1, message: `计划事件「${title}」已在第${chapter}章入账，未重复追加` }
      }
      const seqs = events.map((e) => Number(String(e.id ?? '').replace(/^E/, '')) || 0)
      const seq = (seqs.length ? Math.max(...seqs) : 0) + 1
      const entry: Record<string, unknown> = {
        id: `E${String(seq).padStart(2, '0')}`,
        chapter,
        objectiveFact: title,
        revealed: false,
        happened: false,
      }
      const sid = String(payload.storylineID ?? '')
      if (sid) entry.storylineIDs = [sid]
      entry.plannedFrom = String(payload.from ?? '')
      events.push(entry)
      await snapshotLedger(store, ['events.json'])
      await store.writeJSON('events.json', events)
      return { applied: 1, message: `计划事件 ${entry.id}「${title}」→ 第${chapter}章 入账（happened=false，待写成）` }
    }
    case 'outlineUpdates': {
      const updates: any[] = payloadArray(payload as Record<string, unknown>, ['updates', 'outlineUpdates'])
      if (updates.length === 0) return { applied: 0, message: '没有可同步的大纲更新' }
      // 旧版（Swift）事件级同步：eventID + kind（事件已发生/事件改期/读者已知）+ newChapter
      const events: any[] = await store.readJSON<any[]>('events.json', [])
      let eventsDirty = false
      let eventApplied = 0
      for (const u of updates) {
        if (!u || typeof u.eventID !== 'string') continue
        const idx = events.findIndex((e) => e.id === u.eventID)
        if (idx < 0) continue
        const e = events[idx]
        if (u.kind === '事件已发生') {
          e.happened = true
          if (Number(u.newChapter) > 0) e.chapter = Number(u.newChapter)
          eventsDirty = true
          eventApplied += 1
        } else if (u.kind === '事件改期' && Number(u.newChapter) > 0) {
          e.chapter = Number(u.newChapter)
          eventsDirty = true
          eventApplied += 1
        } else if (u.kind === '读者已知') {
          e.revealed = true
          if (u.reason) e.readerKnowledge = String(u.reason)
          if (Number(u.newChapter) > 0) e.chapter = Number(u.newChapter)
          eventsDirty = true
          eventApplied += 1
        }
      }
      if (eventsDirty) await store.writeJSON('events.json', events)
      const storylineUpdates = updates.filter((u) => u && typeof u.eventID !== 'string')
      const lines = await readArrayChecked(store, 'storylines.json')
      let applied = 0
      const skipped: string[] = []
      for (const u of storylineUpdates) {
        const l = lines.find((x) => x.id === String(u.storylineID))
        if (!l) {
          skipped.push(`[${u.storylineID}] 未找到`)
          continue
        }
        if (u.status) l.status = String(u.status)
        // 无 note 的更新不清掉作者上次的同步备注
        if (u.note !== undefined && u.note !== null) l.syncNote = String(u.note)
        l.syncAt = new Date().toISOString()
        applied += 1
      }
      if (applied > 0) {
        await snapshotLedger(store, ['storylines.json'])
        await store.saveStorylines(lines)
      }
      const total = applied + eventApplied
      return { applied: total, message: `大纲同步 ${total}/${updates.length}${skipped.length ? '；跳过：' + skipped.join('、') : ''}` }
    }
    case 'draft': {
      // 与 Rust 对齐：采纳 = 先快照，再把草稿写入该章正文
      const chapter = Number(payload.chapter ?? 0)
      if (!chapter) return { applied: 0, message: '草稿缺章号' }
      const text = String(payload.text ?? '')
      if (!text.trim()) {
        return { applied: 0, message: '草稿正文为空——拒绝用空串覆盖整章（提案保持待审）' }
      }
      const dir = `chapters/ch-${String(chapter).padStart(3, '0')}`
      const existing = await store.chapterProse(chapter)
      const stamp = new Date().toISOString().replace(/[:.]/g, '')
      await store.writeText(`${dir}/snapshots/prose-${stamp}.md`, existing ?? '')
      await store.writeText(`${dir}/prose.md`, text)
      return { applied: 1, message: `第${chapter}章草稿入正文（采纳前已快照）` }
    }
    case 'skeleton': {
      const chapter = Number(payload.chapter ?? 0)
      if (!chapter) return { applied: 0, message: '骨架缺章号' }
      if (!Array.isArray(payload.beats) || payload.beats.length === 0) {
        return { applied: 0, message: '骨架缺 beats（或为空）——提案保持待审' }
      }
      const rel = `chapters/ch-${String(chapter).padStart(3, '0')}/meta.json`
      // 写路径严格读：坏 JSON / 数组顶层不能当空对象覆盖掉 title/notes。
      const meta = await readObjectChecked(store, rel)
      await snapshotLedger(store, [rel])
      meta['beats'] = payload.beats
      meta['skeletonApproved'] = true
      meta['approvedAt'] = new Date().toISOString()
      await store.writeJSON(rel, meta)
      return { applied: 1, message: '骨架批准入 meta（解锁该章草稿流；原 meta 已备份）' }
    }
    case 'report': {
      const chapter = Number(payload.chapter ?? 0)
      if (!chapter) return { applied: 0, message: '报告缺章号' }
      if (!Array.isArray(payload.findings) || payload.findings.length === 0) {
        return { applied: 0, message: '报告缺 findings（或为空）——提案保持待审' }
      }
      const rel = `chapters/ch-${String(chapter).padStart(3, '0')}/meta.json`
      const meta = await readObjectChecked(store, rel)
      await snapshotLedger(store, [rel])
      meta['report'] = payload.findings
      meta['reportAt'] = new Date().toISOString()
      await store.writeJSON(rel, meta)
      return { applied: 1, message: '验证报告归档章 meta（正文未动；原 meta 已备份）' }
    }
    case 'deslop': {
      const chapter = Number(payload.chapter ?? 0)
      if (!chapter) return { applied: 0, message: '去AI味缺章号' }
      const suggestions: any[] = Array.isArray(payload.suggestions) ? payload.suggestions : []
      if (suggestions.length === 0) return { applied: 0, message: '没有逐处建议' }
      let text = await store.chapterProse(chapter)
      if (!text || !text.trim()) return { applied: 0, message: `第${chapter}章还没有正文，无从替换` }
      let hit = 0
      const skipped: string[] = []
      const seen = new Set<string>()
      const snapRel = `chapters/ch-${String(chapter).padStart(3, '0')}/snapshots/${new Date().toISOString().replace(/[:.]/g, '-')}.md`
      for (const sg of suggestions) {
        const o = String(sg.original ?? ''), r = String(sg.replacement ?? '')
        if (!o) continue
        // 同 original 只处理一次：重复采纳/重复条目不能把第二处同名文本也改掉
        if (seen.has(o)) {
          skipped.push(`重复建议已忽略：${o.slice(0, 12)}…`)
          continue
        }
        seen.add(o)
        const at = text.indexOf(o)
        if (at < 0) { skipped.push(`${o.slice(0, 12)}…未命中`); continue }
        if (hit === 0) await store.writeText(snapRel, text) // 首次改写前快照
        text = text.slice(0, at) + r + text.slice(at + o.length)
        hit += 1
      }
      if (!hit) return { applied: 0, message: `所有建议的 original 都未命中正文：${skipped.join('、')}` }
      await store.writeText(`chapters/ch-${String(chapter).padStart(3, '0')}/prose.md`, text)
      return {
        applied: hit,
        message: `去AI味逐处替换 ${hit}/${suggestions.length}${skipped.length ? '；跳过：' + skipped.join('、') : ''}（首次替换前已快照）`,
      }
    }
    case 'relations': {
      const relations: any[] = Array.isArray(payload.relations) ? payload.relations : []
      const mem = await store.readJSON<Record<string, unknown>>('memory.json', {})
      const cur = Array.isArray(mem['facts']) ? (mem['facts'] as any[]) : []
      let added = 0
      const skipped: string[] = []
      for (const r of relations) {
        const from = String(r.from ?? '').trim()
        const pred = String(r.predicate ?? '').trim()
        const to = String(r.to ?? '').trim()
        if (!from || !pred || !to) { skipped.push('缺 from/predicate/to'); continue }
        const dup = cur.some(
          (f) => f.subject === from && f.predicate === pred && f.object === to,
        )
        if (dup) { skipped.push(`${from}-${pred}-${to} 已在账`); continue }
        cur.push({
          id: crypto.randomUUID(),
          subject: from,
          predicate: pred,
          object: to,
          fromChapter: Number(r.chapter) || 0,
          publicToReader: r.publicToReader !== false,
          source: 'relation',
          note: String(r.evidence ?? ''),
        })
        added += 1
      }
      mem['facts'] = cur
      await store.writeJSON('memory.json', mem)
      return { applied: added, message: `人物关系入账 ${added}/${relations.length}${skipped.length ? '；跳过：' + skipped.join('、') : ''}` }
    }
    case 'memoryPack': {
      const chapter = Number(payload.chapter ?? 0)
      const facts: any[] = Array.isArray(payload.facts) ? payload.facts : []
      if (facts.length === 0) return { applied: 0, message: '没有可入账的事实' }
      const mem = await readObjectChecked(store, 'memory.json')
      const cur = Array.isArray(mem['facts']) ? (mem['facts'] as any[]) : []
      let added = 0
      const skipped: string[] = []
      for (const f of facts) {
        const eff = f.fromChapter ?? chapter
        const dup = cur.some((x) => x.subject === f.subject && x.predicate === f.predicate && x.object === f.object && (x.fromChapter === eff || x.chapter === eff))
        if (dup) { skipped.push(String(f.subject ?? '?')); continue }
        const entry = { id: crypto.randomUUID(), fromChapter: eff, source: 'extracted', ...f }
        upsertAliasFromSubject(mem, entry)
        cur.push(entry)
        added += 1
      }
      if (added > 0) {
        mem['facts'] = cur
        await snapshotLedger(store, ['memory.json'])
        await store.writeJSON('memory.json', mem)
      }
      return { applied: added, message: `事实入账 ${added}/${facts.length}${skipped.length ? '；已在账：' + skipped.join('、') : ''}` }
    }
    case 'aliases': {
      // 与 Rust 对齐：人物别名档案入 memory.json.aliases（canonicalName 归并）
      const incoming: any[] = Array.isArray(payload.aliases) ? payload.aliases : []
      if (incoming.length === 0) return { applied: 0, message: '没有别名档案' }
      const mem = await readObjectChecked(store, 'memory.json')
      const cur = Array.isArray(mem['aliases']) ? (mem['aliases'] as any[]) : []
      let applied = 0
      const skipped: string[] = []
      for (const a of incoming) {
        const canon = String(a?.canonicalName ?? '').trim()
        if (!canon) { skipped.push('别名档案缺 canonicalName，跳过'); continue }
        const pos = cur.findIndex((x) => String(x?.canonicalName ?? '') === canon)
        if (pos >= 0) {
          const existing: any = cur[pos]
          const names: string[] = Array.isArray(existing.aliases)
            ? existing.aliases.filter((n: any) => typeof n === 'string')
            : []
          if (Array.isArray(a.aliases)) {
            for (const raw of a.aliases) {
              if (typeof raw !== 'string') continue
              const n = raw.trim()
              if (n && n !== canon && !names.includes(n)) names.push(n)
            }
          }
          existing.aliases = names
          for (const [k, v] of Object.entries(a)) {
            if (k !== 'canonicalName' && k !== 'aliases' && existing[k] === undefined) existing[k] = v
          }
        } else {
          cur.push({ ...a, canonicalName: canon, aliases: Array.isArray(a.aliases) ? a.aliases : [] })
        }
        applied += 1
      }
      mem['aliases'] = cur
      await snapshotLedger(store, ['memory.json'])
      await store.writeJSON('memory.json', mem)
      return {
        applied,
        message: `人物别名入账 ${applied}/${incoming.length}${skipped.length ? '；跳过：' + skipped.join('、') : ''}`,
      }
    }
    case 'dossier': {
      const chapter = Number(payload.chapter ?? 0)
      if (!chapter) return { applied: 0, message: '档案卡缺章号' }
      const headline = String(payload.headline ?? '').trim()
      const contentKeys = ['scenes', 'cast', 'advanced', 'turns', 'hooks', 'relics']
      const hasContent =
        headline !== '' ||
        contentKeys.some((k) => Array.isArray(payload[k]) && (payload[k] as unknown[]).length > 0)
      // 全空档案卡采纳会按「同章覆盖」清掉该章既有档案：拒绝空载荷。
      if (!hasContent) return { applied: 0, message: '档案卡没有可入库内容——提案保持待审' }
      const cast = (Array.isArray(payload.cast) ? payload.cast : [])
        .filter((n: unknown): n is string => typeof n === 'string')
        .map((n) => n.trim())
        .filter((n) => n !== '')
      // 先严格读档案（坏 dossiers.json 不能先写别名留下半截），再备份、写别名、写档案。
      const all = await readArrayChecked(store, 'dossiers.json')
      await snapshotLedger(store, ['dossiers.json', 'memory.json'])
      await upsertAliasNames(store, cast)
      const entry = {
        chapter,
        headline: payload.headline ?? '',
        scenes: payload.scenes ?? [], cast: payload.cast ?? [],
        hooks: payload.hooks ?? [], relics: payload.relics ?? [],
        advanced: payload.advanced ?? [], turns: payload.turns ?? [],
        updatedAt: new Date().toISOString(),
      }
      const pos = all.findIndex((d) => Number(d.chapter) === chapter)
      if (pos >= 0) all[pos] = entry; else all.push(entry)
      all.sort((a, b) => Number(a.chapter) - Number(b.chapter))
      await store.writeJSON('dossiers.json', all)
      return { applied: 1, message: `第${chapter}章档案卡入库（同章覆盖；原档案/别名表已备份）` }
    }
    case 'clueFixes': {
      const fixes: any[] = payloadArray(payload as Record<string, unknown>, ['fixes', 'clueFixes'])
      if (fixes.length === 0) return { applied: 0, message: '没有修复动作' }
      const written = await store.writtenChapters()
      const latest = Math.max(written.length ? written[written.length - 1] : 1, 1)
      const clues = await readArrayChecked(store, 'clues.json')
      let applied = 0
      const skipped: string[] = []
      for (const fx of fixes) {
        const cid = String(fx.clueID ?? '')
        const k = String(fx.kind ?? '')
        const detail = String(fx.detail ?? fx.action ?? fx.evidence ?? '')
        const c = clues.find((x: any) => x.id === cid) as any
        if (!c) { skipped.push(`${cid} 不在台账`); continue }
        // 补埋/改期必须真正改埋设章：显式字段优先，其次从 detail 的「第N章」取
        // 最后一个（「由第3章改到第9章」→ 9，不能取首个数写成 3）。
        const explicit = firstPositiveNumber(fx.plantedChapter, fx.newTargetChapter, fx.chapter, fx.toChapter)
        const targetChapter =
          explicit ?? ((k === '补埋' || k === '改期') ? chapterFromDetail(detail) : null)
        if ((k === '补埋' || k === '改期') && targetChapter === null) {
          skipped.push(`${cid} ${k} 缺目标章号（detail 写「第N章」或传 plantedChapter）`)
          continue
        }
        const beforeDetail = c.detail
        if (k === '回收' || k === '已回收') c.status = '已兑现'
        else if (k === '搁置') c.status = '搁置'
        else if (k === '放弃') c.status = '放弃'
        // 补埋=补登记为已埋；改期只换章号，保留原状态。
        else if (k === '补埋') c.status = '埋下'
        if (targetChapter !== null) c.plantedChapter = targetChapter
        // 「校正原文」= 修正台账里记的原文/说明：detail 即更正后的文本。
        if (k === '校正原文' && detail.trim() !== '') c.detail = detail
        c.lastActionChapter = latest
        c.actionLog = [...(Array.isArray(c.actionLog) ? c.actionLog : []), { at: latest, kind: k, detail, beforeDetail, ts: new Date().toISOString() }]
        applied += 1
      }
      if (!applied) return { applied: 0, message: `没有可应用的修复：${skipped.join('、')}` }
      await snapshotLedger(store, ['clues.json'])
      await store.writeJSON('clues.json', clues)
      return { applied, message: `埋点修复入账 ${applied}/${fixes.length}${skipped.length ? '；跳过：' + skipped.join('、') : ''}（只改台账，未动正文；原台账已备份）` }
    }
    case 'memo':
    case 'storylines':
    case 'outlineEvents':
      // 历史兼容名：与 Rust 一致，如实报告且提案保持待审
      return { applied: 0, message: `「${payload.kind}」无对应落库语义——提案保持待审` }
    default:
      return { applied: 0, message: `未知提案类型：${payload.kind ?? '（空）'}` }
  }
}

/** 记忆提案的 subject 首次出现即入人物别名档案（与 Rust upsert_alias_from_subject 对齐）。 */
function upsertAliasFromSubject(mem: Record<string, unknown>, fact: any): void {
  const subject = String(fact?.subject ?? '').trim()
  if (!subject) return
  const aliases = Array.isArray(mem['aliases']) ? (mem['aliases'] as any[]) : []
  mem['aliases'] = aliases
  if (aliases.some((a) => String(a?.canonicalName ?? '') === subject)) return
  aliases.push({ canonicalName: subject, aliases: [], source: 'extracted' })
}

function joinCanon(title: string): string {
  return `canon/${safeFileName(title)}.md`
}

/**
 * 采纳前把将被覆盖的原文件复制到 snapshots/ledger/<批次>/<原相对路径>。
 * 非正文落库（meta.beats/report、dossiers、memory、storylines、clues、events）没有
 * 正文快照页那样的回滚入口，但原文件必须可找回——「落点 · 回滚」不再只写「不触正文」。
 */
async function snapshotLedger(store: NovelStore, rels: string[]): Promise<void> {
  const stamp = new Date().toISOString().replace(/[:.]/g, '-')
  const batch = `${stamp}-${crypto.randomUUID().slice(0, 8)}`
  for (const rel of rels) {
    const text = await store.readText(rel)
    if (text === null) continue
    await store.writeText(`snapshots/ledger/${batch}/${rel}`, text)
  }
}

/** 档案卡出场人物补进 memory.aliases（与 Rust upsert_alias_names 对齐；只补缺失）。 */
async function upsertAliasNames(store: NovelStore, names: string[]): Promise<void> {
  if (names.length === 0) return
  const mem = await readObjectChecked(store, 'memory.json')
  const aliases = Array.isArray(mem['aliases']) ? (mem['aliases'] as any[]) : []
  let added = 0
  for (const name of names) {
    if (aliases.some((a) => String(a?.canonicalName ?? '') === name)) continue
    aliases.push({ canonicalName: name, aliases: [], source: 'extracted' })
    added += 1
  }
  if (added === 0) return
  mem['aliases'] = aliases
  await store.writeJSON('memory.json', mem)
}

function firstPositiveNumber(...values: unknown[]): number | null {
  for (const v of values) {
    const n = typeof v === 'number' ? v : typeof v === 'string' && v.trim() !== '' ? Number(v) : NaN
    if (Number.isFinite(n) && n > 0) return Math.floor(n)
  }
  return null
}

/** 「由第3章改到第9章」→ 9：优先取最后一个「第N章/回」，没有章标记才取最后一个数字。 */
function chapterFromDetail(text: string): number | null {
  const marked = [...text.matchAll(/第\s*(\d+)\s*(?:章|回)/g)]
    .map((m) => Number(m[1]))
    .filter((n) => Number.isFinite(n) && n > 0)
  if (marked.length > 0) return marked[marked.length - 1]
  const nums = [...text.matchAll(/\d+/g)]
    .map((m) => Number(m[0]))
    .filter((n) => Number.isFinite(n) && n > 0)
  return nums.length > 0 ? nums[nums.length - 1] : null
}
