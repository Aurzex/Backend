use crate::utils::acquire::{
    CodeMaoClient, HttpMethod, MewError, MewResult, PaginatedIter, PaginationMethod,
};
use serde_json::{Value, json};
use std::time::{SystemTime, UNIX_EPOCH};

// ==================== 工具函数 ====================

/// 获取13位时间戳
fn current_timestamp_13() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("Time went backwards")
        .as_millis()
}

/// 为请求构建器添加时间戳参数
fn with_timestamp(
    builder: crate::utils::acquire::KittyRequestBuilder,
) -> crate::utils::acquire::KittyRequestBuilder {
    builder.with_param("TIME", current_timestamp_13().to_string())
}

/// 为分页迭代器添加时间戳参数
fn paginated_with_timestamp(paginated: PaginatedIter) -> PaginatedIter {
    paginated.with_param("TIME", current_timestamp_13().to_string())
}

// ==================== EduUserAction ====================

pub struct EduUserAction {
    client: &'static CodeMaoClient,
}

impl EduUserAction {
    pub fn new() -> Self {
        Self {
            client: CodeMaoClient::global(),
        }
    }

    /// 更新用户真实姓名
    pub async fn update_user_real_name(&self, user_id: i32, real_name: &str) -> MewResult<bool> {
        let response = self
            .client
            .build_request(
                HttpMethod::GET,
                "https://eduzone.codemao.cn/edu/zone/account/updateName",
                None,
            )
            .with_param("TIME", current_timestamp_13().to_string())
            .with_param("userId", user_id.to_string())
            .with_param("realName", real_name)
            .send()
            .await?;

        Ok(response.status().is_success())
    }

    /// 创建班级
    pub async fn create_class(&self, name: &str) -> MewResult<Value> {
        self.client
            .build_request(
                HttpMethod::POST,
                "https://eduzone.codemao.cn/edu/zone/class",
                None,
            )
            .with_payload(json!({ "name": name }))
            .send()
            .await?
            .json()
            .await
            .map_err(MewError::from)
    }

    /// 删除班级
    pub async fn delete_class(&self, class_id: i32) -> MewResult<bool> {
        let endpoint = format!("https://eduzone.codemao.cn/edu/zone/class/{}", class_id);

        let response = self
            .client
            .build_request(HttpMethod::DELETE, &endpoint, None)
            .with_param("TIME", current_timestamp_13().to_string())
            .send()
            .await?;

        Ok(response.status().as_u16() == 204)
    }

    /// 向班级添加学生
    pub async fn add_students_to_class(&self, names: &[String], class_id: i32) -> MewResult<bool> {
        let endpoint = format!(
            "https://eduzone.codemao.cn/edu/zone/class/{}/students",
            class_id
        );

        let response = self
            .client
            .build_request(HttpMethod::POST, &endpoint, None)
            .with_payload(json!({ "student_names": names }))
            .send()
            .await?;

        Ok(response.status().is_success())
    }

    /// 重置学生密码
    pub async fn reset_student_password(&self, stu_id: i32) -> MewResult<Value> {
        let endpoint = format!(
            "https://eduzone.codemao.cn/edu/zone/students/{}/password",
            stu_id
        );

        self.client
            .build_request(HttpMethod::PATCH, &endpoint, None)
            .with_payload(json!({}))
            .send()
            .await?
            .json()
            .await
            .map_err(MewError::from)
    }

    /// 批量重置密码
    pub async fn execute_bulk_reset_passwords(&self, stu_list: &[i32]) -> MewResult<Value> {
        self.client
            .build_request(
                HttpMethod::PATCH,
                "https://eduzone.codemao.cn/edu/zone/students/password",
                None,
            )
            .with_payload(json!({ "student_id": stu_list }))
            .send()
            .await?
            .json()
            .await
            .map_err(MewError::from)
    }

    /// 从班级删除学生
    pub async fn delete_student_from_class(&self, stu_id: i32) -> MewResult<bool> {
        let endpoint = format!(
            "https://eduzone.codemao.cn/edu/zone/student/remove/{}",
            stu_id
        );

        let response = self
            .client
            .build_request(HttpMethod::POST, &endpoint, None)
            .with_payload(json!({}))
            .send()
            .await?;

        Ok(response.status().is_success())
    }

    /// 创建或更新课程包
    pub async fn create_or_update_lesson_package(
        &self,
        method: HttpMethod,
        avatar_url: &str,
        description: &str,
        name: &str,
        return_data: bool,
    ) -> MewResult<Value> {
        let response = self
            .client
            .build_request(
                method,
                "https://eduzone.codemao.cn/edu/zone/lesson/customized/packages",
                None,
            )
            .with_payload(json!({
                "avatar_url": avatar_url,
                "description": description,
                "name": name
            }))
            .send()
            .await?;

        if return_data {
            response.json().await.map_err(MewError::from)
        } else {
            Ok(json!({ "success": response.status().is_success() }))
        }
    }

    /// 删除作品
    pub async fn delete_work(&self, work_id: i32) -> MewResult<bool> {
        let endpoint = format!(
            "https://eduzone.codemao.cn/edu/zone/work/{}/delete",
            work_id
        );

        let response = self
            .client
            .build_request(HttpMethod::POST, &endpoint, None)
            .with_payload(json!({}))
            .send()
            .await?;

        Ok(response.status().is_success())
    }

    /// 将学生转移到未分配
    pub async fn execute_transfer_to_unassigned(
        &self,
        class_id: i32,
        stu_id: i32,
    ) -> MewResult<bool> {
        let endpoint = format!(
            "https://eduzone.codemao.cn/edu/zone/class/{}/students",
            class_id
        );

        let response = self
            .client
            .build_request(HttpMethod::DELETE, &endpoint, None)
            .with_param("student_ids[]", stu_id.to_string())
            .send()
            .await?;

        Ok(response.status().as_u16() == 204)
    }

    /// 获取活动包详情
    pub async fn fetch_activity_package_details(&self, package_id: i32) -> MewResult<Value> {
        self.client
            .build_request(
                HttpMethod::POST,
                "https://eduzone.codemao.cn/edu/zone/activity/open/package",
                None,
            )
            .with_payload(json!({ "packageId": package_id }))
            .send()
            .await?
            .json()
            .await
            .map_err(MewError::from)
    }

    /// 获取活动包列表
    pub async fn fetch_activity_packages(&self) -> MewResult<Value> {
        self.client
            .build_request(
                HttpMethod::POST,
                "https://eduzone.codemao.cn/edu/zone/activity/list/activity/package",
                None,
            )
            .with_payload(json!({}))
            .send()
            .await?
            .json()
            .await
            .map_err(MewError::from)
    }

    /// 标记所有消息为已读
    pub async fn execute_mark_all_messages_as_read(&self) -> MewResult<bool> {
        let response = self
            .client
            .build_request(
                HttpMethod::POST,
                "https://eduzone.codemao.cn/edu/zone/invite/message/all/read",
                None,
            )
            .with_payload(json!({}))
            .send()
            .await?;

        Ok(response.status().is_success())
    }

    /// 评分学生作品
    pub async fn execute_grade_student_work(
        &self,
        work_id: i32,
        work_name: &str,
        artistic_score: i32,
        creative_score: i32,
        commentary: &str,
        logical_score: i32,
        programming_score: i32,
    ) -> MewResult<bool> {
        let response = self
            .client
            .build_request(
                HttpMethod::PATCH,
                "https://eduzone.codemao.cn/edu/zone/work/manager/works/scores",
                None,
            )
            .with_payload(json!({
                "artistic_score": artistic_score,
                "commentary": commentary,
                "creative_score": creative_score,
                "id": work_id,
                "logical_score": logical_score,
                "programming_score": programming_score,
                "work_name": work_name
            }))
            .send()
            .await?;

        Ok(response.status().as_u16() == 204)
    }

    /// 邀请加入班级
    pub async fn execute_invite_to_class(
        &self,
        class_id: i32,
        types: &str,
        identity: Value,
    ) -> MewResult<bool> {
        let endpoint = format!(
            "https://eduzone.codemao.cn/edu/zone/class/{}/students/invite",
            class_id
        );

        let response = self
            .client
            .build_request(HttpMethod::POST, &endpoint, None)
            .with_payload(json!({
                "identity": identity,
                "type": types,
                "classId": class_id
            }))
            .send()
            .await?;

        Ok(response.status().is_success())
    }

    /// 接受班级邀请
    pub async fn execute_accept_class_invite(&self, message_id: i32) -> MewResult<bool> {
        let endpoint = format!(
            "https://eduzone.codemao.cn/edu/zone/invite/student/message/{}/accept",
            message_id
        );

        let response = self
            .client
            .build_request(HttpMethod::POST, &endpoint, None)
            .with_payload(json!({}))
            .send()
            .await?;

        Ok(response.status().is_success())
    }

    /// 完善教师信息
    pub async fn execute_improve_teacher_info(
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
        let response = self
            .client
            .build_request(
                HttpMethod::POST,
                "https://eduzone.codemao.cn/edu/zone/sign/login/teacher/info/improve",
                None,
            )
            .with_payload(json!({
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
            }))
            .send()
            .await?;

        Ok(response.status().is_success())
    }
}

impl Default for EduUserAction {
    fn default() -> Self {
        Self::new()
    }
}

// ==================== EduDataFetcher ====================

pub struct EduDataFetcher {
    client: &'static CodeMaoClient,
}

impl EduDataFetcher {
    pub fn new() -> Self {
        Self {
            client: CodeMaoClient::global(),
        }
    }

    /// 获取用户资料
    pub async fn fetch_user_profile(&self) -> MewResult<Value> {
        with_timestamp(self.client.build_request(
            HttpMethod::GET,
            "https://eduzone.codemao.cn/edu/zone",
            None,
        ))
        .send()
        .await?
        .json()
        .await
        .map_err(MewError::from)
    }

    /// 获取账户角色
    pub async fn fetch_account_role(&self) -> MewResult<Value> {
        with_timestamp(self.client.build_request(
            HttpMethod::GET,
            "https://eduzone.codemao.cn/api/home/account",
            None,
        ))
        .send()
        .await?
        .json()
        .await
        .map_err(MewError::from)
    }

    /// 获取未读消息数量
    pub async fn fetch_unread_message_count(&self) -> MewResult<Value> {
        with_timestamp(self.client.build_request(
            HttpMethod::GET,
            "https://eduzone.codemao.cn/edu/zone/system/message/unread/num",
            None,
        ))
        .send()
        .await?
        .json()
        .await
        .map_err(MewError::from)
    }

    /// 获取通知生成器
    pub fn fetch_notices_gen(&self, limit: Option<usize>) -> PaginatedIter {
        paginated_with_timestamp(
            self.client
                .paginated("https://eduzone.codemao.cn/edu/zone/system/message/list")
                .with_param("page", "1")
                .with_param("limit", "10")
                .with_pagination_method(PaginationMethod::Page)
                .with_offset_key("page")
                .with_amount_key("limit")
                .with_limit(limit.unwrap_or(10)),
        )
    }

    /// 获取提醒生成器
    pub fn fetch_reminders_gen(&self, limit: Option<usize>) -> PaginatedIter {
        paginated_with_timestamp(
            self.client
                .paginated("https://eduzone.codemao.cn/edu/zone/invite/teacher/messages")
                .with_param("page", "1")
                .with_param("limit", "10")
                .with_pagination_method(PaginationMethod::Page)
                .with_offset_key("page")
                .with_amount_key("limit")
                .with_limit(limit.unwrap_or(10)),
        )
    }

    /// 获取学校类别
    pub async fn fetch_school_categories(&self) -> MewResult<Value> {
        with_timestamp(self.client.build_request(
            HttpMethod::GET,
            "https://eduzone.codemao.cn/edu/zone/school/open/grade/list",
            None,
        ))
        .send()
        .await?
        .json()
        .await
        .map_err(MewError::from)
    }

    /// 获取教室列表
    pub async fn fetch_classrooms(&self, method: &str, limit: Option<usize>) -> MewResult<Value> {
        match method {
            "simple" => self
                .client
                .build_request(
                    HttpMethod::GET,
                    "https://eduzone.codemao.cn/edu/zone/classes/simple",
                    None,
                )
                .send()
                .await?
                .json()
                .await
                .map_err(MewError::from),
            "detail" => {
                let _paginated = self
                    .client
                    .paginated("https://eduzone.codemao.cn/edu/zone/classes/")
                    .with_param("page", "1")
                    .with_pagination_method(PaginationMethod::Page)
                    .with_offset_key("page")
                    .with_response_amount_key("limit")
                    .with_limit(limit.unwrap_or(20));

                Ok(json!({ "paginated": "Use iterator to fetch data" }))
            }
            _ => Ok(json!({})),
        }
    }

    /// 获取学生移除记录生成器
    pub fn fetch_student_removal_records_gen(&self, limit: Option<usize>) -> PaginatedIter {
        paginated_with_timestamp(
            self.client
                .paginated("https://eduzone.codemao.cn/edu/zone/student/remove/record")
                .with_param("page", "1")
                .with_param("limit", "10")
                .with_pagination_method(PaginationMethod::Page)
                .with_offset_key("page")
                .with_amount_key("limit")
                .with_limit(limit.unwrap_or(10)),
        )
    }

    /// 获取班级学生生成器
    pub fn fetch_class_students_gen(&self, invalid: i32, limit: Option<usize>) -> PaginatedIter {
        self.client
            .paginated("https://eduzone.codemao.cn/edu/zone/students")
            .with_param("page", "1")
            .with_param("limit", "100")
            .with_payload(json!({ "invalid": invalid }))
            .with_method(HttpMethod::POST)
            .with_pagination_method(PaginationMethod::Page)
            .with_offset_key("page")
            .with_amount_key("limit")
            .with_limit(limit.unwrap_or(100))
    }

    /// 获取导航菜单
    pub async fn fetch_navigation_menus(&self) -> MewResult<Value> {
        with_timestamp(self.client.build_request(
            HttpMethod::GET,
            "https://eduzone.codemao.cn/api/home/eduzone/menus",
            None,
        ))
        .send()
        .await?
        .json()
        .await
        .map_err(MewError::from)
    }

    /// 获取教育横幅
    pub async fn fetch_edu_banners(&self, type_id: i32) -> MewResult<Value> {
        with_timestamp(
            self.client
                .build_request(
                    HttpMethod::GET,
                    "https://eduzone.codemao.cn/api/home/banners",
                    None,
                )
                .with_param("type_id", type_id.to_string()),
        )
        .send()
        .await?
        .json()
        .await
        .map_err(MewError::from)
    }

    /// 获取服务器时间
    pub async fn fetch_server_time(&self) -> MewResult<Value> {
        with_timestamp(self.client.build_request(
            HttpMethod::GET,
            "https://eduzone.codemao.cn/edu/base/server/time",
            None,
        ))
        .send()
        .await?
        .json()
        .await
        .map_err(MewError::from)
    }

    /// 获取课程包状态
    pub async fn fetch_lesson_package_status(&self) -> MewResult<Value> {
        with_timestamp(self.client.build_request(
            HttpMethod::GET,
            "https://eduzone.codemao.cn/edu/zone/lessons/person/package/remind/status",
            None,
        ))
        .send()
        .await?
        .json()
        .await
        .map_err(MewError::from)
    }

    /// 获取配置
    pub async fn fetch_configuration(&self, tag: &str) -> MewResult<Value> {
        with_timestamp(
            self.client
                .build_request(
                    HttpMethod::GET,
                    "https://eduzone.codemao.cn/edu/base/general/conf",
                    None,
                )
                .with_param("tag", tag),
        )
        .send()
        .await?
        .json()
        .await
        .map_err(MewError::from)
    }

    /// 获取扩展资料
    pub async fn fetch_extended_profile(&self) -> MewResult<Value> {
        with_timestamp(self.client.build_request(
            HttpMethod::GET,
            "https://eduzone.codemao.cn/edu/zone/user-extend/info",
            None,
        ))
        .send()
        .await?
        .json()
        .await
        .map_err(MewError::from)
    }

    /// 获取操作日志
    pub async fn fetch_operation_logs(&self) -> MewResult<Value> {
        with_timestamp(self.client.build_request(
            HttpMethod::GET,
            "https://eduzone.codemao.cn/edu/zone/operation/records",
            None,
        ))
        .send()
        .await?
        .json()
        .await
        .map_err(MewError::from)
    }

    /// 获取教学状态
    pub async fn fetch_teaching_status(&self) -> MewResult<Value> {
        with_timestamp(self.client.build_request(
            HttpMethod::GET,
            "https://eduzone.codemao.cn/edu/zone/teaching/class/remind",
            None,
        ))
        .send()
        .await?
        .json()
        .await
        .map_err(MewError::from)
    }

    /// 获取仪表板统计
    pub async fn fetch_dashboard_stats(&self) -> MewResult<Value> {
        with_timestamp(self.client.build_request(
            HttpMethod::GET,
            "https://eduzone.codemao.cn/edu/zone/homepage/statistic",
            None,
        ))
        .send()
        .await?
        .json()
        .await
        .map_err(MewError::from)
    }

    /// 获取工具菜单
    pub async fn fetch_tool_menu(&self) -> MewResult<Value> {
        with_timestamp(self.client.build_request(
            HttpMethod::GET,
            "https://eduzone.codemao.cn/edu/zone/homepage/menus",
            None,
        ))
        .send()
        .await?
        .json()
        .await
        .map_err(MewError::from)
    }

    /// 获取所有作品生成器
    pub fn fetch_all_works_gen(&self, limit: Option<usize>) -> PaginatedIter {
        paginated_with_timestamp(
            self.client
                .paginated("https://eduzone.codemao.cn/edu/zone/work/manager/student/works")
                .with_param("page", "1")
                .with_pagination_method(PaginationMethod::Page)
                .with_offset_key("page")
                .with_response_amount_key("limit")
                .with_limit(limit.unwrap_or(50)),
        )
    }

    /// 获取管理作品生成器
    pub fn fetch_managed_works_gen(&self, limit: Option<usize>) -> PaginatedIter {
        paginated_with_timestamp(
            self.client
                .paginated("https://eduzone.codemao.cn/edu/zone/work/manager/works")
                .with_param("page", "1")
                .with_pagination_method(PaginationMethod::Page)
                .with_offset_key("page")
                .with_response_amount_key("limit")
                .with_limit(limit.unwrap_or(50)),
        )
    }

    /// 获取个人作品生成器
    pub fn fetch_personal_works_gen(&self, limit: Option<usize>) -> PaginatedIter {
        paginated_with_timestamp(
            self.client
                .paginated("https://eduzone.codemao.cn/edu/zone/work/manager/self/works")
                .with_param("page", "1")
                .with_pagination_method(PaginationMethod::Page)
                .with_offset_key("page")
                .with_response_amount_key("limit")
                .with_limit(limit.unwrap_or(50)),
        )
    }

    /// 获取作品分析
    pub async fn fetch_work_analytics(
        &self,
        class_id: Option<i32>,
        year: i32,
        month: i32,
    ) -> MewResult<Value> {
        let mut builder = self
            .client
            .build_request(
                HttpMethod::GET,
                "https://eduzone.codemao.cn/edu/zone/work/manager/works/statistics",
                None,
            )
            .with_param("year", year.to_string())
            .with_param("month", format!("{:02}", month));

        if let Some(cid) = class_id {
            builder = builder.with_param("class_id", cid.to_string());
        }

        with_timestamp(builder)
            .send()
            .await?
            .json()
            .await
            .map_err(MewError::from)
    }

    /// 获取教学记录生成器
    pub fn fetch_teaching_records_gen(&self, limit: Option<usize>) -> PaginatedIter {
        paginated_with_timestamp(
            self.client
                .paginated("https://eduzone.codemao.cn/edu/zone/teaching/record/list")
                .with_param("page", "1")
                .with_param("limit", "10")
                .with_pagination_method(PaginationMethod::Page)
                .with_offset_key("page")
                .with_amount_key("limit")
                .with_limit(limit.unwrap_or(10)),
        )
    }

    /// 获取教学班级
    pub async fn fetch_teaching_classes(&self) -> MewResult<Value> {
        with_timestamp(self.client.build_request(
            HttpMethod::GET,
            "https://eduzone.codemao.cn/edu/zone/teaching/class/teacher/list",
            None,
        ))
        .send()
        .await?
        .json()
        .await
        .map_err(MewError::from)
    }

    /// 获取学校信息
    pub async fn fetch_school_info(&self, unit_id: i32) -> MewResult<Value> {
        with_timestamp(
            self.client
                .build_request(
                    HttpMethod::GET,
                    "https://eduzone.codemao.cn/edu/zone/school/info",
                    None,
                )
                .with_param("unitId", unit_id.to_string()),
        )
        .send()
        .await?
        .json()
        .await
        .map_err(MewError::from)
    }

    /// 获取官方课程包生成器
    pub fn fetch_official_lesson_packages_gen(&self, limit: Option<usize>) -> PaginatedIter {
        paginated_with_timestamp(
            self.client
                .paginated("https://eduzone.codemao.cn/edu/zone/lesson/offical/packages")
                .with_param("pacakgeEntryType", "0")
                .with_param("topicType", "all")
                .with_param("topicId", "all")
                .with_param("tagId", "all")
                .with_param("page", "1")
                .with_param("limit", "150")
                .with_pagination_method(PaginationMethod::Page)
                .with_offset_key("page")
                .with_amount_key("limit")
                .with_limit(limit.unwrap_or(150)),
        )
    }

    /// 获取课程主题
    pub async fn fetch_lesson_topics(&self) -> MewResult<Value> {
        with_timestamp(
            self.client
                .build_request(
                    HttpMethod::GET,
                    "https://eduzone.codemao.cn/edu/zone/lessons/official/packages/topics",
                    None,
                )
                .with_param("pacakgeEntryType", "0")
                .with_param("topicType", "all"),
        )
        .send()
        .await?
        .json()
        .await
        .map_err(MewError::from)
    }

    /// 获取课程标签
    pub async fn fetch_lesson_tags(&self) -> MewResult<Value> {
        with_timestamp(
            self.client
                .build_request(
                    HttpMethod::GET,
                    "https://eduzone.codemao.cn/edu/zone/lessons/official/packages/topics/all/tags",
                    None,
                )
                .with_param("pacakgeEntryType", "0")
                .with_param("topicType", "all"),
        )
        .send()
        .await?
        .json()
        .await
        .map_err(MewError::from)
    }

    /// 获取自定义课程包生成器
    pub fn fetch_custom_lesson_packages_gen(&self, limit: Option<usize>) -> PaginatedIter {
        paginated_with_timestamp(
            self.client
                .paginated("https://eduzone.codemao.cn/edu/zone/lesson/offical/packages")
                .with_param("page", "1")
                .with_param("limit", "100")
                .with_pagination_method(PaginationMethod::Page)
                .with_offset_key("page")
                .with_amount_key("limit")
                .with_limit(limit.unwrap_or(100)),
        )
    }

    /// 获取或删除自定义课程包
    pub async fn get_or_delete_custom_package(
        &self,
        package_id: i32,
        method: HttpMethod,
    ) -> MewResult<Value> {
        let endpoint = format!(
            "https://eduzone.codemao.cn/edu/zone/lesson/customized/packages/{}",
            package_id
        );

        let response = with_timestamp(self.client.build_request(method, &endpoint, None))
            .send()
            .await?;

        if method == HttpMethod::GET {
            response.json().await.map_err(MewError::from)
        } else {
            Ok(json!({ "success": response.status().is_success() }))
        }
    }

    /// 获取自定义课程包内容
    pub async fn fetch_custom_package_contents(
        &self,
        package_id: i32,
        limit: i32,
    ) -> MewResult<Value> {
        with_timestamp(
            self.client
                .build_request(
                    HttpMethod::GET,
                    "https://eduzone.codemao.cn/edu/zone/lesson/customized/package/lessons",
                    None,
                )
                .with_param("limit", limit.to_string())
                .with_param("package_id", package_id.to_string()),
        )
        .send()
        .await?
        .json()
        .await
        .map_err(MewError::from)
    }

    /// 获取班级邀请
    pub async fn fetch_class_invites(&self) -> MewResult<Value> {
        with_timestamp(self.client.build_request(
            HttpMethod::GET,
            "https://eduzone.codemao.cn/edu/zone/invite/student/message/next",
            None,
        ))
        .send()
        .await?
        .json()
        .await
        .map_err(MewError::from)
    }

    /// 获取即将过期的课程
    pub async fn fetch_expiring_lessons(&self) -> MewResult<Value> {
        with_timestamp(self.client.build_request(
            HttpMethod::GET,
            "https://eduzone.codemao.cn/edu/zone/lesson/offical/packages/expired",
            None,
        ))
        .send()
        .await?
        .json()
        .await
        .map_err(MewError::from)
    }

    /// 获取组织ID (外部URL)
    pub async fn fetch_organization_ids(&self) -> MewResult<Value> {
        let timestamp = current_timestamp_13();
        let url = format!(
            "https://static.codemao.cn/teacher-edu/organization_ids.json?CMTIME={}",
            timestamp
        );
        reqwest::get(&url)
            .await
            .map_err(MewError::from)?
            .json()
            .await
            .map_err(MewError::from)
    }

    /// 获取报告元数据
    pub async fn fetch_report_metadata(&self) -> MewResult<Value> {
        with_timestamp(self.client.build_request(
            HttpMethod::GET,
            "https://eduzone.codemao.cn/edu/zone/analysis/report/info",
            None,
        ))
        .send()
        .await?
        .json()
        .await
        .map_err(MewError::from)
    }

    /// 获取课程分析
    pub async fn fetch_course_analytics(&self) -> MewResult<Value> {
        with_timestamp(self.client.build_request(
            HttpMethod::GET,
            "https://eduzone.codemao.cn/edu/zone/analysis/student/course",
            None,
        ))
        .send()
        .await?
        .json()
        .await
        .map_err(MewError::from)
    }

    /// 获取课程包分析
    pub async fn fetch_lesson_package_analytics(&self) -> MewResult<Value> {
        with_timestamp(self.client.build_request(
            HttpMethod::GET,
            "https://eduzone.codemao.cn/edu/zone/analysis/student/packages",
            None,
        ))
        .send()
        .await?
        .json()
        .await
        .map_err(MewError::from)
    }

    /// 获取教室分析
    pub async fn fetch_classroom_analytics(&self) -> MewResult<Value> {
        with_timestamp(self.client.build_request(
            HttpMethod::GET,
            "https://eduzone.codemao.cn/edu/zone/analysis/student/class/info",
            None,
        ))
        .send()
        .await?
        .json()
        .await
        .map_err(MewError::from)
    }

    /// 获取作品表现
    pub async fn fetch_work_performance(&self) -> MewResult<Value> {
        with_timestamp(self.client.build_request(
            HttpMethod::GET,
            "https://eduzone.codemao.cn/edu/zone/analysis/student/works/situations",
            None,
        ))
        .send()
        .await?
        .json()
        .await
        .map_err(MewError::from)
    }

    /// 获取作品评分
    pub async fn fetch_work_ratings(&self) -> MewResult<Value> {
        with_timestamp(self.client.build_request(
            HttpMethod::GET,
            "https://eduzone.codemao.cn/edu/zone/analysis/student/works/star/info",
            None,
        ))
        .send()
        .await?
        .json()
        .await
        .map_err(MewError::from)
    }

    /// 获取技能评估
    pub async fn fetch_skill_assessment(&self) -> MewResult<Value> {
        with_timestamp(self.client.build_request(
            HttpMethod::GET,
            "https://eduzone.codemao.cn/edu/zone/analysis/student/ability/dimensions",
            None,
        ))
        .send()
        .await?
        .json()
        .await
        .map_err(MewError::from)
    }

    /// 获取技能雷达
    pub async fn fetch_skill_radar(&self) -> MewResult<Value> {
        with_timestamp(self.client.build_request(
            HttpMethod::GET,
            "https://eduzone.codemao.cn/edu/zone/analysis/student/ability/radars",
            None,
        ))
        .send()
        .await?
        .json()
        .await
        .map_err(MewError::from)
    }

    /// 获取美术技能
    pub async fn fetch_art_skills(&self) -> MewResult<Value> {
        with_timestamp(self.client.build_request(
            HttpMethod::GET,
            "https://eduzone.codemao.cn/edu/zone/analysis/student/ability/artistic/dimensions",
            None,
        ))
        .send()
        .await?
        .json()
        .await
        .map_err(MewError::from)
    }

    /// 获取逻辑技能
    pub async fn fetch_logic_skills(&self) -> MewResult<Value> {
        with_timestamp(self.client.build_request(
            HttpMethod::GET,
            "https://eduzone.codemao.cn/edu/zone/analysis/student/ability/logical/dimensions",
            None,
        ))
        .send()
        .await?
        .json()
        .await
        .map_err(MewError::from)
    }

    /// 获取编程技能
    pub async fn fetch_coding_skills(&self) -> MewResult<Value> {
        with_timestamp(self.client.build_request(
            HttpMethod::GET,
            "https://eduzone.codemao.cn/edu/zone/analysis/student/ability/programming/dimensions",
            None,
        ))
        .send()
        .await?
        .json()
        .await
        .map_err(MewError::from)
    }
}

impl Default for EduDataFetcher {
    fn default() -> Self {
        Self::new()
    }
}
