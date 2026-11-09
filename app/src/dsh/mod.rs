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
    fn write_line(&self, msg: &Value) {
        let mut line = msg.to_string();
        line.push('\n');
        if let Ok(mut guard) = self.stdin.lock() {
            if let Some(stdin) = guard.as_mut() {
                let _ = stdin.write_all(line.as_bytes());
                let _ = stdin.flush();
            }
        }
    }
}

/// 可跨线程共享的客户端句柄（Workspace 持有，读线程与请求方共用）。
#[derive(Clone)]
pub struct DshClient {
    inner: Arc<Inner>,
}

impl DshClient {
    /// 启动 `dsh --profile sdk` 子进程并开始读流。root 传入打开的项目目录
    /// （作为 ZHIBI_ROOT 环境变量交给 novel-pack 的 config.root）。
    pub fn spawn(root: &std::path::Path) -> Result<(Self, async_channel::Receiver<DshEvent>)> {
        let bin = std::env::var("ZHIBI_DSH_BIN").unwrap_or_else(|_| "dsh".to_string());
        // novel-pack 能力包：ZHIBI_PACK_PATCH 指定；默认找仓库相对路径
        let mut args: Vec<String> = vec!["--profile".into(), "sdk".into()];
        let patch = std::env::var("ZHIBI_PACK_PATCH")
            .ok()
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
            });
        if let Some(patch) = patch {
            args.push("--patch".into());
            args.push(patch);
        }
        let mut child = Command::new(&bin)
            .args(&args)
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
    }

    fn route(inner: &Arc<Inner>, msg: Value) {
        let method = msg.get("method").and_then(|m| m.as_str()).map(String::from);
        let id = msg.get("id").and_then(|v| v.as_i64());

        match (method, id) {
            // 响应：唤醒等待中的 call()
            (None, Some(id)) => {
                let sender = inner
                    .pending
                    .lock()
                    .ok()
                    .and_then(|mut p| p.remove(&id));
                if let Some(tx) = sender {
                    let _ = tx.send_blocking(msg);
                }
            }
            // 服务端请求（审批/提问等）：M1 未启用能力，统一按方法未实现回复
            (Some(m), Some(id)) => {
                let response = json!({
                    "jsonrpc": "2.0",
                    "id": id,
                    "error": {"code": -32601, "message": format!("执笔 M1 未处理服务端请求：{m}")}
                });
                inner.write_line(&response);
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

    /// 发出请求并等待对应响应（同 id 唤醒）。
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
        self.inner.write_line(&request);

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
    pub async fn initialize(&self, cwd: &str, provider: &str, model: &str) -> Result<Value> {
        self.call(
            "initialize",
            json!({
                "cwd": cwd,
                "provider": provider,
                "model": model,
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
    "配置：ZHIBI_DSH_BIN（dsh 可执行文件）、ZHIBI_PROVIDER、ZHIBI_MODEL".to_string()
}
