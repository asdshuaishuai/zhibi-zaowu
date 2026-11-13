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

/// 作者作用于「指定对象」时，对象的一句话描述。
///
/// 写成"人物：沈砚（ch-3f2a…）"而不是只给 id：子 Agent 拿到裸 id 还得自己查
/// 一遍才知道在说谁，多一次工具调用、也多一次查错的机会。
pub fn describe_character(c: &crate::characters::Character) -> String {
    let mut s = format!("人物：{}", c.name);
    if !c.roles_line().is_empty() {
        s.push_str(&format!("（{}）", c.roles_line()));
    }
    s.push_str(&format!("，id={}", c.id));
    s
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

/// 设定提取任务书：读 imports/ 里的作者导入材料，提取分类后分派各专属 Agent。
///
/// 这是"老书搬进执笔"的主链路：作者把散落的世界观/人物/大纲文档导进来，
/// 总 Agent 负责提取和**分派**——落库仍是各专属 Agent 出提案、收件箱裁决，
/// 总 Agent 自己不写任何台账。
pub fn import_extract_task(doc_count: usize) -> String {
    format!(
        "设定提取：书目录 imports/ 下有 {doc_count} 篇作者导入的原始材料（INDEX.md 有清单）。\n\
         通读后按内容分类提取，**逐类分派给对应专属 Agent**（subagent，agentPreset 参数）：\n\
         - 人物档案（外貌/性格/动机/口吻）→ agentPreset=character\n\
         - 人物关系（谁与谁、从第几章）→ agentPreset=relations\n\
         - 世界设定/伏笔/大纲故事线 → agentPreset=outline\n\
         一篇材料可能同时出多类产出，就分派多次。铁律：\n\
         1. 所有产出一律走提案工具，由作者在收件箱裁决；你不写任何台账或正文。\n\
         2. 材料里没有的不要编——提取只能基于原文，拿不准就跳过并在汇总里列出。\n\
         3. 材料互相矛盾的（同一人两个背景）不要取舍，两条都出提案并注明冲突。\n\
         全部分派完成后，用一段话汇总：每个文档产出了哪些类、各多少条提案。{BASIS_RULE}"
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
    fn unknown_capability_degrades_gracefully() {
        let t = task_for("no-such-capability", 1);
        assert!(!t.contains("skill "), "未知能力不该瞎指技能");
        assert!(!t.is_empty());
    }
}
