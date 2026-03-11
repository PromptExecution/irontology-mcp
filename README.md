# Unified Project Requirements
## `promptexecution` — Rust MCP Agent Orchestration + Semantic Knowledge Runtime

**Version:** 0.1.0-draft  
**Status:** Pre-RFC  
**Audience:** Engineering (primary), Architecture, Executive (§0, §1 only)

---

## §0 Executive Brief

Three parallel initiatives share a spine: **standing data**.

Every dispatchable unit, every software system, every business object exists simultaneously as:
- an **operational fact** (AEMO dispatch, NMI, DUID — strong-typed Rust)
- a **semantic assertion** (RDF triple, OWL class, SHACL shape)
- an **architectural record** (EA repository, TOGAF capability, SysML2 interface block)

Today these three views are disconnected. The same `DUID: BBTHREE1` appears as a raw string in Python, a ghost in iServer365, and an unlinked node in the triple store.

This plan builds the spine: a Rust-native MCP agent runtime where every business object has a stable git-anchored URI, a validated Rust type, and a queryable RDF projection. The same canonical record powers operational pipelines, semantic reasoning, and architectural governance — without rewriting any consumer.

---

## §1 Problem Statement

| Symptom | Root cause |
|---------|-----------|
| NMI/DUID loses asset class after first Python transform | No type carrier across language boundaries |
| Settlement analysts re-join `DUDETAIL` on every query | Provenance stripped at pipeline boundary |
| EA repo, code graph, and RDF store describe same objects with no binding | No shared URI scheme |
| AI agents infer asset topology from DUID prefix heuristics | Standing data never reaches agent context |
| SharePoint project plans cannot be snapshotted or diffed | No bitemporal standing data layer |
| File intake assigns paths by guessing | No SHACL/OWL-driven naming policy |

All symptoms share a cause: **validation at runtime, not at the boundary; provenance discarded, not carried**.

---

## §2 Guiding Principles

1. **Parse at the boundary, carry forever** — invalid states must be unrepresentable downstream (harudagondi / Alexis King)
2. **Git is the bitemporal ledger** — `blob:hash` and `tree:hash` are stable ontology node IDs
3. **One internal contract** — all model paths implement the same provider trait; no vendor SDK leakage
4. **Standing data is the spine** — every business object has operational, semantic, and architectural projections
5. **DSL rules are data** — file processing logic lives in user-authored `.tomllm` + compiled DSL, not code
6. **DRTW / NRtW** — Do Right Then Write / No Rush Then Write; tests before implementation
7. **NRtW / DRY** — find the library; don't build what exists; fix bugs upstream
8. **TRIZ** — contradiction resolution over compromise; preference for inversion and prior action
9. **Agents discover, not guess** — ontology schema and naming conventions are MCP-queryable resources

---

## §3 Architecture Overview

```
┌─────────────────────────────────────────────────────┐
│                    MCP Edge                         │
│     JSON-RPC stdio  +  Streamable HTTP              │
│     tools / resources / prompts registry            │
└─────────────────────┬───────────────────────────────┘
                      │
┌─────────────────────▼───────────────────────────────┐
│                 Orchestrator                        │
│   agent loop · job queue · delegation · policy      │
│         DRTW / NRtW / TRIZ enforcement              │
└──────────────┬────────────────┬────────────────────┘
               │                │
┌──────────────▼────┐  ┌────────▼──────────────────┐
│  Retrieval Fabric │  │     DSL / Rule Engine      │
│ vector+graph+lex  │  │  .tomllm · SHACL · OWL    │
└──────────┬────────┘  └────────┬──────────────────┘
           │                    │
┌──────────▼────────────────────▼──────────────────┐
│                Neumann Knowledge Store            │
│   relational facts · graph edges · embeddings     │
└──────────────────────┬───────────────────────────┘
                       │
┌──────────────────────▼───────────────────────────┐
│              Git Semantic Ledger                  │
│  blob:hash → stable URI · bitemporal snapshots   │
│  git notes → RDF triples · freeze manifests      │
└──────────────────────┬───────────────────────────┘
                       │
┌──────────────────────▼───────────────────────────┐
│             Model Provider Layer                  │
│  provider-api (internal OpenAI contract)          │
│  mistral.rs local ·  remote OpenAI-compat         │
└───────────────────────────────────────────────────┘
```

---

## §4 Workspace Layout

```
promptexecution/
├── Cargo.toml                  # workspace root
├── crates/
│   ├── domain/                 # §5  — business object types (NMI, DUID, AssetClass…)
│   ├── provider-api/           # §6  — ModelProvider trait + DTOs
│   ├── provider-openai/        # §6  — remote OpenAI-compat adapter
│   ├── provider-local/         # §6  — mistral.rs subprocess adapter
│   ├── tomllm/                 # §7  — .tomllm stream-filter parser
│   ├── dsl/                    # §8  — DSL grammar, AST, compiler, rule store
│   ├── git-ledger/             # §9  — git object hashing, notes, freeze, replay
│   ├── indexer/                # §10 — watchexec pipeline, chunking, embed dispatch
│   ├── codegraph/              # §10 — tree-sitter AST → petgraph symbol graph
│   ├── storage-neumann/        # §11 — KnowledgeStore trait + Neumann impl
│   ├── retrieval/              # §12 — fusion retrieval (vector+graph+lexical+ontology)
│   ├── handlers/               # §13 — FileHandler trait + PDF/image/docx/csv impls
│   ├── classifier/             # §13 — SHACL shape matching + OWL class inference
│   ├── naming/                 # §13 — NamingPolicy trait + DSL-driven StoragePlan
│   ├── intake/                 # §13 — end-to-end intake pipeline orchestration
│   ├── mcp-server/             # §14 — MCP transport, tool/resource/prompt registry
│   ├── orchestrator/           # §15 — agent loop, job queue, serial pipeline, delegate
│   ├── forward-mcp/            # §15 — MCP client bridge for cross-agent delegation
│   ├── policy/                 # §15 — DRTW/NRtW/TRIZ, budget, stop conditions
│   └── cli/                    # §16 — standalone daemon entrypoint
├── examples/
│   ├── aemo-nmi/               # NMI/DUID domain types + Arrow schema demo
│   ├── basic-agent/            # single-turn MCP agent
│   ├── code-rag/               # repo search + symbol retrieval
│   ├── receipt-intake/         # PDF → SHACL classify → S3 name
│   └── forward-mcp/            # agent-to-agent delegation
└── tests/
    ├── fixtures/               # parquet/json test datasets (never embedded in test code)
    └── integration/            # BDD-style scenario tests
```

---

## §5 Crate: `domain`

**Purpose:** Canonical Rust business object types for the NEM and general standing data. Single source of truth for all downstream consumers.

### 5.1 Key Types

```rust
// Parse at boundary — never raw str downstream
pub struct Nmi(String);           // 10-char alphanumeric, checksum-validated
pub struct Duid(String);          // 1–20 char, uppercase
pub struct RegionId(/* enum */);  // NSW1 | QLD1 | SA1 | TAS1 | VIC1
pub struct ParticipantId(String); // 1–10 char

// Semantically distinct f64 newtypes — compiler prevents TLF/DLF swap
pub struct TransmissionLossFactor(f64);
pub struct DistributionLossFactor(f64);
pub struct RampRateMwPerMin(f64);  // invariant: >= 0.0

// MMS DUDETAIL enums — exhaustive, no stringly-typed fallthrough
pub enum DispatchType  { Generator, Load, BidirectionalUnit }
pub enum ScheduleType  { Scheduled, SemiScheduled, NonScheduled }
pub enum StartType     { Fast, Slow, NotApplicable }
pub enum FuelSource    { BlackCoal, BrownCoal, NaturalGas, Wind,
                         Solar, Hydro, PumpedHydro, Battery,
                         SolarBattery, Other(String) }
pub enum AssetClass    { ScheduledGenerator, SemiScheduledGenerator,
                         ScheduledLoad, BessStandalone,
                         ColocatedBessSolar, MarketCustomer, VPP }

// Rich standing data record — From<T> downgrades are infallible
pub struct DuDetail { duid, connection_point_id, region, participant_id,
                      dispatch_type, schedule_type, start_type,
                      tlf: TransmissionLossFactor,
                      dlf: DistributionLossFactor,
                      max_ramp_up: RampRateMwPerMin,
                      max_ramp_down: RampRateMwPerMin }

pub struct NmiRecord { nmi, asset_class, primary_duid, region, fuel_source }

// From<T> for NmiRecord: infallible — provenance structurally required
impl From<ColocatedBessSolar> for NmiRecord { ... }
impl From<SolarFarm> for NmiRecord { ... }
```

### 5.2 Arrow Schema Export

```rust
pub fn nmi_record_schema() -> arrow2::datatypes::Schema;
// NOT NULL fields mirror Rust non-Option guarantees
// asset_class: Utf8, NOT NULL — AssetClass enum
// primary_duid: Utf8, NOT NULL — always known
// settlement_date: Timestamp(Ms, Brisbane)
```

### 5.3 TDD Pattern

```
RED:  test_nmi_rejects_9_chars
      test_nmi_rejects_non_alphanumeric
      test_tlf_not_interchangeable_with_dlf  // compile-time test via type
      test_nmi_record_requires_asset_class
      test_colocation_preserves_bess_duid
      test_arrow_schema_nmi_not_null
GREEN: implement parse(), From<T>, schema()
```

### 5.4 Definition of Done

- [ ] All parse constructors `Result<T, E>` — no infallible constructors that accept raw strings
- [ ] `From<T> for NmiRecord` implemented for every concrete asset type
- [ ] Arrow schema tested: NOT NULL fields match Rust non-Option fields
- [ ] Zero `String` → `String` conversions without a parse boundary
- [ ] Python consumer test: schema deserialization round-trip via `pyarrow`

---

## §6 Crate: `provider-api`, `provider-openai`, `provider-local`

**Purpose:** One internal contract for all model interaction. No vendor SDK leaks into orchestration.

### 6.1 Core Trait

```rust
#[async_trait]
pub trait ModelProvider: Send + Sync {
    async fn chat(&self, req: ChatRequest) -> Result<ChatResponse>;
    async fn embed(&self, req: EmbedRequest) -> Result<EmbedResponse>;
    async fn health(&self) -> Result<ProviderHealth>;
    fn model_id(&self) -> &str;
}

pub struct ChatRequest  { model, messages, max_tokens, stream, params }
pub struct EmbedRequest { model, inputs: Vec<String>, batch_size }
// input: Arc<[f32]> for zero-copy fan-out
pub struct EmbedResponse { vectors: Vec<Arc<[f32]>>, model, usage }
```

### 6.2 Implementations

| Crate | Backing | Notes |
|-------|---------|-------|
| `provider-openai` | HTTP `POST /v1/chat/completions`, `/v1/embeddings` | Any OpenAI-compat endpoint |
| `provider-local` | mistral.rs subprocess, OpenAI-compat port | CPU/GPU; crash-isolated |
| `provider-test` | Deterministic fixtures | CI only; replay-safe |

### 6.3 Local Model Strategy

```
runtime daemon
  └── LocalModelManager
        └── spawn mistral.rs --port 11434 --model code
              → POST http://localhost:11434/v1/...
```

CPU mode: `candle` BLAS/SIMD. No LLVM GPU emulation required.

### 6.4 TDD Pattern

```
RED:  test_provider_contract_chat_returns_content
      test_provider_contract_embed_returns_correct_dim
      test_provider_test_is_deterministic
      test_provider_openai_retries_on_429
      test_provider_local_health_check
GREEN: implement adapters
```

### 6.5 Definition of Done

- [ ] `provider-test` fixture-based impl passes all contract tests
- [ ] `provider-openai` passes same contract tests against a mock HTTP server
- [ ] `provider-local` spawns mistral.rs subprocess and passes health check
- [ ] Embedding vectors use `Arc<[f32]>` — zero-copy routing verified by allocation test
- [ ] No `openai` SDK crate appears anywhere except `provider-openai`

---

## §7 Crate: `tomllm`

**Purpose:** Stream-filter parser that strips hint layer before allocation.

### 7.1 Semantics

```
.tomllm file:
  hint channel   → lines starting with `#`  → NEVER allocated, NEVER parsed
  execution channel → remaining lines        → parsed as TOML
```

### 7.2 Implementation

```rust
pub fn load_tomllm<R: Read>(r: R) -> Result<toml::Value> {
    let mut exec = String::new();
    for line in BufReader::new(r).lines() {
        let l = line?;
        if !l.trim_start().starts_with('#') {
            exec.push_str(&l);
            exec.push('\n');
        }
    }
    Ok(toml::from_str(&exec)?)
}
```

Properties: hints never stored, never in heap, never visible to agents. CI test harness reads hint layer separately via a parallel `load_tomllm_hints()` function for evaluation.

### 7.3 TDD Pattern

```
RED:  test_hints_not_in_parsed_value
      test_execution_layer_parses_valid_toml
      test_hint_with_emoji_stripped_correctly
      test_answer_key_readable_by_test_harness_only
GREEN: implement load_tomllm + load_tomllm_hints
```

### 7.4 Definition of Done

- [ ] Heap profiler confirms `#`-lines never allocated during `load_tomllm`
- [ ] Hint layer accessible only via `load_tomllm_hints` (separate function, feature-gated `#[cfg(test)]`)
- [ ] Round-trip: `load_tomllm → serialize → load_tomllm` produces identical `Value`
- [ ] Example `.tomllm` files in `examples/` validated in CI

---

## §8 Crate: `dsl`

**Purpose:** File-type processing rules. User prompts compile → validated DSL AST → stored rules.

### 8.1 Rule Model

```
rule <name>
  when
    extension == ".rs"
    [and media_type == "text/plain"]
    [and contains_field("vendor")]
  then
    handler   = tree_sitter_rust
    extract   = [symbols, calls, imports]
    embed     = [functions, docblocks]
    ontology  = rust_code_graph
    bucket    = "{repo_slug}/code"
    prefix    = "{module_path}/"
    filename  = "{blob_hash}_{symbol}.rs"
    classify  = [shape:RustModule]
```

### 8.2 Pipeline

```
user prompt (natural language)
  ↓  LLM compilation pass (provider-api)
validated DSL AST
  ↓  rule store (Neumann)
rule matcher (watchexec event / intake)
  ↓
handler dispatch
```

### 8.3 Prompt → DSL Compiler Contract

```rust
pub struct DslCompileRequest { prompt: String, examples: Vec<DslRule> }
pub struct DslCompileResult  { rule: DslRule, confidence: f32, warnings: Vec<String> }

#[async_trait]
pub trait DslCompiler {
    async fn compile(&self, req: DslCompileRequest) -> Result<DslCompileResult>;
}
```

### 8.4 TDD Pattern

```
RED:  test_rust_rule_matches_rs_extension
      test_rule_does_not_match_py
      test_compound_when_requires_all_conditions
      test_prompt_compiles_to_valid_ast       // uses provider-test fixture
      test_invalid_dsl_rejected_with_error
GREEN: implement grammar + compiler
```

### 8.5 Definition of Done

- [ ] Grammar defined in EBNF or PEG (documented, not just implicit in code)
- [ ] All built-in rules in `rules/*.tomllm` (hint-stripped at load)
- [ ] DSL compiler tested against `provider-test` — deterministic, no LLM variance in CI
- [ ] Rule store queryable via MCP resource `rules://dsl_catalog`

---

## §9 Crate: `git-ledger`

**Purpose:** Git object hashes as stable ontology node IDs. Bitemporal freeze/replay. RDF triples in git notes.

### 9.1 Identity Model

```
git:blob:<sha>         →  file content identity
git:tree:<sha>         →  directory identity
git:commit:<sha>       →  system-time snapshot
symbol:<blob>:<name>   →  code symbol identity
file:<blob>            →  document identity
```

### 9.2 Core Operations

```rust
pub struct BlobId(pub [u8; 20]);   // git SHA1
pub struct TreeId(pub [u8; 20]);
pub struct CommitId(pub [u8; 20]);

pub struct OntologyNode {
    pub id:      NodeUri,    // "symbol:git:blob:abc123:EmbeddingRouter"
    pub blob:    BlobId,
    pub triples: Vec<Triple>,
}

pub trait GitLedger {
    fn blob_id(&self, path: &Path) -> Result<BlobId>;
    fn attach_triples(&self, blob: BlobId, triples: Vec<Triple>) -> Result<()>;
    fn read_triples(&self, blob: BlobId) -> Result<Vec<Triple>>;
    fn freeze(&self, label: &str, artifacts: &[ArtifactPath]) -> Result<CommitId>;
    fn replay(&self, commit: CommitId) -> Result<FrozenState>;
}
```

### 9.3 Bitemporal Model

```
valid_time  = commit author timestamp  (when content logically existed)
system_time = indexing timestamp       (when system processed it)
```

Freeze manifest committed to git enables time-travel queries:

```
checkout commit X
  → reconstruct ontology graph
  → replay SPARQL query
  → compare with current
```

### 9.4 TDD Pattern

```
RED:  test_blob_id_stable_for_same_content
      test_blob_id_changes_on_content_change
      test_triples_survive_freeze_replay
      test_git_notes_not_in_working_tree
      test_ontology_node_uri_format
GREEN: implement via git2 crate
```

### 9.5 Definition of Done

- [ ] `blob_id()` deterministic for identical content across platforms
- [ ] `attach_triples()` uses `git notes` — no working tree modification
- [ ] `freeze()` produces a commit with deterministic manifest hash
- [ ] `replay()` reconstructs identical `Vec<Triple>` from frozen commit
- [ ] Integration test: freeze → modify → replay → diff shows delta

---

## §10 Crates: `indexer`, `codegraph`

**Purpose:** watchexec-driven incremental re-indexing. AST → symbol graph → embeddings → Neumann.

### 10.1 Watch Pipeline

```
watchexec event
  ↓  git-ledger: compute blob_id, compare with stored
  ↓  (skip if unchanged)
  ↓  rule matcher: select handler + pipeline from dsl
  ↓  handler.extract() → Extraction
  ↓  codegraph.update() → SymbolGraph delta
  ↓  embed_pipeline: chunk → embed → EmbeddingRecord
  ↓  storage-neumann: upsert_facts + upsert_edges + upsert_embeddings
  ↓  git-ledger: attach_triples (ontology edges from extraction)
```

### 10.2 Code Graph

```rust
// Nodes: functions, types, modules, tests, docblocks
// Edges: calls, imports, defines, tests, implements
pub struct SymbolGraph(petgraph::Graph<SymbolNode, EdgeKind>);

pub struct SymbolNode {
    pub id:      NodeUri,     // git:blob:hash:SymbolName
    pub kind:    SymbolKind,  // Function | Type | Module | Test | Doc
    pub doctext: Option<String>,
    pub span:    Span,
}
```

Parsers: `tree-sitter` (Rust, Python, TS, Go, SQL, Markdown, TOML).

### 10.3 Embedding Routing

```rust
pub struct EmbeddingRecord {
    pub id:             Uuid,
    pub vector:         Arc<[f32]>,      // zero-copy fan-out
    pub modality:       Modality,        // CodeSymbol | DocChunk | OntologyNode | …
    pub semantic_weight: f32,
    pub bitemporal:     Bitemporal,
    pub source_blob:    BlobId,
}

// Parallel fan-out per modality
match rec.modality {
    Modality::CodeSymbol => join!(graph_sink.store(r.clone()), vector_sink.store(r)),
    Modality::DocChunk   => join!(vector_sink.store(r.clone()), lexical_sink.store(r)),
    Modality::OntologyNode => graph_sink.store(r).await,
}
```

### 10.4 TDD Pattern

```
RED:  test_unchanged_file_skipped
      test_symbol_graph_adds_function_node
      test_call_edge_detected
      test_embedding_dim_matches_model
      test_fan_out_reaches_all_sinks
      test_blob_id_used_as_symbol_node_id
GREEN: implement indexer pipeline
```

### 10.5 Definition of Done

- [ ] `test_unchanged_file_skipped` — blob hash comparison prevents redundant re-index
- [ ] Symbol graph reconstructible from git notes alone (offline test)
- [ ] Embedding fan-out verified: each modality reaches correct sinks
- [ ] No raw file bytes stored in Neumann — only blob IDs, hashes, and extracted facts
- [ ] Symbols, not raw files, are primary embedding unit

---

## §11 Crate: `storage-neumann`

**Purpose:** Single `KnowledgeStore` trait. Neumann implements it. Other impls for testing.

Current implementation note: Neumann now supports RDF-native Turtle ingestion for ontology resources and direct subject/predicate lookups over stored semantic triples. The current end-to-end path intentionally skips SPARQL in favor of loading stable semantic IRIs and traversing them as first-class graph data.

### 11.1 Trait

```rust
#[async_trait]
pub trait KnowledgeStore: Send + Sync {
    async fn upsert_file(&self, file: FileRecord)          -> Result<()>;
    async fn upsert_facts(&self, facts: Vec<FactRecord>)   -> Result<()>;
    async fn upsert_edges(&self, edges: Vec<EdgeRecord>)   -> Result<()>;
    async fn upsert_embeddings(&self, e: Vec<EmbeddingRecord>) -> Result<()>;
    async fn query(&self, q: SemanticQuery)                -> Result<QueryResult>;
    async fn health(&self)                                 -> Result<StoreHealth>;
}
```

### 11.2 Record Types

```rust
pub struct FileRecord  { id: NodeUri, blob: BlobId, path: String,
                         media_type: String, size: u64, commit: CommitId }
pub struct FactRecord  { subject: NodeUri, predicate: String, object: FactValue }
pub struct EdgeRecord  { from: NodeUri, to: NodeUri, kind: EdgeKind, weight: f32 }
```

### 11.3 Query Model

```rust
pub enum SemanticQuery {
    Vector  { embedding: Arc<[f32]>, top_k: usize, filter: Option<Filter> },
    Graph   { sparql: String },
    Lexical { query: String, top_k: usize },
    Hybrid  { embedding: Arc<[f32]>, sparql: Option<String>,
              lexical: Option<String>, weights: FusionWeights },
}
```

Current semantic ingestion surface:

```rust
pub struct SemanticTriple {
    pub source: String,     // e.g. ontology://naming_conventions
    pub subject: String,    // durable semantic IRI
    pub predicate: String,  // RDF predicate IRI
    pub object: String,     // object IRI or literal
}

#[async_trait]
pub trait KnowledgeStore: Send + Sync {
    async fn ingest_turtle(&self, source: &str, turtle: &str) -> Result<()>;
    async fn related_objects(&self, subject: &str, predicate: &str) -> Result<Vec<String>>;
}
```

### 11.4 Storage Layout

```
.agent/
  state.libsql              # jobs, runs, tool_calls (libSQL — transactional)
  graph.ox                  # ontology graph (Oxigraph — SPARQL)
  chunks.parquet            # text chunks (Arrow/Parquet — columnar)
  embeddings.parquet        # vectors (Arrow/Parquet — SIMD-scannable)
  tantivy/                  # lexical index (tantivy — BM25)
```

> **Note:** Neumann is the primary runtime store. git + parquet snapshots provide bitemporal freeze. libSQL and Oxigraph are fallback for environments where Neumann is unavailable.

### 11.5 TDD Pattern

```
RED:  test_upsert_file_idempotent
      test_vector_query_returns_top_k
      test_graph_query_sparql_finds_edge
      test_hybrid_fusion_scores_plausible
      test_knowledge_store_test_impl_deterministic
GREEN: implement MemoryStore for tests, then NeumannStore
```

### 11.6 Definition of Done

- [ ] `MemoryStore` (in-process, deterministic) passes all trait contract tests
- [ ] `NeumannStore` passes same test suite
- [ ] `upsert_*` operations are idempotent (re-index same blob produces identical state)
- [ ] Hybrid query result order is deterministic for identical inputs (CI-safe)
- [ ] `KnowledgeStore` is the only import from `storage-neumann` in all upstream crates

---

## §12 Crate: `retrieval`

**Purpose:** Fusion retrieval across vector, graph, lexical, ontology lenses.

### 12.1 Fusion

```rust
pub struct FusionWeights {
    pub vector:   f32,  // default 0.35
    pub graph:    f32,  // default 0.30
    pub lexical:  f32,  // default 0.20
    pub ontology: f32,  // default 0.15
}

pub struct SearchRequest {
    pub query:    String,
    pub top_k:    usize,
    pub weights:  FusionWeights,
    pub filter:   Option<Filter>,
    pub expand:   bool,  // graph neighbourhood expansion
}
```

### 12.2 Definition of Done

- [ ] Fusion score deterministic for identical inputs
- [ ] Lexical, vector, and graph legs independently testable
- [ ] MCP tool `repo.search` uses fusion retrieval by default
- [ ] `expand: true` traverses ontology graph for related nodes

---

## §13 Crates: `handlers`, `classifier`, `naming`, `intake`

**Purpose:** File intake pipeline. File type → handler → SHACL classify → naming policy → S3.

### 13.1 Handler Trait

```rust
pub struct IntakeFile { sha256: [u8; 32], bytes: Bytes,
                        path_hint: Option<String>, media_type: Option<String> }

#[async_trait]
pub trait FileHandler: Send + Sync {
    fn score(&self, file: &IntakeFile) -> HandlerScore;  // 0.0–1.0, not bool
    async fn extract(&self, file: &IntakeFile) -> Result<Extraction>;
}

pub struct Extraction {
    pub detected_kind: String,
    pub text:    Option<String>,
    pub fields:  BTreeMap<String, Value>,
    pub dates:   Vec<TemporalValue>,
    pub amounts: Vec<MoneyValue>,
    pub entities: Vec<Entity>,
}
```

### 13.2 Handler Registry

| Handler | Triggers |
|---------|---------|
| `pdf_text` | `application/pdf` + born-digital |
| `pdf_scan` | `application/pdf` + image-dominant → OCR path |
| `image_doc` | `image/*` + aspect ratio → document layout |
| `docx` | `application/vnd.openxmlformats…` |
| `csv_tabular` | `text/csv` + finance field heuristics |
| `email` | `message/rfc822` |
| `generic_binary` | fallback |

External handlers via MCP:
```rust
pub struct McpHandler { target: McpTarget, allowed_tools: Vec<String> }
// implements FileHandler — delegates extract() to remote MCP agent
```

### 13.3 SHACL Classification

```rust
pub struct ClassMatch {
    pub class:       OntologyUri,   // e.g., "doc:Receipt"
    pub shape:       ShapeUri,      // e.g., "shape:ReceiptShape"
    pub confidence:  f32,
    pub matched_by:  Vec<String>,   // field names that satisfied shape
}

#[async_trait]
pub trait Classifier {
    async fn classify(&self, ext: &Extraction) -> Result<Vec<ClassMatch>>;
}
```

Shape matching: required fields present → candidate class. OWL hierarchy: `doc:Receipt rdfs:subClassOf doc:FinancialDocument`.

### 13.4 Naming Policy

```rust
pub struct StoragePlan {
    pub bucket:   String,
    pub prefix:   String,
    pub filename: String,
    pub tags:     BTreeMap<String, String>,
    pub ontology_class: OntologyUri,
    pub shape:    ShapeUri,
}

pub trait NamingPolicy {
    fn derive(&self, ext: &Extraction, class: &ClassMatch) -> Result<StoragePlan>;
}
```

Policy driven by DSL rules:

```
then
  bucket    = "finance-docs-au"
  prefix    = "financial/receipt/{vendor_slug}/{yyyy}/{mm}/"
  filename  = "{date}_{vendor_slug}_{total_minor}_{currency}_receipt.pdf"
  metadata  shacl = shape:ReceiptShape
```

### 13.5 Fallback States

Every file exits in exactly one state:

```rust
pub enum IntakeOutcome { Classified, ClassifiedLowConfidence, Unclassified, FailedExtraction }
```

`Unclassified` → `s3://intake-quarantine/unclassified/`

### 13.6 TDD Pattern

```
RED:  test_receipt_pdf_classified_as_doc_receipt
      test_missing_vendor_produces_low_confidence
      test_unclassified_routes_to_quarantine
      test_mcp_handler_handoff_returns_extraction
      test_naming_policy_produces_canonical_path
      test_ontology_metadata_encoded_in_s3_tags
GREEN: implement handlers + classifier + naming
```

### 13.7 Definition of Done

- [ ] Receipt fixture classified as `doc:Receipt` with confidence > 0.90
- [ ] Unknown file type routes to quarantine, not error
- [ ] `StoragePlan` tags include `ontology_class` and `shape` on every object
- [ ] External MCP handler contract tested with `provider-test` fixture
- [ ] All naming logic in DSL rules — zero hardcoded paths in Rust

---

## §14 Crate: `mcp-server`

**Purpose:** MCP transport (stdio + Streamable HTTP). Tool/resource/prompt registry.

### 14.1 Tools

```
repo.search              repo.read_file           repo.read_symbol
repo.trace_calls         repo.find_dependents      repo.index_status
repo.reindex             repo.freeze_state         repo.attach_ontology
intake.submit            intake.status
ontology.list_classes    ontology.related_resources
ontology.explain_shape
rules.list               rules.compile             rules.apply
agent.run                agent.batch_submit        agent.batch_status
agent.delegate           agent.forward_mcp
memory.search_runs       memory.read_artifact
query.run_native         query.run_semantic
```

### 14.2 Resources (URI-addressed)

```
repo://tree              repo://file/{blob}        repo://symbol/{fqname}
ontology://classes        ontology://predicates     ontology://shapes
ontology://naming_conventions                        ontology://query_languages
rules://dsl_catalog       rules://handlers
run://{id}               artifact://{id}
```

Current startup path:

```rust
Phase2RuntimeConfig::new(...)
  -> optional .with_transport_forwarding()
  -> optional .with_watch(WatchRuntimeConfig { ... })
  -> McpServerRuntime::start_phase2_configured(...)
      -> build ResourceRegistry::with_phase2_resources()
      -> ingest every text/turtle ontology resource into Neumann
      -> register MCP tools, including ontology.related_resources, agent.run, agent.forward_mcp
      -> optionally spawn watchexec against the shared Neumann store
```

Current transport surface:

```rust
McpServerRuntime::serve_stdio()
McpServerRuntime::router(...)

JSON-RPC methods currently handled:
  initialize
  ping
  tools/list
  tools/call
  resources/list
  resources/read
```

Current verification path:

```rust
crates/mcp-server/tests/startup_runtime.rs
  -> startup ingests ontology resources into Neumann
  -> transport-backed agent.forward_mcp works over HTTP
  -> startup watcher indexes a changed file into the shared store

crates/mcp-server/tests/transport.rs
  -> stdio and HTTP return identical tools/list payloads
  -> stdio and HTTP return identical tools/call payloads
```

### 14.3 Definition of Done

- [ ] `tools/list` returns complete registry
- [ ] Every tool has JSON Schema for input validation
- [ ] Every resource URI resolves or returns `404`-equivalent
- [ ] stdio and HTTP transports pass identical tool calls
- [ ] MCP conformance test suite (list → call → resource) passes

---

## §15 Crates: `orchestrator`, `forward-mcp`, `policy`

**Purpose:** Agent loop, async serial job queue, delegation, policy enforcement.

### 15.1 Job Model

```rust
pub struct Job    { id: Uuid, mode: JobMode, steps: Vec<JobStep> }
pub enum  JobMode { Serial | Batch }
pub enum  JobStep { Plan(..) | Retrieve(..) | CallTool(..) |
                    Delegate(..) | Synthesize(..) | Persist(..) }

// Execution: serial within job, async fan-out within step
while let Some(job) = queue.next().await {
    for step in &job.steps {
        let result = exec_step(step, &ctx).await?;
        store_checkpoint(job.id, step, &result).await?;
        if result.requires_wait() { result.wait_handle.await?; }
    }
}
```

### 15.2 Agent Loop

```rust
pub struct AgentState {
    pub task:      String,
    pub turn:      u32,
    pub evidence:  Vec<EvidenceRef>,
    pub scratch:   Vec<ThoughtRecord>,
    pub artifacts: Vec<ArtifactRef>,
    pub budget:    BudgetState,
    pub policy:    PolicyState,
}

// Stop conditions: max_turns | budget | confidence | policy_stop
for turn in 0..state.policy.max_turns {
    let plan     = planner.plan(&state).await?;
    let evidence = retriever.retrieve(&plan).await?;
    let action   = chooser.choose(&plan, &evidence).await?;
    match action {
        Action::CallTool(t) => { ... }
        Action::Delegate(d) => { ... }
        Action::Answer(a)   => return Ok(a),
        Action::Stop        => break,
    }
    state.update(evidence, action)?;
}
```

### 15.3 Policy Module

```rust
pub struct PolicySet {
    pub drtw:                   bool,  // Do Right Then Write
    pub nrtw:                   bool,  // No Rush Then Write
    pub triz_enabled:           bool,
    pub require_evidence:       bool,  // must retrieve before answer
    pub max_turns:              u32,
    pub max_delegations:        u8,
    pub budget_tokens:          u32,
    pub allow_forward_mcp:      bool,
}
```

TRIZ heuristics as planning transforms: contradiction detection, inversion, prior action, segmentation, intermediary.

### 15.4 MCP Forward

```rust
pub struct ForwardRequest {
    pub target:          McpTarget,    // stdio://child:path or http://host
    pub task:            String,
    pub allowed_tools:   Vec<String>,
    pub context_bundle:  ContextBundle,
    pub budget_tokens:   u32,
    pub return_mode:     ReturnMode,   // FinalOnly | FinalWithTrace
}
```

### 15.5 TDD Pattern

```
RED:  test_job_steps_execute_serially
      test_checkpoint_persisted_after_each_step
      test_agent_stops_at_max_turns
      test_agent_stops_at_budget
      test_drtw_requires_retrieve_before_answer
      test_forward_mcp_respects_allowed_tools
GREEN: implement orchestrator + policy
```

### 15.6 Definition of Done

- [ ] Jobs are replayable from checkpoint state (deterministic re-execution)
- [ ] Agent loop provably bounded: max_turns and budget enforced in test
- [ ] `drtw: true` causes test failure if answer produced without evidence retrieval
- [ ] `forward_mcp` calls are auditable: tool name + args logged before execution
- [ ] Policy violations produce structured errors, not panics

---

## §16 Crate: `cli`

**Purpose:** Standalone low-frills daemon entrypoint for the Phase 2 runtime.

```
cargo run -p cli --bin phase2d -- stdio
cargo run -p cli --bin phase2d -- http --addr 127.0.0.1:3000
cargo run -p cli --bin phase2d -- http --addr 127.0.0.1:3000 --watch .
```

Current bootstrap behavior:

```rust
phase2d
  -> builds Phase2RuntimeConfig::new(NeumannConfig::default())
  -> enables TransportForwarder by default
  -> optionally enables WatchRuntimeConfig via --watch
  -> uses DeterministicBackend + provider-test fixture provider as the default low-frills runtime
```

Current verification path:

```rust
crates/cli/tests/stdio.rs
  -> spawn phase2d stdio
  -> issue tools/list over stdin
  -> assert repo.search is present in the MCP registry
```

---

## §17 Cross-Cutting: Standing Data Spine

Every business object MUST have all three projections:

```
┌──────────────────────────────────────────────────────┐
│           Business Object: ColocatedBessSolar        │
│                  NMI: 4XXXXXXX01                     │
├──────────────────┬──────────────────┬────────────────┤
│ Operational      │ Semantic         │ Architectural  │
│ (§5 domain)      │ (§9/§11 graph)   │ (EA repo)      │
│                  │                  │                │
│ ColocatedBessSolar│ rdf:type         │ iServer365     │
│   .nmi           │  nem:BessUnit    │ capability:    │
│   .bess_duid     │ nem:coLocatedWith│  DER_Dispatch  │
│   .solar_duid    │  nem:SolarFarm   │ system:        │
│   .region        │ nem:region       │  SPARTAN        │
│   .bess_mwh      │  nem:NSW1        │ interface:     │
│                  │ ea:hasRecord     │  DISPATCHLOAD  │
│                  │  <iserver365/>   │                │
└──────────────────┴──────────────────┴────────────────┘
         all three views share: git:blob:<hash> URI
```

Any object that exists in one projection MUST be queryable from the others via its canonical URI.

---

## §18 Phases and Milestones

### Phase 1 — Foundation (Weeks 1–4)
**Goal:** Runnable daemon. Parse boundary enforced. Git ledger operational.

| Crate | DoD gate |
|-------|---------|
| `domain` | All `Nmi`/`Duid`/`DuDetail` parse tests green; Arrow schema round-trip |
| `provider-api` | Contract tests pass against `provider-test` |
| `provider-local` | mistral.rs spawns and returns embeddings on CPU |
| `tomllm` | Hint layer never allocated; round-trip test green |
| `git-ledger` | `blob_id` deterministic; `attach_triples` + `read_triples` round-trip |
| `storage-neumann` | `MemoryStore` passes all trait contract tests |
| `mcp-server` | `tools/list` returns registry; incoming stdio JSON-RPC works |
| `cli` | `phase2d stdio` boots and serves `tools/list` without panic |

**Phase 1 DoD:** `examples/aemo-nmi` runs. Single-turn MCP tool call returns structured result.

---

### Phase 2 — Knowledge Layer (Weeks 5–8)
**Goal:** Code graph indexed. Retrieval fusion operational. Ontology queryable.

| Crate | DoD gate |
|-------|---------|
| `codegraph` | Rust symbol graph built from `crates/domain`; call edges present |
| `indexer` | watchexec triggers re-index on file change; unchanged files skipped |
| `retrieval` | Hybrid query returns plausible results for known symbol name |
| `storage-neumann` | `NeumannStore` passes contract tests and ingests ontology Turtle into semantic triples |
| `dsl` | Receipt DSL rule compiles; file matched correctly |
| `mcp-server` | startup ingests ontology resources into Neumann; stdio/HTTP transport parity holds for `tools/list` and `tools/call`; watcher startup is supported |

**Phase 2 DoD:** `examples/code-rag` resolves a symbol and traces call graph via MCP.

---

### Phase 3 — Intake Pipeline (Weeks 9–12)
**Goal:** File intake fully operational. SHACL classify. Named to S3.

| Crate | DoD gate |
|-------|---------|
| `handlers` | Receipt PDF classified as `doc:Receipt` (confidence > 0.90) |
| `classifier` | Unknown file → `Unclassified` → quarantine |
| `naming` | `StoragePlan` includes ontology_class and shape in every S3 tag |
| `intake` | End-to-end: PDF in → S3 object with metadata out |
| `dsl` | Naming rules in `.tomllm` files; zero hardcoded paths in Rust |

**Phase 3 DoD:** `examples/receipt-intake` runs end-to-end against a mock S3.

---

### Phase 4 — Agent Orchestration (Weeks 13–16)
**Goal:** Full agent loop. Delegation. Policy enforcement. Forward-MCP.

| Crate | DoD gate |
|-------|---------|
| `orchestrator` | Job replayable from checkpoint; max_turns enforced |
| `policy` | `drtw: true` fails test when answer precedes evidence |
| `forward-mcp` | Agent delegates to child agent; result returned with trace |
| `mcp-server` | `agent.run`, `agent.delegate`, `agent.forward_mcp` operational |

**Phase 4 DoD:** `examples/forward-mcp` demonstrates cross-agent delegation with audit log.

---

### Phase 5 — Standing Data Spine Integration (Weeks 17–20)
**Goal:** All three projections connected via shared URI. EA repository linkage.

| Deliverable | DoD gate |
|------------|---------|
| Shared URI scheme documented | Every `domain` type has canonical `nem:` URI |
| RDF binding for DUDETAIL classes | `DuDetail` → Turtle triples queryable via `ontology.run_sparql` |
| SHACL shapes for Arrow schema | NmiRecord shape validates Arrow output |
| iServer365 linkage | EA record URIs embedded in ontology as `ea:hasRecord` edges |
| SharePoint replacement | Standing data queryable via SPARQL + git replay |

**Phase 5 DoD:** SPARQL query `SELECT ?duid WHERE { ?duid a nem:BidirectionalUnit }` returns results cross-linked to EA records.

---

## §19 Testing Strategy

### Levels

| Level | Framework | When |
|-------|-----------|------|
| Unit | `#[test]` | Every function with a boundary |
| Contract | Shared trait test suite | Every `impl Trait` |
| Integration | BDD scenarios in `tests/integration/` | Every pipeline |
| Replay | `git checkout + run` | Every phase milestone |
| CI smoke | `provider-test` fixture | Every PR |

### Red/Green Discipline

1. Write failing test first — commit as `test: red - <description>`
2. Implement minimum to pass — commit as `feat: green - <description>`
3. Refactor without changing test outcome — commit as `refactor:`

### Test Data

All multi-row test fixtures in `tests/fixtures/*.parquet` or `*.json`. Never embedded in test code.

### CI Pipeline

```
cargo test --workspace              # all unit + contract tests
cargo test --test integration       # BDD scenarios
cargo run --example aemo-nmi        # smoke
cargo run --example receipt-intake  # smoke
```

---

## §20 Non-Functional Requirements

| Requirement | Target |
|-------------|--------|
| CPU-only inference | Usable at ≥5 tok/s on modern desktop CPU |
| Index idempotency | Re-indexing same blob produces bit-identical graph state |
| Replay determinism | Frozen state at commit X produces identical query results |
| No vendor lock | Swapping `provider-openai` ↔ `provider-local` requires zero orchestrator changes |
| No raw strings downstream | `grep -r 'DUID.*str'` in `orchestrator/` returns zero results |
| Hint layer isolation | Heap allocation profile shows no `#`-prefixed lines from `.tomllm` |
| Agent boundedness | No agent run exceeds `max_turns * max_tokens_per_turn` tokens |

---

## §21 Out of Scope (Phase 1–5)

- Multi-node Neumann consensus (evaluate in Phase 6)
- Distributed queue (local daemon sufficient for Phase 1–4)
- Kubernetes-first deployment (CNCF packaging deferred post-Phase 5)
- SurrealDB (evaluate vs. Neumann in Phase 6)
- Production AEMO MMS live feed (uses MMS snapshot fixtures in Phases 1–5)

---

## §22 Dependency Decisions

| Crate | Rationale |
|-------|-----------|
| `tokio` | async runtime |
| `axum` | HTTP MCP transport |
| `serde` + `serde_json` | wire format |
| `toml` | `.tomllm` execution layer |
| `git2` | git object access, notes |
| `watchexec` | file event supervision |
| `tree-sitter` | multi-language AST |
| `petgraph` | symbol graph storage |
| `oxigraph` | SPARQL / RDF ontology fallback |
| `tantivy` | lexical search |
| `arrow2` | Arrow schema + IPC |
| `parquet2` | Parquet read/write |
| `blake3` | content hashing / dedup |
| `async-trait` | trait objects in async |
| `uuid` | stable IDs |
| `tracing` | structured logging |
| `mistral.rs` | local inference subprocess |
| Neumann | unified runtime knowledge store |

**Do not add:** `openai` SDK crate outside `provider-openai`. `sqlite` or `rusqlite` (libSQL only). Any Python runtime dependency in hot path.
