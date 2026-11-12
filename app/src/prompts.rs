//! 执笔提示词库（Rust 侧）：每个 AI 能力的触发语。
//!
//! M8 之后这里只做两件事：**说清目标与参数**，以及**指向对应技能**。
//! "怎么写"的部分全部搬到 `novel-pack/skills/zhibi-*.md` —— 那是纯文本，
//! 作者能改、AI 能加载、不用重编客户端。
//!
//! 为什么不干脆不发提示词、让模型自己 load 技能：UI 上的按钮必须一次点准。
//! 主 Agent 有很多技能可选，"搭骨架"点下去要立刻走对路径，不能靠模型猜测。
//! 所以这里保留一句明确的触发语 + 技能名，细节留给技能正文。

/// 提案依据要求：所有产出提案的任务书共用；工具侧 sourceRefs / 收件箱依据区配套。
const BASIS_RULE: &str = "提案须给出依据来源（引用既有伏笔/设定/事实编号）。";

/// 能力和技能的对应：触发语里点名技能，模型就去 load。
///
/// 技能名与 `novel-pack/skills/*.md` 的 frontmatter `name` 一致；
/// 名字写错不会报错，只会让模型加载失败后自己找路——所以这里集中管理。
fn skill_of(capability: &str) -> Option<&'static str> {
    Some(match capability {
        "skeleton" => "zhibi-skeleton",
        "draft" => "zhibi-draft",
        "validate" => "zhibi-validate",
        "deslop" => "zhibi-deslop",
        "memory" => "zhibi-memory",
        "inventory" => "zhibi-inventory",
        "classify" => "zhibi-canon-classify",
        "outsync" => "zhibi-outline-sync",
        "audit" => "zhibi-audit",
        "dossier" => "zhibi-dossier",
        "continue" => "zhibi-continue",
        "branches" => "zhibi-branches",
        _ => return None,
    })
}

pub fn task_for(capability: &str, chapter: u32) -> String {
    // 纯输出能力不产提案，也不加依据要求（它们只产出文本/卡片）。
    let produce = !matches!(capability, "continue" | "branches");

    let body = match capability {
        "skeleton" => format!("为第{chapter}章搭骨架。"),
        "draft" => format!("为第{chapter}章写草稿（按已批准骨架）。"),
        "validate" => format!("验证第{chapter}章。"),
        "deslop" => format!("第{chapter}章去AI味。"),
        "memory" => format!("第{chapter}章写完，记一笔账。"),
        "dossier" => format!("提取第{chapter}章档案卡。"),
        "inventory" => "全书伏笔盘点。".into(),
        "classify" => "设定归类。".into(),
        "outsync" => "大纲同步：对账计划与实际。".into(),
        "audit" => "全书连贯性审查。".into(),
        // 轻交互走结构化工具：前端直接读工具参数渲染卡片，不再解析文本。
        "continue" => "续写：读出正文最后一段，给出紧接的下一句。".into(),
        "branches" => "给 3 个故事走向。".into(),
        _ => "读项目现状，给出下一步建议。".into(),
    };

    match skill_of(capability) {
        Some(skill) => {
            if produce {
                format!("{body}先 `skill {skill}` 加载该能力的作业规程并照做。{BASIS_RULE}")
            } else {
                format!("{body}先 `skill {skill}` 加载规程并照做。")
            }
        }
        None => body,
    }
}

pub struct AiButton {
    pub label: &'static str,
    pub capability: &'static str,
}

pub const EDITOR_AI_BUTTONS: &[AiButton] = &[
    AiButton { label: "✦ 续写一句", capability: "continue" },
    AiButton { label: "✦ 给我几个走向", capability: "branches" },
    AiButton { label: "搭骨架", capability: "skeleton" },
    AiButton { label: "一键验证", capability: "validate" },
    AiButton { label: "去AI味", capability: "deslop" },
    AiButton { label: "让AI记一笔", capability: "memory" },
    AiButton { label: "记档案卡", capability: "dossier" },
    AiButton { label: "AI 盘点伏笔", capability: "inventory" },
    AiButton { label: "AI 归类设定", capability: "classify" },
];

pub fn selection_instruction(kind: &str, selected: &str) -> String {
    let verb = match kind {
        "deepen" => "拉深",
        "expand" => "扩写",
        "condense" => "缩写",
        "rewrite" => "改写",
        "deslop" => "去AI味改写",
        _ => "改写",
    };
    // 原文必须原样附上：技能里强调过"original 逐字复制"，这里把要改的那段直接给到，
    // 模型不需要自己去正文里找，也就少一类"找错段"的失败。
    format!(
        "{verb}下面这段。先 `skill zhibi-rewrite` 加载规程，然后按规程用 offer_rewrite 提交。\n\n原文：\n{selected}"
    )
}

pub const SELECTION_BUTTONS: &[(&str, &str)] = &[
    ("拉深", "deepen"),
    ("扩写", "expand"),
    ("缩写", "condense"),
    ("改写", "rewrite"),
    ("去AI味", "deslop"),
];

/// 内置专属 Agent 的清单（id, 中文名, 一句话职责）。
///
/// 与 `dsh::zhibi_presets()` 的 id 一一对应。放这里是因为：
/// 触发语要按 id 生成，而 prompts 不该依赖 dsh 模块的内部结构。
pub const AGENT_PRESETS: &[(&str, &str, &str)] = &[
    ("character", "人物", "人物档案、补全、合并建议、弧线"),
    ("relations", "关系", "关系梳理、章节区间、读者可见性"),
    ("worldline", "世界线", "剧情时间轴、if 分叉、因果推演"),
    ("oracle", "预言", "按大纲预测后续、影响面分析"),
    ("outline", "大纲伏笔", "大纲对账、伏笔盘点、设定归类"),
    ("reviewer", "校审", "连续性审查、客观错误、去 AI 味"),
];

/// 作者直接对着某个专属 Agent 说话时的触发语。
///
/// 为什么走文本前缀而不是另开一个会话：DSH 的 sdk 协议没有"指定 agentPreset 发一条消息"
/// 的方法——子 agent 只能由主 agent 通过 subagent 工具派。所以这里让主 Agent
/// 无条件把这条任务转给指定的子 Agent，作者体验上等于"直接对它说"。
pub fn direct_agent_task(agent: &str, text: &str) -> String {
    let label = AGENT_PRESETS
        .iter()
        .find(|(id, _, _)| *id == agent)
        .map(|(_, label, _)| *label)
        .unwrap_or(agent);
    format!(
        "把下面这件事交给「{label}」子 Agent 做（用 subagent，agentPreset={agent}）。\
         你自己不要直接动手，也不要改派给别人。等它返回后，用一段话转述它的结论。\n\n{text}"
    )
}

/// 预言书推演任务书（M7）：全程走 novel-pack 的世界线沙盒工具链，绝不碰正典。
pub fn oracle_task(premise: &str, fork: u32) -> String {
    format!(
        "预言书推演：假设「{premise}」（fork 自第{fork}章）。\
先 `skill zhibi-oracle` 加载推演规程并照做。铁律：推演只写沙盒，绝不改正典。{BASIS_RULE}"
    )
}

/// 世界线排期核对任务书（M7-3，原「导演台」）：章×幕排期板触发骨架合同核对。
pub fn director_task(chapter: u32) -> String {
    format!("排期板核对第{chapter}章。先 `skill zhibi-director` 加载核对规程并照做。{BASIS_RULE}")
}

/// 预测续写任务书（M8）：固化线之后向前织——有大纲按大纲预测，无大纲按成稿预测。
pub fn forecast_task() -> String {
    format!("预测线推演：为本书开一条 forecast 线向后续写规划。先 `skill zhibi-forecast` 加载规程并照做。{BASIS_RULE}")
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 技能名必须与 novel-pack/skills 下真实存在的文件对得上——
    /// 对不上不会报错，只会让模型加载失败后乱找路，属于最难查的那类问题。
    #[test]
    fn every_capability_points_at_an_existing_skill_file() {
        let skills_dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .unwrap()
            .join("novel-pack")
            .join("skills");
        let mut checked = 0;
        for cap in [
            "skeleton", "draft", "validate", "deslop", "memory", "inventory", "classify",
            "outsync", "audit", "dossier", "continue", "branches",
        ] {
            let Some(skill) = skill_of(cap) else {
                panic!("能力 {cap} 没有对应技能");
            };
            let path = skills_dir.join(format!("{skill}.md"));
            assert!(path.exists(), "{} 不存在", path.display());
            // 触发语里必须真的提到这个技能名，否则模型不知道该 load 什么
            let task = task_for(cap, 3);
            assert!(task.contains(skill), "{cap} 的触发语没提技能名：{task}");
            checked += 1;
        }
        assert_eq!(checked, 12);
    }

    #[test]
    fn proposal_capabilities_carry_the_basis_rule() {
        for cap in ["skeleton", "draft", "memory", "inventory", "audit"] {
            assert!(task_for(cap, 1).contains("依据来源"), "{cap} 缺依据要求");
        }
    }

    #[test]
    fn pure_output_capabilities_skip_the_basis_rule() {
        // 续写/走向不产提案，不该带上"提案须给出依据"这种噪音
        for cap in ["continue", "branches"] {
            assert!(!task_for(cap, 1).contains("依据来源"), "{cap} 不该有依据要求");
        }
    }

    #[test]
    fn selection_instruction_embeds_the_original_text_verbatim() {
        let original = "他推开门，看见灯还亮着。";
        let ins = selection_instruction("rewrite", original);
        assert!(ins.contains(original), "原文必须原样给到模型");
        assert!(ins.contains("zhibi-rewrite"));
    }

    #[test]
    fn direct_agent_task_names_the_preset_and_forbids_improvising() {
        let t = direct_agent_task("relations", "理一下第 12 章谁跟谁什么关系");
        assert!(t.contains("agentPreset=relations"), "必须点明派给哪个 preset");
        assert!(t.contains("关系"), "要用中文名，模型才认得出");
        assert!(t.contains("不要直接动手"), "要明确禁止主 Agent 自己干");
        assert!(t.contains("理一下第 12 章"), "作者原话要原样带上");
    }

    #[test]
    fn every_agent_preset_has_a_label() {
        // 界面上的选择器直接读这张表；缺 label 会显示空按钮
        for (id, label, duty) in AGENT_PRESETS {
            assert!(!id.is_empty());
            assert!(!label.is_empty(), "{id} 缺中文名");
            assert!(!duty.is_empty(), "{id} 缺职责说明");
        }
        // 与 dsh 的 preset 清单必须对得上，否则派活会被拒绝
        let dsh_ids: Vec<&str> = crate::dsh::zhibi_presets().iter().map(|p| p.id).collect();
        for (id, _, _) in AGENT_PRESETS {
            assert!(dsh_ids.contains(id), "{id} 不在 dsh 的 preset 清单里");
        }
    }

    #[test]
    fn unknown_capability_degrades_gracefully() {
        let t = task_for("no-such-capability", 1);
        assert!(!t.contains("skill "), "未知能力不该瞎指技能");
        assert!(!t.is_empty());
    }
}
