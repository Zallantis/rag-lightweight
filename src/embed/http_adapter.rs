use crate::config::EmbeddingConfig;
use crate::embed::service::EmbeddingService;
use async_trait::async_trait;
use reqwest::Client;
use serde::{Deserialize, Serialize};

pub struct HttpEmbeddingService {
    client: Client,
    config: EmbeddingConfig,
}

impl HttpEmbeddingService {
    pub fn new(config: EmbeddingConfig) -> Self {
        let client = Client::builder()
            .timeout(std::time::Duration::from_secs(30))
            .connect_timeout(std::time::Duration::from_secs(10))
            .build()
            .expect("failed to build HTTP client");
        Self { client, config }
    }
}

#[async_trait]
impl EmbeddingService for HttpEmbeddingService {
    async fn embed(&self, texts: Vec<String>) -> crate::error::Result<Vec<Vec<f32>>> {
        let request = EmbeddingRequest {
            model: self.config.model.clone(),
            input: texts,
        };

        let mut req = self.client.post(&self.config.api_url).json(&request);

        if let Some(ref api_key) = self.config.api_key {
            req = req.bearer_auth(api_key);
        }

        let response = req.send().await?;

        if !response.status().is_success() {
            let status = response.status();
            let body = response.text().await.unwrap_or_default();
            tracing::debug!("Embedding API error body: {body}");
            let truncated: String = body.chars().take(256).collect();
            return Err(crate::error::AppError::Embedding(format!(
                "API returned {status}: {truncated}"
            )));
        }

        let result: EmbeddingResponse = response.json().await?;
        let vectors: Vec<Vec<f32>> = result.data.into_iter().map(|d| d.embedding).collect();

        Ok(vectors)
    }
}

#[derive(Serialize)]
struct EmbeddingRequest {
    model: String,
    input: Vec<String>,
}

#[derive(Deserialize)]
struct EmbeddingResponse {
    data: Vec<EmbeddingData>,
}

#[derive(Deserialize)]
struct EmbeddingData {
    embedding: Vec<f32>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::EmbeddingConfig;
    use crate::embed::service::EmbeddingService;
    use wiremock::matchers::method;
    use wiremock::{Mock, MockServer, ResponseTemplate};

    fn make_service(url: &str) -> HttpEmbeddingService {
        HttpEmbeddingService::new(EmbeddingConfig {
            provider: crate::config::EmbeddingProvider::Http,
            api_url: url.to_string(),
            api_key: None,
            model: "test-model".to_string(),
            dimension: 3,
            grpc_url: None,
            grpc_auth_token: None,
            grpc_ca_cert_path: None,
        })
    }

    #[tokio::test]
    async fn constructor_creates_successfully() {
        let service = make_service("http://localhost:9999");
        let _ = service;
    }

    #[tokio::test]
    async fn embed_returns_vectors_on_success() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "data": [
                    {"embedding": [0.1, 0.2, 0.3]},
                    {"embedding": [0.4, 0.5, 0.6]}
                ]
            })))
            .mount(&server)
            .await;

        let service = make_service(&server.uri());
        let result = service
            .embed(vec!["hello".to_string(), "world".to_string()])
            .await
            .unwrap();
        assert_eq!(result.len(), 2);
        assert_eq!(result[0], vec![0.1f32, 0.2, 0.3]);
        assert_eq!(result[1], vec![0.4f32, 0.5, 0.6]);
    }

    #[tokio::test]
    async fn embed_returns_error_on_http_401() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .respond_with(ResponseTemplate::new(401).set_body_string("Unauthorized"))
            .mount(&server)
            .await;

        let service = make_service(&server.uri());
        let result = service.embed(vec!["hello".to_string()]).await;
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("401"));
    }

    #[tokio::test]
    async fn embed_returns_error_on_http_500() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .respond_with(ResponseTemplate::new(500).set_body_string("Internal Server Error"))
            .mount(&server)
            .await;

        let service = make_service(&server.uri());
        let result = service.embed(vec!["test".to_string()]).await;
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("500"));
    }

    #[tokio::test]
    async fn embed_returns_empty_vec_on_empty_data_response() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "data": []
            })))
            .mount(&server)
            .await;

        let service = make_service(&server.uri());
        let result = service.embed(vec!["hello".to_string()]).await.unwrap();
        assert!(
            result.is_empty(),
            "currently returns empty vec — no count mismatch check"
        );
    }

    #[tokio::test]
    async fn embed_sends_bearer_auth_when_api_key_set() {
        use wiremock::matchers::header;

        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(header("authorization", "Bearer sk-test-key"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "data": [{"embedding": [1.0, 2.0, 3.0]}]
            })))
            .mount(&server)
            .await;

        let service = HttpEmbeddingService::new(EmbeddingConfig {
            provider: crate::config::EmbeddingProvider::Http,
            api_url: server.uri(),
            api_key: Some("sk-test-key".to_string()),
            model: "test-model".to_string(),
            dimension: 3,
            grpc_url: None,
            grpc_auth_token: None,
            grpc_ca_cert_path: None,
        });
        let result = service.embed(vec!["test".to_string()]).await.unwrap();
        assert_eq!(result.len(), 1);
        assert_eq!(result[0], vec![1.0f32, 2.0, 3.0]);
    }
}
