import { spawn } from 'node:child_process'
const [binjs, cwd, patch] = process.argv.slice(2)
const args = [binjs, '--profile', 'sdk']
if (patch && patch !== '-') args.push('--patch', patch)
const p = spawn(process.execPath, args, { stdio: ['pipe', 'pipe', 'pipe'], env: process.env })
let err = ''
p.stderr.on('data', (d) => { err += d })
let id = 0
const pending = new Map()
const send = (m, params) => new Promise((r) => {
  const i = ++id
  pending.set(i, r)
  p.stdin.write(JSON.stringify({ jsonrpc: '2.0', id: i, method: m, params }) + '\n')
})
let buf = ''
let header = null
p.stdout.on('data', (d) => {
  buf += d
  let n
  while ((n = buf.indexOf('\n')) >= 0) {
    const l = buf.slice(0, n)
    buf = buf.slice(n + 1)
    if (!l.trim()) continue
    const m = JSON.parse(l)
    if (m.id && !m.method) { pending.get(m.id)?.(m); pending.delete(m.id) }
    else if (m.method === 'session.event' && m.params.event.type === 'request/header') header = m.params.event.data.header
  }
})
await send('initialize', { cwd, provider: 'deepseek-official', model: 'deepseek-chat' })
await send('session/query', { sessionId: 'h', contentBlocks: [{ type: 'text', text: 'hi' }] })
await new Promise((r) => setTimeout(r, 6000))
const tools = header ? header.tools : []
console.log('TOOLS:', tools.map((x) => x.name).join(', '))
const rc = tools.find((x) => x.name === 'run_code')
const desc = rc ? JSON.stringify(rc) : ''
console.log('run_code schema length:', desc.length)
for (const k of ['subagent', 'agent_preset', 'reviewer', 'tools.write', 'tools.edit', 'tools.bash']) console.log(k, desc.includes(k))
const i = desc.indexOf('agent_preset')
if (i >= 0) console.log('...', desc.slice(Math.max(0, i - 300), i + 400))
console.log('STDERR:', err.slice(0, 2000))
p.kill()
