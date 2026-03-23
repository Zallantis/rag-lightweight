use crate::ingest::scanner::{CodeLanguage, FileType};
use sha2::{Digest, Sha256};
use std::sync::OnceLock;
use tiktoken_rs::CoreBPE;

fn get_tokenizer() -> &'static CoreBPE {
    static TOKENIZER: OnceLock<CoreBPE> = OnceLock::new();
    TOKENIZER.get_or_init(|| tiktoken_rs::cl100k_base().expect("cl100k_base tokenizer must be available"))
}

pub struct ChunkResult {
    pub content: String,
    pub token_count: usize,
    pub content_hash: String,
}

pub fn chunk_content(
    content: &str,
    file_type: &FileType,
    max_tokens: usize,
    parser: &mut tree_sitter::Parser,
) -> Vec<ChunkResult> {
    let chunks = match file_type {
        FileType::Code(lang) => chunk_code(content, lang, max_tokens, parser),
        FileType::Markdown => chunk_markdown(content, max_tokens),
        FileType::Pdf | FileType::PlainText => chunk_text(content, max_tokens),
    };

    chunks
        .into_iter()
        .filter(|c| !c.trim().is_empty())
        .map(|c| {
            let token_count = estimate_tokens(&c);
            let content_hash = compute_hash(&c);
            ChunkResult {
                content: c,
                token_count,
                content_hash,
            }
        })
        .collect()
}

fn chunk_code(
    content: &str,
    lang: &CodeLanguage,
    max_tokens: usize,
    parser: &mut tree_sitter::Parser,
) -> Vec<String> {
    match chunk_with_tree_sitter(content, lang, max_tokens, parser) {
        Some(chunks) if !chunks.is_empty() => chunks,
        _ => chunk_text(content, max_tokens),
    }
}

fn chunk_with_tree_sitter(
    content: &str,
    lang: &CodeLanguage,
    max_tokens: usize,
    parser: &mut tree_sitter::Parser,
) -> Option<Vec<String>> {
    let language = get_tree_sitter_language(lang)?;
    parser.set_language(&language).ok()?;
    let tree = parser.parse(content, None)?;
    let root = tree.root_node();

    let mut chunks = Vec::new();
    let mut current_chunk = String::new();
    let mut current_tokens = 0;

    let mut cursor = root.walk();
    for child in root.children(&mut cursor) {
        let node_text = &content[child.byte_range()];
        let node_tokens = estimate_tokens(node_text);

        if node_tokens > max_tokens {
            if !current_chunk.is_empty() {
                chunks.push(current_chunk.clone());
                current_chunk.clear();
                current_tokens = 0;
            }
            let sub_chunks = chunk_text(node_text, max_tokens);
            chunks.extend(sub_chunks);
            continue;
        }

        if current_tokens + node_tokens > max_tokens && !current_chunk.is_empty() {
            chunks.push(current_chunk.clone());
            current_chunk.clear();
            current_tokens = 0;
        }

        if !current_chunk.is_empty() {
            current_chunk.push('\n');
        }
        current_chunk.push_str(node_text);
        current_tokens += node_tokens;
    }

    if !current_chunk.is_empty() {
        chunks.push(current_chunk);
    }

    Some(chunks)
}

fn get_tree_sitter_language(lang: &CodeLanguage) -> Option<tree_sitter::Language> {
    Some(match lang {
        CodeLanguage::Rust => tree_sitter_rust::LANGUAGE.into(),
        CodeLanguage::TypeScript => tree_sitter_typescript::LANGUAGE_TYPESCRIPT.into(),
        CodeLanguage::TypeScriptReact => tree_sitter_typescript::LANGUAGE_TSX.into(),
        CodeLanguage::JavaScript | CodeLanguage::JavaScriptReact => {
            tree_sitter_javascript::LANGUAGE.into()
        }
        CodeLanguage::Python => tree_sitter_python::LANGUAGE.into(),
        CodeLanguage::Go => tree_sitter_go::LANGUAGE.into(),
        CodeLanguage::Ruby => tree_sitter_ruby::LANGUAGE.into(),
        CodeLanguage::Java => tree_sitter_java::LANGUAGE.into(),
        CodeLanguage::C => tree_sitter_c::LANGUAGE.into(),
        CodeLanguage::Cpp => tree_sitter_cpp::LANGUAGE.into(),
        CodeLanguage::CSharp => tree_sitter_c_sharp::LANGUAGE.into(),
    })
}

fn chunk_config(max_tokens: usize) -> text_splitter::ChunkConfig<&'static CoreBPE> {
    text_splitter::ChunkConfig::new(max_tokens).with_sizer(get_tokenizer())
}

fn chunk_markdown(content: &str, max_tokens: usize) -> Vec<String> {
    use text_splitter::MarkdownSplitter;

    MarkdownSplitter::new(chunk_config(max_tokens))
        .chunks(content)
        .map(str::to_string)
        .collect()
}

fn chunk_text(content: &str, max_tokens: usize) -> Vec<String> {
    use text_splitter::TextSplitter;

    TextSplitter::new(chunk_config(max_tokens))
        .chunks(content)
        .map(str::to_string)
        .collect()
}

fn estimate_tokens(text: &str) -> usize {
    get_tokenizer().encode_ordinary(text).len()
}

fn compute_hash(content: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(content.as_bytes());
    format!("{:x}", hasher.finalize())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ingest::scanner::{CodeLanguage, FileType};

    fn make_parser() -> tree_sitter::Parser {
        tree_sitter::Parser::new()
    }

    #[test]
    fn estimate_tokens_empty() {
        assert_eq!(estimate_tokens(""), 0);
    }

    #[test]
    fn estimate_tokens_ascii() {
        assert_eq!(estimate_tokens("hello"), 1);
        assert!(estimate_tokens("hello world") >= 1);
    }

    #[test]
    fn estimate_tokens_multibyte_unicode() {
        let s = "こんにちは";
        let count = estimate_tokens(s);
        assert!(count > 0);
    }

    #[test]
    fn compute_hash_is_deterministic() {
        let h1 = compute_hash("hello world");
        let h2 = compute_hash("hello world");
        assert_eq!(h1, h2);
    }

    #[test]
    fn compute_hash_differs_for_different_content() {
        assert_ne!(compute_hash("foo"), compute_hash("bar"));
    }

    #[test]
    fn compute_hash_is_hex_string() {
        let h = compute_hash("test");
        assert!(h.chars().all(|c| c.is_ascii_hexdigit()));
        assert_eq!(h.len(), 64); // SHA-256 = 32 bytes = 64 hex chars
    }

    #[test]
    fn chunk_content_filters_whitespace_only() {
        let result = chunk_content("   \n  \t  ", &FileType::PlainText, 512, &mut make_parser());
        assert!(result.is_empty());
    }

    #[test]
    fn chunk_content_plain_text_returns_content_hash() {
        let mut parser = make_parser();
        let result = chunk_content("Hello world", &FileType::PlainText, 512, &mut parser);
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].content, "Hello world");
        assert_eq!(result[0].content_hash, compute_hash("Hello world"));
    }

    #[test]
    fn chunk_content_markdown_uses_markdown_splitter() {
        let md = "# Heading\n\nSome paragraph text.\n\n## Sub\n\nMore text.";
        let mut parser = make_parser();
        let result = chunk_content(md, &FileType::Markdown, 512, &mut parser);
        assert!(!result.is_empty());
        for chunk in &result {
            assert!(chunk.token_count <= 512);
        }
    }

    #[test]
    fn chunk_text_respects_max_tokens() {
        // Large content must be split into multiple chunks each within max_tokens
        let content = "hello world ".repeat(200);
        let chunks = chunk_text(&content, 50);
        assert!(chunks.len() > 1);
        for chunk in &chunks {
            assert!(estimate_tokens(chunk) <= 50);
        }
    }

    #[test]
    fn chunk_code_rust_empty_falls_back_to_empty_result() {
        // Empty content: tree-sitter produces no nodes → chunk_text returns nothing →
        // filter removes whitespace-only → result is empty
        let result = chunk_content("", &FileType::Code(CodeLanguage::Rust), 512, &mut make_parser());
        assert!(result.is_empty());
    }

    #[test]
    fn chunk_code_rust_single_function() {
        let code = r#"fn hello() -> &'static str { "world" }"#;
        let mut parser = make_parser();
        let result = chunk_content(code, &FileType::Code(CodeLanguage::Rust), 512, &mut parser);
        assert_eq!(result.len(), 1);
        assert!(result[0].content.contains("fn hello"));
    }

    #[test]
    fn chunk_with_tree_sitter_oversized_node_is_split() {
        // A function with a very large body should be sub-chunked via chunk_text
        let big_body: String = std::iter::repeat("let x = 1;\n").take(300).collect();
        let code = format!("fn big() {{\n{big_body}}}");
        let mut parser = make_parser();
        let result = chunk_content(&code, &FileType::Code(CodeLanguage::Rust), 50, &mut parser);
        assert!(result.len() > 1);
        for chunk in &result {
            assert!(chunk.token_count <= 50);
        }
    }
}
