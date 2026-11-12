// Spike 探针：作为 preset 的子插件，对所在 agent 的 scope 做 tools.restrict；
// 结果写到 /tmp/probe.log 以便取证。
import { appendFileSync } from 'node:fs'

export const name = 'zhibi-spike-probe'
export const inject = ['tools']

function log(line) {
  try {
    appendFileSync('/tmp/probe.log', line + '\n')
  } catch {
    // 取证日志写不出去不影响探针本身
  }
}

export function apply(ctx, config) {
  const deny = Array.isArray(config?.deny) ? config.deny : []
  log('apply called deny=' + deny.join(','))
  try {
    const dispose = ctx.tools.restrict({ deny })
    ctx.effect(() => dispose)
    log('restrict ok')
  } catch (e) {
    log('restrict failed: ' + (e && e.message))
  }
}
