use async_trait::async_trait;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EmbedRole {
    Passage,
    Query,
}

#[async_trait]
pub trait EmbeddingService: Send + Sync {
    async fn embed(&self, texts: Vec<String>) -> crate::error::Result<Vec<Vec<f32>>>;

    async fn embed_with_role(
        &self,
        texts: Vec<String>,
        _role: EmbedRole,
    ) -> crate::error::Result<Vec<Vec<f32>>> {
        self.embed(texts).await
    }
}
