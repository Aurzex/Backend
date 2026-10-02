//! convert 域性能基准(方案 23 §5「基准方法(避免自欺)」)。
//!
//! **只在 release 下有意义**:debug 构建会把 `serde_json` 与字符串操作拖到不成比例,
//! 得出的结论会指向错误的优化方向。此外发布档是 `opt-level = "z"`(为体积),
//! 不适合量测热路径,故用专用档 `bench_perf`(见 `Cargo.toml`,不改动发布档):
//!
//! ```bash
//! cargo test --profile bench_perf --test convert_bench -- --ignored --nocapture
//! ```
//!
//! 每个样本(真作品产物,位于 gitignored 的 `download/`)打印:
//! `read`(读盘)/ `parse`(JSON→Value)/ `core`(转化内核,取自 `report.elapsed_ms`)/
//! `ser`(Value→JSON)/ `e2e`(translate_file 全程,含自身读写与序列化)/
//! 产物字节数 / 积木数 / 告警数 / 产物 SHA256。
//!
//! 断言(三条,**产物不变**是核心):
//! - **产物不变**:与 `tests/fixtures/translate/convert_bench_baseline.json` 逐项 SHA256 相同;
//!   另外 `#meta`(源文件 SHA256 / 产物字节 / 块数 / 告警数)也参与断言 —— "字节没变但报告退化
//!   (块变少、告警变多)"与"输入被换掉"都必须被抓住。
//! - **并发不改产物(第二职责)**:每个样本再用 `entity_concurrency = [`PARALLEL_FACTOR`]`
//!   跑一遍,与并发 1 的 SHA256 必须相同 —— 这是实体级并行(方案 25 S3a)的核心门;
//!   同时它的 `core`/`e2e` 列就是加速比证据(绑核、5 轮取最小)。
//!
//! 性能可以变,产物不能变 —— 所以基准的第一职责是守住 SHA256。
//!
//! **三个环境开关**(开关家族见 `docs/knowledge/repo-conventions.md` §3ter):
//! - `BACKEND_REQUIRE_BENCH=1`:**基准严格模式** —— debug 构建、样本缺失**直接失败**
//!   (默认这两条是"打印后 return 显示 pass",CI/干净检出上等于没有这条门);
//! - `BACKEND_REQUIRE_FIXTURES=1`:**夹具严格模式** —— 与 `convert_facade_bench` /
//!   `convert_work_bench` 同一开关;在本文件里它只管"样本(=夹具)缺了",与 `BACKEND_REQUIRE_BENCH`
//!   分工。**样本"部分缺失"也算缺**(B1):两个开关**任一**打开时,只要有样本不在磁盘上就直接失败
//!   并点名 —— 此前只有"全部缺失"才炸、部分缺失只打印一行警告,而缺的那些键既不被断言、
//!   `W3e` 的重刷守卫又只在 REFRESH 下才拦 ⇒ 默认跑一遍看不出门少守了几档;
//! - `BACKEND_BENCH_REFRESH=1`:**有据刷新基线** —— 写出新的基线文件,并**逐键打印变化**
//!   (产物/元信息),让"为什么变"在提交信息里可审计。
//!
//! **基线缺失(W3a)**:默认模式下也**直接失败** —— 曾经的"NotFound ⇒ 返回空 map + 首次运行
//! 自动写盘"让门在「基线被删 / 全新检出」这两种它必须挡住的情形下静默消失。建基线的唯一通道
//! 是显式 `BACKEND_BENCH_REFRESH=1`(四条路径见 [`load_baseline`])。
//!
//! **重刷不许丢键(W3e)**:`REFRESH` 写出的 `fresh` 只由**存在的**样本构成 ⇒ 样本缺失时重刷会
//! 把缺样本的基线键**永久删掉**(而默认模式下样本缺失只是打印一行警告)⇒ 门的覆盖面会静默缩水。
//! 所以样本缺失时**拒绝写盘**并逐键列出"将被删掉的键"。
//!
//! **NEMO 样本(W6)**:`SAMPLES` 里有两件 NEMO 真作品(源编辑器按**内容**判定),目标都是 KN:
//! `nemo-3.4MB`(在 `download/compile/`,`bcm_version` 0.16.2 = 迁移目标版本 ⇒ 迁移 no-op)与
//! `nemo-old-1.5MB`(在 `download/compile/`(R2 前在 `temp/harness/`,已挪进语料目录);0.11.0 < 0.15.0 ⇒ YC 迁移生效)。
//! NEMO 的版本迁移只由 `TranslateOptions::source_version` 驱动,而 `translate_file`
//! **不会**自动带上它(那是域门面 `translate_work` 的行为)⇒ 该参数经 [`Sample::source_version`]
//! 逐样本透传,并作为 `#meta.source_version` 记进基线。不传就把"未迁移"的产物锁成基线:
//! 门是绿的,证的东西却是错的。
//!
//! **分配计数(W7)**:本二进制挂了一个**只统计**的 `CountingAllocator`(见文件内定义),把每个样本
//! 的分配读数写进 `#meta` 并打印。计量窗口与口径(**数字没有窗口就无意义**):
//!
//! - **窗口** = **一次** `translate_file` 调用。参数构造在快照**之前**,读产物算 SHA256 / 打印 / 样本
//!   元信息都在快照**之后**;取值轮次 = `RUNS` 轮的**最后一轮**(warmup 与其余轮**不计入**);
//! - 窗口**包含**:读源文件 + `serde_json` 解析 + 转化内核 + 序列化 + 流式写盘(≈ `e2e` 那一列);
//!   窗口**不含**:读产物算 SHA256、`std::fs::metadata`、报告打印、测试框架自身;
//! - **串行腿与并行腿分开记**(`alloc_*_serial` / `alloc_*_parallel`):`thread::scope` 的线程、
//!   任务包、排队本身也要分配,并发 1 与 8 本来就不是一回事;
//! - 读数是**全进程累计的两点差**(不是"窗口内独占"),所以快照之间的**其它线程**若也在分配会计进来
//!   —— 并行腿的快照取在 `translate_file` 返回之后(此时工作线程已 join),故抖动**只可能**来自
//!   测试框架的旁观线程;实测同机连跑 4 次、6 个样本 × 2 条腿的读数**逐位相同**(证据见提交信息),
//!   所以并行腿也一并进基线;若将来出现抖动,就把不稳定的那条腿去掉,只留串行腿;
//! - **活性自检**(一次性,已复原):临时在 `translate_file` 入口插一次 `black_box(vec![0u8; 1<<20])`,
//!   6 个样本 × 2 条腿**全部**恰好 +1 次 / +1.0 MiB,产物 SHA 门仍绿;撤掉后读数逐位回到上表
//!   —— 证明计数器数的是真分配、窗口真的罩住被测调用;
//! - **跨机不可比**(分配器版本、核数、线程数、地址空间布局都会变)⇒ 这两组键**只作本地判据**,
//!   **先不当门**:它们写进 `#meta`(基线里看得见、能 diff),但默认模式的 `#meta` 断言会**跳过**
//!   这几个键(见 [`META_RECORD_ONLY`])。跑稳之后再考虑升级成"只许变小"。
//!
//! 测量纪律:这台机器(笔记本/CPU 调频)上**绝对毫秒会漂**(同一二进制两次跑
//! `core` 差 20–40% 是常事),所以:
//! - 每个样本取 `RUNS` 次的**最小值**(≈最少干扰);
//! - 判断"新旧谁快"要在**同一轮内并排比**(见 `convert_facade_flow_bench`),
//!   跨轮比绝对值只能看量级,不能当结论。

use std::alloc::{GlobalAlloc, Layout, System};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};

use backend::core::convert::EditorType;
use backend::core::convert::translate::{TargetEditor, TranslateOptions, translate_file};
use sha2::{Digest, Sha256};

// ---------------------------------------------------------------------------
// 分配计数(W7)
// ---------------------------------------------------------------------------

/// 只统计、不改分配行为的全局分配器:数**次数**与**请求字节**(`layout.size()`)。
///
/// 唯一的 `unsafe`:实现 `GlobalAlloc` 必须 `unsafe impl`,两个方法内部各自直接转发给
/// [`System`](系统分配器),不碰指针算术、不加任何逻辑 ⇒ 除计数外与默认分配器逐字节同行为。
/// `realloc` / `alloc_zeroed` **不覆写**:std 的默认实现分别落在 `alloc` + `dealloc` 与 `alloc` 上
/// ⇒ 重分配算 1 次、清零分配算 1 次,口径统一且与实现无关。
struct CountingAllocator;

static ALLOC_COUNT: AtomicU64 = AtomicU64::new(0);
static ALLOC_BYTES: AtomicU64 = AtomicU64::new(0);

unsafe impl GlobalAlloc for CountingAllocator {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        ALLOC_COUNT.fetch_add(1, Ordering::Relaxed);
        ALLOC_BYTES.fetch_add(layout.size() as u64, Ordering::Relaxed);
        // SAFETY:本分配器的契约就是"原样转发给 System";`layout` 由调用方按其契约给出。
        unsafe { System.alloc(layout) }
    }

    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        // SAFETY:同 `alloc`(`ptr` 由上面对 `System.alloc` 的转发产出,且 `layout` 与之一致)。
        unsafe { System.dealloc(ptr, layout) }
    }
}

#[global_allocator]
static ALLOCATOR: CountingAllocator = CountingAllocator;

/// 取一次分配快照:`(次数, 累计请求字节)`
///
/// `Relaxed` 够用:并行腿的快照取在 `translate_file` 返回**之后**,此时 `thread::scope` 已经
/// join 过所有工作线程(join 建立 happens-before),它们的 `fetch_add` 必然可见。
fn alloc_snapshot() -> (u64, u64) {
    (
        ALLOC_COUNT.load(Ordering::Relaxed),
        ALLOC_BYTES.load(Ordering::Relaxed),
    )
}

/// 快照差(窗口内的分配)
fn alloc_delta(before: (u64, u64)) -> (u64, u64) {
    let now = alloc_snapshot();
    (now.0 - before.0, now.1 - before.1)
}

/// `#meta` 里**只记录、不作门**的键:跨机不可比,先只作本地判据(见模块文档的"分配计数"一节)
const META_RECORD_ONLY: &[&str] = &[
    "alloc_count_serial",
    "alloc_bytes_serial",
    "alloc_count_parallel",
    "alloc_bytes_parallel",
];

/// 去掉 [`META_RECORD_ONLY`] 之后的 `#meta`,用于与基线比较(那些键写进基线但**不参与断言**)
fn meta_for_assert(meta: &serde_json::Value) -> serde_json::Value {
    let mut meta = meta.clone();
    if let Some(object) = meta.as_object_mut() {
        for key in META_RECORD_ONLY {
            object.remove(*key);
        }
    }
    meta
}

/// S2:[`META_RECORD_ONLY`] 的**钉子** —— 剔除的必须**恰好**是那 4 个分配键
///
/// 为什么需要:那几个"只记录不判"的键靠 [`meta_for_assert`] 剔掉才不参与断言;哪天名单被**改大**
/// (多塞进一个本该断言的键,比如 `warnings`),门就静默变松而没人知道。两条断言:①机制(名单外的
/// 键一个不少地留下)②名单本身(改名/增删都红)。
#[test]
fn meta_record_only_strips_exactly_the_alloc_keys() {
    let meta = serde_json::json!({
        "source_sha256": "sha",
        "blocks_total": 1,
        "warnings": 2,
        "source_version": "0.11.0",
        "alloc_count_serial": 3,
        "alloc_bytes_serial": 4,
        "alloc_count_parallel": 5,
        "alloc_bytes_parallel": 6,
    });
    assert_eq!(
        meta_for_assert(&meta),
        serde_json::json!({
            "source_sha256": "sha",
            "blocks_total": 1,
            "warnings": 2,
            "source_version": "0.11.0",
        }),
        "剔除的必须恰好是分配键 —— 名单一被改大,本该断言的键就会静默不设防"
    );
    assert_eq!(
        META_RECORD_ONLY,
        [
            "alloc_count_serial",
            "alloc_bytes_serial",
            "alloc_count_parallel",
            "alloc_bytes_parallel"
        ],
        "名单本身钉住(改键名或增删条目都会在这里红)"
    );
}

/// 每个样本重复次数(取**最小值**:CPU 基准里最小值≈最少干扰,比中位数稳)
const RUNS: usize = 5;

/// 实体级并发的对照值(方案 25 S3a):同一输入在 1 与 8 下产物必须逐字节相同。
/// 实际线程数还会被工作项数与可用核数夹取(见 `remint::workers`)
const PARALLEL_FACTOR: usize = 8;

/// 样本 = 仓库里的真作品产物(gitignored,缺失即整测试跳过)
struct Sample {
    label: &'static str,
    path: &'static str,
    /// 源编辑器(`detect_editor` 按**内容**判定;用于"正向 Kitten4→KN 才有实体级并行"的守卫)
    source_editor: EditorType,
    target: TargetEditor,
    /// 目标编辑器标识(用于产物扩展名/基线键)
    slug: &'static str,
    /// 源作品的 `bcm_version`(NEMO 版本迁移的**唯一**驱动;`None` = 不迁移)
    ///
    /// NEMO 的编辑版文档里没有 `bcm_version`(`translate_file` 也不会自动带上 —— 那是域门面
    /// `translate_work` 的行为)⇒ 不给这一列,门锁住的就是"未迁移"口径的产物:门是绿的,
    /// 但证错了东西。值来自作品元信息(`<work_id>.meta` 的 `bcm_version`)。
    source_version: Option<&'static str>,
}

const SAMPLES: &[Sample] = &[
    Sample {
        label: "kitten4-10.8MB",
        path: "download/compile/原气骑士 且听风吟_136021231.bcm4",
        source_editor: EditorType::Kitten4,
        target: TargetEditor::KittenN,
        slug: "kn",
        source_version: None,
    },
    Sample {
        label: "kitten4-0.3MB",
        path: "download/compile/几何对战-联机_215246857.bcm4",
        source_editor: EditorType::Kitten4,
        target: TargetEditor::KittenN,
        slug: "kn",
        source_version: None,
    },
    Sample {
        // ⚠️ 夹具目录纪律:样本所在目录**必须只读**(任何测试/工具都不得写入)。
        // 本样本原先在 `download/convert/`,而那是 `PathConfig::convert_file_path()`
        // = 转化域门面的**默认输出目录** —— 真机门
        // `convert_live::translate_work_creates_draft_when_ignored` 会往那里写同名产物,
        // 跑一次真机门就把本夹具覆盖(旧字节不可恢复)。现挪到 `download/fixtures/`
        // (gitignored;两个 bench 只读,仓库里没有任何写点)。
        label: "kn-9.4MB",
        path: "download/fixtures/Phigros 自制谱模拟器_195038626.kn.bcmkn",
        source_editor: EditorType::Neko,
        target: TargetEditor::Kitten4,
        slug: "kitten4",
        source_version: None,
    },
    Sample {
        label: "kn-3.7MB",
        path: "download/compile/HEX Editor_317683843.bcmkn",
        source_editor: EditorType::Neko,
        target: TargetEditor::Kitten4,
        slug: "kitten4",
        source_version: None,
    },
    Sample {
        // W6:NEMO 侧此前**没有任何字节基线**(SAMPLES 只有 4 个 Kitten 样本)
        label: "nemo-3.4MB",
        path: "download/compile/蛋仔派对2-奥姆返场新盲盒生存赛重做_194684070/user_works/194684070/194684070.bcm",
        source_editor: EditorType::Nemo,
        target: TargetEditor::KittenN,
        slug: "kn",
        // 作品元信息(`…/194684070.meta`)的 `bcm_version`;与 `nemo_tests::REAL_SAMPLES` 同值。
        // 它**等于**迁移目标版本(`nemo_mapping::NEMO_BCM_VERSION = "0.16.2"`)且 ≥ YC 段边界
        // 0.15.0 ⇒ 官方与这里的迁移标志都是 (false, false):产物是"不需要迁移"的官方口径。
        // 这条仍由本字段**显式固定**(而不是靠"没传"的默认);下一件老版本 NEMO 作品才会让
        // 这个参数在字节上显形(本轮已用临时改成 0.14.0 的 A/B 证明它真的驱动管线)。
        source_version: Some("0.16.2"),
    },
    Sample {
        // 第二件 NEMO 样本:**老版本**(0.11.0 < 0.15.0)⇒ 版本迁移**真的生效**(YC 给 `controls_if`
        // 补 `else="1"`),把上面那件(0.16.2 = 迁移目标版本 ⇒ 迁移是 no-op)证不到的"迁移这段路"
        // 也纳进字节基线。
        label: "nemo-old-1.5MB",
        // 这条路径原先在 `temp/harness/`(**非**语料目录),已按 R2 挪进 `download/`(与其它样本同处):
        // `temp/` 的约定是"及时清理",而它一旦被清,`W3e` 的"重刷拒写盘"会让**所有**基线键都刷不了。
        // 它是 `temp/harness/fetch_nemo.js` 从**公开**作品 API
        // (`GET /creation-tools/v1/works/103791894/source/public` → `work_urls[0]`)拉下来的原始 NEMO
        // 编辑版(与另一件同形态:`actors.actors_dict`,不含 `bcm_version`);官方 harness 门
        // (`nemo_tests::nemo_real_samples_match_official_products`)用的也是同一个文件。
        // 挪位置不改内容 ⇒ 基线里该样本的 SHA 与 `#meta` 一字不变(由本文件的 SHA 断言守)。
        path: "download/compile/nemo-103791894.bcm",
        source_editor: EditorType::Nemo,
        target: TargetEditor::KittenN,
        slug: "kn",
        // 平台侧该作品的 `bcm_version`(2026-10-01 直连 `…/source/public` 实读 = "0.11.0";
        // 与 `docs/rounds/27` §9.1 的记载同一值)。0.11.0 < YC 段边界 0.15.0 ⇒ migration_flags
        // = (false, true):**YC 迁移生效**(本轮已用"改成 None 再跑"的 A/B 证明它会改字节:
        // 传 0.11.0 的产物 SHA = 4b6038f9…,改成 None = a0dbfb41…)。
        source_version: Some("0.11.0"),
    },
];

const BASELINE: &str = "tests/fixtures/translate/convert_bench_baseline.json";

fn sha256_hex(bytes: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    // sha2 0.11 的 digest 类型不实现 LowerHex,自行拼十六进制
    hasher
        .finalize()
        .iter()
        .fold(String::with_capacity(64), |mut acc, byte| {
            use std::fmt::Write as _;
            let _ = write!(acc, "{byte:02x}");
            acc
        })
}

/// 取最小值(见 RUNS 注释)
fn best(samples: Vec<f64>) -> f64 {
    samples.into_iter().fold(f64::INFINITY, f64::min)
}

fn ms(d: Duration) -> f64 {
    d.as_secs_f64() * 1000.0
}

/// 每个基准测试**独立**的临时目录(cargo 默认并行跑测试函数,共用一个目录会互相删)
fn bench_dir(tag: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "backend-convert-bench-{tag}-{}-{:08x}",
        std::process::id(),
        fastrand::u32(..)
    ));
    std::fs::create_dir_all(&dir).expect("创建基准临时目录失败");
    dir
}

/// 单个样本的一次量测结果
struct Measured {
    /// 本次转换实际使用的实体级工作线程数(反向恒 1)
    entity_workers: usize,
    read_ms: f64,
    parse_ms: f64,
    core_ms: f64,
    ser_ms: f64,
    e2e_ms: f64,
    out_bytes: u64,
    blocks_total: usize,
    blocks_converted: usize,
    warnings: usize,
    sha256: String,
    /// 分配计数窗口读数(见模块文档"分配计数"):窗口 = 一轮 `translate_file`
    alloc_count: u64,
    alloc_bytes: u64,
}

fn measure(sample: &Sample, dir: &Path, entity_concurrency: usize) -> Measured {
    // 预热一轮(不计入):首个样本会吃冷缓存/调频爬升,不预热的话它最不可信
    warmup(sample, dir, entity_concurrency);

    let mut read = Vec::new();
    let mut parse = Vec::new();
    let mut core = Vec::new();
    let mut ser = Vec::new();
    let mut e2e = Vec::new();
    let mut last: Option<(std::path::PathBuf, String, usize, usize, usize, u64, usize)> = None;
    let mut alloc = (0u64, 0u64);

    for run in 0..RUNS {
        let t = Instant::now();
        let text = std::fs::read_to_string(sample.path).expect("读样本失败");
        read.push(ms(t.elapsed()));

        let t = Instant::now();
        let value: serde_json::Value = serde_json::from_str(&text).expect("解析样本失败");
        parse.push(ms(t.elapsed()));
        drop(value);

        // 分配窗口(W7,见模块文档):**只**包住这一轮 `translate_file`。
        // - 参数构造(`sample_options` 会 clone 版本号等)提到快照**之前**;
        // - 读数取**最后一轮**(前 `RUNS-1` 轮 + warmup 已经把一次性铺垫吃完);
        // - `Instant::now()` 自身不分配,放在快照之后也不影响 `e2e` 的口径。
        let options = sample_options(sample, dir, entity_concurrency);
        let alloc_before = (run == RUNS - 1).then(alloc_snapshot);
        let t = Instant::now();
        let outcome =
            translate_file(Path::new(sample.path), sample.target, options).expect("转化失败");
        if let Some(before) = alloc_before {
            alloc = alloc_delta(before);
        }
        // 声明的 `source_editor` 由内容判定(`detect_editor`)复核:样本被换成别的格式/编辑器时,
        // 下面那条"正向 Kitten4 样本的并发必须真的开起来"的空门守卫就不再成立(它按本字段判)
        assert_eq!(
            outcome.report.from, sample.source_editor,
            "{}:声明的 source_editor 与 `detect_editor` 的判定不一致(样本被换了?)",
            sample.label
        );
        e2e.push(ms(t.elapsed()));
        core.push(outcome.report.elapsed_ms as f64);

        let output = std::fs::read_to_string(&outcome.output).expect("读产物失败");
        let t = Instant::now();
        let doc: serde_json::Value = serde_json::from_str(&output).expect("产物不是 JSON");
        ser.push(ms(t.elapsed()));
        drop(doc);

        last = Some((
            outcome.output.clone(),
            sha256_hex(output.as_bytes()),
            outcome.report.blocks_total,
            outcome.report.blocks_converted,
            outcome.report.warnings().len(),
            output.len() as u64,
            outcome.report.entity_workers,
        ));
    }

    let (_, sha256, blocks_total, blocks_converted, warnings, out_bytes, entity_workers) =
        last.unwrap();
    Measured {
        entity_workers,
        read_ms: best(read),
        parse_ms: best(parse),
        core_ms: best(core),
        ser_ms: best(ser),
        e2e_ms: best(e2e),
        out_bytes,
        blocks_total,
        blocks_converted,
        warnings,
        sha256,
        alloc_count: alloc.0,
        alloc_bytes: alloc.1,
    }
}

/// 不计入统计的一轮(见 `measure` 注释)
fn warmup(sample: &Sample, dir: &Path, entity_concurrency: usize) {
    let _ = translate_file(
        Path::new(sample.path),
        sample.target,
        sample_options(sample, dir, entity_concurrency),
    );
}

/// 样本的转换选项:`source_version` 是 NEMO 版本迁移的**唯一**驱动,必须逐样本透传
///
/// 不传就会把"未迁移"的产物锁进基线(门是绿的,证的东西错了,见 [`Sample::source_version`])。
fn sample_options(sample: &Sample, dir: &Path, entity_concurrency: usize) -> TranslateOptions {
    let options = TranslateOptions::new()
        .output_dir(dir)
        .deterministic_ids(true)
        .keep_source(false)
        .entity_concurrency(entity_concurrency);
    match sample.source_version {
        Some(version) => options.source_version(version),
        None => options,
    }
}

#[test]
#[ignore = "性能基准:需 --profile bench_perf 且本机有 download/ 真作品样本"]
fn convert_bench() {
    if cfg!(debug_assertions) {
        if strict_mode() {
            panic!("严格模式:convert_bench 必须在 --profile bench_perf(release)下跑");
        }
        eprintln!("[convert_bench] 这是 debug 构建,数字会误导;请用 --profile bench_perf 运行");
        return;
    }
    let missing: Vec<&str> = SAMPLES
        .iter()
        .filter(|s| !Path::new(s.path).exists())
        .map(|s| s.path)
        .collect();
    if missing.len() == SAMPLES.len() {
        if strict_samples() {
            panic!(
                "严格模式(基准开关 = {} / 夹具开关 = {}):样本全缺(需先反编译作品到 download/):{missing:#?}",
                strict_mode(),
                require_fixtures()
            );
        }
        eprintln!("[convert_bench] 缺样本(需先反编译作品到 download/),跳过。缺:{missing:#?}");
        return;
    }
    if !missing.is_empty() {
        // B1:部分缺失同样是"门静默缩水" —— 缺的那些键既不被断言、`W3e` 的重刷守卫又只在 REFRESH
        // 下才拦 ⇒ 默认跑一遍看不出覆盖面少了几档。两个严格开关下**任一**样本缺失即失败并点名。
        if strict_samples() {
            panic!(
                "严格模式(基准开关 = {} / 夹具开关 = {}):样本缺 {}/{} 件 —— 缺样本的基线键不参与断言,\
                 等于门静默缩水:{missing:#?}",
                strict_mode(),
                require_fixtures(),
                missing.len(),
                SAMPLES.len()
            );
        }
        eprintln!("[convert_bench] 警告:部分样本缺失,这些键不参与断言:{missing:#?}");
    }
    let dir = bench_dir("flow");

    println!(
        "\n| 样本 | 源 MB | read ms | parse ms | core ms | ser ms | e2e ms | 产物 MB | 块(源→产物) | 告警 | SHA256 |"
    );
    println!("| --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- |");

    let available = std::thread::available_parallelism().map_or(1, |n| n.get());
    println!(
        "\n实体级并发对照(方案 25 S3a):可用核数 {available},请求并发 {PARALLEL_FACTOR}         (实际线程数还会被工作项数与可用核数夹取;反向不支持该选项,应当零差异)"
    );

    let baseline = load_baseline();
    // **不许静默丢键(W3e)**:重刷写出的 `fresh` 只由**存在的**样本构成 ⇒ 样本缺失时重刷会把
    // 缺样本的基线键**永久删掉**,事后从基线文件本身看不出来(而默认模式下样本缺失只是打印一行
    // 警告)⇒ 门的覆盖面静默缩水。所以在量测**之前**就拒绝(连同下面列出的键,别白跑一遍)。
    if refresh_mode() && !missing.is_empty() {
        let mut would_drop: Vec<String> = Vec::new();
        for sample in SAMPLES {
            if Path::new(sample.path).exists() {
                continue;
            }
            let key = format!("{}-{}", sample.label, sample.slug);
            for candidate in [key.clone(), format!("{key}#meta")] {
                if baseline.contains_key(&candidate) {
                    would_drop.push(candidate);
                }
            }
        }
        panic!(
            "BACKEND_BENCH_REFRESH=1 被拒绝:样本缺失 ⇒ 重刷写出的基线只含跑过的样本,\
             会把下列 {} 个基线键**永久删掉**(覆盖面静默缩水,事后看不出来)。\
             先让样本回到磁盘(反编译/转换);若该样本确实要移除,请在同一个提交里手工删掉\
             它的键并写明原因。\n  缺样本:{missing:#?}\n  将被删:{would_drop:#?}",
            would_drop.len()
        );
    }
    let mut fresh = serde_json::Map::new();
    let mut mismatched = Vec::new();
    let mut parallel_mismatched = Vec::new();
    let mut meta_mismatched = Vec::new();

    for sample in SAMPLES {
        if !Path::new(sample.path).exists() {
            println!("| {} | (缺样本,跳过) | | | | | | | | | |", sample.label);
            continue;
        }
        let src_bytes = std::fs::metadata(sample.path).expect("stat 失败").len();
        let source_sha = sha256_hex(&std::fs::read(sample.path).expect("读源文件失败"));
        let m = measure(sample, &dir, 1);
        println!(
            "| {} | {:.1} | {:.0} | {:.0} | {:.0} | {:.0} | {:.0} | {:.1} | {}→{} | {} | {} |",
            sample.label,
            src_bytes as f64 / 1e6,
            m.read_ms,
            m.parse_ms,
            m.core_ms,
            m.ser_ms,
            m.e2e_ms,
            m.out_bytes as f64 / 1e6,
            m.blocks_total,
            m.blocks_converted,
            m.warnings,
            &m.sha256[..16],
        );

        // ── 第二职责:同一输入在并发 1 与 N 下产物必须逐字节相同(实体级并行的核心门)
        let p = measure(sample, &dir, PARALLEL_FACTOR);
        println!(
            "| {} [实体并发={PARALLEL_FACTOR}] | {:.1} | {:.0} | {:.0} | {:.0} | {:.0} | {:.0} | {:.1} | {}→{} | {} | {} |",
            sample.label,
            src_bytes as f64 / 1e6,
            p.read_ms,
            p.parse_ms,
            p.core_ms,
            p.ser_ms,
            p.e2e_ms,
            p.out_bytes as f64 / 1e6,
            p.blocks_total,
            p.blocks_converted,
            p.warnings,
            &p.sha256[..16],
        );
        println!(
            "└ {} 加速比(并发 1 → {PARALLEL_FACTOR},实际线程 {} → {}):core {:.0} → {:.0} ms({:.2}×),e2e {:.0} → {:.0} ms({:.2}×),产物 SHA256 {}",
            sample.label,
            m.entity_workers,
            p.entity_workers,
            m.core_ms,
            p.core_ms,
            m.core_ms / p.core_ms.max(f64::MIN_POSITIVE),
            m.e2e_ms,
            p.e2e_ms,
            m.e2e_ms / p.e2e_ms.max(f64::MIN_POSITIVE),
            if p.sha256 == m.sha256 {
                "相同 ✅"
            } else {
                "不同 ❌"
            },
        );

        // 分配读数(W7):**只打印 + 记进 `#meta`**,不参与断言(跨机不可比,见模块文档)
        println!(
            "└ {} 分配(窗口 = 一轮 translate_file;串行腿 / 并发 {PARALLEL_FACTOR} 腿):{} 次 / {:.1} MiB  ·  {} 次 / {:.1} MiB",
            sample.label,
            m.alloc_count,
            m.alloc_bytes as f64 / (1024.0 * 1024.0),
            p.alloc_count,
            p.alloc_bytes as f64 / (1024.0 * 1024.0),
        );

        // 空门守卫:若本机可用核数 ≥ 2,则**正向 Kitten4→KN** 样本的"实体并发=8"必须真的开起
        // 多线程,否则这一行只是"串行 vs 串行",SHA 相同毫无意义(`taskset -c 2` 就会这样:
        // `available_parallelism` 按亲和掩码算,单核下会被折成 1)。
        // 只对 Kitten4 源:实体级并行是正向 Kitten4 路径的实现(NEMO→KN 的管线恒串行,恒 1;
        // 反向 KN→Kitten4 同样恒 1),按目标编辑器判会把这两类错算成"空门"。
        if available >= 2 && sample.source_editor == EditorType::Kitten4 && p.entity_workers <= 1 {
            panic!(
                "{}:实体并发={PARALLEL_FACTOR} 实际只开了 {} 个线程(正向 Kitten4→KN 样本应并行)——  并发对照成了空门",
                sample.label, p.entity_workers
            );
        }

        let key = format!("{}-{}", sample.label, sample.slug);
        if p.sha256 != m.sha256 {
            parallel_mismatched.push((key.clone(), m.sha256.clone(), p.sha256.clone()));
        }
        if let Some(old) = baseline.get(&key).and_then(|v| v.as_str())
            && old != m.sha256
        {
            mismatched.push((key.clone(), old.to_string(), m.sha256.clone()));
        }
        fresh.insert(key.clone(), serde_json::Value::String(m.sha256.clone()));
        let mut meta = serde_json::json!({
            "source_sha256": source_sha,
            "source_bytes": src_bytes,
            "output_bytes": m.out_bytes,
            "blocks_total": m.blocks_total,
            "blocks_converted": m.blocks_converted,
            "warnings": m.warnings,
            // 分配计数(W7):窗口 = 那一腿的一轮 `translate_file`(定义见模块文档)。
            // 串行/并行两腿分开记;这两组键属于 [`META_RECORD_ONLY`] ⇒ **写进基线但不参与断言**
            // (跨机不可比,先只作本地判据)。
            "alloc_count_serial": m.alloc_count,
            "alloc_bytes_serial": m.alloc_bytes,
            "alloc_count_parallel": p.alloc_count,
            "alloc_bytes_parallel": p.alloc_bytes,
        });
        // NEMO 的口径进基线:该样本是靠 `source_version` 驱动迁移的,不记下来就分不清
        // "这份 SHA 是迁移后的产物"还是"参数没接上、被锁成未迁移"(只给有版本的样本加键 ⇒
        // 既有 Kitten 样本的 `#meta` 一字不变)。
        if let Some(version) = sample.source_version {
            meta["source_version"] = serde_json::Value::String(version.to_string());
        }
        // 元信息也参与断言:字节没变但块数/告警退化、或输入被换掉,都要抓。
        // **但**分配计数那几个键要先剔除 —— 它们是"只记录不判"(跨机不可比,作门会假红)。
        if let Some(old) = baseline.get(&format!("{key}#meta"))
            && meta_for_assert(old) != meta_for_assert(&meta)
        {
            meta_mismatched.push((key.clone(), old.clone(), meta.clone()));
        }
        fresh.insert(format!("{key}#meta"), meta);
    }

    if !parallel_mismatched.is_empty() {
        eprintln!(
            "\n[convert_bench] 实体级并发改变了产物(并发 1 与 {PARALLEL_FACTOR} 的 SHA256 不一致 = 失败):"
        );
        for (key, serial, parallel) in &parallel_mismatched {
            eprintln!("  {key}\n    并发 1 {serial}\n    并发 {PARALLEL_FACTOR} {parallel}");
        }
        panic!("convert 基准:实体级并发改变了产物");
    }

    // 有据重刷:显式开关才写,并逐键打印变化(让"为什么变"可审计)。
    // (样本缺失时根本到不了这里 —— 上面 W3e 的守卫已经拒绝写盘并列出会被删的键)
    if refresh_mode() {
        eprintln!("\n[convert_bench] BACKEND_BENCH_REFRESH=1:**重刷基线** —— 变化逐键列出:");
        for key in fresh.keys() {
            let old = baseline.get(key);
            let new = fresh.get(key);
            if old != new {
                eprintln!("  {key}\n    旧 {old:?}\n    新 {new:?}");
            }
        }
        std::fs::write(BASELINE, serde_json::to_string_pretty(&fresh).unwrap())
            .expect("写基线失败");
        println!("[convert_bench] 基线已写入 {BASELINE}(记得在提交信息里写清原因)");
        return;
    }

    if !meta_mismatched.is_empty() {
        eprintln!("\n[convert_bench] 元信息与基线不一致(字节可能没变,但**报告退化或输入被换**):");
        for (key, old, new) in &meta_mismatched {
            eprintln!("  {key}\n    基线 {old}\n    现在 {new}");
        }
        panic!("convert 基准:元信息(source SHA/块数/告警/字节)与基线不一致");
    }

    if !mismatched.is_empty() {
        eprintln!("\n[convert_bench] 产物与基线不一致(性能改了但产物变了 = 失败):");
        for (key, old, new) in &mismatched {
            eprintln!("  {key}\n    基线 {old}\n    现在 {new}");
        }
        panic!("convert 基准:产物 SHA256 与基线不一致");
    }

    // 走到这里基线必然非空且对得上:缺失在 `load_baseline` 就炸了(W3a),`REFRESH` 已在上面的分支
    // 写完并 return。原先这里的"首次运行 ⇒ 自动写基线"分支已删 —— 那是默认模式下的静默重建。
    println!(
        "\n[convert_bench] 产物 SHA256 与元信息都与基线一致 ✅;实体级并发 1 vs {PARALLEL_FACTOR} 同 SHA256 ✅"
    );
}

/// 读基线。**四条路径(W3a)**:
/// 1. 存在且能解析 ⇒ 正常返回(门照常比);
/// 2. 存在但解析失败 ⇒ **一律炸**(不因 `REFRESH` 放宽:被手改坏的基线不该被"顺手重刷"掩盖;
///    真要从头重建就**先删文件**再 `BACKEND_BENCH_REFRESH=1`,让"重建"在 diff 里看得见);
/// 3. `NotFound` + `BACKEND_BENCH_REFRESH=1` ⇒ 返回空 map —— "从零建基线"的**唯一合法通道**
///    (必须放在这里判:`load_baseline` 在 REFRESH 分支之前调用,无条件炸会把首跑自己堵死);
/// 4. `NotFound` + 其它(默认 / 严格模式)⇒ **炸**。原先这里是"返回空 map,再由后半段
///    `baseline.is_empty()` 分支静默写盘" —— 那等于把门在「基线被删 / 全新检出」时关掉。
fn load_baseline() -> serde_json::Map<String, serde_json::Value> {
    let text = match std::fs::read_to_string(BASELINE) {
        Ok(text) => text,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            if refresh_mode() {
                return serde_json::Map::new();
            }
            panic!(
                "基线 {BASELINE} 缺失(严格模式 = {})—— 建基线请显式跑一次 BACKEND_BENCH_REFRESH=1;\
                 默认模式不再静默重建(那会让门在「基线被删 / 全新检出」时消失)",
                strict_mode()
            );
        }
        Err(e) => panic!("读基线 {BASELINE} 失败:{e}"),
    };
    // 基线损坏必须炸:静默 `unwrap_or_default()` 会把"基线被删/弄坏"变成"悄悄重建基线",
    // 第一职责门就白设了。注意这条**不因 `BACKEND_BENCH_REFRESH=1` 放宽** —— 要重建就先删文件
    // 再重刷,让"重建"这件事在 diff 里看得见。
    match serde_json::from_str(&text) {
        Ok(map) => map,
        Err(e) => panic!(
            "基线 {BASELINE} 解析失败(不要手改;确要重建请先删掉该文件再用 BACKEND_BENCH_REFRESH=1 跑一次):{e}"
        ),
    }
}

/// `BACKEND_REQUIRE_BENCH=1`:把"静默跳过"变成失败(见模块文档)
fn strict_mode() -> bool {
    std::env::var("BACKEND_REQUIRE_BENCH").is_ok_and(|v| matches!(v.as_str(), "1" | "true" | "yes"))
}

/// `BACKEND_REQUIRE_FIXTURES=1`:**夹具**严格模式 —— 与 `convert_facade_bench` / `convert_work_bench`
/// 同一开关(见 `docs/knowledge/repo-conventions.md` §3ter)。在本文件里它管"样本(=夹具)缺了",
/// 与 `BACKEND_REQUIRE_BENCH`(基准自身:debug 构建 / 基线)分工;两者**任一**打开都让缺样本直接失败
/// (B1:此前只有"样本全缺"才炸,**部分缺失只打印一行警告** ⇒ 基线里那些键静默不设防)。
fn require_fixtures() -> bool {
    std::env::var("BACKEND_REQUIRE_FIXTURES")
        .is_ok_and(|v| matches!(v.as_str(), "1" | "true" | "yes"))
}

/// 缺样本时的统一出口:`strict_mode() || require_fixtures()` 下 panic 并点名缺了谁,否则返回 false
/// (调用方打印警告后继续用剩下的样本)。
fn strict_samples() -> bool {
    strict_mode() || require_fixtures()
}

/// `BACKEND_BENCH_REFRESH=1`:有据重刷基线(打印逐键变化;不写就永远不写)
fn refresh_mode() -> bool {
    std::env::var("BACKEND_BENCH_REFRESH").is_ok_and(|v| matches!(v.as_str(), "1" | "true" | "yes"))
}
