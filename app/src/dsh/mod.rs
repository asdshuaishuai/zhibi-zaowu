//! DSH（DeepSeek Harness）sdk 子进程客户端。
//!
//! 协议：newline-delimited JSON-RPC over stdio（`dsh --profile sdk`）。
//! - 请求：initialize / session.query / session.wait / shutdown
//! - 通知：session.event（全量会话事件流 → 右侧轨迹面板）、session.status、
//!   session.query.completed、subagent.*、user-question.cancelled
//!
//! M1 边界：不启用 approvalRequests 与 userQuestions 能力（审批与提问由 runtime
//! 进程内应答）；未知服务端请求一律回 -32601。会话事件按原始 envelope 透传，
//! UI 侧只提取展示所需字段——DSH 演进时协议层不必跟着改。

use std::collections::HashMap;
use std::io::{BufRead, BufReader, Write};
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicI64, Ordering};
use std::sync::{Arc, Mutex};

use anyhow::{Context as _, Result, anyhow};
use serde_json::{Value, json};

/// UI 消费的事件：通知原样上抛，进程异常以 Error 上抛。
#[derive(Debug, Clone)]
pub enum DshEvent {
    Notification { method: String, params: Value },
    Error(String),
    /// 服务端请求审批（`approval/request`）：GUI 必须应答，否则 DSH 按 fail-closed 拒绝。
    Approval { id: Value, session_id: String, tool: String, reason: String },
    /// 服务端请求用户回答（`user-question/ask`）：GUI 弹窗，答案或取消都要回。
    Question { id: Value, session_id: String, request_id: String, questions: Value },
}

/// 主 Agent 的形态。切换 = 用另一份 patch 重启该书进程（会话由 DSH 持久化，可 resume）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AgentMode {
    /// 托管（默认）：PTC 执行器，主 Agent 可经 subagent 指挥各专属 Agent。
    Managed,
    /// 极简：native 执行器、只读、无 subagent；提案与共创。
    Minimal,
    /// 创造：在托管基础上开放官方 Cordis 创造工具，用于造本书专属 Agent。
    Creator,
}

impl AgentMode {
    #[allow(dead_code)] // 落设置文件与未来 CLI 覆盖时用
    pub fn key(self) -> &'static str {
        match self {
            AgentMode::Managed => "managed",
            AgentMode::Minimal => "minimal",
            AgentMode::Creator => "creator",
        }
    }

    #[allow(dead_code)]
    pub fn from_key(k: &str) -> Option<Self> {
        match k.trim() {
            "managed" | "托管" => Some(AgentMode::Managed),
            "minimal" | "极简" => Some(AgentMode::Minimal),
            "creator" | "创造" => Some(AgentMode::Creator),
            _ => None,
        }
    }
}

/// 每本书一个 DSH_HOME，作为 Agent 配置与凭据的隔离边界。
///
/// 放在项目目录之外，避免 AI 在 workspace-write 下改到自己的配置：
/// `~/Library/Application Support/zhibi/homes/<书目录名的稳定哈希>`。
/// 书名改了也不换 home（用路径哈希而不是书名）；保留 `ZHIBI_DSH_HOME` 覆盖用于测试。
pub fn home_dir(root: &std::path::Path) -> std::path::PathBuf {
    if let Ok(p) = std::env::var("ZHIBI_DSH_HOME") {
        if !p.trim().is_empty() {
            return std::path::PathBuf::from(p);
        }
    }
    let canon = std::fs::canonicalize(root).unwrap_or_else(|_| root.to_path_buf());
    let key = short_hash(canon.to_string_lossy().as_ref());
    let name = canon
        .file_stem()
        .map(|s| s.to_string_lossy().to_string())
        .unwrap_or_else(|| "book".into());
    let safe: String = name
        .chars()
        .map(|c| if c.is_alphanumeric() || c == '-' || c == '_' { c } else { '_' })
        .collect();
    let cache_key = canon.display().to_string();
    if let Ok(cache) = home_cache().lock() {
        if let Some(hit) = cache.get(&cache_key) {
            return hit.clone();
        }
    }
    let base = std::env::var("HOME")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|_| std::path::PathBuf::from("/tmp"))
        .join("Library")
        .join("Application Support")
        .join("zhibi")
        .join("homes");
    let out = base.join(format!("{safe}-{key}"));
    if let Ok(mut cache) = home_cache().lock() {
        cache.insert(cache_key, out.clone());
    }
    out
}

/// 进程内缓存：`home_dir` 依赖全局 `HOME`，而测试会临时改它；真实运行中
/// 环境变量也可能被外部改动。缓存首次解析结果，保证同一进程内同一本书永远同一个 home。
fn home_cache() -> &'static std::sync::Mutex<std::collections::HashMap<String, std::path::PathBuf>> {
    static CACHE: std::sync::OnceLock<
        std::sync::Mutex<std::collections::HashMap<String, std::path::PathBuf>>,
    > = std::sync::OnceLock::new();
    CACHE.get_or_init(|| std::sync::Mutex::new(std::collections::HashMap::new()))
}

/// FNV-1a 64：只用于目录名，不需要密码学强度。
fn short_hash(s: &str) -> String {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for b in s.as_bytes() {
        h ^= *b as u64;
        h = h.wrapping_mul(0x0000_0100_0000_01b3);
    }
    format!("{h:016x}")[..8].to_string()
}

struct Inner {
    stdin: Mutex<Option<std::process::ChildStdin>>,
    // 子进程句柄仅用于 shutdown 强制终止；读线程持有 stdout 的所有权
    #[allow(dead_code)]
    child: Mutex<Option<Child>>,
    pending: Mutex<HashMap<i64, async_channel::Sender<Value>>>,
    next_id: AtomicI64,
    ui_tx: async_channel::Sender<DshEvent>,
}

impl Inner {
    fn write_line(&self, msg: &Value) -> Result<()> {
        let mut line = msg.to_string();
        line.push('\n');
        let mut guard = self.stdin.lock().map_err(|_| anyhow!("stdin 锁不可用"))?;
        let Some(stdin) = guard.as_mut() else {
            return Err(anyhow!("DSH 运行时已退出（stdin 已关闭）"));
        };
        stdin.write_all(line.as_bytes())?;
        stdin.flush()?;
        Ok(())
    }
}

/// 可跨线程共享的客户端句柄（Workspace 持有，读线程与请求方共用）。
#[derive(Clone)]
pub struct DshClient {
    inner: Arc<Inner>,
}

/// 一个专属 Agent 的声明（写进 DSH 的 agent-preset 行）。
pub struct PresetSpec {
    pub id: &'static str,
    pub label: &'static str,
    pub order: u32,
    pub persona: &'static str,
    /// 该 Agent 明确不可用的全局工具。spike 已证：preset 自己的 tool-fs 开关无效，
    /// 子 agent 会继承 host 平面全部工具，因此必须靠 tools.restrict 收窄。
    pub deny: &'static [&'static str],
}

/// 内置专属 Agent。主 Agent（托管模式）通过 subagent 指挥它们；
/// 作者也能在模块页直接对着某个 Agent 说话。
pub fn zhibi_presets() -> Vec<PresetSpec> {
    vec![
        PresetSpec {
            id: "character",
            label: "人物",
            order: 1,
            persona: "你负责本书的人物档案：从正文与档案里核对人物信息，补全外貌、性格、动机、背景、目标、缺陷、口吻与弧线，并提出合并/拆分建议。只描述可取证的事实；不确定的标注为待作者确认。",
            deny: &["bash", "write", "edit", "plugin_manager", "workflow"],
        },
        PresetSpec {
            id: "relations",
            label: "关系",
            order: 2,
            persona: "你负责人物关系网：梳理谁与谁之间的关系、关系类型与方向，标注每段关系从第几章成立、到第几章结束、读者是否已知。同一对人物可以有多段不同区间的关系。",
            deny: &["bash", "write", "edit", "plugin_manager", "workflow"],
        },
        PresetSpec {
            id: "worldline",
            label: "世界线",
            order: 3,
            persona: "你负责世界线推演：从指定节点分叉，沿因果链推演后续，每个节点标注章节与状态。所有推演只写沙盒，不改正典；要进正典必须走提案由作者采纳。",
            deny: &["bash", "write", "edit", "plugin_manager", "workflow"],
        },
        PresetSpec {
            id: "oracle",
            label: "预言",
            order: 4,
            persona: "你负责预测与影响分析：按大纲（无大纲则按成稿）向前预测 2-3 个走向，说明每个走向呼应了哪些到期伏笔与设定铁律，并评估影响面（波及几条线、几个设定、最大风险）。",
            deny: &["bash", "write", "edit", "plugin_manager", "workflow"],
        },
        PresetSpec {
            id: "outline",
            label: "大纲伏笔",
            order: 5,
            persona: "你负责大纲、伏笔与设定的整理：对账计划与实际，找出断线、逾期未兑现、烂账，并给出可执行修正。台账的最终落库由作者决定。",
            deny: &["bash", "write", "edit", "plugin_manager", "workflow"],
        },
        PresetSpec {
            id: "reviewer",
            label: "校审",
            order: 6,
            persona: "你是校审员：只查客观错误（连续性矛盾、设定违背、伏笔合同未兑现、物理不可能、动机断裂），不评文风好坏，不代改正文。",
            deny: &["bash", "write", "edit", "plugin_manager", "workflow"],
        },
    ]
}

/// 定位 novel-pack 的安装目录（.app 内 Resources/runtime/novel-pack 或仓库 novel-pack/）。
fn pack_dir() -> Option<std::path::PathBuf> {
    let exe = std::env::current_exe().ok()?;
    let mut dir = exe.parent()?.to_path_buf();
    for _ in 0..5 {
        let rt = dir
            .join("Resources")
            .join("runtime")
            .join("novel-pack");
        if rt.join("cordis.patch.yml").exists() {
            return Some(rt);
        }
        let dev = dir.join("novel-pack");
        if dev.join("cordis.patch.yml").exists() {
            return Some(dev);
        }
        dir = dir.parent()?.to_path_buf();
    }
    None
}

/// guard.ts 的绝对路径（preset 子行要它）。
fn pack_guard_path() -> Option<std::path::PathBuf> {
    let p = pack_dir()?.join("src").join("guard.ts");
    p.exists().then_some(p)
}

/// 读 novel-pack 的 patch 模板，把插件路径重写成包内绝对路径，落到缓存目录。
///
/// 模板里的 `name:` 是构建机的绝对路径或裸包名，分发后都不成立；Loader 对绝对路径
/// 直接 pathToFileURL 加载，所以这里统一改写成“实际安装位置下的 src/index.ts”。
/// 同时把 `@zhibi/novel-pack/guard` 这类导出子路径换成同目录的 src/guard.ts。
fn rewrite_pack_patch(tpl: &std::path::Path, pack_dir: &std::path::Path) -> Option<String> {
    let text = std::fs::read_to_string(tpl).ok()?;
    let entry = pack_dir.join("src").join("index.ts");
    let guard = pack_dir.join("src").join("guard.ts");
    let entry_s = entry.display().to_string();
    let guard_s = guard.display().to_string();
    let rewritten: String = text
        .lines()
        .map(|line| {
            let t = line.trim_start();
            if !t.starts_with("name:") {
                return line.to_string();
            }
            // 保留原缩进，只换值
            let indent = &line[..line.len() - t.len()];
            if line.contains("novel-pack/guard") || line.contains("/guard'") {
                format!("{indent}name: '{guard_s}'")
            } else if line.contains("novel-pack") {
                format!("{indent}name: '{entry_s}'")
            } else {
                line.to_string()
            }
        })
        .collect::<Vec<_>>()
        .join("\n");
    let cache = std::env::temp_dir().join("zhibi-runtime");
    std::fs::create_dir_all(&cache).ok()?;
    let tag = short_hash(&pack_dir.display().to_string());
    let out = cache.join(format!("novel-pack-{tag}.patch.yml"));
    std::fs::write(&out, rewritten).ok()?;
    Some(out.display().to_string())
}

impl DshClient {
    /// 包内运行时定位：.app/Contents/Resources/runtime/{node/bin/node, dsh/lib/bin.js}。
    /// 也兼容从仓库 target 目录运行的开发态（向上找 .vendor）。
    fn bundled_runtime() -> Option<(String, String)> {
        let exe = std::env::current_exe().ok()?;
        let mut dir = exe.parent()?.to_path_buf();
        for _ in 0..5 {
            // macOS .app：MacOS/ → Contents/Resources/runtime
            let rt = dir.join("Resources").join("runtime");
            // 包内目录名沿用 dsh（build_app.sh 把 .vendor/dsh-runtime 拷成 runtime/dsh）
            let node = rt.join("node").join("bin").join("node");
            let binjs = rt.join("dsh").join("lib").join("bin.js");
            if node.exists() && binjs.exists() {
                return Some((node.display().to_string(), binjs.display().to_string()));
            }
            // 开发态：仓库根 .vendor（vendor_runtime.sh 产出 dsh-runtime；旧名 dsh 兼容）
            let v = dir.join(".vendor");
            let node2 = v.join("node").join("bin").join("node");
            for name in ["dsh-runtime", "dsh"] {
                let binjs2 = v.join(name).join("lib").join("bin.js");
                if node2.exists() && binjs2.exists() {
                    return Some((node2.display().to_string(), binjs2.display().to_string()));
                }
            }
            dir = dir.parent()?.to_path_buf();
        }
        None
    }

    /// 包内能力包 patch：Resources/runtime/novel-pack/cordis.patch.yml。
    /// patch 模板里的 plugin 路径是构建机绝对路径，不能直接用于分发——
    /// 这里把 name 行重写成包内实际路径，落到缓存目录后返回临时 patch。
    fn bundled_patch() -> Option<String> {
        let exe = std::env::current_exe().ok()?;
        let mut dir = exe.parent()?.to_path_buf();
        for _ in 0..5 {
            let rt = dir.join("Resources").join("runtime").join("novel-pack");
            let tpl = rt.join("cordis.patch.yml");
            if tpl.exists() {
                return rewrite_pack_patch(&tpl, &rt);
            }
            // 开发态：仓库内 patch
            let pack = dir.join("novel-pack");
            let dev = pack.join("cordis.patch.yml");
            if dev.exists() {
                return rewrite_pack_patch(&dev, &pack);
            }
            dir = dir.parent()?.to_path_buf();
        }
        None
    }





    /// 生成该模式的 patch 并返回路径。
    ///
    /// 三个模式共用同一份“专属 Agent 预设”清单（人物/关系/世界线/预言/大纲/校审），
    /// 差别只在主 Agent 的执行器与可用子 agent：
    /// - 托管：PTC 执行器（模型写 TS 程序批量调工具）+ subagent 可派全部预设；
    /// - 极简：native 执行器、无 subagent、只保留提案与卡片工具；
    /// - 创造：托管 + 官方 Cordis 创造工具（用于持久化本书专属 preset）。
    ///
    /// 写进 `<DSH_HOME>/zhibi-mode.patch.yml`，进程池换模式时重写即可。
    fn mode_patch(
        root: &std::path::Path,
        mode: AgentMode,
        home: &std::path::Path,
    ) -> Option<String> {
        // 作者显式给了覆盖就不用生成的：
        if std::env::var("ZHIBI_MODE_PATCH").is_ok() {
            return std::env::var("ZHIBI_MODE_PATCH").ok();
        }

        // guard 插件的绝对路径：从 novel-pack 的安装/检出位置推导。
        // 找不到就不写这一行（专属 Agent 会退回“继承全部工具”，由校审 persona 兜底，
        // 并在轨迹里提示），绝不写一个无效路径让整个预设加载失败。
        let guard_path = pack_guard_path().map(|p| p.display().to_string());
        let presets = zhibi_presets();
        let preset_ids: Vec<&str> = presets.iter().map(|p| p.id).collect();

        let mut lines: Vec<String> = vec![
            "# 由执笔生成：主 Agent 形态与专属 Agent 预设。改这里不会生效，改 app/src/dsh/agent_patch.rs。"
                .into(),
        ];

        match mode {
            AgentMode::Minimal => {
                lines.push("- id: tool-executor-native".into());
                lines.push("  disabled: false".into());
                lines.push("- id: tool-executor-ptc".into());
                lines.push("  disabled: true".into());
                // 极简档没有子 agent：把两个委派工具关掉，避免模型尝试未配置的 preset。
                lines.push("- id: tool-subagent".into());
                lines.push("  disabled: true".into());
                lines.push("- id: tool-subagent-fork".into());
                lines.push("  disabled: true".into());
                lines.push("- id: tool-workflow".into());
                lines.push("  disabled: true".into());
            }
            AgentMode::Managed | AgentMode::Creator => {
                lines.push("- id: tool-executor-native".into());
                lines.push("  disabled: true".into());
                lines.push("- id: tool-executor-ptc".into());
                lines.push("  disabled: false".into());
                lines.push("- id: tool-subagent".into());
                lines.push("  config:".into());
                lines.push("    provider: spawn".into());
                lines.push("    toolName: subagent".into());
                lines.push("    agentPreset: outline".into());
                lines.push("    allowedAgentPresets:".into());
                for id in &preset_ids {
                    lines.push(format!("      - {id}"));
                }
            }
        }

        if mode == AgentMode::Creator {
            // 官方创造模式的两个依赖：Cordis 主机侧运行器 + inspect 提供者。
            lines.push("- id: cordis-host-runner".into());
            lines.push("  disabled: false".into());
        }

        // 预设注册表 + 每个专属 Agent 一行。
        lines.push("- insert:".into());
        lines.push("    - id: agent-preset-registry".into());
        lines.push("      name: '@deepseek-ai/dsh-agent-preset-registry'".into());
        lines.push("      config:".into());
        lines.push("        default: outline".into());
        for p in &presets {
            lines.push(format!("    - id: preset-{}", p.id));
            lines.push("      name: '@deepseek-ai/dsh-agent-preset'".into());
            lines.push("      config:".into());
            lines.push(format!("        id: {}", p.id));
            lines.push(format!("        name: {}", p.label));
            lines.push(format!("        order: {}", p.order));
            lines.push("        plugins:".into());
            lines.push("          - id: persona".into());
            lines.push("            name: '@deepseek-ai/dsh-persona'".into());
            lines.push("            config:".into());
            lines.push("              prefix: >-".into());
            lines.push(format!("                {}", p.persona));
            lines.push("          - id: tool-executor-native".into());
            lines.push("            name: '@deepseek-ai/dsh-tool-executor-native'".into());
            if let Some(guard) = &guard_path {
                lines.push("          - id: readonly-guard".into());
                // 直接写绝对路径：preset 子行没有 novel-pack 的包名解析上下文，
                // 只有主插件是从 patch 的 name 行加载的。
                lines.push(format!("            name: '{guard}'"));
                lines.push("            config:".into());
                lines.push("              deny:".into());
                for d in p.deny {
                    lines.push(format!("                - {d}"));
                }
            }
        }

        let path = home.join("zhibi-mode.patch.yml");
        let body = lines.join("\n") + "\n";
        // 内容没变就不重写（避免无谓的 mtime 变化与 HMR 抖动）
        let unchanged = std::fs::read_to_string(&path)
            .map(|old| old == body)
            .unwrap_or(false);
        if !unchanged && std::fs::write(&path, body).is_err() {
            return None;
        }
        let _ = root;
        Some(path.display().to_string())
    }

    /// 启动 `dsh --profile sdk` 子进程并开始读流。
    ///
    /// `root` 是打开的项目目录，同时作为：
    /// - 子进程 cwd（DSH 的 workspace-write 边界、项目内 AGENTS.md / 技能目录的解析基准）；
    /// - `ZHIBI_ROOT`（novel-pack 的 config.root）；
    /// - `DSH_HOME` 的隔离键（每本书一个 home，见 `home_dir`）。
    ///
    /// `mode` 决定主 Agent 的形态（托管 PTC / 极简 / 创造），由 `mode_patch` 生成临时 patch。
    pub fn spawn(
        root: &std::path::Path,
        mode: AgentMode,
    ) -> Result<(Self, async_channel::Receiver<DshEvent>)> {
        // 启动顺序：ZHIBI_DSH_BIN 覆盖 → 包内 runtime/node + runtime/dsh/lib/bin.js
        //（与 .app 一起分发，不依赖全局 npm）→ PATH 里的 dsh（开发态）。
        let mut node_prefix: Option<(String, String)> = None; // (node, bin.js)
        if std::env::var("ZHIBI_DSH_BIN").is_err() {
            if let Some((node, binjs)) = Self::bundled_runtime() {
                node_prefix = Some((node, binjs));
            }
        }
        let bin = std::env::var("ZHIBI_DSH_BIN").unwrap_or_else(|_| "dsh".to_string());

        let home = home_dir(root);
        std::fs::create_dir_all(&home).ok();

        // 补丁层顺序（后者覆盖前者）：本书覆盖层 mode patch → novel-pack 能力包。
        // mode patch 由我们生成，负责执行器、专属 Agent 预设与 subagent 允许列表。
        let mut args: Vec<String> = vec!["--profile".into(), "sdk".into()];
        if let Some(patch) = Self::mode_patch(root, mode, &home) {
            args.push("--patch".into());
            args.push(patch);
        }
        if let Some(patch) = std::env::var("ZHIBI_PACK_PATCH")
            .ok()
            .or_else(|| Self::bundled_patch())
            .or_else(|| {
                let cwd = std::path::Path::new("novel-pack/cordis.patch.yml");
                cwd.exists().then(|| cwd.display().to_string())
            })
            .or_else(|| {
                // 从可执行文件位置向上找仓库（target/debug → 仓库根）
                let exe = std::env::current_exe().ok()?;
                let mut dir = exe.parent()?;
                for _ in 0..4 {
                    let cand = dir.join("novel-pack").join("cordis.patch.yml");
                    if cand.exists() { return Some(cand.display().to_string()); }
                    dir = dir.parent()?;
                }
                None
            })
        {
            args.push("--patch".into());
            args.push(patch);
        }
        let mut cmd = match &node_prefix {
            Some((node, binjs)) => {
                let mut c = Command::new(node);
                c.arg(binjs);
                c
            }
            None => Command::new(&bin),
        };
        let mut child = cmd
            .args(&args)
            .current_dir(root)
            .env("DSH_HOME", &home)
            .env("ZHIBI_ROOT", root)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit())
            .spawn()
            .with_context(|| format!("启动 {bin} --profile sdk 失败（可用 ZHIBI_DSH_BIN 指定可执行文件路径）"))?;

        let stdin = child.stdin.take().context("stdin 未接管")?;
        let stdout = child.stdout.take().context("stdout 未接管")?;

        let (ui_tx, ui_rx) = async_channel::unbounded::<DshEvent>();
        let inner = Arc::new(Inner {
            stdin: Mutex::new(Some(stdin)),
            child: Mutex::new(Some(child)),
            pending: Mutex::new(HashMap::new()),
            next_id: AtomicI64::new(1),
            ui_tx,
        });

        // 读线程：一行一个 JSON-RPC 消息，分类路由
        let reader_inner = Arc::clone(&inner);
        std::thread::Builder::new()
            .name("dsh-stdio-reader".into())
            .spawn(move || Self::read_loop(reader_inner, stdout))
            .context("创建读线程失败")?;

        Ok((Self { inner }, ui_rx))
    }

    fn read_loop(inner: Arc<Inner>, stdout: std::process::ChildStdout) {
        let reader = BufReader::new(stdout);
        for line in reader.lines() {
            let Ok(line) = line else {
                break;
            };
            if line.trim().is_empty() {
                continue;
            }
            let Ok(msg) = serde_json::from_str::<Value>(&line) else {
                let _ = inner.ui_tx.send_blocking(DshEvent::Error(format!(
                    "无法解析的消息：{line:.200}"
                )));
                continue;
            };
            Self::route(&inner, msg);
        }
        let _ = inner.ui_tx.send_blocking(DshEvent::Error(
            "DSH 运行时已退出（stdout 关闭）——轨迹历史保留在会话日志里。".into(),
        ));
        // 清空 pending：所有仍在等待响应的 call() 立刻拿到通道关闭错误，
        // 而不是永久挂在 rx.recv()（聊天停在「思考中…」）。
        if let Ok(mut pending) = inner.pending.lock() {
            pending.clear();
        }
    }

    fn route(inner: &Arc<Inner>, msg: Value) {
        let method = msg.get("method").and_then(|m| m.as_str()).map(String::from);
        // 服务端请求的 id 可能是字符串也可能是数字，原样回传才不会错配。
        let req_id = msg.get("id").cloned().filter(|v| !v.is_null());
        let id = msg.get("id").and_then(|v| v.as_i64());

        match (method, req_id) {
            // 响应：唤醒等待中的 call()
            (None, Some(_)) => {
                let sender = inner
                    .pending
                    .lock()
                    .ok()
                    .and_then(|mut p| id.and_then(|i| p.remove(&i)));
                if let Some(tx) = sender {
                    let _ = tx.send_blocking(msg);
                }
            }
            // 服务端请求：审批与提问转给 GUI 应答（拒答会 fail-closed 拒绝该操作）
            (Some(m), Some(rid)) => {
                let params = msg.get("params").cloned().unwrap_or(Value::Null);
                match m.as_str() {
                    "approval/request" => {
                        let _ = inner.ui_tx.send_blocking(DshEvent::Approval {
                            id: rid,
                            session_id: str_at(&params, "sessionId"),
                            tool: str_at(&params, "toolName"),
                            reason: str_at(&params, "reason"),
                        });
                    }
                    "user-question/ask" => {
                        let _ = inner.ui_tx.send_blocking(DshEvent::Question {
                            id: rid,
                            session_id: str_at(&params, "sessionId"),
                            request_id: str_at(&params, "requestId"),
                            questions: params.get("questions").cloned().unwrap_or(Value::Null),
                        });
                    }
                    // 其余服务端请求一律按「未实现」回，避免 DSH 端永久等待
                    other => {
                        let response = json!({
                            "jsonrpc": "2.0",
                            "id": rid,
                            "error": {"code": -32601, "message": format!("执笔未处理服务端请求：{other}")}
                        });
                        let _ = inner.write_line(&response);
                    }
                }
            }

            // 通知：转发给 UI
            (Some(m), None) => {
                let params = msg.get("params").cloned().unwrap_or(Value::Null);
                let _ = inner.ui_tx.send_blocking(DshEvent::Notification {
                    method: m,
                    params,
                });
            }
            // 无 method 无 id 的杂散消息
            _ => {}
        }
    }

    /// 应答一次审批请求。`outcome` 取值：`allowed-once` / `rejected` / `cancelled`。
    /// DSH 对无应答、非法应答一律 fail-closed（当作拒绝），所以 GUI 必须回。
    pub fn answer_approval(&self, id: &Value, outcome: &str) -> Result<()> {
        let outcome = match outcome {
            "allowed-once" | "rejected" | "cancelled" => outcome,
            other => return Err(anyhow!("非法审批结果：{other}")),
        };
        self.inner.write_line(&json!({
            "jsonrpc": "2.0",
            "id": id,
            "result": { "outcome": outcome }
        }))
    }

    /// 应答一次提问请求。`answers` 形如 `[{"id":"q1","selected":["A"]}]`；
    /// 传 `None` 表示用户取消（DSH 会结束本轮，不视为批准）。
    pub fn answer_question(&self, id: &Value, answers: Option<Value>) -> Result<()> {
        let result = match answers {
            Some(a) => json!({ "answers": a }),
            None => json!({ "cancelled": true }),
        };
        self.inner.write_line(&json!({
            "jsonrpc": "2.0",
            "id": id,
            "result": result
        }))
    }

    /// 单次 RPC 的最长等待：运行时若卡死/静默退出，调用方必须拿到错误而不是永久挂起。
    const CALL_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(120);

    /// 发出请求并等待对应响应（同 id 唤醒；超时/进程退出都会返回错误）。
    pub async fn call(&self, method: &str, params: Value) -> Result<Value> {
        let id = self.inner.next_id.fetch_add(1, Ordering::SeqCst);
        let (tx, rx) = async_channel::bounded::<Value>(1);
        self.inner
            .pending
            .lock()
            .map_err(|_| anyhow!("pending 表不可用"))?
            .insert(id, tx);

        let request = json!({
            "jsonrpc": "2.0",
            "id": id,
            "method": method,
            "params": params,
        });
        if let Err(e) = self.inner.write_line(&request) {
            // 写不出去就不要挂着等一个永远不会来的响应
            if let Ok(mut pending) = self.inner.pending.lock() {
                pending.remove(&id);
            }
            return Err(anyhow!("{method} 发送失败：{e}"));
        }

        // 看门狗：超时仍未收到响应 → 从 pending 表移除 sender，recv 立即得 Err。
        // 每请求一个短生命周期线程，读写端都不依赖额外 async 运行时。
        let timeout_inner = Arc::clone(&self.inner);
        let timeout_id = id;
        let timeout_method = method.to_string();
        let _ = std::thread::Builder::new()
            .name("dsh-call-timeout".into())
            .spawn(move || {
                std::thread::sleep(Self::CALL_TIMEOUT);
                if let Ok(mut pending) = timeout_inner.pending.lock() {
                    if let Some(tx) = pending.remove(&timeout_id) {
                        let _ = tx.send_blocking(json!({
                            "jsonrpc": "2.0",
                            "id": timeout_id,
                            "error": {"code": -32000, "message": format!("{timeout_method} 超时（120s 无响应）")}
                        }));
                    }
                }
            });

        let response = rx
            .recv()
            .await
            .map_err(|_| anyhow!("请求 {method} 的通道已关闭（运行时可能已退出）"))?;

        if let Some(err) = response.get("error") {
            let message = err
                .get("message")
                .and_then(|m| m.as_str())
                .unwrap_or("未知错误");
            return Err(anyhow!("{method} 失败：{message}"));
        }
        Ok(response.get("result").cloned().unwrap_or(Value::Null))
    }

    /// 进程级握手：记录 cwd，选择 provider/model 路由。
    ///
    /// 同时打开两个能力（缺了就无法渲染审批卡与提问弹窗）：
    /// - `approvalRequests`：让服务端把审批发给客户端（创造模式装 bundle、越权写入都要它）；
    /// - `capabilities.userQuestions`：让 `ask_user_question` 走客户端弹窗。
    pub async fn initialize(&self, cwd: &str, provider: &str, model: &str) -> Result<Value> {
        self.call(
            "initialize",
            json!({
                "cwd": cwd,
                "provider": provider,
                "model": model,
                "approvalRequests": true,
                "capabilities": { "userQuestions": true },
            }),
        )
        .await
    }

    /// 会话级查询：未知 id 会惰性创建 session + agent。
    pub async fn session_query(&self, session_id: &str, text: &str) -> Result<Value> {
        self.call(
            "session/query",
            json!({
                "sessionId": session_id,
                "contentBlocks": [{"type": "text", "text": text}],
            }),
        )
        .await
    }

    /// 接回一个已持久化的会话（进程重启、切模式之后用）。
    ///
    /// 实测：新进程 resume 后上下文完整——上一轮说过的词还记得。会话历史在 DSH 的
    /// jsonl 里，所以切模式重启不会丢对话。
    ///
    /// 会话不存在（第一次启动）会被拒；调用方应把它当成“不需要 resume”，不是错误。
    pub async fn session_resume(&self, session_id: &str) -> Result<Value> {
        self.call("session/resume", json!({ "sessionId": session_id }))
            .await
    }

    /// 结束运行时（M2 接退出流程；M1 仅保留 API）。
    #[allow(dead_code)]
    pub fn shutdown(&self) {
        let _ = self.call("shutdown", Value::Null);
        if let Ok(mut guard) = self.inner.child.lock() {
            if let Some(child) = guard.as_mut() {
                let _ = child.kill();
            }
            *guard = None;
        }
        if let Ok(mut guard) = self.inner.stdin.lock() {
            *guard = None;
        }
    }
}

/// M1 模型路由：sdk server 只对 `deepseek-official` 做兜底适配器挂载；
/// 自定义 OpenAI 兼容端点在 M3 由 novel-pack 挂适配器后走环境变量覆盖。
pub fn model_route() -> (String, String) {
    let provider = std::env::var("ZHIBI_PROVIDER").unwrap_or_else(|_| "deepseek-official".into());
    let model = std::env::var("ZHIBI_MODEL").unwrap_or_else(|_| "deepseek-chat".into());
    (provider, model)
}

#[allow(dead_code)]
pub fn env_note() -> String {
    "配置：ZHIBI_DSH_BIN（dsh 可执行文件）、ZHIBI_DSH_HOME（覆盖每书 home）、\
ZHIBI_MODE_PATCH（覆盖生成的模式 patch）、ZHIBI_PROVIDER、ZHIBI_MODEL"
        .to_string()
}

/// 读 JSON 里的字符串字段，缺失或类型不对时给空串（协议字段都会演进，不能 panic）。
fn str_at(v: &Value, key: &str) -> String {
    v.get(key)
        .and_then(|x| x.as_str())
        .unwrap_or("")
        .to_string()
}


#[cfg(test)]
mod tests {
    use super::*;

    fn tmp_home(tag: &str) -> std::path::PathBuf {
        let p = std::env::temp_dir().join(format!("zhibi-mode-test-{tag}"));
        std::fs::create_dir_all(&p).unwrap();
        p
    }

    fn make_patch(mode: AgentMode, tag: &str) -> String {
        let home = tmp_home(tag);
        let patch = DshClient::mode_patch(std::path::Path::new("/tmp/book"), mode, &home)
            .expect("应生成 patch");
        std::fs::read_to_string(patch).unwrap()
    }

    #[test]
    fn managed_mode_uses_ptc_and_lists_all_presets() {
        let yml = make_patch(AgentMode::Managed, "managed");
        assert!(yml.contains("- id: tool-executor-ptc\n  disabled: false"));
        assert!(yml.contains("- id: tool-executor-native\n  disabled: true"));
        // 主 agent 要能派所有专属 Agent
        for id in ["character", "relations", "worldline", "oracle", "outline", "reviewer"] {
            assert!(yml.contains(&format!("      - {id}")), "managed 缺 {id} 的允许列表");
            assert!(yml.contains(&format!("        id: {id}")), "managed 缺 {id} 的预设声明");
        }
        assert!(yml.contains("agent-preset-registry"));
    }

    #[test]
    fn minimal_mode_disables_delegation_and_ptc() {
        let yml = make_patch(AgentMode::Minimal, "minimal");
        assert!(yml.contains("- id: tool-executor-native\n  disabled: false"));
        assert!(yml.contains("- id: tool-executor-ptc\n  disabled: true"));
        assert!(yml.contains("- id: tool-subagent\n  disabled: true"));
        assert!(!yml.contains("allowedAgentPresets"));
    }

    #[test]
    fn creator_mode_adds_cordis_runner() {
        let yml = make_patch(AgentMode::Creator, "creator");
        assert!(yml.contains("- id: tool-executor-ptc\n  disabled: false"));
        assert!(yml.contains("- id: cordis-host-runner\n  disabled: false"));
    }

    #[test]
    fn every_preset_carries_a_readonly_guard_deny_list() {
        let yml = make_patch(AgentMode::Managed, "guard");
        // grader 存在时，每个 preset 都要有 guard 且至少 deny 掉写工具
        if yml.contains("readonly-guard") {
            assert_eq!(
                yml.matches("- id: readonly-guard").count(),
                zhibi_presets().len(),
                "每个专属 Agent 都要挂 guard"
            );
            assert!(yml.contains("                - write"));
            assert!(yml.contains("                - bash"));
        }
    }

    #[test]
    fn patch_is_stable_across_calls() {
        // 内容没变就不该重写（避免 mtime 抖动触发 HMR/preset revision 变化）
        let home = tmp_home("stable");
        let a = DshClient::mode_patch(std::path::Path::new("/tmp/book"), AgentMode::Managed, &home)
            .unwrap();
        let m1 = std::fs::metadata(&a).unwrap().modified().unwrap();
        std::thread::sleep(std::time::Duration::from_millis(20));
        DshClient::mode_patch(std::path::Path::new("/tmp/book"), AgentMode::Managed, &home).unwrap();
        let m2 = std::fs::metadata(&a).unwrap().modified().unwrap();
        assert_eq!(m1, m2, "相同内容不应重写文件");
    }

    #[test]
    fn home_dir_is_isolated_per_book_and_stable() {
        let a = home_dir(std::path::Path::new("/tmp/book-a"));
        let b = home_dir(std::path::Path::new("/tmp/book-b"));
        let a2 = home_dir(std::path::Path::new("/tmp/book-a"));
        assert_ne!(a, b, "不同书要隔离");
        assert_eq!(a, a2, "同一书必须稳定（否则每次启动都换 home，凭据与 preset 全丢）");
        assert!(a.to_string_lossy().contains("zhibi/homes"));
    }
}
