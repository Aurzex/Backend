use crate::utils::acquire::{BaseKey, CodeMaoClient, HttpMethod, MewResult};
use serde_json::{Value, json};

// Ranking struct
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
    ///
    /// # Arguments
    /// * `data` - 排行榜数据
    ///
    /// # Returns
    /// 更新结果
    pub fn update_ranking_list(&self, data: Value) -> MewResult<Value> {
        let response = self
            .client
            .build_request(
                HttpMethod::PUT,
                "/neko/ranking-list/fullUpdate",
                Some(BaseKey::Creation),
            )
            .with_payload(data)
            .send()?;
        self.client.response_to_json(response)
    }

    /// 清空排行榜
    ///
    /// # Arguments
    /// * `ranking_id` - 排行榜 ID
    ///
    /// # Returns
    /// 清空结果
    pub fn clear_ranking_list(&self, ranking_id: &str) -> MewResult<Value> {
        let response = self
            .client
            .build_request(
                HttpMethod::PUT,
                "/neko/ranking-list/clear",
                Some(BaseKey::Creation),
            )
            .with_param("id", ranking_id)
            .send()?;
        self.client.response_to_json(response)
    }

    /// 获取排行榜记录
    ///
    /// # Arguments
    /// * `ranking_id` - 排行榜 ID
    /// * `work_id` - 作品 ID
    ///
    /// # Returns
    /// 排行榜记录列表
    pub fn fetch_ranking_records(&self, ranking_id: &str, work_id: i32) -> MewResult<Value> {
        let response = self
            .client
            .build_request(
                HttpMethod::GET,
                "/neko/ranking-list/record/list",
                Some(BaseKey::Creation),
            )
            .with_param("id", ranking_id)
            .with_param("work_id", work_id.to_string())
            .send()?;
        self.client.response_to_json(response)
    }

    /// 添加排行榜记录
    ///
    /// # Arguments
    /// * `work_id` - 作品 ID
    /// * `value` - 记录值
    /// * `ranking_id` - 排行榜 ID
    ///
    /// # Returns
    /// 添加结果
    pub fn add_ranking_record(
        &self,
        work_id: i32,
        value: &str,
        ranking_id: i32,
    ) -> MewResult<Value> {
        let data = json!({
            "work_id": work_id,
            "value": value,
            "id": ranking_id,
        });

        let response = self
            .client
            .build_request(
                HttpMethod::POST,
                "/neko/ranking-list/record",
                Some(BaseKey::Creation),
            )
            .with_payload(data)
            .send()?;
        self.client.response_to_json(response)
    }

    /// 创建排行榜
    ///
    /// # Arguments
    /// * `data` - 排行榜数据
    ///
    /// # Returns
    /// 创建结果
    pub fn create_ranking_list(&self, data: Value) -> MewResult<Value> {
        let response = self
            .client
            .build_request(
                HttpMethod::POST,
                "/neko/ranking-list",
                Some(BaseKey::Creation),
            )
            .with_payload(data)
            .send()?;
        self.client.response_to_json(response)
    }

    /// 删除排行榜
    ///
    /// # Arguments
    /// * `ranking_id` - 排行榜 ID
    /// * `work_id` - 作品 ID
    ///
    /// # Returns
    /// 删除结果
    pub fn delete_ranking_list(&self, ranking_id: &str, work_id: i32) -> MewResult<Value> {
        let endpoint = format!("/neko/ranking-list/{}", ranking_id);

        let response = self
            .client
            .build_request(HttpMethod::DELETE, &endpoint, Some(BaseKey::Creation))
            .with_param("id", ranking_id)
            .with_param("work_id", work_id.to_string())
            .send()?;
        self.client.response_to_json(response)
    }
}

impl Default for Ranking {
    fn default() -> Self {
        Self::new()
    }
}

// CoconutCloud struct
pub struct CoconutCloud {
    client: &'static CodeMaoClient,
}

impl CoconutCloud {
    pub fn new() -> Self {
        Self {
            client: CodeMaoClient::global(),
        }
    }

    /// 设置云字典键值
    ///
    /// # Arguments
    /// * `dict_id` - 字典 ID
    /// * `key` - 键名
    /// * `value` - 值
    ///
    /// # Returns
    /// 操作结果
    pub fn set_dictionary_value(&self, dict_id: &str, key: &str, value: Value) -> MewResult<Value> {
        let type_name = match value {
            Value::String(_) => "str",
            Value::Number(_) => "int",
            Value::Bool(_) => "bool",
            Value::Array(_) => "list",
            Value::Object(_) => "dict",
            Value::Null => "NoneType",
        };

        let data = json!({
            "key": key,
            "type": type_name,
            "value": value,
        });

        let endpoint = format!("/coconut/webdb/try/dict/{}/set", dict_id);

        let response = self
            .client
            .build_request(HttpMethod::POST, &endpoint, Some(BaseKey::Creation))
            .with_payload(data)
            .send()?;
        self.client.response_to_json(response)
    }

    /// 删除云字典键
    ///
    /// # Arguments
    /// * `dict_id` - 字典 ID
    /// * `key` - 键名
    ///
    /// # Returns
    /// 操作结果
    pub fn delete_dictionary_key(&self, dict_id: &str, key: &str) -> MewResult<Value> {
        let endpoint = format!("/coconut/webdb/try/dict/{}/remove", dict_id);

        let response = self
            .client
            .build_request(HttpMethod::DELETE, &endpoint, Some(BaseKey::Creation))
            .with_param("key", key)
            .send()?;
        self.client.response_to_json(response)
    }

    /// 清空云字典
    ///
    /// # Arguments
    /// * `dict_id` - 字典 ID
    ///
    /// # Returns
    /// 操作结果
    pub fn clear_dictionary(&self, dict_id: &str) -> MewResult<Value> {
        let endpoint = format!("/coconut/webdb/try/dict/clear/{}", dict_id);

        let response = self
            .client
            .build_request(HttpMethod::DELETE, &endpoint, Some(BaseKey::Creation))
            .send()?;
        self.client.response_to_json(response)
    }

    /// 获取云字典所有键
    ///
    /// # Arguments
    /// * `dict_id` - 字典 ID
    ///
    /// # Returns
    /// 键名列表
    pub fn get_dictionary_keys(&self, dict_id: &str) -> MewResult<Value> {
        let endpoint = format!("/coconut/webdb/try/dict/{}/keys", dict_id);

        let response = self
            .client
            .build_request(HttpMethod::GET, &endpoint, Some(BaseKey::Creation))
            .send()?;
        self.client.response_to_json(response)
    }

    /// 获取云字典值
    ///
    /// # Arguments
    /// * `dict_id` - 字典 ID
    /// * `key` - 键名
    ///
    /// # Returns
    /// 键值
    pub fn get_dictionary_value(&self, dict_id: &str, key: &str) -> MewResult<Value> {
        let endpoint = format!("/coconut/webdb/try/dict/{}/getvalue", dict_id);

        let response = self
            .client
            .build_request(HttpMethod::GET, &endpoint, Some(BaseKey::Creation))
            .with_param("key", key)
            .send()?;
        self.client.response_to_json(response)
    }

    /// 查询云数据表
    ///
    /// # Arguments
    /// * `table_id` - 表 ID
    /// * `queries` - 查询条件列表
    ///
    /// # Returns
    /// 查询结果
    pub fn query_table(&self, table_id: &str, queries: Value) -> MewResult<Value> {
        let data = json!({
            "querys": {
                "querys": queries
            }
        });

        let endpoint = format!("/coconut/clouddb/runtime/{}/select", table_id);

        let response = self
            .client
            .build_request(HttpMethod::POST, &endpoint, Some(BaseKey::Creation))
            .with_payload(data)
            .send()?;
        self.client.response_to_json(response)
    }

    /// 更新云数据表
    ///
    /// # Arguments
    /// * `table_id` - 表 ID
    /// * `queries` - 查询条件列表
    /// * `values` - 更新值列表
    ///
    /// # Returns
    /// 更新结果
    pub fn update_table_rows(
        &self,
        table_id: &str,
        queries: Value,
        values: Value,
    ) -> MewResult<Value> {
        let data = json!({
            "querys": {
                "querys": queries
            },
            "values": values
        });

        let endpoint = format!("/coconut/clouddb/runtime/{}/update", table_id);

        let response = self
            .client
            .build_request(HttpMethod::PUT, &endpoint, Some(BaseKey::Creation))
            .with_payload(data)
            .send()?;
        self.client.response_to_json(response)
    }

    /// 插入云数据表行
    ///
    /// # Arguments
    /// * `table_id` - 表 ID
    /// * `values` - 值列表
    ///
    /// # Returns
    /// 插入结果
    pub fn insert_table_rows(&self, table_id: &str, values: Value) -> MewResult<Value> {
        let data = json!({
            "values": values
        });

        let endpoint = format!("/coconut/clouddb/runtime/{}/insert", table_id);

        let response = self
            .client
            .build_request(HttpMethod::POST, &endpoint, Some(BaseKey::Creation))
            .with_payload(data)
            .send()?;
        self.client.response_to_json(response)
    }

    /// 删除云数据表行
    ///
    /// # Arguments
    /// * `table_id` - 表 ID
    /// * `queries` - 查询条件列表
    ///
    /// # Returns
    /// 删除结果
    pub fn delete_table_rows(&self, table_id: &str, queries: Value) -> MewResult<Value> {
        let data = json!({
            "querys": {
                "querys": queries
            }
        });

        let endpoint = format!("/coconut/clouddb/runtime/{}/delete", table_id);

        let response = self
            .client
            .build_request(HttpMethod::PUT, &endpoint, Some(BaseKey::Creation))
            .with_payload(data)
            .send()?;
        self.client.response_to_json(response)
    }

    /// 清空云数据表
    ///
    /// # Arguments
    /// * `table_id` - 表 ID
    ///
    /// # Returns
    /// 清空结果
    pub fn clear_table(&self, table_id: &str) -> MewResult<Value> {
        let endpoint = format!("/coconut/clouddb/v2/runtime/{}/clear", table_id);

        let response = self
            .client
            .build_request(HttpMethod::PUT, &endpoint, Some(BaseKey::Creation))
            .send()?;
        self.client.response_to_json(response)
    }

    /// 获取云数据表行数
    ///
    /// # Arguments
    /// * `table_id` - 表 ID
    ///
    /// # Returns
    /// 行数信息
    pub fn get_table_row_count(&self, table_id: &str) -> MewResult<Value> {
        let endpoint = format!("/coconut/clouddb/runtime/{}/count", table_id);

        let response = self
            .client
            .build_request(HttpMethod::GET, &endpoint, Some(BaseKey::Creation))
            .with_param("type", "RECORD")
            .send()?;
        self.client.response_to_json(response)
    }

    /// 获取云数据表信息
    ///
    /// # Arguments
    /// * `table_ids` - 表 ID 列表
    ///
    /// # Returns
    /// 表信息列表
    pub fn get_table_info(&self, table_ids: &[String]) -> MewResult<Value> {
        let ids_str = table_ids.join(",");

        let response = self
            .client
            .build_request(
                HttpMethod::GET,
                "/coconut/clouddb/v2/runtime/list",
                Some(BaseKey::Creation),
            )
            .with_param("db_ids", ids_str)
            .send()?;
        self.client.response_to_json(response)
    }

    /// 加载作品数据
    ///
    /// # Arguments
    /// * `work_id` - 作品 ID
    /// * `channel` - 通道 ("0": H5, "1": 社区)
    ///
    /// # Returns
    /// 作品数据
    pub fn load_work_data(&self, work_id: i32, channel: &str) -> MewResult<Value> {
        let endpoint = format!("/coconut/web/work/{}/load", work_id);

        let response = self
            .client
            .build_request(HttpMethod::GET, &endpoint, Some(BaseKey::Creation))
            .with_param("channel", channel)
            .send()?;
        self.client.response_to_json(response)
    }
}

impl Default for CoconutCloud {
    fn default() -> Self {
        Self::new()
    }
}

// 云数据库类型枚举
pub enum CloudDatabaseType {
    Dict = 1,
    Table = 2,
}

// CoconutCloudAdmin struct
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
    ///
    /// # Arguments
    /// * `db_type` - 数据库类型 (1: 字典, 2: 数据表, None: 全部)
    ///
    /// # Returns
    /// 数据库列表
    pub fn list_user_databases(&self, db_type: Option<CloudDatabaseType>) -> MewResult<Value> {
        let mut builder = self.client.build_request(
            HttpMethod::GET,
            "/coconut/clouddb/user/list",
            Some(BaseKey::Creation),
        );

        if let Some(db_type_val) = db_type {
            builder = builder.with_param("type", (db_type_val as i32).to_string());
        }

        let response = builder.send()?;
        self.client.response_to_json(response)
    }

    /// 获取用户云数据库详细信息列表
    ///
    /// # Arguments
    /// * `db_type` - 数据库类型 (1: 字典, 2: 数据表)
    ///
    /// # Returns
    /// 数据库详细信息列表
    pub fn list_user_databases_detail(
        &self,
        db_type: Option<CloudDatabaseType>,
    ) -> MewResult<Value> {
        let mut builder = self.client.build_request(
            HttpMethod::GET,
            "/coconut/clouddb/user/list/detail",
            Some(BaseKey::Creation),
        );

        if let Some(db_type_val) = db_type {
            builder = builder.with_param("type", (db_type_val as i32).to_string());
        }

        let response = builder.send()?;
        self.client.response_to_json(response)
    }

    /// 获取作品关联的云字典列表
    ///
    /// # Arguments
    /// * `work_id` - 作品 ID
    ///
    /// # Returns
    /// 云字典列表
    pub fn list_work_dicts(&self, work_id: i32) -> MewResult<Value> {
        let response = self
            .client
            .build_request(
                HttpMethod::GET,
                "/coconut/webdb/admin/dict",
                Some(BaseKey::Creation),
            )
            .with_param("work_id", work_id.to_string())
            .send()?;
        self.client.response_to_json(response)
    }

    /// 按类型获取云字典列表
    ///
    /// # Arguments
    /// * `dict_type` - 字典类型
    ///
    /// # Returns
    /// 云字典列表
    pub fn list_work_dicts_by_type(&self, dict_type: i32) -> MewResult<Value> {
        let response = self
            .client
            .build_request(
                HttpMethod::GET,
                "/coconut/webdb/admin/dict",
                Some(BaseKey::Creation),
            )
            .with_param("type", dict_type.to_string())
            .send()?;
        self.client.response_to_json(response)
    }

    /// 获取云字典条目列表(分页)
    ///
    /// # Arguments
    /// * `dict_id` - 字典 ID
    /// * `work_id` - 作品 ID
    /// * `offset` - 偏移量(从1开始)
    /// * `limit` - 每页数量(默认500)
    ///
    /// # Returns
    /// 字典条目列表
    pub fn get_dict_entries(
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
            .send()?;
        self.client.response_to_json(response)
    }

    /// 迁移云字典环境
    ///
    /// # Arguments
    /// * `db_id` - 数据库 ID
    /// * `from_env` - 源环境 (2: 开发环境?)
    /// * `to_env` - 目标环境 (1: 生产环境?)
    ///
    /// # Returns
    /// 迁移结果
    pub fn migrate_dict(&self, db_id: &str, from_env: i32, to_env: i32) -> MewResult<Value> {
        let data = json!({
            "db_id": db_id,
            "from_env": from_env,
            "to_env": to_env,
        });

        let response = self
            .client
            .build_request(
                HttpMethod::PUT,
                "/coconut/webdb/admin/dict/migrate",
                Some(BaseKey::Creation),
            )
            .with_payload(data)
            .send()?;
        self.client.response_to_json(response)
    }
}

impl Default for CoconutCloudAdmin {
    fn default() -> Self {
        Self::new()
    }
}
