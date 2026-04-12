# bin/rag-darwin-arm64 — User Guide

## Quick Start

### 1. Install Ollama and set up the embedding model

```bash
brew install ollama
ollama serve

# Download GGUF and register in Ollama
make model
```

This downloads `embeddinggemma-300M-Q8_0` (~329 MB) and creates the `embeddinggemma` model in Ollama.

Alternatively, pull a model directly from Ollama:

```bash
ollama pull nomic-embed-text
```

### 2. Configure environment

Create `.env` file:

```env
EMBEDDING_API_URL=http://localhost:11434/v1/embeddings
EMBEDDING_MODEL=embeddinggemma
EMBEDDING_DIMENSION=768
```

### 3. Index your codebase

```bash
bin/rag-darwin-arm64 ingest ./path/to/project --exclude target,node_modules,.git
```

### 4. Embed chunks

```bash
bin/rag-darwin-arm64 embed
```

### 5. Start search server

```bash
bin/rag-darwin-arm64 serve
```

### 6. Connect from Claude Code

Add to `.mcp.json` or `~/.claude/settings.json`:

```json
{
  "mcpServers": {
    "rag": {
      "url": "http://127.0.0.1:3100/mcp"
    }
  }
}
```

---

## Workflow

All operations are manual and sequential:

```
1. ingest  →  scan files, create documents, chunk
2. embed   →  embed all pending chunks via API
3. serve   →  start MCP server for search
```

The daemon (`serve`) only serves search requests. To update the index:

```
1. Stop daemon (Ctrl+C)
2. Run ingest (new/changed files)
3. Run embed (new chunks)
4. Start daemon again
```

---

## Commands

### `ingest <path>` — Index files

Scans files, creates documents, chunks them. Runs to completion, then exits.

```bash
# Index a project
bin/rag-darwin-arm64 ingest ./my-project

# Only Rust and Markdown files
bin/rag-darwin-arm64 ingest ./my-project --extensions rs,md

# Exclude patterns
bin/rag-darwin-arm64 ingest ./my-project --exclude target,node_modules,.git,dist

# Custom source label
bin/rag-darwin-arm64 ingest ./docs --source documentation

# Larger chunks
bin/rag-darwin-arm64 ingest ./my-project --max-tokens 1024
```

Re-running `ingest` on the same path is safe — unchanged files are skipped (content hash comparison).

### `embed` — Embed pending chunks

Finds all chunks without vectors, sends them to the embedding API in batches, writes vectors back. Shows progress, exits when done.

```bash
bin/rag-darwin-arm64 embed

# Custom batch size
bin/rag-darwin-arm64 embed --batch-size 32
```

### `serve` — Start MCP server

Starts the MCP search server. Serves until stopped with Ctrl+C.

```bash
# Default (localhost:3100)
bin/rag-darwin-arm64 serve

# Custom port
bin/rag-darwin-arm64 serve --port 4000

# Bind to all interfaces
bin/rag-darwin-arm64 serve --host 0.0.0.0
```

### `status` — Show statistics

```bash
bin/rag-darwin-arm64 status
```

Shows: document count, chunk count, embedded/pending chunks, documents by source.

---

## Ollama Setup

### Recommended Models

| Model | Dimension | Size | Notes |
|-------|-----------|------|-------|
| `embeddinggemma` | 768 | 329M | Default, bundled via `make model` |
| `nomic-embed-text` | 768 | 274M | Good balance of quality and speed |
| `mxbai-embed-large` | 1024 | 670M | Higher quality, slower |
| `all-minilm` | 384 | 46M | Fastest, lower quality |
| `snowflake-arctic-embed2` | 1024 | 568M | Strong multilingual support |

### Verify Ollama

```bash
curl -s http://localhost:11434/v1/embeddings \
  -H "Content-Type: application/json" \
  -d '{"model": "nomic-embed-text", "input": ["hello world"]}' \
  | python3 -c "import sys,json; print(len(json.load(sys.stdin)['data'][0]['embedding']))"
```

Should print `768` (or whatever dimension your model uses).

---

## Configuration

### Environment Variables

Create `.env` in the working directory or export in shell.

#### Required

| Variable | Example | Description |
|----------|---------|-------------|
| `EMBEDDING_MODEL` | `nomic-embed-text` | Model name |
| `EMBEDDING_DIMENSION` | `768` | Vector dimension |
| `EMBEDDING_API_URL` | `http://localhost:11434/v1/embeddings` | Embedding API endpoint (required when `EMBEDDING_PROVIDER=http`) |

#### Optional

| Variable | Default | Description |
|----------|---------|-------------|
| `EMBEDDING_PROVIDER` | `http` | Embedding backend: `http` or `grpc` |
| `DB_PATH` | `./data/surreal` | Database directory |
| `HOST` | `127.0.0.1` | Server listen address |
| `PORT` | `3100` | MCP server port |
| `LOG_LEVEL` | `info` | Log level (trace/debug/info/warn/error) |
| `MAX_CHUNK_TOKENS` | `512` | Max tokens per chunk |
| `RETRIEVE_LIMIT` | `100` | Search candidates per stage |
| `SEARCH_TOP_K` | `5` | Final results returned |
| `EMBEDDING_API_KEY` | — | API key for paid services (OpenAI). Not needed for Ollama |
| `EMBEDDING_BATCH_SIZE` | `64` | Texts per API call |
| `INFERENCE_SERVICE_URL` | `http://localhost:50060` | gRPC endpoint (when `EMBEDDING_PROVIDER=grpc`) |
| `INFERENCE_SERVICE_AUTH_TOKEN` | — | Bearer token for wzd-inference-service RPCs |
| `INFERENCE_SERVICE_CA_CERT` | — | Path to PEM CA certificate for TLS connections to wzd-inference-service |

### Example Configurations

#### Ollama (local, free)

```env
EMBEDDING_API_URL=http://localhost:11434/v1/embeddings
EMBEDDING_MODEL=embeddinggemma
EMBEDDING_DIMENSION=768
```

#### Ollama (nomic-embed-text)

```env
EMBEDDING_API_URL=http://localhost:11434/v1/embeddings
EMBEDDING_MODEL=nomic-embed-text
EMBEDDING_DIMENSION=768
```

#### OpenAI

```env
EMBEDDING_API_URL=https://api.openai.com/v1/embeddings
EMBEDDING_API_KEY=sk-...
EMBEDDING_MODEL=text-embedding-3-small
EMBEDDING_DIMENSION=1536
```

#### vLLM / Text Embeddings Inference

```env
EMBEDDING_API_URL=http://localhost:8080/v1/embeddings
EMBEDDING_MODEL=BAAI/bge-large-en-v1.5
EMBEDDING_DIMENSION=1024
```

#### wzd-inference-service (gRPC)

```env
EMBEDDING_PROVIDER=grpc
EMBEDDING_MODEL=bge-m3
EMBEDDING_DIMENSION=1024
INFERENCE_SERVICE_URL=http://localhost:50060
INFERENCE_SERVICE_AUTH_TOKEN=your-embed-token
```

With TLS:

```env
EMBEDDING_PROVIDER=grpc
EMBEDDING_MODEL=bge-m3
EMBEDDING_DIMENSION=1024
INFERENCE_SERVICE_URL=https://gpu-host:50060
INFERENCE_SERVICE_AUTH_TOKEN=your-embed-token
INFERENCE_SERVICE_CA_CERT=/path/to/ca.pem
```

The wzd-inference-service uses gRPC and supports role-based embedding (passage vs query prefixes are applied automatically). Optional settings:

- `INFERENCE_SERVICE_AUTH_TOKEN` — Bearer token for inference service RPCs
- `INFERENCE_SERVICE_CA_CERT` — path to PEM CA certificate for TLS connections to wzd-inference-service

---

## MCP Tools

### context_search

Hybrid semantic + full-text search.

```
query   (string, required)  — search query
limit   (integer, default 5) — max results
source  (string, optional)  — filter by source label
```

### get_document

Fetch a document by ID.

```
id  (string, required) — document ID
```

### list_documents

List indexed documents.

```
source  (string, optional)  — filter by source
limit   (integer, default 20) — max results
offset  (integer, default 0)  — pagination offset
```

### stats

System statistics: document/chunk counts, embedding progress, documents by source.

---

## Supported File Types

| Category | Extensions | Parsing |
|----------|-----------|---------|
| Code | `.rs`, `.ts`, `.tsx`, `.js`, `.jsx`, `.py`, `.go`, `.rb`, `.java`, `.c`, `.cpp`, `.cs` | tree-sitter |
| Markdown | `.md`, `.mdx` | Markdown-aware splitting |
| PDF | `.pdf` | Text extraction + splitting |
| Plain text | `.txt`, `.toml`, `.yaml`, `.yml`, `.json`, `.xml`, `.html`, `.css`, `.sql`, `.sh` | Text splitting |

Binary files are automatically skipped.

---

## Typical Scenarios

### Index multiple sources

```bash
bin/rag-darwin-arm64 ingest ./backend --source backend
bin/rag-darwin-arm64 ingest ./frontend --source frontend
bin/rag-darwin-arm64 ingest ./docs --source docs
bin/rag-darwin-arm64 embed
bin/rag-darwin-arm64 serve
```

### Update index after code changes

```bash
# Stop daemon first (Ctrl+C), then:
bin/rag-darwin-arm64 ingest ./my-project --exclude target,node_modules,.git
bin/rag-darwin-arm64 embed
bin/rag-darwin-arm64 serve
```

### Change embedding model

Changing model requires re-embedding (vectors are incompatible between models).

```bash
rm -rf ./data/surreal
# Update .env with new model/dimension
bin/rag-darwin-arm64 ingest ./my-project
bin/rag-darwin-arm64 embed
bin/rag-darwin-arm64 serve
```

---

## Troubleshooting

### Ollama connection refused

Ensure Ollama is running: `ollama serve`

### Wrong embedding dimension

Check dimension matches the model:

```bash
curl -s http://localhost:11434/v1/embeddings \
  -d '{"model":"nomic-embed-text","input":["test"]}' \
  | python3 -c "import sys,json; print(len(json.load(sys.stdin)['data'][0]['embedding']))"
```

### Database locked

The database supports one process at a time. Stop the daemon before running `ingest` or `embed`:

```bash
# If daemon is running, stop it first (Ctrl+C), then run CLI commands
```

### Re-index from scratch

```bash
rm -rf ./data/surreal
bin/rag-darwin-arm64 ingest ./my-project
bin/rag-darwin-arm64 embed
bin/rag-darwin-arm64 serve
```
