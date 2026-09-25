use crate::core::convert::decompile::editors::{
    CocoDecompiler, CocoFetcher, KittenDecompiler, KittenFetcher, NekoDecompiler, NekoFetcher,
    NemoDecompiler, NemoFetcher, WoodDecompiler, WoodFetcher,
};
pub(crate) mod editors;

use crate::core::convert::shared::ShadowBuilder;
use crate::core::convert::shared::ValueExt;
use crate::core::convert::shared::{
    CodeMaoHttpClient, DecompilerConfig, DraftUpload, EditorType, FileService, HttpClient,
    IdGenerator, RawWorkData, Result, ResultExt, WorkFetcher, WorkInfo, batch_map, create_draft,
    supports_account_upload,
};
use crate::core::convert::shared::{DecompilerError, WorkId};
use crate::utils::requests::{CodeMaoClient, MewError};
use log::error;
use log::{debug, info, warn};
use serde_json::Value;
use serde_json::json;
use std::collections::HashMap;
use std::collections::HashSet;
use std::fmt::Write as _;
use std::path::{Path, PathBuf};
use std::sync::{Arc, OnceLock};

// 来自 src/core/convert/decompile/mod.rs

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
    /// 是否把产物上传到**当前账号**并建一份同名草稿(默认 false)
    ///
    /// 开 = 替用户在平台落一份草稿(等同发布动作,需调用方明确授权);产物名形如
    /// `反编译副本 KittenN ← 330773110(可删)`。仅 Kitten4 / KittenN / NEMO 有已知的
    /// 建作品端点,其余类型会明确报错而不是静默跳过。
    upload_to_account: bool,
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
            upload_to_account: false,
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

    /// 把产物上传到当前账号并建一份同名草稿(默认 false;开 = 替用户发布动作,需明确授权)
    pub fn upload_to_account(mut self, on: bool) -> Self {
        self.upload_to_account = on;
        self
    }

    /// 是否上传到账号
    pub(crate) fn uploads_to_account(&self) -> bool {
        self.upload_to_account
    }
}

// 作品类型 → 处理器(静态分派)
//
// 这里原本是一张 `HashMap<EditorType, Box<dyn Fn(..) -> Box<dyn ..>>>` 注册表(工厂模式)。
// 编辑器集合是**固定的 7 种**(由 `EditorType` 枚举穷尽),没有外部注册方,注册表只带来两层
// 动态分派与两个工厂类型别名 ⇒ 换成 `match`,编译期穷尽性同时成为"新编辑器必须接线"的检查。
// 作品类型集合见 `shared.rs::EditorType`(见 `docs/rounds/31` §3 A2)。

/// 按作品类型构造抓取器
fn fetcher_for(
    work_type: EditorType,
    client: Box<dyn HttpClient>,
    config: Arc<DecompilerConfig>,
) -> Box<dyn WorkFetcher> {
    match work_type {
        // Kitten2/3/4 共用同一套抓取与反编译
        EditorType::Kitten2 | EditorType::Kitten3 | EditorType::Kitten4 => {
            Box::new(KittenFetcher::new(client, config))
        }
        EditorType::Neko => Box::new(NekoFetcher::new(client, config)),
        EditorType::Nemo => Box::new(NemoFetcher::new(client, config)),
        EditorType::Wood => Box::new(WoodFetcher::new(client, config)),
        EditorType::Coco => Box::new(CocoFetcher::new(client, config)),
    }
}

/// 按作品类型构造反编译器
fn decompiler_for(
    work_type: EditorType,
    config: &Arc<DecompilerConfig>,
) -> Box<dyn WorkDecompiler> {
    match work_type {
        EditorType::Kitten2 | EditorType::Kitten3 | EditorType::Kitten4 => {
            Box::new(KittenDecompiler)
        }
        EditorType::Neko => Box::new(NekoDecompiler::new(config.crypto_salt.as_slice())),
        EditorType::Nemo => Box::new(NemoDecompiler),
        EditorType::Wood => Box::new(WoodDecompiler),
        EditorType::Coco => Box::new(CocoDecompiler),
    }
}

/// 反编译结果:产物路径 + 编辑器 + (开启了「上传到账号」时的)新作品 id
#[derive(Debug, Clone)]
pub struct DecompileOutcome {
    /// 产物文件路径(反编译产物,落盘位置由 `output_dir` 决定)
    pub artifact: PathBuf,
    /// 新作品 id;仅当 [`DecompileOptions::upload_to_account`] 为真时存在
    pub work_id: Option<i64>,
    /// 产物对应的编辑器(决定它是哪种作品文件)
    pub editor: EditorType,
}

// 主入口
pub struct CodemaoDecompiler {
    config: Arc<DecompilerConfig>,
    client: Arc<CodeMaoClient>,
    id_generator: IdGenerator,
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

    /// 使用自定义选项反编译单个作品,只返回产物路径
    pub fn decompile_with_options(
        &self,
        work_id: WorkId,
        options: DecompileOptions,
    ) -> Result<PathBuf> {
        Ok(self.decompile_outcome(work_id, options)?.artifact)
    }

    /// 使用自定义选项反编译单个作品,返回产物路径、编辑器,以及开启了
    /// [`DecompileOptions::upload_to_account`] 时新建的草稿作品 id
    pub fn decompile_outcome(
        &self,
        work_id: WorkId,
        options: DecompileOptions,
    ) -> Result<DecompileOutcome> {
        self.decompile_inner(work_id, &options)
    }

    /// 批处理反编译多个作品,返回与输入顺序一致的 `Vec<Result>`(只含产物路径)
    pub fn decompile_batch(
        &self,
        work_ids: &[WorkId],
        options: DecompileOptions,
    ) -> Vec<Result<PathBuf>> {
        self.decompile_batch_outcomes(work_ids, options)
            .into_iter()
            .map(|result| result.map(|outcome| outcome.artifact))
            .collect()
    }

    /// 批处理反编译多个作品,返回与输入顺序一致的结果(含新作品 id,若开启「上传到账号」)
    pub fn decompile_batch_outcomes(
        &self,
        work_ids: &[WorkId],
        options: DecompileOptions,
    ) -> Vec<Result<DecompileOutcome>> {
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
        let options_ref = &per_work;
        batch_map(
            work_ids,
            concurrency,
            |&id| self.decompile_inner(id, options_ref),
            || DecompilerError::Other {
                msg: "反编译线程异常".to_string(),
                source: None,
            },
        )
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
    /// → (可选)**上传到账号**
    fn decompile_inner(
        &self,
        work_id: WorkId,
        options: &DecompileOptions,
    ) -> Result<DecompileOutcome> {
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

        // 可选:把产物原样建到当前账号(备份/搬家);默认关,开=在平台落一份草稿
        let new_work_id = if options.uploads_to_account() {
            Some(self.upload_to_account(work_id, &context, &saved)?)
        } else {
            None
        };

        Ok(DecompileOutcome {
            artifact: saved,
            work_id: new_work_id,
            editor: context.work_info.work_type,
        })
    }

    /// 把反编译产物上传到当前账号并建一份同名草稿,返回新作品 id
    ///
    /// 产物就是刚才落盘的那一份(与反编译结果逐字节相同),不做资源重传;
    /// 上传渠道按编辑器选(NEMO 走 `nemo_android_ios`),建作品调用见 `shared::upload`。
    fn upload_to_account(
        &self,
        work_id: WorkId,
        context: &DecompilerContext,
        artifact: &Path,
    ) -> Result<i64> {
        let editor = context.work_info.work_type;
        if !supports_account_upload(editor) {
            return Err(DecompilerError::Mew(MewError::InvalidArgument(format!(
                "{editor:?} 没有已知的建作品端点,不能上传到账号\
                 (支持 Kitten4 / KittenN / NEMO;Coco / Wood / Kitten2 / Kitten3 只能反编译到本地)"
            ))));
        }
        info!("作品 [work_id={}] 上传产物到当前账号…", work_id);
        let spec = DraftUpload {
            artifact,
            editor,
            source_work_id: work_id,
            kind: "反编译",
            save_path: "decompile-backup",
            bcm_version: &context.work_info.bcm_version,
            n_blocks: None,
        };
        let new_work_id = create_draft(self.client.as_ref(), &spec)?;
        info!("已建草稿作品 id={new_work_id}(名称含「可删」)");
        Ok(new_work_id)
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

        // 静态分派(见上方 `fetcher_for` / `decompiler_for`):7 种类型穷尽匹配,没有"不支持"分支
        let fetcher = fetcher_for(
            work_info.work_type,
            http_client.clone(),
            self.config.clone(),
        );
        let decompiler = decompiler_for(work_info.work_type, &self.config);
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

        // 直接构造(原先套了一层 builder:7 个 Option 字段 + 6 个 setter + 一个零调用的
        // `Default`,只服务这一处构造 ⇒ 纯仪式,见 `docs/rounds/31` §3.6 骨架评审)
        let config = self.config.clone();
        let context = DecompilerContext {
            output_dir: Some(output_path.to_path_buf()),
            resource_concurrency: options.resource_concurrency.max(1),
            download_resources: !options.skip_resources,
            work_info,
            http_client,
            file_service: FileService::new(config.clone()),
            id_generator: self.id_generator.clone(),
            config,
        };

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
        info!("资源下载:共 {total} 个(跳过已存在 {skipped}),并发 {concurrency}");
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
                                failures
                                    .lock()
                                    .unwrap()
                                    .push(format!("{}: {}", task.url, error));
                            }
                        }
                        Err(error) => {
                            failures
                                .lock()
                                .unwrap()
                                .push(format!("{}: {}", task.url, error));
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

// 来自 src/core/convert/decompile/blocks.rs
// 积木反编译:编译版积木树 → 编辑版 `block_data_json`。
// 三件事在同一文件里(原 `blocks/{mod,core,special}.rs`,合并原因:它们是同一个状态机):
// - **插槽命名**(`child_input_name`)与**根块判定**(`referenced_ids`):规则唯一来源,
// 与 Kitten2/3 的 blocksXML 序列化共用;
// - **骨架**(`BlockDecompilerCore`):`next`/`child_block`/`conditions`/`params` 的递归展开、
// 自动布局、连接表与 `parent_id` 维护;
// - **专用反编译器**(`If`/`FunctionDef`/`FunctionCall`/`TextJoin`/`Mutation` 等)与
// 按编译版 `type` 分派的 `create_block_decompiler`。

/// 编译版 `child_block` → 编辑版语句插槽名。
///
/// 唯一的规则来源:反编译重建(本模块)与 Kitten2/3 的 blocksXML 序列化
/// ([`crate::core::convert::decompile::editors::kitten::XmlBlockWriter`])共用,
/// 避免两处各写一份后漂移。
pub(crate) fn child_input_name(block_type: &str, index: usize, conditions_count: usize) -> String {
    match block_type {
        "controls_if" | "controls_if_no_else" => {
            if index < conditions_count {
                // 编辑版插槽名为 DO0/DO1/...(无空格)
                format!("DO{}", index)
            } else {
                // 编辑版 else 分支插槽名为 ELSE(无编号)
                "ELSE".to_string()
            }
        }
        // 函数定义块的函数体插槽名为 STACK
        "procedures_2_defnoreturn" => "STACK".to_string(),
        _ => "DO".to_string(),
    }
}

// 积木上下文
#[derive(Clone)]
pub(crate) struct BlockContext {
    pub(crate) actor_data: Value,
    pub(crate) functions: Arc<HashMap<String, Value>>,
    pub(crate) variable_map: Arc<HashMap<String, String>>, // UUID -> 变量名
    pub(crate) shadow_builder: ShadowBuilder,
    pub(crate) blocks: HashMap<String, Value>,
    pub(crate) connections: HashMap<String, HashMap<String, Value>>,
    // 布局游标:编译版无 location 时按树形自动排列积木,避免恢复产物全部重叠在 [0,0]
    pub(crate) layout_col: f64,
    pub(crate) layout_row: f64,
}

impl BlockContext {
    pub(crate) fn new(
        actor_data: Value,
        functions: Arc<HashMap<String, Value>>,
        shadow_builder: ShadowBuilder,
        variable_map: Arc<HashMap<String, String>>,
    ) -> Self {
        Self {
            actor_data,
            functions,
            variable_map,
            shadow_builder,
            blocks: HashMap::new(),
            connections: HashMap::new(),
            layout_col: 0.0,
            layout_row: 0.0,
        }
    }

    pub(crate) fn with_capacity(
        actor_data: Value,
        functions: Arc<HashMap<String, Value>>,
        shadow_builder: ShadowBuilder,
        variable_map: Arc<HashMap<String, String>>,
        blocks_cap: usize,
        connections_cap: usize,
    ) -> Self {
        Self {
            actor_data,
            functions,
            variable_map,
            shadow_builder,
            blocks: HashMap::with_capacity(blocks_cap),
            connections: HashMap::with_capacity(connections_cap),
            layout_col: 0.0,
            layout_row: 0.0,
        }
    }

    pub(crate) fn insert_connection(
        &mut self,
        source_id: &str,
        target_id: &str,
        connection_info: Value,
    ) {
        self.connections
            .entry(source_id.to_string())
            .or_default()
            .insert(target_id.to_string(), connection_info);
    }
}

/// 编译版块表里「被引用过」的块 id(`next_block`/`child_block`/`conditions`/`params`)。
///
/// 编译版的引用**恒为内联对象**(Kitten2/3/4 的 `compiled_block_map` 实测 2 236 处采样
/// 全是对象,见 `docs/rounds/21-convert-domain-consolidation-plan.md` §7-1);字符串 id 只出现在
/// **编辑版** `block_data_json` 的 `connections`。真出现字符串说明编译格式漂移了 ——
/// 这里显式报错,而不是静默漏掉引用(那会把子块当成根块、产物多出一堆散块)。
pub(crate) fn referenced_ids(blocks: &serde_json::Map<String, Value>) -> Result<HashSet<String>> {
    fn id_of(value: &Value) -> Option<&str> {
        value.get("id").and_then(Value::as_str)
    }
    fn reject_string(block_id: &str, field: &str) -> DecompilerError {
        DecompilerError::InvalidResponse(format!(
            "块 {block_id} 的 {field} 是字符串 id:编译版引用恒为内联对象(见 docs/rounds/21 §7-1)"
        ))
    }

    let mut referenced: HashSet<String> = HashSet::new();
    for (block_id, block) in blocks {
        if let Some(next) = block.get("next_block") {
            if next.is_string() {
                return Err(reject_string(block_id, "next_block"));
            }
            if let Some(id) = id_of(next) {
                referenced.insert(id.to_string());
            }
        }
        for field in ["child_block", "conditions"] {
            let Some(items) = block.get(field).and_then(Value::as_array) else {
                continue;
            };
            for item in items {
                if item.is_string() {
                    return Err(reject_string(block_id, field));
                }
                if let Some(id) = id_of(item) {
                    referenced.insert(id.to_string());
                }
            }
        }
        // `params` 的值允许是标量(非引用一律忽略),只有内联对象才算引用
        if let Some(params) = block.get("params").and_then(Value::as_object) {
            for param_value in params.values() {
                if let Some(id) = id_of(param_value) {
                    referenced.insert(id.to_string());
                }
            }
        }
    }
    Ok(referenced)
}

// ===========================================================================
// 反编译骨架(原 blocks/core.rs)
// ===========================================================================
// 积木反编译核心

pub(crate) struct BlockDecompilerCore<'a> {
    compiled: &'a Value,
}

impl<'a> BlockDecompilerCore<'a> {
    pub(crate) fn new(compiled: &'a Value) -> Self {
        Self { compiled }
    }

    pub(crate) fn decompile(&mut self, context: &mut BlockContext) -> Result<Value> {
        let config = &context.shadow_builder.config;
        let id = self.compiled.get_str_or("id", "");
        let block_type = self.compiled.get_str_or("type", "");
        let is_shadow = config.shadow_types.contains(block_type);
        // 编辑版 is_output 与编译版 output_type 严格对应(0→false,2→true)
        let output_type = self.compiled.get_i64_or_default("output_type", 0);
        let is_output = is_shadow || output_type > 0;

        let location = self.compiled.get_array_opt("location").map_or_else(
            || {
                // 编译版无 location:按树形自动布局,避免全部重叠在 [0,0]
                let loc = json!([context.layout_col, context.layout_row]);
                context.layout_row += 70.0;
                loc
            },
            |arr| Value::Array(arr.clone()),
        );

        let mut block_value = json!({
            "id": id,
            "type": block_type,
            "location": location,
            "is_shadow": is_shadow,
            "is_output": is_output,
            "collapsed": false,
            "disabled": false,
            "parent_id": null,
            "deletable": true,
            "movable": true,
            "editable": true,
            "visible": "visible",
            "fields": {},
            "field_constraints": {},
            "field_extra_attr": {},
            "comment": self.compiled.get("comment").cloned().unwrap_or(Value::Null),
            "mutation": "",
        });

        let mut shadows: HashMap<String, Value> = HashMap::new();

        self.process_next(context, &mut block_value)?;
        self.process_children(context, &mut shadows, &mut block_value)?;
        self.process_conditions(context, &mut shadows, &mut block_value)?;
        self.process_params(context, &mut shadows, &mut block_value)?;

        if let Some(obj) = block_value.as_object_mut() {
            let shadows_map: serde_json::Map<String, Value> = shadows.into_iter().collect();
            obj.insert("shadows".to_string(), Value::Object(shadows_map));
        }

        // 不再在此处插入 blocks:所有调用方(process_next/children/conditions/params、
        // 顶层循环、FunctionCall 参数块)都会将返回值重新插入 context.blocks,
        // 原深克隆 + 哈希插入 + 丢弃每积木重复一次,属纯浪费
        Ok(block_value)
    }

    fn process_next(&self, context: &mut BlockContext, block_value: &mut Value) -> Result<()> {
        if let Some(next_compiled) = self.compiled.get("next_block")
            && !next_compiled.is_null()
        {
            let parent_id = block_value
                .get("id")
                .and_then(|v| v.as_str())
                .ok_or_else(|| DecompilerError::InvalidResponse("当前块缺少 id".to_string()))?
                .to_string();

            // 树内块也经专用分派,保证 callnoreturn/controls_if 等专用反编译器生效
            let mut decompiler = create_block_decompiler(next_compiled);
            // 下一层链块向右缩进一个层级
            context.layout_col += 220.0;
            let next_block = decompiler.decompile(context)?;
            context.layout_col -= 220.0;
            let next_id = next_block
                .get("id")
                .and_then(|v| v.as_str())
                .ok_or_else(|| DecompilerError::InvalidResponse("next_block缺少id".to_string()))?
                .to_string();
            context.blocks.insert(next_id.clone(), next_block);
            if let Some(b) = context.blocks.get_mut(&next_id)
                && let Some(o) = b.as_object_mut()
            {
                o.insert("parent_id".to_string(), json!(parent_id));
            }
            context.insert_connection(&parent_id, &next_id, json!({"type": "next"}));
        }
        Ok(())
    }

    fn process_children(
        &self,
        context: &mut BlockContext,
        shadows: &mut HashMap<String, Value>,
        block_value: &mut Value,
    ) -> Result<()> {
        if let Some(children) = self.compiled.get("child_block").and_then(|v| v.as_array()) {
            let conditions_count = self
                .compiled
                .get("conditions")
                .and_then(|v| v.as_array())
                .map_or(0, std::vec::Vec::len);

            let parent_id = block_value
                .get("id")
                .and_then(|v| v.as_str())
                .ok_or_else(|| DecompilerError::InvalidResponse("当前块缺少 id".to_string()))?
                .to_string();

            for (i, child) in children.iter().enumerate() {
                if !child.is_null() {
                    let mut decompiler = create_block_decompiler(child);
                    context.layout_col += 220.0;
                    let child_block = decompiler.decompile(context)?;
                    context.layout_col -= 220.0;
                    let child_id = child_block
                        .get("id")
                        .and_then(|v| v.as_str())
                        .ok_or_else(|| {
                            DecompilerError::InvalidResponse("child_block缺少id".to_string())
                        })?
                        .to_string();
                    let input_name =
                        child_input_name(self.compiled.get_str_or("type", ""), i, conditions_count);
                    context.blocks.insert(child_id.clone(), child_block);
                    if let Some(b) = context.blocks.get_mut(&child_id)
                        && let Some(o) = b.as_object_mut()
                    {
                        o.insert("parent_id".to_string(), json!(parent_id));
                    }
                    context.insert_connection(
                        &parent_id,
                        &child_id,
                        json!({
                            "type": "input",
                            "input_type": "statement",
                            "input_name": input_name
                        }),
                    );
                    if let std::collections::hash_map::Entry::Vacant(e) = shadows.entry(input_name)
                    {
                        let shadow_value = context.shadow_builder.create("logic_empty", None, None);
                        e.insert(shadow_value);
                    }
                }
            }
        }
        Ok(())
    }

    fn process_conditions(
        &self,
        context: &mut BlockContext,
        shadows: &mut HashMap<String, Value>,
        block_value: &mut Value,
    ) -> Result<()> {
        if let Some(conditions) = self.compiled.get("conditions").and_then(|v| v.as_array()) {
            let parent_id = block_value
                .get("id")
                .and_then(|v| v.as_str())
                .ok_or_else(|| DecompilerError::InvalidResponse("当前块缺少 id".to_string()))?
                .to_string();

            for (i, condition) in conditions.iter().enumerate() {
                let input_name = format!("IF{}", i);
                if condition.is_null() {
                    let shadow_value = context.shadow_builder.create("logic_empty", None, None);
                    shadows.insert(input_name, shadow_value);
                } else {
                    let mut decompiler = create_block_decompiler(condition);
                    context.layout_col += 220.0;
                    let condition_block = decompiler.decompile(context)?;
                    context.layout_col -= 220.0;
                    let cond_id = condition_block
                        .get("id")
                        .and_then(|v| v.as_str())
                        .ok_or_else(|| {
                            DecompilerError::InvalidResponse("condition_block缺少id".to_string())
                        })?
                        .to_string();
                    context.blocks.insert(cond_id.clone(), condition_block);
                    if let Some(b) = context.blocks.get_mut(&cond_id)
                        && let Some(o) = b.as_object_mut()
                    {
                        o.insert("parent_id".to_string(), json!(parent_id));
                    }
                    context.insert_connection(
                        &parent_id,
                        &cond_id,
                        json!({
                            "type": "input",
                            "input_type": "value",
                            "input_name": input_name
                        }),
                    );
                    let shadow_value = context.shadow_builder.create("logic_empty", None, None);
                    shadows.insert(input_name, shadow_value);
                }
            }
        }
        Ok(())
    }

    fn infer_shadow_type(&self, param_name: &str, value: &Value) -> &'static str {
        match param_name {
            "condition" | "BOOL" => "logic_empty",
            "message" | "MESSAGE" => "broadcast_input",
            "sound_id" | "SOUND" => "get_audios",
            "whole_sound" | "all_sounds" => "get_whole_audios",
            "style_id" | "costume" | "COSTUME" => "get_current_costume",
            "scene" | "SCENE" | "scene_id" => "get_current_scene",
            "list" | "LIST" => "lists_get",
            _ => match value {
                Value::String(_) => "text",
                Value::Bool(_) => "logic_boolean",
                _ => "math_number",
            },
        }
    }

    fn process_params(
        &self,
        context: &mut BlockContext,
        shadows: &mut HashMap<String, Value>,
        block_value: &mut Value,
    ) -> Result<()> {
        let block_type = self.compiled.get_str_or("type", "");
        // 过程定义/调用块的 params(参数名→参数块)由 FunctionDef/FunctionCallDecompiler
        // 单独处理,此处跳过避免双连接与 fields 污染
        if block_type == "procedures_2_defnoreturn"
            || block_type == "procedures_2_callnoreturn"
            || block_type == "procedures_2_callreturn"
        {
            return Ok(());
        }
        if let Some(params) = self.compiled.get_object_opt("params") {
            let parent_id = block_value
                .get("id")
                .and_then(|v| v.as_str())
                .ok_or_else(|| DecompilerError::InvalidResponse("当前块缺少 id".to_string()))?
                .to_string();

            for (name, value) in params {
                if value.is_object() {
                    let mut decompiler = create_block_decompiler(value);
                    context.layout_col += 220.0;
                    let param_block = decompiler.decompile(context)?;
                    context.layout_col -= 220.0;
                    // 类型名较短,转为拥有值以解除对 param_block 的借用,允许随后移动
                    let param_type = param_block
                        .get("type")
                        .and_then(|v| v.as_str())
                        .unwrap_or("")
                        .to_string();
                    let param_id = param_block
                        .get("id")
                        .and_then(|v| v.as_str())
                        .ok_or_else(|| {
                            DecompilerError::InvalidResponse("param_block缺少id".to_string())
                        })?
                        .to_string();
                    // 先取类型再移动:param_block 的 clone 仅为满足 insert 的取所有权,
                    // 移动后不再需要原值
                    context.blocks.insert(param_id.clone(), param_block);
                    if let Some(b) = context.blocks.get_mut(&param_id)
                        && let Some(o) = b.as_object_mut()
                    {
                        o.insert("parent_id".to_string(), json!(parent_id));
                    }
                    context.insert_connection(
                        &parent_id,
                        &param_id,
                        json!({
                            "type": "input",
                            "input_type": "value",
                            "input_name": name
                        }),
                    );
                    if context
                        .shadow_builder
                        .config
                        .shadow_types
                        .contains(&param_type)
                    {
                        // 编辑版 shadow 模板显示的是类型默认值(如 math_number 的 0),
                        // 与参数块实际值无关,因此不传 text
                        let shadow_value = context.shadow_builder.create(
                            &param_type,
                            Some(param_id.clone()),
                            None,
                        );
                        shadows.insert(name.clone(), shadow_value);
                    } else {
                        let shadow_type = self.infer_shadow_type(name, &Value::Null);
                        let shadow_value = context.shadow_builder.create(shadow_type, None, None);
                        shadows.insert(name.clone(), shadow_value);
                    }
                } else {
                    // 处理基本类型参数(如变量 UUID 引用)
                    // 布尔开关参数(如 bump 的 warp)在编辑版中不呈现
                    // (无 shadow,无 fields),跳过以对齐编辑版格式
                    if value.is_boolean() {
                        continue;
                    }
                    if name == "VAR" {
                        // 编辑版格式:变量引用以 UUID 存入 fields(variables_set/get 均如此),
                        // 且不生成 shadow(编辑版变量块的 shadows 中无 VAR 键)
                        if let Some(fields) = block_value
                            .as_object_mut()
                            .and_then(|v| v.get_mut("fields").and_then(|v| v.as_object_mut()))
                        {
                            fields.insert(name.clone(), value.clone());
                        }
                        continue;
                    }
                    let shadow_type = self.infer_shadow_type(name, value);
                    let num_str;
                    let shadow_text = match value {
                        Value::String(s) => Some(s.as_str()),
                        Value::Number(n) => {
                            num_str = n.to_string();
                            Some(num_str.as_str())
                        }
                        _ => None,
                    };
                    let shadow_value =
                        context
                            .shadow_builder
                            .create(shadow_type, None, shadow_text);
                    shadows.insert(name.clone(), shadow_value);

                    if let Some(fields) = block_value
                        .as_object_mut()
                        .and_then(|v| v.get_mut("fields").and_then(|v| v.as_object_mut()))
                    {
                        fields.insert(name.clone(), value.clone());
                    }
                }
            }
        }
        Ok(())
    }
}

// ===========================================================================
// 专用积木反编译器与分派(原 blocks/special.rs)
// ===========================================================================
// 反编译器上下文
// 反编译器上下文

// 积木反编译器 trait 与具体实现
pub(crate) trait BlockDecompiler<'a>: Send + Sync {
    fn decompile(&mut self, context: &mut BlockContext) -> Result<Value>;
}

pub(crate) struct DefaultBlockDecompiler<'a> {
    core: BlockDecompilerCore<'a>,
}

impl<'a> DefaultBlockDecompiler<'a> {
    pub(crate) fn new(compiled: &'a Value) -> Self {
        Self {
            core: BlockDecompilerCore::new(compiled),
        }
    }
}

impl<'a> BlockDecompiler<'a> for DefaultBlockDecompiler<'a> {
    fn decompile(&mut self, context: &mut BlockContext) -> Result<Value> {
        self.core.decompile(context)
    }
}

pub(crate) struct IfBlockDecompiler<'a> {
    core: BlockDecompilerCore<'a>,
    compiled: &'a Value,
}

impl<'a> IfBlockDecompiler<'a> {
    pub(crate) fn new(compiled: &'a Value) -> Self {
        let conditions_count = compiled
            .get("conditions")
            .and_then(|v| v.as_array())
            .map_or(0, std::vec::Vec::len);
        let core = BlockDecompilerCore::new(compiled);
        Self { core, compiled }
    }
}

impl<'a> BlockDecompiler<'a> for IfBlockDecompiler<'a> {
    fn decompile(&mut self, context: &mut BlockContext) -> Result<Value> {
        let mut block_value = self.core.decompile(context)?;
        let children = self
            .compiled
            .get("child_block")
            .and_then(|v| v.as_array())
            .ok_or_else(|| DecompilerError::Decompile("child_block不存在".to_string()))?;
        let conditions_len = self
            .compiled
            .get("conditions")
            .and_then(|v| v.as_array())
            .map_or(0, std::vec::Vec::len);

        // 根据方案8.1 修正 else 属性的判断
        let has_else = children.len() > conditions_len
            && !children.last().is_none_or(serde_json::Value::is_null);

        if let Some(obj) = block_value.as_object_mut() {
            let mut shadows_mut = obj.get_mut("shadows").and_then(|s| s.as_object_mut());
            if let Some(shadows) = shadows_mut.as_mut() {
                if has_else {
                    // 编辑版:有 else 时 shadows 同时含 ELSE_TEXT 与 ELSE
                    shadows.insert("ELSE_TEXT".to_string(), json!(""));
                    shadows.insert("ELSE".to_string(), json!(""));
                } else {
                    shadows.insert("EXTRA_ADD_ELSE".to_string(), json!(""));
                }
            }
            // 编辑版:有 else 时 mutation 标记 else="1",无 else 时为空字符串
            if has_else {
                let mutation =
                    r#"<mutation xmlns="http://www.w3.org/1999/xhtml" else="1"></mutation>"#
                        .to_string();
                obj.insert("mutation".to_string(), Value::String(mutation));
            }
        }
        Ok(block_value)
    }
}

pub(crate) struct TextJoinDecompiler<'a> {
    core: BlockDecompilerCore<'a>,
    compiled: &'a Value,
}

impl<'a> TextJoinDecompiler<'a> {
    pub(crate) fn new(compiled: &'a Value) -> Self {
        Self {
            core: BlockDecompilerCore::new(compiled),
            compiled,
        }
    }
}

impl<'a> BlockDecompiler<'a> for TextJoinDecompiler<'a> {
    fn decompile(&mut self, context: &mut BlockContext) -> Result<Value> {
        let mut block_value = self.core.decompile(context)?;
        let param_count = self
            .compiled
            .get("params")
            .and_then(|v| v.as_object())
            .map_or(0, serde_json::Map::len);
        let mutation = format!(r#"<mutation items="{}"></mutation>"#, param_count);
        if let Some(obj) = block_value.as_object_mut() {
            obj.insert("mutation".to_string(), Value::String(mutation));
        }
        Ok(block_value)
    }
}

pub(crate) struct AskAndChooseDecompiler<'a> {
    core: BlockDecompilerCore<'a>,
    compiled: &'a Value,
}

impl<'a> AskAndChooseDecompiler<'a> {
    pub(crate) fn new(compiled: &'a Value) -> Self {
        Self {
            core: BlockDecompilerCore::new(compiled),
            compiled,
        }
    }
}

impl<'a> BlockDecompiler<'a> for AskAndChooseDecompiler<'a> {
    fn decompile(&mut self, context: &mut BlockContext) -> Result<Value> {
        let mut block_value = self.core.decompile(context)?;
        let item_count = self
            .compiled
            .get("params")
            .and_then(|v| v.as_object())
            .map_or(0, serde_json::Map::len);
        let mutation = format!(r#"<mutation items="{}"></mutation>"#, item_count);
        if let Some(obj) = block_value.as_object_mut() {
            obj.insert("mutation".to_string(), Value::String(mutation));
        }
        Ok(block_value)
    }
}

pub(crate) struct SetEntityShowHideDecompiler<'a> {
    core: BlockDecompilerCore<'a>,
    compiled: &'a Value,
}

impl<'a> SetEntityShowHideDecompiler<'a> {
    pub(crate) fn new(compiled: &'a Value) -> Self {
        Self {
            core: BlockDecompilerCore::new(compiled),
            compiled,
        }
    }
}

impl<'a> BlockDecompiler<'a> for SetEntityShowHideDecompiler<'a> {
    fn decompile(&mut self, context: &mut BlockContext) -> Result<Value> {
        let mut block_value = self.core.decompile(context)?;
        let need_text = self
            .compiled
            .get("need_text")
            .and_then(serde_json::Value::as_bool)
            .unwrap_or(false);
        let time_block_id = self
            .compiled
            .get("time")
            .and_then(|v| v.as_str())
            .unwrap_or("");
        let mutation = format!(
            r#"<mutation need_text="{}" time="{}"></mutation>"#,
            need_text, time_block_id
        );
        if let Some(obj) = block_value.as_object_mut() {
            obj.insert("mutation".to_string(), Value::String(mutation));
        }
        Ok(block_value)
    }
}

pub(crate) struct TextSelectChangeableDecompiler<'a> {
    core: BlockDecompilerCore<'a>,
    compiled: &'a Value,
}

impl<'a> TextSelectChangeableDecompiler<'a> {
    pub(crate) fn new(compiled: &'a Value) -> Self {
        Self {
            core: BlockDecompilerCore::new(compiled),
            compiled,
        }
    }
}

impl<'a> BlockDecompiler<'a> for TextSelectChangeableDecompiler<'a> {
    fn decompile(&mut self, context: &mut BlockContext) -> Result<Value> {
        let mut block_value = self.core.decompile(context)?;
        let item_count = self
            .compiled
            .get("params")
            .and_then(|v| v.as_object())
            .map_or(0, serde_json::Map::len);
        let mutation = format!(r#"<mutation items="{}"></mutation>"#, item_count);
        if let Some(obj) = block_value.as_object_mut() {
            obj.insert("mutation".to_string(), Value::String(mutation));
        }
        Ok(block_value)
    }
}

pub(crate) struct FunctionDefDecompiler<'a> {
    core: BlockDecompilerCore<'a>,
    compiled: &'a Value,
}

impl<'a> FunctionDefDecompiler<'a> {
    pub(crate) fn new(compiled: &'a Value) -> Self {
        Self {
            // 函数体 child_block 使用 STACK 插槽
            core: BlockDecompilerCore::new(compiled),
            compiled,
        }
    }
}

impl<'a> BlockDecompiler<'a> for FunctionDefDecompiler<'a> {
    fn decompile(&mut self, context: &mut BlockContext) -> Result<Value> {
        let mut block_value = self.core.decompile(context)?;
        let procedure_name = self.compiled.get_str_or("procedure_name", "");
        let params = self
            .compiled
            .get("params")
            .and_then(|v| v.as_object())
            .cloned()
            .unwrap_or_default();
        let block = block_value
            .as_object_mut()
            .ok_or_else(|| DecompilerError::Decompile("block_value不是对象".to_string()))?;

        if let Some(shadows) = block.get_mut("shadows").and_then(|s| s.as_object_mut()) {
            // 编辑版 defnoreturn shadows 键集合:DEFINE / PARAMS0..n / MUTATOR / STACK
            shadows.insert("PROCEDURES_2_DEFNORETURN_DEFINE".to_string(), json!(""));
            shadows.insert("PROCEDURES_2_DEFNORETURN_MUTATOR".to_string(), json!(""));
            shadows.insert("STACK".to_string(), json!(""));
            for i in 0..params.len() {
                // 每个参数插槽配一个 math_number 占位 shadow(编辑版同款)
                let shadow_value = context.shadow_builder.create("math_number", None, None);
                shadows.insert(format!("PARAMS{}", i), shadow_value);
            }
        }

        let mut mutation_args = String::with_capacity(params.len() * 32);
        let parent_id = block
            .get("id")
            .and_then(|v| v.as_str())
            .ok_or_else(|| DecompilerError::InvalidResponse("当前块缺少 id".to_string()))?
            .to_string();

        for (i, (param_name, _)) in params.iter().enumerate() {
            // 编辑版插槽名为 PARAMS0/PARAMS1/...(无空格)
            let input_name = format!("PARAMS{}", i);
            let _ = write!(mutation_args, r#"<arg name="{}"></arg>"#, input_name);

            // 生成稳定的参数块(编辑版 is_shadow=false,可编辑)
            let param_block_id = context.shadow_builder.id_generator.generate(20);
            let param_block = json!({
                "id": param_block_id,
                "type": "procedures_2_stable_parameter",
                "is_shadow": false,
                "is_output": true,
                "fields": {
                    "param_name": param_name,
                    "param_default_value": ""
                },
                "location": [0, 0],
                "collapsed": false,
                "disabled": false,
                "parent_id": parent_id,
                "deletable": true,
                "movable": true,
                "editable": true,
                "visible": "visible",
                "comment": null,
                "mutation": "",
                "shadows": {},
                "field_constraints": {},
                "field_extra_attr": {}
            });
            context.blocks.insert(param_block_id.clone(), param_block);
            context.insert_connection(
                &parent_id,
                &param_block_id,
                json!({
                    "type": "input",
                    "input_type": "value",
                    "input_name": input_name
                }),
            );
        }

        // 编辑版 mutation:<mutation xmlns="..."><arg name="PARAMS0"></arg>...</mutation>
        let mutation = format!(
            r#"<mutation xmlns="http://www.w3.org/1999/xhtml">{}</mutation>"#,
            mutation_args
        );
        block.insert("mutation".to_string(), Value::String(mutation));

        let fields = block
            .get_mut("fields")
            .and_then(|v| v.as_object_mut())
            .ok_or_else(|| DecompilerError::Decompile("fields对象不存在".to_string()))?;
        fields.insert(
            "NAME".to_string(),
            Value::String(procedure_name.to_string()),
        );
        Ok(block_value)
    }
}

pub(crate) struct FunctionCallDecompiler<'a> {
    core: BlockDecompilerCore<'a>,
    compiled: &'a Value,
}

impl<'a> FunctionCallDecompiler<'a> {
    pub(crate) fn new(compiled: &'a Value) -> Self {
        Self {
            core: BlockDecompilerCore::new(compiled),
            compiled,
        }
    }
}

impl<'a> BlockDecompiler<'a> for FunctionCallDecompiler<'a> {
    fn decompile(&mut self, context: &mut BlockContext) -> Result<Value> {
        let mut block_value = self.core.decompile(context)?;
        let procedure_name = self.compiled.get_str_or("procedure_name", "");

        let (def_id, disabled) = if let Some(func) = context.functions.get(procedure_name) {
            let id = func.get("id").and_then(|v| v.as_str()).unwrap_or("");
            (id.to_string(), false)
        } else {
            error!("调用未定义的函数: {},将禁用该积木", procedure_name);
            (String::new(), true)
        };

        let params = self
            .compiled
            .get("params")
            .and_then(|v| v.as_object())
            .cloned()
            .unwrap_or_default();
        let block = block_value
            .as_object_mut()
            .ok_or_else(|| DecompilerError::Decompile("block_value不是对象".to_string()))?;

        block.insert("disabled".to_string(), Value::Bool(disabled));

        let mut mutation = String::from(r#"<mutation xmlns="http://www.w3.org/1999/xhtml""#);
        let _ = write!(mutation, r#" name="{}""#, procedure_name);
        let _ = write!(mutation, r#" def_id="{}""#, def_id);
        mutation.push('>');
        for (param_name, _) in &params {
            let _ = write!(
                mutation,
                r#"<procedures_2_parameter_shadow name="{}" value="0"></procedures_2_parameter_shadow>"#,
                param_name
            );
        }
        mutation.push_str("</mutation>");
        block.insert("mutation".to_string(), Value::String(mutation));

        if let Some(shadows) = block.get_mut("shadows").and_then(|s| s.as_object_mut()) {
            shadows.insert("NAME".to_string(), json!(""));
        }

        let parent_id = block
            .get("id")
            .and_then(|v| v.as_str())
            .ok_or_else(|| DecompilerError::InvalidResponse("当前块缺少 id".to_string()))?
            .to_string();

        for (param_index, (_param_name, param_value)) in params.iter().enumerate() {
            // 编辑版插槽名为 ARG0/ARG1/...(无空格)
            let input_name = format!("ARG{}", param_index);
            if param_value.is_object() {
                let mut param_decompiler = BlockDecompilerCore::new(param_value);
                let param_block = param_decompiler.decompile(context)?;
                let param_id = param_block
                    .get("id")
                    .ok_or_else(|| {
                        DecompilerError::InvalidResponse("param_block缺少id".to_string())
                    })?
                    .as_str()
                    .ok_or_else(|| {
                        DecompilerError::InvalidResponse("param_block id不是字符串".to_string())
                    })?
                    .to_string();
                context.blocks.insert(param_id.clone(), param_block);
                if let Some(b) = context.blocks.get_mut(&param_id)
                    && let Some(o) = b.as_object_mut()
                {
                    o.insert("parent_id".to_string(), json!(parent_id));
                }
                context.insert_connection(
                    &parent_id,
                    &param_id,
                    json!({
                        "type": "input",
                        "input_type": "value",
                        "input_name": input_name
                    }),
                );
                if let Some(shadows) = block.get_mut("shadows").and_then(|s| s.as_object_mut()) {
                    let shadow_value =
                        context
                            .shadow_builder
                            .create("default_value", Some(param_id), None);
                    shadows.insert(input_name, shadow_value);
                }
            } else if let Some(shadows) = block.get_mut("shadows").and_then(|s| s.as_object_mut()) {
                let shadow_value = context.shadow_builder.create("default_value", None, None);
                shadows.insert(input_name, shadow_value);
            }
        }

        let fields = block
            .get_mut("fields")
            .and_then(|v| v.as_object_mut())
            .ok_or_else(|| DecompilerError::Decompile("fields对象不存在".to_string()))?;
        fields.insert(
            "NAME".to_string(),
            Value::String(procedure_name.to_string()),
        );
        Ok(block_value)
    }
}

pub(crate) struct MutationDecompiler<'a> {
    inner: DefaultBlockDecompiler<'a>,
    mutation: String,
}

impl<'a> MutationDecompiler<'a> {
    pub(crate) fn new(compiled: &'a Value, mutation: String) -> Self {
        Self {
            inner: DefaultBlockDecompiler::new(compiled),
            mutation,
        }
    }
}

impl<'a> BlockDecompiler<'a> for MutationDecompiler<'a> {
    fn decompile(&mut self, context: &mut BlockContext) -> Result<Value> {
        let mut block_value = self.inner.decompile(context)?;
        if let Some(obj) = block_value.as_object_mut() {
            obj.insert("mutation".to_string(), Value::String(self.mutation.clone()));
        }
        Ok(block_value)
    }
}

// 积木反编译器工厂
/// 按块类型分派专用反编译器
/// 树内递归(process_next/children/conditions/params)也使用本函数,
/// 否则嵌套的 procedures_2_callnoreturn / controls_if 等不会走专用反编译器,
/// 导致 NAME/mutation/ARG 参数块/if-else 结构缺失
/// 独立于 BlockDecompilerFactory,避免其 lifetime 绑定 BlockContext
pub(crate) fn create_block_decompiler<'a>(
    compiled: &'a Value,
) -> Box<dyn BlockDecompiler<'a> + 'a> {
    let block_type = compiled.get_str_or("type", "");
    match block_type {
        "controls_if" | "controls_if_no_else" => Box::new(IfBlockDecompiler::new(compiled)),
        "text_join" => Box::new(TextJoinDecompiler::new(compiled)),
        "ask_and_choose" => Box::new(AskAndChooseDecompiler::new(compiled)),
        "set_entity_show_hide" => Box::new(SetEntityShowHideDecompiler::new(compiled)),
        "text_select_changeable" => Box::new(TextSelectChangeableDecompiler::new(compiled)),
        "procedures_2_defnoreturn" => Box::new(FunctionDefDecompiler::new(compiled)),
        "procedures_2_callnoreturn" | "procedures_2_callreturn" => {
            Box::new(FunctionCallDecompiler::new(compiled))
        }
        "procedures_2_return_value" => {
            let item_count = compiled
                .get("params")
                .and_then(|v| v.as_object())
                .map_or(0, serde_json::Map::len);
            let mutation = format!("<mutation items=\"{}\"></mutation>", item_count);
            Box::new(MutationDecompiler::new(compiled, mutation))
        }
        "procedures_2_stable_parameter" | "procedures_2_parameter" => {
            Box::new(DefaultBlockDecompiler::new(compiled))
        }
        _ => Box::new(DefaultBlockDecompiler::new(compiled)),
    }
}
