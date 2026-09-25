use serde_json::Value;

// Value 扩展
pub(crate) trait ValueExt {
    fn get_i64_or_default(&self, key: &str, default: i64) -> i64;
    fn get_str_or<'a>(&'a self, key: &str, default: &'a str) -> &'a str;
    fn get_string_or(&self, key: &str, default: &str) -> String;
    fn get_array_opt(&self, key: &str) -> Option<&Vec<Value>>;
    fn get_object_opt(&self, key: &str) -> Option<&serde_json::Map<String, Value>>;
}

impl ValueExt for Value {
    fn get_i64_or_default(&self, key: &str, default: i64) -> i64 {
        self.get(key)
            .and_then(serde_json::Value::as_i64)
            .unwrap_or(default)
    }

    fn get_str_or<'a>(&'a self, key: &str, default: &'a str) -> &'a str {
        self.get(key).and_then(|v| v.as_str()).unwrap_or(default)
    }

    fn get_string_or(&self, key: &str, default: &str) -> String {
        self.get_str_or(key, default).to_string()
    }

    fn get_array_opt(&self, key: &str) -> Option<&Vec<Value>> {
        self.get(key).and_then(|v| v.as_array())
    }

    fn get_object_opt(&self, key: &str) -> Option<&serde_json::Map<String, Value>> {
        self.get(key).and_then(|v| v.as_object())
    }
}
