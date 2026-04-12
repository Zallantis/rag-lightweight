# wzd-rag-lightweight — Specification

Lightweight, single-binary, open-source RAG system. Simplified standalone version of [rag-server](../README.md) for single-client local use.

## Architecture Overview

```
  CLI (interactive)                    Daemon
  ─────────────────                    ──────
  ingest <path>  ──┐                   serve ──── MCP Server (HTTP/SSE)
  embed          ──┼──► Embedded DB ◄────────────  context_search
  status         ──┘    (SurrealKV)               stats, get_document
                             │                    list_documents
                             │
                    ┌────────▼────────┐
                    │  External       │
                    │  Embedding API  │
                    │  (OpenAI-compat)│
                    └─────────────────┘
```

- Single binary, two modes: CLI (ingest/embed/status) and daemon (serve)
- Embedded SurrealDB with SurrealKV (Rust-native, no C dependencies)
- Vectors stored directly in SurrealDB (HNSW index for KNN)
- Hybrid search: vector similarity + BM25 fulltext, merged via RRF
- Built-in MCP server (HTTP/SSE, `serve` mode only)
- Pluggable embedding adapters: HTTP (OpenAI-compatible `/v1/embeddings`) and gRPC (wzd-embed)
- All ingestion, chunking, embedding — manual CLI execution only
- No background workers, no job queues, no management API
- No authentication, no encryption, no multi-tenancy

---

## CLI Structure

```
wzd-rag-lightweight <command> [flags]
```

### Commands

| Command | Mode | Description |
|---------|------|-------------|
| `serve` | Daemon | Start MCP server for search requests |
| `ingest <path>` | CLI | Scan files, create documents, chunk — runs to completion |
| `embed` | CLI | Embed all pending chunks via external API — runs to completion |
| `status` | CLI | Show DB statistics |

> **Workflow:** Stop daemon → `ingest` → `embed` → start daemon.
> CLI commands open the database directly. The daemon must not be running during CLI operations (SurrealKV single-process lock).

### Global Flags

| Flag | Env Var | Default | Description |
|------|---------|---------|-------------|
| `--db-path` | `DB_PATH` | `./data/surreal` | SurrealDB data directory |
| `--log-level` | `LOG_LEVEL` | `info` | Log level (trace/debug/info/warn/error) |

### `serve` Flags

| Flag | Env Var | Default | Description |
|------|---------|---------|-------------|
| `--host` | `HOST` | `127.0.0.1` | MCP server listen address |
| `--port` | `PORT` | `3100` | MCP server listen port |

### `ingest` Flags

| Flag | Default | Description |
|------|---------|-------------|
| `--extensions` | all supported | Filter by file extensions (comma-separated) |
| `--exclude` | — | Exclude glob patterns (e.g. `target,node_modules,.git`) |
| `--source` | `local` | Source label for documents |
| `--max-tokens` | `512` | Max tokens per chunk |

### `embed` Flags

| Flag | Env Var | Default | Description |
|------|---------|---------|-------------|
| `--batch-size` | `EMBEDDING_BATCH_SIZE` | `64` | Texts per API call |

---

## Database Schema

Embedded SurrealDB. Single namespace `rag`, single database `main`.

### document

```sql
DEFINE TABLE document SCHEMAFULL;
DEFINE FIELD source      ON document TYPE string;
DEFINE FIELD source_id   ON document TYPE string;
DEFINE FIELD title       ON document TYPE string;
DEFINE FIELD content     ON document TYPE string;
DEFINE FIELD content_hash ON document TYPE option<string>;
DEFINE FIELD metadata    ON document TYPE option<object> FLEXIBLE;
DEFINE FIELD created_at  ON document TYPE datetime DEFAULT time::now();
DEFINE FIELD updated_at  ON document TYPE datetime DEFAULT time::now();

DEFINE INDEX idx_document_source       ON document FIELDS source;
DEFINE INDEX idx_document_source_id    ON document FIELDS source, source_id UNIQUE;
DEFINE INDEX idx_document_content_hash ON document FIELDS content_hash;

-- Full-text search (BM25)
DEFINE ANALYZER vs TOKENIZERS blank,class FILTERS lowercase,ascii,snowball(english);
DEFINE INDEX ft_content ON document FIELDS content FULLTEXT ANALYZER vs BM25(1.2, 0.75);
```

### chunk

```sql
DEFINE TABLE chunk SCHEMAFULL;
DEFINE FIELD document      ON chunk TYPE record<document>;
DEFINE FIELD content        ON chunk TYPE string;
DEFINE FIELD position       ON chunk TYPE int;
DEFINE FIELD token_count    ON chunk TYPE option<int>;
DEFINE FIELD content_hash   ON chunk TYPE option<string>;
DEFINE FIELD embedded_at    ON chunk TYPE option<datetime>;
DEFINE FIELD vector         ON chunk TYPE option<array<float>>;
DEFINE FIELD metadata       ON chunk TYPE option<object> FLEXIBLE;
DEFINE FIELD created_at     ON chunk TYPE datetime DEFAULT time::now();

DEFINE INDEX idx_chunk_document     ON chunk FIELDS document;
DEFINE INDEX idx_chunk_doc_position ON chunk FIELDS document, position UNIQUE;

-- Vector index (HNSW for KNN search, SurrealDB v3)
-- DIMENSION set dynamically at startup from EMBEDDING_DIMENSION config
DEFINE INDEX idx_chunk_vector ON chunk FIELDS vector HNSW DIMENSION {dim} DIST COSINE TYPE F32;
```

### Design decisions

- **Two tables only** — `document` and `chunk`. No job queue, no config table.
- **Vectors on chunk table** — SurrealDB HNSW index enables KNN directly.
- **Pending chunks** — chunks without `vector` field need embedding (`WHERE vector IS NONE`).
- **content_hash** on document — deduplication and incremental update detection.
- **HNSW DIMENSION** applied dynamically at startup from `EMBEDDING_DIMENSION` env var.

---

## Daemon Mode (`serve`)

Single tokio task — MCP HTTP/SSE server only.

```
wzd-rag-lightweight serve
  │
  └─ MCP Server (axum + rmcp)
       ├─ Listens on HOST:PORT
       ├─ Opens embedded SurrealDB (read path)
       ├─ Initializes embedding service (for query embedding)
       └─ Serves 4 MCP tools
```

The daemon does NOT perform any ingestion, chunking, or embedding. It only serves search requests. Graceful shutdown via SIGINT/SIGTERM.

---

## Ingestion (`ingest`)

Runs interactively in CLI, processes everything synchronously, exits when done.

```
wzd-rag-lightweight ingest <path>
  │
  ├─ 1. Open embedded SurrealDB, apply schema
  │
  ├─ 2. File Scanner
  │    ├─ Walk directory recursively (walkdir)
  │    ├─ Filter by extension whitelist / exclude patterns
  │    ├─ Detect type: code | markdown | pdf | plain text
  │    └─ Skip binary files (via content_inspector)
  │
  ├─ 3. For each file:
  │    ├─ Read content
  │    ├─ Compute content_hash (SHA-256)
  │    ├─ Upsert document (skip if content_hash unchanged)
  │    ├─ If new/changed: delete old chunks, run chunker
  │    └─ Insert new chunks into DB
  │
  ├─ 4. Print summary (files scanned, documents created/updated/skipped, chunks created)
  │
  └─ 5. Close DB, exit
```

### Supported File Types

| Category | Extensions | Parser |
|----------|-----------|--------|
| Code | `.rs`, `.ts`, `.tsx`, `.js`, `.jsx`, `.py`, `.go`, `.rb`, `.java`, `.c`, `.cpp`, `.cs` | tree-sitter |
| Markdown | `.md`, `.mdx` | text-splitter (markdown mode) |
| PDF | `.pdf` | pdf-extract → text-splitter |
| Plain text | `.txt`, `.toml`, `.yaml`, `.yml`, `.json`, `.xml`, `.html`, `.css`, `.sql`, `.sh` | text-splitter |

### Chunking Strategy

- **Code**: tree-sitter syntactic parsing. Split at function/method/class boundaries. Fallback to text-splitter.
- **Markdown**: text-splitter with markdown awareness (split at headers).
- **PDF**: Extract text via pdf-extract, then text-splitter.
- **Plain text**: text-splitter with configurable max token size.
- Token counting via tiktoken-rs. Default max chunk: 512 tokens.

### Incremental Updates

- Documents identified by `(source, source_id)` UNIQUE index.
- On re-ingest, content_hash compared. If unchanged — skip.
- If changed — old chunks deleted, new chunks created.

---

## Embedding (`embed`)

Runs interactively in CLI, embeds all pending chunks, exits when done.

```
wzd-rag-lightweight embed
  │
  ├─ 1. Open embedded SurrealDB
  │
  ├─ 2. Count pending: SELECT count() FROM chunk WHERE vector IS NONE
  │    └─ If 0 → print "nothing to embed", exit
  │
  ├─ 3. Loop until all embedded:
  │    ├─ Fetch batch: SELECT id, content FROM chunk WHERE vector IS NONE LIMIT $batch_size
  │    ├─ Call embedding API: POST /v1/embeddings
  │    ├─ Update chunks: SET vector = $vec, embedded_at = time::now()
  │    └─ Print progress: [420/1200] 35% embedded
  │
  ├─ 4. Print summary (total embedded, elapsed time)
  │
  └─ 5. Close DB, exit
```

### Embedding Providers

Selected via `EMBEDDING_PROVIDER` env var (`http` or `grpc`, default `http`).

#### HTTP Adapter (OpenAI-compatible)

```
POST {EMBEDDING_API_URL}
Authorization: Bearer {EMBEDDING_API_KEY}  (only if EMBEDDING_API_KEY is set)

{
  "model": "{EMBEDDING_MODEL}",
  "input": ["text1", "text2", ...]
}
```

Response: standard OpenAI embeddings format.

#### gRPC Adapter (wzd-embed)

Connects to wzd-embed-service via gRPC (`EmbedInferenceService.Embed` RPC). Supports role-based embedding: `PASSAGE` role for document indexing, `QUERY` role for search queries. Roles enable the service to apply appropriate prefixes for asymmetric embedding models.

```
gRPC: {EMBED_SERVICE_URL}/wzd.EmbedInferenceService/Embed
Authorization: Bearer {EMBED_SERVICE_AUTH_TOKEN}  (optional)

EmbedRequest {
  model_id: "{EMBEDDING_MODEL}",
  texts: ["text1", "text2", ...],
  role: PASSAGE | QUERY
}
```

Proto files vendored from `rag-server/protos/wzd/`. Only Embed, ListModels, and ModelInfo RPCs are used.

Authentication and TLS:
- `EMBED_SERVICE_AUTH_TOKEN` — protects embedding inference RPCs (optional, disabled if not set)
- `EMBED_SERVICE_CA_CERT` — path to PEM CA certificate for TLS; when set, the gRPC channel is configured with the custom CA and system roots

### Configuration

| Env Var | Required | Default | Description |
|---------|----------|---------|-------------|
| `EMBEDDING_PROVIDER` | no | `http` | Embedding backend: `http` or `grpc` |
| `EMBEDDING_MODEL` | yes | — | Model name (e.g. `nomic-embed-text`, `bge-m3`) |
| `EMBEDDING_DIMENSION` | yes | — | Vector dimension (e.g. 768, 1024, 1536) |
| `EMBEDDING_API_URL` | http only | — | OpenAI-compatible endpoint (e.g. `http://localhost:11434/v1/embeddings`) |
| `EMBEDDING_API_KEY` | no | — | API key for external embedding service (not needed for Ollama) |
| `EMBEDDING_BATCH_SIZE` | no | `64` | Texts per API call |
| `EMBED_SERVICE_URL` | no | `http://localhost:50060` | gRPC endpoint for wzd-embed service |
| `EMBED_SERVICE_AUTH_TOKEN` | no | — | Bearer token for wzd-embed embedding RPCs |

| `EMBED_SERVICE_CA_CERT` | no | — | Path to PEM CA certificate for TLS connections |

---

## Hybrid Search Pipeline

```
Query
  │
  ├─ 1. Embed Query
  │    └─ embedding_api.embed([query]) → query_vector
  │
  ├─ 2. Vector Search (parallel)              3. Full-Text Search (parallel)
  │    │                                           │
  │    │  SELECT id, document, content,            │  SELECT record::id(id) AS eid,
  │    │    vector::similarity::cosine(            │    source, title, content,
  │    │      vector, $query_vec) AS score         │    search::score(0) AS score
  │    │  FROM chunk                               │  FROM document
  │    │  WHERE vector <|$retrieve_limit|>         │  WHERE content @0@ $query
  │    │    $query_vec                             │  ORDER BY score DESC
  │    │  ORDER BY score DESC                      │  LIMIT $retrieve_limit
  │    │                                           │
  │    └─ Vec<ScoredChunk>                         └─ Vec<DocumentResult>
  │
  ├─ 4. RRF Merge (k=60)
  │    ├─ Aggregate vector chunks to document scores (max score per doc)
  │    ├─ RRF: score = sum(1/(k + rank_vector + 1)) + sum(1/(k + rank_fts + 1))
  │    └─ Sort by RRF score descending
  │
  └─ 5. Return TOP_N results
       └─ { document_id, source, title, content, score }
```

### Search Configuration

| Env Var | Default | Description |
|---------|---------|-------------|
| `RETRIEVE_LIMIT` | `100` | Candidates from each search stage |
| `SEARCH_TOP_K` | `5` | Final results returned |

---

## MCP Server

HTTP/SSE transport via axum + rmcp. Open access, no authentication.

### Server Info

- Name: `wzd-rag-lightweight`
- Protocol: `2025-03-26`
- Transport: Streamable HTTP

### Tools (4)

#### context_search

Hybrid semantic + full-text search.

```json
{
  "query": "string (required)",
  "limit": "integer (default: 5, max: 50)",
  "source": "string (optional, filter by source)"
}
```

Response: JSON array of `{ document_id, source, title, content, score }`.

#### get_document

Fetch document by ID.

```json
{
  "id": "string (required)"
}
```

Response: `{ id, source, source_id, title, content }`.

#### list_documents

Paginated document listing.

```json
{
  "source": "string (optional)",
  "limit": "integer (default: 20, max: 200)",
  "offset": "integer (default: 0)"
}
```

Response: JSON array of `{ id, source, source_id, title }` (no content).

#### stats

RAG system statistics.

```json
{}
```

Response:
```json
{
  "documents": 150,
  "chunks": 1200,
  "embedded_chunks": 1180,
  "pending_chunks": 20,
  "documents_by_source": [{ "source": "local", "count": 150 }],
  "embedding_model": "nomic-embed-text",
  "embedding_dimension": 768
}
```

---

## Configuration Summary

| Env Var | Required | Default | Description |
|---------|----------|---------|-------------|
| `DB_PATH` | no | `./data/surreal` | SurrealDB data directory |
| `HOST` | no | `127.0.0.1` | MCP server listen address |
| `PORT` | no | `3100` | MCP server listen port |
| `LOG_LEVEL` | no | `info` | Log level |
| `MAX_CHUNK_TOKENS` | no | `512` | Max tokens per chunk |
| `RETRIEVE_LIMIT` | no | `100` | Search candidates per stage |
| `SEARCH_TOP_K` | no | `5` | Final search results |
| `EMBEDDING_PROVIDER` | no | `http` | Embedding backend: `http` or `grpc` |
| `EMBEDDING_API_URL` | http only | — | Embedding API endpoint |
| `EMBEDDING_API_KEY` | no | — | API key for external embedding service (not needed for Ollama) |
| `EMBEDDING_MODEL` | yes | — | Embedding model name |
| `EMBEDDING_DIMENSION` | yes | — | Vector dimension |
| `EMBEDDING_BATCH_SIZE` | no | `64` | Texts per API call |
| `EMBED_SERVICE_URL` | no | `http://localhost:50060` | gRPC endpoint for wzd-embed service |
| `EMBED_SERVICE_AUTH_TOKEN` | no | — | Bearer token for wzd-embed embedding RPCs |

| `EMBED_SERVICE_CA_CERT` | no | — | Path to PEM CA certificate for TLS connections |

All env vars overridable by CLI flags. CLI flags take precedence.

---

## Module Structure

```
src/
├── main.rs                    # Entry point, clap CLI routing
├── cli/
│   ├── mod.rs                 # Clap derive structs
│   ├── serve.rs               # Start MCP daemon
│   ├── ingest.rs              # File scanning, document creation, chunking
│   ├── embed.rs               # Batch embedding of pending chunks
│   └── status.rs              # Show stats
├── config.rs                  # Env + CLI merge
├── db/
│   ├── mod.rs                 # SurrealDB embedded client init
│   ├── schema.rs              # Initial schema (embedded, applied on first run)
│   ├── documents.rs           # Document CRUD
│   ├── chunks.rs              # Chunk CRUD
│   └── search.rs              # Vector KNN + FTS queries
├── embed/
│   ├── mod.rs                 # Provider factory (create_embedding_service)
│   ├── service.rs             # EmbeddingService trait, EmbedRole enum
│   ├── http_adapter.rs        # HttpEmbeddingService (OpenAI-compatible)
│   ├── grpc_adapter.rs        # GrpcEmbeddingService (wzd-embed via tonic)
│   └── proto.rs               # Generated protobuf types (tonic::include_proto!)
├── ingest/
│   ├── mod.rs
│   ├── scanner.rs             # File walker + type detection
│   └── chunker.rs             # Chunking (code/markdown/pdf/text)
├── search/
│   ├── mod.rs
│   ├── pipeline.rs            # SearchPipeline, SearchContext
│   ├── vector.rs              # VectorSearchStage (SurrealDB KNN)
│   ├── fulltext.rs            # FullTextSearchStage (SurrealDB BM25)
│   └── merge.rs               # RRF merge (k=60)
├── mcp/
│   ├── mod.rs
│   ├── server.rs              # MCP ServerHandler impl
│   └── tools.rs               # Tool definitions + output types
└── error.rs                   # Error types (thiserror)
```

---

## Dependencies

```toml
[dependencies]
# Async runtime
tokio = { version = "1", features = ["full"] }

# Database (embedded)
surrealdb = { version = "3", features = ["kv-surrealkv"] }

# Web server (MCP transport)
axum = "0.7"

# MCP Protocol
rmcp = { version = "1", features = ["server", "transport-streamable-http-server"] }

# HTTP client (embedding API)
reqwest = { version = "0.12", features = ["json"] }

# gRPC client (wzd-embed)
tonic = { version = "0.12", features = ["channel"] }
prost = "0.13"

# CLI
clap = { version = "4", features = ["derive", "env"] }

# Serialization
serde = { version = "1", features = ["derive"] }
serde_json = "1"

# Logging
tracing = "0.1"
tracing-subscriber = { version = "0.3", features = ["fmt", "env-filter"] }

# Error handling
anyhow = "1"
thiserror = "2"

# Hashing
sha2 = "0.10"

# Time
chrono = { version = "0.4", features = ["serde"] }

# Chunking
tree-sitter = "0.24"
tree-sitter-rust = "0.24"
tree-sitter-typescript = "0.23"
tree-sitter-javascript = "0.23"
tree-sitter-python = "0.23"
tree-sitter-go = "0.23"
tree-sitter-ruby = "0.23"
tree-sitter-java = "0.23"
tree-sitter-c = "0.23"
tree-sitter-cpp = "0.23"
tree-sitter-c-sharp = "0.23"
tiktoken-rs = "0.6"
text-splitter = { version = "0.18", features = ["tiktoken-rs", "markdown"] }

# File type detection
content_inspector = "0.2"

# PDF
pdf-extract = "0.10"

# Async trait
async-trait = "0.1"

# File system walking
walkdir = "2"

# Config
dotenvy = "0.15"
```

---

## Reference Files (rag-server)

Code to port/adapt from the full rag-server:

These files from rag-server can be used as implementation reference:

| File | What to reference |
|------|-------------------|
| `crates/wzd-embed/src/http_adapter.rs` | HTTP embedding adapter pattern |
| `crates/wzd-services/src/search_engine/stages/merge.rs` | RRF merge algorithm (k=60) |
| `crates/wzd-services/src/retrieval_engine/server.rs` | MCP ServerHandler pattern (ignore auth/token/rate-limit logic) |
| `crates/wzd-db/src/documents/queries.rs` | FTS query pattern (`@0@` operator) |

---

## Implementation Phases

### Phase 1: Foundation
- `main.rs` + `cli/mod.rs` (clap)
- `config.rs` (env + CLI merge)
- `error.rs` (error types)
- `db/mod.rs` + `db/schema.rs` (embedded SurrealDB init + schema)
- `db/documents.rs` + `db/chunks.rs` (CRUD)

### Phase 2: Ingestion
- `ingest/scanner.rs` (file walking + detection)
- `ingest/chunker.rs` (tree-sitter, markdown, PDF, text splitting)
- `cli/ingest.rs` (scan → upsert → chunk → exit)

### Phase 3: Embedding
- `embed/service.rs` + `embed/http_adapter.rs` (trait + HTTP adapter)
- `cli/embed.rs` (batch embed pending chunks → exit)

### Phase 4: Search + MCP
- `db/search.rs` (vector KNN + FTS queries)
- `search/pipeline.rs` + stages (vector, fulltext, merge)
- `mcp/server.rs` + `mcp/tools.rs` (MCP handler)
- `cli/serve.rs` (start MCP server)
- `cli/status.rs` (show stats)
