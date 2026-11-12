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

test('关系区间覆盖与共享夹具一致', () => {
  // interval.covers 在 Rust 侧（relations.rs）；TS 侧没有等价实现，
  // 这里只校验夹具本身的边界语义没被写歪（防的是"改夹具时手滑"）。
  for (const c of cases.intervalCovers) {
    const started = c.from === 0 || c.chapter >= c.from
    const notEnded = c.to === 0 || c.chapter < c.to
    assert.equal(started && notEnded, c.covers, `[${c.from},${c.to}) vs ${c.chapter}（${c.why ?? ''}）`)
  }
})
