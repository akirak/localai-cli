use anyhow::{Context, Result};
use reqwest::Method;
use serde_json::Value;

pub struct LocalAIClient {
    client: reqwest::Client,
    base_url: String,
    api_key: Option<String>,
}

impl LocalAIClient {
    pub fn new(base_url: String, api_key: Option<String>) -> Self {
        let base_url = base_url.trim_end_matches('/').to_string();
        Self {
            client: reqwest::Client::new(),
            base_url,
            api_key,
        }
    }

    pub async fn request_json(
        &self,
        method: Method,
        path: &str,
        query: Option<Vec<(String, String)>>,
        body: Option<Value>,
    ) -> Result<Value> {
        let url = format!("{}{}", self.base_url, path);
        let mut req = self.client.request(method, &url);

        if let Some(key) = &self.api_key {
            req = req.header("Authorization", format!("Bearer {key}"));
        }

        if let Some(q) = query {
            req = req.query(&q);
        }

        if let Some(b) = body {
            req = req.json(&b);
        }

        let resp = req.send().await?;
        let status = resp.status();

        if status.is_success() {
            let text = resp.text().await?;
            if text.trim().is_empty() {
                Ok(Value::Object(serde_json::Map::new()))
            } else {
                serde_json::from_str(&text)
                    .with_context(|| format!("invalid JSON response: {text}"))
            }
        } else {
            let text = resp.text().await.unwrap_or_default();
            Err(anyhow::anyhow!("HTTP {status}: {text}"))
        }
    }

    pub async fn request_multipart(
        &self,
        path: &str,
        form: reqwest::multipart::Form,
    ) -> Result<Value> {
        let url = format!("{}{}", self.base_url, path);
        let mut req = self.client.post(&url).multipart(form);

        if let Some(key) = &self.api_key {
            req = req.header("Authorization", format!("Bearer {key}"));
        }

        let resp = req.send().await?;
        let status = resp.status();

        if status.is_success() {
            let text = resp.text().await?;
            if text.trim().is_empty() {
                Ok(Value::Object(serde_json::Map::new()))
            } else {
                serde_json::from_str(&text)
                    .with_context(|| format!("invalid JSON response: {text}"))
            }
        } else {
            let text = resp.text().await.unwrap_or_default();
            Err(anyhow::anyhow!("HTTP {status}: {text}"))
        }
    }
}
