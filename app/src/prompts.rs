//! 执笔提示词库（Rust 侧）：每个 AI 能力的任务指令。
//! 与旧版 PromptLibrary 同源语义：能力 → 任务书；创作法典由 novel-pack 侧注入（M3 后续）。

pub fn task_for(capability: &str, chapter: u32) -> String {
    match capability {
        "skeleton" => format!(
            "为第{chapter}章搭骨架：先读上一章结尾、伏笔台账与大纲，再按期待感管理给出 4-6 个节拍（每节拍含建议字数、场景层视角/地点/出场、章尾钩子形态、到期伏笔进合同），用 propose_skeleton 提案。"
        ),
        "draft" => format!("为第{chapter}章写草稿：按已批准骨架逐拍成文，正文以 propose_draft 提案，采纳前不落盘。"),
        "validate" => format!(
            "验证第{chapter}章：先跑确定性体检，再查客观错误（连续性矛盾/设定违背/伏笔合同未兑现/物理不可能/动机断裂），只报告不代改，用 propose_validation 提案。"
        ),
        "deslop" => format!(
            "第{chapter}章去AI味：先本地 25 项扫描，再三 pass（叙事架构→篇章→措辞）逐处给 original/replacement/gate/reason，用 propose_deslop 提案。"
        ),
        "memory" => format!(
            "第{chapter}章写完，记一笔：提取事实三元组（主语/谓语/宾语/生效章/读者可见）与伏笔候选，用 propose_memory 提案。"
        ),
        "inventory" => "全书伏笔盘点：对照正文找出已埋未登记、登记未兑现、烂账（种下原文对不上/兑现逾期/动作日志指向不存在的章），每条配可一键采纳的修复动作，用 propose_clues 提案。".into(),
        "classify" => "设定归类：对每篇设定文档给出分类（世界观/势力人物/修炼体系/剧情大纲/题材规则/未分类）与可选规范化改名，用 propose_canon_classify 提案。".into(),
        "outsync" => "大纲同步：对账「计划 vs 实际」，为断线/待收束的剧情线与偏移事件给出更新建议，用 propose_outline_updates 提案。".into(),
        "audit" => "全书连贯性审查：死者复出/位置跳跃/人称漂移/时间线倒挂/台账烂账，每条给修复动作，用 propose_clue_fixes 提案。".into(),
        "dossier" => format!("提取第{chapter}章档案卡：场景/出场/推进/拐点/悬念/关键细节（检索锚点），用 propose_dossier 提案。"),
        "continue" => "只输出下一句续写（不超过 60 字），紧贴上文语气与在场人物。不要调用工具，不要解释，只给那一句。".into(),
        "branches" => "给 3 个故事走向，每个 80-150 字，分别以【A】【B】【C】开头标记，各自标注情绪标签。不要调用工具，不要写正文之外的说明。".into(),
        _ => "读项目现状，给出下一步建议。".into(),
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
    AiButton { label: "去AI味", capability: "deslop" },    AiButton { label: "让AI记一笔", capability: "memory" },
    AiButton { label: "AI 盘点伏笔", capability: "inventory" },
    AiButton { label: "AI 归类设定", capability: "classify" },
];

pub fn selection_instruction(kind: &str, selected: &str) -> String {
    let verb = match kind {
        "deepen" => "拉深（同一段写得更细更满，保持事实与视角）",
        "expand" => "扩写（展开更多细节与回合）",
        "condense" => "缩写（收紧节奏删冗余，保留关键动作）",
        "rewrite" => "改写（换一种写法，保持说什么不变）",
        "deslop" => "去AI味改写（按 25 项检测逐处修正措辞与结构）",
        _ => "改写",
    };
    format!(
        "{verb}。只输出改写后的文本本身，不要解释、不要引号、不要调用工具。\n\n原文：\n{selected}"
    )
}

pub const SELECTION_BUTTONS: &[(&str, &str)] = &[
    ("拉深", "deepen"),
    ("扩写", "expand"),
    ("缩写", "condense"),
    ("改写", "rewrite"),
    ("去AI味", "deslop"),
];

/// 预言书推演任务书（M7）：全程走 novel-pack 的世界线沙盒工具链，绝不碰正典。
pub fn oracle_task(premise: &str, fork: u32) -> String {
    format!(
        "预言书推演：假设「{premise}」（fork 自第{fork}章）。\
流程：1) get_world_state 读正典图；2) propose_worldline 开一条新线（自动取 β/γ/…）；\
3) simulate_worldline 写入推演节点/边/影响清单——沿因果链至少走 3 跳，每条 diff 必须有 \
before/after/reason，衍生实体自动标 line。\
铁律：推演只写 worldlines.json 沙盒，绝不改正典；正典变更只能以 diffs 形态呈现，\
由作者逐条晋升为提案。最后用一段话总结影响面（波及几条线、几个设定、最大风险）。"
    )
}

/// 导演台排期核对任务书（M7-3）：章×幕排期板触发骨架合同核对。
pub fn director_task(chapter: u32) -> String {
    format!(
        "导演台核对第{chapter}章排期：对照骨架合同（到期伏笔应收/种/兑）与故事线健康度，\
给出本章的事件/伏笔挂接建议与冲突预警；需要落账的部分用 propose_clues / propose_outline_updates 提案，只报告不代改。"
    )
}
