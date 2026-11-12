// 完整事件追踪：node chat.mjs <bin.js> <cwd> <patch|-> <seconds> <prompt>
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
const sessions = new Set()
p.stdout.on('data', (d) => {
  buf += d
  let n
  while ((n = buf.indexOf('\n')) >= 0) {
    const l = buf.slice(0, n)
    buf = buf.slice(n + 1)
    if (!l.trim()) continue
    const m = JSON.parse(l)
    if (m.id && !m.method) { pending.get(m.id)?.(m); pending.delete(m.id); continue }
    if (m.method === 'session.event') {
      const sid = m.params.sessionId
      sessions.add(sid)
      const e = m.params.event
      const t = e.type
      if (t === 'tool/call') console.log(`[${sid}] CALL ${e.data.name} ${String(e.data.arguments).slice(0, 400)}`)
      else if (t === 'tool/result') console.log(`[${sid}] RESULT ${JSON.stringify(e.data).slice(0, 500)}`)
      else if (t === 'assistant/message') console.log(`[${sid}] SAY ${JSON.stringify(e.data.message.content).slice(0, 400)}`)
      else if (t === 'turn/end') { console.log(`[${sid}] TURN-END ${JSON.stringify(e.data.reason).slice(0, 200)}`) }
      else if (t === 'agent-preset/selected') console.log(`[${sid}] PRESET ${JSON.stringify(e.data)}`)
    } else if (m.method === 'subagent.started' || m.method === 'subagent.finished') console.log('NOTIFY', m.method, JSON.stringify(m.params).slice(0, 300))
    else if (m.method === 'session.query.completed') { console.log('COMPLETED', JSON.stringify(m.params).slice(0, 300)); done = true }
  }
})
await send('initialize', { cwd, provider: process.env.ZP ?? 'deepseek-account', model: process.env.ZM ?? 'deepseek-v4-flash' })
await send('session/query', { sessionId: 'm' + Date.now(), contentBlocks: [{ type: 'text', text: prompt }] })
const end = Date.now() + Number(secs) * 1000
while (!done && Date.now() < end) await new Promise((r) => setTimeout(r, 500))
console.log('SESSIONS', [...sessions])
p.kill()
