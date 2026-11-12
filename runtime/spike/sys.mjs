import { spawn } from 'node:child_process'
const [binjs, cwd, patch, outfile] = process.argv.slice(2)
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
let sys = ''
p.stdout.on('data', (d) => {
  buf += d
  let n
  while ((n = buf.indexOf('\n')) >= 0) {
    const l = buf.slice(0, n)
    buf = buf.slice(n + 1)
    if (!l.trim()) continue
    const m = JSON.parse(l)
    if (m.id && !m.method) { pending.get(m.id)?.(m); pending.delete(m.id) }
    else if (m.method === 'session.event' && m.params.event.type === 'system/message') {
      sys = m.params.event.data.message.content.map((c) => c.text).join('')
    }
  }
})
await send('initialize', { cwd, provider: 'deepseek-official', model: 'deepseek-chat' })
await send('session/query', { sessionId: 's', contentBlocks: [{ type: 'text', text: 'hi' }] })
await new Promise((r) => setTimeout(r, 6000))
import('node:fs').then((fs) => fs.writeFileSync(outfile, sys))
console.log('system length', sys.length, '| STDERR:', err.slice(0, 1500))
p.kill()
