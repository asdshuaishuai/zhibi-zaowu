#!/usr/bin/env python3
"""造一本「形状逼真」的长篇 fixture，用于桌面实机冒烟与性能观察。

不是随机填充：章节有真实的中文叙事段落、人物反复出场但会断续、
伏笔有埋有收有静默、事件挂到具体章、记忆事实与正文对得上。
这样打开应用看到的是"一本真书"，而不是测试数据。
"""
import json
import os
import random
import shutil
import sys
from pathlib import Path

random.seed(20261116)

TITLE = "冒烟长卷"
ROOT = Path(sys.argv[1] if len(sys.argv) > 1 else "/tmp/zhibi-fixture") / f"{TITLE}.zhibi"

PEOPLE = ["沈砚", "顾青", "老周", "阿绣", "裴照", "无咎", "陆七", "钟晚"]
PLACES = ["雨巷", "灯市", "码头", "断桥", "旧宅", "关外"]
OBJECTS = ["断刀", "半块虎符", "灯契", "旧信", "青铜铃", "船票"]

SENTENCES = [
    "{p}按着{o}走进{pl}，雨点把灯影敲成碎片。",
    "{p}没说话，只是把{o}往袖里又推了一寸。",
    "{pl}今夜没有守夜人，只有风从檐下穿过去。",
    "{p}想起那年上元，{q}站在灯下，什么也没说。",
    "{q}把{o}搁在桌上，手上的泥还没干。",
    "远处有人在敲更，三下，又停了。",
    "{p}数着自己的呼吸，等{t}转过街角。",
    "灯芯哔剥一声，{p}的影子在墙上裂开。",
    "{q}说：这盏灯不能灭。{p}没有接话。",
    "{pl}的水位比上个月高了半尺，{p}注意到了。",
]

def prose_for(n, focus):
    q = random.choice([x for x in PEOPLE if x != focus])
    lines = []
    # 每章约 12–18 段，覆盖到接近 chapterWordTarget（真实长篇的形状，
    # 而不是 200 字的示意稿——性能与排版都按真实长度才测得准）
    for _ in range(random.randint(12, 18)):
        lines.append("")
    for _ in range(random.randint(120, 160)):
        t = random.choice(SENTENCES)
        lines.append(
            t.format(
                p=focus,
                q=q,
                o=random.choice(OBJECTS),
                pl=random.choice(PLACES),
                t=random.choice([x for x in PEOPLE if x != focus]),
            )
        )
    return "\n\n".join(lines)


def main():
    if ROOT.exists():
        shutil.rmtree(ROOT)
    for d in ["canon", "proposals", "chapters", "journal"]:
        (ROOT / d).mkdir(parents=True, exist_ok=True)

    total_chapters = 120
    written = 118  # 留两章空章，验证"未动笔"显示

    # project.json：目标 120 章 × 3000 字，带近 14 天写作记录
    daily = {}
    base = 20261102
    for i in range(14):
        daily[str(base + i)] = random.randint(800, 3400)
    (ROOT / "project.json").write_text(
        json.dumps(
            {
                "title": TITLE,
                "genre": "古风悬疑",
                "createdAt": "2026-10-01T00:00:00Z",
                "targetChapters": total_chapters,
                "chapterWordTarget": 3000,
                "dailyWords": daily,
                "coverStyle": 2,
            },
            ensure_ascii=False,
            indent=2,
        ),
        encoding="utf-8",
    )

    # 章节
    for n in range(1, written + 1):
        ch = ROOT / "chapters" / f"ch-{n:03d}"
        ch.mkdir(parents=True, exist_ok=True)
        focus = PEOPLE[(n - 1) % len(PEOPLE)]
        text = prose_for(n, focus) if n % 37 else ""  # 每 37 章放一个空章
        (ch / "prose.md").write_text(text, encoding="utf-8")
        (ch / "meta.json").write_text(
            json.dumps(
                {"title": f"第{n}回 · {random.choice(PLACES)}", "number": n},
                ensure_ascii=False,
                indent=2,
            ),
            encoding="utf-8",
        )

    # 人物：8 人，含别名
    chars = []
    for i, p in enumerate(PEOPLE):
        chars.append(
            {
                "id": f"c{i+1:03d}",
                "name": p,
                "aliases": [p[0] + "哥"] if i % 3 == 0 else [],
                "role": "主角" if i == 0 else ("反派" if i == 4 else "配角"),
                "faction": random.choice(["守灯人", "镖局", "关外客"]),
                "age": str(random.randint(18, 52)),
                "appearance": f"{p}身形清瘦，左眉有一道旧疤。" if i % 2 else f"{p}常年一身靛蓝短打。",
                "personality": random.choice(["沉默寡言", "面冷心热", "喜怒不形于色"]),
                "motivation": random.choice(["查清当年灯约", "找回失散的妹妹", "守住最后一盏灯"]),
                "background": f"{p}生于{random.choice(PLACES)}，少年时随人学过手艺。",
                "voice": random.choice(["说话极短，常只答一个字", "爱用反问，句尾带笑"]),
                "status": "活着",
                "statusChapter": random.randint(1, 60),
                "goals": ["弄清断刀来历"],
                "flaws": ["不肯信人"],
            }
        )
    (ROOT / "characters.json").write_text(
        json.dumps(chars, ensure_ascii=False, indent=2), encoding="utf-8"
    )

    # 故事线
    lines = [
        {"id": "L01", "name": "断刀来历", "status": "推进中", "kind": "main", "isThroughLine": True},
        {"id": "L02", "name": "灯约旧案", "status": "推进中", "kind": "main", "isThroughLine": False},
        {"id": "L03", "name": "顾青身世", "status": "断线", "kind": "sub", "isThroughLine": False},
    ]
    (ROOT / "storylines.json").write_text(
        json.dumps(lines, ensure_ascii=False, indent=2), encoding="utf-8"
    )

    # 事件：约 90 条挂到各章
    events = []
    for i in range(90):
        ch = random.randint(1, written)
        events.append(
            {
                "id": f"E{i+1:03d}",
                "chapter": ch,
                "objectiveFact": f"{random.choice(PEOPLE)}在{random.choice(PLACES)}"
                f"{random.choice(['拿到了', '丢了', '看见了', '烧了'])}{random.choice(OBJECTS)}",
                "revealed": random.random() > 0.25,
                "happened": True,
                "storylineIDs": [random.choice(["L01", "L02", "L03"])],
            }
        )
    (ROOT / "events.json").write_text(
        json.dumps(events, ensure_ascii=False, indent=2), encoding="utf-8"
    )

    # 伏笔：含已收、推进中、静默很久的
    clues = []
    for i in range(18):
        planted = random.randint(1, 100)
        status = random.choice(["埋下", "推进中", "已兑现", "待埋"])
        last = planted if status == "埋下" else min(written, planted + random.randint(0, 40))
        item = {
            "id": f"F{i+1:02d}",
            "title": random.choice(
                ["半块虎符", "灯契缺页", "钟晚的旧信", "码头的船票", "青铜铃的裂纹", "老周的口供"]
            ),
            "detail": "正文里提过一次，尚未解释。",
            "status": status,
            "plantedChapter": planted,
            "lastActionChapter": last,
            "timing": random.choice(["慢", "中", "快"]),
        }
        if status == "已兑现":
            item["resolvedChapter"] = min(written, planted + random.randint(2, 30))
        clues.append(item)
    (ROOT / "clues.json").write_text(
        json.dumps(clues, ensure_ascii=False, indent=2), encoding="utf-8"
    )

    # 记忆：事实 + 别名；故意留两处单值冲突（配偶指向两人）
    facts = []
    fid = 0
    for n in range(1, written + 1, 2):
        fid += 1
        s = PEOPLE[(n - 1) % len(PEOPLE)]
        facts.append(
            {
                "id": f"mem-{fid:03d}",
                "subject": s,
                "predicate": random.choice(["获得", "失去", "师从", "去过", "结义"]),
                "object": random.choice(OBJECTS + PLACES + PEOPLE),
                "fromChapter": n,
                "publicToReader": True,
            }
        )
    facts.append({"id": "mem-c1", "subject": "沈砚", "predicate": "配偶", "object": "顾青", "fromChapter": 30, "publicToReader": True})
    facts.append({"id": "mem-c2", "subject": "沈砚", "predicate": "配偶", "object": "阿绣", "fromChapter": 70, "publicToReader": True})
    aliases = [{"canonicalName": p, "aliases": [p[0] + "哥"]} for p in PEOPLE if random.random() > 0.5]
    (ROOT / "memory.json").write_text(
        json.dumps({"facts": facts, "aliases": aliases}, ensure_ascii=False, indent=2),
        encoding="utf-8",
    )

    # 世界线：α 正典 + 两条预演
    wls = [
        {
            "id": "α", "name": "正典", "premise": "当前事实基准", "kind": "base",
            "forkedChapter": 0, "status": "基准", "nodes": [], "edges": [], "diffs": [],
            "derived": {"characters": [], "canon": [], "clues": []},
        },
        {
            "id": "β", "name": "师兄未死", "premise": "如果老周当年没死", "kind": "what-if",
            "forkedChapter": 40, "status": "推演中",
            "nodes": [
                {"id": "b1", "kind": "event", "label": "老周在码头现身", "chapter": 55, "state": "planned"},
                {"id": "b2", "kind": "event", "label": "师徒对峙", "chapter": 62, "state": "planned"},
            ],
            "edges": [{"from": "b1", "to": "b2", "kind": "influence"}],
            "diffs": [
                {"target": "事件", "targetRef": "老周归来", "kind": "add", "before": "—",
                 "after": "老周在第 55 章现身码头", "reason": "若当年被打捞救起", "severity": "high"},
            ],
            "derived": {"characters": [], "canon": [], "clues": []},
        },
        {
            "id": "γ", "name": "灯约提前", "premise": "如果灯约比预想早一年", "kind": "forecast",
            "forkedChapter": 118, "status": "推演中",
            "nodes": [
                {"id": "g1", "kind": "event", "label": "提前点灯", "chapter": 125, "state": "planned"}
            ],
            "edges": [],
            "diffs": [
                {"target": "事件", "targetRef": "提前点灯", "kind": "add", "before": "—",
                 "after": "第 125 章提前点灯", "reason": "灯契被改动", "severity": "medium"}
            ],
            "derived": {"characters": [], "canon": [], "clues": []},
        },
    ]
    (ROOT / "worldlines.json").write_text(
        json.dumps(wls, ensure_ascii=False, indent=2), encoding="utf-8"
    )

    # 设定
    (ROOT / "canon" / "灯约.md").write_text(
        "# 灯约\n\n每年上元夜，守灯人须点亮一百零八盏灯。少一盏，来年水涨三尺。\n",
        encoding="utf-8",
    )
    (ROOT / "canon" / "断刀.md").write_text(
        "# 断刀\n\n刀身断在第三节，断口呈青灰色。凡持此刀者，皆与旧案有关。\n",
        encoding="utf-8",
    )

    (ROOT / "canvas.json").write_text("{}", encoding="utf-8")
    (ROOT / "proposals" / "inbox.json").write_text("[]", encoding="utf-8")

    words = sum(
        len("".join(c for c in (ROOT / "chapters" / f"ch-{n:03d}" / "prose.md").read_text(encoding="utf-8") if "\u4e00" <= c <= "\u9fff"))
        for n in range(1, written + 1)
    )
    print(f"OK {ROOT}")
    print(f"章 {written}/{total_chapters}，汉字约 {words}，人物 {len(PEOPLE)}，事件 {len(events)}，伏笔 {len(clues)}，事实 {len(facts)}")


if __name__ == "__main__":
    main()
