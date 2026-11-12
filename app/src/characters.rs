//! 人物实体：从“别名 + 事实三元组”升级为有档案的人。
//!
//! 为什么要独立文件而不是继续用 memory.json：`aliases` 只记“这个人叫什么”，
//! `facts` 只记“谁做了什么”。作者需要的是外貌、口吻、动机、弧线这些写小说时
//! 真正要查的东西——它们既不是别名也不是事实，塞进 memory.json 会把那个文件变成杂物间。
//!
//! 兼容策略：
//! - `memory.json` 的 aliases/facts **完全不动**，画布与旧工具继续用它们。
//! - 第一次打开书时从 aliases + facts 做一次无损迁移到 `characters.json`，
//!   迁移只“补空”，不覆盖作者已经写下的字段。
//! - 出场章节由 dossier 推导，不落盘——它是派生事实，存下来就会和 dossier 冲突。

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

/// 人物弧线上的一个点：第几章、变成什么样。
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", default)]
pub struct ArcPoint {
    pub chapter: u32,
    /// 这一章之后他/她的状态或立场变化，一句话。
    pub state: String,
    #[serde(default)]
    pub note: String,
}

/// 一个人物。
///
/// 字段刻意分成“作者会认真写”（外貌/口吻/动机/弧线）和“AI 容易瞎编”两类：
/// 后者带 `locked` 标记，AI 只在未锁定时才建议修改——作者写下的东西不该被提案覆盖。
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", default)]
pub struct Character {
    /// 稳定 id，改名不变。
    pub id: String,
    /// 本名（文档内统一指称）。
    pub name: String,
    #[serde(default)]
    pub aliases: Vec<String>,
    /// 角色定位：主角/反派/导师/配角…（作者自定，不枚举死）
    #[serde(default)]
    pub role: String,
    /// 阵营/所属。
    #[serde(default)]
    pub faction: String,
    #[serde(default)]
    pub age: String,
    #[serde(default)]
    pub appearance: String,
    #[serde(default)]
    pub personality: String,
    #[serde(default)]
    pub motivation: String,
    #[serde(default)]
    pub background: String,
    /// 目标（想得到什么）。
    #[serde(default)]
    pub goals: Vec<String>,
    /// 缺陷（挡住他/她的东西）。
    #[serde(default)]
    pub flaws: Vec<String>,
    /// 口吻：说话方式、用词、口头禅——写对白时最有用的一条。
    #[serde(default)]
    pub voice: String,
    /// 当前状态（活着/受伤/失忆…）与它成立的章节。
    #[serde(default)]
    pub status: String,
    #[serde(default)]
    pub status_chapter: u32,
    /// 弧线。
    #[serde(default)]
    pub arc: Vec<ArcPoint>,
    #[serde(default)]
    pub tags: Vec<String>,
    /// 作者锁定、不允许 AI 改的字段名（如 ["voice", "motivation"]）。
    #[serde(default)]
    pub locked_fields: Vec<String>,
    #[serde(default)]
    pub notes: String,
}

impl Character {
    /// 新建时的最简档案：只保证 id 与名字。
    pub fn new(name: &str) -> Self {
        // 纳秒 + 递增计数：同一毫秒内连建两个人物（迁移时很常见）不能撞 id
        static SEQ: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0);
        let seq = SEQ.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        Self {
            id: format!("ch-{nanos:x}{seq:x}"),
            name: name.to_string(),
            ..Default::default()
        }
    }

    pub fn is_locked(&self, field: &str) -> bool {
        self.locked_fields.iter().any(|f| f == field)
    }

    /// 把另一个人合并进来：别名并集、空格子补上、弧线按章排序。
    ///
    /// 冲突策略：`self` 已有非空值时保留（作者手写优先），只补空。
    /// 这是“合并”而不是“覆盖”——合并人物是纠错，不该顺手改掉已写好的档案。
    pub fn absorb(&mut self, other: &Character) {
        for a in &other.aliases {
            let a = a.trim();
            if !a.is_empty() && a != self.name && !self.aliases.iter().any(|x| x == a) {
                self.aliases.push(a.to_string());
            }
        }
        // 被合并者的本名也变成别名（否则以后提到那个名字就找不到人）
        if other.name != self.name && !self.aliases.iter().any(|x| *x == other.name) {
            self.aliases.push(other.name.clone());
        }
        macro_rules! fill {
            ($($f:ident),*) => { $( if self.$f.trim().is_empty() && !other.$f.trim().is_empty() { self.$f = other.$f.clone(); } )* };
        }
        fill!(role, faction, age, appearance, personality, motivation, background, voice, status, notes);
        for g in &other.goals {
            if !self.goals.iter().any(|x| x == g) {
                self.goals.push(g.clone());
            }
        }
        for f in &other.flaws {
            if !self.flaws.iter().any(|x| x == f) {
                self.flaws.push(f.clone());
            }
        }
        for t in &other.tags {
            if !self.tags.iter().any(|x| x == t) {
                self.tags.push(t.clone());
            }
        }
        for p in &other.arc {
            if !self.arc.iter().any(|x| x.chapter == p.chapter && x.state == p.state) {
                self.arc.push(p.clone());
            }
        }
        self.arc.sort_by_key(|p| p.chapter);
        if self.status_chapter == 0 {
            self.status_chapter = other.status_chapter;
        }
    }

    /// 是否匹配某个指称（本名或任一别名）。关系/事实里出现的人名用它归一。
    pub fn matches(&self, name: &str) -> bool {
        let n = name.trim();
        !n.is_empty() && (self.name == n || self.aliases.iter().any(|a| a == n))
    }
}

/// `characters.json` 的读写。
pub struct Roster {
    root: PathBuf,
}

impl Roster {
    pub fn new(book_root: &Path) -> Self {
        Self {
            root: book_root.to_path_buf(),
        }
    }

    fn path(&self) -> PathBuf {
        self.root.join("characters.json")
    }

    /// 读全部人物。文件不存在 → 空；坏文件 → 空（宁可少显示，不让书打不开）。
    pub fn all(&self) -> Vec<Character> {
        std::fs::read_to_string(self.path())
            .ok()
            .and_then(|t| serde_json::from_str::<Vec<Character>>(&t).ok())
            .unwrap_or_default()
    }

    /// 写回。走原子写，并且**先校验再写**：坏数据不该覆盖好档案。
    pub fn save(&self, list: &[Character]) -> Result<(), String> {
        // 序列化一次就能发现无法表达的结构（理论上不该发生，但便宜）
        let json = serde_json::to_string_pretty(list).map_err(|e| e.to_string())?;
        let tmp = self.path().with_extension("tmp");
        std::fs::write(&tmp, json).map_err(|e| e.to_string())?;
        std::fs::rename(&tmp, self.path()).map_err(|e| e.to_string())
    }

    /// 按本名或别名找人。
    pub fn find(&self, name: &str) -> Option<Character> {
        self.all().into_iter().find(|c| c.matches(name))
    }

    pub fn upsert(&self, c: Character) -> Result<(), String> {
        let mut list = self.all();
        match list.iter().position(|x| x.id == c.id) {
            Some(i) => list[i] = c,
            None => list.push(c),
        }
        self.save(&list)
    }

    pub fn remove(&self, id: &str) -> Result<(), String> {
        let mut list = self.all();
        list.retain(|c| c.id != id);
        self.save(&list)
    }

    /// 合并两人：`into_id` 吸收 `from_id`，然后删掉后者。
    pub fn merge(&self, into_id: &str, from_id: &str) -> Result<(), String> {
        if into_id == from_id {
            return Err("不能合并到自己".into());
        }
        let mut list = self.all();
        let from = list
            .iter()
            .find(|c| c.id == from_id)
            .cloned()
            .ok_or_else(|| "要合并的人物不存在".to_string())?;
        let pos = list
            .iter()
            .position(|c| c.id == into_id)
            .ok_or_else(|| "目标人物不存在".to_string())?;
        list[pos].absorb(&from);
        list.retain(|c| c.id != from_id);
        self.save(&list)
    }

    /// 从 memory.json 的 aliases + facts 迁移一次。
    ///
    /// 只补空，不覆盖：作者已经在 characters.json 里写下的字段一律不动。
    /// 返回 (新增人数, 补全人数)，供界面告诉作者迁移了什么。
    pub fn migrate_from_memory(&self, facts: &[serde_json::Value], aliases: &[serde_json::Value]) -> (usize, usize) {
        let mut list = self.all();
        let mut added = 0usize;

        let ensure = |list: &mut Vec<Character>, name: &str, added: &mut usize| {
            let name = name.trim();
            if name.is_empty() {
                return;
            }
            if list.iter().any(|c| c.matches(name)) {
                // 已存在：此时不做任何事，字段补全由下面的 aliases 段落统一处理
                return;
            }
            list.push(Character::new(name));
            *added += 1;
        };

        // 1) aliases 档案：本名 + 别名
        for a in aliases {
            let Some(canonical) = a.get("canonicalName").and_then(|v| v.as_str()) else {
                continue;
            };
            if !list.iter().any(|c| c.matches(canonical)) {
                let mut c = Character::new(canonical);
                c.aliases = a
                    .get("aliases")
                    .and_then(|v| v.as_array())
                    .map(|arr| {
                        arr.iter()
                            .filter_map(|x| x.as_str())
                            .filter(|x| *x != canonical)
                            .map(String::from)
                            .collect()
                    })
                    .unwrap_or_default();
                list.push(c);
                added += 1;
            }
        }

        // 2) facts 里出现的人名。
        //
        // subject 几乎总是人（“谁做了什么”），可以直接建；
        // object 可能是物或事（“获得·断刀”“前往·长安”），只认两种：
        //   - 已经在 aliases 档案里的人；
        //   - 该谓词明显指向人（师从/敌对/父子…），且名字 2-4 字。
        // 只按字数判断会把“断刀”当成人，这是迁移最容易出的错。
        let known: Vec<String> = list.iter().map(|c| c.name.clone()).collect();
        const PERSON_PREDICATES: &[&str] = &[
            "师从", "师父", "徒弟", "父子", "父女", "母子", "母女", "兄弟", "姐妹", "夫妻",
            "敌对", "仇敌", "同乡", "共谋", "暗恋", "主仆", "盟友", "部下", "上司", "相识",
            "结义", "救过", "杀过", "背叛", "信任", "利用", "喜欢", "恨",
        ];
        for f in facts {
            let predicate = f.get("predicate").and_then(|v| v.as_str()).unwrap_or("");
            if let Some(name) = f.get("subject").and_then(|v| v.as_str()) {
                ensure(&mut list, name, &mut added);
            }
            if let Some(name) = f.get("object").and_then(|v| v.as_str()) {
                let is_known_person = known.iter().any(|k| k == name)
                    || list.iter().any(|c| c.matches(name));
                let relational =
                    PERSON_PREDICATES.contains(&predicate) && (2..=4).contains(&name.chars().count());
                if is_known_person || relational {
                    ensure(&mut list, name, &mut added);
                }
            }
        }

        if added > 0 {
            if let Err(e) = self.save(&list) {
                eprintln!("迁移人物档案失败：{e}");
                return (0, 0);
            }
        }
        (added, 0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn book(tag: &str) -> PathBuf {
        let p = std::env::temp_dir().join(format!("zhibi-roster-{tag}"));
        let _ = std::fs::remove_dir_all(&p);
        std::fs::create_dir_all(&p).unwrap();
        p
    }

    #[test]
    fn migration_creates_people_from_aliases_and_facts() {
        let root = book("migrate");
        let r = Roster::new(&root);
        let aliases = vec![serde_json::json!({
            "canonicalName": "沈砚",
            "aliases": ["砚哥", "沈兄"]
        })];
        let facts = vec![
            serde_json::json!({"subject": "沈砚", "predicate": "获得", "object": "断刀"}),
            serde_json::json!({"subject": "老周", "predicate": "师从", "object": "沈砚"}),
        ];
        let (added, _) = r.migrate_from_memory(&facts, &aliases);
        // 沈砚（来自 aliases）+ 老周（来自 facts）
        assert_eq!(added, 2, "应从 aliases 与 facts 各建一份档案");
        let all = r.all();
        let shen = all.iter().find(|c| c.name == "沈砚").unwrap();
        assert_eq!(shen.aliases, vec!["砚哥", "沈兄"]);
        assert!(all.iter().any(|c| c.name == "老周"));
    }

    #[test]
    fn migration_never_overwrites_author_fields() {
        let root = book("migrate-keep");
        let r = Roster::new(&root);
        let mut mine = Character::new("沈砚");
        mine.voice = "话少，句短，常以问句收尾".into();
        mine.locked_fields = vec!["voice".into()];
        r.save(&[mine]).unwrap();

        let aliases = vec![serde_json::json!({"canonicalName": "沈砚", "aliases": ["砚哥"]})];
        let (added, _) = r.migrate_from_memory(&[], &aliases);
        assert_eq!(added, 0, "已存在的人不该被重复创建");
        let shen = r.all().into_iter().find(|c| c.name == "沈砚").unwrap();
        assert_eq!(shen.voice, "话少，句短，常以问句收尾", "作者写的口吻不能被迁移覆盖");
    }

    #[test]
    fn migration_is_idempotent() {
        let root = book("idempotent");
        let r = Roster::new(&root);
        let aliases = vec![serde_json::json!({"canonicalName": "沈砚", "aliases": ["砚哥"]})];
        r.migrate_from_memory(&[], &aliases);
        let (second, _) = r.migrate_from_memory(&[], &aliases);
        assert_eq!(second, 0, "重复迁移不该再新增");
        assert_eq!(r.all().len(), 1);
    }

    #[test]
    fn merge_keeps_target_fields_and_absorbs_alias() {
        let root = book("merge");
        let r = Roster::new(&root);
        let mut a = Character::new("沈砚");
        a.voice = "话少".into();
        let mut b = Character::new("沈砚之");
        b.aliases = vec!["砚哥".into()];
        b.motivation = "找出杀父之人".into();
        let (aid, bid) = (a.id.clone(), b.id.clone());
        r.save(&[a, b]).unwrap();

        r.merge(&aid, &bid).unwrap();
        let all = r.all();
        assert_eq!(all.len(), 1, "被合并者应删除");
        let shen = &all[0];
        assert_eq!(shen.voice, "话少", "目标已有值保留");
        assert_eq!(shen.motivation, "找出杀父之人", "目标空缺被补齐");
        assert!(shen.aliases.contains(&"砚哥".to_string()));
        assert!(shen.aliases.contains(&"沈砚之".to_string()), "被合并者本名应成为别名");
    }

    #[test]
    fn find_matches_alias() {
        let root = book("find");
        let r = Roster::new(&root);
        let mut c = Character::new("沈砚");
        c.aliases = vec!["砚哥".into()];
        r.save(&[c]).unwrap();
        assert!(r.find("砚哥").is_some(), "别名也要能找人");
        assert!(r.find("不存在").is_none());
    }

    #[test]
    fn migration_does_not_turn_objects_into_people() {
        let root = book("not-people");
        let r = Roster::new(&root);
        let facts = vec![
            // 物：谓词是“获得”，宾语“断刀”不是人
            serde_json::json!({"subject":"沈砚","predicate":"获得","object":"断刀"}),
            // 地点：谓词是“前往”
            serde_json::json!({"subject":"沈砚","predicate":"前往","object":"长安"}),
            // 关系谓词：宾语是人
            serde_json::json!({"subject":"老周","predicate":"师从","object":"沈砚"}),
        ];
        let (added, _) = r.migrate_from_memory(&facts, &[]);
        let names: Vec<String> = r.all().into_iter().map(|c| c.name).collect();
        assert!(names.contains(&"沈砚".to_string()));
        assert!(names.contains(&"老周".to_string()));
        assert!(!names.contains(&"断刀".to_string()), "刀不是人");
        assert!(!names.contains(&"长安".to_string()), "地名不是人");
        assert_eq!(added, 2, "只应建两个真实人物");
    }

    #[test]
    fn corrupt_file_reads_as_empty_but_is_not_clobbered_by_reads() {
        let root = book("corrupt");
        std::fs::write(root.join("characters.json"), "{ 坏掉").unwrap();
        let r = Roster::new(&root);
        assert!(r.all().is_empty(), "坏文件当空读");
        // 读不该改文件：坏数据仍留着，等作者或修复流程处理
        assert!(std::fs::read_to_string(root.join("characters.json")).unwrap().contains("坏掉"));
    }
}
