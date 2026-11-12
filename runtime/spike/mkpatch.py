import yaml

here = '/Volumes/data/dev/coding/novel/zhibi-next/runtime/spike/probe/index.js'
deny = ['bash', 'write', 'edit', 'plugin_manager', 'workflow']

reviewer_plugins = [
    {'id': 'persona', 'name': '@deepseek-ai/dsh-persona',
     'config': {'prefix': '你是校审员，只读不改。', 'complete': True, 'includeRuntimeContext': False}},
    {'id': 'tool-executor-native', 'name': '@deepseek-ai/dsh-tool-executor-native'},
    {'id': 'readonly-guard', 'name': here, 'config': {'deny': deny}},
]

doc = [
    {'id': 'tool-executor-native', 'disabled': True},
    {'id': 'tool-executor-ptc', 'disabled': False},
    {'id': 'tool-subagent', 'config': {
        'provider': 'spawn', 'toolName': 'subagent',
 'allowedAgentPresets': ['reviewer'], 'agentPreset': 'reviewer'}},
    {'insert': [
        {'id': 'agent-preset-registry', 'name': '@deepseek-ai/dsh-agent-preset-registry',
         'config': {'default': 'reviewer'}},
 {'id': 'preset-reviewer', 'name': '@deepseek-ai/dsh-agent-preset',
  'config': {'id': 'reviewer', 'name': '校审', 'plugins': reviewer_plugins}},
    ]},
]

with open('host-ptc-restrict.patch.yml', 'w') as f:
    f.write('# Spike: reviewer preset with tools.restrict deny list\n')
    f.write(yaml.safe_dump(doc, allow_unicode=True, sort_keys=False))
print('ok')
