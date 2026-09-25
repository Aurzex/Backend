# 第二十一轮方案 — convert 域收敛(文件数 → ~15)与逻辑重构

日期:2026-09-25 · 基线:HEAD `1dfa363`(第二十轮已落地) · 范围:`src/core/convert/**`(36 文件 / 14,321 行 = 生产 impl 9 801 + 生成物 1 665 + 测试 2 855)+ `src/utils/filedata.rs` 的 convert 相关项
本轮**只出方案,不改代码**。功能不变是硬前提;所有结论都有 `路径:行号` 证据(评审方法见附录 A)。

---

## 0. 结论摘要

| 项 | 现状 | 目标 |
| -- | ---- | ---- |
| 生产文件数 | **34**(另:生成物 1、测试文件 1) | **16**(可选压到 13) |
| 测试文件数 | 1(`reverse_tests.rs`)+ 8 处内联 | 3(`translate/tests/{mod,forward,reverse}.rs`) |
| 最大文件 | `mapping.rs` 1 923 行 | `mapping.rs` ~1610(去内联测试)、`assembly.rs` ~1430 |
| 纯 plumbing 文件 | `shared/mod.rs` 37 行、`editors/mod.rs` 36 行纯再导出 | 合并/收敛(见 §3) |
| 已知缺陷 | 1 条并发竞态(P0)、1 条目录失效(P0)、1 条弱化 XML 工具(P0) | 本轮方案的第一批提交修掉 |
| 三条待定已定 | 编译版引用=内联对象(2 236 处取样)、`keep_source` 接线、重传资源不算有损 | 见 §7(2026-09-25 定) |

三条判断(与直觉不同,但证据在 §2):

1. **文件多不是职责多,而是重构期的"小文件习惯"**:36 个文件里 2 个是纯 `mod`/再导出(`shared/mod.rs` 37、`editors/mod.rs` 37),7 个是基础设施碎片(17~108 行:`fetch`/`crypto`/`http`/`json`/`files`/`error`/`model`),典型的一个函数一个文件(如 `shared/fetch.rs` 只有 17 行);真正的"大块逻辑"只有 4 个(`mapping` 1610、`neko` 910、`finish` 763、`kitten4_finish` 667)。
2. **压缩的杠杆是"抽测试 + 并碎石"**:全部内联测试 **1 765 行**(占生产 impl 9 801 行的 18%),其中 `finish.rs` 449、`translate/mod.rs` 417、`neko.rs` 341、`mapping.rs` 313;抽走后生产文件平均瘦 18%,三个大文件分别瘦 16%(mapping)/27%(neko)/37%(finish)。而 `translate/mod.rs` 本体就有 **437 行**(门面 + 管线混在一起),它需要的是拆分而不是"去测试"。
3. **`shared/` 名不副实**:9 个文件里只有 3 块(`DecompilerError`/`EditorType`+`WorkId`/`IdGenerator`)真被 `translate` 依赖,其余(`ValueExt`/`DecompilerConfig`/`ShadowTemplate`/`WorkInfo`/`FileService`/`CryptoService`/`HttpClient`/`RawWorkData`)**只有 `decompile` 用**——按"共享"摆放会持续误导后来者把 decompile-only 的东西往里堆。

---

## 1. 现状盘点

### 1.1 分组与体量(impl / test 行数按 `#[cfg(test)]` 切)

| 组 | 文件数 | impl 行 | test 行 |
| -- | ------ | ------- | ------- |
| 域门面/子域门面(`convert/mod` 174、`decompile/mod` 383、`translate/mod` 437) | 3 | 994 | 417 |
| `shared/` 地基(含 37 行纯再导出的 `mod.rs`) | 9 | 1 015 | 0 |
| `decompile/` 引擎与编辑器 | 14 | 3 012 | 0 |
| `translate/` 实现(含 `report` 140) | 8 | 4 780 | 1 348 |
| `translate/` 测试(独立文件 `reverse_tests.rs`) | 1 | 0 | 1 090 |
| 生成物 `tables_gen.rs` | 1 | 1 665 | 0 |
| **合计** | **36** | **11 466** | **2 855**(生产 impl 9 801 + 生成物 1 665) |

> 口径:`impl` = 文件内**顶层** `#[cfg(test)]` 起的行之前;`translate/mod.rs:45` 的 `#[cfg(test)] mod reverse_tests;` 只是模块声明,测试体从 `:437` 起;`reverse_tests.rs` 由该声明挂载,文件内无标记,整体按测试计。

### 1.2 需要特别点名的文件

| 文件 | 现象 |
| ---- | ---- |
| `shared/mod.rs`(37) | 纯再导出门面,`pub use` 与 `convert/mod.rs:21` 形成**双层再导出链** |
| `decompile/editors/mod.rs`(36) | 同上,纯再导出 |
| `shared/files.rs`(83) | 两个互不相关的关注点:`FileService`(仅 decompile)+ `IdGenerator`(跨域) |
| `shared/model.rs`(153) | 跨域(`EditorType`/`WorkId`)+ 仅 decompile(`WorkInfo`)+ 死谓词混在一起 |
| `translate/mod.rs`(854) | 门面 + 公开类型 + **整条管线实现**塞在一起(437 行 impl);另有 417 行内联测试 |
| `translate/reverse_tests.rs`(1 089) | 纯测试,却多包了一层无意义的 `mod reverse_tests_inner` |
| `mapping.rs`(1 923) | 正+反双向算法 + 手写表 + 313 行内联测试,三合一 |

---

## 2. 评审发现

### 2.1 正确性问题(必须先修,带证据)

| # | 问题 | 证据 | 为什么现有测试抓不到 |
| - | ---- | ---- | -------------------- |
| **P0-1** | `translate_works` 并发时**争用同一个 staging 目录并互相删除**:每个 `translate_work` 都用 `download/convert/staging`,`remove_dir_all` 在反编译与转化之间执行;`translate_works` 用 `thread::scope` 并发跑同一 chunk → 线程 A 可能删掉线程 B 正在用的源文件 | `convert/mod.rs:43-45`(共享目录)、`:65`(无条件删)、`:94-108`(并发) | 62 个单测都不经过 `translate_works`(需活平台 + `concurrency>1`) |
| **P0-2** | NEMO/WOOD **忽略 `DecompileOptions::output_dir`**:产物仍写 `default_output_dir`,而 raw 数据写进自定义目录 → 一次调用产物分落两地 | `decompile/editors/nemo.rs:67`、`decompile/editors/wood.rs:53`(`let base_dir = &context.config.default_output_dir;`);对比 `contract.rs:32` 的 `output_dir.unwrap_or(...)` 与 `contract.rs:59-66` 的 `save_path_result` 直接返回旧路径 | 无单测;live 测试都用默认目录 |
| **P0-3** | `neko.rs` 的 `attr_value` 是 `mapping.rs` 同名的**弱化版**:`xml.find('>')` 不跳过引号内的 `>`,mutation 属性值里含 `>` 会截错 | `translate/neko.rs:811-830` 用 `xml.find('>')` 取标签尾,未复用 `mapping.rs:430-440 start_tag_end`(引号感知);且 attr 匹配逻辑与 `mapping.rs:400-416 attr_span` 重复 | `unrewrite_calls` 现有用例的 mutation 不含引号内 `>` |
| **P0-4**(已核实:**官方行为,不改**) | 云列表(`cloud_lists_length/get_value/delete`)只产 `inputs.list`、**不产** `shadows.list`,而普通列表两者都产 —— 这是**官方刻意如此**,不是我们漏了 | 官方 bundle 定点:`main-vendors`(sha256 `23dd6052…`)byte **5922654** 的云列表分支只做 `c.inputs.list = T; delete c.fields.list`;普通列表分支(77699+)另写 `c.shadows.list` | — 仅需在 `mapping.rs` 加注释说明该不对称是有意对齐官方 |

### 2.2 重复(双侧锚定)

| # | 重复物 | 位置 A | 位置 B | 判定 |
| - | ------ | ------ | ------ | ---- |
| D1 | XML 属性读取(`attr_span`/`attr_value`/`set_attr_value`/`start_tag_end`) | `mapping.rs:400-440` | `translate/neko.rs:811-830`(只取 `find('>')`,弱化版) | 真重复,合一(顺带修 P0-3) |
| D2 | JS 值强转族 | `mapping.rs:234 js_text` / `:246 value_key` / `:256 truthy` | `finish.rs:725 truthy` / `:741 text` / `:750 number`;`translate/neko.rs:892 field_text` | 真重复;且**`truthy` 语义分歧**(mapping `NaN→真`、finish `NaN→假`) |
| D3 | `XHTML` 命名空间常量 | `mapping.rs:70` | `translate/neko.rs:74` | 真重复(合一) |
| D4 | `math_number` 节点构造 | `mapping.rs:517 math_number_node` | `translate/neko.rs:357-374`(`VC` 补默认 `math_number`,内联同形) | 该合并 |
| D5 | `num()` 逐字节相同 | `finish.rs:716-721` | `kitten4_finish.rs:660-665` | 该合并 |
| D6 | `project_name` 兜底 | `finish.rs:515-518` | `kitten4_finish.rs:633-637`(常量不同) | 该合并 |
| D7 | `theme ↔ style` 正逆表 | `finish.rs:588-597` | `kitten4_finish.rs:29-40` | 该合并为一张双向表 |
| D8 | 坐标正逆公式 | `finish.rs:565-584 stage_position` | `kitten4_finish.rs:641-658 kitten4_position` | 该合并(互逆,写在一处防漂移) |
| D9 | Kitten4 版本串 `"4.11.20"` | `convert/mod.rs:149` | `kitten4_finish.rs:23 KITTEN4_APPLICATION_VERSION` | 该合并为一个常量 |
| D10 | 「到 `download/convert` 的路径」 | `filedata.rs:70-72 convert_file_path()`(仅 `translate_file` 用) | `convert/mod.rs:43-45`(手拼 `…/convert/staging`) | 该统一(顺带修 P0-1) |
| D11 | 根块发现(compiled_block_map 里未被引用者) | `decompile/editors/kitten/decompiler.rs:47-90`(识别字符串+对象引用) | `decompile/editors/kitten/xml.rs:26-53`(只识别对象引用) | 真重复且**契约分叉**;合一并统一"两种引用都识别" |
| D12 | 子插槽命名规则(`controls_if→DO{n}/ELSE` 等) | `decompile/editors/kitten/xml.rs:130-141` | `decompile/blocks/mod.rs:25-40` | 真重复(合一) |
| D13 | per-editor fetcher 样板(「取 URL → 二次 GET」) | `decompile/editors/kitten/mod.rs:36-49` | `decompile/editors/coco.rs:29-42` 等 | **不建议合并**(每个编辑器的差异点才是重点);可选抽公共构造 |
| D14 | `EditorType::is_kitten` 与 `use_xml_shadow` 分支一字不差 | `shared/model.rs:51-56` | `shared/model.rs:69-76` | 前者零调用 → 删(见 2.3) |

### 2.3 死代码与未用参数(全部有 grep 证据,删除安全)

| 符号 | 位置 | 证据 |
| ---- | ---- | ---- |
| `BlockJson::{collect_ids,input,input_mut,is_empty_node,walk_mut}`、`BlockTree::{find_root,walk_mut}` | `blockjson.rs:164,190,199,204,213,231,253` | 全仓零调用(除定义处) |
| `ParsedEntity.comments` + 其深拷贝 | `kitten.rs:34,82-86` | 唯一消费者 `translate/mod.rs:273` 只取 `.tree`;反向硬编码 `comments: {}` → **注释从不往返** |
| `from_value` 的 kind 兜底分支 | `blockjson.rs:134-141` | `de_string` 已把缺失/null 折成 `""`,该分支不可达 |
| `EditorType::is_kitten/is_nemo/is_neko/is_coco/is_wood` | `shared/model.rs:51-68` | 零调用(仅 `use_xml_shadow` 有调用者) |
| `ValueExt::{get_str,get_i64,get_bool,get_object,get_array,get_str_opt}` | `shared/json.rs:20,28,36,44,52,82` | 零调用(trait 是 `pub(crate)`,编译器不报 dead_code) |
| `BlockDecompilerFactory` 的 `config`/`id_generator` 字段 | `blocks/special.rs:509-522` | 只存不读,`create` 纯转发自由函数 |
| `DecompilerContextBuilder::file_service` setter | `decompile/context.rs:55-57` | 零调用(setter 无人调用 → `build` 永远走 `None` 分支) |
| `DecompilerConfig::client_secret` | `shared/config.rs:35,371` | 零读取 |
| `WorkInfo::{version,preview_url,application_version}` | `shared/model.rs:110-118` | 只写不读(`user_id`/`name`/`id`/`work_type` 有使用) |
| `blocks/mod.rs:9` 的 `pub(crate) use core::BlockDecompilerCore` | `decompile/blocks/mod.rs:9` | 零消费者(special.rs 走 `super::`) |
| `BlockDecompilerBehavior` trait + `get_child_input_name` 的 `_conditions_count` | `blocks/mod.rs:13-15,25` | 单实现单调用;参数被所有实现忽略 |
| `TranslateOptions::keeps_source` / `keep_source` 字段 | `translate/mod.rs:77,89,144-146` | **访问器零调用**——承诺的「保留源作品引用」从未实现 |
| `build_kitten4_document` 的 `Result` 包装 | `kitten4_finish.rs:197-205` | 函数体无 `?`/`Err`,恒 `Ok` |
| `translate_kn_to_kitten` 的 `ids` 参数 / `RevCtx._ids` | `mapping.rs:819,828` | 反向不现铸 id,参数全程未读 |

### 2.4 一致性、可见性与公开面

| # | 问题 | 证据 |
| - | ---- | ---- |
| C1 | 两个方向**错误类型不对称**:正向装配用 `DecompilerError`(经 `.map_err(TranslateError::from)` 拉回),反向直接 `TranslateError` | `finish.rs:98` vs `kitten4_finish.rs:197`;`translate/mod.rs:320-321` |
| C2 | `DecompilerError::Decompile` 文案「反编译错误」被 translate 用来承载「积木图有环/连接指向不存在」等解析错误 | `shared/error.rs:7-28`;构造点 `kitten.rs:111,132,146,158,169` |
| C3 | `is_lossy` 把「官方会重新上传」的 `DroppedProperty` 也算有损 → **正向 `strict` 在任何含 `data:`/非 https 造型的真作品上必失败** | `report.rs:88-93`(`is_lossy` 只豁免 `RemintedId`);构造点 `finish.rs:343,371`;正向 strict 无测试 |
| C4 | `detect_editor` 永不返回 `Kitten2`/`Coco`/`Wood`;Kitten2 被标成 `Kitten3`(与错误文案「Kitten2/3」口径不一致) | `translate/mod.rs:400-435` |
| C5 | `create_draft_work` 草稿名是 `"转化自检 {id} (可删)"`(注释却写「沿用源作品名」);「自检」疑似笔误,会落到用户平台草稿名 | `convert/mod.rs:127-128` |
| C6 | `StageOrientation` 文档说显式方向「只换画布尺寸、不改坐标」,但 `kitten4_position` 实际按画布缩放坐标;`kitten4_variables` 里 `let _ = landscape;` 直接丢弃参数 | `kitten4_finish.rs:46-48,641-658,518` |
| C7 | 可见性过宽:`translate_type`/`is_text_placeholder`/`shadow_xml` 仅本文件用却 `pub(crate)` | `mapping.rs:210,219,224` |
| C8 | 双层再导出链,`convert/mod.rs:21` 与 `shared/mod.rs:33-36` 都在让同一批类型变 `pub` | 两处 |
| C9 | 跨域撞名:`cloudvar.rs:68` 另有 `EditorType`(Kitten/Nemo/Coco/KittenN),与 convert 的同名不同义 | `src/core/cloudvar.rs:68`(本 slice 外,建议随本轮改名) |
| C10 | `translate_work` 里 `work_id_created.or(outcome.work_id)` 永不生效(`translate_file` 恒 `None`) | `convert/mod.rs:69`;`translate/mod.rs:391-396` |

### 2.5 性能(3.7 MB 真作品下会放大)

| # | 问题 | 证据 |
| - | ---- | ---- |
| F1 | 正向查表是**线性扫描**:`translate_type`(表 368 条)/`shadow_xml`/`get_mapped_name`/`map_field_name`/`mapped_field_text` 每积木每影子各扫一次;而**反向已用 `LazyLock<BTreeMap>`** → 两个方向实现风格不一致,且正向是 O(n·m) 字符串比较 | `mapping.rs:210,224,303,312,267` vs `:844,854` |
| F2 | 热路径无谓 clone:`route_children` 每节点 `node.kind.clone()`;`get_mapped_name(...).into_owned()` 与影子循环无条件 `into_owned()`(`Cow` 白做);`parse_kn_entity` 整个 `nekoBlockJsonList` `to_vec()` | `mapping.rs:754,768,712-717`;`translate/neko.rs:494` |
| F3 | `NekoDecompiler::decompile` 每次 `crypto_service.clone()`(克隆 salt `Vec<u8>`);`BCMKNDecryptor` 只是 `CryptoService::decrypt_bcmkn` + `from_utf8` + `parse` 的薄壳,且仅此一处使用 | `decompile/editors/neko.rs:82-83`;`shared/crypto.rs:91-108` |

### 2.6 测试组织

| # | 问题 | 证据 |
| - | ---- | ---- |
| T1 | 内联测试占生产文件 18%:`finish.rs` 449/1212(37%)、`translate/mod.rs` 417/854(49%)、`neko.rs` 341/1251(27%)、`mapping.rs` 313/1923(16%)、`kitten.rs` 166/466(36%)、`ids.rs` 41/100(41%)、`blockjson.rs` 38/369(10%) | 见 §1.1 |
| T2 | `reverse_tests.rs` 多包一层无意义的 `mod reverse_tests_inner`(所有 `#[test]` 都在里面) | `reverse_tests.rs:6-16` |
| T3 | 测试落盘到固定系统临时目录 `temp_dir()/backend-convert-test`(跨运行残留/同名碰撞风险),偏离 cargo 的 `target/` 约定 | `translate/mod.rs:796,844`;`reverse_tests.rs:1061-1077` |
| T4 | 覆盖缺口:**正向 `strict`、显式 `StageOrientation`、`translate_works` 并发、NEMO/WOOD 自定义输出目录** 四条路径零断言(与 §2.1 的 P0 一一对应) | — |

---

## 3. 目标结构(16 个生产文件 + 3 个测试文件)

### 3.1 合并映射表(行数为 impl 口径,不含测试)

| # | 目标文件 | 来源(现状) | 预计行数 | 合并理由 |
| - | -------- | ---------- | -------- | -------- |
| 1 | `convert/mod.rs` | 同现状(174) | 174 | 域门面 + 跨子域编排,位置不变 |
| 2 | `convert/shared.rs` | `shared/{mod,error,model*,files*}` 抽出跨域三块(≈200) | 200 | `shared/` 只保留**真跨域契约**:错误 + `EditorType`/`WorkId` + `IdGenerator`;4 个碎片文件(16~85 行)不值得各占一个 |
| 3 | `decompile/engine.rs` | `decompile/{mod,context,contract}` + `shared/fetch.rs` + `shared/model.rs` 的 `WorkInfo` | ≈596 | 「反编译域编排与契约」——改作品类型/改保存策略必然同时动它们 |
| 4 | `decompile/config.rs` | `shared/config.rs` + `decompile/shadow.rs` | ≈564 | `ShadowBuilder` 就是「把 config 大表变成 shadow」,同一条数据流 |
| 5 | `decompile/infra.rs` | `shared/{crypto,http,json}` + `shared/files.rs` 的 `FileService` | ≈306 | 全是无状态工具、只被 decompile 调用;合一个「引擎管道」文件 |
| 6 | `decompile/blocks.rs` | `decompile/blocks/{mod,core,special}` | ≈993 | 积木反编译是一个状态机(上下文 + 骨架 + 专用块分派);990 行是一次专门的「块类型分派表」,可接受 |
| 7 | `decompile/editors/mod.rs` | 同现状(36) | 40 | 注册表(插件 seam),保留 |
| 8 | `decompile/editors/kitten.rs` | `editors/kitten/{mod,decompiler,xml}` | ≈835 | Kitten 2/3/4 的抓取 + 反编译 + `blocksXML` 序列化是一体三面 |
| 9 | `decompile/editors/simple.rs` | `editors/{neko,coco,wood}` | ≈554 | 三个「单文件抓取+解密/重组」编辑器;`coco` 与 `kitten` 的 fetcher 近乎逐字相同,合并在小体量下更好维护 |
| 10 | `decompile/editors/nemo.rs` | 同现状(297) | 297 | 目录树重组 + 资源管理器自成一体,保留 |
| 11 | `translate/mod.rs` | `translate/mod.rs` 去测试(437)+ `report.rs`(140) | ≈575 | 门面 + 公开类型 + 双向管线;`translate_file`/`translate_work`/`translate_works` 的编排与报告类型同为公开面 |
| 12 | `translate/model.rs` | `translate/{blockjson 331,ids 59}` | ≈390 | 中核节点/树 + id 生成,都是「数据模型」,无算法 |
| 13 | `translate/kitten.rs` | 同现状(466,去测试 167) | ≈300 | Kitten4 邻接表 adapter(双向) |
| 14 | `translate/neko.rs` | 同现状(1250,去测试 342) | ≈910 | KN adapter + 程序集(zC/KC 及其逆向) |
| 15 | `translate/mapping.rs` | 同现状(1923,去测试 314) | ≈1610 | 语义映射双向 + 手写表;**唯一语义决策层**,不宜再拆(正反逐条对照的心智模型) |
| 16 | `translate/assembly.rs` | `translate/{finish 762 + kitten4_finish 667}` | ≈1430 | 两个「文档装配」互为逆映射、共享大量助手(见 D5~D9);合在一处才能把助手与常量收敛成一份 |
| — | `translate/tables_gen.rs` | 同现状(生成物) | 1665 | 不计入生产文件 |
| T1-T3 | `translate/tests/{mod,forward,reverse}.rs` | 内联测试(1 765)+ `reverse_tests.rs`(1 090)+ 夹具 | ≈2 900 | 测试集中、生产文件瘦身;`mod.rs` 提供夹具(小作品 JSON + 官方基线) |

**计数**:生产 16 个(含生成物则 17)。要压到 **13** 的三个可选合并(见 §3.3)。

### 3.2 明确**不合并**及理由

| 保留 | 理由 |
| ---- | ---- |
| `decompile/editors/{nemo}.rs` 与 `editors/mod.rs` | per-editor 文件是 registry 的插件 seam:新增编辑器=新文件 + 注册一行,合并会让所有编辑器在同一文件里耦合演进 |
| `translate/{mapping,neko,assembly}.rs` | 三块各自 >900 行、变更节奏不同(映射 vs 程序集 vs 装配);合并只会变成 4000 行巨石 |
| `convert/mod.rs` 与 `translate/mod.rs` | 两级公开面(域/子域),是文档与发现性的锚点 |
| `tables_gen.rs` | 生成物,与手写代码混放会让格式化和 diff 都变脏 |
| `decompile/blocks.rs` 里的专用块分派 | 与 core 骨架分离会让「块类型表」的增删牵动骨架代码 |

### 3.3 更激进的三个可选合并(若要 13 个生产文件)

1. `decompile/config.rs` → 并入 `decompile/engine.rs`(≈1150):配置是引擎的固有部分,但单文件会变成全仓最大。
2. `decompile/editors/simple.rs` → 并入 `editors/mod.rs`(≈590):注册表 + 三个轻量编辑器。
3. `translate/kitten.rs` → 并入 `translate/model.rs`(≈690):「Kitten 邻接表编码 + 中核模型」,但对 `neko.rs` 的对称性变差。

> 建议:**先落 16 个**,观察一轮后再决定是否做 §3.3。

---

## 4. 逻辑重构清单(按优先级;功能不变)

每条格式:**问题 → 方案 → 行为风险 → 验证**。

### P0(缺陷,先修)

1. **`translate_works` staging 竞态**(§2.1 P0-1)
   → 每个 work 用独立 staging 子目录(`convert_file_path().join("staging").join(work_id)`,或 `IdGenerator` 现铸短 id),只删自己的子目录;路径统一走 `PathConfig::convert_file_path()`(顺带 D10)。
   → 风险:仅影响 `translate_works` 并发路径;单作品路径行为不变。
   → 验证:新增一个不依赖网络的结构测试(注入假反编译不可行时,至少断言两个 work 的 staging 子目录名不同)+ 现有测试全绿。

2. **NEMO/WOOD 忽略 `output_dir`**(§2.1 P0-2)
   → `DecompileResult::Path` 语义改为「相对输出目录的路径」,由 `save_result` 用 `output_dir.unwrap_or(default)` 拼绝对路径;`save_path_result` 同步改签名。
   → 风险:**可见行为变化**(NEMO/WOOD 产物落点)→ 属缺陷修复,需在 CHANGELOG/README 注明;依赖旧行为的调用方极少(README 未承诺)。
   → 验证:新单测(传自定义目录断言产物落点)+ live 测试默认目录不变(现有 `compile_live` 的 NEMO 用例是 `#[ignore]`,可临时打开跑一次)。

3. **`neko::attr_value` 弱化版**(§2.1 P0-3)
   → 删除 `translate/neko.rs:811-830` 的本地实现,改用 `mapping.rs` 的 `start_tag_end`(430-440)+ `attr_span`(400-416)(见 D1 合并)。
   → 风险:极低(更严格的解析是修正而非改语义)。
   → 验证:新增 1 个「属性值内含 `>`」的 `attr_value` 单测;`unrewrite_calls` 现有测试保持绿。

4. **云列表 shadow 不对称 → 写注释固化**(§2.1 P0-4)
   → 在 `mapping.rs` 云列表分支加注释:官方 byte 5922654 只写 `inputs.list`;普通列表写 `shadows.list`。**不改行为**。
   → 风险:无。
   → 验证:`cargo test`(含官方差分门)不变。

### P1(去重与一致性)

5. **XML 工具合一**(D1)→ 抽到 `translate/model.rs`(或独立小工具):`attr_span`/`attr_value`/`set_attr_value`/`start_tag_end`。
   → 风险:低。→ 验证:差分门 + 新增 `>` 用例(与 P0-3 合并提交)。

6. **JS 强转族合一 + 统一语义**(D2)→ 一个 `coerce` 模块(`truthy`/`as_text`/`as_number`/`value_key`),统一 `NaN→假`(与 `finish` 现有语义一致),`mapping`/`finish`/`neko`/`assembly` 改用。
   → 风险:低(serde_json 不可表示 NaN,实际输入无分歧;但需保留 `finish` 的既有断言)。
   → 验证:把三方现有相关断言集中到新模块的表驱动测试里。

7. **装配侧共享助手与常量收敛**(D5~D9,C1)→ `translate/assembly.rs` 内提供唯一一份 `num`/`project_name`/`theme ↔ style` 双向表/`stage_position ↔ position` 互逆对/`KITTEN4_APPLICATION_VERSION`;反向不再自查表;两个装配方向统一返回 `TranslateError`(去掉 `DecompilerError::TypeMismatch` 渗入)。
   → 风险:低(纯抽取;`num` 逐字节相同、坐标公式互逆已由往返测试覆盖)。
   → 验证:官方差分门(正向)+ KN→K4→KN 往返(反向)全绿;补一个「显式 Portrait/Landscape」的反向测试(填 T4 缺口,顺带 C6)。

8. **`is_lossy` 分类修正**(C3,已定)→ 新增 `TranslateWarning::ReuploadedOnImport { path }`(非有损,文案:「官方导入时会重新上传资源,产物 url 由平台重写;我们保留源 url/cdn_url」),`finish.rs:343`(theatre.styles[*].url)与 `:371`(audio[*].url)两处改用它;`is_lossy`(report.rs:88-93)豁免集合从 `{RemintedId}` 扩为 `{RemintedId, ReuploadedOnImport}`。`DroppedProperty` 只留真损失(KN 顶层键、变量样式图标),因此**反向** strict 测试(`reverse_tests.rs:1080-1086`)仍按原样失败,行为不变。`TranslateWarning` 是公开枚举,新增变体对下游穷举匹配是破坏性变更(0.1.0 窗口内可接受),README:185 的 strict 说明同步为「只挡真损失」。
   → 风险:改变正向 `strict` 的接受度(此前必失败,此后可用)—— 属修正,需在文档注明。
   → 验证:新增正向 `strict` 测试(含 `data:` 造型的真作品片段)。

9. **`detect_editor` 与文案对齐**(C4)→ 增加 `Kitten2` 判定(有 `blocksXML` 且顶层无 `size`),错误文案按实际类型给(Kitten2/3 不支持)。
   → 风险:低(仅影响报错文案与 `EditorType` 值)。→ 验证:单测覆盖 Kitten2/Kitten3/Nemo/Neko 四种输入。

10. **`keep_source` 接线**(§2.3/§7-2,已定;不删)→ 官方行为是「导入 Kitten 作品时把原始 Kitten 文件字节作为 `bcm4` 重新上传,URL 写进 KN 作品的 `source` 字段」(`docs/20:127,156`)。落地分两层,保持 `translate` 子域**不碰网络**(docs/20 §6.1):
    - `translate` 侧新增纯函数入口(如 `TranslateOptions::source_url: Option<&str>`,`translate_file` 在写盘前把 `source` 注入 KN 文档顶层)——纯逻辑、可单测;
    - `convert/mod.rs::translate_work` 侧编排:当 `options.keeps_source()` 且目标为 KN 时,把第 1 步已落地的源作品文件(`staging` 里的编辑版/原件字节,`convert/mod.rs:50-52`)上传,拿 URL 交给 `translate_file`;上传或注入失败 → 记 `TranslateWarning`(非致命),继续出产物。
    → 风险:每次转发多一次原件上传(样例原件 0.7~6 MB);KN 文档多一个平台侧字段。反向不受影响(`source` 在 Kitten4 侧仍是 `DroppedProperty`,`kitten4_finish.rs:331`)。
    → 验证:单测(注入 URL 后断言产物 `source` 等于该 URL;未开启时产物无 `source` 键)+ 真机一次 Kitten4→KN 上传,确认 KN 作品详情里 `source` 指向原件。

11. **草稿名修正**(C5)→ 取源作品名(`<源名> - 转换副本`),或去掉「自检」字样;抽成纯函数以便单测。
    → 风险:用户侧名字变化。→ 验证:纯函数单测。

### P2(清理与性能)

12. **删死代码**(§2.3 全表)→ 逐项删除;`BlockDecompilerFactory` 直接换 `create_block_decompiler`;`BlockDecompilerBehavior` trait 合并进 `BlockBehavior` 固有方法;`get_child_input_name` 去掉被忽略的参数。
    → 风险:无(`pub(crate)` + 零引用)。→ 验证:编译无新警告 + 62 单测绿。

13. **可见性收紧**(C7)+ **双层再导出合并**(C8)→ 三函数降 `fn`;`convert/mod.rs` 与 `shared.rs` 只保留一条 `pub use` 链。
    → 风险:无。→ 验证:编译 + `convert::DecompilerError` 等公开路径的 doc 测试/示例仍可解析。

14. **正向查表改 `LazyLock<HashMap>`**(F1)→ 与反向统一风格;`translate_type` 等 O(1)。
    → 风险:无(值一致)。→ 验证:现有 mapping 测试 + 官方差分门。

15. **去热路径 clone**(F2)+ **crypto 瘦身**(F3)→ `route_children` 先借 `&str` 再 `mem::take`;`Cow` 只在变化时 `into_owned`;`parse_kn_entity` 对 `Vec` 直接迭代;`BCMKNDecryptor` 并入 `CryptoService`,salt 用 `Arc` 避免克隆;`neko` 的 `"bcmkn"` 改走 `file_extension` 表。
    → 风险:无/极低。→ 验证:62 单测 + 真机 `convert_live`(3.7 MB 作品)+ `compile_live`(NEKO 产物扩展名仍 `.bcmkn`)。

16. **Kitten 编译版根块/插槽规则合一**(D11,D12,§7-1 已定)→ 抽一份 `referenced_ids(&Map<String,Value>)`:**只按对象读**(`next_block.id`、`child_block[*].id`、`conditions[*].id`、`params.*.id`),与 `xml.rs:26-55` 现有形态一致;`decompiler.rs:51-57,61-67,72-78` 里那三处字符串分支删除,改为遇字符串返回 `DecompilerError::InvalidResponse`(显式失败)。同时把 `child_input_name` 抽到同一处,`xml.rs:130-141` 与 `blocks/mod.rs:25-40` 共用。
    为什么不留容错:字符串引用在编译版里 0/2 236 命中(§7-1),留着只会让"格式变了"变成**静默少积木**(`xml.rs` 现在跳过字符串 → 根块集合算错 → 产物少块且不报错);显式失败能立刻暴露格式漂移。
    → 风险:低(行为只在"格式未知"时从静默变报错)。
    → 验证:Kitten3/Kitten4 各跑一个真实作品,根块集合与槽名与改造前逐字节一致(比对 `download/compile/raw/*` 的编译版输出);**Kitten2 无本地样本**(3 个样本是 Kitten3+Kitten4),实施前补一个真机 Kitten2 作品确认形态;若 Kitten2 确为字符串引用,则把字符串分支保留在**这一份**共享助手里(而非两处)。

17. **错误语义与命名**(C2)→ 最小改动:把 `DecompilerError::Decompile` 的显示文案改为域中立(如「作品解析失败:…」),不改枚举名(公开面已定档);如需彻底,另开一轮把域级错误改名 `ConvertError`(会动公开路径,收益中等)。
    → 风险:低(文案)/中(改名)。→ 验证:文案改动不影响断言(现有测试不校验文案)。

18. **`cloudvar::EditorType` 撞名**(C9)→ 建议改名为 `CloudEditorType`(convert 的 `EditorType` 已公开定档);跨域小改动。
    → 风险:低(仓内引用可全量改)。→ 验证:编译 + 真机云变量测试。

19. **测试组织**(T1~T3)→ 生产文件里的 `#[cfg(test)]` 全部迁到 `translate/tests/{mod,forward,reverse}.rs`;摊平 `mod reverse_tests_inner`;测试落盘改为 `target/` 下唯一子目录或每次现铸临时目录。
    → 风险:无(纯搬迁)。→ 验证:测试数不变(62)+ `cargo clippy` 无新警告。

---

## 5. 迁移步骤(4 个提交,每步可独立回滚)

| 提交 | 内容 | 验收 |
| ---- | ---- | ---- |
| **S1** | **死代码清理 + 去重**(§4 P1-5~11 中不涉及文件移动的部分 + P2-12~15):删死代码、抽 XML/强转/装配助手、修 P0-1~P0-3、`keep_source` 处理、文案与分类修正 | `cargo check --all-targets` + `cargo test`(62) + `cargo clippy` 全绿;官方差分门与真机 `convert_live` 不改判 |
| **S2** | **文件合并(纯搬迁)**:按 §3.1 表平移(`git mv` + 逐段剪切);`shared/` 收缩;测试抽到 `translate/tests/` | 同上 + `wc -l` 复核目标表;行多重集核对(搬迁前后 impl 行不丢) |
| **S3** | **行为修复(单独成提交)**:P0-2(NEMO/WOOD 输出目录)+ C6(StageOrientation 语义与文档对齐)+ 补 T4 的四条覆盖缺口 | 新增测试 + `cargo test --test compile_live`(含临时打开 NEMO 用例) |
| **S4** | **文档同步**:README(目录树/模块一览/示例路径)、`docs/20` 的路径锚点、本方案的实施进度勾选 | `cargo test` + 示例路径 grep 零残留 |

每步之间不混:搬迁提交不改逻辑(S1 改逻辑但不搬文件),便于定位回归。

---

## 6. 风险与不做清单

**风险**

| 风险 | 缓解 |
| ---- | ---- |
| 合并大文件后 review 成本上升 | 目标文件都有单一主题(`mapping`/`neko`/`assembly`);§3.1 表给出每个文件的一句话职责 |
| P0-2 修复改变 NEMO/WOOD 产物落点 | 单独提交 + README/文档注明;真机跑一次 NEMO 反编译确认 |
| D11 契约统一可能改变根块集合 | 先做只读核实(§7),再以「两种引用都识别」保守实现,并用真作品前后对比 |
| 测试搬家过程中遗漏断言 | S2 要求「测试数不变 + 全绿」,并在搬迁前后各跑一次计数对比 |
| `strict` 语义调整(C3)影响到既有自动化 | 文档明确「strict 只挡真损失」;如需旧行为可加显式开关 |

**不做**

- 不引入语义 IR(`IrStmt`/`Expr` 那类)——理由见 `docs/20` §6.2;
- 不新增第三方依赖(`parking_lot`/`tempfile` 等一概不加,沿用 std);
- 不改公开路径与类型名(`convert::{EditorType, WorkId, DecompilerError}`、`translate::{TargetEditor, TranslateOptions, TranslateReport, translate_file}`),`DecompilerError` 改名留待单独评审;
- 不合并 per-editor 插件文件与 `blocks` 的块类型分派;
- 不为「对称」而把正向/反向算法强行合一个函数(它们共享助手,不共享控制流);
- 不顺手重写 `decompile/` 的算法(本轮只做搬迁 + 去重 + 死代码)。

---

## 7. 需在实施前定/核实的三件事

三条已于 2026-09-25 定论(证据见各条目),S1 按此实施。

| # | 事项 | 决定 | 证据与落地 |
| - | ---- | ---- | ---------- |
| 1 | **编译版块引用形态** | **只按内联对象读**;遇字符串显式报错,不再静默容错 | 对 `download/compile/raw/` 3 个真作品(`春风得意`=Kitten3、`几何对战-联机`/`原气骑士`=Kitten4)取样 **2 236 处**:`next_block` 100% 是内联对象,`child_block`/`conditions` 是内联对象数组,**字符串引用 0 处**。字符串 id 只存在于**编辑版**(`block_data_json` 的 `blocks`+`connections`,见 `translate/kitten.rs:143-160`),两处混用是这次"三处假设不一致"的根因。落地见 §4-16 |
| 2 | **`keep_source`** | **接线(实现),不删除**;仅 Kitten4→KN 方向生效;上传失败降级为报告告警 | 官方反混淆代码 `w.source = '' + T;`(docs/20 §3.1,原文见 `docs/20:156`)、`docs/20:127`「保留原件」、`docs/20:528` 的字段注释均确认这是官方行为;`finish.rs:39` 我们自己写了「`source` 字段不在这里」= 已知未实现。删除会砍掉一个官方式功能,保留不实现则是静默假承诺。落地见 §4-10 |
| 3 | **「官方会重传资源」算不算有损** | **不算**:新增非有损类别 `TranslateWarning::ReuploadedOnImport`,`is_lossy` 豁免它 | 现 `DroppedProperty` 混装两类:(a) 真丢(KN 顶层键 `guideUrl`/`resourceZip`/`courseMaterials`/`source` 在 Kitten4 无对应字段,`kitten4_finish.rs:315-335`);(b) 官方导入时会重新上传资源、产物 url 由平台重写,我们保留源 url(`finish.rs:343,371`)。把 (b) 并入有损会让 `strict(true)` **在任何含非 https/`data:` 造型的真作品上必失败**。落地见 §4-8 |

---

## 附录 A — 评审方法与证据

1. **三份独立分片评审**(只读,`file:line` 锚定):
   - A:`translate/{mapping,neko,kitten,blockjson}.rs` + `tables_gen.rs` 消费面;
   - B:`translate/{mod,finish,kitten4_finish,report,ids,reverse_tests}.rs` + `convert/mod.rs` + `filedata.rs`;
   - C:`convert/{shared/*,decompile/*}` + 跨域耦合面。
2. **量化自查**:逐文件 `impl/test` 行数(`#[cfg(test)]` 切分)、公开面清单、`pub use` 链、零引用符号 grep。
3. **官方源码定点核实**:云列表 shadow 行为(§2.1 P0-4)直接对 `main-vendors.9b801394.js`(sha256 `23dd6052…`)byte **5922654** 的原始代码段取证。
4. **我的复核**:`keeps_source` 零消费(全仓 grep)、staging 竞态(`convert/mod.rs:43-45,65,94-108` 三点同读)均已确认。

## 附录 B — 行数账(现状 → 目标)

| 目标文件 | 来源 impl 行 | 目标 impl 行 | 变化 |
| -------- | ------------ | ------------ | ---- |
| `convert/mod.rs` | 174 | 174 | — |
| `convert/shared.rs` | `shared/` 1 015 − 迁出(≈815) | ~200 | 收缩到跨域契约 |
| `decompile/engine.rs` | `decompile/mod 383 + context 84 + contract 61 + fetch 17 + WorkInfo ≈51` | ~596 | 4+1 → 1 |
| `decompile/config.rs` | `config 413 + shadow 151` | ~564 | 2 → 1 |
| `decompile/infra.rs` | `crypto 108 + http 61 + json 86 + FileService ≈51` | ~306 | 4 → 1 |
| `decompile/blocks.rs` | `108 + 359 + 526` | ~993 | 3 → 1 |
| `decompile/editors/mod.rs` | 36 | ~40 | — |
| `decompile/editors/kitten.rs` | `251 + 373 + 211` | ~835 | 3 → 1 |
| `decompile/editors/simple.rs` | `101 + 235 + 218` | ~554 | 3 → 1 |
| `decompile/editors/nemo.rs` | 297 | 297 | — |
| `translate/mod.rs` | `mod 437 + report 140` | ~575 | 含报告类型(去内联测试) |
| `translate/model.rs` | `blockjson 331 + ids 59` | ~390 | 2 → 1 |
| `translate/kitten.rs` | 300 | ~300 | 去内联测试 |
| `translate/neko.rs` | 910 | ~910 | 去内联测试(341) |
| `translate/mapping.rs` | 1610 | ~1610 | 去内联测试(313) |
| `translate/assembly.rs` | `finish 763 + kitten4_finish 667` | ~1430 | 2 → 1 |
| **生产合计**(不含生成物/内联测试) | **9 801** | **≈9 600(删死代码后)** | 34 → 16 文件 |
| `translate/tests/*` | 2 855 | ~2 900 | 7 处内联 + 1 外置 → 3 文件 |

---

## 8. 实施记录(2026-09-25)

### 8.1 S1 死代码 + P0 + 去重 + 一致性 + 性能(5 提交,已完成)

| 提交 | 内容 | 验证 |
| ---- | ---- | ---- |
| `ff887d1` S1.1 | 删零引用代码块:blockjson 6 方法 + 不可达兜底、`EditorType` 5 谓词、`WorkInfo` 3 字段、`ValueExt` 6 方法、`BlockBehavior`/`BlockDecompilerBehavior`/`BlockDecompilerFactory`、`ParsedEntity.comments`、`client_secret`、`file_service` 注入、恒 `Ok` 的 `Result`、反向未读的 `ids` 参数;D12 插槽命名合一 | 62 单测 |
| `917fa8e` S1.2 | P0-1 staging 竞态(每作品每次调用独立子目录 + 失败也清理);P0-3 删掉 `neko::attr_value` 弱化版改用 `mapping::xml_attr_value`;P0-4 云列表不对称补注释;两个回归测试 | 64 单测 + convert_live/compile_live |
| `7b1c77c` S1.3 | 去重:`num`/`truthy`/`project_name_at`/`XHTML`/`math_number_node` 收敛为唯一实现;D11 `referenced_ids` 合一并改**对象-only + 遇字符串显式报错** | 64 单测;真样本 462 块上新旧集合逐一相等、插槽名与旧枚举全组合相同 |
| `c4d22f7` S1.4 | C3 新增 `ReuploadedOnImport`(非有损,`is_lossy` 豁免);`keep_source` 接线(官方「保留原件」);草稿名修正;`detect_editor` 说明;C7 可见性收紧;C10 死回退;C2 文案中立 | 66 单测 + 真机离线门 |
| `d28f872` S1.5b | F2 去热路径分配;F3 `BCMKNDecryptor` 并入 `CryptoService` + salt 用 `Arc` + NEKO 扩展名走表;C9 `cloudvar::EditorType` → `CloudEditorType`;T3 测试落盘目录唯一化 | 66 单测 + 三个真机门 |
| `caf5daa` S1.5a | F1 正向查表改 `LazyLock<HashMap>`(6 个索引,`or_insert` = 首个命中) | 全表键逐一等价;真夹具正向管线 1.739ms → 1.487ms/轮(-15%) |

### 8.2 S2 文件合并:34 → **18** 个生产文件(已完成主体)

| 提交 | 合并 | 结果 |
| ---- | ---- | ---- |
| `3cad268` | `report.rs` → `translate/mod.rs` | -1 |
| `e70b06b` | `blockjson.rs` + `ids.rs` → `translate/model.rs` | -1 |
| `9858b8b` | `finish.rs` + `kitten4_finish.rs` → `translate/assembly.rs` | -1 |
| D7/D8 | 主题表合一为 `VAR_STYLE_TABLE`;坐标公式不合并、同文件相邻 + 互逆说明 | — |
| S2.2a | `{context,contract}.rs` → `decompile/mod.rs` | -2 |
| S2.2b | `blocks/{mod,core,special}` → `decompile/blocks.rs` | -2 |
| S2.2c | `editors/{coco,neko,wood}` → `editors/simple.rs`;`editors/kitten/{mod,decompiler,xml}` → `editors/kitten.rs` | -4 |
| S2.2d | `shared/` 9 → 5(`infra.rs` 收 crypto/http/json/FileService;`model.rs` 收 fetch/IdGenerator;`config.rs` 收 shadow) | -4 |

最终:`convert/mod.rs`、`convert/shared/{mod,error,model,config,infra}.rs`、`convert/decompile/{mod,blocks,editors/{mod,kitten,nemo,simple}}.rs`、`convert/translate/{mod,model,kitten,neko,mapping,assembly}.rs` = **18 个生产文件**(+生成物 `tables_gen.rs`、测试 `reverse_tests.rs`)。

### 8.3 执行中与方案的偏差(按证据调整)

| 项 | 方案 | 实际 | 理由 |
| -- | ---- | ---- | ---- |
| D2 强转族 | 合一 `truthy/as_text/as_number/value_key/field_text` | 只合一 `truthy`(+`num`) | 实测 `js_text`/`text`/`field_text` 对 `undefined`/`null`/数字 0 的处理**本来就不同**(各自照抄官方不同分支),合并会改行为 |
| D5~D9 | 全部合一 | D5/D6/D7 合一,D8 改为同文件对照 | 坐标两式参数形态与 `num()` 取整口径不同,强行抽象掩盖差异(与「不为对称而合并」一致) |
| D11 | 字符串与对象都识别 | **对象-only** + 字符串显式报错 | 真样本 462 块 4 256 处引用无一是字符串(字符串 id 属编辑版 `connections`);静默容错会让格式漂移变成"少积木不报错" |
| C4 `detect_editor` | 增加 Kitten2 判据 | 只补说明,不加判据 | 本地无 Kitten2 样本、`docs/20` 无区分标记,且 2/3 都不支持转化;不编造判据 |
| C8 双 `pub use` 链 | 合并为一条 | 保持两层(内部再导出 + 对外门面) | 11 个文件按 `shared::{...}` 取用;合并只会让导入更啰嗦,两层分工明确已加注释 |
| keep_source | `TranslateOptions` 加字段、装配时写 `source` | `convert` 编排上传 + `translate::set_source_reference` 纯函数 | 避免"先上传再转化"的失败浪费,并保持 `translate` 不碰网络(分层纪律) |
| S2.3 测试抽取 | 内联测试抽到 `translate/tests/*` | **不做** | 被抽测试大量访问**私有项**,搬出文件必须放宽可见性(与 C7 直接冲突);Rust 惯例本就是同文件 `#[cfg(test)]`。改为规则:测试模块一律放文件末尾(已对 `assembly.rs` 执行,消除 clippy `items_after_test_module`) |
| `decompile/engine.rs` | mod+context+contract 合成 engine.rs | 并入 `decompile/mod.rs` | 少一层门面文件,引用改写面更小,文件数收益相同 |
| `shared.rs` 单文件 | shared → 1 文件 | shared 保留 5 文件 | config(565 行大表)/infra(工具)/model(模型)关注点差异大,合一个文件变杂物箱 |

### 8.4 新发现(本轮记录,未修)

| # | 发现 | 证据 | 影响 |
| - | ---- | ---- | ---- |
| N1 | **约 9 MB 作品的 CDN 上传在全局 30s 超时下必失败**,源文件与产物上传同样超时 | 探针:`上传失败 31.2s → Http(Timeout(Global))` / `35.5s`;`requests.rs:272` 全局 30s | `translate_work(upload=true)` 在慢网+大作品下不可用;`keep_source` 的源文件上传也受影响(失败降级为日志,不影响产物)。建议另开一轮:上传走独立超时或分片 |
| N2 | `keep_source` 上传的是**反编译重建的编辑版**,官方上传原始文件字节 | `docs/20:156`;`convert/mod.rs::attach_source_reference` 注释 | 平台的「保留原件」若要能回打开原件,需拿到作品原始文件字节(当前管线不保留) |

### 8.5 剩余

- **S3.2** `StageOrientation` 语义与文档对齐(C6:`kitten4_position` 实际按画布缩放坐标、`kitten4_variables` 的 `let _ = landscape`);
- **S3.3** 补覆盖缺口:正向 `strict`、显式 `StageOrientation`、`translate_works` 并发、NEMO/WOOD 自定义输出目录(最后一条已随 S3.1 真机覆盖);
- **S4** README/docs/20 的路径锚点同步(本轮改了文件结构:README 未涉及 convert 文件路径,`docs/20` 的目录树段落已过时);
- N1/N2 建议单开一轮。
