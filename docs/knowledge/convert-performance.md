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

   故最优区间为 8–16。下载是 I/O 密集,**不能用 `available_parallelism` 折算**(低核机器会折到近串行、高核机器会放宽到限流区)。本库把"作品级 × 单作品资源级"总线程**封顶 16**(常量 `RESOURCE_DOWNLOAD_BUDGET`)。
2. **`locate_resource` 在 hot path 上占比最大**(转换剖析里约 2/3)。
3. **NEMO 逐条 XML 解析**:3.5 MB 在 Node 下要 4–5 分钟(逐条 `DOMParser`);Rust 侧按"单文件秒级"设计。
4. **整份文档 JSON 三进三出**:`translate_file` 先 `read_to_string + from_str`,产物再 `to_string + write`;`translate_work` 还多一轮"先落盘再读回"。

## 2bis. 热点归因(2026-10-05 剖析读数,单机)

> 方法:`cargo test --profile bench_perf --test convert_bench -- --ignored --nocapture`(6 样本、`RUNS = 5` 取最小),
> 以 `CARGO_PROFILE_BENCH_PERF_DEBUG=1` + `-C force-frame-pointers=yes` 另建一份带符号的二进制放 `target/prof/`,
> `taskset -c 0-3 perf record -F 199 -g --call-graph fp` 采样本进程(含测试框架),平坦热点按 `perf report --sort dso,symbol`。
> 机器:i5-5200U(2 物理核 / 4 逻辑核)、glibc 默认分配器。**跨机不可比**,只作方向判据;要当门须先建基线。

### 2bis.1 阶段耗时(2026-10-05 中位,ms)

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

| 归类 | 占比(各行口径不同、行间互有重叠,故合计大于 100%) |
| --- | --- |
| libc(分配/释放及其内部,含未解析地址簇) | 36.8% |
| 其它/未分类(std/core 粘合、`from_utf8`、`StrSearcher`、clone、sip 哈希、`Value::index_into` 等) | 36.1% |
| `serde_json` 反序列化(`Value` 树) | 9.2% |
| `Value`/`BTreeMap` 析构(`drop_glue`) | 9.6% |
| `BTreeMap<String, Value>` 插入与遍历 | 8.4% |
| `serde_json` 序列化与转义(`format_escaped_str`) | 8.4% |
| `sha2` | 8.6% |
| 本库 `backend::core::convert` 逻辑合计 | 8.5% |

- **本表的读法是比量级,不是按比例求余**:各行取自不同的归类汇总口径(有的含其子树、`其它/未分类` 为兜底桶),行间**互有重叠**,八行相加为 **125.6%**;因此只能用来判断"哪一类最贵",不能用"100% 减去某行"推别处的占比。
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
5. **`e2e` 未归类占比**在 kitten4-10.8MB 高达 32%(227 ms),而 NEMO 只有 4–5% —— [INFERENCE] 前者对应"巨型 `Value` 文档的物化与析构 + 写盘",与 2bis.3 里 `drop_glue`/`BTreeMap` 析构的读数一致;要坐实需在 `translate_file` 内部再加一次快照(2026-10-05 未做)。

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
   **引用本条时必须带上端到端尺度**:这一点换算到整条流水线只有约 −5% 分配,低于同轮 A/B 的可测门 —— 2026-09-26 已按此规模试过并回退(`../rounds/37` §10.6,P6:去 `flatten` + 手写 `from_value`/`to_value`,产物逐字节等价、三轮交替量不到收益),2026-10-05 的探针不翻该结论。要真吃这一块,须与"少建中间 `Value` 树"的大改同批做并自带 A/B。
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
3. **键序等价是可以工程化的**:块写出的字节等价靠三条口径撑住 —— 键序 = `serde_json::Map`(= `BTreeMap`)的字节序、`shield` 恒写(旧路径靠 `fill_shield` 补)、`extra` 与已知键**撞名时 `extra` 胜**(旧路径是"已知字段先写、flatten 后写覆盖")。三条都必须有常驻等价测试(2026-10-05 排的三条:块写出、空/最小树、三处挂点的整份文档),否则产物门只在整条管线上抓、抓不到单元级错位。

### 2bis.10 反向(KN → Kitten4)产物侧流式写出后的读数(2026-10-05,Step 4 反向落地)

同一轮 A/B(A = `8be59cf`):

| 样本 / 指标 | A | B |
| --- | --- | --- |
| kn-9.4MB `e2e` / `core` | 280 / 133 ms | **237 / 94 ms(−15.4% / −29%)** |
| kn-9.4MB 分配(次数 / 字节) | 798 343 / 156.9 MiB | **602 799 / 141.0 MiB(−24.5%)** |
| kn-3.7MB `e2e` / `core` / 分配 | 81 / 34 ms / 196 004 | **67 / 26 ms / 141 085(−17% / −24% / −28%)** |
| 正向两样本 / NEMO 两样本 分配 | — | **逐位不变** |

**三条结论**(与 §2bis.9 同族,但反向的产物形态不同):

1. **"同一套做法"要按产物形态各写一份**:反向的块表是**邻接表**(`{blocks, connections}`)而不是数组,默认键策略也不同(Kitten4 每块补 12 个缺省键、`parent_id` 强制、`shield` 不恒写)⇒ 复用不了正向的写出器;但**方法论完全可复用**(快照现状 → 按引用直写 → 常驻字节等价门 → SHA 门 → 同轮 A/B),2026-10-05 的两次都一次到位。
2. **流式化的真正障碍往往是"夹在中间的那道 `Value` 改造"**:本步卡住的不是写出器,而是装配期的 `mark_unknown_blocks`(把编辑器不认识的积木改成 `incompatible_*` 标记)——它在 `Value` 上做,而流式路径没有那份 `Value`。移植到 typed 形态后反而更直白(kind/fields/shadows/mutation 与 connections 都是现成字段),但**必须逐条对齐旧口径的时序**:旧流程是"先补缺省键 → 再由标记删掉 `fields`/`shadows`",移植版若照抄"标记块也补缺省",产物就多出两个空键 —— 这条由常驻等价门当场抓出(单测红),否则会一路走到 SHA 门才发现。
3. **共用一个变换是硬要求**:移植后 `mark_unknown_blocks_encoded` 是内存路径与文件路径**唯一**的实现(旧 `Value` 版已删),不存在"两条路径对同一输入行为不同"的分叉。

### 2bis.11 NEMO 侧符号级剖面(2026-10-05;`rounds/37` §4 P9「去重复解析」据此判不做)

**方法**(与 §2bis 同,但**必须显式关掉 strip**):

```bash
CARGO_TARGET_DIR=target/prof CARGO_PROFILE_BENCH_PERF_DEBUG=1 CARGO_PROFILE_BENCH_PERF_STRIP=false \
  RUSTFLAGS="-C force-frame-pointers=yes" cargo test --profile bench_perf --test convert_bench --no-run
taskset -c 0-3 perf record -F 299 -g --call-graph fp -o /tmp/bench.perf -- \
  target/prof/bench_perf/deps/convert_bench-<hash> --ignored --nocapture
perf report --stdio -i /tmp/bench.perf --no-children --sort symbol -g none
```

> **坑**:`bench_perf` 档 `inherits = "release"`,而发布档是 `strip = true`,因此只设 `CARGO_PROFILE_BENCH_PERF_DEBUG=1` 拿到的仍是**被剥掉符号**的二进制(报告里表现为一堆 `[.] 0x…` 未解析地址);要符号必须同时 `CARGO_PROFILE_BENCH_PERF_STRIP=false`。

**读数**(6 样本一轮,占该轮 self time;分母含测试框架自身的 `sha256_hex`):

| 成本组 | 逐条 self(%) | 合计 |
| --- | --- | --- |
| 测试框架校验和与 `.bcmkn` 解密链 | `sha2::sha256::soft::unroll::compress` 11.17 | 11.2% |
| 源 JSON 解析 | `skip_to_escape` 3.39、`parse_str` 1.51、`visit_map` 1.17 与 0.32、`from_utf8` 1.15、`deserialize_any` 1.04、`next_key_seed` 0.79、`ignore_value` 0.21 | 9.6% |
| 分配器 | `cfree` 3.96、`malloc` 2.38、`__rust_alloc` 2.21 | 8.6% |
| `Value` 表构建与析构 | `drop_glue::<Value>` 2.20 与 1.35、`IntoIter::dying_next` 1.76 与 1.10、`BTreeMap::insert` 1.73 与 1.10、`insert_entry` 1.14 | 10.4% |
| 产物写出 | `format_escaped_str` 3.36、`Value::serialize` 0.92、`model::write_block` 0.92 | 5.2% |
| **XML 全链** | `parse_fragment` 0.82、`Parser::parse_name` 0.45、`attr_span` 0.21、`XmlNode::serialize` 0.13、`drop_glue::<XmlNode>` 0.09、`count_source_elements` 0.06、`XmlNode::attr` 0.05、`text_content` 0.04,其余各 ≤0.03 | **1.9%** |
| NEMO 业务函数 | `normalize_integral_numbers` 0.62、`Mapper::parse_block` 0.34、`Mapper::parse_fields` 0.18 | 1.1% |

**结论**:

1. **P9 判不做**:NEMO 两腿约占该轮 `e2e` 的 36%,据此折算 XML 全链约为 NEMO 腿耗时的 5%;而 P9 能动的只是其中"程序集条目的 `blocksXML` 被解析两到三次"那一部分,上界远低于噪声(仓库判胜口径是 `e2e` 出现两位数百分比差)。`text_content` 每次一个 String 的分配同样未进 0.05% 榜(0.04%)。
2. NEMO 装配期的 `json!` 整段再物化**已不存在**:现存的 `json!` 都是 2–5 键的小字面量,大段一律走 `shared::json_obj` 搬所有权(Step 1 已覆盖 NEMO,见 §2bis.7)。
3. **NEMO 唯一剩下的可测杠杆仍是产物侧流式写出**(上界 17–20%,须写第三套写出器;该决策见 `../goals/convert-backlog.md` §1),2026-10-05 的剖面不动摇该结论 —— 产物写出与 `Value` 构建两项合计约 15%。
4. **库自身没有超过 1% 的单符号**(全基准最高是 `model::write_block` 0.92%)。剩余成本结构与 §2bis.5 一致,集中在源解析、表构建、产物写出与分配这四类数据表示层开销上。

**同一轮采样里的按调用树归因**(inclusive,占该轮全部样本;同一会话,`-F 399`):

| 位置 | 占比 | 占 NEMO core 的比例 |
| --- | --- | --- |
| `nemo::convert_nemo_document`(整个 NEMO 转换) | 9.47% | 100% |
| └ `nemo::parse_entity` | 6.43% | 68% |
| &nbsp;&nbsp;├ `prepare_blocks_xml`(XML 解析 + 迁移 + 12 趟 transform) | 3.16% | 33% |
| &nbsp;&nbsp;│ └ `xml::parse_fragment` | 1.88% | 20% |
| &nbsp;&nbsp;└ `nemo_mapping::translate_nemo_to_kn`(映射成强类型树) | 2.82% | 30% |
| └ `nemo::tree_to_json`(积木树写成产物节点) | 1.45% | 15% |
| 其余(装配、`normalize_integral_numbers`、实体条目构造) | 1.59% | 17% |

**由此得到的一条耐久事实**:**NEMO 的每实体工作占其 `core` 的 83%**(`parse_entity` 68% + `tree_to_json` 15%),而实体负载分布极均:

| 样本 | 组 | 实体数 | 源 XML | 最大实体占比 | 前 3 实体占比 |
| --- | --- | --- | --- | --- | --- |
| nemo-3.4MB | `actors_dict` | 847 | 2.18 MB | 2.2% | 6.4% |
| nemo-3.4MB | `scenes_dict` | 38 | 0.19 MB | 46.4%(该组仅 0.19 MB) | 66.0% |
| nemo-old-1.5MB | `actors_dict` | 280 | 0.71 MB | 7.1% | 17.4% |
| nemo-old-1.5MB | `scenes_dict` | 18 | 0.02 MB | 29.0% | 70.5% |

两件样本的 `procedures_dict` 均为空(0 条),因此 §4 里 P9 的"程序集条目被解析两到三次"在基准样本上没有成本,只影响带程序集的真作品,而 XML 全链本身也只有 1.9%。

**现状与上界**:`run_items`(实体级并行)只被正向的 `convert_kitten4_document` 使用,`TranslateReport.entity_workers` 的文档只写"反向暂不支持",**NEMO 方向从未接入**(`../rounds/27` §2 只记了"并行脚手架被硬编码",未评估);按上面的 83% 与同机正向实测的 `core` 1.87×(4 逻辑核 / 2 物理核)折算,NEMO `core` 期望约 1.6×、`e2e` 约 −27%(240 → 约 175 ms),分配次数不变。
**该上界已在同日落地**(`../rounds/48-nemo-entity-parallelism.md`):实测 1 vs 8 为 `core` 1.44× / `e2e` 1.31×,而**分配次数并未"不变"**——记录法使 NEMO 腿的分配 +13.9%,见 §2bis.12。

### 2bis.12 NEMO 实体级并行落地后的读数(2026-10-05,`../rounds/48-nemo-entity-parallelism.md`)

同轮内对照(同一二进制、`taskset -c 0-3`、可用核数 4;`entity_concurrency` 1 vs 8):

| 样本 | `core` | `e2e` | 实际线程 | 分配次数(1 / 8) |
| --- | --- | --- | --- | --- |
| nemo-3.4MB | 282 → **196 ms(1.44×)** | 398 → **303 ms(1.31×)** | 1 → 4 | 1 541 425 / 1 541 583 |
| nemo-old-1.5MB | 85 → **70 ms(1.21×)** | 126 → **104 ms(1.21×)** | 1 → 4 | 472 850 / 472 974 |
| kitten4-10.8MB(对照组,2026-10-05 未改动) | 275 → 165 ms(1.67×) | 433 → 318 ms(1.36×) | 1 → 4 | 909 987 / 910 168 |

三条耐久结论:

1. **默认并发 1 的产物逐字节不变**,由基准的 SHA256 与 `#meta` 直接守住(该基线写于串行实现时代)。
   与正向同款设计:并发 1 也走临时 id 机制,"产物不变"靠的是**铸造顺序 = 项序**。
2. **代价在分配**:NEMO 串行腿的分配由 1 353 318 涨到 **1 541 425(+13.9%)**、分配字节 164.8 → 178.3 MiB(+8%)——
   每铸一个 id 多付"临时 id + 兑现表键 + 改写后替换"。这不是 2026-10-05 新造的模式,而是正向并行(方案 25 S3a)
   同一套设计的既有代价。**因此 §2.2 的 NEMO 分配目标(≤1 300 000)距离更远;而"NEMO 产物侧流式写出"
   与 2026-10-05 的收益正交(它降的正是分配),价值相应更高。** ⇒ **该方向已于 2026-10-06 落地**(`../rounds/49`):
   分配 −14.2% / −14.3%(读数见 §2bis.13),即把这条代价的大部分收了回来。
3. **漂移纪律**:同一份未改动的正向代码在本会话两次读数相差 35%(`core` 204 → 275 ms),故只采信
   同轮内的 1 vs 8 对照;"默认并发 1 的时间开销"不做结论(与 `../rounds/25` §9 对同款路径的判定一致)。

### 2bis.13 NEMO 产物侧流式写出落地后的读数(2026-10-06,`9cf1616`,`../rounds/49-nemo-product-streaming.md`)

同轮 A/B(A = 改动前即 `../rounds/48` 的落地状态,B = 改动后;一侧两轮取最小,`taskset -c 0-3`,可用核数 4;分配窗口 = 一轮 `translate_file` 串行腿):

| 样本 | `core` A → B | `e2e` A → B | 分配次数 A → B | 分配 MiB A → B |
| --- | --- | --- | --- | --- |
| nemo-3.4MB | 255–260 → **207–209**(−18.8%) | 352–357 → **307–310**(−12.8%) | 1 541 425 → **1 322 584**(**−14.2%**) | 178.3 → 155.3 |
| nemo-old-1.5MB | 77–84 → **63–65**(−18.2%) | 107–115 → **99–101**(−7.5%) | 472 850 → **405 054**(**−14.3%**) | 59.3 → 52.4 |
| kitten4-10.8MB / kn-9.4MB / kn-3.7MB / kitten4-0.3MB(未改动对照) | — | — | **逐位相同**(909 987 / 602 799 / 141 085 / 28 864) | — |

**读法与耐久结论**:

1. **分配计数是本装置里唯一的"精确量"**:未改动方向两侧差值恒为 0、同一样本跨会话逐位可复现(2026-10-06 A 侧 `nemo-3.4MB` 的
   1 541 425 与 §2bis.12 的记录值逐位相同)⇒ "−14.2% / −14.3%"是结论级证据;**毫秒列只能同向佐证**
   (同轮里未改动的 `kn-9.4MB` 的 `core` 两侧差 55%、`kitten4-10.8MB` 差 11%,而 NEMO 两侧各自内部一致、无交叠)。
2. **"三条挂点同形"是可复用的判据**:NEMO 的 KN 产物与正向同形(三处挂点/键名一致),差别只有块表 `shield` 补键口径
   ⇒ 复用同一份 `assembly::ProductDocument`,而不是新写一套写出器。反向(KN → Kitten4)之所以不能这样复用的是
   **产物形态不同**(邻接表 `{blocks, connections}` 而非数组),不是"方向不同"。
3. **数字归一必须跟着数据表示走**:旧口径在整份产物 `Value` 上归一,流式路径没有那份 `Value` ⇒ 拆成"非块表部分照旧 +
   块表在 typed 树上就地归一(`BlockJson` 的四个 `Value` 字段 + 三处子槽)"。这类"整份 `Value` 上的后处理"是流式重写的**必付迁移项**
   (同族先例:反向的 `mark_unknown_blocks` 从 `Value` 形态移植到 typed 形态,见 §2bis.10)。

### 2bis.14 反向(KN → Kitten4)源侧骨架落地后的读数(2026-10-06,`845b2c2`,`../rounds/50-kn-source-skeleton.md`)

同轮 A/B(A = 改动前即 `9cf1616` 的落地状态;B 侧两轮,分配读数两轮逐位相同;`taskset -c 0-3`,可用核数 4;分配窗口 = 一轮 `translate_file` 串行腿):

| 样本 | 分配次数 A → B | 分配 MiB A → B | `e2e` A → B | 产物 SHA256 |
| --- | --- | --- | --- | --- |
| kn-9.4MB | 602 799 → **442 291**(**−26.6%**) | 141.0 → 124.9 | 269 → 221 / 134 | 与基线一致 |
| kn-3.7MB | 141 085 → **108 224**(**−23.3%**) | 43.8 → 41.2 | 71 → 62 / 36 | 与基线一致 |
| kitten4-10.8MB(未改动对照) | 909 987 → 909 986(−1) | 184.6 → 184.6 | 435 → 316 / 453 | 与基线一致 |
| nemo-3.4MB(未改动对照) | 1 322 584 → 1 322 584(逐位相同) | 155.3 → 155.3 | 348 → 336 / 319 | 与基线一致 |

**读法与耐久结论**:

1. **省掉的是"整份源文档的 `Value` 中间树"**:反向的块表(三处 `nekoBlockJsonList`)在源文档里占比高于 Kitten4 的
   `block_data_json`,所以同法收益更大(−26.6% / −23.3% 对正向 Step 5 的 −13.6%)。**判据是"这份字段在源文档里有多大"**。
2. **"文档形状参数化"是可复用做法**:两个方向的骨架路径**同深**(文档 → 段 → 字典 → 实体 → 叶子键),只是键名不同 ⇒
   键名提成编译期挂点表(`SourceShape { TARGETS, LEAF }` + `PhantomData`),三层 `Visitor` 一份实现两个形状,
   不必写第二套解析、也不需要 `DeserializeSeed` 样板。前提是"段名/字典名在同一形状内唯一"。
3. **`core` 在该步会"涨",那是口径漂移不是变慢**:`report.elapsed_ms` 自管线入口起算,而骨架路径把块表的 JSON 解析
   从 `translate_file` 的 `from_str`(`core` 之外)挪进管线 ⇒ 与 Step 5 复盘里记的同名现象一致(判收益看 `e2e`/分配)。
4. **摘掉源文档里的键不影响识别**:`detect_editor` 认 KN 只看 `actors.actorsDict` 在不在(与块表无关);
   装配侧按既有约定从不读块表(`rounds/37` P2)⇒ 源侧骨架对这两处都是透明替换。

### 2bis.15 P6(手写去 `#[serde(flatten)]`)按分配口径的复核(2026-10-06)

`../rounds/49` §7 把这条列为"条件成熟时才值得"(口径由**时间**换成**分配**)的候选。在 `../rounds/50` 之后复核,**结论:维持不做**,这次的理由比"低于 A/B 的可测门"硬:

1. **它唯一能影响的那条硬指标已达标且余量很大**:`../rounds/47` §2.2 里正向两行的分配目标是
   `kitten4-10.8MB` ≤1 200 000、`kitten4-0.3MB` ≤50 000;2026-10-06 实测 **909 986** / **28 863**(余量 24% / 42%)。
   而 P6 的端到端上界(单点 −12% 摊薄到流水线 ≈ **−5% 分配**,见 `../rounds/47` §4 Step 5b)买不到任何"必须达标"的东西。
2. **剩下的唯一未达标项走不到它那条路**:六行里唯一没达标的指标是 `nemo-3.4MB` 的 `e2e`(307–310 vs ≤300)
   与分配(1 322 584 vs ≤1 300 000),而基准量的是 `translate_file` ⇒ NEMO 的文件路径**全程不经过** `BlockJson` 的 serde:
   `nemo.rs` 里唯一调用 `BlockJson::to_value` 的地方是 `tree_to_json`,而它只服务**内存**路径与
   `put_block_table` 的 `None` 分支;文件路径的块表由 `model::write_block_tree` 直写(`../rounds/49`)。
   ⇒ **P6 对那一行的影响为零**(可核:该调用图上没有 `from_value`/`to_value`)。
3. **时间收益此前两次同轮 A/B 都量不到**(`../rounds/37` §10.6:逐字节等价、三轮交替零收益 ⇒ 回退;
   `../rounds/47` §4 Step 5b 复核同一结论)。

⇒ 判定维持"不立项"。**重开条件(正面口径)**:出现"分配成为一等指标 **且** 需要 5% 级削减"的新前提,
且届时"单点 −12% 摊到流水线"的换算仍成立 —— 按本篇 §5 的基准纪律,先探针、再同轮 A/B。

### 2bis.16 实体级并发"默认自动"的阈值标定(2026-10-06,`0ead279`,`../rounds/51-entity-concurrency-auto.md`)

原默认是固定 1(由调用方显式传);`../goals/convert-backlog.md` §1 的待决项是"是否对大作品自动开、
阈值定多少"。标定装置:一次性探针(`taskset -c 0-3`、`--profile bench_perf`、1 与 8 各预热一轮 + 3 轮取最小),
样本取本地语料的中段(探针跑完即删)。

| 样本 | 源块 | `core` 加速 | `e2e` 加速 |
| --- | --- | --- | --- |
| k4-1.3MB | 1 491 | 1.56× | **1.36×** |
| k4-1.6MB | 2 063 | 1.68× | **1.51×** |
| k4-3.2MB | 3 497 | 1.49× | **1.30×** |
| k4-6.0MB | 7 762 | 1.82× | **1.34×** |

**耐久结论**:

1. **交叉点比"小样本无收益"那条点外推出来的更早**:374 条(0.3 MB)测不到收益,是因为它 `core` 只有 6 ms
   (`e2e` 10 ms)—— 可并行的工作量比固定开销还小,属**量级**问题;到 1 491 条就已 1.36×。
   ⇒ 阈值取 **1 000 条**(字节口径 **400 KB**,换算依据:正向骨架路径 ≈760 B/块、NEMO ≈250 B/块)。
2. **权重的单位随路径而变是既有事实**:`Value` 路径是源积木条数、原文(骨架)路径与 NEMO 是块表字节数
   (权重原本只服务并行装箱)⇒ 阈值给两个常量、`WeightUnit` 显式标出单位,别用一个数硬套两种口径。
3. **"自动"与"固定"在预算折算时必须分开**:作品级 × 实体级的折算(`fold_entity_concurrency`)对固定值折成
   `min(请求, 可用核数/有效作品并发)`,对自动**只压低核数预算**、不动"够不够大"的判断
   (阈值按作品大小判,与作品级并发无关)—— 否则批量入口会把"自动"悄悄折成串行。
4. **代价仍要记账**:NEMO 侧并行使分配 +13.9%(§2bis.12);"自动"只是把这笔代价收窄到"够大的作品",
   2026-10-06 按**时间收益**开(1.2–1.5×),没有做新的分配规避。

## 3. 已落地的优化(都有数字)

| 优化 | 做法 | 收益 |
| --- | --- | --- |
| 资源下载并发 | 分块并发,总线程封顶 16 | 4× 级(402 → 92 s) |
| `skip_resources` | 只要积木/结构时跳过资源下载 | 53× 级 |
| 内存直通 | `DecompiledArtifact` 直接交给 translate,避免落盘 → 读回 | 省一次 serialize + 一次 parse(10 MB 级 ≈0.3–1.0 s) |
| 去逐块克隆 | `BlockJson::from_value` 不再 `obj.clone()` | 12 764 块受益 |
| **实体级并行(正向)** | 临时 id + 串行兑现/改写;`entity_concurrency` 默认**自动**(够大才开,见本表末行与 §2bis.16) | 10.8 MB:`core` 285 → 184 ms(**1.55×**)、`e2e` 696 → 590 ms(1.18×);**并发 1 与并发 N 产物 SHA256 相同** |
| 两级预算折算 | 作品级 × 实体级:固定值折成 `min(请求, 可用核数 / 有效作品并发)`;"自动"只压低核数预算、不动"够不够大"的判断(`rounds/51`) | 防止 `batch × entity` 超订;只改并行度不碰产物 |
| 反编译侧并发折算 | `decompile_batch` 作品级 × 资源级总线程封顶 16 | 见 §2 |
| **源侧骨架 + `RawValue` 直喂** | 文件入口先按骨架读(`bdj` 留原文),`BlockData::Raw` 直接反序列化成强类型树,省掉 `Value → BlockJson` 那一趟;旧形态自动回落 | 10.8 MB:`e2e` 595/580 → 546/545 ms(同轮 A/B),分配 1 538 778 → 1 329 054(−13.6%);0.3 MB:分配 48 436 → 42 277;**产物 SHA256 全绿**;归因见 §2bis.8 |
| **产物流式写出(正向)** | 块树按引用直写成 JSON(`model::write_block_tree`,不建中间 `Value`)+ `assembly::ProductDocument` 三处块表挂点;公开面 `TranslateDocument.document: Value` 不变(内存路径仍 `into_value()`) | 10.8 MB:`e2e` 541 → **421 ms(−22%)**、`core` 373 → 257 ms、分配 1 329 054 → **909 987(−31.5%)**;0.3 MB:`e2e` 15 → 9 ms;反向/NEMO 逐位不变;**产物 SHA256 全绿**;读数与结论见 §2bis.9 |
| **产物流式写出(反向)** | 邻接表形态的字节等价写出(`model::write_encoded_blocks`)+ `assembly::Kitten4ProductDocument` 两处挂点;`mark_unknown_blocks` 从 `Value` 形态移植到 typed 形态(两条路径共用) | kn-9.4MB:`e2e` 280 → **237 ms(−15.4%)**、`core` 133 → 94 ms、分配 798 343 → **602 799(−24.5%)**;kn-3.7MB:`e2e` 81 → 67 ms、分配 196 004 → 141 085;**产物 SHA256 全绿**;读数与结论见 §2bis.10 |
| **实体级并行(NEMO)** | 与正向同构的四阶段(临时 id 记录 → 串行兑现 → 并行 `remap_tree` + 条目组装 + `tree_to_json` → 串行装配);`NemoParseContext` 的两张只读表改 `Arc` 共享;默认**自动**(见本表末行与 §2bis.16) | nemo-3.4MB:`core` 282 → **196 ms(1.44×)**、`e2e` 398 → **303 ms(1.31×)**;nemo-old-1.5MB 两项均 1.21×;**产物 SHA256 与 `#meta` 全绿**;代价:分配 +13.9%(记录法);读数与结论见 §2bis.12 与 `../rounds/48-nemo-entity-parallelism.md` |
| **产物流式写出(NEMO)** | NEMO → KN 的文件入口复用**正向同一份** `assembly::ProductDocument`:块表编码按 `model::ShieldPolicy` 参数化(正向恒写 `shield`、NEMO 假值不写);装配两条路径共用 `nemo::assemble_nemo`,分叉只在 `put_block_table`;数字归一拆成"非块表部分照旧 + 块表在 typed 树上就地归一" | nemo-3.4MB:分配 1 541 425 → **1 322 584(−14.2%)**、`core` 255–260 → **207–209 ms**、`e2e` 352–357 → **307–310 ms**;nemo-old-1.5MB:分配 472 850 → **405 054(−14.3%)**、`core` 77–84 → **63–65 ms**;未改动方向分配逐位不变,**产物 SHA256 与 `#meta` 全绿**;读数与结论见 §2bis.13 与 `../rounds/49-nemo-product-streaming.md` |
| **源侧骨架(反向)** | `source.rs` 按**文档形状**参数化(Kitten4 / KittenN 共用一套三层 `Visitor`;键名是编译期挂点表),反向的三处 `nekoBlockJsonList` 留原文直喂 `model::parse_kn_entity_typed`;定义体解析拆出"树来源由调用方给"的入口;形态不合在**实体粒度**回落 `Value` | kn-9.4MB:分配 602 799 → **442 291(−26.6%)**、分配 MiB 141.0 → 124.9;kn-3.7MB:分配 141 085 → **108 224(−23.3%)**;正向两样本分配逐位不变(−1 且可解释),**产物 SHA256 与 `#meta` 全绿**;读数与结论见 §2bis.14 与 `../rounds/50-kn-source-skeleton.md` |
| **实体级并发默认自动** | `EntityConcurrency::{Auto, Fixed}`;"够大" = `Σ权重 ≥ 阈值`(条数 1 000 / 字节 400 KB);显式给值即固定(0 与 1 都是串行);批量折算对"自动"只压低核数预算、不动阈值判断 | 阈值由一次性探针夹逼(**1 491 条起 `e2e` 1.30–1.51×**;374 条测不到收益,但那档 `core` 只有 6 ms 属量级问题)⇒ 小作品默认串行、大作品默认并行;产物 SHA256 与"1 vs 8 同 SHA256"门不变;读数与结论见 §2bis.16 与 `../rounds/51-entity-concurrency-auto.md` |

## 4. 判定**不做**(有证据)

| 项 | 结论 | 理由 |
| --- | --- | --- |
| 反向(KN->Kitten4)实体级并行 | **不做** | 63 个工作项、最大一项占 **36.7%**,且两段必须串行(`unrewrite_calls` 依赖全局 `call_targets`、`def_root_from_entry` 把定义根挂进宿主实体)=> Amdahl 上限 1.9×,**实际远低于 1.5×**,而反向 `core` 只有 ~200 ms |
| `RawValue` 顶层只透传 | **不做** | 透传占比 ≈0%(见 `convert-semantics.md` §7) |
| 单遍遍历合并 | **不做** | 只省遍历,不省逐块匹配/字段改写 |
| NEMO 去重复解析(`rounds/37` §4 P9:程序集条目的 `blocksXML` 被解析两到三次、`text_content` 每次一个 String、三趟 `replace_*`) | **不做**(2026-10-05 判) | 符号级剖面:XML 全链只占基准 self 的 **1.9%**(折算约 NEMO 腿耗时的 5%),而 P9 能动的只是其中"程序集条目多解析一两次"那一小半;`text_content` 0.04%、三趟 `replace_*` 均未进 0.05% 榜 => 上界远低于噪声。读数与命令见 §2bis.11 |
| P6 手写去 `#[serde(flatten)]`(逐节点 serde;`rounds/37` §10.6 试过并回退) | **不做**(2026-10-06 按**分配口径**复核) | 它唯一能影响的正向分配目标已达标且余量 24%(909 986 vs ≤1 200 000);唯一未达标的那一行(NEMO 的 `e2e`/分配)走的是**不经过该 serde** 的写出路径(文件路径直写块表);时间收益两次同轮 A/B 均量不到。判据与重开条件见 §2bis.15 |

## 5. 基准方法(避免自欺)

- **绑核**:`taskset -c 0-3`,**5 轮取最小**(本机 2 物理核 / 4 逻辑核);报告要区分 `core` 与 `e2e`。
- **字节门**:并发优化必须证"并发 1 与并发 N 产物 SHA256 相同",再谈加速比。
  样本现为 **6 个**(2 Kitten4 + 2 KN(Neko) + 2 NEMO);NEMO 那两件的 `#meta` 带 `source_version`
  (`0.11.0` 那件低于 0.15.0,故 **YC 版本迁移真的进基线**;`0.16.2` 那件等于迁移目标版本,迁移是 no-op)。
- **`#meta` 与产物 SHA 是两类不同性质的信号**(判断"回归 vs 夹具问题"就看这个):
  - `#meta` 不一致 = **输入夹具被换 / 元信息漂了**(`source_sha256`/`source_bytes`/块数/告警数);
  - **产物 SHA 不一致 = 行为/产物变了**(真正的回归)。
  两者都不通过时还要能**同时看到**:**`tests/convert_bench.rs` 的断言顺序曾把产物 SHA 的红遮住**
  (`#meta` 先 panic)—— 已修(`636127f`):比较抽成纯函数一次返回**全部**不一致项、两类**分开标注**、
  尾部单次 panic 报两类计数与逐项(样本名 -> 键 -> 基线 -> 现在);2026-10-02 的一次误判就是这么来的。
  见 `../rounds/40-gates-cleanup-and-real-defects.md` §8.3.1/§8.6。
- **两种门要分清(一遍知就够)**:
  - **可复现门**:样本来自可控采集(`download/compile/**` 等),故产物 SHA 原则上可复现,变了就是行为变了;
  - **快照冻结门**:`kn-9.4MB` 的产物 SHA **是输入快照的函数** —— 夹具里的实体/积木 id 是**随机 UUID**
    (未开 `deterministic_ids`),而反向会把源实体 id 带进产物,故**重新采集(或再跑一次产它的真机门)
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
- 代码锚点:`src/core/convert/decompile/mod.rs`(`RESOURCE_DOWNLOAD_BUDGET`)、`src/core/convert/mod.rs`(两级预算折算)、`src/core/convert/shared.rs::json_obj`(搬所有权、不重新物化)、`tests/convert_bench.rs`(自有 SHA256 基线:6 样本,2 Kitten4 + 2 KN(Neko) + 2 NEMO)。
