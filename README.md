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
- works with OpenAI-compatible embedding APIs (Ollama, OpenAI, vLLM) and wzd-inference-service gRPC

## Commands

The CLI exposes four main commands:

- `wzd-rag-lightweight ingest <path>`
- `wzd-rag-lightweight embed`
- `wzd-rag-lightweight serve`
- `wzd-rag-lightweight status`

See [USER_GUIDE.md](./USER_GUIDE.md) for setup and usage details.

## MCP Tools

The `serve` command exposes these MCP tools:

| Tool | Description |
|------|-------------|
| `context_search` | Hybrid semantic + full-text search |
| `get_document` | Retrieve a single document by ID |
| `list_documents` | List/filter documents with pagination |
| `stats` | Knowledge base statistics |
| `create_document` | Create document with immediate chunking & embedding |
| `update_document` | Update document with re-chunking & re-embedding |
| `set_document_parent` | Set/remove parent in document hierarchy |
| `get_document_parent` | Get parent document |
| `get_document_children` | Get direct children |
| `get_document_ancestors` | Get path to root |
| `get_document_descendants` | Get full subtree |

### Document Hierarchy

Documents support parent-child relationships with unlimited nesting depth. Use `set_document_parent` to build trees, or pass `parent_id` when creating a document.

### Filtering by custom_attributes

The `list_documents` tool accepts a `filters` parameter for querying documents by `custom_attributes`.

**Operators:** `$eq`, `$ne`, `$gt`, `$gte`, `$lt`, `$lte`, `$in`, `$contains`, `$any` (array intersection), `$all` (array superset)

**Logical:** `$and`, `$or` (nestable). Top-level object fields use implicit AND.

**Nested paths** via nested objects.

Examples:

```json
{"filters": {"category": "docs"}}
```

```json
{"filters": {"version": {"$gte": 2}, "tags": {"$contains": "api"}}}
```

```json
{
  "filters": {
    "$or": [
      {"category": "docs"},
      {"category": "logs"}
    ]
  }
}
```

```json
{"filters": {"config": {"env": {"$in": ["prod", "staging"]}}}}
```

See [docs/filters/PLAN.md](./docs/filters/PLAN.md) for the full DSL specification.

## License

Licensed under the Apache License, Version 2.0. See [LICENSE](./LICENSE) for details.
