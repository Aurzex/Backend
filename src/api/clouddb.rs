use crate::utils::acquire::{BaseKey, ClientAccess, CodeMaoClient, HttpMethod, MewResult};
use log::debug;
use serde_json::{Value, json};

// 排行榜管理

/// 排行榜管理器,封装全量更新,增删查等操作
pub struct Ranking {
    client: &'static CodeMaoClient,
}

impl Ranking {
    pub fn new() -> Self {
        Self {
            client: CodeMaoClient::global(),
        }
    }

    /// 更新排行榜(全量更新)
    pub fn update_ranking_list(&self, data: Value) -> MewResult<Value> {
        debug!("正在全量更新排行榜");
        self.send_and_parse(
            self.client
                .build_request(
                    HttpMethod::Put,
                    "/neko/ranking-list/fullUpdate",
                    Some(BaseKey::Creation),
                )
                .with_payload(data),
        )
    }

    /// 清空指定排行榜
    pub fn clear_ranking_list(&self, ranking_id: &str) -> MewResult<Value> {
        debug!("清空排行榜: id={}", ranking_id);
        self.send_and_parse(
            self.client
                .build_request(
                    HttpMethod::Put,
                    "/neko/ranking-list/clear",
                    Some(BaseKey::Creation),
                )
                .with_param("id", ranking_id),
        )
    }

    /// 获取排行榜记录
    pub fn fetch_ranking_records(&self, ranking_id: &str, work_id: i32) -> MewResult<Value> {
        debug!("获取排行榜记录: id={}, work_id={}", ranking_id, work_id);
        self.send_and_parse(
            self.client
                .build_request(
                    HttpMethod::Get,
                    "/neko/ranking-list/record/list",
                    Some(BaseKey::Creation),
                )
                .with_param("id", ranking_id)
                .with_param("work_id", work_id.to_string()),
        )
    }

    /// 添加排行榜记录
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
        debug!("添加排行榜记录: {:?}", data);
        self.send_and_parse(
            self.client
                .build_request(
                    HttpMethod::Post,
                    "/neko/ranking-list/record",
                    Some(BaseKey::Creation),
                )
                .with_payload(data),
        )
    }

    /// 创建新排行榜
    pub fn create_ranking_list(&self, data: Value) -> MewResult<Value> {
        debug!("创建排行榜: {:?}", data);
        self.send_and_parse(
            self.client
                .build_request(
                    HttpMethod::Post,
                    "/neko/ranking-list",
                    Some(BaseKey::Creation),
                )
                .with_payload(data),
        )
    }

    /// 删除排行榜
    pub fn delete_ranking_list(&self, ranking_id: &str, work_id: i32) -> MewResult<Value> {
        let endpoint = format!("/neko/ranking-list/{}", ranking_id);
        debug!("删除排行榜: id={}, work_id={}", ranking_id, work_id);
        self.send_and_parse(
            self.client
                .build_request(HttpMethod::Delete, &endpoint, Some(BaseKey::Creation))
                .with_param("id", ranking_id)
                .with_param("work_id", work_id.to_string()),
        )
    }
}

impl Default for Ranking {
    fn default() -> Self {
        Self::new()
    }
}

// 云字典/数据表操作(用户端)

/// 云数据操作器(普通用户权限),用于操作云字典和云数据表
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
    /// 根据值的 JSON 类型自动推断 `type` 字段
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
        debug!(
            "设置云字典值: dict={}, key={}, type={}",
            dict_id, key, type_name
        );
        let endpoint = format!("/coconut/webdb/try/dict/{}/set", dict_id);
        self.send_and_parse(
            self.client
                .build_request(HttpMethod::Post, &endpoint, Some(BaseKey::Creation))
                .with_payload(data),
        )
    }

    /// 删除云字典中的键
    pub fn delete_dictionary_key(&self, dict_id: &str, key: &str) -> MewResult<Value> {
        debug!("删除云字典键: dict={}, key={}", dict_id, key);
        let endpoint = format!("/coconut/webdb/try/dict/{}/remove", dict_id);
        self.send_and_parse(
            self.client
                .build_request(HttpMethod::Delete, &endpoint, Some(BaseKey::Creation))
                .with_param("key", key),
        )
    }

    /// 清空云字典
    pub fn clear_dictionary(&self, dict_id: &str) -> MewResult<Value> {
        debug!("清空云字典: dict={}", dict_id);
        let endpoint = format!("/coconut/webdb/try/dict/clear/{}", dict_id);
        self.send_and_parse(
            self.client
                .build_request(HttpMethod::Delete, &endpoint, Some(BaseKey::Creation)),
        )
    }

    /// 获取云字典的所有键
    pub fn get_dictionary_keys(&self, dict_id: &str) -> MewResult<Value> {
        debug!("获取云字典所有键: dict={}", dict_id);
        let endpoint = format!("/coconut/webdb/try/dict/{}/keys", dict_id);
        self.send_and_parse(
            self.client
                .build_request(HttpMethod::Get, &endpoint, Some(BaseKey::Creation)),
        )
    }

    /// 获取云字典中指定键的值
    pub fn get_dictionary_value(&self, dict_id: &str, key: &str) -> MewResult<Value> {
        debug!("获取云字典值: dict={}, key={}", dict_id, key);
        let endpoint = format!("/coconut/webdb/try/dict/{}/getvalue", dict_id);
        self.send_and_parse(
            self.client
                .build_request(HttpMethod::Get, &endpoint, Some(BaseKey::Creation))
                .with_param("key", key),
        )
    }

    /// 查询云数据表
    pub fn query_table(&self, table_id: &str, queries: Value) -> MewResult<Value> {
        let data = json!({
            "querys": {
                "querys": queries
            }
        });
        debug!("查询云数据表: table={}", table_id);
        let endpoint = format!("/coconut/clouddb/runtime/{}/select", table_id);
        self.send_and_parse(
            self.client
                .build_request(HttpMethod::Post, &endpoint, Some(BaseKey::Creation))
                .with_payload(data),
        )
    }

    /// 更新云数据表行
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
        debug!("更新云数据表: table={}", table_id);
        let endpoint = format!("/coconut/clouddb/runtime/{}/update", table_id);
        self.send_and_parse(
            self.client
                .build_request(HttpMethod::Put, &endpoint, Some(BaseKey::Creation))
                .with_payload(data),
        )
    }

    /// 插入云数据表行
    pub fn insert_table_rows(&self, table_id: &str, values: Value) -> MewResult<Value> {
        let data = json!({
            "values": values
        });
        debug!("插入云数据表: table={}", table_id);
        let endpoint = format!("/coconut/clouddb/runtime/{}/insert", table_id);
        self.send_and_parse(
            self.client
                .build_request(HttpMethod::Post, &endpoint, Some(BaseKey::Creation))
                .with_payload(data),
        )
    }

    /// 删除云数据表行
    pub fn delete_table_rows(&self, table_id: &str, queries: Value) -> MewResult<Value> {
        let data = json!({
            "querys": {
                "querys": queries
            }
        });
        debug!("删除云数据表行: table={}", table_id);
        let endpoint = format!("/coconut/clouddb/runtime/{}/delete", table_id);
        self.send_and_parse(
            self.client
                .build_request(HttpMethod::Put, &endpoint, Some(BaseKey::Creation))
                .with_payload(data),
        )
    }

    /// 清空云数据表
    pub fn clear_table(&self, table_id: &str) -> MewResult<Value> {
        debug!("清空云数据表: table={}", table_id);
        let endpoint = format!("/coconut/clouddb/v2/runtime/{}/clear", table_id);
        self.send_and_parse(
            self.client
                .build_request(HttpMethod::Put, &endpoint, Some(BaseKey::Creation)),
        )
    }

    /// 获取云数据表的行数
    pub fn get_table_row_count(&self, table_id: &str) -> MewResult<Value> {
        debug!("获取数据表行数: table={}", table_id);
        let endpoint = format!("/coconut/clouddb/runtime/{}/count", table_id);
        self.send_and_parse(
            self.client
                .build_request(HttpMethod::Get, &endpoint, Some(BaseKey::Creation))
                .with_param("type", "RECORD"),
        )
    }

    /// 获取多个云数据表的信息
    pub fn get_table_info(&self, table_ids: &[String]) -> MewResult<Value> {
        let ids_str = table_ids.join(",");
        debug!("获取数据表信息: ids={}", ids_str);
        self.send_and_parse(
            self.client
                .build_request(
                    HttpMethod::Get,
                    "/coconut/clouddb/v2/runtime/list",
                    Some(BaseKey::Creation),
                )
                .with_param("db_ids", ids_str),
        )
    }

    /// 加载作品数据(H5 或社区版)
    pub fn load_work_data(&self, work_id: i32, channel: &str) -> MewResult<Value> {
        debug!("加载作品数据: work_id={}, channel={}", work_id, channel);
        let endpoint = format!("/coconut/web/work/{}/load", work_id);
        self.send_and_parse(
            self.client
                .build_request(HttpMethod::Get, &endpoint, Some(BaseKey::Creation))
                .with_param("channel", channel),
        )
    }
}

impl Default for CoconutCloud {
    fn default() -> Self {
        Self::new()
    }
}

// 云数据库管理(管理员权限)

/// 云数据库类型枚举
#[derive(Debug, Clone, Copy)]
pub enum CloudDatabaseType {
    Dict = 1,
    Table = 2,
}

/// 云数据库管理员操作器,用于查询,迁移等管理功能
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
    pub fn list_user_databases(&self, db_type: Option<CloudDatabaseType>) -> MewResult<Value> {
        let mut builder = self.client.build_request(
            HttpMethod::Get,
            "/coconut/clouddb/user/list",
            Some(BaseKey::Creation),
        );
        if let Some(db_type_val) = db_type {
            builder = builder.with_param("type", (db_type_val as i32).to_string());
        }
        debug!("获取用户数据库列表: type={:?}", db_type);
        self.send_and_parse(builder)
    }

    /// 获取用户云数据库详细信息列表
    pub fn list_user_databases_detail(
        &self,
        db_type: Option<CloudDatabaseType>,
    ) -> MewResult<Value> {
        let mut builder = self.client.build_request(
            HttpMethod::Get,
            "/coconut/clouddb/user/list/detail",
            Some(BaseKey::Creation),
        );
        if let Some(db_type_val) = db_type {
            builder = builder.with_param("type", (db_type_val as i32).to_string());
        }
        debug!("获取用户数据库详情: type={:?}", db_type);
        self.send_and_parse(builder)
    }

    /// 获取作品关联的云字典列表
    pub fn list_work_dicts(&self, work_id: i32) -> MewResult<Value> {
        debug!("获取作品云字典: work_id={}", work_id);
        self.send_and_parse(
            self.client
                .build_request(
                    HttpMethod::Get,
                    "/coconut/webdb/admin/dict",
                    Some(BaseKey::Creation),
                )
                .with_param("work_id", work_id.to_string()),
        )
    }

    /// 按类型获取云字典列表
    pub fn list_work_dicts_by_type(&self, dict_type: i32) -> MewResult<Value> {
        debug!("按类型获取云字典: type={}", dict_type);
        self.send_and_parse(
            self.client
                .build_request(
                    HttpMethod::Get,
                    "/coconut/webdb/admin/dict",
                    Some(BaseKey::Creation),
                )
                .with_param("type", dict_type.to_string()),
        )
    }

    /// 获取云字典条目列表(分页)
    pub fn get_dict_entries(
        &self,
        dict_id: i32,
        work_id: i32,
        offset: Option<i32>,
        limit: Option<i32>,
    ) -> MewResult<Value> {
        let endpoint = format!("/coconut/webdb/admin/dict/{}", dict_id);
        let offset_val = offset.unwrap_or(1);
        let limit_val = limit.unwrap_or(500);
        debug!(
            "获取字典条目: dict={}, work={}, offset={}, limit={}",
            dict_id, work_id, offset_val, limit_val
        );
        self.send_and_parse(
            self.client
                .build_request(HttpMethod::Get, &endpoint, Some(BaseKey::Creation))
                .with_param("work_id", work_id.to_string())
                .with_param("offset", offset_val.to_string())
                .with_param("limit", limit_val.to_string()),
        )
    }

    /// 迁移云字典环境
    pub fn migrate_dict(&self, db_id: &str, from_env: i32, to_env: i32) -> MewResult<Value> {
        let data = json!({
            "db_id": db_id,
            "from_env": from_env,
            "to_env": to_env,
        });
        debug!("迁移云字典: db={}, from={}, to={}", db_id, from_env, to_env);
        self.send_and_parse(
            self.client
                .build_request(
                    HttpMethod::Put,
                    "/coconut/webdb/admin/dict/migrate",
                    Some(BaseKey::Creation),
                )
                .with_payload(data),
        )
    }
}

impl Default for CoconutCloudAdmin {
    fn default() -> Self {
        Self::new()
    }
}

impl ClientAccess for Ranking {
    fn client(&self) -> &CodeMaoClient {
        self.client
    }
}

impl ClientAccess for CoconutCloud {
    fn client(&self) -> &CodeMaoClient {
        self.client
    }
}

impl ClientAccess for CoconutCloudAdmin {
    fn client(&self) -> &CodeMaoClient {
        self.client
    }
}
