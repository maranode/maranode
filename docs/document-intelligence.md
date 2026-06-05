# Document Intelligence

Maranode treats uploaded documents as first-class objects, not just raw text. When you ingest a PDF or document file, the pipeline extracts structured content, tracks page numbers, detects section headings, and generates a summary - so answers to your questions can cite exactly where in a document the information came from.

## How it works

1. **Extraction** - the file is parsed and split into pages. For PDFs, metadata (title, author, page count) is pulled from the document info dictionary.
2. **Chunking** - each page is split into overlapping chunks using a boundary-aware strategy (paragraph breaks -> sentence ends -> whitespace). Each chunk carries its page number and inferred section heading.
3. **Embedding** - chunks are embedded with the configured embedding model and stored in the local vector database.
4. **Summarization** - after ingestion, the first ~8,000 characters are passed to the active LLM to generate a 3–5 sentence summary, stored alongside the document.
5. **Retrieval** - when you ask a question, the most relevant chunks are retrieved and injected into the prompt. Citations include page numbers and section headings.

## Supported formats

| Format | Page tracking | Metadata |
|---|---|---|
| `.pdf` | ✓ per page | title, author, page count |
| `.txt`, `.md`, `.csv`, `.log`, `.rst` | single page | - |

Scanned image PDFs (no text layer) are rejected with a clear error message. Convert them with OCR first (e.g. `ocrmypdf`).

## Citations in answers

With page tracking enabled, citations in RAG answers include the page number and section:

```
According to the contract [1, Liability Clause, p.12], the maximum liability is capped at...
```

Without section/page info (plain text files):

```
The blood pressure reading [1] (source: patient-notes.txt) was 120/80.
```

## Ingest via web UI

Open **Knowledge Base** in the sidebar. Choose a collection, optionally add a source label, then drag a file into the drop zone or click to browse. After upload, the document row shows:

- Title (from PDF metadata, or filename)
- Author (if present in PDF metadata)
- Page count
- Chunk count
- Ingestion time
- AI-generated summary (displayed below the row)

## Ingest via API

```bash
# Upload a file
curl -X POST http://localhost:11984/v1/rag/documents/upload \
  -H "Authorization: Bearer $ADMIN_KEY" \
  -F "file=@annual-report.pdf" \
  -F "collection=finance" \
  -F "source=annual-report-2024.pdf"

# Response includes summary when an LLM model is available
# {
#   "document_id": "...",
#   "collection": "finance",
#   "chunks": 47,
#   "pages": 12,
#   "summary": "This annual report covers FY2024 results for Acme Corp..."
# }
```

## Document API

```bash
# get document details (metadata + summary)
curl http://localhost:11984/v1/rag/documents/<id>

# get just the summary
curl http://localhost:11984/v1/rag/documents/<id>/summary

# list documents in a collection (includes metadata)
curl http://localhost:11984/v1/rag/collections/finance/documents
```

## Summarization

Summarization runs automatically on file upload if an LLM model is installed. It uses a short prompt asking for 3–5 sentences covering the main topic, key facts, and conclusions. The summary is generated once at ingest time and stored - it does not re-run on subsequent queries.

If no LLM model is installed at ingest time, the summary field is `null`. You can re-ingest the document later to generate a summary once a model is available.

## Configuration

Document intelligence uses the same RAG configuration as standard retrieval:

```toml
[rag]
enabled            = true
embedding_model    = "bge-m3:latest"
chunk_size         = 1200
chunk_overlap      = 200
```

Smaller `chunk_size` values produce more granular page-level citations. Larger values preserve more context per chunk but may span multiple pages. The default (1200 characters) works well for most documents.
