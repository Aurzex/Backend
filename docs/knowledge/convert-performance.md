# 转换与反编译性能(实测基线)

> 知识库条目:**已经量到的数字、瓶颈归因、已落地优化及其收益、判定不做的项**。
> 方案与执行记录见 `../rounds/22-*`(NEMO 反编译)、`../rounds/23-*`(转换总体)、`../rounds/25-*`(实体级并行)、`../rounds/29-*`(扫描台账)。

## 1. 基线(实测)

| 场景 | 优化前 | 现在 |
| --- | --- | --- |
| NEMO 作品反编译(含资源下载) | **2m42s** | **13s**(并发 + `skip_resources`) |
| KN 作品反编译 | 46s(待核验) | **1.5s** |
| Kitten4 作品反编译 | 单块 `from_value` | **1.5s**(逐块克隆改掉) |
| 转换 Kitten4->KN(离线、确定性) | — | **38 ms**(374 输入积木 -> 439 产物节点) |
| 转换 NEMO->KN(离线、确定性) | — | **core 365–442 ms / e2e 466–556 ms**(3.5 MB 源 -> 7.7 MB 产物;15 482 源元素 -> 13 637 产物节点) |
| 转换 NEMO->KN(老版本 0.11.0,**含 YC 迁移**) | — | **core 108–121 ms / e2e 142–152 ms**(1.5 MB 源 -> 2.4 MB 产物;4 862 源元素 -> 4 421 产物节点) |
| 转换(官方 JS 同输入参照) | 355 ms(Node) | — |
| 资源删除(批量) | 7.9s | **5.3s**(并行 delete) |

> KN 行的**机制待核验**(2026-10-05 复核):`46s` 这个读数的测量方法与日志**不在仓内**(`git log -S'46 s'` 在 `docs/` 零命中,该行由三库重构时写就),原先附的"模式分派"因此**没有证据支撑**。可核验的部分:当时与现在的 KN 路径形状相同 —— 详情 GET -> 取 `source_urls[0]` -> GET -> reversed-base64 + AES-256-GCM 解密 -> 一次 `serde_json::from_str` -> 落盘(对照 `338fb8f^:src/core/decoders.rs` 的 `NekoFetcher`/`NekoDecompiler` 与今 `src/core/convert/decompile/editors.rs::NekoDecompiler`),即 KN 的**反编译本体从来不是 CPU 热点**。`[INFERENCE]` 46s 更可能出在当时的请求侧(每次取件重建 `CloudAuthenticator` 白付一次串行 `currentTime` RTT,见 `../rounds/29` 第 2 条;该条已改为进程级缓存)。坐实办法:今日按同一作品复量(`cargo test --test compile_live`),旧值需翻回旧提交再量。

## 2. 瓶颈归因(按证据)

1. **请求数 × RTT**,不是 CPU:资源下载耗时几乎全部在这里。实测曲线(同一 NEMO 作品):

   | 并发 | 1 | 8 | 16 | 32 |
| --- | --- | --- | --- | --- |
   | 耗时 | 402 s | 103 s | **92 s** | 127 s + **CDN 限流丢 2 个文件** |

   => 最优区间 8–16。下载是 I/O 密集,**不能用 `available_parallelism` 折算**(低核机器会折到近串行、高核机器会放宽到限流区)。本库把"作品级 × 单作品资源级"总线程**封顶 16**(常量 `RESOURCE_DOWNLOAD_BUDGET`)。
2. **`locate_resource` 在 hot path 上占比最大**(转换剖析里约 2/3)。
3. **NEMO 逐条 XML 解析**:3.5 MB 在 Node 下要 4–5 分钟(逐条 `DOMParser`);Rust 侧按"单文件秒级"设计。
4. **整份文档 JSON 三进三出**:`translate_file` 先 `read_to_string + from_str`,产物再 `to_string + write`;`translate_work` 还多一轮"落盘->读回"。

## 2bis. 热点归因(2026-10-05 剖析读数,单机)

> 方法:`cargo test --profile bench_perf --test convert_bench -- --ignored --nocapture`(6 样本、`RUNS = 5` 取最小),
> 以 `CARGO_PROFILE_BENCH_PERF_DEBUG=1` + `-C force-frame-pointers=yes` 另建一份带符号的二进制放 `target/prof/`,
> `taskset -c 0-3 perf record -F 199 -g --call-graph fp` 采样本进程(含测试框架),平坦热点按 `perf report --sort dso,symbol`。
> 机器:i5-5200U(2 物理核 / 4 逻辑核)、glibc 默认分配器。**跨机不可比**,只作方向判据;要当门须先建基线。

### 2bis.1 阶段耗时(本轮中位,ms)

| 样本 | read | parse | core | ser | e2e | e2e 未归类 |
| --- | --- | --- | --- | --- | --- | --- |
| kitten4-10.8 | 7 | 101 | 292 | 87 | 714 | 227(32%) |
| kitten4-0.3 | 0 | 2 | 7 | 2 | 19 | 8(42%) |
| kn-9.4 | 5 | 35 | 191 | 45 | 330 | 54(16%) |
| kn-3.7 | 2 | 7 | 59 | 13 | 101 | 20(20%) |
| nemo-3.4 | 2 | 12 | 282 | 55 | 370 | 19(5%) |
| nemo-old-1.5 | 1 | 6 | 90 | 15 | 117 | 5(4%) |

### 2bis.2 分配(窗口 = 一轮 `translate_file`;`#meta` 的串行腿读数)

| 样本 | 产物节点 | 分配次数 | 分配 MiB | 次/节点 | 平均 B/次 | 分配 MiB/源 MiB |
| --- | --- | --- | --- | --- | --- | --- |
| kitten4-10.8 | 14 024 | 2 125 796 | 309.4 | 152 | 153 | 29.9 |
| kitten4-0.3 | 439 | 68 665 | 9.9 | 156 | 152 | 32.3 |
| kn-9.4 | 4 624 | 992 187 | 183.7 | 215 | 194 | 20.6 |
| kn-3.7 | 1 597 | 263 559 | 61.5 | 166 | 244 | 17.4 |
| nemo-3.4 | 13 637 | 1 626 347 | 197.7 | 119 | 127 | 58.7 |
| nemo-old-1.5 | 4 421 | 505 321 | 65.1 | 114 | 135 | 46.3 |

### 2bis.3 进程级 CPU 归属(self 占比)

| 归类 | 占比 |
| --- | --- |
| libc(分配/释放及其内部,含未解析地址簇) | 36.8% |
| 其它/未分类(std/core 粘合、`from_utf8`、`StrSearcher`、clone、sip 哈希、`Value::index_into` 等) | 36.1% |
| `serde_json` 反序列化(`Value` 树) | 9.2% |
| `Value`/`BTreeMap` 析构(`drop_glue`) | 9.6% |
| `BTreeMap<String, Value>` 插入与遍历 | 8.4% |
| `serde_json` 序列化与转义(`format_escaped_str`) | 8.4% |
| `sha2` | 8.6% |
| 本库 `backend::core::convert` 逻辑合计 | 8.5% |

- **`sha2` 那 8.6% 全部落在测试框架的 `convert_bench::sha256_hex` 子树里**(校验和),不是库的热点;库内 sha256 只在 `.bcmkn` 解密链上。
- **本库逻辑没有单点热点**:最高的单符号 0.91%(还是 `Map` 反序列化),业务函数都在 0.5% 以下(`normalize_integral_numbers` 0.44、`parse_fragment` 0.37、`transform_shadow_xml` 0.35、`remap_node`/`build_node`/`parse_name` 各 0.30、`fill_shield` 0.24)。
- `perf stat`:task-clock 31.7 s、**IPC 1.30**、分支失误 1.28%、cache-misses 508.8 M、user 29.4 s / sys 1.5 s ⇒ 瓶颈是**内存表示层的访存与分配**,不是算法分支。

### 2bis.4 分配器 A/B(只换分配器,产物零变化)

同二进制、同输入,`LD_PRELOAD` 换进程分配器,3 轮取中位;`core`/`e2e` 单位 ms;每一轮产物 SHA256 与 `#meta` 均与基线一致。

| 样本 | glibc `core`/`e2e` | jemalloc `core`/`e2e` | tcmalloc `core`/`e2e` |
| --- | --- | --- | --- |
| kitten4-10.8 | 292 / 714 | 252 / 592(−13.7% / −17.1%) | 248 / 610(−15.1% / −14.6%) |
| kn-9.4 | 191 / 330 | 151 / 266(−20.9% / −19.4%) | 169 / 297(−11.5% / −10.0%) |
| kn-3.7 | 59 / 101 | 49 / 83(−16.9% / −17.8%) | 56 / 97(−5.1% / −4.0%) |
| nemo-3.4 | 282 / 370 | 218 / 289(−22.7% / −21.9%) | 223 / 301(−20.9% / −18.6%) |
| nemo-old-1.5 | 90 / 117 | 67 / 93(−25.6% / −20.5%) | 73 / 100(−18.9% / −14.5%) |
| kitten4-0.3 | 7 / 19 | 6 / 17(−14% / −10.5%) | 6 / 17 |
| 整套 test 墙钟 | 28.86 s | 24.40 s(−15.5%) | 25.37 s(−12.1%) |

### 2bis.5 由读数得到的判断

1. **转换的时间主要花在"数据表示层",不是转换算法**:`Value` 树的解析/析构 + `BTreeMap<String, Value>` 操作 + 序列化转义合计约 35%,再加上 libc 那 36.8%(其大头就是这些短命小对象的分配与释放)。产物节点摊到 **114–215 次分配 / 节点、平均 127–244 B / 次**,分配字节是源文件的 **17–59 倍**。
2. **因此"少建中间 `Value` 树"(应自写序列化那条大改)的方向得到平坦热点支持**;也解释了 P6(只去掉 `#[serde(flatten)]`)为何 0 收益:它只动这条链上的一小块。
3. **最便宜的可省资源是分配器本身**:换 jemalloc/tcmalloc 就能拿到 14%–26%(`core`)与 12%–16%(整套墙钟)且产物逐字节不变 —— 代价是引入全局分配器(库内选择 = 产品决策;消费者自行指定亦可)。
4. **并发不是这些样本的瓶颈**:正向实体级 `core` 1.44–1.56×、`e2e` 1.12–1.17×;反向与 NEMO 均 1.00×(与 `rounds/25` 的 Amdahl 结论一致)。
5. **`e2e` 未归类占比**在 kitten4-10.8MB 高达 32%(227 ms),而 NEMO 只有 4–5% —— [INFERENCE] 前者对应"巨型 `Value` 文档的物化与析构 + 写盘",与 2bis.3 里 `drop_glue`/`BTreeMap` 析构的读数一致;要坐实需在 `translate_file` 内部再加一次快照(本轮未做)。

### 2bis.6 构建档位:`opt-level` 的代价(2026-10-05 实测;**已采纳,发布档改为 3**)

> **结论已落地(2026-10-05,用户拍板)**:`[profile.release]` 的 `opt-level` 由 `"z"` 改为 `3`(`Cargo.toml`);
> 该档只作用于**本仓作为顶层**的构建,下游 rlib 消费者仍用自己的档,因此不构成本库的对外变更。
> `bench_perf` 档保持自身设置不变(关 LTO、`unwind`),历史读数可比。

同一棵树、同一输入,只改 `opt-level`(两者都 `lto = true`、`panic` 覆写为 `unwind` 以便跑测试),3 轮取最小:

| 样本 | `"z"`(现行发布档)`core`/`e2e` | `3`(bench_perf 档)`core`/`e2e` | 差 |
| --- | --- | --- | --- |
| kitten4-10.8 | 361 / 843 | 282 / 684 | −21.9% / −18.9% |
| kn-9.4 | 223 / 406 | 185 / 321 | −17.0% / −20.9% |
| kn-3.7 | 64 / 123 | 56 / 98 | −12.5% / −20.3% |
| nemo-3.4 | 376 / 490 | 280 / 361 | −25.5% / −26.3% |
| nemo-old-1.5 | 123 / 163 | 88 / 113 | −28.5% / −30.7% |
| 整套 test 墙钟 | 34.59 s | 27.82 s | −19.6% |
| `backend` bin 体积 | 2.20 MiB | 2.90 MiB | +32% |

- 命令:`CARGO_PROFILE_RELEASE_PANIC=unwind [CARGO_PROFILE_RELEASE_OPT_LEVEL=3] CARGO_TARGET_DIR=target/rel… cargo test --profile release --test convert_bench --no-run`(profile 覆盖用环境变量,不改 `Cargo.toml`)。
- 适用范围:`[profile.*]` 只对**本仓作为顶层**的构建生效(`cargo build` / `cargo test` / 把本仓当顶层);rlib 被下游消费时用的是**下游自己的 profile**。故该差值是"本仓自建产物"的代价,不是下游必然承担的。
- `bench_perf` 档本来就是 `opt-level = 3` ⇒ 本文件其余读数都代表 **3 档**。

依据:2026-10-05 的剖析会话(命令与读数见本节方法段;`#meta` 分配读数同时落 `tests/fixtures/translate/convert_bench_baseline.json`)。

### 2bis.7 去掉 `json!` 深拷贝后的读数(2026-10-05,Step 1 落地)

`json!(expr)` 展开成 `serde_json::to_value(&expr)`(见 serde_json `macros.rs`),因此把**已经构造好的 `Value`** 塞进 `json!({ "k": v })` 会把整份子树按 `Serialize` 重新物化一次。装配期有 6+ 处这种写法(正向 `assembly::build_document` 的 `actors`/`scenes`/`procedures`/`styles`/`variables`/`audios`、`model::procedures_to_json`、反向 `model::build_block_data_json`、NEMO `convert_nemo_document` 的各段),改成"直接搬所有权"(`shared::json_obj`)后:

| 样本 | `core` 前 → 后 | `e2e` 前 → 后 | 分配次数 前 → 后 |
| --- | --- | --- | --- |
| kitten4-10.8MB | 292 → 277 | 697 → 545(−21.8%) | 2 125 796 → 1 538 778(−27.6%) |
| kn-9.4MB | 189 → 125(−33.9%) | 330 → 265 | 995 789 → 798 341 |
| nemo-3.4MB | 281 → 202(−28.1%) | 360 → 282 | 1 626 347 → 1 353 316 |

**耐久的读数与结论**:正向的 `core`(取 `report.elapsed_ms`)只降约 5%,而 `e2e` 降 22% ⇒ §2bis.5 第 5 条那条推断成立:**`e2e` 里"未归类"的那一段(原 227 ms / 32%)就是产物物化与深拷贝**,现降到约 71 ms。也就是说**"性能优化只盯着 `core` 会看漏装配与写出这一整段"**,后续读数必须同时报 `e2e`。产物 SHA 与 `#meta` 全绿(逐字节不变)。

### 2bis.8 源侧 `block_data_json` 的读数与归因(2026-10-05,Step 5 落地)

装置:同一份 10.3 MiB 样本(209 个实体,`bdj` 原文合计 9.1 MiB),`--profile bench_perf`,3 轮取最小,窗口 = 一次 `from_str`(全局计数分配器)。探针跑完即删。

| 项 | ms | 分配次数 | 分配字节 |
| --- | --- | --- | --- |
| 整份文档 → `Value` | 110.1 | 555 517 | 61.3 MiB |
| 骨架读法(`block_data_json` 记为 `IgnoredAny`,只跳过) | 21.4 | 29 302 | 4.6 MiB |
| 骨架读法(`block_data_json` 记为 `Box<RawValue>`,捕获原文) | 18.7 | 454 | 9.9 MiB |
| `bdj` 本体 → `Value` | 136.0 | 532 116 | 58.8 MiB |
| `bdj` 本体 → 强类型(**带 `#[serde(flatten)]`**) | 155.5 | 596 110 | 91.4 MiB |
| `bdj` 本体 → 强类型(**手写 `Deserialize`,无 `flatten`**) | 136.1 | 531 907 | 59.5 MiB |
| `bdj` 本体 → 仅扫描(`IgnoredAny`) | 14.3 | 209 | — |

由这份表得到三条**耐久结论**:

1. **`#[serde(flatten)]` 是本域最大的单点税**:同一形状的强类型结构与手写 `Deserialize`(未知键直接进 `Map`)相比,`flatten` 版多 12% 分配、14% 时间、54% 分配字节。原因是派生实现必须把**整个结构**先缓冲成 `Content` 再逐字段转换(`FlatMapDeserializer` 口径)——`BlockJson` 在 `flatten extra: Map<String, Value>` 上正好踩中。凡是要对源/产物积木做"强类型解析"的地方,这条都成立。
   **引用本条时必须带上端到端尺度**:这一点换算到整条流水线只有约 −5% 分配,低于同轮 A/B 的可测门 —— 2026-09-26 已按此规模试过并回退(`../rounds/37` §10.6,P6:去 `flatten` + 手写 `from_value`/`to_value`,产物逐字节等价、三轮交替量不到收益),本次探针不翻该结论。要真吃这一块,须与"少建中间 `Value` 树"的大改同批做并自带 A/B。
2. **"省掉 `Value` 中间树"≠"省掉整份成本"**:`bdj` 直接流式强类型解析(596 110 次)比先建 `Value`(532 116 次)还贵 —— 中间树省掉的那一趟,被"文本 → 强类型"的流式解析又付了一遍(同一份 `flatten` 税)。**Step 5 的净收益来自省掉 `Value → BlockJson` 的 `from_value` 那一趟**(实测约 −209 700 次分配),不是省掉 `Value` 本身。故 Step 5 之后再继续"绕开 `Value`"已无剩余空间;剩下的那一处(`flatten`)按其附注不立项。
3. **原文捕获本身很便宜,但骨架必须手写 `Deserialize` 才拿得到这个数**:捕获 9.1 MiB `bdj` 原文只花 454 次分配 / 9.9 MiB 字节(≈一次 9.9 MiB 复制);而探针里带 `#[serde(flatten)] rest: Map<String, Value>` 的骨架版要 29 302 次分配 —— `flatten` 会把整份文档重新缓冲一遍,等于把刚省下的中间树又建回来(故上表前两行不可直接相减,差异里含 `flatten` 那一项)。落地实现(`translate/source.rs`)因此对"文档/theatre/实体"三层各写一个流式 `Visitor`。

**隐藏契约(必须遵守)**:`RawValue` 只做**解析入口**,绝不透传进产物 —— 源语料不是紧凑 JSON(实测 `download/compile/k4edit/174408420-0.bcm4` 含 14 580 个空白字符),透传会改产物字节;产物一律由同一套序列化器重新写出。另:`Box<RawValue>` 只能在 `serde_json::from_str` 这类**文本**反序列化器上捕获;对 `Value` 反序列化它时 serde_json 会走 `OwnedRawDeserializer { raw_value: Some(self.to_string()) }`(先把子树重新序列化成 `String`),此时待省的 `Value` 早已建好,净收益为零。

**快速通道必须按格式设闸(同轮 A/B 实测的回归)**:`translate_file` 的骨架尝试若对所有格式无条件执行,KN/NEMO 文档会白付一次全量骨架解析 —— `nemo-3.4MB` 的 `e2e` +26 ms、分配 +50 638 次(+3.7%)。落地实现改为先做一次 `text.contains("\"block_data_json\"")` 字节扫描(定长 ASCII 键,JSON 里不可能有别的写法),命中才试骨架。**这类"按格式分流的快速通道"必须自带格式判据,否则非目标格式全额负担新开销。**

### 2bis.9 产物侧流式写出后的读数(2026-10-05,Step 2/3 落地)

同一轮 A/B(`--profile bench_perf`,`git worktree` 双树交替,A = `d937cda`):

| 样本 / 指标 | A(改动前) | B(流式产物) |
| --- | --- | --- |
| kitten4-10.8MB `e2e` | 541 ms | **421 ms(−22%)** |
| kitten4-10.8MB `core` | 373 ms | **257 ms(−31%)** |
| kitten4-10.8MB 分配次数 | 1 329 054 | **909 987(−31.5%)** |
| kitten4-10.8MB 分配字节 | 231.2 MiB | **184.6 MiB(−20%)** |
| kitten4-0.3MB `e2e` / 分配 | 15 ms / 42 277 | **9 ms / 28 864** |
| kitten4-10.8MB 并发 8 `e2e` / `core` | 417 / 260 ms | **316 / 156 ms** |
| kn-9.4 / kn-3.7 / nemo / nemo-old 分配 | — | **逐位不变**(本步只改正向) |

**三条耐久结论**:

1. **产物侧最大的分配来源是"每块按 `Serialize` 重新物化一份 `Map<String, Value>`"**(`BlockJson::to_value` + `fill_shield`),而不是"序列化"本身:把这一层换成按引用直写后,分配次数 −31.5%、`e2e` −22%;而 §2bis.8 的实测显示"序列化到 sink"那份只有 36 ms / 1 次分配。**省的是物化,不是编码。**
2. **"先试一次引用式改写"与"根本不做那份 `Value`"是两回事**:早先那版只把 `tree_to_json` 换成引用式写法,但因为它的**返回类型仍是 `Value`**,块 `Value` 照造不误 ⇒ 同轮 A/B 时间中性、分配 +1.5%(回退)。本步改成"文档级流式 + 块树按引用直写"才吃到 31.5%。判据:**看新代码有没有真正删掉那个类型**,而不是看它写得更快。
3. **键序等价是可以工程化的**:块写出的字节等价靠三条口径撑住 —— 键序 = `serde_json::Map`(= `BTreeMap`)的字节序、`shield` 恒写(旧路径靠 `fill_shield` 补)、`extra` 与已知键**撞名时 `extra` 胜**(旧路径是"已知字段先写、flatten 后写覆盖")。三条都必须有常驻等价测试(本次三条:块写出、空/最小树、三处挂点的整份文档),否则产物门只在整条管线上抓、抓不到单元级错位。

### 2bis.10 反向(KN → Kitten4)产物侧流式写出后的读数(2026-10-05,Step 4 反向落地)

同一轮 A/B(A = `8be59cf`):

| 样本 / 指标 | A | B |
| --- | --- | --- |
| kn-9.4MB `e2e` / `core` | 280 / 133 ms | **237 / 94 ms(−15.4% / −29%)** |
| kn-9.4MB 分配(次数 / 字节) | 798 343 / 156.9 MiB | **602 799 / 141.0 MiB(−24.5%)** |
| kn-3.7MB `e2e` / `core` / 分配 | 81 / 34 ms / 196 004 | **67 / 26 ms / 141 085(−17% / −24% / −28%)** |
| 正向两样本 / NEMO 两样本 分配 | — | **逐位不变** |

**三条结论**(与 §2bis.9 同族,但反向的产物形态不同):

1. **"同一套做法"要按产物形态各写一份**:反向的块表是**邻接表**(`{blocks, connections}`)而不是数组,默认键策略也不同(Kitten4 每块补 12 个缺省键、`parent_id` 强制、`shield` 不恒写)⇒ 复用不了正向的写出器;但**方法论完全可复用**(快照现状 → 按引用直写 → 常驻字节等价门 → SHA 门 → 同轮 A/B),本次两次都一次到位。
2. **流式化的真正障碍往往是"夹在中间的那道 `Value` 改造"**:本步卡住的不是写出器,而是装配期的 `mark_unknown_blocks`(把编辑器不认识的积木改成 `incompatible_*` 标记)——它在 `Value` 上做,而流式路径没有那份 `Value`。移植到 typed 形态后反而更直白(kind/fields/shadows/mutation 与 connections 都是现成字段),但**必须逐条对齐旧口径的时序**:旧流程是"先补缺省键 → 再由标记删掉 `fields`/`shadows`",移植版若照抄"标记块也补缺省",产物就多出两个空键 —— 这条由常驻等价门当场抓出(单测红),否则会一路走到 SHA 门才发现。
3. **共用一个变换是硬要求**:移植后 `mark_unknown_blocks_encoded` 是内存路径与文件路径**唯一**的实现(旧 `Value` 版已删),不存在"两条路径对同一输入行为不同"的分叉。

## 3. 已落地的优化(都有数字)

| 优化 | 做法 | 收益 |
| --- | --- | --- |
| 资源下载并发 | 分块并发,总线程封顶 16 | 4× 级(402 -> 92 s) |
| `skip_resources` | 只要积木/结构时跳过资源下载 | 53× 级 |
| 内存直通 | `DecompiledArtifact` 直接交给 translate,避免落盘->读回 | 省一次 serialize + 一次 parse(10 MB 级 ≈0.3–1.0 s) |
| 去逐块克隆 | `BlockJson::from_value` 不再 `obj.clone()` | 12 764 块受益 |
| **实体级并行(正向)** | 临时 id + 串行兑现/改写;`entity_concurrency` 默认 1 | 10.8 MB:`core` 285 -> 184 ms(**1.55×**)、`e2e` 696 -> 590 ms(1.18×);**并发 1 与并发 N 产物 SHA256 相同** |
| 两级预算折算 | `entity_concurrency = min(请求, 可用核数 / 有效作品并发)` | 防止 `batch × entity` 超订;只改并行度不碰产物 |
| 反编译侧并发折算 | `decompile_batch` 作品级 × 资源级总线程封顶 16 | 见 §2 |
| **源侧骨架 + `RawValue` 直喂** | 文件入口先按骨架读(`bdj` 留原文),`BlockData::Raw` 直接反序列化成强类型树,省掉 `Value → BlockJson` 那一趟;旧形态自动回落 | 10.8 MB:`e2e` 595/580 → 546/545 ms(同轮 A/B),分配 1 538 778 → 1 329 054(−13.6%);0.3 MB:分配 48 436 → 42 277;**产物 SHA256 全绿**;归因见 §2bis.8 |
| **产物流式写出(正向)** | 块树按引用直写成 JSON(`model::write_block_tree`,不建中间 `Value`)+ `assembly::ProductDocument` 三处块表挂点;公开面 `TranslateDocument.document: Value` 不变(内存路径仍 `into_value()`) | 10.8 MB:`e2e` 541 → **421 ms(−22%)**、`core` 373 → 257 ms、分配 1 329 054 → **909 987(−31.5%)**;0.3 MB:`e2e` 15 → 9 ms;反向/NEMO 逐位不变;**产物 SHA256 全绿**;读数与结论见 §2bis.9 |
| **产物流式写出(反向)** | 邻接表形态的字节等价写出(`model::write_encoded_blocks`)+ `assembly::Kitten4ProductDocument` 两处挂点;`mark_unknown_blocks` 从 `Value` 形态移植到 typed 形态(两条路径共用) | kn-9.4MB:`e2e` 280 → **237 ms(−15.4%)**、`core` 133 → 94 ms、分配 798 343 → **602 799(−24.5%)**;kn-3.7MB:`e2e` 81 → 67 ms、分配 196 004 → 141 085;**产物 SHA256 全绿**;读数与结论见 §2bis.10 |

## 4. 判定**不做**(有证据)

| 项 | 结论 | 理由 |
| --- | --- | --- |
| 反向(KN->Kitten4)实体级并行 | **不做** | 63 个工作项、最大一项占 **36.7%**,且两段必须串行(`unrewrite_calls` 依赖全局 `call_targets`、`def_root_from_entry` 把定义根挂进宿主实体)=> Amdahl 上限 1.9×,**实际远低于 1.5×**,而反向 `core` 只有 ~200 ms |
| `RawValue` 顶层只透传 | **不做** | 透传占比 ≈0%(见 `convert-semantics.md` §7) |
| 单遍遍历合并 | **不做** | 只省遍历,不省逐块匹配/字段改写 |

## 5. 基准方法(避免自欺)

- **绑核**:`taskset -c 0-3`,**5 轮取最小**(本机 2 物理核 / 4 逻辑核);报告要区分 `core` 与 `e2e`。
- **字节门**:并发优化必须证"并发 1 与并发 N 产物 SHA256 相同",再谈加速比。
  样本现为 **6 个**(4 Kitten + 2 NEMO);NEMO 那两件的 `#meta` 带 `source_version`
  (`0.11.0` 那件 < 0.15.0 => **YC 版本迁移真的进基线**;`0.16.2` 那件等于迁移目标版本,迁移是 no-op)。
- **`#meta` 与产物 SHA 是两类不同性质的信号**(判断"回归 vs 夹具问题"就看这个):
  - `#meta` 不一致 = **输入夹具被换 / 元信息漂了**(`source_sha256`/`source_bytes`/块数/告警数);
  - **产物 SHA 不一致 = 行为/产物变了**(真正的回归)。
  两者都不通过时还要能**同时看到**:**`tests/convert_bench.rs` 的断言顺序曾把产物 SHA 的红遮住**
  (`#meta` 先 panic)—— 已修(`636127f`):比较抽成纯函数一次返回**全部**不一致项、两类**分开标注**、
  尾部单次 panic 报两类计数与逐项(样本名 -> 键 -> 基线 -> 现在);2026-10-02 的一次误判就是这么来的。
  见 `../rounds/40-gates-cleanup-and-real-defects.md` §8.3.1/§8.6。
- **两种门要分清(一遍知就够)**:
  - **可复现门**:样本来自可控采集(`download/compile/**` 等)=> 产物 SHA 原则上可复现,变了就是行为变了;
  - **快照冻结门**:`kn-9.4MB` 的产物 SHA **是输入快照的函数** —— 夹具里的实体/积木 id 是**随机 UUID**
    (未开 `deterministic_ids`),而反向会把源实体 id 带进产物 => **重新采集(或再跑一次产它的真机门)
    就必变 SHA**,必须再走一次**有据刷新**(`BACKEND_BENCH_REFRESH=1` + 逐键解释)。
    详见 `../rounds/40-gates-cleanup-and-real-defects.md` §8.3。
- **无数据不做**:任何"看起来更快"的改动,要么有基准点,要么承认在噪声内(例:某次优化 `core` 259 vs 261 ms 属噪声,保留它的理由只是"构造上更少分配")。
- 官方差分门只做**语义比较**(官方从不逐字节对齐)。

## 依据

- `../rounds/22-nemo-decompile-performance.md` §1–§4(现象/根因/实测 A-B)、§7(复现)。
- `../rounds/23-convert-performance-plan.md` §1–§3、§5、§7(落地记录)。
- `../rounds/25-convert-entity-parallelism-plan.md` §1(分布)、§9(正向落地)、§10(反向判不做)。
- `../rounds/29-optimization-scan-ledger.md`(P0/P1 台账;第 2 条 = `CloudAuthenticator` 每次取件重建)。
- KN 行的机制复核(2026-10-05):`git log -S'46 s' -- docs/` 零命中;旧/今 KN 路径对照 = `338fb8f^:src/core/decoders.rs`(`NekoFetcher`/`NekoDecompiler`)vs `src/core/convert/decompile/editors.rs::NekoDecompiler`。
- `../rounds/47-data-layer-rewrite-plan.md`(2026-10-05 数据表示层重写方案与分步读数;§2bis/§2bis.6/§2bis.7 的剖析、`opt-level` 与 Step 1 读数都出自该轮的会话与提交)。
- 代码锚点:`src/core/convert/decompile/mod.rs`(`RESOURCE_DOWNLOAD_BUDGET`)、`src/core/convert/mod.rs`(两级预算折算)、`src/core/convert/shared.rs::json_obj`(搬所有权、不重新物化)、`tests/convert_bench.rs`(自有 SHA256 基线:6 样本,4 Kitten + 2 NEMO)。
