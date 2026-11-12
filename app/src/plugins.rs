//! 内置插件清单与开关。
//!
//! ## 为什么不是读 package.json
//!
//! 计划里写的是"读 `plugins.json`（由安装的 package.json 生成：id、包名、说明、
//! 必需/可选）"。实际落地改成**在代码里维护一张表**，理由有两个：
//!
//! 1. package.json 只知道包名，不知道"这个包在 patch 里对应哪一行"、
//!    "关掉它作者会失去什么"。能开关的最小单位是 **patch 行 id**，不是 npm 包——
//!    一个包可能插入三行（`tool-cordis` + `cordis-host-runner` + provider 注册）。
//! 2. 关错的后果差别很大：关掉 `tool-bash` 只是少了跑命令的能力，关掉
//!    `session-persistence-jsonl` 会让所有会话历史消失。这个区别只能人工标注。
//!
//! 所以这张表只收**作者可以安全开关**的行，其余一律不出现在界面上。
//!
//! ## 必需项锁定
//!
//! `required: true` 的项在界面上显示为锁定（不可点）。它们是"关掉之后产品
//! 就不成立了"的那些：会话持久化、审批、权限、提案工具。
//!
//! ## 生效时机
//!
//! 写进 `settings.json` 的 `pluginOverrides`，`dsh::mode_patch` 生成 patch 时
//! 按它追加 `disabled` 行。也就是说下次启动运行时生效——与"模型/联网"这些
//! 设置项一致，不需要单独一条生效路径。

/// 一个可开关的插件（= patch 里的一行）。
pub struct PluginSpec {
    /// patch 行 id（`- id: ...`），关掉就是给这一行 `disabled: true`
    pub row: &'static str,
    /// 界面上的名字
    pub label: &'static str,
    /// 一行说明：它是干什么的
    pub what: &'static str,
    /// 关掉之后作者会失去什么。这一段比 `what` 重要——
    /// 作者是在做减法，必须知道代价。
    pub without: &'static str,
    /// 分组（界面按组排）
    pub group: Group,
    /// 必需项：关不掉（关掉产品就不成立）
    pub required: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Group {
    /// 写作能力：直接影响 AI 能做什么
    Writing,
    /// 工具面：AI 可以动的东西
    Tools,
    /// 运行与诊断
    Runtime,
}

impl Group {
    pub fn label(self) -> &'static str {
        match self {
            Group::Writing => "写作能力",
            Group::Tools => "工具面",
            Group::Runtime => "运行与诊断",
        }
    }
}

/// 可开关的插件清单。
pub const PLUGINS: &[PluginSpec] = &[
    PluginSpec {
        row: "tool-web",
        label: "联网检索",
        what: "search / fetch：查资料、核对史实与名词",
        without: "AI 只能靠正文与档案里的内容，不能上网查",
        group: Group::Writing,
        required: false,
    },
    PluginSpec {
        row: "tool-subagent",
        label: "子 Agent（专属 Agent 派活）",
        what: "把任务派给人物 / 关系 / 世界线 / 预言 / 大纲 / 校审",
        without: "所有活都由主 Agent 自己干，专属 Agent 形同虚设",
        group: Group::Writing,
        required: false,
    },
    PluginSpec {
        row: "goal",
        label: "长任务目标（goal）",
        what: "跨回合的写作目标：AI 自己排步骤、跟进、收尾",
        without: "长任务要作者分成多次说，AI 不会自己接着往下做",
        group: Group::Writing,
        required: false,
    },
    PluginSpec {
        row: "tool-ask-user",
        label: "反问作者",
        what: "信息不够时 AI 可以停下来问一句，而不是自己编",
        without: "信息不足时 AI 只能猜——猜错就是设定漂移",
        group: Group::Writing,
        required: false,
    },
    PluginSpec {
        row: "tool-bash",
        label: "命令行",
        what: "跑 shell 命令（统计字数、批量改名、跑校验脚本）",
        without: "AI 只能用文件读写工具，碰不了命令行",
        group: Group::Tools,
        required: false,
    },
    PluginSpec {
        row: "tool-fs",
        label: "文件读写",
        what: "读写书目录里的文件（正文、台账、设定）",
        without: "AI 读不到正文，也写不了任何东西——等于废掉",
        group: Group::Tools,
        required: true,
    },
    PluginSpec {
        row: "tool-fs-search",
        label: "全文搜索",
        what: "在全书里搜人名、名词、句子（AI 的追查手段）",
        without: "AI 只能整章读，长书里查一个细节会翻很久",
        group: Group::Tools,
        required: false,
    },
    PluginSpec {
        row: "tool-jobs",
        label: "后台任务",
        what: "长时间命令放后台跑，AI 边等边做别的",
        without: "长命令会卡住当前回合",
        group: Group::Tools,
        required: false,
    },
    PluginSpec {
        row: "mcp-client",
        label: "MCP 外部工具",
        what: "接外部 MCP server（自定义词典、资料库等）",
        without: "只能用内置工具",
        group: Group::Tools,
        required: false,
    },
    PluginSpec {
        row: "session-persistence-jsonl",
        label: "会话历史落盘",
        what: "把对话写成 jsonl，重启 / 切模式后 resume 接回来",
        without: "关掉就会话全丢——切一次模式等于重新认识这本书",
        group: Group::Runtime,
        required: true,
    },
    PluginSpec {
        row: "approval",
        label: "审批门禁",
        what: "危险操作（写正文、删文件）先问作者",
        without: "AI 可以直接动手，事后才知道改了什么",
        group: Group::Runtime,
        required: true,
    },
    PluginSpec {
        row: "compaction-basic",
        label: "上下文压缩",
        what: "长会话自动压缩，免得超窗口后失忆",
        without: "写久了 AI 会忘记前面的内容，直到报错",
        group: Group::Runtime,
        required: false,
    },
    PluginSpec {
        row: "session-telemetry-otel",
        label: "性能追踪（OTel）",
        what: "把一轮的耗时、token 用量打到 OTel（排查卡顿用）",
        without: "卡的时候看不到是哪一步慢",
        group: Group::Runtime,
        required: false,
    },
];

/// 按分组给出清单（界面直接照着渲染）。
pub fn grouped() -> Vec<(Group, Vec<&'static PluginSpec>)> {
    let mut out: Vec<(Group, Vec<&'static PluginSpec>)> = Vec::new();
    for g in [Group::Writing, Group::Tools, Group::Runtime] {
        let items: Vec<&'static PluginSpec> = PLUGINS.iter().filter(|p| p.group == g).collect();
        if !items.is_empty() {
            out.push((g, items));
        }
    }
    out
}

pub fn find(row: &str) -> Option<&'static PluginSpec> {
    PLUGINS.iter().find(|p| p.row == row)
}

/// 这个插件此刻是开着的吗。
///
/// 默认开。必需的项永远算开着（即使设置里被写成了 false——那可能是手改
/// settings.json 留下的，不能让它真的把产品关死）。
pub fn enabled(overrides: &std::collections::BTreeMap<String, bool>, row: &str) -> bool {
    if let Some(spec) = find(row) {
        if spec.required {
            return true;
        }
    }
    overrides.get(row).copied().unwrap_or(true)
}

/// 生成要追加到 patch 的 `disabled` 行。
///
/// 只写**被关掉**的那些：默认全开，写一堆 `disabled: false` 只会让
/// patch 文件更难读，而且升级时如果 DSH 加了一行同 id 的新配置，
/// 我们这条空写的行会把它整个盖掉（patch 是按行覆盖的）。
pub fn disabled_rows(
    overrides: &std::collections::BTreeMap<String, bool>,
) -> Vec<(&'static str, bool)> {
    PLUGINS
        .iter()
        .filter(|p| !p.required && !enabled(overrides, p.row))
        .map(|p| (p.row, true))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_required_plugin_cannot_be_disabled() {
        let mut over = std::collections::BTreeMap::new();
        over.insert("tool-fs".to_string(), false);
        // 手改 settings.json 把必需项关掉了：不能让它真的生效
        assert!(enabled(&over, "tool-fs"));
        assert!(!disabled_rows(&over).iter().any(|(r, _)| *r == "tool-fs"));
    }

    #[test]
    fn default_is_enabled_and_only_off_switches_are_written() {
        let over = std::collections::BTreeMap::new();
        let rows = disabled_rows(&over);
        assert!(rows.is_empty(), "默认全开，不该写任何 disabled 行：{rows:?}");
        assert!(enabled(&over, "tool-web"));
    }

    #[test]
    fn turning_one_off_writes_exactly_one_row() {
        let mut over = std::collections::BTreeMap::new();
        over.insert("tool-web".to_string(), false);
        let rows = disabled_rows(&over);
        assert_eq!(rows, vec![("tool-web", true)]);
        assert!(!enabled(&over, "tool-web"));
        assert!(enabled(&over, "tool-bash"), "关一个不能连坐别的");
    }

    #[test]
    fn every_row_id_looks_like_a_patch_row() {
        // 这些字符串直接拼进 `- id: {}`，写错了会静默不生效（DSH 对
        // 不存在的 id 什么都不做）。这里至少挡住空格和空串。
        for p in PLUGINS {
            assert!(!p.row.is_empty());
            assert!(!p.row.contains(' '), "行 id 里不能有空格：{}", p.row);
            assert!(!p.label.is_empty() && !p.what.is_empty() && !p.without.is_empty());
        }
    }

    #[test]
    fn row_ids_are_unique() {
        let mut seen = std::collections::BTreeSet::new();
        for p in PLUGINS {
            assert!(seen.insert(p.row), "重复的行 id：{}", p.row);
        }
    }

    #[test]
    fn groups_cover_every_plugin() {
        let total: usize = grouped().iter().map(|(_, v)| v.len()).sum();
        assert_eq!(total, PLUGINS.len(), "有插件没被任何分组收进去");
    }
}
