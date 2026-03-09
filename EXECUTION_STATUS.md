# Phase 2 Execution Status

This file tracks execution of `PRD-1.md` (Phase 2: Knowledge Layer) against `README.md` references.

## Implemented Now

- Workspace scaffolded with Phase 2 crates:
  - `codegraph`, `indexer`, `dsl`, `retrieval`, `storage-neumann`, `mcp-server`
- Code graph extraction (Rust + Python) and symbol graph primitives.
- Incremental indexer pipeline with unchanged blob skip.
- Deterministic DSL rule compilation and file matching.
- Fusion retrieval scoring with deterministic ranking test.
- NeumannStore-compatible in-memory production stub with vector query path.
- MCP tools:
  - `repo.search`
  - `repo.read_symbol`
  - `ontology.list_classes`

## Additional Progress (This Iteration)

- SymbolGraph edge deduplication to improve idempotency guarantees.
- Added codegraph idempotency test for same-source extraction stability.
- Added indexer pipeline behavior tests:
  - rule mismatch short-circuit (no handler/provider call)
  - changed file indexing and modality routing
- Added deterministic retrieval backend wiring using modality modules.
- Strengthened MCP tool-registry test to validate tool calls, not only registration.

## Tests Added

- `crates/codegraph/tests/rust_parser.rs`
- `crates/codegraph/tests/idempotency.rs`
- `crates/indexer/tests/unchanged_skip.rs`
- `crates/indexer/tests/pipeline_behaviour.rs`
- `crates/dsl/tests/compiler.rs`
- `crates/retrieval/tests/determinism.rs`
- `crates/retrieval/tests/modalities.rs`
- `crates/storage-neumann/tests/contract.rs`
- `crates/mcp-server/tests/tool_registry.rs`

## Execution Blockers

- Full `cargo test` execution is currently blocked in this environment because MSVC linker `link.exe` is unavailable.
- Dependencies from crates.io were fetched successfully, so network fetch is no longer the blocker.

## Remaining To Reach Full PRD Parity

- Real `watchexec` watcher integration in `indexer`.
- Full tree-sitter extraction breadth (imports/calls/types/tests/docblocks) with richer edge semantics.
- LALRPOP grammar wired as active parser (currently deterministic handwritten parser in `dsl::compiler`).
- Production Neumann client integration (current implementation is in-memory contract-oriented stub).
- End-to-end MCP server runtime wiring (current implementation includes core tools/resources and registry).
