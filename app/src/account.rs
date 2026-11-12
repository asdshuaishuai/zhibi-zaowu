//! DeepSeek 账号：读本地凭据、查余额。
//!
//! 凭据来源与 DSH 共用同一个文件（`$DSH_HOME/.credentials.yaml`），不另起一套：
//! 账号 grant 在 `records["deepseek-account-platform/default"]` 的 `payload.token`。
//! 我们只读不改——登录仍由 DSH 自己的流程完成，避免两处写同一个 0600 文件。
//!
//! 余额接口是 `platform.deepseek.com/api/v0/users/get_user_summary`，
//! 必须带一个像浏览器的 `User-Agent`，否则被前置防护直接挡成 HTML 错误页
//!（实测：不带 UA 返回 "Request Blocked" 页面，带 UA 返回正常 JSON）。

use anyhow::{Result, anyhow};
use serde_json::Value;

/// 账号快照：登没登录、余额多少。
#[derive(Debug, Clone, Default)]
pub struct AccountSnapshot {
    /// 是否读到了可用的账号 grant。
    pub signed_in: bool,
    /// 账本余额（CNY），形如 "19.8911..."；没有就是空串。
    pub balance: String,
    /// 赠金余额。
    pub bonus: String,
    /// 累计消费。
    pub spent: String,
    /// 读失败的原因（凭据缺失、网络、接口变更都会落在这里）。
    pub error: Option<String>,
}

/// 从 DSH 的凭据文件里取账号 grant 的 token。
///
/// 这是一个极小的 YAML 读取：只找 `token:` 那一行。上完整 YAML 解析器只为了
/// 一个字段不值得，而且 DSH 的凭据结构可能演进，这里刻意只依赖最小形状。
pub fn read_grant_token(home: &std::path::Path) -> Option<String> {
    let path = home.join(".credentials.yaml");
    let text = std::fs::read_to_string(path).ok()?;
    // 只在 account 记录段里找，避免误取 browser-session 的 secret
    let start = text.find("deepseek-account-platform/default")?;
    let tail = &text[start..];
    let end = tail.find("- ").unwrap_or(tail.len());
    for line in tail[..end].lines() {
        let t = line.trim();
        if let Some(rest) = t.strip_prefix("token:") {
            let v = rest.trim().trim_matches(['"', '\'']);
            if !v.is_empty() {
                return Some(v.to_string());
            }
        }
    }
    None
}

/// 查余额。同步阻塞（ureq），调用方应在后台线程里跑。
pub fn fetch_balance(home: &std::path::Path) -> AccountSnapshot {
    let Some(token) = read_grant_token(home) else {
        return AccountSnapshot {
            signed_in: false,
            error: Some("未找到账号凭据（可在 DSH 里登录一次，或直接配置 API Key）".into()),
            ..Default::default()
        };
    };
    match request_summary(&token) {
        Ok(v) => {
            let data = v
                .get("data")
                .and_then(|d| d.get("biz_data"))
                .cloned()
                .unwrap_or(Value::Null);
            let pick = |key: &str, field: &str| -> String {
                data.get(key)
                    .and_then(|a| a.as_array())
                    .and_then(|a| a.first())
                    .and_then(|o| o.get(field))
                    .and_then(|f| f.as_str())
                    .map(|s| trim_decimal(s))
                    .unwrap_or_default()
            };
            AccountSnapshot {
                signed_in: true,
                balance: pick("normal_wallets", "balance"),
                bonus: pick("bonus_wallets", "balance"),
                spent: pick("total_costs", "amount"),
                error: None,
            }
        }
        Err(e) => AccountSnapshot {
            signed_in: true,
            error: Some(format!("余额查询失败：{e}")),
            ..Default::default()
        },
    }
}

/// "19.8911756800000000" → "19.89"：接口给的精度远超显示需要。
fn trim_decimal(s: &str) -> String {
    match s.split_once('.') {
        Some((int, frac)) => {
            let frac: String = frac.chars().take(2).collect();
            format!("{int}.{frac}")
        }
        None => s.to_string(),
    }
}

fn request_summary(token: &str) -> Result<Value> {
    // UA 是必须的：缺了会被前置防护挡成 HTML 页面（实测）
    const UA: &str = "Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) \
AppleWebKit/537.36 (KHTML, like Gecko) Chrome/131.0.0.0 Safari/537.36";
    let resp = ureq::get("https://platform.deepseek.com/api/v0/users/get_user_summary")
        .set("x-dsh-auth-token", token)
        .set("accept", "application/json")
        .set("user-agent", UA)
        .timeout(std::time::Duration::from_secs(20))
        .call()
        .map_err(|e| anyhow!("{e}"))?;
    let body = resp.into_string().map_err(|e| anyhow!("{e}"))?;
    let v: Value = serde_json::from_str(&body)
        .map_err(|_| anyhow!("接口返回的不是 JSON（可能被防护拦截）"))?;
    // 业务码非 0 说明 token 失效
    let code = v.get("code").and_then(|c| c.as_i64()).unwrap_or(-1);
    if code != 0 {
        return Err(anyhow!(
            "接口业务码 {code}（凭据可能已失效，可在 DSH 里重新登录）"
        ));
    }
    Ok(v)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_grant_token_only_from_account_record() {
        let dir = std::env::temp_dir().join("zhibi-account-test");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(
            dir.join(".credentials.yaml"),
            "version: 1\nrecords:\n  client-connection/browser-session:\n    kind: grant\n    \
payload:\n      secret: browser-secret\n  deepseek-account-platform/default:\n    kind: grant\n    \
payload:\n      version: 1\n      token: account-token-abc\n      issuer: \
https://platform.deepseek.com\n",
        )
        .unwrap();
        // 必须取账号 token，不能误取 browser-session 的 secret
        assert_eq!(read_grant_token(&dir).as_deref(), Some("account-token-abc"));
    }

    #[test]
    fn missing_credentials_report_signed_out() {
        let dir = std::env::temp_dir().join("zhibi-account-test-empty");
        std::fs::create_dir_all(&dir).unwrap();
        let _ = std::fs::remove_file(dir.join(".credentials.yaml"));
        let snap = fetch_balance(&dir);
        assert!(!snap.signed_in);
        assert!(snap.error.is_some(), "缺凭据要给出可读原因");
    }

    #[test]
    fn trims_balance_to_two_decimals() {
        assert_eq!(trim_decimal("19.8911756800000000"), "19.89");
        assert_eq!(trim_decimal("0"), "0");
        // 只截断不补齐：接口给多少有效位就显示多少，不假装精度
        assert_eq!(trim_decimal("100.5"), "100.5");
    }
}
