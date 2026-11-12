import { spawn } from 'node:child_process'
const [binjs, cwd, patch] = process.argv.slice(2)
const args = [binjs, '--profile', 'sdk']; if (patch && patch !== '-') args.push('--patch', patch)
const p = spawn(process.execPath, args, { stdio: ['pipe', 'pipe', 'inherit'], env: process.env })
let id = 0; const pending = new Map()
const send = (m, params) => new Promise(r => { const i = ++id; pending.set(i, r); p.stdin.write(JSON.stringify({ jsonrpc: '2.0', id: i, method: m, params }) + '\n') })
let buf = ''; let header = null; let sys = ''
p.stdout.on('data', d => { buf += d; let n; while ((n = buf.indexOf('\n')) >= 0) { const l = buf.slice(0, n); buf = buf.slice(n + 1); if (!l.trim()) continue; const m = JSON.parse(l)
  if (m.id && !m.method) { pending.get(m.id)?.(m); pending.delete(m.id) }
  else if (m.method === 'session.event') { const e = m.params.event; if (e.type === 'request/header') header = e.data.header; if (e.type === 'system/message') sys = e.data.message.content.map(c => c.text).join('') } } })
await send('initialize', { cwd, provider: 'deepseek-official', model: 'deepseek-chat' })
await send('session/query', { sessionId: 'h', contentBlocks: [{ type: 'text', text: 'hi' }] })
await new Promise(r => setTimeout(r, 6000))
console.log('TOOLS:', (header?.tools ?? []).map(t => t.name).join(', '))
console.log('SYSTEM-LEN:', sys.length)
console.log(sys.slice(0, 1500))
p.kill()
