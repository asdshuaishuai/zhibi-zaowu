// 验证：进程重启后 session/resume 能否接回历史
import { spawn } from 'node:child_process'
const [binjs, cwd, patch, mode] = process.argv.slice(2)
const args = [binjs, '--profile', 'sdk']
if (patch && patch !== '-') args.push('--patch', patch)
const p = spawn(process.execPath, args, { stdio: ['pipe', 'pipe', 'inherit'], env: process.env })
let id = 0
const pending = new Map()
const send = (m, params) => new Promise((r) => {
  const i = ++id
  pending.set(i, r)
  p.stdin.write(JSON.stringify({ jsonrpc: '2.0', id: i, method: m, params }) + '\n')
})
let buf = ''
p.stdout.on('data', (d) => {
  buf += d
  let n
  while ((n = buf.indexOf('\n')) >= 0) {
    const l = buf.slice(0, n)
    buf = buf.slice(n + 1)
    if (!l.trim()) continue
    const m = JSON.parse(l)
    if (m.method === 'approval/request' && m.id) {
      p.stdin.write(JSON.stringify({ jsonrpc: '2.0', id: m.id, result: { outcome: 'allowed-once' } }) + '\n')
      continue
    }
    if (m.id && !m.method) { pending.get(m.id)?.(m); pending.delete(m.id); continue }
    if (m.method === 'session.event') {
      const e = m.params.event
      if (e.type === 'user/message' && mode === 'read') console.log('HISTORY user:', JSON.stringify(e.data.content).slice(0, 160))
      if (e.type === 'assistant/message' && mode === 'read') console.log('HISTORY assistant:', JSON.stringify(e.data.message.content).slice(0, 160))
      if (e.type === 'turn/end') console.log('TURN-END', JSON.stringify(e.data.reason).slice(0, 120))
    }
  }
})
const SID = 'resume-probe-1'
await send('initialize', { cwd, provider: 'deepseek-account', model: 'deepseek-v4-flash' })
if (mode === 'write') {
  const q = await send('session/query', { sessionId: SID, contentBlocks: [{ type: 'text', text: '记住这个词：青锋。只回复收到。' }] })
  console.log('QUERY', JSON.stringify(q).slice(0, 120))
  await new Promise((r) => setTimeout(r, 12000))
  await send('shutdown', null).catch(() => {})
} else {
  const r = await send('session/resume', { sessionId: SID })
  console.log('RESUME', JSON.stringify(r).slice(0, 300))
  await new Promise((r2) => setTimeout(r2, 4000))
  const q2 = await send('session/query', { sessionId: SID, contentBlocks: [{ type: 'text', text: '刚让你记住的词是什么？只回复那个词。' }] })
  console.log('FOLLOWUP', JSON.stringify(q2).slice(0, 120))
  await new Promise((r2) => setTimeout(r2, 14000))
  await send('shutdown', null).catch(() => {})
}
p.kill()
