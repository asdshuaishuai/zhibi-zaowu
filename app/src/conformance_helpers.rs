//! 供一致性夹具使用的薄适配层。
//!
//! 为什么不直接调现有类型：`crate::journal::WritingMode` 是"档位"，而夹具测的是
//! **"档位 + 该章是否授权"这一个组合判断**——它正好是 novel-pack 里
//! `canWriteLedger` / `canWriteProse` 的形状。把这两条判断单独抽出来，
//! 两端的测试才在比同一件事，而不是比两个形状不同的 API。

/// 写作档位 + 逐章授权。
pub struct WritePolicy<'a> {
    pub mode: &'a str,
    /// 作者显式授权代笔的章节。授权必须显式——空表就是一章都没授权。
    pub ghost: Vec<u32>,
}

impl WritePolicy<'_> {
    /// 能否直写台账。辅助档不行，共写/代笔可以。
    pub fn can_write_ledger(&self) -> bool {
        matches!(self.mode, "cowrite" | "ghostwrite")
    }

    /// 能否直写某章正文。只有代笔档 + 该章被显式授权。
    ///
    /// 注意档位优先：辅助档即使 `ghost` 里有这一章也不行——
    /// 否则作者把档位调回辅助，之前授权的章节会静默变成可写。
    pub fn can_write_prose(&self, chapter: u32) -> bool {
        self.mode == "ghostwrite" && self.ghost.contains(&chapter)
    }
}
