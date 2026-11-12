//! 专属 Agent 的能力表：谁能干什么、针对谁干。
//!
//! ## 这不是"对话对象"
//!
//! 六个专属 Agent（人物 / 关系 / 世界线 / 预言 / 大纲伏笔 / 校审）不是聊天窗口，
//! 而是**被调用的能力**。作者从不"和关系 Agent 聊天"，他是：
//!
//! 1. **在模块页点一键能力** —— 人物页「全量补全」、世界线页「推演后续」。
//! 2. **选中一个对象后点操作** —— 选中沈砚 →「补全这张卡」；选中第 30 章那个节点
//!    →「从这儿往后推」。
//! 3. **直接对主 Agent 说人话** —— 「把第 12 章的人物关系捋一下」。主 Agent 自己
//!    判断该派给谁、要不要拆成几步。
//!
//! 三条路最终都汇到同一个执行者。区别只在**路由由谁决定**：
//! 前两条页面写死了目标 Agent（作者已经用点击表达了意图），第三条由主 Agent 推断。
//!
//! ## 为什么页面要写死路由
//!
//! 作者点了「补全沈砚」，意图已经明确。如果这时还让主 Agent 自己挑 Agent，
//! 它可能挑错、可能自己干了、也可能反过来问作者"要派给谁"——三次都算失败。
//! 所以页面按钮发出的指令里**必须点名 preset**，主 Agent 只负责转交和汇总。

/// 一个专属 Agent 的完整能力描述。
pub struct AgentSpec {
    /// 与 `dsh::zhibi_presets()` 的 id 一致
    pub id: &'static str,
    /// 中文名（按钮与人话里用）
    pub label: &'static str,
    /// 一句话职责，用来给主 Agent 做路由说明
    pub duty: &'static str,
    /// **模块级一键能力**：不需要选中任何对象，处理整个模块。
    pub bulk: &'static [BulkCapability],
    /// **针对指定对象的操作**：作者选中某个人物/线/节点后可以做的。
    pub targeted: &'static [TargetedOp],
}

/// 模块级一键能力。
pub struct BulkCapability {
    /// 稳定键（事件里传递）
    pub key: &'static str,
    /// 按钮文字
    pub label: &'static str,
    /// 一句话说明它做什么（按钮旁边的灰字）
    pub hint: &'static str,
}

/// 针对指定对象的操作。
pub struct TargetedOp {
    pub key: &'static str,
    pub label: &'static str,
    /// 这个操作需要什么样的对象（用于页面判断该不该显示）
    pub target_kind: TargetKind,
}

/// 操作对象是什么。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TargetKind {
    /// 某个人物（人物页选中）
    Character,
    /// 某一章（编辑器当前章）
    Chapter,
    /// 某条世界线
    Line,
    /// 世界线上的某个节点
    LineNode,
    /// 某个台账条目（伏笔/事件/故事线）
    Ledger,
}

/// 六个专属 Agent 的能力表。
///
/// 加能力的顺序就是按钮显示的顺序：**作者最常做的前面**。
pub const AGENTS: &[AgentSpec] = &[
    AgentSpec {
        id: "character",
        label: "人物",
        duty: "人物档案：补全、合并建议、弧线、口吻",
        bulk: &[
            BulkCapability {
                key: "char-bulk-fill",
                label: "全量补全",
                hint: "扫正文与档案，把每个人缺的字段补上（提案）",
            },
            BulkCapability {
                key: "char-bulk-audit",
                label: "核对一遍",
                hint: "找出同名不同人、同人不同名、档案与正文矛盾",
            },
        ],
        targeted: &[
            TargetedOp {
                key: "char-fill",
                label: "补全这张卡",
                target_kind: TargetKind::Character,
            },
            TargetedOp {
                key: "char-arc",
                label: "梳理弧线",
                target_kind: TargetKind::Character,
            },
        ],
    },
    AgentSpec {
        id: "relations",
        label: "关系",
        duty: "人物关系：关系类型、章节区间、读者可见性",
        bulk: &[
            BulkCapability {
                key: "rel-bulk-scan",
                label: "全量梳理",
                hint: "从正文重建关系网，标出每段关系从第几章到第几章",
            },
        ],
        targeted: &[
            TargetedOp {
                key: "rel-chapter",
                label: "捋这一章的关系",
                target_kind: TargetKind::Chapter,
            },
            TargetedOp {
                key: "rel-char",
                label: "这个人跟谁有关",
                target_kind: TargetKind::Character,
            },
        ],
    },
    AgentSpec {
        id: "worldline",
        label: "世界线",
        duty: "剧情时间轴：if 分叉、因果推演、节点落章",
        bulk: &[
            BulkCapability {
                key: "wl-bulk-tidy",
                label: "整理节点",
                hint: "核对每条线的节点落在哪一章，标出没排章的",
            },
        ],
        targeted: &[
            TargetedOp {
                key: "wl-from-node",
                label: "从这儿往后推",
                target_kind: TargetKind::LineNode,
            },
            TargetedOp {
                key: "wl-extend",
                label: "把这条线推下去",
                target_kind: TargetKind::Line,
            },
        ],
    },
    AgentSpec {
        id: "oracle",
        label: "预言",
        duty: "按大纲预测后续、影响面分析、呼应到期伏笔",
        bulk: &[BulkCapability {
            key: "oracle-forecast",
            label: "预测后续",
            hint: "按现有势头推 2-3 个走向，说明各自呼应哪些到期伏笔",
        }],
        targeted: &[TargetedOp {
            key: "oracle-line",
            label: "看这条线的影响面",
            target_kind: TargetKind::Line,
        }],
    },
    AgentSpec {
        id: "outline",
        label: "大纲伏笔",
        duty: "大纲对账、伏笔盘点、设定归类",
        bulk: &[
            BulkCapability {
                key: "outline-bulk-sync",
                label: "大纲对账",
                hint: "计划 vs 实际：断线、偏移事件、该收没收的线",
            },
            BulkCapability {
                key: "outline-bulk-clues",
                label: "伏笔盘点",
                hint: "已埋未登记、登记未兑现、烂账",
            },
            BulkCapability {
                key: "outline-bulk-classify",
                label: "设定归类",
                hint: "给每篇设定一个分类，可选改名",
            },
        ],
        targeted: &[TargetedOp {
            key: "outline-item",
            label: "核对这一条",
            target_kind: TargetKind::Ledger,
        }],
    },
    AgentSpec {
        id: "reviewer",
        label: "校审",
        duty: "只查客观错误：连续性、设定违背、伏笔合同、物理不可能",
        bulk: &[BulkCapability {
            key: "review-bulk-audit",
            label: "全书连贯性审查",
            hint: "死者复出、位置跳跃、时间线倒挂、台账烂账",
        }],
        targeted: &[TargetedOp {
            key: "review-chapter",
            label: "验收这一章",
            target_kind: TargetKind::Chapter,
        }],
    },
];

/// 按 id 找 Agent 规格。
pub fn agent(id: &str) -> Option<&'static AgentSpec> {
    AGENTS.iter().find(|a| a.id == id)
}

/// 装配给主 Agent 的路由说明。
///
/// 主 Agent 收到「把第 12 章的人物关系捋一下」时要自己判断派给谁。
/// 判断依据就是这张表——所以它必须进系统提示，而不是靠模型猜名字。
///
/// 写成"什么任务 → 派给谁"的形式，比列一串 Agent 名字有用得多：
/// 模型执行的是匹配任务类型，不是理解组织架构。
pub fn routing_hint() -> String {
    let mut out = String::from("## 专属 Agent\n\n写作类任务派给对应子 Agent（subagent 的 agentPreset 参数），不要自己硬做：\n");
    for a in AGENTS {
        out.push_str(&format!("- `{}`（{}）：{}\n", a.id, a.label, a.duty));
    }
    out.push_str(
        "\n路由原则：\n\
         1. 人物档案、人物口吻、人物弧线 → `character`\n\
         2. 谁跟谁什么关系、关系从第几章到第几章 → `relations`\n\
         3. 剧情时间轴、if 分叉、某个节点之后会怎样 → `worldline`\n\
         4. 接下来该怎么写、这条假设影响面多大 → `oracle`\n\
         5. 大纲对不上、伏笔该收没收、设定杂乱 → `outline`\n\
         6. 只查客观错误、不评文风 → `reviewer`\n\n\
         一次任务涉及多个方面就依次派多个子 Agent，最后汇总。\n\
         纯工具性的杂活（读文件、列目录、改一个章号）你自己做，不用派。",
    );
    out
}

/// 模块一键能力的完整指令（页面按钮发出）。
///
/// 风格与 `direct` 的区别：这里说明"处理整个模块"，不指定对象。
pub fn bulk_task(agent_id: &str, capability_key: &str) -> Option<String> {
    let a = agent(agent_id)?;
    let cap = a.bulk.iter().find(|c| c.key == capability_key)?;
    Some(format!(
        "让「{}」子 Agent 做这件事（subagent，agentPreset={}）：{}。\n\
         你自己不要动手，等它返回后用一段话转述结论与产出的提案数。",
        a.label, a.id, cap.hint
    ))
}

/// 针对指定对象的操作指令（选中对象后点按钮发出）。
///
/// `target` 是给人看的对象描述（如「人物：沈砚（ch-3f2a…）」或
/// 「世界线 β 的第 30 章节点『师兄现身』」）。把描述写进去而不是只写 id：
/// 子 Agent 拿到 id 还得自己查一遍才能知道在说谁，多一次工具调用、也多一次查错的机会。
pub fn targeted_task(agent_id: &str, op_key: &str, target: &str) -> Option<String> {
    let a = agent(agent_id)?;
    let op = a.targeted.iter().find(|o| o.key == op_key)?;
    Some(format!(
        "让「{}」子 Agent 做这件事（subagent，agentPreset={}）：对{}执行「{}」。\n\
         只处理这一个对象，不要顺带改动别的。等它返回后转述结论。",
        a.label, a.id, target, op.label
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_agent_id_matches_a_real_dsh_preset() {
        // 页面按钮发出的指令里点名 preset；名字对不上会被 DSH 拒绝派活，
        // 而且错误信息不会指出是这里写错了——必须靠测试兜住。
        let dsh_ids: Vec<&str> = crate::dsh::zhibi_presets().iter().map(|p| p.id).collect();
        assert_eq!(AGENTS.len(), 6, "内置 Agent 就是 6 个");
        for a in AGENTS {
            assert!(
                dsh_ids.contains(&a.id),
                "{} 不在 dsh preset 清单里（{:?}）",
                a.id,
                dsh_ids
            );
            assert!(!a.label.is_empty() && !a.duty.is_empty());
        }
    }

    #[test]
    fn every_agent_has_at_least_one_entry_point() {
        // 一个没有任何入口的 Agent 等于不存在：作者点不到它，主 Agent 也
        // 没有触发它的理由。这条测试防的是"加了 preset 忘了挂能力"。
        for a in AGENTS {
            assert!(
                !a.bulk.is_empty() || !a.targeted.is_empty(),
                "{} 没有任何入口（既没有一键能力也没有指定对象操作）",
                a.id
            );
        }
    }

    #[test]
    fn capability_keys_are_unique_across_agents() {
        // 事件里只传 key，重名会让页面发出的指令落到错误的 Agent 上
        let mut seen: Vec<&str> = Vec::new();
        for a in AGENTS {
            for c in a.bulk {
                assert!(!seen.contains(&c.key), "重复的 bulk key: {}", c.key);
                seen.push(c.key);
            }
            for t in a.targeted {
                assert!(!seen.contains(&t.key), "重复的 targeted key: {}", t.key);
                seen.push(t.key);
            }
        }
    }

    #[test]
    fn bulk_task_names_the_preset_and_forbids_improvising() {
        let t = bulk_task("character", "char-bulk-fill").unwrap();
        assert!(t.contains("agentPreset=character"), "必须点名 preset");
        assert!(t.contains("人物"), "要带中文名，模型才认得出");
        assert!(t.contains("不要动手"), "要明确禁止主 Agent 自己干");
        assert!(t.contains("提案"), "产出形态要写清");
    }

    #[test]
    fn targeted_task_embeds_the_object_description() {
        let t = targeted_task("character", "char-fill", "人物：沈砚（ch-3f2a）").unwrap();
        assert!(t.contains("沈砚"), "对象描述要原样带上");
        assert!(t.contains("只处理这一个对象"), "指定对象操作不能扩散到别的对象");
        assert!(t.contains("agentPreset=character"));
    }

    #[test]
    fn unknown_keys_return_none_instead_of_a_broken_prompt() {
        // 页面按钮的 key 写错时宁可不发，也不要发一条主 Agent 看不懂的指令
        assert!(bulk_task("character", "no-such-key").is_none());
        assert!(bulk_task("no-such-agent", "char-bulk-fill").is_none());
        assert!(targeted_task("character", "no-such-op", "x").is_none());
        // 一键能力不能当指定对象操作用，反之亦然
        assert!(targeted_task("character", "char-bulk-fill", "x").is_none());
        assert!(bulk_task("character", "char-fill").is_none());
    }

    #[test]
    fn routing_hint_covers_every_agent() {
        // 主 Agent 靠这段文字决定派给谁。漏一个 Agent 就等于作者说了那类事
        // 它不知道怎么处理，只能自己瞎做。
        let hint = routing_hint();
        for a in AGENTS {
            assert!(hint.contains(a.id), "路由说明漏了 {}", a.id);
            assert!(hint.contains(a.duty), "路由说明漏了 {} 的职责", a.id);
        }
        assert!(hint.contains("自己硬做") || hint.contains("不要硬做"));
    }

    #[test]
    fn every_targeted_op_declares_a_target_kind() {
        // 页面靠 target_kind 决定"选中这个对象时该不该显示这个按钮"
        for a in AGENTS {
            for t in a.targeted {
                // 枚举本身没有非法值，这里验的是"没有漏写"这个契约：
                // 只要编译过就说明每个 TargetedOp 都给了 kind。
                let _ = match t.target_kind {
                    TargetKind::Character
                    | TargetKind::Chapter
                    | TargetKind::Line
                    | TargetKind::LineNode
                    | TargetKind::Ledger => true,
                };
            }
        }
    }
}
