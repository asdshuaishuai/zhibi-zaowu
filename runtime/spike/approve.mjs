// 验证：开启 approvalRequests 后，客户端应答 approval/request 能否放行 plugin_manager
import { spawn } from 'node:child_process'
const [binjs, cwd, patch, secs, prompt] = process.argv.slice(2)
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
let done = false
p.stdout.on('data', (d) => {
  buf += d
  let n
  while ((n = buf.indexOf('\n')) >= 0) {
    const l = buf.slice(0, n)
    buf = buf.slice(n + 1)
    if (!l.trim()) continue
    const m = JSON.parse(l)
    if (m.method === 'approval/request' && m.id) {
      console.log('APPROVAL-REQ', JSON.stringify(m.params).slice(0, 300), '-> allowed-once')
      p.stdin.write(JSON.stringify({ jsonrpc: '2.0', id: m.id, result: { outcome: 'allowed-once' } }) + '\n')
      continue
    }
    if (m.id && !m.method) { pending.get(m.id)?.(m); pending.delete(m.id); continue }
    if (m.method === 'session.event') {
      const e = m.params.event
      if (e.type === 'tool/call') console.log('CALL', e.data.name, String(e.data.arguments).slice(0, 300))
      if (e.type === 'tool/result') console.log('RESULT', JSON.stringify(e.data.message?.content ?? e.data).slice(0, 700))
      if (e.type === 'turn/end') console.log('TURN-END', JSON.stringify(e.data.reason).slice(0, 200))
    } else if (m.method === 'session.query.completed') done = true
  }
})
await send('initialize', { cwd, provider: 'deepseek-account', model: 'deepseek-v4-flash', approvalRequests: true })
await send('session/query', { sessionId: 'a' + Date.now(), contentBlocks: [{ type: 'text', text: prompt }] })
const end = Date.now() + Number(secs) * 1000
while (!done && Date.now() < end) await new Promise((r) => setTimeout(r, 500))
p.kill()
