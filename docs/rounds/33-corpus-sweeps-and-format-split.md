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

## 2. 两个形态陷阱(本轮最值钱的结论)

Kitten4 文档在库里有**两种形态**,名字像、内容不一样:

| 形态 | 谁产出 | `block_data_json` | 谁吃 |
| --- | --- | --- | --- |
| `download/compile/*.bcm4` | 反编译器 | **map**(`{"blocks":{…}}`) | 上传路线(交回平台) |
| Kitten4 **编辑格式** | 平台"作品源码"接口 | **JSON 字符串** | 正向转换 `convert_kitten4_document` |

**踩坑记录**:第一版正向扫描器直接吃 `*.bcm4`,当场复现
`作品文件解析失败: 外部错误: JSON error: invalid type: map, expected a string`(夹具 `A28社区-开幕_174408420.bcm4` 就能复现)。
⇒ 不是产品 bug,但**是个真坑**:两个半场对同一编辑器的文档形态假设不同,而这条边界没有任何注释写着。

**第二个坑**:`GET /creation-tools/v1/works/{id}/source/public` 用**普通 API 客户端**(`WorkDataFetcher`)
去取会返回 `422 40101001 作品不存在` —— 报错还带误导性;而反编译器内部用**同一个 URL** 却成功
⇒ 差别在**端侧请求头**,由 `CodeMaoHttpClient`(反编译器自己的客户端)带上。
它目前是 `pub(crate)` ⇒ 集成测试里的采集器拿不到,所以编辑格式语料现在还抓不下来。

## 3. 下一轮的第一步(明确、可做)

给库加一个**公开的"取作品源码"入口**(复用 `CodeMaoHttpClient` 的头),或在 `DecompileOptions` 上加一个
`keep_source_doc` 之类的开关,把**编辑格式**文档一并落盘。之后:

- 采集器把 Kitten4 作品的编辑格式文档写进 `download/compile/k4raw/`;
- `k4_corpus_round_trip_sweep` 自动纳入(它已经按这个目录找语料);
- 正向方向就有了真语料 —— 现在它的覆盖面只有官方基线夹具(`diff_tests`),
  而**平台上的 Kitten4 作品是官方编辑器产出的**,写法比夹具杂得多。

> **不做**:不把"map → 字符串"自己拼一遍当语料 —— 那是拿自造的输入验自家转换,结论不可信。

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
