# Copilot Coding Agent Instructions — `irontology-mcp`

## Repository Purpose

`irontology-mcp` is a **Rust-native MCP (Model Context Protocol) agent runtime** that exposes a semantic knowledge graph over MCP. Every business object has a stable git-anchored URI, a validated Rust type, and a queryable RDF projection. Agents discover naming conventions, ontology schema, and code structure through MCP tools — not by guessing.

Primary MCP tools exposed:
- `repo.search` — fused vector + graph + lexical retrieval over the indexed codebase
- `repo.read_symbol` — resolve a specific symbol by FQN
- `ontology.list_classes` — list OWL/SHACL classes in the semantic layer
- `ontology.related_resources` — retrieve RDF resources linked to a URI

---

## Workspace Layout

```
irontology-mcp/
├── Cargo.toml                  # workspace root (resolver = "2", edition 2021)
├── crates/
│   ├── domain/                 # Canonical Rust business types (NMI, DUID, AssetClass…)
│   ├── codegraph/              # tree-sitter AST → petgraph symbol graph
│   ├── indexer/                # watchexec/interval pipeline: blob hash, chunking, embed dispatch
│   ├── dsl/                    # DSL grammar, AST, rule compiler, file matcher
│   ├── retrieval/              # Fusion retrieval (vector + graph + lexical + ontology)
│   ├── storage-neumann/        # KnowledgeStore trait + NeumannStore in-memory impl
│   └── mcp-server/             # MCP transport, ToolRegistry, ResourceRegistry
└── .promptexecution.toml       # (per-directory) source metadata, adapters, poll config
```

---

## Guiding Principles

Follow these in priority order when making any change:

1. **Parse at the boundary, carry forever** — invalid states must be unrepresentable downstream. Use strong Rust newtypes; never pass raw `String` where a validated type exists.
2. **Git is the bitemporal ledger** — `blob:<hash>` and `tree:<hash>` are stable ontology node IDs.
3. **One internal contract** — all model/provider paths implement the same trait; no vendor SDK leakage into domain or retrieval crates.
4. **Standing data is the spine** — every business object has operational, semantic, and architectural projections simultaneously.
5. **DSL rules are data** — file processing logic lives in user-authored `.promptexecution.toml` + compiled DSL, not in bespoke Rust code.
6. **DRTW / NRtW** — Do Right Then Write / No Rush Then Write. Write tests before implementations; always verify with `cargo test` before committing.
7. **NRtW / DRY** — Find the library; don't build what exists; fix bugs upstream rather than wrapping them.
8. **TRIZ** — Prefer contradiction resolution over compromise. Use inversion and prior-action patterns before adding complexity.
9. **Agents discover, not guess** — ontology schema and naming conventions are MCP-queryable resources, not implicit conventions embedded in code comments.

---

## Coding Conventions

### Rust
- **Edition:** 2021 (`[workspace.package] edition = "2021"`)
- **Workspace deps:** Always use `dep.workspace = true` for crates listed in `[workspace.dependencies]` (currently: `anyhow`, `async-trait`, `serde`, `serde_json`, `tokio`).
- **Error handling:** Use `anyhow::Result` for fallible public APIs. Reserve `thiserror` for domain-specific typed errors in `domain` and `dsl` crates.
- **Async runtime:** `tokio` with `#[tokio::test]` for async tests. Feature flags: `rt` and `macros` minimum.
- **Trait objects:** Prefer `Arc<dyn Trait>` over `Box<dyn Trait>` for shared registries (see `ToolRegistry`, `ResourceRegistry`).
- **Newtype pattern:** All business identifiers (NMI, DUID, RegionId…) are newtypes with parse-at-boundary constructors. Never accept raw `&str` in domain logic.
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
- Tool and resource URIs must be stable and semantically meaningful (e.g., `ontology://phase2/owl`).

### DSL / Intake Configuration
- Per-directory source metadata is configured in `.promptexecution.toml` files (consumed by `crates/indexer`).
- The `[poll]` section controls watch vs. interval mode; `[adapters]` maps file types to handlers.

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
