use crate::utils::acquire::{
    CodeMaoClient, HTTPStatus, HttpMethod, KittyRequestBuilder, MewResult, PaginatedIter,
    PaginationMethod,
};
use log::debug;
use serde_json::{Value, json};
use std::time::{SystemTime, UNIX_EPOCH};

// ==================== 工具函数 ====================

/// 获取 13 位毫秒时间戳（本地时间）。
///
/// 若系统时间异常（早于 Unix 纪元），则返回 0 并记录警告。
fn current_timestamp_13() -> u128 {
    match SystemTime::now().duration_since(UNIX_EPOCH) {
        Ok(dur) => dur.as_millis(),
        Err(_) => {
            log::warn!("系统时间异常，无法获取时间戳，返回 0");
            0
        }
    }
}

// ==================== 教育用户操作 ====================

/// 教育管理相关操作（班级、学生、作业等）。
pub struct EduUserAction {
    client: &'static CodeMaoClient,
}

impl EduUserAction {
    pub fn new() -> Self {
        Self {
            client: CodeMaoClient::global(),
        }
    }

    // ---------- 私有辅助 ----------

    /// 发送请求并返回 status == 预期状态码
    fn check_status(&self, builder: KittyRequestBuilder, expected: HTTPStatus) -> MewResult<bool> {
        let response = builder.send()?;
        Ok(response.status() == expected as u16)
    }

    // ---------- 公共方法 ----------

    /// 更新用户真实姓名
    pub fn update_user_real_name(&self, user_id: i32, real_name: &str) -> MewResult<bool> {
        debug!(
            "更新用户真实姓名: user_id={}, real_name={}",
            user_id, real_name
        );
        let timestamp = current_timestamp_13();
        let builder = self
            .client
            .build_request(
                HttpMethod::Get,
                "https://eduzone.codemao.cn/edu/zone/account/updateName",
                None,
            )
            .with_param("TIME", timestamp.to_string())
            .with_param("userId", user_id.to_string())
            .with_param("realName", real_name);
        self.check_status(builder, HTTPStatus::Ok)
    }

    /// 创建班级
    pub fn create_class(&self, name: &str) -> MewResult<Value> {
        debug!("创建班级: name={}", name);
        let data = json!({ "name": name });
        let response = self
            .client
            .build_request(
                HttpMethod::Post,
                "https://eduzone.codemao.cn/edu/zone/class",
                None,
            )
            .with_payload(data)
            .send()?;
        self.client.response_to_json(response)
    }

    /// 重命名班级
    pub fn rename_class(&self, class_id: i32, class_name: &str) -> MewResult<bool> {
        debug!("重命名班级: class_id={}, new_name={}", class_id, class_name);
        let timestamp = current_timestamp_13();
        let data = json!({ "name": class_name });
        let endpoint = format!("https://eduzone.codemao.cn/edu/zone/class/{}", class_id);
        let builder = self
            .client
            .build_request(HttpMethod::Patch, &endpoint, None)
            .with_param("TIME", timestamp.to_string())
            .with_payload(data);
        self.check_status(builder, HTTPStatus::NoContent)
    }

    /// 删除班级
    pub fn delete_class(&self, class_id: i32) -> MewResult<bool> {
        debug!("删除班级: class_id={}", class_id);
        let timestamp = current_timestamp_13();
        let endpoint = format!("https://eduzone.codemao.cn/edu/zone/class/{}", class_id);
        let builder = self
            .client
            .build_request(HttpMethod::Delete, &endpoint, None)
            .with_param("TIME", timestamp.to_string());
        self.check_status(builder, HTTPStatus::NoContent)
    }

    /// 向班级添加学生
    pub fn add_students_to_class(&self, names: &[String], class_id: i32) -> MewResult<bool> {
        debug!("添加学生到班级: class_id={}, names={:?}", class_id, names);
        let data = json!({ "student_names": names });
        let endpoint = format!(
            "https://eduzone.codemao.cn/edu/zone/class/{}/students",
            class_id
        );
        let builder = self
            .client
            .build_request(HttpMethod::Post, &endpoint, None)
            .with_payload(data);
        self.check_status(builder, HTTPStatus::Ok)
    }

    /// 重置学生密码
    pub fn reset_student_password(&self, stu_id: i32) -> MewResult<Value> {
        debug!("重置学生密码: stu_id={}", stu_id);
        let endpoint = format!(
            "https://eduzone.codemao.cn/edu/zone/students/{}/password",
            stu_id
        );
        let response = self
            .client
            .build_request(HttpMethod::Patch, &endpoint, None)
            .with_payload(json!({}))
            .send()?;
        self.client.response_to_json(response)
    }

    /// 批量重置学生密码
    pub fn execute_bulk_reset_passwords(&self, stu_list: &[i32]) -> MewResult<Value> {
        debug!("批量重置密码: students={:?}", stu_list);
        let data = json!({ "student_id": stu_list });
        let response = self
            .client
            .build_request(
                HttpMethod::Patch,
                "https://eduzone.codemao.cn/edu/zone/students/password",
                None,
            )
            .with_payload(data)
            .send()?;
        self.client.response_to_json(response)
    }

    /// 从班级移除学生
    pub fn delete_student_from_class(&self, stu_id: i32) -> MewResult<bool> {
        debug!("从班级移除学生: stu_id={}", stu_id);
        let endpoint = format!(
            "https://eduzone.codemao.cn/edu/zone/student/remove/{}",
            stu_id
        );
        let builder = self
            .client
            .build_request(HttpMethod::Post, &endpoint, None)
            .with_payload(json!({}));
        self.check_status(builder, HTTPStatus::Ok)
    }

    /// 创建或更新自定义课程包
    pub fn create_or_update_lesson_package(
        &self,
        method: HttpMethod,
        avatar_url: &str,
        description: &str,
        name: &str,
        return_data: bool,
    ) -> MewResult<Value> {
        debug!("创建/更新课程包: method={:?}, name={}", method, name);
        let data = json!({
            "avatar_url": avatar_url,
            "description": description,
            "name": name
        });
        let response = self
            .client
            .build_request(
                method,
                "https://eduzone.codemao.cn/edu/zone/lesson/customized/packages",
                None,
            )
            .with_payload(data)
            .send()?;
        if return_data {
            self.client.response_to_json(response)
        } else {
            Ok(json!({ "success": response.status() == HTTPStatus::Ok as u16 }))
        }
    }

    /// 删除作品
    pub fn delete_work(&self, work_id: i32) -> MewResult<bool> {
        debug!("删除作品: work_id={}", work_id);
        let endpoint = format!(
            "https://eduzone.codemao.cn/edu/zone/work/{}/delete",
            work_id
        );
        let builder = self
            .client
            .build_request(HttpMethod::Post, &endpoint, None)
            .with_payload(json!({}));
        self.check_status(builder, HTTPStatus::Ok)
    }

    /// 将学生转移到未分班
    pub fn execute_transfer_to_unassigned(&self, class_id: i32, stu_id: i32) -> MewResult<bool> {
        debug!("转移学生到未分班: class_id={}, stu_id={}", class_id, stu_id);
        let endpoint = format!(
            "https://eduzone.codemao.cn/edu/zone/class/{}/students",
            class_id
        );
        let builder = self
            .client
            .build_request(HttpMethod::Delete, &endpoint, None)
            .with_param("student_ids[]", stu_id.to_string());
        self.check_status(builder, HTTPStatus::NoContent)
    }

    /// 获取活动包详情
    pub fn fetch_activity_package_details(&self, package_id: i32) -> MewResult<Value> {
        debug!("获取活动包详情: package_id={}", package_id);
        let data = json!({ "packageId": package_id });
        let response = self
            .client
            .build_request(
                HttpMethod::Post,
                "https://eduzone.codemao.cn/edu/zone/activity/open/package",
                None,
            )
            .with_payload(data)
            .send()?;
        self.client.response_to_json(response)
    }

    /// 获取活动包列表
    pub fn fetch_activity_packages(&self) -> MewResult<Value> {
        debug!("获取活动包列表");
        let response = self
            .client
            .build_request(
                HttpMethod::Post,
                "https://eduzone.codemao.cn/edu/zone/activity/list/activity/package",
                None,
            )
            .with_payload(json!({}))
            .send()?;
        self.client.response_to_json(response)
    }

    /// 标记所有消息为已读
    pub fn execute_mark_all_messages_as_read(&self) -> MewResult<bool> {
        debug!("标记所有消息为已读");
        let builder = self
            .client
            .build_request(
                HttpMethod::Post,
                "https://eduzone.codemao.cn/edu/zone/invite/message/all/read",
                None,
            )
            .with_payload(json!({}));
        self.check_status(builder, HTTPStatus::Ok)
    }

    /// 为学生作品评分
    pub fn execute_grade_student_work(
        &self,
        work_id: i32,
        work_name: &str,
        artistic_score: i32,
        creative_score: i32,
        commentary: &str,
        logical_score: i32,
        programming_score: i32,
    ) -> MewResult<bool> {
        debug!("评分作品: work_id={}, name={}", work_id, work_name);
        let data = json!({
            "artistic_score": artistic_score,
            "commentary": commentary,
            "creative_score": creative_score,
            "id": work_id,
            "logical_score": logical_score,
            "programming_score": programming_score,
            "work_name": work_name
        });
        let builder = self
            .client
            .build_request(
                HttpMethod::Patch,
                "https://eduzone.codemao.cn/edu/zone/work/manager/works/scores",
                None,
            )
            .with_payload(data);
        self.check_status(builder, HTTPStatus::NoContent)
    }

    /// 邀请学生加入班级
    pub fn execute_invite_to_class(
        &self,
        class_id: i32,
        types: &str,
        identity: Value,
    ) -> MewResult<bool> {
        debug!("邀请学生加入班级: class_id={}, type={}", class_id, types);
        let data = json!({
            "identity": identity,
            "type": types,
            "classId": class_id
        });
        let endpoint = format!(
            "https://eduzone.codemao.cn/edu/zone/class/{}/students/invite",
            class_id
        );
        let builder = self
            .client
            .build_request(HttpMethod::Post, &endpoint, None)
            .with_payload(data);
        self.check_status(builder, HTTPStatus::Ok)
    }

    /// 接受班级邀请
    pub fn execute_accept_class_invite(&self, message_id: i32) -> MewResult<bool> {
        debug!("接受班级邀请: message_id={}", message_id);
        let endpoint = format!(
            "https://eduzone.codemao.cn/edu/zone/invite/student/message/{}/accept",
            message_id
        );
        let builder = self
            .client
            .build_request(HttpMethod::Post, &endpoint, None)
            .with_payload(json!({}));
        self.check_status(builder, HTTPStatus::Ok)
    }

    /// 完善教师信息
    pub fn execute_improve_teacher_info(
        &self,
        user_id: i32,
        real_name: &str,
        grade: Vec<String>,
        school_id: i32,
        school_name: &str,
        school_type: i32,
        country_id: &str,
        province_id: i32,
        city_id: i32,
        district_id: i32,
        teacher_card_number: &str,
    ) -> MewResult<bool> {
        debug!("完善教师信息: user_id={}", user_id);
        let data = json!({
            "id": user_id,
            "real_name": real_name,
            "grade": grade,
            "schoolId": school_id,
            "schoolName": school_name,
            "schoolType": school_type,
            "country_id": country_id,
            "province_id": province_id,
            "city_id": city_id,
            "district_id": district_id,
            "teacherCardNumber": teacher_card_number
        });
        let builder = self
            .client
            .build_request(
                HttpMethod::Post,
                "https://eduzone.codemao.cn/edu/zone/sign/login/teacher/info/improve",
                None,
            )
            .with_payload(data);
        self.check_status(builder, HTTPStatus::Ok)
    }
}

impl Default for EduUserAction {
    fn default() -> Self {
        Self::new()
    }
}

// ==================== 教育数据获取器 ====================

/// 教育平台数据查询接口。
pub struct EduDataFetcher {
    client: &'static CodeMaoClient,
}

impl EduDataFetcher {
    pub fn new() -> Self {
        Self {
            client: CodeMaoClient::global(),
        }
    }

    // ---------- 私有辅助 ----------

    /// 为请求构建器附加当前时间戳参数 `TIME`。
    fn add_timestamp_to_builder(builder: KittyRequestBuilder) -> KittyRequestBuilder {
        let timestamp = current_timestamp_13();
        builder.with_param("TIME", timestamp.to_string())
    }

    /// 为分页迭代器附加当前时间戳参数 `TIME`。
    fn add_timestamp_to_paginated(paginated: PaginatedIter) -> PaginatedIter {
        let timestamp = current_timestamp_13();
        paginated.with_iter_param("TIME", timestamp.to_string())
    }

    /// 发送请求并将响应解析为 JSON。
    fn send_and_parse(&self, builder: KittyRequestBuilder) -> MewResult<Value> {
        let response = builder.send()?;
        self.client.response_to_json(response)
    }

    /// 构建一个基础分页迭代器，使用 Page 分页方式，初始页码 1。
    fn build_paginated(
        &self,
        endpoint: &str,
        page_size: usize,
        default_limit: usize,
    ) -> PaginatedIter {
        self.client
            .paginated(endpoint)
            .with_iter_param("page", "1")
            .with_page_size(page_size)
            .with_pagination_method(PaginationMethod::Page)
            .with_offset_key("page")
            .with_amount_key("limit")
            .with_limit(default_limit)
    }

    // ---------- 公共方法 ----------

    /// 获取用户基本信息
    pub fn fetch_user_profile(&self) -> MewResult<Value> {
        debug!("获取用户基本信息");
        let builder =
            self.client
                .build_request(HttpMethod::Get, "https://eduzone.codemao.cn/edu/zone", None);
        let builder = Self::add_timestamp_to_builder(builder);
        self.send_and_parse(builder)
    }

    /// 获取账号角色
    pub fn fetch_account_role(&self) -> MewResult<Value> {
        debug!("获取账号角色");
        let builder = self.client.build_request(
            HttpMethod::Get,
            "https://eduzone.codemao.cn/api/home/account",
            None,
        );
        let builder = Self::add_timestamp_to_builder(builder);
        self.send_and_parse(builder)
    }

    /// 获取未读消息数量
    pub fn fetch_unread_message_count(&self) -> MewResult<Value> {
        debug!("获取未读消息数量");
        let builder = self.client.build_request(
            HttpMethod::Get,
            "https://eduzone.codemao.cn/edu/zone/system/message/unread/num",
            None,
        );
        let builder = Self::add_timestamp_to_builder(builder);
        self.send_and_parse(builder)
    }

    /// 系统通知分页迭代器
    pub fn fetch_notices_gen(&self, limit: Option<usize>) -> PaginatedIter {
        debug!("获取系统通知迭代器");
        let mut paginated = self.build_paginated(
            "https://eduzone.codemao.cn/edu/zone/system/message/list",
            10,
            limit.unwrap_or(10),
        );
        paginated = Self::add_timestamp_to_paginated(paginated);
        paginated
    }

    /// 教师提醒消息分页迭代器
    pub fn fetch_reminders_gen(&self, limit: Option<usize>) -> PaginatedIter {
        debug!("获取教师提醒迭代器");
        let mut paginated = self.build_paginated(
            "https://eduzone.codemao.cn/edu/zone/invite/teacher/messages",
            10,
            limit.unwrap_or(10),
        );
        paginated = Self::add_timestamp_to_paginated(paginated);
        paginated
    }

    /// 获取学校年级列表
    pub fn fetch_school_categories(&self) -> MewResult<Value> {
        debug!("获取学校年级列表");
        let builder = self.client.build_request(
            HttpMethod::Get,
            "https://eduzone.codemao.cn/edu/zone/school/open/grade/list",
            None,
        );
        let builder = Self::add_timestamp_to_builder(builder);
        self.send_and_parse(builder)
    }

    /// 获取班级简要列表
    pub fn fetch_classrooms_simple(&self) -> MewResult<Value> {
        debug!("获取班级简要列表");
        let response = self
            .client
            .build_request(
                HttpMethod::Get,
                "https://eduzone.codemao.cn/edu/zone/classes/simple",
                None,
            )
            .send()?;
        self.client.response_to_json(response)
    }

    /// 班级详细信息分页迭代器，可按名称搜索
    pub fn fetch_classrooms_detail(
        &self,
        limit: Option<usize>,
        class_name: Option<&str>,
    ) -> PaginatedIter {
        debug!("获取班级详细迭代器: class_name={:?}", class_name);
        let mut paginated = self
            .build_paginated(
                "https://eduzone.codemao.cn/edu/zone/classes/",
                20, // 默认页面大小 20
                limit.unwrap_or(20),
            )
            .with_response_amount_key("limit"); // 服务器返回的实际每页大小键
        if let Some(name) = class_name {
            paginated = paginated.with_iter_param("class_name", name);
        }
        paginated = Self::add_timestamp_to_paginated(paginated);
        paginated
    }

    /// 学生移除记录分页迭代器
    pub fn fetch_student_removal_records_gen(&self, limit: Option<usize>) -> PaginatedIter {
        debug!("获取学生移除记录迭代器");
        let mut paginated = self.build_paginated(
            "https://eduzone.codemao.cn/edu/zone/student/remove/record",
            10,
            limit.unwrap_or(10),
        );
        paginated = Self::add_timestamp_to_paginated(paginated);
        paginated
    }

    /// 班级学生列表分页迭代器（支持无效/有效筛选）
    pub fn fetch_class_students_gen(&self, invalid: i32, limit: Option<usize>) -> PaginatedIter {
        debug!("获取班级学生迭代器: invalid={}", invalid);
        let data = json!({ "invalid": invalid });
        
        self
            .client
            .paginated("https://eduzone.codemao.cn/edu/zone/students")
            .with_iter_param("page", "1")
            .with_page_size(100)
            .with_iter_payload(data)
            .with_iter_method(HttpMethod::Post)
            .with_pagination_method(PaginationMethod::Page)
            .with_offset_key("page")
            .with_amount_key("limit")
            .with_limit(limit.unwrap_or(100))
    }

    /// 获取导航菜单
    pub fn fetch_navigation_menus(&self) -> MewResult<Value> {
        debug!("获取导航菜单");
        let builder = self.client.build_request(
            HttpMethod::Get,
            "https://eduzone.codemao.cn/api/home/eduzone/menus",
            None,
        );
        let builder = Self::add_timestamp_to_builder(builder);
        self.send_and_parse(builder)
    }

    /// 获取教育平台 Banner
    pub fn fetch_edu_banners(&self, type_id: i32) -> MewResult<Value> {
        debug!("获取教育Banner: type_id={}", type_id);
        let builder = self
            .client
            .build_request(
                HttpMethod::Get,
                "https://eduzone.codemao.cn/api/home/banners",
                None,
            )
            .with_param("type_id", type_id.to_string());
        let builder = Self::add_timestamp_to_builder(builder);
        self.send_and_parse(builder)
    }

    /// 获取服务器时间
    pub fn fetch_server_time(&self) -> MewResult<Value> {
        debug!("获取服务器时间");
        let builder = self.client.build_request(
            HttpMethod::Get,
            "https://eduzone.codemao.cn/edu/base/server/time",
            None,
        );
        let builder = Self::add_timestamp_to_builder(builder);
        self.send_and_parse(builder)
    }

    /// 获取课程包提醒状态
    pub fn fetch_lesson_package_status(&self) -> MewResult<Value> {
        debug!("获取课程包提醒状态");
        let builder = self.client.build_request(
            HttpMethod::Get,
            "https://eduzone.codemao.cn/edu/zone/lessons/person/package/remind/status",
            None,
        );
        let builder = Self::add_timestamp_to_builder(builder);
        self.send_and_parse(builder)
    }

    /// 获取通用配置
    pub fn fetch_configuration(&self, tag: &str) -> MewResult<Value> {
        debug!("获取配置: tag={}", tag);
        let builder = self
            .client
            .build_request(
                HttpMethod::Get,
                "https://eduzone.codemao.cn/edu/base/general/conf",
                None,
            )
            .with_param("tag", tag);
        let builder = Self::add_timestamp_to_builder(builder);
        self.send_and_parse(builder)
    }

    /// 获取扩展用户资料
    pub fn fetch_extended_profile(&self) -> MewResult<Value> {
        debug!("获取扩展用户资料");
        let builder = self.client.build_request(
            HttpMethod::Get,
            "https://eduzone.codemao.cn/edu/zone/user-extend/info",
            None,
        );
        let builder = Self::add_timestamp_to_builder(builder);
        self.send_and_parse(builder)
    }

    /// 获取操作日志
    pub fn fetch_operation_logs(&self) -> MewResult<Value> {
        debug!("获取操作日志");
        let builder = self.client.build_request(
            HttpMethod::Get,
            "https://eduzone.codemao.cn/edu/zone/operation/records",
            None,
        );
        let builder = Self::add_timestamp_to_builder(builder);
        self.send_and_parse(builder)
    }

    /// 获取教学提醒状态
    pub fn fetch_teaching_status(&self) -> MewResult<Value> {
        debug!("获取教学提醒状态");
        let builder = self.client.build_request(
            HttpMethod::Get,
            "https://eduzone.codemao.cn/edu/zone/teaching/class/remind",
            None,
        );
        let builder = Self::add_timestamp_to_builder(builder);
        self.send_and_parse(builder)
    }

    /// 获取仪表盘统计数据
    pub fn fetch_dashboard_stats(&self) -> MewResult<Value> {
        debug!("获取仪表盘统计数据");
        let builder = self.client.build_request(
            HttpMethod::Get,
            "https://eduzone.codemao.cn/edu/zone/homepage/statistic",
            None,
        );
        let builder = Self::add_timestamp_to_builder(builder);
        self.send_and_parse(builder)
    }

    /// 获取工具箱菜单
    pub fn fetch_tool_menu(&self) -> MewResult<Value> {
        debug!("获取工具箱菜单");
        let builder = self.client.build_request(
            HttpMethod::Get,
            "https://eduzone.codemao.cn/edu/zone/homepage/menus",
            None,
        );
        let builder = Self::add_timestamp_to_builder(builder);
        self.send_and_parse(builder)
    }

    /// 所有作品分页迭代器
    pub fn fetch_all_works_gen(&self, limit: Option<usize>) -> PaginatedIter {
        debug!("获取所有作品迭代器");
        let mut paginated = self
            .build_paginated(
                "https://eduzone.codemao.cn/edu/zone/work/manager/student/works",
                50,
                limit.unwrap_or(50),
            )
            .with_response_amount_key("limit");
        paginated = Self::add_timestamp_to_paginated(paginated);
        paginated
    }

    /// 管理作品分页迭代器
    pub fn fetch_managed_works_gen(&self, limit: Option<usize>) -> PaginatedIter {
        debug!("获取管理作品迭代器");
        let mut paginated = self
            .build_paginated(
                "https://eduzone.codemao.cn/edu/zone/work/manager/works",
                50,
                limit.unwrap_or(50),
            )
            .with_response_amount_key("limit");
        paginated = Self::add_timestamp_to_paginated(paginated);
        paginated
    }

    /// 个人作品分页迭代器
    pub fn fetch_personal_works_gen(&self, limit: Option<usize>) -> PaginatedIter {
        debug!("获取个人作品迭代器");
        let mut paginated = self
            .build_paginated(
                "https://eduzone.codemao.cn/edu/zone/work/manager/self/works",
                50,
                limit.unwrap_or(50),
            )
            .with_response_amount_key("limit");
        paginated = Self::add_timestamp_to_paginated(paginated);
        paginated
    }

    /// 作品统计分析
    pub fn fetch_work_analytics(
        &self,
        class_id: Option<i32>,
        year: i32,
        month: i32,
    ) -> MewResult<Value> {
        debug!(
            "获取作品分析: class_id={:?}, year={}, month={}",
            class_id, year, month
        );
        let mut builder = self
            .client
            .build_request(
                HttpMethod::Get,
                "https://eduzone.codemao.cn/edu/zone/work/manager/works/statistics",
                None,
            )
            .with_param("year", year.to_string())
            .with_param("month", format!("{:02}", month));
        if let Some(cid) = class_id {
            builder = builder.with_param("class_id", cid.to_string());
        }
        builder = Self::add_timestamp_to_builder(builder);
        self.send_and_parse(builder)
    }

    /// 教学记录分页迭代器
    pub fn fetch_teaching_records_gen(&self, limit: Option<usize>) -> PaginatedIter {
        debug!("获取教学记录迭代器");
        let mut paginated = self.build_paginated(
            "https://eduzone.codemao.cn/edu/zone/teaching/record/list",
            10,
            limit.unwrap_or(10),
        );
        paginated = Self::add_timestamp_to_paginated(paginated);
        paginated
    }

    /// 获取教师班级列表
    pub fn fetch_teaching_classes(&self) -> MewResult<Value> {
        debug!("获取教师班级列表");
        let builder = self.client.build_request(
            HttpMethod::Get,
            "https://eduzone.codemao.cn/edu/zone/teaching/class/teacher/list",
            None,
        );
        let builder = Self::add_timestamp_to_builder(builder);
        self.send_and_parse(builder)
    }

    /// 获取学校信息
    pub fn fetch_school_info(&self, unit_id: i32) -> MewResult<Value> {
        debug!("获取学校信息: unit_id={}", unit_id);
        let builder = self
            .client
            .build_request(
                HttpMethod::Get,
                "https://eduzone.codemao.cn/edu/zone/school/info",
                None,
            )
            .with_param("unitId", unit_id.to_string());
        let builder = Self::add_timestamp_to_builder(builder);
        self.send_and_parse(builder)
    }

    /// 官方课程包分页迭代器
    pub fn fetch_official_lesson_packages_gen(&self, limit: Option<usize>) -> PaginatedIter {
        debug!("获取官方课程包迭代器");
        let mut paginated = self
            .client
            .paginated("https://eduzone.codemao.cn/edu/zone/lesson/offical/packages")
            .with_iter_param("pacakgeEntryType", "0")
            .with_iter_param("topicType", "all")
            .with_iter_param("topicId", "all")
            .with_iter_param("tagId", "all")
            .with_iter_param("page", "1")
            .with_page_size(150)
            .with_pagination_method(PaginationMethod::Page)
            .with_offset_key("page")
            .with_amount_key("limit")
            .with_limit(limit.unwrap_or(150));
        paginated = Self::add_timestamp_to_paginated(paginated);
        paginated
    }

    /// 获取课程主题列表
    pub fn fetch_lesson_topics(&self) -> MewResult<Value> {
        debug!("获取课程主题");
        let builder = self
            .client
            .build_request(
                HttpMethod::Get,
                "https://eduzone.codemao.cn/edu/zone/lessons/official/packages/topics",
                None,
            )
            .with_param("pacakgeEntryType", "0")
            .with_param("topicType", "all");
        let builder = Self::add_timestamp_to_builder(builder);
        self.send_and_parse(builder)
    }

    /// 获取课程标签列表
    pub fn fetch_lesson_tags(&self) -> MewResult<Value> {
        debug!("获取课程标签");
        let builder = self
            .client
            .build_request(
                HttpMethod::Get,
                "https://eduzone.codemao.cn/edu/zone/lessons/official/packages/topics/all/tags",
                None,
            )
            .with_param("pacakgeEntryType", "0")
            .with_param("topicType", "all");
        let builder = Self::add_timestamp_to_builder(builder);
        self.send_and_parse(builder)
    }

    /// 自定义课程包分页迭代器
    pub fn fetch_custom_lesson_packages_gen(&self, limit: Option<usize>) -> PaginatedIter {
        debug!("获取自定义课程包迭代器");
        let mut paginated = self.build_paginated(
            "https://eduzone.codemao.cn/edu/zone/lesson/offical/packages",
            100,
            limit.unwrap_or(100),
        );
        paginated = Self::add_timestamp_to_paginated(paginated);
        paginated
    }

    /// 获取或删除自定义课程包
    pub fn get_or_delete_custom_package(
        &self,
        package_id: i32,
        method: HttpMethod,
    ) -> MewResult<Value> {
        debug!(
            "获取/删除自定义课程包: package_id={}, method={:?}",
            package_id, method
        );
        let endpoint = format!(
            "https://eduzone.codemao.cn/edu/zone/lesson/customized/packages/{}",
            package_id
        );
        let builder = self.client.build_request(method, &endpoint, None);
        let builder = Self::add_timestamp_to_builder(builder);
        let response = builder.send()?;
        if method == HttpMethod::Get {
            self.client.response_to_json(response)
        } else {
            Ok(json!({ "success": response.status() == HTTPStatus::Ok as u16 }))
        }
    }

    /// 获取自定义课程包内容
    pub fn fetch_custom_package_contents(&self, package_id: i32, limit: i32) -> MewResult<Value> {
        debug!(
            "获取自定义课程包内容: package_id={}, limit={}",
            package_id, limit
        );
        let builder = self
            .client
            .build_request(
                HttpMethod::Get,
                "https://eduzone.codemao.cn/edu/zone/lesson/customized/package/lessons",
                None,
            )
            .with_param("limit", limit.to_string())
            .with_param("package_id", package_id.to_string());
        let builder = Self::add_timestamp_to_builder(builder);
        self.send_and_parse(builder)
    }

    /// 获取班级邀请
    pub fn fetch_class_invites(&self) -> MewResult<Value> {
        debug!("获取班级邀请");
        let builder = self.client.build_request(
            HttpMethod::Get,
            "https://eduzone.codemao.cn/edu/zone/invite/student/message/next",
            None,
        );
        let builder = Self::add_timestamp_to_builder(builder);
        self.send_and_parse(builder)
    }

    /// 获取即将过期的课程包
    pub fn fetch_expiring_lessons(&self) -> MewResult<Value> {
        debug!("获取即将过期课程包");
        let builder = self.client.build_request(
            HttpMethod::Get,
            "https://eduzone.codemao.cn/edu/zone/lesson/offical/packages/expired",
            None,
        );
        let builder = Self::add_timestamp_to_builder(builder);
        self.send_and_parse(builder)
    }

    /// 获取组织 ID 列表
    pub fn fetch_organization_ids(&self) -> MewResult<Value> {
        debug!("获取组织ID列表");
        let timestamp = current_timestamp_13();
        let response = self
            .client
            .build_request(
                HttpMethod::Get,
                "https://static.codemao.cn/teacher-edu/organization_ids.json",
                None,
            )
            .with_param("CMTIME", timestamp.to_string())
            .send()?;
        self.client.response_to_json(response)
    }

    // ---------- 数据分析相关 ----------

    /// 获取报告元数据
    pub fn fetch_report_metadata(&self) -> MewResult<Value> {
        debug!("获取报告元数据");
        let builder = self.client.build_request(
            HttpMethod::Get,
            "https://eduzone.codemao.cn/edu/zone/analysis/report/info",
            None,
        );
        let builder = Self::add_timestamp_to_builder(builder);
        self.send_and_parse(builder)
    }

    /// 获取课程分析
    pub fn fetch_course_analytics(&self) -> MewResult<Value> {
        debug!("获取课程分析");
        let builder = self.client.build_request(
            HttpMethod::Get,
            "https://eduzone.codemao.cn/edu/zone/analysis/student/course",
            None,
        );
        let builder = Self::add_timestamp_to_builder(builder);
        self.send_and_parse(builder)
    }

    /// 获取课程包分析
    pub fn fetch_lesson_package_analytics(&self) -> MewResult<Value> {
        debug!("获取课程包分析");
        let builder = self.client.build_request(
            HttpMethod::Get,
            "https://eduzone.codemao.cn/edu/zone/analysis/student/packages",
            None,
        );
        let builder = Self::add_timestamp_to_builder(builder);
        self.send_and_parse(builder)
    }

    /// 获取班级分析
    pub fn fetch_classroom_analytics(&self) -> MewResult<Value> {
        debug!("获取班级分析");
        let builder = self.client.build_request(
            HttpMethod::Get,
            "https://eduzone.codemao.cn/edu/zone/analysis/student/class/info",
            None,
        );
        let builder = Self::add_timestamp_to_builder(builder);
        self.send_and_parse(builder)
    }

    /// 获取作品表现
    pub fn fetch_work_performance(&self) -> MewResult<Value> {
        debug!("获取作品表现");
        let builder = self.client.build_request(
            HttpMethod::Get,
            "https://eduzone.codemao.cn/edu/zone/analysis/student/works/situations",
            None,
        );
        let builder = Self::add_timestamp_to_builder(builder);
        self.send_and_parse(builder)
    }

    /// 获取作品评分分布
    pub fn fetch_work_ratings(&self) -> MewResult<Value> {
        debug!("获取作品评分分布");
        let builder = self.client.build_request(
            HttpMethod::Get,
            "https://eduzone.codemao.cn/edu/zone/analysis/student/works/star/info",
            None,
        );
        let builder = Self::add_timestamp_to_builder(builder);
        self.send_and_parse(builder)
    }

    /// 获取技能评估维度
    pub fn fetch_skill_assessment(&self) -> MewResult<Value> {
        debug!("获取技能评估维度");
        let builder = self.client.build_request(
            HttpMethod::Get,
            "https://eduzone.codemao.cn/edu/zone/analysis/student/ability/dimensions",
            None,
        );
        let builder = Self::add_timestamp_to_builder(builder);
        self.send_and_parse(builder)
    }

    /// 获取技能雷达图
    pub fn fetch_skill_radar(&self) -> MewResult<Value> {
        debug!("获取技能雷达图");
        let builder = self.client.build_request(
            HttpMethod::Get,
            "https://eduzone.codemao.cn/edu/zone/analysis/student/ability/radars",
            None,
        );
        let builder = Self::add_timestamp_to_builder(builder);
        self.send_and_parse(builder)
    }

    /// 获取艺术技能维度
    pub fn fetch_art_skills(&self) -> MewResult<Value> {
        debug!("获取艺术技能维度");
        let builder = self.client.build_request(
            HttpMethod::Get,
            "https://eduzone.codemao.cn/edu/zone/analysis/student/ability/artistic/dimensions",
            None,
        );
        let builder = Self::add_timestamp_to_builder(builder);
        self.send_and_parse(builder)
    }

    /// 获取逻辑技能维度
    pub fn fetch_logic_skills(&self) -> MewResult<Value> {
        debug!("获取逻辑技能维度");
        let builder = self.client.build_request(
            HttpMethod::Get,
            "https://eduzone.codemao.cn/edu/zone/analysis/student/ability/logical/dimensions",
            None,
        );
        let builder = Self::add_timestamp_to_builder(builder);
        self.send_and_parse(builder)
    }

    /// 获取编程技能维度
    pub fn fetch_coding_skills(&self) -> MewResult<Value> {
        debug!("获取编程技能维度");
        let builder = self.client.build_request(
            HttpMethod::Get,
            "https://eduzone.codemao.cn/edu/zone/analysis/student/ability/programming/dimensions",
            None,
        );
        let builder = Self::add_timestamp_to_builder(builder);
        self.send_and_parse(builder)
    }
}

impl Default for EduDataFetcher {
    fn default() -> Self {
        Self::new()
    }
}
