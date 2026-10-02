# 第三十五轮记录 — KN -> Kitten4:补上 `theatre.groups`,角色才显示

## 1. 症状(用户实机 + 无头浏览器复现)

`docs/rounds/34` §4nonies 留下的最后一条:转换产物在 Kitten4 编辑器里
**作品名、变量、场景页签都进得来,但"3 个角色一个都不出现" => 画布 0 块**。

同一手法复测(无头 Chromium + 线上 `kitten4.codemao.cn` 的「打开本地作品」入口,
数画布 `g.blocklyBlockCanvas g.blocklyDraggable`):

| 文件 | 作品名 | 画布积木 |
| --- | --- | --- |
| 对照:本项目的**反编译**产物(能正常读) |  | **52** |
| 本项目的**转换**(KN -> Kitten4)产物(修前) |  | **0** |

## 2. 根因:平台用 `theatre.groups` 表达"谁在场景里",本项目把它写成空表

对照平台原件(反编译产物保留的就是平台结构)与本项目的转换产物:

| 位置 | 平台原件 | 修前本项目的产物 |
| --- | --- | --- |
| `theatre.groups` | **每个角色一条**:`{actors:[角色id], id, is_fold:false, is_group:false, name:"", scene:<场景id>, visible:true}` | **`{}`** |
| `scene.group_order` | 本场景各组 id 的列表(与 `scene.actors` 顺序一致) | **`[]`** |

`scene.actors`、`theatre.actors`、造型/变量这些都对得上(逐项自洽检查:场景引用的角色缺失 0、
样式引用缺失 0),**只有这两处是空的** —— 编辑器按 `groups` + `group_order` 枚举场景里的角色,
两处都空 => 一个角色都不列 => 画布自然 0 块。

为什么空:KN(Neko/KittenN)侧**没有分组概念**(`theatre` 都不存在,实体在顶层
`actors.actorsDict`/`scenes.scenesDict`),写出器于是硬写了空表(`assembly.rs` 原为
`theatre.insert("groups", json!({}))`)。

## 3. 修法

`assembly.rs` 装配阶段(`build_kitten4_document`)按"**一角色一组**"合成:

- 新增 `synthesize_group`:组条目形态**照平台原件**(字段、语义一致),组 id 由 `IdSource` 现铸
  —— `deterministic_ids(true)` 下稳定,基准/回归可逐字节比;
- 遍历每个场景的 `actors`(反向写出的就是 KN `actorIds`),逐角色建组,并把它记进该场景的
  `group_order`;**不在任何 `actorIds` 里的角色**(装配阶段兜底挂到第一个场景)**也补一条**,
  否则它不显示;
- 组 id 现铸需要 `IdSource` => 装配函数多收一个 `&mut model::IdSource`;顺带把
  `landscape`/`canvas`/`kn_stage` 三个参数打包成 `StageSize`(否则触发
  `clippy::too_many_arguments`)。

## 4. 验证(三层)

1. **单测**(`reverse_tests::reverse_synthesizes_groups_so_editor_lists_actors`):
   4 个角色(含一个"不在任何 `actorIds` 里"的兜底角色)必须产出 4 条组;组 id 与键一致、
   组条目字段照平台形态、每组的 `scene` 指向真实场景;每个场景的 `group_order` 只能引用
   **本场景**的组,且每个角色恰好被列一次。
2. **真作品**:`now`(NEKO,273988379)用当前代码重新转换 =>
   `actors=101 / scenes=9 / **groups=101**`,组 id 与键不一致 **0**、组的 `scene` 不在
   `scenes` 里 **0**、`group_order` 引用坏掉 **0**、未被分组的角色 **0**。
3. **实机**(无头 Chromium + 真编辑器):修后同一文件 —— 作品名 `now` 、场景页签 、
   **角色列表出现 `NOW` / `Adobe` / 背景** 、画布上是
   `当收到广播 -> 显示 -> 保持等待直到 -> 切换到造型 -> 在 0.1 秒内逐渐隐藏` 
   (同一手法下对照文件 52 块,说明手法本身有效)。

## 5. 覆盖范围

- **Nemo -> Kitten4** 走的是 `nemo.rs`(产出 KN 文档)=> KN -> Kitten4 => 同一段装配代码 => 一并修好;
- **反编译**路径不涉及:它把平台原件的 `groups` 原样复原(`decompile/editors.rs`)。

## 6. 门

`cargo fmt --check`  / `cargo clippy --all-targets -- -D warnings`  /
`cargo test` 全绿(单测 101 过 / 0 红,`repo_hygiene` 与实机门同过)。

> **方法留档**:跨编辑器转换的判据要**对着平台真实文件比"字段/表形态"**,不能只比积木 ——
> 这一轮的"缺表"在积木层面完全看不出来(积木一块没少,只是编辑器不列角色)。
