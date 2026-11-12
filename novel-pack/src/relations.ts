// 关系区间的判定（TS 侧）。
//
// 与 Rust `app/src/relations.rs` 的 `Interval::covers` 是同一套语义：
// 起章含、止章不含、0 表示该端开放。两边必须一致——否则同一个
// relations.json 在客户端和插件里会算出不同的"此刻关系"，
// 而这种分歧不会报错，只会让画布上少一条边。
//
// 为什么要单独一个文件而不是塞进 store.ts：store.ts 是磁盘 IO，
// 这是纯判定。测试要能只测判定，不必造一个项目目录。

/** 一段关系区间（与 relations.json 的字段同名）。 */
export interface RelationInterval {
  fromChapter: number
  toChapter: number
  label?: string
  publicToReader?: boolean
}

/**
 * 第 chapter 章时这段关系是否成立。
 *
 * 边界语义（与 Rust 一致，改一侧就要改另一侧 + 夹具）：
 * - 起章**含**：第 5 章拜师，第 5 章就是师徒
 * - 止章**不含**：写到第 20 章时关系已经变了，第 20 章不再是师徒
 * - 0 表示该端开放：`{from:0,to:0}` = 一直成立
 */
export function intervalCovers(iv: RelationInterval, chapter: number): boolean {
  const started = iv.fromChapter === 0 || chapter >= iv.fromChapter
  const notEnded = iv.toChapter === 0 || chapter < iv.toChapter
  return started && notEnded
}

/** 区间合法性：终点不能早于起点（0 表示开放，不算非法）。 */
export function intervalIsValid(iv: RelationInterval): boolean {
  return iv.toChapter === 0 || iv.toChapter > iv.fromChapter
}

/**
 * 某个时刻仍然成立的所有区间（同一对人物可以有多段并存的关系）。
 */
export function activeIntervals(
  intervals: RelationInterval[],
  chapter: number,
): RelationInterval[] {
  return intervals.filter((iv) => intervalCovers(iv, chapter))
}
