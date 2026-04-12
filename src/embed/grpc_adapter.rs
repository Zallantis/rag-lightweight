use async_trait::async_trait;
use tonic::transport::{Certificate, Channel, ClientTlsConfig};

use crate::embed::proto::wzd::{
    self as pb, embed_inference_service_client::EmbedInferenceServiceClient,
};
use crate::embed::service::{EmbedRole, EmbeddingService};

pub struct GrpcEmbeddingService {
    channel: Channel,
    model_id: String,
    auth_token: Option<String>,
}

impl GrpcEmbeddingService {
    pub fn new(
        url: &str,
        model_id: String,
        auth_token: Option<String>,
        ca_cert_path: Option<&str>,
    ) -> crate::error::Result<Self> {
        let mut endpoint = Channel::from_shared(url.to_string())
            .map_err(|e| crate::error::AppError::Grpc(format!("invalid endpoint URL: {e}")))?
            .connect_timeout(std::time::Duration::from_secs(10))
            .timeout(std::time::Duration::from_secs(120));

        if let Some(path) = ca_cert_path {
            let pem = std::fs::read(path).map_err(|e| {
                crate::error::AppError::Grpc(format!("failed to read CA cert {path}: {e}"))
            })?;
            let tls = ClientTlsConfig::new()
                .ca_certificate(Certificate::from_pem(pem))
                .with_enabled_roots();
            endpoint = endpoint
                .tls_config(tls)
                .map_err(|e| crate::error::AppError::Grpc(format!("TLS config error: {e}")))?;
        }

        let channel = endpoint.connect_lazy();

        Ok(Self {
            channel,
            model_id,
            auth_token,
        })
    }

    fn make_request<T>(&self, inner: T) -> tonic::Request<T> {
        let mut req = tonic::Request::new(inner);
        if let Some(ref token) = self.auth_token
            && let Ok(val) = format!("Bearer {token}").parse()
        {
            req.metadata_mut().insert("authorization", val);
        }
        req
    }
}

#[async_trait]
impl EmbeddingService for GrpcEmbeddingService {
    async fn embed(&self, texts: Vec<String>) -> crate::error::Result<Vec<Vec<f32>>> {
        self.embed_with_role(texts, EmbedRole::Passage).await
    }

    async fn embed_with_role(
        &self,
        texts: Vec<String>,
        role: EmbedRole,
    ) -> crate::error::Result<Vec<Vec<f32>>> {
        let proto_role = match role {
            EmbedRole::Passage => pb::EmbedRole::Passage as i32,
            EmbedRole::Query => pb::EmbedRole::Query as i32,
        };

        let request = pb::EmbedRequest {
            model_id: self.model_id.clone(),
            texts,
            role: proto_role,
        };

        let mut client = EmbedInferenceServiceClient::new(self.channel.clone());
        let response = client
            .embed(self.make_request(request))
            .await
            .map_err(|s| crate::error::AppError::Grpc(format!("embed RPC failed: {s}")))?
            .into_inner();

        let vectors: Vec<Vec<f32>> = response
            .embeddings
            .into_iter()
            .map(|fv| fv.values)
            .collect();

        Ok(vectors)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn constructor_accepts_valid_url() {
        let svc =
            GrpcEmbeddingService::new("http://localhost:50060", "test-model".into(), None, None);
        assert!(svc.is_ok());
    }

    #[test]
    fn constructor_rejects_invalid_url() {
        let svc = GrpcEmbeddingService::new("not a url at all \0", "model".into(), None, None);
        assert!(svc.is_err());
    }

    #[tokio::test]
    async fn make_request_injects_auth_token() {
        let svc = GrpcEmbeddingService::new(
            "http://localhost:50060",
            "m".into(),
            Some("my-secret".into()),
            None,
        )
        .unwrap();
        let req = svc.make_request(());
        let val = req.metadata().get("authorization").unwrap();
        assert_eq!(val, "Bearer my-secret");
    }

    #[tokio::test]
    async fn make_request_skips_auth_when_none() {
        let svc =
            GrpcEmbeddingService::new("http://localhost:50060", "m".into(), None, None).unwrap();
        let req = svc.make_request(());
        assert!(req.metadata().get("authorization").is_none());
    }

    #[test]
    fn constructor_rejects_missing_ca_cert() {
        let result = GrpcEmbeddingService::new(
            "https://localhost:50060",
            "m".into(),
            None,
            Some("/nonexistent/ca.pem"),
        );
        assert!(result.is_err());
        let err = format!("{}", result.err().unwrap());
        assert!(err.contains("CA cert"));
    }
}
