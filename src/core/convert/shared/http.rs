use super::error::Result;
use crate::utils::requests::{CodeMaoClient, HttpMethod};
use serde_json::Value;
use std::sync::Arc;

// HTTP 客户端
pub(crate) trait HttpClient: Send + Sync {
    fn get_json(&self, url: &str, headers: Option<Vec<(String, String)>>) -> Result<Value>;
    fn get_binary(&self, url: &str) -> Result<Vec<u8>>;
    fn get_text(&self, url: &str) -> Result<String>;
    fn box_clone(&self) -> Box<dyn HttpClient>;
}

impl Clone for Box<dyn HttpClient> {
    fn clone(&self) -> Self {
        self.box_clone()
    }
}

#[derive(Clone)]
pub(crate) struct CodeMaoHttpClient {
    client: Arc<CodeMaoClient>,
}

impl CodeMaoHttpClient {
    pub(crate) fn new(client: Arc<CodeMaoClient>) -> Self {
        Self { client }
    }
}

impl HttpClient for CodeMaoHttpClient {
    fn get_json(&self, url: &str, headers: Option<Vec<(String, String)>>) -> Result<Value> {
        let mut request_builder = self.client.build_request(HttpMethod::Get, url, None);
        if let Some(headers_map) = headers {
            request_builder = request_builder.with_headers(headers_map);
        }
        let response = request_builder.send()?;
        Ok(self.client.response_to_json(response)?)
    }

    fn get_binary(&self, url: &str) -> Result<Vec<u8>> {
        let response = self
            .client
            .build_request(HttpMethod::Get, url, None)
            .send()?;
        Ok(self.client.response_to_binary(response)?)
    }

    fn get_text(&self, url: &str) -> Result<String> {
        let response = self
            .client
            .build_request(HttpMethod::Get, url, None)
            .send()?;
        Ok(self.client.response_to_string(response)?)
    }

    fn box_clone(&self) -> Box<dyn HttpClient> {
        Box::new(self.clone())
    }
}
