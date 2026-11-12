// 双端一致性（TypeScript 侧）。
//
// 与 app/src/conformance.rs 读**同一个**夹具 tests/conformance/cases.json。
// 两侧断言同一组期望值：任一侧改了规则而没改夹具，两侧测试会同时失败。
//
// 跑法：node --test novel-pack/test/   （Node 24 自带 test runner）

import { test } from 'node:test'
import assert from 'node:assert/strict'
import { readFileSync, mkdtempSync, writeFileSync } from 'node:fs'
import { tmpdir } from 'node:os'
import { dirname, join } from 'node:path'
import { fileURLToPath } from 'node:url'

import { safeFileName, chapterDirName } from '../src/store.ts'
import { intervalCovers, intervalIsValid, activeIntervals } from '../src/relations.ts'
import {
  canWriteLedger,
  canWriteProse,
  currentMode,
  ghostwriteChapters,
} from '../src/write-policy.ts'

const here = dirname(fileURLToPath(import.meta.url))
const root = join(here, '..', '..')
const cases = JSON.parse(readFileSync(join(root, 'tests', 'conformance', 'cases.json'), 'utf8'))

test('safeFileName 与共享夹具一致', () => {
  for (const c of cases.safeFileName) {
    assert.equal(safeFileName(c.in), c.out, `safeFileName(${JSON.stringify(c.in)})`)
  }
})

test('chapterDirName 与共享夹具一致', () => {
  for (const c of cases.chapterDirName) {
    assert.equal(chapterDirName(c.n), c.out, `chapterDirName(${c.n})`)
  }
})

test('写作档位闸门与共享夹具一致', (t) => {
  // write-policy 是"读书里的 zhibi-auth.json"，而夹具给的是期望值本身。
  // 这里用临时目录把夹具的输入落成真实文件再读回来——测的是**读文件 + 判断**
  // 这条完整链路，不是把判断逻辑抄一遍。
  for (const c of cases.writePolicy) {
    const dir = mkdtempSync(join(tmpdir(), 'zhibi-conf-'))
    writeFileSync(
      join(dir, 'zhibi-auth.json'),
      JSON.stringify({
        writingMode: c.mode,
        ghostwriteChapters: c.ghostwriteChapters ?? [],
      }),
    )
    assert.equal(currentMode(dir), c.mode, `档位应读成 ${c.mode}`)
    assert.deepEqual(
      ghostwriteChapters(dir),
      c.ghostwriteChapters ?? [],
      `授权章节应读回 ${JSON.stringify(c.ghostwriteChapters ?? [])}`,
    )
    assert.equal(
      canWriteLedger(dir).allowed,
      c.ledger,
      `${c.mode} 的台账权限与夹具不符（${c.why ?? ''}）`,
    )
    assert.equal(
      canWriteProse(dir, c.chapter).allowed,
      c.prose,
      `${c.mode} 第 ${c.chapter} 章的正文权限与夹具不符（${c.why ?? ''}）`,
    )
  }
  t.diagnostic(`覆盖 ${cases.writePolicy.length} 个档位组合`)
})

test('关系区间覆盖与共享夹具一致', (t) => {
  // 两端各有真实实现在跑这条夹具（Rust: relations::Interval::covers，
  // TS: relations.ts intervalCovers）。以前这里只校验夹具自身语义，
  // 等于没测——TS 侧改了边界规则也不会红。
  for (const c of cases.intervalCovers) {
    const iv = { fromChapter: c.from, toChapter: c.to }
    assert.equal(
      intervalCovers(iv, c.chapter),
      c.covers,
      `[${c.from},${c.to}) vs 第${c.chapter}章（${c.why ?? ''}）`,
    )
  }
  t.diagnostic(`覆盖 ${cases.intervalCovers.length} 个区间边界组合`)
})

test('区间合法性判定与共享夹具一致', () => {
  // 终点早于起点是坏数据；0 表示开放，不算坏。
  for (const c of cases.intervalCovers) {
    const expected = c.to === 0 || c.to > c.from
    assert.equal(intervalIsValid({ fromChapter: c.from, toChapter: c.to }), expected)
  }
  assert.equal(intervalIsValid({ fromChapter: 10, toChapter: 5 }), false)
  assert.equal(intervalIsValid({ fromChapter: 10, toChapter: 10 }), false, '空区间不算合法')
})

test('同一时刻的并存区间都会被取到', () => {
  // 第 5-19 章师徒，第 20 章起仇敌：第 19 章只有师徒，第 20 章只有仇敌，
  // 中间不存在"两个都对"的时刻——夹具的边界语义要是写歪了，这里会露出来。
  const ivs = [
    { fromChapter: 5, toChapter: 20, label: '师徒' },
    { fromChapter: 20, toChapter: 0, label: '仇敌' },
  ]
  assert.deepEqual(activeIntervals(ivs, 19).map((i) => i.label), ['师徒'])
  assert.deepEqual(activeIntervals(ivs, 20).map((i) => i.label), ['仇敌'])
  assert.deepEqual(activeIntervals(ivs, 4).map((i) => i.label), [])
})
