// Spike 用的最小 sdk JSON-RPC 客户端：node rpc.mjs <dsh bin.js> <cwd> <patch|-> <prompt> [seconds]
import { spawn } from 'node:child_process'
const [binjs, cwd, patch, prompt, secs = '25'] = process.argv.slice(2)
const args = [binjs, '--profile', 'sdk']
if (patch && patch !== '-') args.push('--patch', patch)
const p = spawn(process.execPath, args, { stdio: ['pipe', 'pipe', 'inherit'], env: process.env })
let id = 0
const pending = new Map()
const send = (method, params) => new Promise((res) => {
  const i = ++id; pending.set(i, res)
  p.stdin.write(JSON.stringify({ jsonrpc: '2.0', id: i, method, params }) + '\n')
})
let buf = ''
const events = []
p.stdout.on('data', (d) => {
  buf += d
  let n
  while ((n = buf.indexOf('\n')) >= 0) {
    const line = buf.slice(0, n); buf = buf.slice(n + 1)
    if (!line.trim()) continue
    const m = JSON.parse(line)
    if (m.id && !m.method) { pending.get(m.id)?.(m); pending.delete(m.id) }
    else if (m.method === 'session.event') { const e = m.params.event; events.push(e); console.log('EVENT', e.type, JSON.stringify(e.data).slice(0, 220)) }
    else console.log('NOTIFY', m.method, JSON.stringify(m.params).slice(0, 200))
  }
})
const init = await send('initialize', { cwd, provider: process.env.ZP ?? 'deepseek-official', model: process.env.ZM ?? 'deepseek-chat', approvalRequests: false })
console.log('INIT', JSON.stringify(init).slice(0, 300))
const q = await send('session/query', { sessionId: 'spike-1', contentBlocks: [{ type: 'text', text: prompt }] })
console.log('QUERY', JSON.stringify(q).slice(0, 300))
await new Promise((r) => setTimeout(r, Number(secs) * 1000))
await send('shutdown', null).catch(() => {})
p.kill()
console.log('TOOLS-SEEN', [...new Set(events.filter(e => e.type === 'tool/call').map(e => e.data.name))])
