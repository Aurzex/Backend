use crate::utils::acquire::{
    CodeMaoClient, HTTPStatus, HttpMethod, PaginatedIter, PaginationMethod,
};
use serde_json::{Value, json};
use std::collections::HashMap;
use std::time::{SystemTime, UNIX_EPOCH};

// 工具函数：获取13位时间戳
fn current_timestamp_13() -> u128 {
    let start = SystemTime::now();
    let since_the_epoch = start
        .duration_since(UNIX_EPOCH)
        .expect("Time went backwards");
    since_the_epoch.as_millis()
}

pub struct UserAction {
    client: &'static CodeMaoClient,
}

impl UserAction {
    pub fn new() -> Self {
        Self {
            client: CodeMaoClient::global(),
        }
    }

    pub fn update_user_real_name(
        &self,
        user_id: i32,
        real_name: &str,
    ) -> Result<bool, Box<dyn std::error::Error>> {
        let timestamp = current_timestamp_13();
        let mut params = HashMap::new();
        params.insert("TIME".to_string(), timestamp.to_string());
        params.insert("userId".to_string(), user_id.to_string());
        params.insert("realName".to_string(), real_name.to_string());

        let response = self.client.send_request(
            HttpMethod::GET,
            "https://eduzone.codemao.cn/edu/zone/account/updateName",
            Some(&params),
            None,
            None,
        )?;
        Ok(response.status() == HTTPStatus::Ok as u16)
    }

    pub fn create_class(&self, name: &str) -> Result<Value, Box<dyn std::error::Error>> {
        let data = json!({ "name": name });
        let response = self.client.send_request(
            HttpMethod::POST,
            "https://eduzone.codemao.cn/edu/zone/class",
            None,
            Some(&data),
            None,
        )?;
        Ok(self.client.response_to_json(response)?)
    }

    pub fn delete_class(&self, class_id: i32) -> Result<bool, Box<dyn std::error::Error>> {
        let timestamp = current_timestamp_13();
        let mut params = HashMap::new();
        params.insert("TIME".to_string(), timestamp.to_string());

        let endpoint = format!("https://eduzone.codemao.cn/edu/zone/class/{}", class_id);
        let response =
            self.client
                .send_request(HttpMethod::DELETE, &endpoint, Some(&params), None, None)?;
        Ok(response.status() == HTTPStatus::NoContent as u16)
    }

    pub fn add_students_to_class(
        &self,
        names: &[String],
        class_id: i32,
    ) -> Result<bool, Box<dyn std::error::Error>> {
        let data = json!({ "student_names": names });
        let endpoint = format!(
            "https://eduzone.codemao.cn/edu/zone/class/{}/students",
            class_id
        );
        let response =
            self.client
                .send_request(HttpMethod::POST, &endpoint, None, Some(&data), None)?;
        Ok(response.status() == HTTPStatus::Ok as u16)
    }

    pub fn reset_student_password(&self, stu_id: i32) -> Result<Value, Box<dyn std::error::Error>> {
        let endpoint = format!(
            "https://eduzone.codemao.cn/edu/zone/students/{}/password",
            stu_id
        );
        let response =
            self.client
                .send_request(HttpMethod::PATCH, &endpoint, None, Some(&json!({})), None)?;
        Ok(self.client.response_to_json(response)?)
    }

    pub fn execute_bulk_reset_passwords(
        &self,
        stu_list: &[i32],
    ) -> Result<Value, Box<dyn std::error::Error>> {
        let data = json!({ "student_id": stu_list });
        let response = self.client.send_request(
            HttpMethod::PATCH,
            "https://eduzone.codemao.cn/edu/zone/students/password",
            None,
            Some(&data),
            None,
        )?;
        Ok(self.client.response_to_json(response)?)
    }

    pub fn delete_student_from_class(
        &self,
        stu_id: i32,
    ) -> Result<bool, Box<dyn std::error::Error>> {
        let endpoint = format!(
            "https://eduzone.codemao.cn/edu/zone/student/remove/{}",
            stu_id
        );
        let response =
            self.client
                .send_request(HttpMethod::POST, &endpoint, None, Some(&json!({})), None)?;
        Ok(response.status() == HTTPStatus::Ok as u16)
    }

    pub fn create_or_update_lesson_package(
        &self,
        method: HttpMethod,
        avatar_url: &str,
        description: &str,
        name: &str,
        return_data: bool,
    ) -> Result<Value, Box<dyn std::error::Error>> {
        let data = json!({
            "avatar_url": avatar_url,
            "description": description,
            "name": name
        });
        let response = self.client.send_request(
            method,
            "https://eduzone.codemao.cn/edu/zone/lesson/customized/packages",
            None,
            Some(&data),
            None,
        )?;

        if return_data {
            Ok(self.client.response_to_json(response)?)
        } else {
            Ok(json!({ "success": response.status() == HTTPStatus::Ok as u16 }))
        }
    }

    pub fn delete_work(&self, work_id: i32) -> Result<bool, Box<dyn std::error::Error>> {
        let endpoint = format!(
            "https://eduzone.codemao.cn/edu/zone/work/{}/delete",
            work_id
        );
        let response =
            self.client
                .send_request(HttpMethod::POST, &endpoint, None, Some(&json!({})), None)?;
        Ok(response.status() == HTTPStatus::Ok as u16)
    }

    pub fn execute_transfer_to_unassigned(
        &self,
        class_id: i32,
        stu_id: i32,
    ) -> Result<bool, Box<dyn std::error::Error>> {
        let mut params = HashMap::new();
        params.insert("student_ids[]".to_string(), stu_id.to_string());
        let endpoint = format!(
            "https://eduzone.codemao.cn/edu/zone/class/{}/students",
            class_id
        );
        let response =
            self.client
                .send_request(HttpMethod::DELETE, &endpoint, Some(&params), None, None)?;
        Ok(response.status() == HTTPStatus::NoContent as u16)
    }

    pub fn fetch_activity_package_details(
        &self,
        package_id: i32,
    ) -> Result<Value, Box<dyn std::error::Error>> {
        let data = json!({ "packageId": package_id });
        let response = self.client.send_request(
            HttpMethod::POST,
            "https://eduzone.codemao.cn/edu/zone/activity/open/package",
            None,
            Some(&data),
            None,
        )?;
        Ok(self.client.response_to_json(response)?)
    }

    pub fn fetch_activity_packages(&self) -> Result<Value, Box<dyn std::error::Error>> {
        let response = self.client.send_request(
            HttpMethod::POST,
            "https://eduzone.codemao.cn/edu/zone/activity/list/activity/package",
            None,
            Some(&json!({})),
            None,
        )?;
        Ok(self.client.response_to_json(response)?)
    }

    pub fn execute_mark_all_messages_as_read(&self) -> Result<bool, Box<dyn std::error::Error>> {
        let response = self.client.send_request(
            HttpMethod::POST,
            "https://eduzone.codemao.cn/edu/zone/invite/message/all/read",
            None,
            Some(&json!({})),
            None,
        )?;
        Ok(response.status() == HTTPStatus::Ok as u16)
    }

    pub fn execute_grade_student_work(
        &self,
        work_id: i32,
        work_name: &str,
        artistic_score: i32,
        creative_score: i32,
        commentary: &str,
        logical_score: i32,
        programming_score: i32,
    ) -> Result<bool, Box<dyn std::error::Error>> {
        let data = json!({
            "artistic_score": artistic_score,
            "commentary": commentary,
            "creative_score": creative_score,
            "id": work_id,
            "logical_score": logical_score,
            "programming_score": programming_score,
            "work_name": work_name
        });
        let response = self.client.send_request(
            HttpMethod::PATCH,
            "https://eduzone.codemao.cn/edu/zone/work/manager/works/scores",
            None,
            Some(&data),
            None,
        )?;
        Ok(response.status() == HTTPStatus::NoContent as u16)
    }

    pub fn execute_invite_to_class(
        &self,
        class_id: i32,
        types: &str,
        identity: Value,
    ) -> Result<bool, Box<dyn std::error::Error>> {
        let data = json!({
            "identity": identity,
            "type": types,
            "classId": class_id
        });
        let endpoint = format!(
            "https://eduzone.codemao.cn/edu/zone/class/{}/students/invite",
            class_id
        );
        let response =
            self.client
                .send_request(HttpMethod::POST, &endpoint, None, Some(&data), None)?;
        Ok(response.status() == HTTPStatus::Ok as u16)
    }

    pub fn execute_accept_class_invite(
        &self,
        message_id: i32,
    ) -> Result<bool, Box<dyn std::error::Error>> {
        let endpoint = format!(
            "https://eduzone.codemao.cn/edu/zone/invite/student/message/{}/accept",
            message_id
        );
        let response =
            self.client
                .send_request(HttpMethod::POST, &endpoint, None, Some(&json!({})), None)?;
        Ok(response.status() == HTTPStatus::Ok as u16)
    }

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
    ) -> Result<bool, Box<dyn std::error::Error>> {
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
        let response = self.client.send_request(
            HttpMethod::POST,
            "https://eduzone.codemao.cn/edu/zone/sign/login/teacher/info/improve",
            None,
            Some(&data),
            None,
        )?;
        Ok(response.status() == HTTPStatus::Ok as u16)
    }
}

impl Default for UserAction {
    fn default() -> Self {
        Self::new()
    }
}

pub struct DataFetcher {
    client: &'static CodeMaoClient,
}

impl DataFetcher {
    pub fn new() -> Self {
        Self {
            client: CodeMaoClient::global(),
        }
    }

    fn add_timestamp_params(params: &mut HashMap<String, String>) {
        let timestamp = current_timestamp_13();
        params.insert("TIME".to_string(), timestamp.to_string());
    }

    pub fn fetch_user_profile(&self) -> Result<Value, Box<dyn std::error::Error>> {
        let mut params = HashMap::new();
        Self::add_timestamp_params(&mut params);
        let response = self.client.send_request(
            HttpMethod::GET,
            "https://eduzone.codemao.cn/edu/zone",
            Some(&params),
            None,
            None,
        )?;
        Ok(self.client.response_to_json(response)?)
    }

    pub fn fetch_account_role(&self) -> Result<Value, Box<dyn std::error::Error>> {
        let mut params = HashMap::new();
        Self::add_timestamp_params(&mut params);
        let response = self.client.send_request(
            HttpMethod::GET,
            "https://eduzone.codemao.cn/api/home/account",
            Some(&params),
            None,
            None,
        )?;
        Ok(self.client.response_to_json(response)?)
    }

    pub fn fetch_unread_message_count(&self) -> Result<Value, Box<dyn std::error::Error>> {
        let mut params = HashMap::new();
        Self::add_timestamp_params(&mut params);
        let response = self.client.send_request(
            HttpMethod::GET,
            "https://eduzone.codemao.cn/edu/zone/system/message/unread/num",
            Some(&params),
            None,
            None,
        )?;
        Ok(self.client.response_to_json(response)?)
    }

    pub fn fetch_notices_gen(&self, limit: Option<usize>) -> PaginatedIter {
        let mut params = HashMap::new();
        Self::add_timestamp_params(&mut params);
        params.insert("page".to_string(), "1".to_string());
        params.insert("limit".to_string(), "10".to_string());

        let mut paginated = self
            .client
            .paginated("https://eduzone.codemao.cn/edu/zone/system/message/list")
            .with_params(params)
            .with_pagination_method(PaginationMethod::Page)
            .with_offset_key("page")
            .with_amount_key("limit");

        if let Some(limit_val) = limit {
            paginated = paginated.with_limit(limit_val);
        } else {
            paginated = paginated.with_limit(10);
        }
        paginated
    }

    pub fn fetch_reminders_gen(&self, limit: Option<usize>) -> PaginatedIter {
        let mut params = HashMap::new();
        Self::add_timestamp_params(&mut params);
        params.insert("page".to_string(), "1".to_string());
        params.insert("limit".to_string(), "10".to_string());

        let mut paginated = self
            .client
            .paginated("https://eduzone.codemao.cn/edu/zone/invite/teacher/messages")
            .with_params(params)
            .with_pagination_method(PaginationMethod::Page)
            .with_offset_key("page")
            .with_amount_key("limit");

        if let Some(limit_val) = limit {
            paginated = paginated.with_limit(limit_val);
        } else {
            paginated = paginated.with_limit(10);
        }
        paginated
    }

    pub fn fetch_school_categories(&self) -> Result<Value, Box<dyn std::error::Error>> {
        let mut params = HashMap::new();
        Self::add_timestamp_params(&mut params);
        let response = self.client.send_request(
            HttpMethod::GET,
            "https://eduzone.codemao.cn/edu/zone/school/open/grade/list",
            Some(&params),
            None,
            None,
        )?;
        Ok(self.client.response_to_json(response)?)
    }

    pub fn fetch_classrooms(
        &self,
        method: &str,
        limit: Option<usize>,
    ) -> Result<Value, Box<dyn std::error::Error>> {
        if method == "simple" {
            let response = self.client.send_request(
                HttpMethod::GET,
                "https://eduzone.codemao.cn/edu/zone/classes/simple",
                None,
                None,
                None,
            )?;
            Ok(self.client.response_to_json(response)?)
        } else if method == "detail" {
            let mut params = HashMap::new();
            Self::add_timestamp_params(&mut params);
            params.insert("page".to_string(), "1".to_string());

            let paginated = self
                .client
                .paginated("https://eduzone.codemao.cn/edu/zone/classes/")
                .with_params(params)
                .with_pagination_method(PaginationMethod::Page)
                .with_offset_key("page")
                .with_response_amount_key("limit")
                .with_limit(limit.unwrap_or(20));

            Ok(json!({ "paginated": "Use iterator to fetch data" }))
        } else {
            Ok(json!({}))
        }
    }

    pub fn fetch_student_removal_records_gen(&self, limit: Option<usize>) -> PaginatedIter {
        let mut params = HashMap::new();
        Self::add_timestamp_params(&mut params);
        params.insert("page".to_string(), "1".to_string());
        params.insert("limit".to_string(), "10".to_string());

        let mut paginated = self
            .client
            .paginated("https://eduzone.codemao.cn/edu/zone/student/remove/record")
            .with_params(params)
            .with_pagination_method(PaginationMethod::Page)
            .with_offset_key("page")
            .with_amount_key("limit");

        if let Some(limit_val) = limit {
            paginated = paginated.with_limit(limit_val);
        } else {
            paginated = paginated.with_limit(10);
        }
        paginated
    }

    pub fn fetch_class_students_gen(&self, invalid: i32, limit: Option<usize>) -> PaginatedIter {
        let mut params = HashMap::new();
        params.insert("page".to_string(), "1".to_string());
        params.insert("limit".to_string(), "100".to_string());

        let data = json!({ "invalid": invalid });

        let mut paginated = self
            .client
            .paginated("https://eduzone.codemao.cn/edu/zone/students")
            .with_params(params)
            .with_payload(data)
            .with_method(HttpMethod::POST)
            .with_pagination_method(PaginationMethod::Page)
            .with_offset_key("page")
            .with_amount_key("limit");

        if let Some(limit_val) = limit {
            paginated = paginated.with_limit(limit_val);
        } else {
            paginated = paginated.with_limit(100);
        }
        paginated
    }

    pub fn fetch_navigation_menus(&self) -> Result<Value, Box<dyn std::error::Error>> {
        let mut params = HashMap::new();
        Self::add_timestamp_params(&mut params);
        let response = self.client.send_request(
            HttpMethod::GET,
            "https://eduzone.codemao.cn/api/home/eduzone/menus",
            Some(&params),
            None,
            None,
        )?;
        Ok(self.client.response_to_json(response)?)
    }

    pub fn fetch_edu_banners(&self, type_id: i32) -> Result<Value, Box<dyn std::error::Error>> {
        let mut params = HashMap::new();
        Self::add_timestamp_params(&mut params);
        params.insert("type_id".to_string(), type_id.to_string());
        let response = self.client.send_request(
            HttpMethod::GET,
            "https://eduzone.codemao.cn/api/home/banners",
            Some(&params),
            None,
            None,
        )?;
        Ok(self.client.response_to_json(response)?)
    }

    pub fn fetch_server_time(&self) -> Result<Value, Box<dyn std::error::Error>> {
        let mut params = HashMap::new();
        Self::add_timestamp_params(&mut params);
        let response = self.client.send_request(
            HttpMethod::GET,
            "https://eduzone.codemao.cn/edu/base/server/time",
            Some(&params),
            None,
            None,
        )?;
        Ok(self.client.response_to_json(response)?)
    }

    pub fn fetch_lesson_package_status(&self) -> Result<Value, Box<dyn std::error::Error>> {
        let mut params = HashMap::new();
        Self::add_timestamp_params(&mut params);
        let response = self.client.send_request(
            HttpMethod::GET,
            "https://eduzone.codemao.cn/edu/zone/lessons/person/package/remind/status",
            Some(&params),
            None,
            None,
        )?;
        Ok(self.client.response_to_json(response)?)
    }

    pub fn fetch_configuration(&self, tag: &str) -> Result<Value, Box<dyn std::error::Error>> {
        let mut params = HashMap::new();
        Self::add_timestamp_params(&mut params);
        params.insert("tag".to_string(), tag.to_string());
        let response = self.client.send_request(
            HttpMethod::GET,
            "https://eduzone.codemao.cn/edu/base/general/conf",
            Some(&params),
            None,
            None,
        )?;
        Ok(self.client.response_to_json(response)?)
    }

    pub fn fetch_extended_profile(&self) -> Result<Value, Box<dyn std::error::Error>> {
        let mut params = HashMap::new();
        Self::add_timestamp_params(&mut params);
        let response = self.client.send_request(
            HttpMethod::GET,
            "https://eduzone.codemao.cn/edu/zone/user-extend/info",
            Some(&params),
            None,
            None,
        )?;
        Ok(self.client.response_to_json(response)?)
    }

    pub fn fetch_operation_logs(&self) -> Result<Value, Box<dyn std::error::Error>> {
        let mut params = HashMap::new();
        Self::add_timestamp_params(&mut params);
        let response = self.client.send_request(
            HttpMethod::GET,
            "https://eduzone.codemao.cn/edu/zone/operation/records",
            Some(&params),
            None,
            None,
        )?;
        Ok(self.client.response_to_json(response)?)
    }

    pub fn fetch_teaching_status(&self) -> Result<Value, Box<dyn std::error::Error>> {
        let mut params = HashMap::new();
        Self::add_timestamp_params(&mut params);
        let response = self.client.send_request(
            HttpMethod::GET,
            "https://eduzone.codemao.cn/edu/zone/teaching/class/remind",
            Some(&params),
            None,
            None,
        )?;
        Ok(self.client.response_to_json(response)?)
    }

    pub fn fetch_dashboard_stats(&self) -> Result<Value, Box<dyn std::error::Error>> {
        let mut params = HashMap::new();
        Self::add_timestamp_params(&mut params);
        let response = self.client.send_request(
            HttpMethod::GET,
            "https://eduzone.codemao.cn/edu/zone/homepage/statistic",
            Some(&params),
            None,
            None,
        )?;
        Ok(self.client.response_to_json(response)?)
    }

    pub fn fetch_tool_menu(&self) -> Result<Value, Box<dyn std::error::Error>> {
        let mut params = HashMap::new();
        Self::add_timestamp_params(&mut params);
        let response = self.client.send_request(
            HttpMethod::GET,
            "https://eduzone.codemao.cn/edu/zone/homepage/menus",
            Some(&params),
            None,
            None,
        )?;
        Ok(self.client.response_to_json(response)?)
    }

    pub fn fetch_all_works_gen(&self, limit: Option<usize>) -> PaginatedIter {
        let mut params = HashMap::new();
        Self::add_timestamp_params(&mut params);
        params.insert("page".to_string(), "1".to_string());

        let mut paginated = self
            .client
            .paginated("https://eduzone.codemao.cn/edu/zone/work/manager/student/works")
            .with_params(params)
            .with_pagination_method(PaginationMethod::Page)
            .with_offset_key("page")
            .with_response_amount_key("limit");

        if let Some(limit_val) = limit {
            paginated = paginated.with_limit(limit_val);
        } else {
            paginated = paginated.with_limit(50);
        }
        paginated
    }

    pub fn fetch_managed_works_gen(&self, limit: Option<usize>) -> PaginatedIter {
        let mut params = HashMap::new();
        Self::add_timestamp_params(&mut params);
        params.insert("page".to_string(), "1".to_string());

        let mut paginated = self
            .client
            .paginated("https://eduzone.codemao.cn/edu/zone/work/manager/works")
            .with_params(params)
            .with_pagination_method(PaginationMethod::Page)
            .with_offset_key("page")
            .with_response_amount_key("limit");

        if let Some(limit_val) = limit {
            paginated = paginated.with_limit(limit_val);
        } else {
            paginated = paginated.with_limit(50);
        }
        paginated
    }

    pub fn fetch_personal_works_gen(&self, limit: Option<usize>) -> PaginatedIter {
        let mut params = HashMap::new();
        Self::add_timestamp_params(&mut params);
        params.insert("page".to_string(), "1".to_string());

        let mut paginated = self
            .client
            .paginated("https://eduzone.codemao.cn/edu/zone/work/manager/self/works")
            .with_params(params)
            .with_pagination_method(PaginationMethod::Page)
            .with_offset_key("page")
            .with_response_amount_key("limit");

        if let Some(limit_val) = limit {
            paginated = paginated.with_limit(limit_val);
        } else {
            paginated = paginated.with_limit(50);
        }
        paginated
    }

    pub fn fetch_work_analytics(
        &self,
        class_id: Option<i32>,
        year: i32,
        month: i32,
    ) -> Result<Value, Box<dyn std::error::Error>> {
        let mut params = HashMap::new();
        Self::add_timestamp_params(&mut params);
        params.insert("year".to_string(), year.to_string());
        params.insert("month".to_string(), format!("{:02}", month));
        if let Some(cid) = class_id {
            params.insert("class_id".to_string(), cid.to_string());
        }
        let response = self.client.send_request(
            HttpMethod::GET,
            "https://eduzone.codemao.cn/edu/zone/work/manager/works/statistics",
            Some(&params),
            None,
            None,
        )?;
        Ok(self.client.response_to_json(response)?)
    }

    pub fn fetch_teaching_records_gen(&self, limit: Option<usize>) -> PaginatedIter {
        let mut params = HashMap::new();
        Self::add_timestamp_params(&mut params);
        params.insert("page".to_string(), "1".to_string());
        params.insert("limit".to_string(), "10".to_string());

        let mut paginated = self
            .client
            .paginated("https://eduzone.codemao.cn/edu/zone/teaching/record/list")
            .with_params(params)
            .with_pagination_method(PaginationMethod::Page)
            .with_offset_key("page")
            .with_amount_key("limit");

        if let Some(limit_val) = limit {
            paginated = paginated.with_limit(limit_val);
        } else {
            paginated = paginated.with_limit(10);
        }
        paginated
    }

    pub fn fetch_teaching_classes(&self) -> Result<Value, Box<dyn std::error::Error>> {
        let mut params = HashMap::new();
        Self::add_timestamp_params(&mut params);
        let response = self.client.send_request(
            HttpMethod::GET,
            "https://eduzone.codemao.cn/edu/zone/teaching/class/teacher/list",
            Some(&params),
            None,
            None,
        )?;
        Ok(self.client.response_to_json(response)?)
    }

    pub fn fetch_school_info(&self, unit_id: i32) -> Result<Value, Box<dyn std::error::Error>> {
        let mut params = HashMap::new();
        Self::add_timestamp_params(&mut params);
        params.insert("unitId".to_string(), unit_id.to_string());
        let response = self.client.send_request(
            HttpMethod::GET,
            "https://eduzone.codemao.cn/edu/zone/school/info",
            Some(&params),
            None,
            None,
        )?;
        Ok(self.client.response_to_json(response)?)
    }

    pub fn fetch_official_lesson_packages_gen(&self, limit: Option<usize>) -> PaginatedIter {
        let mut params = HashMap::new();
        Self::add_timestamp_params(&mut params);
        params.insert("pacakgeEntryType".to_string(), "0".to_string());
        params.insert("topicType".to_string(), "all".to_string());
        params.insert("topicId".to_string(), "all".to_string());
        params.insert("tagId".to_string(), "all".to_string());
        params.insert("page".to_string(), "1".to_string());
        params.insert("limit".to_string(), "150".to_string());

        let mut paginated = self
            .client
            .paginated("https://eduzone.codemao.cn/edu/zone/lesson/offical/packages")
            .with_params(params)
            .with_pagination_method(PaginationMethod::Page)
            .with_offset_key("page")
            .with_amount_key("limit");

        if let Some(limit_val) = limit {
            paginated = paginated.with_limit(limit_val);
        } else {
            paginated = paginated.with_limit(150);
        }
        paginated
    }

    pub fn fetch_lesson_topics(&self) -> Result<Value, Box<dyn std::error::Error>> {
        let mut params = HashMap::new();
        Self::add_timestamp_params(&mut params);
        params.insert("pacakgeEntryType".to_string(), "0".to_string());
        params.insert("topicType".to_string(), "all".to_string());
        let response = self.client.send_request(
            HttpMethod::GET,
            "https://eduzone.codemao.cn/edu/zone/lessons/official/packages/topics",
            Some(&params),
            None,
            None,
        )?;
        Ok(self.client.response_to_json(response)?)
    }

    pub fn fetch_lesson_tags(&self) -> Result<Value, Box<dyn std::error::Error>> {
        let mut params = HashMap::new();
        Self::add_timestamp_params(&mut params);
        params.insert("pacakgeEntryType".to_string(), "0".to_string());
        params.insert("topicType".to_string(), "all".to_string());
        let response = self.client.send_request(
            HttpMethod::GET,
            "https://eduzone.codemao.cn/edu/zone/lessons/official/packages/topics/all/tags",
            Some(&params),
            None,
            None,
        )?;
        Ok(self.client.response_to_json(response)?)
    }

    pub fn fetch_custom_lesson_packages_gen(&self, limit: Option<usize>) -> PaginatedIter {
        let mut params = HashMap::new();
        Self::add_timestamp_params(&mut params);
        params.insert("page".to_string(), "1".to_string());
        params.insert("limit".to_string(), "100".to_string());

        let mut paginated = self
            .client
            .paginated("https://eduzone.codemao.cn/edu/zone/lesson/offical/packages")
            .with_params(params)
            .with_pagination_method(PaginationMethod::Page)
            .with_offset_key("page")
            .with_amount_key("limit");

        if let Some(limit_val) = limit {
            paginated = paginated.with_limit(limit_val);
        } else {
            paginated = paginated.with_limit(100);
        }
        paginated
    }

    pub fn get_or_delete_custom_package(
        &self,
        package_id: i32,
        method: HttpMethod,
    ) -> Result<Value, Box<dyn std::error::Error>> {
        let mut params = HashMap::new();
        Self::add_timestamp_params(&mut params);
        let endpoint = format!(
            "https://eduzone.codemao.cn/edu/zone/lesson/customized/packages/{}",
            package_id
        );
        let response = self
            .client
            .send_request(method, &endpoint, Some(&params), None, None)?;

        if method == HttpMethod::GET {
            Ok(self.client.response_to_json(response)?)
        } else {
            Ok(json!({ "success": response.status() == HTTPStatus::Ok as u16 }))
        }
    }

    pub fn fetch_custom_package_contents(
        &self,
        package_id: i32,
        limit: i32,
    ) -> Result<Value, Box<dyn std::error::Error>> {
        let mut params = HashMap::new();
        Self::add_timestamp_params(&mut params);
        params.insert("limit".to_string(), limit.to_string());
        params.insert("package_id".to_string(), package_id.to_string());
        let response = self.client.send_request(
            HttpMethod::GET,
            "https://eduzone.codemao.cn/edu/zone/lesson/customized/package/lessons",
            Some(&params),
            None,
            None,
        )?;
        Ok(self.client.response_to_json(response)?)
    }

    pub fn fetch_class_invites(&self) -> Result<Value, Box<dyn std::error::Error>> {
        let mut params = HashMap::new();
        Self::add_timestamp_params(&mut params);
        let response = self.client.send_request(
            HttpMethod::GET,
            "https://eduzone.codemao.cn/edu/zone/invite/student/message/next",
            Some(&params),
            None,
            None,
        )?;
        Ok(self.client.response_to_json(response)?)
    }

    pub fn fetch_expiring_lessons(&self) -> Result<Value, Box<dyn std::error::Error>> {
        let mut params = HashMap::new();
        Self::add_timestamp_params(&mut params);
        let response = self.client.send_request(
            HttpMethod::GET,
            "https://eduzone.codemao.cn/edu/zone/lesson/offical/packages/expired",
            Some(&params),
            None,
            None,
        )?;
        Ok(self.client.response_to_json(response)?)
    }

    pub fn fetch_organization_ids(&self) -> Result<Value, Box<dyn std::error::Error>> {
        let mut params = HashMap::new();
        let timestamp = current_timestamp_13();
        params.insert("CMTIME".to_string(), timestamp.to_string());
        let response = self.client.send_request(
            HttpMethod::GET,
            "https://static.codemao.cn/teacher-edu/organization_ids.json",
            Some(&params),
            None,
            None,
        )?;
        Ok(self.client.response_to_json(response)?)
    }

    pub fn fetch_report_metadata(&self) -> Result<Value, Box<dyn std::error::Error>> {
        let mut params = HashMap::new();
        Self::add_timestamp_params(&mut params);
        let response = self.client.send_request(
            HttpMethod::GET,
            "https://eduzone.codemao.cn/edu/zone/analysis/report/info",
            Some(&params),
            None,
            None,
        )?;
        Ok(self.client.response_to_json(response)?)
    }

    pub fn fetch_course_analytics(&self) -> Result<Value, Box<dyn std::error::Error>> {
        let mut params = HashMap::new();
        Self::add_timestamp_params(&mut params);
        let response = self.client.send_request(
            HttpMethod::GET,
            "https://eduzone.codemao.cn/edu/zone/analysis/student/course",
            Some(&params),
            None,
            None,
        )?;
        Ok(self.client.response_to_json(response)?)
    }

    pub fn fetch_lesson_package_analytics(&self) -> Result<Value, Box<dyn std::error::Error>> {
        let mut params = HashMap::new();
        Self::add_timestamp_params(&mut params);
        let response = self.client.send_request(
            HttpMethod::GET,
            "https://eduzone.codemao.cn/edu/zone/analysis/student/packages",
            Some(&params),
            None,
            None,
        )?;
        Ok(self.client.response_to_json(response)?)
    }

    pub fn fetch_classroom_analytics(&self) -> Result<Value, Box<dyn std::error::Error>> {
        let mut params = HashMap::new();
        Self::add_timestamp_params(&mut params);
        let response = self.client.send_request(
            HttpMethod::GET,
            "https://eduzone.codemao.cn/edu/zone/analysis/student/class/info",
            Some(&params),
            None,
            None,
        )?;
        Ok(self.client.response_to_json(response)?)
    }

    pub fn fetch_work_performance(&self) -> Result<Value, Box<dyn std::error::Error>> {
        let mut params = HashMap::new();
        Self::add_timestamp_params(&mut params);
        let response = self.client.send_request(
            HttpMethod::GET,
            "https://eduzone.codemao.cn/edu/zone/analysis/student/works/situations",
            Some(&params),
            None,
            None,
        )?;
        Ok(self.client.response_to_json(response)?)
    }

    pub fn fetch_work_ratings(&self) -> Result<Value, Box<dyn std::error::Error>> {
        let mut params = HashMap::new();
        Self::add_timestamp_params(&mut params);
        let response = self.client.send_request(
            HttpMethod::GET,
            "https://eduzone.codemao.cn/edu/zone/analysis/student/works/star/info",
            Some(&params),
            None,
            None,
        )?;
        Ok(self.client.response_to_json(response)?)
    }

    pub fn fetch_skill_assessment(&self) -> Result<Value, Box<dyn std::error::Error>> {
        let mut params = HashMap::new();
        Self::add_timestamp_params(&mut params);
        let response = self.client.send_request(
            HttpMethod::GET,
            "https://eduzone.codemao.cn/edu/zone/analysis/student/ability/dimensions",
            Some(&params),
            None,
            None,
        )?;
        Ok(self.client.response_to_json(response)?)
    }

    pub fn fetch_skill_radar(&self) -> Result<Value, Box<dyn std::error::Error>> {
        let mut params = HashMap::new();
        Self::add_timestamp_params(&mut params);
        let response = self.client.send_request(
            HttpMethod::GET,
            "https://eduzone.codemao.cn/edu/zone/analysis/student/ability/radars",
            Some(&params),
            None,
            None,
        )?;
        Ok(self.client.response_to_json(response)?)
    }

    pub fn fetch_art_skills(&self) -> Result<Value, Box<dyn std::error::Error>> {
        let mut params = HashMap::new();
        Self::add_timestamp_params(&mut params);
        let response = self.client.send_request(
            HttpMethod::GET,
            "https://eduzone.codemao.cn/edu/zone/analysis/student/ability/artistic/dimensions",
            Some(&params),
            None,
            None,
        )?;
        Ok(self.client.response_to_json(response)?)
    }

    pub fn fetch_logic_skills(&self) -> Result<Value, Box<dyn std::error::Error>> {
        let mut params = HashMap::new();
        Self::add_timestamp_params(&mut params);
        let response = self.client.send_request(
            HttpMethod::GET,
            "https://eduzone.codemao.cn/edu/zone/analysis/student/ability/logical/dimensions",
            Some(&params),
            None,
            None,
        )?;
        Ok(self.client.response_to_json(response)?)
    }

    pub fn fetch_coding_skills(&self) -> Result<Value, Box<dyn std::error::Error>> {
        let mut params = HashMap::new();
        Self::add_timestamp_params(&mut params);
        let response = self.client.send_request(
            HttpMethod::GET,
            "https://eduzone.codemao.cn/edu/zone/analysis/student/ability/programming/dimensions",
            Some(&params),
            None,
            None,
        )?;
        Ok(self.client.response_to_json(response)?)
    }
}

impl Default for DataFetcher {
    fn default() -> Self {
        Self::new()
    }
}
