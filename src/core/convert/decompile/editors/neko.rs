use crate::api::auth::CloudAuthenticator;
use crate::core::convert::decompile::{
    context::DecompilerContext,
    contract::{DecompileResult, WorkDecompiler, save_json_result},
};
use crate::core::convert::shared::{
    CryptoService, DecompilerConfig, DecompilerError, HttpClient, RawWorkData, Result, WorkFetcher,
    WorkInfo,
};
use std::path::{Path, PathBuf};
use std::sync::Arc;

// NEKO
pub(crate) struct NekoFetcher {
    http_client: Box<dyn HttpClient>,
    config: Arc<DecompilerConfig>,
}

impl NekoFetcher {
    pub(crate) fn new(http_client: Box<dyn HttpClient>, config: Arc<DecompilerConfig>) -> Self {
        Self {
            http_client,
            config,
        }
    }
}

impl WorkFetcher for NekoFetcher {
    fn fetch(&self, work_info: &WorkInfo) -> Result<RawWorkData> {
        let detail_url = format!(
            "{}/neko/community/player/published-work-detail/{}",
            self.config.creation_base_url, work_info.id
        );

        let mut auth = CloudAuthenticator::new(None);
        let device_auth = auth
            .generate_x_device_auth()
            .map_err(|e| DecompilerError::Other {
                msg: format!("生成设备认证失败: {}", e),
                source: Some(Box::new(DecompilerError::Other {
                    msg: e.to_string(),
                    source: None,
                })),
            })?;

        // 修复:generate_x_device_auth 已返回 JSON 字符串({"sign":...,"timestamp":...,"client_id":...}),
        // 直接作为 header 值.此前二次 serde_json::to_string 会再包一层引号转义,
        // 服务器解析 device-auth 失败返回 500 "Not a JSON Object",导致 NEKO 作品无法获取原始数据
        let headers: Vec<(String, String)> =
            vec![("x-creation-tools-device-auth".to_string(), device_auth)];

        let detail = self.http_client.get_json(&detail_url, Some(headers))?;

        let encrypted_url = detail
            .get("source_urls")
            .and_then(|v| v.as_array())
            .and_then(|arr| arr.first())
            .and_then(|v| v.as_str())
            .ok_or_else(|| DecompilerError::InvalidResponse("无法获取source_urls".to_string()))?;

        let encrypted_content = self.http_client.get_text(encrypted_url)?;
        Ok(RawWorkData::NekoEncrypted(encrypted_content))
    }
}

pub(crate) struct NekoDecompiler {
    crypto_service: CryptoService,
}

impl NekoDecompiler {
    pub(crate) fn new(salt: &[u8]) -> Self {
        Self {
            crypto_service: CryptoService::new(salt),
        }
    }
}

impl WorkDecompiler for NekoDecompiler {
    fn decompile(&self, raw: RawWorkData, _context: &DecompilerContext) -> Result<DecompileResult> {
        match raw {
            RawWorkData::NekoEncrypted(encrypted) => {
                // `CryptoService` 内部只有 `Arc<[u8]>` salt,直接借用即可,无需克隆
                let decrypted_json = self.crypto_service.decrypt_bcmkn_json(&encrypted)?;
                Ok(DecompileResult::Json(decrypted_json))
            }
            _ => Err(DecompilerError::Decompile(
                "NekoDecompiler 需要 NekoEncrypted 数据".into(),
            )),
        }
    }

    fn save_result(
        &self,
        result: &DecompileResult,
        output_dir: Option<&Path>,
        context: &DecompilerContext,
    ) -> Result<PathBuf> {
        // 扩展名与其它编辑器同一来源(与 coco/kitten 一致,不再硬编码)
        let extension = context
            .work_info
            .file_extension(&context.config)
            .trim_start_matches('.')
            .to_owned();
        save_json_result(result, output_dir, context, &extension, "NEKO")
    }
}
