use crate::utils::acquire::{BaseKey, CodeMaoClient, HttpMethod, MewError, MewResult};
use serde_json::{Value, json};

// ==================== 云数据库类型枚举 ====================

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CloudDatabaseType {
    Dict = 1,
    Table = 2,
}

impl CloudDatabaseType {
    pub async fn as_i32(&self) -> i32 {
        *self as i32
    }
}

// ==================== Ranking struct ====================

pub struct Ranking {
    client: &'static CodeMaoClient,
}

impl Ranking {
    pub fn new() -> Self {
        Self {
            client: CodeMaoClient::global(),
        }
    }

    /// 更新排行榜 (全量更新)
    pub async fn update_ranking_list(&self, data: Value) -> MewResult<Value> {
        let response = self
            .client
            .build_request(
                HttpMethod::PUT,
                "/neko/ranking-list/fullUpdate",
                Some(BaseKey::Creation),
            )
            .with_payload(data)
            .send()
            .await?;
        response.json().await.map_err(MewError::from)
    }

    /// 清空排行榜
    pub async fn clear_ranking_list(&self, ranking_id: &str) -> MewResult<Value> {
        let response = self
            .client
            .build_request(
                HttpMethod::PUT,
                "/neko/ranking-list/clear",
                Some(BaseKey::Creation),
            )
            .with_param("id", ranking_id)
            .send()
            .await?;
        response.json().await.map_err(MewError::from)
    }

    /// 获取排行榜记录
    pub async fn fetch_ranking_records(&self, ranking_id: &str, work_id: i32) -> MewResult<Value> {
        let response = self
            .client
            .build_request(
                HttpMethod::GET,
                "/neko/ranking-list/record/list",
                Some(BaseKey::Creation),
            )
            .with_param("id", ranking_id)
            .with_param("work_id", work_id.to_string())
            .send()
            .await?;
        response.json().await.map_err(MewError::from)
    }

    /// 添加排行榜记录
    pub async fn add_ranking_record(
        &self,
        work_id: i32,
        value: &str,
        ranking_id: i32,
    ) -> MewResult<Value> {
        let response = self
            .client
            .build_request(
                HttpMethod::POST,
                "/neko/ranking-list/record",
                Some(BaseKey::Creation),
            )
            .with_payload(json!({
                "work_id": work_id,
                "value": value,
                "id": ranking_id,
            }))
            .send()
            .await?;
        response.json().await.map_err(MewError::from)
    }

    /// 创建排行榜
    pub async fn create_ranking_list(&self, data: Value) -> MewResult<Value> {
        let response = self
            .client
            .build_request(
                HttpMethod::POST,
                "/neko/ranking-list",
                Some(BaseKey::Creation),
            )
            .with_payload(data)
            .send()
            .await?;
        response.json().await.map_err(MewError::from)
    }

    /// 删除排行榜
    pub async fn delete_ranking_list(&self, ranking_id: &str, work_id: i32) -> MewResult<Value> {
        let endpoint = format!("/neko/ranking-list/{}", ranking_id);
        let response = self
            .client
            .build_request(HttpMethod::DELETE, &endpoint, Some(BaseKey::Creation))
            .with_param("id", ranking_id)
            .with_param("work_id", work_id.to_string())
            .send()
            .await?;
        response.json().await.map_err(MewError::from)
    }
}

impl Default for Ranking {
    fn default() -> Self {
        Self::new()
    }
}

// ==================== CoconutCloud struct ====================

pub struct CoconutCloud {
    client: &'static CodeMaoClient,
}

impl CoconutCloud {
    pub fn new() -> Self {
        Self {
            client: CodeMaoClient::global(),
        }
    }

    /// 获取值对应的类型名称
    fn value_type_name(value: &Value) -> &'static str {
        match value {
            Value::String(_) => "str",
            Value::Number(_) => "int",
            Value::Bool(_) => "bool",
            Value::Array(_) => "list",
            Value::Object(_) => "dict",
            Value::Null => "NoneType",
        }
    }

    /// 设置云字典键值
    pub async fn set_dictionary_value(
        &self,
        dict_id: &str,
        key: &str,
        value: Value,
    ) -> MewResult<Value> {
        let type_name = Self::value_type_name(&value);
        let endpoint = format!("/coconut/webdb/try/dict/{}/set", dict_id);

        let response = self
            .client
            .build_request(HttpMethod::POST, &endpoint, Some(BaseKey::Creation))
            .with_payload(json!({
                "key": key,
                "type": type_name,
                "value": value,
            }))
            .send()
            .await?;
        response.json().await.map_err(MewError::from)
    }

    /// 删除云字典键
    pub async fn delete_dictionary_key(&self, dict_id: &str, key: &str) -> MewResult<Value> {
        let endpoint = format!("/coconut/webdb/try/dict/{}/remove", dict_id);
        let response = self
            .client
            .build_request(HttpMethod::DELETE, &endpoint, Some(BaseKey::Creation))
            .with_param("key", key)
            .send()
            .await?;
        response.json().await.map_err(MewError::from)
    }

    /// 清空云字典
    pub async fn clear_dictionary(&self, dict_id: &str) -> MewResult<Value> {
        let endpoint = format!("/coconut/webdb/try/dict/clear/{}", dict_id);
        let response = self
            .client
            .build_request(HttpMethod::DELETE, &endpoint, Some(BaseKey::Creation))
            .send()
            .await?;
        response.json().await.map_err(MewError::from)
    }

    /// 获取云字典所有键
    pub async fn get_dictionary_keys(&self, dict_id: &str) -> MewResult<Value> {
        let endpoint = format!("/coconut/webdb/try/dict/{}/keys", dict_id);
        let response = self
            .client
            .build_request(HttpMethod::GET, &endpoint, Some(BaseKey::Creation))
            .send()
            .await?;
        response.json().await.map_err(MewError::from)
    }

    /// 获取云字典值
    pub async fn get_dictionary_value(&self, dict_id: &str, key: &str) -> MewResult<Value> {
        let endpoint = format!("/coconut/webdb/try/dict/{}/getvalue", dict_id);
        let response = self
            .client
            .build_request(HttpMethod::GET, &endpoint, Some(BaseKey::Creation))
            .with_param("key", key)
            .send()
            .await?;
        response.json().await.map_err(MewError::from)
    }

    /// 查询云数据表
    pub async fn query_table(&self, table_id: &str, queries: Value) -> MewResult<Value> {
        let endpoint = format!("/coconut/clouddb/runtime/{}/select", table_id);
        let response = self
            .client
            .build_request(HttpMethod::POST, &endpoint, Some(BaseKey::Creation))
            .with_payload(json!({
                "querys": {
                    "querys": queries
                }
            }))
            .send()
            .await?;
        response.json().await.map_err(MewError::from)
    }

    /// 更新云数据表
    pub async fn update_table_rows(
        &self,
        table_id: &str,
        queries: Value,
        values: Value,
    ) -> MewResult<Value> {
        let endpoint = format!("/coconut/clouddb/runtime/{}/update", table_id);
        let response = self
            .client
            .build_request(HttpMethod::PUT, &endpoint, Some(BaseKey::Creation))
            .with_payload(json!({
                "querys": {
                    "querys": queries
                },
                "values": values
            }))
            .send()
            .await?;
        response.json().await.map_err(MewError::from)
    }

    /// 插入云数据表行
    pub async fn insert_table_rows(&self, table_id: &str, values: Value) -> MewResult<Value> {
        let endpoint = format!("/coconut/clouddb/runtime/{}/insert", table_id);
        let response = self
            .client
            .build_request(HttpMethod::POST, &endpoint, Some(BaseKey::Creation))
            .with_payload(json!({
                "values": values
            }))
            .send()
            .await?;
        response.json().await.map_err(MewError::from)
    }

    /// 删除云数据表行
    pub async fn delete_table_rows(&self, table_id: &str, queries: Value) -> MewResult<Value> {
        let endpoint = format!("/coconut/clouddb/runtime/{}/delete", table_id);
        let response = self
            .client
            .build_request(HttpMethod::PUT, &endpoint, Some(BaseKey::Creation))
            .with_payload(json!({
                "querys": {
                    "querys": queries
                }
            }))
            .send()
            .await?;
        response.json().await.map_err(MewError::from)
    }

    /// 清空云数据表
    pub async fn clear_table(&self, table_id: &str) -> MewResult<Value> {
        let endpoint = format!("/coconut/clouddb/v2/runtime/{}/clear", table_id);
        let response = self
            .client
            .build_request(HttpMethod::PUT, &endpoint, Some(BaseKey::Creation))
            .send()
            .await?;
        response.json().await.map_err(MewError::from)
    }

    /// 获取云数据表行数
    pub async fn get_table_row_count(&self, table_id: &str) -> MewResult<Value> {
        let endpoint = format!("/coconut/clouddb/runtime/{}/count", table_id);
        let response = self
            .client
            .build_request(HttpMethod::GET, &endpoint, Some(BaseKey::Creation))
            .with_param("type", "RECORD")
            .send()
            .await?;
        response.json().await.map_err(MewError::from)
    }

    /// 获取云数据表信息
    pub async fn get_table_info(&self, table_ids: &[String]) -> MewResult<Value> {
        let ids_str = table_ids.join(",");
        let response = self
            .client
            .build_request(
                HttpMethod::GET,
                "/coconut/clouddb/v2/runtime/list",
                Some(BaseKey::Creation),
            )
            .with_param("db_ids", ids_str)
            .send()
            .await?;
        response.json().await.map_err(MewError::from)
    }

    /// 加载作品数据
    pub async fn load_work_data(&self, work_id: i32, channel: &str) -> MewResult<Value> {
        let endpoint = format!("/coconut/web/work/{}/load", work_id);
        let response = self
            .client
            .build_request(HttpMethod::GET, &endpoint, Some(BaseKey::Creation))
            .with_param("channel", channel)
            .send()
            .await?;
        response.json().await.map_err(MewError::from)
    }
}

impl Default for CoconutCloud {
    fn default() -> Self {
        Self::new()
    }
}

// ==================== CoconutCloudAdmin struct ====================

pub struct CoconutCloudAdmin {
    client: &'static CodeMaoClient,
}

impl CoconutCloudAdmin {
    pub fn new() -> Self {
        Self {
            client: CodeMaoClient::global(),
        }
    }

    /// 获取用户云数据库列表
    pub async fn list_user_databases(
        &self,
        db_type: Option<CloudDatabaseType>,
    ) -> MewResult<Value> {
        let mut builder = self.client.build_request(
            HttpMethod::GET,
            "/coconut/clouddb/user/list",
            Some(BaseKey::Creation),
        );

        if let Some(t) = db_type {
            builder = builder.with_param("type", t.as_i32().await.to_string());
        }

        let response = builder.send().await?;
        response.json().await.map_err(MewError::from)
    }

    /// 获取用户云数据库详细信息列表
    pub async fn list_user_databases_detail(
        &self,
        db_type: Option<CloudDatabaseType>,
    ) -> MewResult<Value> {
        let mut builder = self.client.build_request(
            HttpMethod::GET,
            "/coconut/clouddb/user/list/detail",
            Some(BaseKey::Creation),
        );

        if let Some(t) = db_type {
            builder = builder.with_param("type", t.as_i32().await.to_string());
        }

        let response = builder.send().await?;
        response.json().await.map_err(MewError::from)
    }

    /// 获取作品关联的云字典列表
    pub async fn list_work_dicts(&self, work_id: i32) -> MewResult<Value> {
        let response = self
            .client
            .build_request(
                HttpMethod::GET,
                "/coconut/webdb/admin/dict",
                Some(BaseKey::Creation),
            )
            .with_param("work_id", work_id.to_string())
            .send()
            .await?;
        response.json().await.map_err(MewError::from)
    }

    /// 按类型获取云字典列表
    pub async fn list_work_dicts_by_type(&self, dict_type: i32) -> MewResult<Value> {
        let response = self
            .client
            .build_request(
                HttpMethod::GET,
                "/coconut/webdb/admin/dict",
                Some(BaseKey::Creation),
            )
            .with_param("type", dict_type.to_string())
            .send()
            .await?;
        response.json().await.map_err(MewError::from)
    }

    /// 获取云字典条目列表(分页)
    pub async fn get_dict_entries(
        &self,
        dict_id: i32,
        work_id: i32,
        offset: Option<i32>,
        limit: Option<i32>,
    ) -> MewResult<Value> {
        let endpoint = format!("/coconut/webdb/admin/dict/{}", dict_id);
        let response = self
            .client
            .build_request(HttpMethod::GET, &endpoint, Some(BaseKey::Creation))
            .with_param("work_id", work_id.to_string())
            .with_param("offset", offset.unwrap_or(1).to_string())
            .with_param("limit", limit.unwrap_or(500).to_string())
            .send()
            .await?;
        response.json().await.map_err(MewError::from)
    }

    /// 迁移云字典环境
    pub async fn migrate_dict(&self, db_id: &str, from_env: i32, to_env: i32) -> MewResult<Value> {
        let response = self
            .client
            .build_request(
                HttpMethod::PUT,
                "/coconut/webdb/admin/dict/migrate",
                Some(BaseKey::Creation),
            )
            .with_payload(json!({
                "db_id": db_id,
                "from_env": from_env,
                "to_env": to_env,
            }))
            .send()
            .await?;
        response.json().await.map_err(MewError::from)
    }
}

impl Default for CoconutCloudAdmin {
    fn default() -> Self {
        Self::new()
    }
}
