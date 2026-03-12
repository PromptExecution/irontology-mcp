# `promptexecution`

Rust MCP runtime for ontology-driven code and document systems.

The project is pivoting to a clearer runtime boundary:

- Rust remains the host, type system, storage layer, and MCP server.
- Rhai becomes the embedded behavior runtime for ontology-defined object extensions.
- Python remains available, but only as a logical executor for interop with LLM/agent tooling, ontology discovery, and curation of Rhai-facing schemas.
- Git remains the immutable semantic ledger.
- Neumann remains the live semantic store for facts, graph edges, and embeddings.

This keeps the binary stable while allowing ontology behavior to evolve from data and scripts rather than Rust recompiles.

## Why This Pivot

The earlier direction mixed three concerns:

- schema validation
- runtime object behavior
- agentic external tool interop

That blurred the boundary between "what an ontology object is" and "how agents or external tools reason about it".

The new stance is:

- validation belongs at the host boundary
- object behavior belongs in a constrained embedded runtime
- Python belongs at the edge for tool interoperability, not at the core of object execution

## Core Model

An ontology extension should be able to add:

- fields
- validation constraints
- derived properties
- enrichment logic
- graph emission rules
- naming/storage logic

without requiring a new Rust binary for each extension.

What it should not do is mutate the host contract itself at runtime. Rust traits stay fixed. Runtime extensions plug into those traits through a stable adapter.

## Architecture

```text
                           +----------------------+
                           |   MCP clients/tools  |
                           +----------+-----------+
                                      |
                         JSON-RPC stdio / HTTP MCP
                                      |
                 +--------------------v--------------------+
                 |              Rust host core             |
                 |-----------------------------------------|
                 | mcp-server                              |
                 | orchestrator                            |
                 | retrieval                               |
                 | intake / handlers / naming / classifier |
                 | provider-api                            |
                 +---------+----------------+--------------+
                           |                |
                           |                |
                +----------v----+     +-----v------------------+
                |  Rhai runtime  |     | Python logical executor|
                |----------------|     |------------------------|
                | ontology hooks |     | agent tool interop     |
                | derived fields |     | discovery pipelines    |
                | routing logic  |     | schema curation        |
                | graph emitters |     | offline workflows      |
                +----------+-----+     +-----------+------------+
                           |                         |
                           +------------+------------+
                                        |
                           +------------v------------+
                           | Validated host adapter  |
                           |-------------------------|
                           | fixed Rust trait surface|
                           | schema boundary         |
                           | audit / limits          |
                           +------------+------------+
                                        |
                +-----------------------+------------------------+
                |                                                |
      +---------v----------+                         +------------v------------+
      | Neumann live store |                         | Git semantic ledger     |
      |--------------------|                         |-------------------------|
      | facts              |                         | blob/tree/commit IDs    |
      | graph edges        |                         | snapshots / replay      |
      | embeddings         |                         | manifests / provenance  |
      +--------------------+                         +-------------------------+
```

## Ontology Runtime Extension Flow

```text
ontology bundle
  |
  +-- ontology metadata
  +-- schema fragment
  +-- Rhai behavior module
  +-- optional Python discovery recipe
  |
  v
validate at host boundary
  |
  +-- JSON Schema / host rules
  +-- SHACL/OWL classification inputs
  +-- allowed hook surface check
  |
  v
compile / load Rhai
  |
  +-- register host functions
  +-- bind object context
  +-- apply execution limits
  |
  v
runtime object behavior
  |
  +-- derive fields
  +-- emit ontology edges
  +-- choose storage/naming plan
  +-- expose MCP-discoverable semantics
  |
  v
persist to Neumann + snapshot to Git
```

## Python's Role After The Pivot

Python is still in scope, but no longer as the core object runtime.

Python workflows are retained for:

- external LLM/agent tool interoperability
- ontology discovery pipelines
- semantic analysis and curation workflows
- generation or refinement of Rhai schema/config artifacts
- batch offline processing that does not need to live inside the host process

Python is not the authority for:

- canonical runtime object behavior
- host-side validation contracts
- core ontology execution inside the daemon

That means the system can still hand work to Python over MCP or other executor boundaries, but a running ontology object inside `phase2d` is still mediated by Rust and Rhai.

## Runtime Contracts

The host contract stays in Rust. Extensions plug into it.

Examples of stable host-side traits already present in the workspace:

- `ModelProvider`
- `KnowledgeStore`
- `FileHandler`
- `Classifier`
- `NamingPolicy`
- `AgentExecutor`
- MCP `Tool` and `Resource` surfaces

The runtime-extensible layer should not attempt to create new Rust trait impls dynamically. Instead, it should provide a stable adapter shape such as:

```text
ontology object data
  -> validate
  -> bind host context
  -> execute Rhai hook
  -> map result back into Rust contract
```

Likely hook categories:

- `validate_object`
- `derive_fields`
- `classify`
- `emit_edges`
- `storage_plan`
- `display_label`
- `mcp_projection`

## Workspace

Current workspace crates:

```text
crates/
  classifier/
  cli/
  codegraph/
  domain/
  dsl/
  forward-mcp/
  handlers/
  indexer/
  intake/
  mcp-server/
  naming/
  orchestrator/
  provider-api/
  provider-local/
  provider-openai/
  provider-test/
  retrieval/
  storage-neumann/
  tomllm/
```

## Current Implemented Spine

The repo already has the core Phase 2 runtime skeleton:

- `mcp-server` with incoming JSON-RPC over stdio and HTTP
- `phase2d` daemon entrypoint in `crates/cli`
- `forward-mcp` transport for stdio and HTTP delegation
- `provider-local` managed local OpenAI-compatible model adapter
- `indexer` watcher runtime with `watchexec`
- `storage-neumann` as the live semantic store
- ontology resources ingested at startup
- DSL parsing and rule-driven intake foundations

Current startup examples:

```bash
cargo run -p cli --bin phase2d -- stdio
cargo run -p cli --bin phase2d -- http --addr 127.0.0.1:3000
cargo run -p cli --bin phase2d -- http --addr 127.0.0.1:3000 --watch .
```

## New Direction For Configuration

The next runtime configuration layer should move from hardcoded startup defaults to a data-driven bundle:

```text
runtime config
  |
  +-- provider config
  +-- store config
  +-- watch roots
  +-- forward-MCP targets
  +-- ontology registry
  +-- schema registry
  +-- Rhai packages/modules
  +-- Python executor targets
```

That bundle should be loadable without rebuilding the binary.

## Recommended Extension Split

```text
Rust host
  - type safety
  - storage
  - transport
  - validation boundary
  - limits / audit / replay

Rhai
  - embedded runtime behavior
  - ontology object hooks
  - dynamic derived fields
  - configurable graph / naming logic

Python
  - discovery and curation workflows
  - external agent interoperability
  - tooling ecosystems not worth rewriting in Rust
```

## Design Principles

1. Rust traits remain the stable host contract.
2. Ontology extensions are data plus constrained script, not ad hoc binary patches.
3. Validation happens before behavior execution.
4. Every runtime extension must be auditable and replayable.
5. Python remains useful, but only across an executor boundary.
6. Git snapshots the semantic state; Neumann serves the live state.
7. MCP exposes discoverability so agents learn the ontology instead of guessing it.

## Roadmap After The Pivot

### 1. Config-driven `phase2d`

Replace the hardcoded daemon bootstrap with a runtime config model for:

- providers
- store backends
- watch roots
- MCP forwarding targets
- ontology registries
- Rhai module locations
- Python executor registrations

### 2. Ontology Extension Runtime

Add a first-class extension layer:

- schema bundle format
- Rhai module loader
- host adapter for validated object hooks
- execution limits and audit traces

### 3. Python Executor Boundary

Formalize Python as a logical executor:

- MCP-forwardable Python workers
- discovery workflows that emit ontology candidates
- curation workflows that refine Rhai-facing schemas
- no direct promotion of Python objects into host runtime contracts

### 4. Discovery Surface

Expose runtime extension state over MCP:

- available ontologies
- available schemas
- registered Rhai modules
- Python executor catalog
- validation status
- version / snapshot provenance

## Non-goals

This pivot is not aiming to:

- make Python the embedded object runtime
- dynamically generate Rust traits
- allow unrestricted scripting inside the daemon
- replace Git as the immutable ledger
- replace Neumann as the live semantic store

## Status

This branch captures the architecture pivot in documentation first.

The codebase already supports the current Phase 2 runtime skeleton. The next implementation phase is to align configuration and ontology extension loading with this model.
