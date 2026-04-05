use rmcp::schemars::{self, JsonSchema};
use serde::{Deserialize, Serialize};

use crate::ingest::scanner::{CodeLanguage, FileType};

#[derive(Deserialize, JsonSchema)]
pub struct ContextSearchInput {
    #[schemars(description = "Search query text")]
    pub query: String,
    #[schemars(description = "Max results to return (default 5, max 50)")]
    pub limit: Option<usize>,
    #[schemars(description = "Filter by document source label")]
    pub source: Option<String>,
}

#[derive(Deserialize, JsonSchema)]
pub struct GetDocumentInput {
    #[schemars(description = "Document record ID (without 'document:' prefix)")]
    pub id: String,
}

#[derive(Deserialize, JsonSchema)]
pub struct ListDocumentsInput {
    #[schemars(description = "Filter by document source label")]
    pub source: Option<String>,
    #[schemars(description = "Max documents to return (default 20, max 200)")]
    pub limit: Option<usize>,
    #[schemars(description = "Offset for pagination")]
    pub offset: Option<usize>,
    #[schemars(description = "Filter documents by custom_attributes using a JSON DSL.\n\n\
OPERATORS (applied to a field value):\n\
- Exact match: {\"field\": \"value\"} or {\"field\": {\"$eq\": \"value\"}}\n\
- Comparison: $ne (!=), $gt (>), $gte (>=), $lt (<), $lte (<=)\n\
- Set membership: $in — scalar field is one of values: {\"status\": {\"$in\": [\"active\", \"pending\"]}}\n\
- Array contains scalar: $contains — {\"tags\": {\"$contains\": \"api\"}}\n\
- Array intersection (ANY match): $any — field array shares at least one element with filter array: {\"tags\": {\"$any\": [\"api\", \"graphql\"]}}\n\
- Array superset (ALL match): $all — field array contains all filter values: {\"tags\": {\"$all\": [\"api\", \"rest\"]}}\n\
- Null check: {\"field\": null} matches documents where field is not set\n\n\
LOGICAL OPERATORS:\n\
- $and: all conditions must match: {\"$and\": [{\"a\": 1}, {\"b\": 2}]}\n\
- $or: at least one must match: {\"$or\": [{\"category\": \"docs\"}, {\"category\": \"logs\"}]}\n\
- Top-level object keys are implicitly ANDed: {\"a\": 1, \"b\": 2} means a=1 AND b=2\n\
- $and/$or can be nested: {\"$or\": [{\"$and\": [{\"a\": 1}, {\"b\": 2}]}, {\"c\": 3}]}\n\n\
NESTED PATHS (dot-path via nested objects):\n\
- {\"config\": {\"env\": \"prod\"}} filters on custom_attributes.config.env = \"prod\"\n\
- {\"config\": {\"env\": {\"$in\": [\"prod\", \"staging\"]}}} uses operator on nested path\n\n\
EXAMPLES:\n\
- Simple: {\"category\": \"docs\"}\n\
- Multiple conditions: {\"category\": \"docs\", \"version\": {\"$gte\": 2}}\n\
- OR: {\"$or\": [{\"priority\": \"high\"}, {\"priority\": \"critical\"}]}\n\
- Array intersection: {\"tags\": {\"$any\": [\"api\", \"graphql\"]}}\n\
- Nested path + operator: {\"metadata\": {\"source\": {\"$ne\": \"deprecated\"}}}")]
    pub filters: Option<serde_json::Value>,
}

#[derive(Serialize)]
pub struct StatsOutput {
    pub documents: usize,
    pub chunks: usize,
    pub embedded_chunks: usize,
    pub pending_chunks: usize,
    pub documents_by_source: Vec<crate::db::documents::SourceCount>,
    pub embedding_model: String,
    pub embedding_dimension: usize,
}

#[derive(Deserialize, JsonSchema)]
pub struct CreateDocumentInput {
    #[schemars(description = "Document title")]
    pub title: String,
    #[schemars(description = "Document content (text, markdown, or code)")]
    pub content: String,
    #[schemars(description = "Source label for grouping (default: 'mcp')")]
    pub source: Option<String>,
    #[schemars(description = "Unique identifier within the source (auto-generated if omitted)")]
    pub source_id: Option<String>,
    #[schemars(description = "Content type hint: 'markdown', 'plain_text', 'rust', 'typescript', 'python', 'go', 'ruby', 'java', 'javascript', 'c', 'cpp', 'csharp'. Auto-detected if omitted.")]
    pub content_type: Option<String>,
    #[schemars(description = "Parent document ID for hierarchy (without 'document:' prefix)")]
    pub parent_id: Option<String>,
    #[schemars(description = "Custom attributes as a JSON object")]
    pub custom_attributes: Option<serde_json::Value>,
}

#[derive(Deserialize, JsonSchema)]
pub struct UpdateDocumentInput {
    #[schemars(description = "Document record ID (without 'document:' prefix)")]
    pub id: String,
    #[schemars(description = "New title (unchanged if omitted)")]
    pub title: Option<String>,
    #[schemars(description = "New content (triggers re-chunking and re-embedding if changed)")]
    pub content: Option<String>,
    #[schemars(description = "Content type hint for chunking (same options as create_document)")]
    pub content_type: Option<String>,
    #[schemars(description = "Custom attributes as a JSON object (replaces existing)")]
    pub custom_attributes: Option<serde_json::Value>,
}

#[derive(Serialize)]
pub struct MutationResult {
    pub id: String,
    pub title: String,
    pub source: String,
    pub chunks_created: usize,
    pub chunks_embedded: usize,
}

#[derive(Deserialize, JsonSchema)]
pub struct SetDocumentParentInput {
    #[schemars(description = "Child document ID (without 'document:' prefix)")]
    pub child_id: String,
    #[schemars(description = "Parent document ID (without 'document:' prefix). Omit to make root.")]
    pub parent_id: Option<String>,
}

#[derive(Deserialize, JsonSchema)]
pub struct DocumentIdInput {
    #[schemars(description = "Document record ID (without 'document:' prefix)")]
    pub id: String,
}

pub fn parse_content_type(hint: Option<&str>, content: &str) -> FileType {
    match hint {
        Some("markdown" | "md") => FileType::Markdown,
        Some("plain_text" | "text") => FileType::PlainText,
        Some("rust") => FileType::Code(CodeLanguage::Rust),
        Some("typescript" | "ts") => FileType::Code(CodeLanguage::TypeScript),
        Some("tsx") => FileType::Code(CodeLanguage::TypeScriptReact),
        Some("python" | "py") => FileType::Code(CodeLanguage::Python),
        Some("go") => FileType::Code(CodeLanguage::Go),
        Some("javascript" | "js") => FileType::Code(CodeLanguage::JavaScript),
        Some("jsx") => FileType::Code(CodeLanguage::JavaScriptReact),
        Some("ruby" | "rb") => FileType::Code(CodeLanguage::Ruby),
        Some("java") => FileType::Code(CodeLanguage::Java),
        Some("c") => FileType::Code(CodeLanguage::C),
        Some("cpp" | "c++") => FileType::Code(CodeLanguage::Cpp),
        Some("csharp" | "cs") => FileType::Code(CodeLanguage::CSharp),
        _ => auto_detect_content_type(content),
    }
}

fn auto_detect_content_type(content: &str) -> FileType {
    let trimmed = content.trim_start();
    if trimmed.starts_with('#') || content.contains("\n## ") || content.contains("\n```") {
        FileType::Markdown
    } else {
        FileType::PlainText
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_explicit_markdown() {
        assert!(matches!(parse_content_type(Some("markdown"), ""), FileType::Markdown));
        assert!(matches!(parse_content_type(Some("md"), ""), FileType::Markdown));
    }

    #[test]
    fn parse_explicit_code() {
        assert!(matches!(parse_content_type(Some("rust"), ""), FileType::Code(CodeLanguage::Rust)));
        assert!(matches!(parse_content_type(Some("python"), ""), FileType::Code(CodeLanguage::Python)));
    }

    #[test]
    fn auto_detect_markdown_heading() {
        assert!(matches!(parse_content_type(None, "# Title\nContent"), FileType::Markdown));
    }

    #[test]
    fn auto_detect_markdown_code_block() {
        assert!(matches!(parse_content_type(None, "text\n```rust\ncode\n```"), FileType::Markdown));
    }

    #[test]
    fn auto_detect_plain_text() {
        assert!(matches!(parse_content_type(None, "Just some plain text."), FileType::PlainText));
    }
}
