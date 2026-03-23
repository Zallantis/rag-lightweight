# rag-lightweight

`rag-lightweight` is a lightweight, single-binary RAG system for local or
single-client use. It combines an embedded SurrealDB store, manual
ingestion/embedding workflows, hybrid search, and an MCP server in one
Rust codebase.

## What It Does

- indexes local files into an embedded database
- chunks documents and stores vectors in SurrealDB
- supports hybrid retrieval with vector search and BM25
- exposes search tools over MCP in `serve` mode
- works with OpenAI-compatible embedding APIs, including Ollama

## Commands

The CLI exposes four main commands:

- `wzd-rag-lightweight ingest <path>`
- `wzd-rag-lightweight embed`
- `wzd-rag-lightweight serve`
- `wzd-rag-lightweight status`

See [USER_GUIDE.md](./USER_GUIDE.md) for setup and usage details.

## License

Licensed under the Apache License, Version 2.0. See [LICENSE](./LICENSE) for details.
