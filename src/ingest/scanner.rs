use std::path::{Path, PathBuf};
use walkdir::WalkDir;

#[derive(Debug, Clone, PartialEq)]
pub enum FileType {
    Code(CodeLanguage),
    Markdown,
    Pdf,
    PlainText,
}

#[derive(Debug, Clone, PartialEq)]
pub enum CodeLanguage {
    Rust,
    TypeScript,
    TypeScriptReact,
    JavaScript,
    JavaScriptReact,
    Python,
    Go,
    Ruby,
    Java,
    C,
    Cpp,
    CSharp,
}

#[derive(Debug)]
pub struct ScannedFile {
    pub path: PathBuf,
    pub file_type: FileType,
    pub relative_path: String,
}

const CODE_EXTENSIONS: &[(&str, CodeLanguage)] = &[
    ("rs", CodeLanguage::Rust),
    ("ts", CodeLanguage::TypeScript),
    ("tsx", CodeLanguage::TypeScriptReact),
    ("js", CodeLanguage::JavaScript),
    ("jsx", CodeLanguage::JavaScriptReact),
    ("py", CodeLanguage::Python),
    ("go", CodeLanguage::Go),
    ("rb", CodeLanguage::Ruby),
    ("java", CodeLanguage::Java),
    ("c", CodeLanguage::C),
    ("cpp", CodeLanguage::Cpp),
    ("cs", CodeLanguage::CSharp),
];

const MARKDOWN_EXTENSIONS: &[&str] = &["md", "mdx"];
const PDF_EXTENSIONS: &[&str] = &["pdf"];
const PLAIN_TEXT_EXTENSIONS: &[&str] = &[
    "txt", "toml", "yaml", "yml", "json", "xml", "html", "css", "sql", "sh",
];

pub fn detect_file_type(path: &Path) -> Option<FileType> {
    let ext = path.extension()?.to_str()?.to_lowercase();

    for (code_ext, lang) in CODE_EXTENSIONS {
        if ext == *code_ext {
            return Some(FileType::Code(lang.clone()));
        }
    }

    if MARKDOWN_EXTENSIONS.contains(&ext.as_str()) {
        return Some(FileType::Markdown);
    }

    if PDF_EXTENSIONS.contains(&ext.as_str()) {
        return Some(FileType::Pdf);
    }

    if PLAIN_TEXT_EXTENSIONS.contains(&ext.as_str()) {
        return Some(FileType::PlainText);
    }

    None
}

pub fn scan_directory(
    root: &Path,
    extensions: Option<&[String]>,
    exclude: Option<&[String]>,
) -> Vec<ScannedFile> {
    let mut files = Vec::new();

    for entry in WalkDir::new(root)
        .follow_links(false)
        .into_iter()
        .filter_map(|e| e.ok())
    {
        let path = entry.path();

        if !path.is_file() {
            continue;
        }

        if let Some(excludes) = exclude {
            let path_str = path.to_string_lossy();
            if excludes.iter().any(|pattern| path_str.contains(pattern)) {
                continue;
            }
        }

        if let Some(exts) = extensions {
            if let Some(ext) = path.extension().and_then(|e| e.to_str()) {
                if !exts.iter().any(|e| e == ext) {
                    continue;
                }
            } else {
                continue;
            }
        }

        let Some(file_type) = detect_file_type(path) else {
            continue;
        };

        if file_type != FileType::Pdf {
            if let Ok(mut f) = std::fs::File::open(path) {
                use std::io::Read;
                let mut buf = [0u8; 8192];
                let n = f.read(&mut buf).unwrap_or(0);
                if content_inspector::inspect(&buf[..n]).is_binary() {
                    continue;
                }
            }
        }

        let relative_path = path
            .strip_prefix(root)
            .unwrap_or(path)
            .to_string_lossy()
            .to_string();

        files.push(ScannedFile {
            path: path.to_path_buf(),
            file_type,
            relative_path,
        });
    }

    tracing::info!("Scanned {} files in {}", files.len(), root.display());
    files
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    #[test]
    fn detects_rust_source() {
        assert_eq!(
            detect_file_type(Path::new("main.rs")),
            Some(FileType::Code(CodeLanguage::Rust))
        );
    }

    #[test]
    fn detects_typescript() {
        assert_eq!(
            detect_file_type(Path::new("app.ts")),
            Some(FileType::Code(CodeLanguage::TypeScript))
        );
    }

    #[test]
    fn detects_tsx_not_ts() {
        assert_eq!(
            detect_file_type(Path::new("component.tsx")),
            Some(FileType::Code(CodeLanguage::TypeScriptReact))
        );
    }

    #[test]
    fn detects_python() {
        assert_eq!(
            detect_file_type(Path::new("script.py")),
            Some(FileType::Code(CodeLanguage::Python))
        );
    }

    #[test]
    fn detects_markdown() {
        assert_eq!(
            detect_file_type(Path::new("README.md")),
            Some(FileType::Markdown)
        );
        assert_eq!(
            detect_file_type(Path::new("doc.mdx")),
            Some(FileType::Markdown)
        );
    }

    #[test]
    fn detects_pdf() {
        assert_eq!(
            detect_file_type(Path::new("report.pdf")),
            Some(FileType::Pdf)
        );
    }

    #[test]
    fn detects_plain_text_variants() {
        for ext in &[
            "txt", "toml", "yaml", "yml", "json", "xml", "html", "css", "sql", "sh",
        ] {
            let filename = format!("file.{ext}");
            assert_eq!(
                detect_file_type(Path::new(&filename)),
                Some(FileType::PlainText),
                "Expected PlainText for .{ext}"
            );
        }
    }

    #[test]
    fn returns_none_for_unknown_extension() {
        assert_eq!(detect_file_type(Path::new("file.xyz")), None);
        assert_eq!(detect_file_type(Path::new("file.log")), None);
        assert_eq!(detect_file_type(Path::new("file.bin")), None);
    }

    #[test]
    fn returns_none_for_no_extension() {
        assert_eq!(detect_file_type(Path::new("Makefile")), None);
        assert_eq!(detect_file_type(Path::new("Dockerfile")), None);
    }

    #[test]
    fn extension_detection_is_case_insensitive() {
        assert_eq!(
            detect_file_type(Path::new("README.MD")),
            Some(FileType::Markdown)
        );
        assert_eq!(
            detect_file_type(Path::new("main.RS")),
            Some(FileType::Code(CodeLanguage::Rust))
        );
    }

    #[test]
    fn scan_directory_finds_known_extensions() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("main.rs"), "fn main() {}").unwrap();
        std::fs::write(dir.path().join("README.md"), "# Hello").unwrap();
        std::fs::write(dir.path().join("notes.txt"), "notes").unwrap();

        let files = scan_directory(dir.path(), None, None);
        assert_eq!(files.len(), 3);
    }

    #[test]
    fn scan_directory_respects_extension_filter() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("main.rs"), "fn main() {}").unwrap();
        std::fs::write(dir.path().join("lib.rs"), "pub fn lib() {}").unwrap();
        std::fs::write(dir.path().join("script.py"), "pass").unwrap();

        let files = scan_directory(dir.path(), Some(&["rs".to_string()]), None);
        assert_eq!(files.len(), 2);
        assert!(files.iter().all(|f| f.relative_path.ends_with(".rs")));
    }

    #[test]
    fn scan_directory_respects_exclude_patterns() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir(dir.path().join("target")).unwrap();
        std::fs::write(dir.path().join("target/build.rs"), "fn main() {}").unwrap();
        std::fs::write(dir.path().join("main.rs"), "fn main() {}").unwrap();

        let files = scan_directory(dir.path(), None, Some(&["target".to_string()]));
        assert_eq!(files.len(), 1);
        assert!(files[0].relative_path.contains("main.rs"));
    }

    #[test]
    fn scan_directory_skips_files_with_unknown_extension() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("file.log"), "log data").unwrap();
        std::fs::write(dir.path().join("main.rs"), "fn main() {}").unwrap();

        let files = scan_directory(dir.path(), None, None);
        assert_eq!(files.len(), 1);
        assert!(files[0].relative_path.contains("main.rs"));
    }

    #[test]
    fn scan_directory_skips_files_with_no_extension() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("Makefile"), "all:").unwrap();
        std::fs::write(dir.path().join("main.rs"), "fn main() {}").unwrap();

        let files = scan_directory(dir.path(), None, None);
        assert_eq!(files.len(), 1);
    }
}
