# Copilot Coding Agent Instructions — `irontology-mcp`

## Repository Purpose

`irontology-mcp` is a **Rust-native MCP (Model Context Protocol) agent runtime** that exposes a semantic knowledge graph over MCP. Every business object has a stable git-anchored URI, a validated Rust type, and a queryable semantic projection — OWL class definitions, SHACL shape constraints, and RDF triples. Agents discover naming conventions, ontology schema, and code structure through MCP tools — not by guessing.

Primary MCP tools exposed:
- `repo.search` — fused vector + graph + lexical retrieval over the indexed codebase
- `repo.read_symbol` — read symbol metadata by stable node ID (`git:blob:{blob_id}:{symbol}`)
- `ontology.list_classes` — list OWL/SHACL classes in the semantic layer
- `ontology.related_resources` — retrieve RDF resources linked to a URI

---

## Workspace Layout

```
irontology-mcp/
├── Cargo.toml                  # workspace root (resolver = "2", edition 2021)
└── crates/
    ├── domain/                 # Canonical Rust EA artifact types (newtypes with parse-at-boundary)
    ├── codegraph/              # tree-sitter AST → petgraph symbol graph; NodeUri = git:blob:{hash}:{symbol}
    ├── indexer/                # watchexec/interval pipeline: blob hash, chunking, embed dispatch
    ├── dsl/                    # DSL grammar, AST, rule compiler, file matcher (.tomllm rules)
    ├── retrieval/              # Fusion retrieval (vector + graph + lexical + ontology)
    ├── storage-neumann/        # KnowledgeStore trait + NeumannStore in-memory impl
    └── mcp-server/             # MCP transport, ToolRegistry, ResourceRegistry
└── ...                         # other workspace members, CI config, etc.

---
Note: `.promptexecution.toml` files are used on a per-directory basis for source metadata, adapters, and polling configuration where present (for example, alongside specific crates or integration directories). There is no required root-level `.promptexecution.toml` in this repository.


## Guiding Principles

Follow these in priority order when making any change:

1. **Parse at the boundary, carry forever** — invalid states must be unrepresentable downstream. Use strong Rust newtypes; never pass raw `String` where a validated type exists.
2. **Git is the bitemporal ledger** — `git:blob:{blob_id}:{symbol}` URIs are stable ontology node IDs anchored to git object hashes (see `crates/codegraph/src/symbol_node.rs`).
3. **One internal contract** — all model/provider paths implement the same trait; no vendor SDK leakage into domain or retrieval crates.
4. **Standing data is the spine** — every business object has operational, semantic, and architectural projections simultaneously.
5. **DSL rules are data** — file processing logic lives in user-authored `.tomllm` DSL rules compiled by `crates/dsl`, not in bespoke Rust code.
6. **DRTW / NRtW** — Do Right Then Write / No Rush Then Write. Write tests before implementations; always verify with `cargo test` before committing.
7. **NRtW / DRY** — Find the library; don't build what exists; fix bugs upstream rather than wrapping them.
8. **TRIZ** — Prefer contradiction resolution over compromise. Use inversion and prior-action patterns before adding complexity.
9. **Agents discover, not guess** — ontology schema and naming conventions are MCP-queryable resources, not implicit conventions embedded in code comments.

---

## Coding Conventions

### Rust
- **Edition:** 2021 (`[workspace.package] edition = "2021"`)
- **Workspace deps:** Use `dep.workspace = true` in `[dependencies]` for crates listed in `[workspace.dependencies]` (`anyhow`, `async-trait`, `serde`, `serde_json`, `tokio`). In `[dev-dependencies]`, declare tokio explicitly — `tokio = { version = "1", features = ["macros", "rt"] }` — following the existing convention in the repo (even though the workspace declaration already includes those features).
- **Error handling:** Use `anyhow::Result` for fallible public APIs. Reserve `thiserror` for domain-specific typed errors in `domain` and `dsl` crates.
- **Async runtime:** `tokio` with `#[tokio::test]` for async tests. Feature flags: `rt` and `macros` minimum.
- **Trait objects:** Prefer `Arc<dyn Trait>` over `Box<dyn Trait>` for shared registries (see `ToolRegistry`, `ResourceRegistry`).
- **Newtype pattern:** All domain identifiers are newtypes with parse-at-boundary constructors. Never accept raw `&str` in domain logic.
- **No `unwrap()` in library code** — use `?` propagation or explicit error mapping.

### Testing
- Tests live in `crates/<name>/tests/` as integration tests or in `src/` as inline unit tests.
- Use descriptive snake_case test module names that reflect behaviour: `unchanged_skip`, `pipeline_behaviour`, `determinism`.
- Test fixtures (parquet, JSON) go in `tests/fixtures/` — never embedded inline in test source.
- BDD-style scenario tests go in `tests/integration/`.
- Run tests with: `cargo test --workspace`

### MCP Tools / Resources
- Every new tool implements the `Tool` trait (`name`, `description`, `input_schema`, `call`).
- Register tools via `ToolRegistry::register` in `ToolRegistry::with_phase2_tools`.
- Resources follow the `Resource { uri, mime_type, body }` struct; Turtle resources are auto-ingested by `McpServerRuntime::start_phase2`.
- Tool and resource URIs must be stable and semantically meaningful (e.g., `ontology://classes`, `ontology://shapes`).

### DSL Rules
- File processing rules are authored in `.tomllm` syntax and compiled by `crates/dsl`.
- Each rule has a `when` clause (conditions on `extension`, `media_type`, `contains_field`) and a `then` clause (actions: `handler`, `extract`, `bucket`, `prefix`).
- The `crates/dsl::RuleMatcher` evaluates compiled rules against `InputFile` structs at indexing time.

---

## Domain Model — Enterprise Architecture Artifacts

The `domain` crate provides strongly typed Rust newtypes for all business and EA artifacts. Agents must use these types rather than raw strings at every crate boundary.

### EA Artifact Taxonomy (current + planned)

```
ArchitecturalArtifact
├── EnterpriseArchitecture
│   ├── ArchimateElement     # ArchiMate 3.x motivation/strategy/business/application/technology layers
│   ├── TogafCapability      # TOGAF ADM capability and architecture building block
│   ├── SysmlBlock           # SysML2 Block Definition Diagram (BDD) element
│   ├── IncoseRequirement    # INCOSE MBSE V-model requirement node
│   └── IserverEntity        # orbus iServer365 repository object (Visio-linked)
├── DataArtifact
│   ├── DataSilo             # bounded storage domain with URI scheme + access policy
│   ├── ColumnSchema         # Apache Arrow schema: field name, data type, nullability
│   ├── SqlStatement         # validated SQL (SELECT / INSERT / DDL) with table ref tracking
│   └── OciImage             # OCI container image reference (registry/repo:tag@digest)
└── InfrastructureArtifact
    ├── KubernetesPod        # k8s Pod spec reference (namespace/name), CNI/CSI annotations
    └── OrgActor             # organizational actor with role, icon URI, permission scope
```

### URI Scheme by Silo

All node IDs in the knowledge graph follow one of the validated URI patterns below. Agents must never construct URIs ad hoc — use the typed constructors:

| Silo | Pattern (regex) | Example |
|------|----------------|---------|
| Code symbol | `^git:blob:[0-9a-f]{40}:[A-Za-z_][A-Za-z0-9_:]+$` | `git:blob:4b825dc642cb6eb9a060e54bf8d69288fbee4904:MyStruct::new` |
| Ontology resource | `^ontology://[a-z_/]+$` | `ontology://classes` |
| EA object | `^ea://[a-z_]+/[A-Za-z0-9_-]+$` | `ea://archimate/AppComponent-42` |
| Data silo | `^silo://[a-z_-]+(/[a-z0-9_/-]*)?$` | `silo://neumann/embeddings` |
| OCI image | `^oci://[a-z0-9._/-]+:[a-z0-9._-]+(@sha256:[0-9a-f]{64})?$` | `oci://ghcr.io/org/img:v1.2.3` |
| K8s resource | `^k8s://[a-z0-9-]+/[a-z]+/[a-z0-9-]+$` | `k8s://default/pod/indexer-abc` |
| Actor icon | `^actor://icons/[A-Za-z][A-Za-z0-9_]+\.svg$` | `actor://icons/DataEngineer.svg` |

URI validation is enforced at parse time — invalid URIs are rejected before entering the knowledge store.

---

## Data Source Notations

The knowledge graph can ingest and reason over multiple data representation formats:

- **Apache Arrow / Parquet** — columnar in-memory format for high-throughput dataframe pipelines. Schema defined as `ColumnSchema` newtypes; used for test fixtures and bulk standing-data loads.
- **SQL** — `SqlStatement` newtype captures table refs at parse time; enables cross-silo dependency tracking (which pipelines read which tables).
- **RDF / Turtle / JSON-LD** — primary semantic layer; OWL classes and SHACL shapes in `crates/mcp-server/src/resources/ontology.rs`; SPARQL queries via `storage-neumann`.
- **DSL `.tomllm` rules** — user-authored file-processing rules compiled by `crates/dsl`; drive handler selection, extraction fields, and storage routing.

---

## Indexer Event Hooks

The indexer pipeline (`crates/indexer/src/pipeline.rs`) fires in this sequence for each file event:

1. **`GitLedger::blob_id(path)`** — compute SHA-1 object hash; skip if store already has this blob.
2. **`RuleMatcher::match_file(intake)`** — evaluate `.tomllm` DSL rules; abort if no rule matches.
3. **`Handler::extract(intake)`** — parse content (tree-sitter for code, text extraction for docs).
4. **`chunk_text(text, 512)`** — split extraction into fixed-size chunks.
5. **`ModelProvider::embed(request)`** — dispatch chunks to embedding model.
6. **`KnowledgeStore::upsert_embeddings(records)`** — persist with `Modality::CodeSymbol` or `Modality::DocChunk`.

Hooks for future extension points: ontology triple attachment via `git notes`, SHACL shape validation before upsert, and Visio diagram relationship extraction.

---

## Computation Patterns

Use these idiomatic patterns consistently:

- **Monadic `Result` chains** — chain fallible steps with `?`; collect with `Iterator::map(…).collect::<Result<Vec<_>>>()?`.
- **Parallel async** — use `tokio::join!` for independent concurrent operations (see fusion retrieval: vector + graph + lexical + ontology run in parallel).
- **Map/reduce over node sets** — `SymbolGraph::nodes()` and `SymbolGraph::edges()` are iterators; apply `filter_map`, `fold`, or `flat_map` directly.
- **Functional trait objects** — `Arc<dyn Trait>` for shared, cloneable provider/store handles; `Box<dyn Trait>` only for owned, single-use values.
- **No imperative mutation across await points** — hold `Arc<Mutex<_>>` or redesign with message-passing if shared mutable state spans async boundaries.

---

## Infrastructure Context

- **OCI containers** — runtime artifacts reference `OciImage` URIs. All container builds must produce reproducible digests.
- **Kubernetes** — `KubernetesPod` nodes in the knowledge graph track workload identity, CNI network policies, and CSI volume claims as semantic edges.
- **Organizational actors** — each `OrgActor` role has a unique SVG icon URI (`actor://icons/{role}.svg`) enabling consistent visualization in diagram renderers and Visio exports. Roles include: `Developer`, `Architect`, `DataEngineer`, `OperationsEngineer`, `DataSteward`, `AiAgent`.

---

## EA Diagram Reasoning (Visio / ArchiMate / SysML2)

The system forms **structured, reasoned opinions** on relationships between objects in diagram sources (Visio `.vsdx`, ArchiMate `.archimate`, SysML2 `.sysml`):

- Diagram objects are ingested as `IserverEntity` or `ArchimateElement` nodes with stable `ea://` URIs.
- Relationships between diagram objects become typed RDF triples in the knowledge store (`ex:realizes`, `ex:influences`, `ex:composes`, `ex:flows`).
- SHACL shapes validate structural constraints on the diagram graph (e.g., every `AppComponent` must `ex:realizes` at least one `BusinessProcess`).
- Agents can query diagram topology via `ontology.related_resources` or SPARQL over `storage-neumann`.

---

## b00t Agentic Orchestration Context

This repository is designed to be queried and operated by AI agents using the **b00t** agentic command language (`elasticdotventures/_b00t_`). Key conventions:

- Before coding in a new language or domain, load relevant skills: e.g., `b00t learn rust` or `b00t grok ontology`.
- Use `b00t grok <topic>` to retrieve situational awareness from the semantic knowledge graph before making changes.
- The MCP server exposed by this repo is a first-class b00t knowledge source; always prefer MCP tool queries over heuristic guessing.
- Operate in an **OODA loop** (Observe → Orient → Decide → Act) and apply **TRIZ** contradiction resolution before introducing new abstractions.
- All agent actions require explicit human approval before execution. Do not bypass user-intervention checkpoints.

---

## Build & Test Quick Reference

```bash
# Build all crates
cargo build --workspace

# Run all tests
cargo test --workspace

# Check without building
cargo check --workspace

# Lint
cargo clippy --workspace -- -D warnings

# Format
cargo fmt --all
```

---

## Security & Safety

- Never commit secrets, credentials, or tokens.
- All agent actions that mutate state (file writes, network calls, external API invocations) require explicit user approval.
- Do not introduce code that bypasses approval or oversight mechanisms.
- Validate all inputs at crate boundaries using the newtype parse-at-boundary pattern.
