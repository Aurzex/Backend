# 第三十三轮记录 — 语料扩容 / 双向往返扫描 / 两个形态陷阱

## 1. 改动清单

1. **修复列表影子丢失**(rounds/32 §3.4):正向第 9 步原先写在子块循环 `for … inputs.chain(statements)` 体内,
   条件却只依赖块类型与 `fields.list`,因此**没有已连接子块的块**永远不会进入该循环,既不转换字段也不生成影子;
   而反向对所有块都折叠,因此往返一次丢失那 28 个 `delete_list_item` 的影子。提到循环之外后实体侧差异消失
   (`pure_list_get` 合计由 503 增至 507),官方基线差分门仍通过。提交 `0dce9d6`。
2. **专测语料由 2 件增至 3 件**:`HEX Editor_317683843.bcmkn` 进入 `PROCEDURE_LIBRARIES`(它满足"程序集库"前提);
   `滑动算法_301113412.bcmkn` 是普通作品(没有定义),前提不成立,改由新扫描器覆盖。提交 `36bb84f`。
3. **新增反向往返扫描器** `kn_corpus_round_trip_sweep`:任意 `download/compile/*.bcmkn` 自动纳入,
   逐条打印实体侧 / 定义体侧差异。提交 `36bb84f`。
4. **新增语料采集器** `tests/convert_corpus_harvest.rs`(`#[ignore]`):公开发现流
   (`/creation-tools/v1/pc/discover/newest-work`,**匿名可读**)取 id,再查作品详情定类型,最后反编译落盘。
   实测一次采集 8 件(7 件 Kitten4 与 1 件 KN),耗时 26 秒。
5. **新增正向往返扫描器** `k4_corpus_round_trip_sweep`(由 Kitten4 转为 KN,再返回 Kitten4),语料目录
   `download/compile/k4raw/`(见 §3,当前为空,自动跳过)。

## 2. Kitten 文档的**三种**形态(第三十三轮的关键结论)

同一个"Kitten 作品文档",库内外至少存在三种形态,名称相似而内容不同,**误用后果亦各不相同**:

| # | 形态 | 产出方 | 积木所在 | 消费方 | 误用后果 |
| --- | --- | --- | --- | --- | --- |
| 形态一 | **上传格式**(`download/compile/*.bcm4`) | 反编译器 | `block_data_json` 是 **map** | 上传路线(交回平台) | 正向报错:`invalid type: map, expected a string` |
| 形态二 | **编译态 / 播放器载荷**(`player/load/{id}`) | 平台 | **不在** `actors[*].block_data_json` 里,零散在别处 | 反编译器(其自身即读取该形态) | 正向**静默产出空 KN** —— 实测由 654 块降至 13 块、由 61 块降至 0 块,且不报错 |
| 形态三 | **编辑格式** | Kitten4 编辑器存盘 | `theatre.actors[*].block_data_json` 是 **JSON 字符串** | 正向转换 `convert_kitten4_document` | 无 |

**两次误判记录**:
1. 第一版正向扫描器直接读取形态一 `*.bcm4`,即复现
   `作品文件解析失败: 外部错误: JSON error: invalid type: map, expected a string`(夹具 `A28社区-开幕_174408420.bcm4` 即可复现)。
2. 改用采集器取得的 `player/load` 载荷(形态二)后**不报错**,但扫描出"所有块归零"——一度误判正向转换器丢弃了全部程序。
   逐个打印后才查明:那些文档的 `theatre.actors[*]` **并不存在 `block_data_json`**,而块数口径又在统计
   文档里**所有** `type` 字段(将素材/音频对象一并计入)。因此**两次误判均源于语料或口径,而非产品缺陷**。

由此,正向扫描器已具备**形态守卫**:文档里没有任何角色/场景带 `block_data_json` 就直接跳过并说明原因;
块数口径也改为**只统计 `block_data_json` 里的块**。

### 2.1 形态三编辑格式**无法获取**(端点探测,2026-09-26)

正向转换的实际输入为形态三,而平台能提供的只有形态二:

| 端点 | 结果 |
| --- | --- |
| `GET /kitten/r2/work/player/load/{id}` | 200,但返回的是形态二编译态 |
| `GET /kitten/r2/work/edit/load/{id}` | **404** |
| `GET /kitten/work/ide/load/{id}` | **404** |
| `GET /creation-tools/v1/works/{id}/source/public` | **422** `40101001 作品不存在`(缺少端侧请求头;`WorkDataFetcher` 取不到,
编译器自己的 `CodeMaoHttpClient` 头齐,但 Kitten 系走的是 player/load) |

因此形态三只存在于**编辑器自身发出的保存载荷**里。若要将其作为语料,只能从浏览器会话中抓取(或找到平台对应端点)。
第三十三轮不再推测,推测的代价即上述两次误判。

## 3. 下一轮的第一步(明确、可做)

**A. 获取形态三(编辑格式)语料**:在浏览器中打开 Kitten4 编辑器并保存一个作品,抓取该保存请求的 body
(或逆向出编辑器加载作品所用的端点)。落盘到 `download/compile/k4raw/` 后,
`k4_corpus_round_trip_sweep` 会**自动**纳入它(目录名即为此预留)。
在此之前,正向方向的覆盖面只有官方基线夹具(`diff_tests`)。

**B. 反向侧剩余项**(rounds/32 §4.3):定义体里 `script_variables` / `break` / `repeat_n_times` /
`logic_compare` / `temporary_list` 的减少,随新语料一起复查:先分清哪些是"同一个调用点塌陷"的连带计数。

> **不做**:其一、不将"map 到字符串"或"编译态到编辑态"的转换自行拼装为语料(以自造输入验证自身转换,结论不可信);
> 其二、未取得形态三之前,不对正向转换器作出"丢程序"的结论(第三十三轮已证明其为语料错误)。

## 3bis. 定义体侧 6 条 / 21 块的"缺口"已查明:**残块归一化**,并非丢失(第三十三轮)

rounds/32 留下的最后一项(定义体里 `script_variables` / `break` / `repeat_n_times` / `logic_compare` /
`temporary_list` / `callreturn` / `parameter` 的减少,预算不超过 6 / 21)第三十三轮**已查明**。

**方法**:将源 / 中间态 / 往返三态全部落盘,按定义体逐块做**可达性**分析
(源 KN 用条目内 `parent_id` 链求可达集,中间态按扁平块表的 `parent_id` 反查)。

**结果**:所有"丢失"的块都是**孤儿** —— 它们所在子树的根块 `parent_id` 为空,且**任何可达块都不引用它们**。
逐条对应如下:

| 定义 | 差异 | 孤儿簇 |
| --- | --- | --- |
| `c50c29e1` | `repeat_n_times -1`、`break -1`、`math_number -1` | `repeat_n_times`(根)、`break` 与计数块 |
| `b9858f0c` | `logic_compare -1`、`callreturn -1`、`parameter -1` | 孤儿 `logic_compare` 及其 `callreturn`/`parameter` |
| `bfa2f83c` | `callreturn -1`、`callnoreturn -1`、`parameter -2`、`temporary_list -1` | 孤儿 `callreturn`/`callnoreturn` 两簇 |
| `6be6ac61` | `script_variables -1`、`script_variables_param -2`、`variables_set -1` | 孤儿 `script_variables` 簇 |
| `71540544` | `callreturn -1`、`parameter -2` | 孤儿 `callreturn` 簇 |

(唯一"被引用"的孤儿是 mutation 里的 `List`/`String` 伪对象 —— 其为形参**类型元数据**,并非块。)

**结论**:`proceduresDict` 的条目里除定义根外还挂有编辑器残留的、**不属于任何定义**的块;
反向按定义根重建树时它们自然消失(正确行为),正向也无法重新生成,因此这是**归一化**,并非保真损失。

**处置**:
- `def_census` 的口径从"整条条目"改成"**定义根子树**"(`accumulate_subtree`),并保留原有"首根不是定义块就跳过整条"的语义
  (修改该条会使断言误报:`定义 id 不丢` 那条断言会因为镜像侧 id 被重铸而误报,第三十三轮已遇到此情况);
- 定义体侧的两条豁免(`pure_list_get:`、`procedures_2_callreturn:`)随之删除;
- 预算按 rounds/28 §4.3 从不超过 6 / 21 收紧到 0 / 0,三件真作品语料全部通过。

## 4. 已就位的检查手段(供后续使用)

```bash
cargo test --lib kn_corpus_round_trip_sweep -- --nocapture     # 反向:任意 download/compile/*.bcmkn
cargo test --lib k4_corpus_round_trip_sweep -- --nocapture     # 正向:download/compile/k4raw/*.json
cargo test --test convert_corpus_harvest -- --ignored --nocapture   # 抓语料(HARVEST_N / HARVEST_KINDS / HARVEST_OFFSET)
```

两个扫描器**只守两条准则**:其一、每件作品都能完成转换(解码 / 解析 / 两个方向均不 Err、不 panic);
其二、往返确定性(同一输入两遍逐字节一致,任一步骤不确定均会被检出)。类型多重集差异**只打印、不断言**;
新语料上的差异须先辨明语义(结构性问题、退化数据还是缺陷)再定性,判据保留在专测中。

## 5. 第三十三轮扫描结论(反向侧,5 件)

5 件中 3 件有差异,**全部落在已文档化的族内**:横屏包装 `math_arithmetic` 与 `math_number` 成对、
`calculate` 与 `bcm_translator_text_return_value_block` 一一对应、定义体侧退化影子 `fields.list="?"` 归一化
(rounds/32 §3.5)。新采集的 `喵大战_259251919.bcmkn` **零差异**,扩充后的 `HEX Editor` 也只多出横屏成对。
