# runtime/

DSH 基础运行环境的源码与冒烟工具。

## `zhibi-runtime/`

我们自己的 npm 包，只声明 DSH 的 sdk 闭包（不含 web 前端、桌面端、ACP、实验包）。

- `package.json` —— **DSH 版本号只在这里维护一处**。升级 DSH 时改这里的版本，再跑
  `scripts/vendor_runtime.sh --force`。
- `lib/` —— 官方 `@deepseek-ai/dsh` 的 `lib/*.js` **原样拷贝**（`bin.js` 会动态 import
  带 hash 的兄弟块，必须整目录）。不含 `node_modules`。
- `dsh-upstream-package.json` —— 官方 `dsh` 的 package.json 副本，`bin.js` 的
  `INSTALL_ANCHOR` 会读它取版本号与安装范围。
- `node_modules/` —— `vendor_runtime.sh` 安装产物，已 gitignore。

体积：依赖约 274 MB（原整包 758 MB）。

## `spike/`

M0/M1 的协议验证脚本（`rpc.mjs` / `header.mjs` / `chat.mjs` / `approve.mjs`）与
`probe/` 只读守卫探针。结论见 `docs/SPIKE-M0.md`。

用法（需要真实模型）：

```sh
cd runtime/zhibi-runtime
DSH_HOME=/tmp/xxx ../../.vendor/node/bin/node ../spike/chat.mjs \
  lib/bin.js <cwd> <patch.yml|-> <秒> "<提示词>"
```

`chat.mjs` 会打印所有 `tool/call`、`tool/result`、`agent-preset/selected`，
用来确认“主 Agent 是否真的派给了某个专属 Agent”。
