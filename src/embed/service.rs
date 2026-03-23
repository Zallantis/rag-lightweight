use async_trait::async_trait;

#[async_trait]
pub trait EmbeddingService: Send + Sync {
    async fn embed(&self, texts: Vec<String>) -> crate::error::Result<Vec<Vec<f32>>>;
}
