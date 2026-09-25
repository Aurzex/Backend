pub(crate) mod blocks;
pub(crate) mod editors;

use crate::core::convert::decompile::editors::{
    CocoDecompiler, CocoFetcher, KittenDecompiler, KittenFetcher, NekoDecompiler, NekoFetcher,
    NemoDecompiler, NemoFetcher, WoodDecompiler, WoodFetcher,
};
use crate::core::convert::shared::{
    CodeMaoHttpClient, DecompilerConfig, EditorType, FileService, HttpClient, IdGenerator,
    RawWorkData, Result, ResultExt, WorkFetcher, WorkInfo,
};
use crate::core::convert::shared::{DecompilerError, WorkId};
use crate::utils::requests::CodeMaoClient;
use log::{debug, info, warn};
use serde_json::Value;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, OnceLock};

// 反编译选项(构建器)
/// 反编译调用配置,供外部通过链式方法定制(门面模式的参数对象)
#[derive(Debug, Clone)]
pub struct DecompileOptions {
    /// 输出目录;`None` 时使用 `DecompilerConfig::default_output_dir`
    output_dir: Option<PathBuf>,
    /// 是否保存原始(未反编译)数据到 `output_dir/raw/`,默认 true
    save_raw: bool,
    /// 批处理并发数(≥1),默认 1
    batch_concurrency: usize,
    /// 资源文件下载并发数(≥1),默认 8。NEMO/WOOD 逐个下载造型/素材,
    /// 串行时请求数 × RTT 就是总耗时(实测 NEMO 1 390 个文件串行 ≈ 8 分钟)
    resource_concurrency: usize,
    /// 是否下载资源文件(默认 true)。关掉只产出文档与元数据
    /// (NEMO 的 `.bcm`/`.userimg`/`.meta`/`.cover`),用于"只要作品结构"或
    /// "准备上传到平台"的场景 —— 1390 次请求 → 0 次
    skip_resources: bool,
}

impl Default for DecompileOptions {
    fn default() -> Self {
        Self::new()
    }
}

impl DecompileOptions {
    pub fn new() -> Self {
        Self {
            output_dir: None,
            save_raw: true,
            batch_concurrency: 1,
            resource_concurrency: 8,
            skip_resources: false,
        }
    }

    /// 指定输出目录
    pub fn output_dir(mut self, dir: impl Into<PathBuf>) -> Self {
        self.output_dir = Some(dir.into());
        self
    }

    /// 是否保存原始数据(默认 true)
    pub fn save_raw(mut self, on: bool) -> Self {
        self.save_raw = on;
        self
    }

    /// 批处理并发数(默认 1)
    pub fn batch_concurrency(mut self, n: usize) -> Self {
        self.batch_concurrency = n.max(1);
        self
    }

    /// 资源文件下载并发数(默认 8)
    pub fn resource_concurrency(mut self, n: usize) -> Self {
        self.resource_concurrency = n.max(1);
        self
    }

    /// 跳过资源文件下载,只产出文档与元数据(默认 false)
    pub fn skip_resources(mut self, on: bool) -> Self {
        self.skip_resources = on;
        self
    }
}

// 作品处理器注册表(注册表模式)
/// fetcher 构造器:按作品类型创建对应的 `WorkFetcher`
pub(crate) type FetcherFactory =
    Box<dyn Fn(Box<dyn HttpClient>, Arc<DecompilerConfig>) -> Box<dyn WorkFetcher> + Send + Sync>;
/// decompiler 构造器:按作品类型创建对应的 `WorkDecompiler`
pub(crate) type DecompilerFactory =
    Box<dyn Fn(&Arc<DecompilerConfig>) -> Box<dyn WorkDecompiler> + Send + Sync>;

/// 作品类型 → 处理器(fetcher/decompiler)的注册表
/// 新增作品类型时只需 `register`,无需修改门面代码(开闭原则)
pub(crate) struct WorkProcessorRegistry {
    fetchers: HashMap<EditorType, FetcherFactory>,
    decompilers: HashMap<EditorType, DecompilerFactory>,
}

impl WorkProcessorRegistry {
    pub(crate) fn new() -> Self {
        Self {
            fetchers: HashMap::new(),
            decompilers: HashMap::new(),
        }
    }

    /// 注册某一作品类型的 fetcher 与 decompiler 构造器
    pub(crate) fn register(
        &mut self,
        work_type: EditorType,
        fetcher: FetcherFactory,
        decompiler: DecompilerFactory,
    ) {
        self.fetchers.insert(work_type, fetcher);
        self.decompilers.insert(work_type, decompiler);
    }

    /// 按作品类型创建 fetcher
    pub(crate) fn fetcher_for(
        &self,
        work_type: &EditorType,
        client: Box<dyn HttpClient>,
        config: Arc<DecompilerConfig>,
    ) -> Result<Box<dyn WorkFetcher>> {
        self.fetchers
            .get(work_type)
            .ok_or_else(|| DecompilerError::UnsupportedType(format!("{:?}", work_type)))
            .map(|factory| factory(client, config))
    }

    /// 按作品类型创建 decompiler
    pub(crate) fn decompiler_for(
        &self,
        work_type: &EditorType,
        config: &Arc<DecompilerConfig>,
    ) -> Result<Box<dyn WorkDecompiler>> {
        self.decompilers
            .get(work_type)
            .ok_or_else(|| DecompilerError::UnsupportedType(format!("{:?}", work_type)))
            .map(|factory| factory(config))
    }

    /// 内置全部作品类型的默认注册
    fn with_defaults() -> Self {
        let mut registry = Self::new();
        // Kitten2/3/4 共用 KittenFetcher / KittenDecompiler
        for wt in [
            EditorType::Kitten2,
            EditorType::Kitten3,
            EditorType::Kitten4,
        ] {
            registry.register(
                wt,
                Box::new(|client, config| Box::new(KittenFetcher::new(client, config))),
                Box::new(|_| Box::new(KittenDecompiler)),
            );
        }
        registry.register(
            EditorType::Neko,
            Box::new(|client, config| Box::new(NekoFetcher::new(client, config))),
            Box::new(|config| Box::new(NekoDecompiler::new(config.crypto_salt.as_slice()))),
        );
        registry.register(
            EditorType::Nemo,
            Box::new(|client, config| Box::new(NemoFetcher::new(client, config))),
            Box::new(|_| Box::new(NemoDecompiler)),
        );
        registry.register(
            EditorType::Wood,
            Box::new(|client, config| Box::new(WoodFetcher::new(client, config))),
            Box::new(|_| Box::new(WoodDecompiler)),
        );
        registry.register(
            EditorType::Coco,
            Box::new(|client, config| Box::new(CocoFetcher::new(client, config))),
            Box::new(|_| Box::new(CocoDecompiler)),
        );
        registry
    }
}

impl Default for WorkProcessorRegistry {
    fn default() -> Self {
        Self::with_defaults()
    }
}

// 主入口
pub struct CodemaoDecompiler {
    config: Arc<DecompilerConfig>,
    client: Arc<CodeMaoClient>,
    id_generator: IdGenerator,
    registry: Arc<WorkProcessorRegistry>,
}

impl CodemaoDecompiler {
    /// 使用自定义 HTTP 客户端构造反编译器(注入客户端,便于独立实例/测试)
    pub fn new(client: CodeMaoClient) -> Self {
        Self::new_inner(None, Arc::new(client))
    }

    fn new_inner(config: Option<DecompilerConfig>, client: Arc<CodeMaoClient>) -> Self {
        let config = Arc::new(config.unwrap_or_default());
        Self {
            config,
            client,
            id_generator: IdGenerator::new(),
            registry: Arc::new(WorkProcessorRegistry::default()),
        }
    }

    /// 全局单例门面:复用全局 HTTP 客户端与默认注册表
    /// 多次反编译不重复创建客户端(性能优化)
    pub fn global() -> &'static Self {
        static GLOBAL: OnceLock<CodemaoDecompiler> = OnceLock::new();
        GLOBAL.get_or_init(|| {
            let client = Arc::new(CodeMaoClient::global().clone());
            Self::new_inner(None, client)
        })
    }
    /// 反编译单个作品(默认选项,向后兼容)
    pub fn decompile(&self, work_id: WorkId, output_dir: Option<&Path>) -> Result<PathBuf> {
        let mut options = DecompileOptions::new();
        if let Some(dir) = output_dir {
            options = options.output_dir(dir.to_path_buf());
        }
        self.decompile_with_options(work_id, options)
    }

    /// 使用自定义选项反编译单个作品
    pub fn decompile_with_options(
        &self,
        work_id: WorkId,
        options: DecompileOptions,
    ) -> Result<PathBuf> {
        self.decompile_inner(work_id, &options)
    }

    /// 批处理反编译多个作品,返回与输入顺序一致的 `Vec<Result>`
    pub fn decompile_batch(
        &self,
        work_ids: &[WorkId],
        options: DecompileOptions,
    ) -> Vec<Result<PathBuf>> {
        let concurrency = options.batch_concurrency.max(1);
        if concurrency == 1 || work_ids.len() <= 1 {
            return work_ids
                .iter()
                .map(|&id| self.decompile_inner(id, &options))
                .collect();
        }
        // 两级并发预算:**资源下载是 I/O 密集(RTT 主导),上限必须是固定常量而不是核数**。
        //
        // `docs/rounds/22` §3 实测(同一 NEMO 作品):并发 1 = 402s、8 = 103s、16 = 92s、
        // **32 = 127s 且被 CDN 限流丢了 2 个文件**。所以:
        // - 按 `available_parallelism` 折算会两头都错:低核机器折到近串行、高核机器放宽到限流区;
        // - 正确做法是把"作品级 × 单作品资源级"的**总线程数**封在 16
        //   (即 `batch=4` 时每个作品 4 个下载线程,而不是 4×8=32)。
        // 默认 `batch_concurrency = 1`(单作品 8)时本折算不退化为任何变化。
        let mut per_work = options.clone();
        per_work.resource_concurrency = options
            .resource_concurrency
            .max(1)
            .min((RESOURCE_DOWNLOAD_BUDGET / concurrency).max(1));
        debug!(
            "批量反编译并发预算:作品级 {concurrency} × 资源级 {} ≤ {RESOURCE_DOWNLOAD_BUDGET}",
            per_work.resource_concurrency
        );
        // 按并发数分块,块内并发执行(thread::scope),块间顺序收集保持结果顺序
        let options_ref = &per_work;
        let mut results = Vec::with_capacity(work_ids.len());
        for chunk in work_ids.chunks(concurrency) {
            let chunk_results: Vec<Result<PathBuf>> = std::thread::scope(|scope| {
                let handles: Vec<_> = chunk
                    .iter()
                    .map(|&id| scope.spawn(move || self.decompile_inner(id, options_ref)))
                    .collect();
                handles
                    .into_iter()
                    .map(|handle| {
                        handle.join().unwrap_or_else(|_| {
                            Err(DecompilerError::Other {
                                msg: "反编译线程异常".to_string(),
                                source: None,
                            })
                        })
                    })
                    .collect()
            });
            results.extend(chunk_results);
        }
        results
    }

    /// 反编译成**内存产物**(Kitten/NEKO 不落盘;方案 23 P0-2)
    ///
    /// 与 [`Self::decompile_with_options`] 共用同一段主流程(取信息 → 取原始数据 →
    /// 反编译),区别只在最后**不**调 `save_result`:
    ///
    /// - Kitten / NEKO:直接返回内存文档 —— 旧路径是"写 JSON → `translate` 再读回",
    ///   10 MB 级作品白付一次 serialize + 一次 parse;
    /// - NEMO:走 [`WorkDecompiler::editable_document`] 直接给明文编辑版(仍然不落盘、不下资源);
    /// - WOOD:只有落盘形态,退回旧路径(落盘并返回路径),调用方按文件处理。
    pub fn decompile_artifact_with(
        &self,
        work_id: WorkId,
        options: DecompileOptions,
    ) -> Result<DecompiledArtifact> {
        let (decompiler, raw, context) = self.prepare_decompile(work_id, &options)?;
        // 内存形态优先(NEMO):只要文档,不落盘、不下资源
        if let Some(editable) = decompiler.editable_document(&raw, &context)? {
            // NEMO 在配置里**没有**扩展名(它的产物是资源目录),但编辑版文档本体就是
            // `<work_id>.bcm`(见 `NemoResourceManager::save_core_files`);产出文件名沿用同一口径.
            let extension = match context.work_info.work_type {
                EditorType::Nemo => "bcm".to_string(),
                _ => context
                    .work_info
                    .file_extension(&context.config)
                    .trim_start_matches('.')
                    .to_owned(),
            };
            let file_name = FileService::safe_filename(
                &context.work_info.name,
                context.work_info.id.get(),
                &extension,
            );
            info!(
                "作品 [work_id={}] 反编译完成(内存编辑版,未落盘;将命名为 {file_name})",
                work_id
            );
            return Ok(DecompiledArtifact::Document {
                document: editable.document,
                file_name,
                source_version: editable.source_version,
            });
        }
        let result = decompiler.decompile(raw, &context)?;
        match result {
            DecompileResult::Json(document) => {
                let extension = context
                    .work_info
                    .file_extension(&context.config)
                    .trim_start_matches('.')
                    .to_owned();
                // 与 `save_json_result` 同一命名口径(同名同扩展名,只是不落盘),
                // 让调用方在需要落盘/上传时拿到**逐字一致**的文件名
                let file_name = FileService::safe_filename(
                    &context.work_info.name,
                    context.work_info.id.get(),
                    &extension,
                );
                info!(
                    "作品 [work_id={}] 反编译完成(内存,未落盘;将命名为 {file_name})",
                    work_id
                );
                Ok(DecompiledArtifact::Document {
                    document,
                    file_name,
                    // 只有走"内存形态编辑版"的反编译器(NEMO)能给出源版本;
                    // Kitten/NEKO 方向不做版本迁移,这里留空。
                    source_version: String::new(),
                })
            }
            result @ DecompileResult::Path(_) => {
                let output_path = options
                    .output_dir
                    .as_deref()
                    .unwrap_or(&self.config.default_output_dir);
                let saved = decompiler.save_result(&result, Some(output_path), &context)?;
                info!(
                    "作品 [work_id={}] 反编译完成,保存至: {}",
                    work_id,
                    saved.display()
                );
                Ok(DecompiledArtifact::Path(saved))
            }
        }
    }

    /// 反编译主流程(模板方法)
    /// 流程为:获取信息 → 创建处理器 → 取原始数据 → (可选)保存原始数据 → 反编译 → 保存结果
    fn decompile_inner(&self, work_id: WorkId, options: &DecompileOptions) -> Result<PathBuf> {
        let (decompiler, result, context) = self.decompile_core(work_id, options)?;
        // 确定输出目录(用户指定或默认)
        let output_path = options
            .output_dir
            .as_deref()
            .unwrap_or(&self.config.default_output_dir);
        let saved = decompiler.save_result(&result, Some(output_path), &context)?;
        info!(
            "作品 [work_id={}] 反编译完成,保存至: {}",
            work_id,
            saved.display()
        );
        Ok(saved)
    }

    /// 反编译主流程的**核心**(模板方法的前半段):取信息 → 建上下文 → 取原始数据
    /// → (可选)存原始数据 → 反编译。落盘与否由调用方决定。
    fn decompile_core(
        &self,
        work_id: WorkId,
        options: &DecompileOptions,
    ) -> Result<(Box<dyn WorkDecompiler>, DecompileResult, DecompilerContext)> {
        let (decompiler, raw, context) = self.prepare_decompile(work_id, options)?;
        let result = decompiler.decompile(raw, &context)?;
        Ok((decompiler, result, context))
    }

    /// 反编译的前半段(取信息 → 建上下文 → 取原始数据 → 可选存原始数据),**不**调 `decompile`
    ///
    /// 拆出来是为了让 [`Self::decompile_artifact_with`] 能在"真反编译"之前先问一句
    /// [`WorkDecompiler::editable_document`](NEMO:只要文档、不要资源目录)。
    fn prepare_decompile(
        &self,
        work_id: WorkId,
        options: &DecompileOptions,
    ) -> Result<(Box<dyn WorkDecompiler>, RawWorkData, DecompilerContext)> {
        info!("开始反编译作品 [work_id={}]", work_id);
        let http_client = Box::new(CodeMaoHttpClient::new(self.client.clone()));
        let work_info = self
            .fetch_work_info(&*http_client, work_id)
            .with_context(|| format!("获取作品 {} 信息失败", work_id))?;

        let fetcher = self
            .registry
            .fetcher_for(
                &work_info.work_type,
                http_client.clone(),
                self.config.clone(),
            )
            .with_context(|| format!("不支持的{}作品类型", work_id))?;
        let decompiler = self
            .registry
            .decompiler_for(&work_info.work_type, &self.config)
            .with_context(|| format!("不支持的{}作品类型", work_id))?;
        let raw = fetcher
            .fetch(&work_info)
            .with_context(|| format!("获取作品 {} 原始数据失败", work_id))?;

        // 确定输出目录(用户指定或默认)
        let output_path = options
            .output_dir
            .as_deref()
            .unwrap_or(&self.config.default_output_dir);

        // 可选:保存原始数据到 raw/ 子目录
        if options.save_raw {
            self.save_raw_data(&work_info, &raw, output_path)
                .with_context(|| format!("保存作品 {} 原始数据失败", work_id))?;
        }

        let context = DecompilerContextBuilder::new()
            .output_dir(output_path)
            .resources(options.resource_concurrency, !options.skip_resources)
            .work_info(work_info)
            .http_client(http_client)
            .config(self.config.clone())
            .id_generator(self.id_generator.clone())
            .build()?;

        Ok((decompiler, raw, context))
    }

    /// 将获取到的未编译原始数据保存到 `output_dir/raw/` 目录下
    /// 文件名格式为 `raw-{作品名称}.{扩展名}`,其中名称经过安全过滤
    fn save_raw_data(
        &self,
        work_info: &WorkInfo,
        raw: &RawWorkData,
        output_dir: &Path,
    ) -> Result<PathBuf> {
        let raw_dir = output_dir.join("raw");
        std::fs::create_dir_all(&raw_dir)?;
        // 安全的基础文件名,不含扩展名
        let base_name = format!(
            "raw-{}",
            FileService::safe_filename(&work_info.name, work_info.id.get(), "")
        );
        match raw {
            RawWorkData::Kitten(data) | RawWorkData::Coco(data) | RawWorkData::Wood(data) => {
                let filename = format!("{}.json", base_name);
                let path = raw_dir.join(filename);
                FileService::write_json(&path, data)?;
                Ok(path)
            }
            RawWorkData::NekoEncrypted(s) => {
                let filename = format!("{}.txt", base_name);
                let path = raw_dir.join(filename);
                std::fs::write(&path, s)?;
                Ok(path)
            }
            RawWorkData::Nemo(bcm, src) => {
                let bcm_filename = format!("{}.bcm.json", base_name);
                let src_filename = format!("{}.src.json", base_name);
                let bcm_path = raw_dir.join(bcm_filename);
                let src_path = raw_dir.join(src_filename);
                FileService::write_json(&bcm_path, bcm)?;
                FileService::write_json(&src_path, src)?;
                // 返回主文件(bcm)的路径
                Ok(bcm_path)
            }
        }
    }
    fn fetch_work_info(&self, http_client: &dyn HttpClient, work_id: WorkId) -> Result<WorkInfo> {
        let url = format!(
            "{}/creation-tools/v1/works/{}",
            self.config.base_url, work_id
        );
        let data = http_client.get_json(&url, None)?;
        WorkInfo::from_api_response(&data)
    }
}

/// 便捷反编译函数:使用全局单例门面(复用 HTTP 客户端),功能与之前一致
/// `output_dir` 传 `None` 时写入 `default_output_dir`,返回产物文件路径;自定义客户端请用 `CodemaoDecompiler::new(client)`
pub fn decompile_work(work_id: WorkId, output_dir: Option<&Path>) -> Result<PathBuf> {
    CodemaoDecompiler::global().decompile(work_id, output_dir)
}

/// 便捷反编译函数:使用自定义选项;自定义客户端请用 `CodemaoDecompiler::new(client)`
pub fn decompile_work_with(work_id: WorkId, options: DecompileOptions) -> Result<PathBuf> {
    CodemaoDecompiler::global().decompile_with_options(work_id, options)
}

/// 便捷批量反编译函数:返回与输入顺序一致的 `Vec<Result<PathBuf>>`;自定义客户端请用 `CodemaoDecompiler::new(client)`
pub fn decompile_works(work_ids: &[WorkId], options: DecompileOptions) -> Vec<Result<PathBuf>> {
    CodemaoDecompiler::global().decompile_batch(work_ids, options)
}


// ===========================================================================
// 资源并发下载(原串行逐文件下载是 NEMO 反编译的瓶颈)
// ===========================================================================

/// 一个待下载资源
pub(crate) struct ResourceTask {
    pub(crate) url: String,
    /// 落盘路径(内容寻址命名由调用方决定)
    pub(crate) dest: PathBuf,
}

/// 并发下载一组资源,返回失败清单(不中断整体流程)。
///
/// 为什么需要:实测 NEMO 作品(work 194684070)有 **1 390 个** 造型文件、共 49 MB,
/// 串行下载 ≈ 8 分钟 —— 总耗时几乎全在"请求数 × RTT"上,而不是带宽。
/// 这里用固定线程数消费共享队列(不新增依赖),并做两件与之配套的事:
///
/// - **按 url 去重**:同一 url 在多个造型/素材里复用时只下一次;
/// - **跳过已存在且非空的文件**:重跑/断点续传接近零成本(内容寻址命名下同名即同内容)。
///
/// 资源下载的**总线程预算**(作品级 × 单作品资源级的上限)。
///
/// 取值依据:`docs/rounds/22` §3 的实测曲线(8 = 103s、16 = 92s、32 = 127s 且 CDN 限流丢文件),
/// 最优区间 8–16;下载耗时几乎全在请求数 × RTT,与核数无关,所以这里是**固定常量**。
const RESOURCE_DOWNLOAD_BUDGET: usize = 16;

/// `concurrency` 来自 [`DecompileOptions::resource_concurrency`]。
pub(crate) fn download_resources_parallel(
    client: &dyn HttpClient,
    tasks: Vec<ResourceTask>,
    concurrency: usize,
) -> Vec<String> {
    use std::sync::Mutex;
    use std::sync::atomic::{AtomicUsize, Ordering};

    let concurrency = concurrency.max(1);
    let total = tasks.len();
    if total == 0 {
        return Vec::new();
    }
    let started = std::time::Instant::now();
    // 先过滤:已存在且非空的不再请求
    let pending: Vec<&ResourceTask> = tasks
        .iter()
        .filter(|task| {
            !task
                .dest
                .metadata()
                .map(|m| m.is_file() && m.len() > 0)
                .unwrap_or(false)
        })
        .collect();
    let skipped = total - pending.len();
    if !pending.is_empty() {
        info!(
            "资源下载:共 {total} 个(跳过已存在 {skipped}),并发 {concurrency}"
        );
    }

    let cursor = AtomicUsize::new(0);
    let failures: Mutex<Vec<String>> = Mutex::new(Vec::new());
    let done = AtomicUsize::new(0);
    let workers = concurrency.min(pending.len().max(1));
    std::thread::scope(|scope| {
        for _ in 0..workers {
            scope.spawn(|| {
                loop {
                    let index = cursor.fetch_add(1, Ordering::Relaxed);
                    let Some(task) = pending.get(index) else {
                        return;
                    };
                    match client.get_binary(&task.url) {
                        Ok(data) => {
                            if let Err(error) = FileService::write_binary(&task.dest, &data) {
                                failures.lock().unwrap().push(format!("{}: {}", task.url, error));
                            }
                        }
                        Err(error) => {
                            failures.lock().unwrap().push(format!("{}: {}", task.url, error));
                        }
                    }
                    let finished = done.fetch_add(1, Ordering::Relaxed) + 1;
                    if finished.is_multiple_of(200) || finished == pending.len() {
                        info!(
                            "资源下载进度 {finished}/{} (用时 {:.1?})",
                            pending.len(),
                            started.elapsed()
                        );
                    }
                }
            });
        }
    });
    let mut failures = failures.into_inner().unwrap_or_default();
    // 失败重试:高并发下 CDN 会限流/超时(实测 32 线程丢 2 个文件),
    // 失败的串行重试一轮即可补回 —— 比"直接降低并发"更划算,也不至于静默缺文件。
    if !failures.is_empty() {
        warn!("资源下载失败 {} 个,串行重试", failures.len());
        let mut retried = Vec::new();
        for line in &failures {
            let url = line.split(": ").next().unwrap_or_default();
            let Some(task) = pending.iter().find(|task| task.url == url) else {
                retried.push(line.clone());
                continue;
            };
            match client.get_binary(&task.url) {
                Ok(data) => {
                    if let Err(error) = FileService::write_binary(&task.dest, &data) {
                        retried.push(format!("{}: {}", task.url, error));
                    }
                }
                Err(error) => retried.push(format!("{}: {}", task.url, error)),
            }
        }
        failures = retried;
    }
    info!(
        "资源下载完成:{} 个(跳过 {skipped},失败 {})用时 {:.1?}",
        total,
        failures.len(),
        started.elapsed()
    );
    failures
}

// ===========================================================================
// 反编译上下文(原 context.rs)
// ===========================================================================
pub(crate) struct DecompilerContext {
    /// 本次调用的输出目录(`DecompileOptions::output_dir`),供**自建目录树**的反编译器
    /// (NEMO/WOOD)与 `save_result` 用同一个落点;`None` 时回退 `config.default_output_dir`
    pub(crate) output_dir: Option<PathBuf>,
    /// 资源下载并发数(见 [`DecompileOptions::resource_concurrency`])
    pub(crate) resource_concurrency: usize,
    /// 是否下载资源(见 [`DecompileOptions::skip_resources`])
    pub(crate) download_resources: bool,
    pub(crate) work_info: WorkInfo,
    pub(crate) http_client: Box<dyn HttpClient>,
    pub(crate) file_service: FileService,
    pub(crate) id_generator: IdGenerator,
    pub(crate) config: Arc<DecompilerConfig>,
}

// Context Builder
pub(crate) struct DecompilerContextBuilder {
    output_dir: Option<PathBuf>,
    resource_concurrency: usize,
    download_resources: bool,
    work_info: Option<WorkInfo>,
    http_client: Option<Box<dyn HttpClient>>,
    config: Option<Arc<DecompilerConfig>>,
    id_generator: Option<IdGenerator>,
}

impl Default for DecompilerContextBuilder {
    fn default() -> Self {
        Self::new()
    }
}

impl DecompilerContextBuilder {
    pub(crate) fn new() -> Self {
        Self {
            output_dir: None,
            resource_concurrency: 8,
            download_resources: true,
            work_info: None,
            http_client: None,
            config: None,
            id_generator: None,
        }
    }

    /// 资源下载并发数 / 是否下载资源(见 [`DecompileOptions`])
    pub(crate) fn resources(mut self, concurrency: usize, download: bool) -> Self {
        self.resource_concurrency = concurrency.max(1);
        self.download_resources = download;
        self
    }

    /// 本次调用的输出目录(自建目录树的反编译器用它当根)
    pub(crate) fn output_dir(mut self, dir: impl Into<PathBuf>) -> Self {
        self.output_dir = Some(dir.into());
        self
    }

    pub(crate) fn work_info(mut self, info: WorkInfo) -> Self {
        self.work_info = Some(info);
        self
    }

    pub(crate) fn http_client(mut self, client: Box<dyn HttpClient>) -> Self {
        self.http_client = Some(client);
        self
    }

    pub(crate) fn config(mut self, config: Arc<DecompilerConfig>) -> Self {
        self.config = Some(config);
        self
    }

    pub(crate) fn id_generator(mut self, generator: IdGenerator) -> Self {
        self.id_generator = Some(generator);
        self
    }

    pub(crate) fn build(self) -> Result<DecompilerContext> {
        let config = self.config.unwrap_or_default();
        Ok(DecompilerContext {
            output_dir: self.output_dir,
            resource_concurrency: self.resource_concurrency,
            download_resources: self.download_resources,
            work_info: self.work_info.ok_or_else(|| DecompilerError::Other {
                msg: "缺少work_info".into(),
                source: None,
            })?,
            http_client: self.http_client.ok_or_else(|| DecompilerError::Other {
                msg: "缺少http_client".into(),
                source: None,
            })?,
            file_service: FileService::new(config.clone()),
            id_generator: self.id_generator.unwrap_or_default(),
            config,
        })
    }
}

// ===========================================================================
// 反编译契约(原 contract.rs)
// ===========================================================================
// 结果类型与 Trait
#[derive(Debug)]
pub(crate) enum DecompileResult {
    Json(Value),
    Path(String),
}

/// 反编译产物(内存形态,方案 23 P0-2)
///
/// 与 `DecompileResult` 的区别:这是**对外**的产物描述(Kitten/NEKO 直接在内存里,
/// 不再"先落盘再读回"),`DecompiledArtifact::Path` 用于只有落盘形态的 NEMO/WOOD。
#[derive(Debug)]
pub enum DecompiledArtifact {
    /// 编辑版文档(Kitten/NEKO/NEMO):内容在内存里,**未落盘**
    Document {
        /// 编辑版文档
        document: Value,
        /// 该文档落盘时应当使用的文件名(含扩展名,与 `save_result` 一致)
        file_name: String,
        /// 源作品的 `bcm_version`(作品元信息里的;取不到时为空串)。
        ///
        /// 只有 NEMO 方向用得到:老作品的转化要做版本迁移(见 `docs/rounds/27` §9.3),
        /// 而编辑版文档里**没有**版本号,只能靠元信息带过来。
        source_version: String,
    },
    /// 只有落盘形态的产物(NEMO/WOOD):文件或资源目录
    Path(PathBuf),
}

/// 反编译器的**内存形态编辑版**(只有需要的反编译器实现;见 [`WorkDecompiler::editable_document`])
#[derive(Debug)]
pub(crate) struct EditableDocument {
    /// 编辑版文档(NEMO 是明文 JSON,无解密步骤)
    pub(crate) document: Value,
    /// 源作品的 `bcm_version`(作品元信息里的;取不到时为空串)
    pub(crate) source_version: String,
}

pub(crate) trait WorkDecompiler: Send + Sync {
    fn decompile(&self, raw: RawWorkData, context: &DecompilerContext) -> Result<DecompileResult>;
    fn save_result(
        &self,
        result: &DecompileResult,
        output_dir: Option<&Path>,
        context: &DecompilerContext,
    ) -> Result<PathBuf>;

    /// 内存形态的编辑版(**默认没有**):给"只要文档、不要资源目录"的调用方(如 `translate`)用。
    ///
    /// NEMO 的产物本来是"资源目录"(`.bcm` + `.userimg` + `.meta` + 下载的素材),
    /// 但互相转化只需要那份明文编辑版 JSON —— 这条入口让 `translate_work` 全程内存直通
    /// (方案 23 P0-2 的同一思路),顺带省掉一次资源下载。
    fn editable_document(
        &self,
        _raw: &RawWorkData,
        _context: &DecompilerContext,
    ) -> Result<Option<EditableDocument>> {
        Ok(None)
    }
}

/// 将 JSON 反编译结果写入输出目录,返回文件路径(供各反编译器共用)
pub(crate) fn save_json_result(
    result: &DecompileResult,
    output_dir: Option<&Path>,
    context: &DecompilerContext,
    extension: &str,
    decompiler_name: &str,
) -> Result<PathBuf> {
    match result {
        DecompileResult::Json(json) => {
            let output_path = output_dir.unwrap_or(&context.config.default_output_dir);
            FileService::ensure_dir(output_path)?;
            let filename = FileService::safe_filename(
                &context.work_info.name,
                context.work_info.id.get(),
                extension,
            );
            let filepath = output_path.join(filename);
            FileService::write_json(&filepath, json)?;
            Ok(filepath)
        }
        _ => Err(DecompilerError::Decompile(format!(
            "{}反编译器应返回JSON",
            decompiler_name
        ))),
    }
}

/// 返回路径型反编译结果(供返回路径的反编译器共用)
pub(crate) fn save_path_result(result: &DecompileResult, decompiler_name: &str) -> Result<PathBuf> {
    match result {
        DecompileResult::Path(path) => Ok(PathBuf::from(path)),
        _ => Err(DecompilerError::Decompile(format!(
            "{}反编译器应返回路径",
            decompiler_name
        ))),
    }
}
