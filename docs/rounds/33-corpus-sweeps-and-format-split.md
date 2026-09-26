# 第三十三轮记录 — 语料扩容 / 双向往返扫描 / 两个形态陷阱

## 1. 做了什么

1. **修掉列表影子丢失**(rounds/32 §3.4):正向 (9) 步原先写在子块循环 `for … inputs.chain(statements)` 体内,
   条件却只依赖块类型与 `fields.list` ⇒ **没有已连接子块的块**永远进不了循环,既不转换字段也不造影子;
   而反向对所有块都折叠 ⇒ 往返一圈丢掉那 28 个 `delete_list_item` 的影子。提到循环外后实体侧差异消失
   (`pure_list_get` 合计 503 → 507),官方基线差分门仍通过。提交 `0dce9d6`。
2. **专测语料 2 → 3 件**:`HEX Editor_317683843.bcmkn` 进 `PROCEDURE_LIBRARIES`(它满足"程序集库"前提);
   `滑动算法_301113412.bcmkn` 是普通作品(没有定义)⇒ 前提不成立,改由新扫描器覆盖。提交 `36bb84f`。
3. **新增反向往返扫描器** `kn_corpus_round_trip_sweep`:任意 `download/compile/*.bcmkn` 自动纳入,
   逐条打印实体侧 / 定义体侧差异。提交 `36bb84f`。
4. **新增语料采集器** `tests/convert_corpus_harvest.rs`(`#[ignore]`):公开发现流
   (`/creation-tools/v1/pc/discover/newest-work`,**匿名可读**)拿 id → 作品详情定类型 → 反编译落盘。
   实测一次抓 8 件(7 件 Kitten4 + 1 件 KN),26 秒。
5. **新增正向往返扫描器** `k4_corpus_round_trip_sweep`(Kitten4 → KN → Kitten4),语料目录
   `download/compile/k4raw/`(见 §3,当前为空 ⇒ 自动跳过)。

## 2. Kitten 文档的**三种**形态(本轮最值钱的结论)

同一个"Kitten 作品文档",库内外至少三种样子,名字像、内容不一样,**喂错的后果还各不相同**:

| # | 形态 | 谁产出 | 积木在哪 | 谁吃 | 喂错会怎样 |
| --- | --- | --- | --- | --- | --- |
| ① | **上传格式**(`download/compile/*.bcm4`) | 反编译器 | `block_data_json` 是 **map** | 上传路线(交回平台) | 正向报错:`invalid type: map, expected a string` |
| ② | **编译态 / 播放器载荷**(`player/load/{id}`) | 平台 | **不在** `actors[*].block_data_json` 里,零散在别处 | 反编译器(它自己就是取这个) | 正向**静默产出空 KN** —— 实测 654 块 → 13、61 块 → 0,不报错 |
| ③ | **编辑格式** | Kitten4 编辑器存盘 | `theatre.actors[*].block_data_json` 是 **JSON 字符串** | 正向转换 `convert_kitten4_document` | —— |

**踩坑记录(两次,都记在这)**:
1. 第一版正向扫描器直接吃 ①`*.bcm4`,当场复现
   `作品文件解析失败: 外部错误: JSON error: invalid type: map, expected a string`(夹具 `A28社区-开幕_174408420.bcm4` 就能复现)。
2. 改用采集器抓到的 `player/load` 载荷(②)后**不报错**,但扫描出"所有块归零"——一度以为正向转换器把程序全丢了。
   逐个打印才发现:那些文档的 `theatre.actors[*]` **根本没有 `block_data_json`**,而我的"块数"口径又在数
   文档里**所有** `type` 字段(把素材/音频对象也算进去了)。⇒ **两次都是语料/口径错,不是产品错**。

⇒ 现在正向扫描器有**形态守卫**:文档里没有任何角色/场景带 `block_data_json` 就直接跳过并说明原因;
块数口径也改成**只数 `block_data_json` 里的块**。

### 2.1 ③ 编辑格式目前**取不到**(端点探测,2026-09-26)

正向转换真正的输入是 ③,而平台能给的只有 ②:

| 端点 | 结果 |
| --- | --- |
| `GET /kitten/r2/work/player/load/{id}` | 200,但给的是 ② 编译态 |
| `GET /kitten/r2/work/edit/load/{id}` | **404** |
| `GET /kitten/work/ide/load/{id}` | **404** |
| `GET /creation-tools/v1/works/{id}/source/public` | **422** `40101001 作品不存在`(缺端侧头;`WorkDataFetcher` 取不到,
编译器自己的 `CodeMaoHttpClient` 头齐,但 Kitten 系走的是 player/load) |

⇒ ③ 只存在于**编辑器自己发出去的保存载荷**里。要拿它当语料,只能从浏览器会话里抓(或找到平台对应端点)。
本轮已经不猜了 —— 猜错的代价就是上面那两次误判。

## 3. 下一轮的第一步(明确、可做)

**A. 拿 ③(编辑格式)语料**:在浏览器里打开 Kitten4 编辑器、保存一个作品,抓那条保存请求的 body
(或逆向出编辑器加载作品时用的端点)。落盘到 `download/compile/k4raw/` 后,
`k4_corpus_round_trip_sweep` 会**自动**纳入它(目录名就是为了这个留的)。
在这之前,正向方向的覆盖面只有官方基线夹具(`diff_tests`)。

**B. 反向侧剩余项**(rounds/32 §4.3):定义体里 `script_variables` / `break` / `repeat_n_times` /
`logic_compare` / `temporary_list` 的减少,随新语料一起复查:先分清哪些是"同一个调用点塌陷"的连带计数。

> **不做**:① 不把"map → 字符串"或"编译态 → 编辑态"自己拼一遍当语料(拿自造输入验自家转换,结论不可信);
> ② 不在没拿到 ③ 之前对正向转换器下"丢程序"的结论(本轮已经证明那是语料错)。

## 3bis. 定义体侧那 6 条 / 21 块"缺口"查清了:**残块归一化**,不是丢失(本轮)

rounds/32 留下的最后一项(定义体里 `script_variables` / `break` / `repeat_n_times` / `logic_compare` /
`temporary_list` / `callreturn` / `parameter` 的减少,预算 `≤6 / ≤21`)本轮**查清了**。

**方法**:把源 / 中间态 / 往返三态都落盘,按定义体逐块做**可达性**分析
(源 KN 用条目内 `parent_id` 链求可达集,中间态按扁平块表的 `parent_id` 反查)。

**结果**:所有"丢失"的块都是**孤儿** —— 它们所在子树的根块 `parent_id` 为空,且**任何可达块都不引用它们**。
逐条对上:

| 定义 | 差异 | 孤儿簇 |
| --- | --- | --- |
| `c50c29e1` | `repeat_n_times -1`、`break -1`、`math_number -1` | `repeat_n_times`(根)+ `break` + 计数块 |
| `b9858f0c` | `logic_compare -1`、`callreturn -1`、`parameter -1` | 孤儿 `logic_compare` + 其 `callreturn`/`parameter` |
| `bfa2f83c` | `callreturn -1`、`callnoreturn -1`、`parameter -2`、`temporary_list -1` | 孤儿 `callreturn`/`callnoreturn` 两簇 |
| `6be6ac61` | `script_variables -1`、`script_variables_param -2`、`variables_set -1` | 孤儿 `script_variables` 簇 |
| `71540544` | `callreturn -1`、`parameter -2` | 孤儿 `callreturn` 簇 |

(唯一"被引用"的孤儿是 mutation 里的 `List`/`String` 伪对象 —— 那是形参**类型元数据**,不是块。)

**结论**:`proceduresDict` 的条目里除定义根外还挂着编辑器残留的、**不属于任何定义**的块;
反向按定义根重建树时它们自然消失(正确行为),正向也不可能凭空再造 ⇒ **归一化**,不是保真损失。

**处置**:
- `def_census` 的口径从"整条条目"改成"**定义根子树**"(`accumulate_subtree`),并保留原有"首根不是定义块就跳过整条"的语义
  (改这条会假红:`定义 id 不丢` 那条断言会因为镜像侧 id 被重铸而误报 —— 本轮踩过);
- 定义体侧的两条豁免(`pure_list_get:`、`procedures_2_callreturn:`)随之删除;
- 预算按 rounds/28 §4.3 从 **`≤6 / ≤21`** 收紧到 **`0 / 0`**,三件真作品语料全绿。

## 4. 已就位的检查手段(留给后面)

```bash
cargo test --lib kn_corpus_round_trip_sweep -- --nocapture     # 反向:任意 download/compile/*.bcmkn
cargo test --lib k4_corpus_round_trip_sweep -- --nocapture     # 正向:download/compile/k4raw/*.json
cargo test --test convert_corpus_harvest -- --ignored --nocapture   # 抓语料(HARVEST_N / HARVEST_KINDS / HARVEST_OFFSET)
```

两个扫描器**只守两条铁律**:① 每件作品都转换得动(解码 / 解析 / 两个方向都不 Err、不 panic);
② 往返确定性(同输入两遍逐字节一致,两腿任一腿不确定都会被抓住)。类型多重集差异**只打印、不断言** ——
新语料上的差异要先读懂语义(结构性?退化数据?缺陷?)再定性,判据留在专测里。

## 5. 本轮扫描结论(反向侧,5 件)

3/5 有差异,**全部落在已文档化的族里**:横屏包装 `math_arithmetic` + `math_number` 成对、
`calculate ⇄ bcm_translator_text_return_value_block` 1:1、定义体侧退化影子 `fields.list="?"` 归一化
(rounds/32 §3.5)。新抓的 `喵大战_259251919.bcmkn` **零差异**,扩充后的 `HEX Editor` 也只多出横屏成对。
