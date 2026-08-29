use crate::core::unpacker::{
    CodeMaoHttpClient, CocoDecompiler, CocoFetcher, DecompilerConfig, DecompilerContextBuilder,
    FileService, HttpClient, IdGenerator, KittenDecompiler, KittenFetcher, NekoDecompiler,
    NekoFetcher, NemoDecompiler, NemoFetcher, RawWorkData, Result, ResultExt, WoodDecompiler,
    WoodFetcher, WorkDecompiler, WorkFetcher, WorkInfo, WorkType,
};
pub use crate::core::unpacker::DecompilerError;
use crate::utils::requests::CodeMaoClient;
use log::info;
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
    fetchers: HashMap<WorkType, FetcherFactory>,
    decompilers: HashMap<WorkType, DecompilerFactory>,
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
        work_type: WorkType,
        fetcher: FetcherFactory,
        decompiler: DecompilerFactory,
    ) {
        self.fetchers.insert(work_type, fetcher);
        self.decompilers.insert(work_type, decompiler);
    }

    /// 按作品类型创建 fetcher
    pub(crate) fn fetcher_for(
        &self,
        work_type: &WorkType,
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
        work_type: &WorkType,
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
        for wt in [WorkType::Kitten2, WorkType::Kitten3, WorkType::Kitten4] {
            registry.register(
                wt,
                Box::new(|client, config| Box::new(KittenFetcher::new(client, config))),
                Box::new(|_| Box::new(KittenDecompiler)),
            );
        }
        registry.register(
            WorkType::Neko,
            Box::new(|client, config| Box::new(NekoFetcher::new(client, config))),
            Box::new(|config| Box::new(NekoDecompiler::new(config.crypto_salt.as_slice()))),
        );
        registry.register(
            WorkType::Nemo,
            Box::new(|client, config| Box::new(NemoFetcher::new(client, config))),
            Box::new(|_| Box::new(NemoDecompiler)),
        );
        registry.register(
            WorkType::Wood,
            Box::new(|client, config| Box::new(WoodFetcher::new(client, config))),
            Box::new(|_| Box::new(WoodDecompiler)),
        );
        registry.register(
            WorkType::Coco,
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
    pub fn decompile(&self, work_id: i64, output_dir: Option<&Path>) -> Result<PathBuf> {
        let mut options = DecompileOptions::new();
        if let Some(dir) = output_dir {
            options = options.output_dir(dir.to_path_buf());
        }
        self.decompile_with_options(work_id, options)
    }

    /// 使用自定义选项反编译单个作品
    pub fn decompile_with_options(
        &self,
        work_id: i64,
        options: DecompileOptions,
    ) -> Result<PathBuf> {
        self.decompile_inner(work_id, &options)
    }

    /// 批处理反编译多个作品,返回与输入顺序一致的 `Vec<Result>`
    pub fn decompile_batch(
        &self,
        work_ids: &[i64],
        options: DecompileOptions,
    ) -> Vec<Result<PathBuf>> {
        let concurrency = options.batch_concurrency.max(1);
        if concurrency == 1 || work_ids.len() <= 1 {
            return work_ids
                .iter()
                .map(|&id| self.decompile_inner(id, &options))
                .collect();
        }
        // 按并发数分块,块内并发执行(thread::scope),块间顺序收集保持结果顺序
        let options_ref = &options;
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

    /// 反编译主流程(模板方法)
    /// 流程为:获取信息 → 创建处理器 → 取原始数据 → (可选)保存原始数据 → 反编译 → 保存结果
    fn decompile_inner(&self, work_id: i64, options: &DecompileOptions) -> Result<PathBuf> {
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
            .work_info(work_info)
            .http_client(http_client)
            .config(self.config.clone())
            .id_generator(self.id_generator.clone())
            .build()?;

        let result = decompiler.decompile(raw, &context)?;
        let saved = decompiler.save_result(&result, Some(output_path), &context)?;
        info!(
            "作品 [work_id={}] 反编译完成,保存至: {}",
            work_id,
            saved.display()
        );
        Ok(saved)
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
            FileService::safe_filename(&work_info.name, work_info.id, "")
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
    fn fetch_work_info(&self, http_client: &dyn HttpClient, work_id: i64) -> Result<WorkInfo> {
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
pub fn decompile_work(work_id: i64, output_dir: Option<&Path>) -> Result<PathBuf> {
    CodemaoDecompiler::global().decompile(work_id, output_dir)
}

/// 便捷反编译函数:使用自定义选项;自定义客户端请用 `CodemaoDecompiler::new(client)`
pub fn decompile_work_with(work_id: i64, options: DecompileOptions) -> Result<PathBuf> {
    CodemaoDecompiler::global().decompile_with_options(work_id, options)
}

/// 便捷批量反编译函数:返回与输入顺序一致的 `Vec<Result<PathBuf>>`;自定义客户端请用 `CodemaoDecompiler::new(client)`
pub fn decompile_works(work_ids: &[i64], options: DecompileOptions) -> Vec<Result<PathBuf>> {
    CodemaoDecompiler::global().decompile_batch(work_ids, options)
}
