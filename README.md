# `promptexecution`

Rust MCP runtime for enterprise knowledge ingestion, latent ontology discovery, and downstream impact analysis.

The system is not primarily a code-understanding tool. Code is only one source class among many.

The broader goal is to ingest and correlate:

- program code from one or more git repositories
- architecture documents
- project plans
- meeting notes
- presentations
- diagrams and process maps
- database schemas and records
- content from silos such as SharePoint

and turn that material into a queryable semantic substrate that helps agents and humans:

- discover hidden dependencies
- distinguish overloaded terms that mean different things in different domains
- understand downstream impacts of changes
- surface missing or contradictory assumptions
- support architectural and operational decision-making

## Core Thesis

Organizations already contain a latent ontology.

It is scattered across:

- source code
- naming conventions
- schemas
- slide decks
- architecture artifacts
- meeting language
- operational records
- file structures

The same term can mean different things in different areas. The same dependency can appear as:

- a code import
- a business rule in a meeting note
- a process handoff in a diagram
- a column dependency in a database schema
- an ownership boundary in an architecture document

The system should not assume those ideas are already precise. It must extract evidence, preserve provenance, infer candidate semantics, and progressively build a usable ontology of the enterprise.

## What This Runtime Is

The runtime has four jobs:

1. Ingest heterogeneous artifacts from many systems.
2. Convert them into evidence-bearing semantic objects.
3. Correlate those objects across silos to discover shared or conflicting meaning.
4. Expose the resulting knowledge through MCP so agents can explore impacts and dependencies safely.

## What This Runtime Is Not

It is not:

- just a code graph
- just a vector database
- just a document search tool
- just an RDF store
- just an agent shell

It is a semantic runtime that combines all of those as supporting capabilities.

## Architecture

```text
                    +----------------------------------+
                    |      Source systems / silos      |
                    |----------------------------------|
                    | git repos                        |
                    | SharePoint / file stores         |
                    | wiki / docs / notes             |
                    | presentations / PDFs             |
                    | diagrams / process models        |
                    | DB schemas / operational records |
                    +----------------+-----------------+
                                     |
                          connectors / extractors
                                     |
                   +-----------------v------------------+
                   |     Normalized artifact layer      |
                   |------------------------------------|
                   | Artifact                           |
                   | Anchor / span / section            |
                   | metadata / source identity         |
                   | timestamps / provenance            |
                   +-----------------+------------------+
                                     |
                          extraction / interpretation
                                     |
              +----------------------v----------------------+
              |             Evidence and claims             |
              |---------------------------------------------|
              | entities                                    |
              | candidate concepts                          |
              | relations                                   |
              | schema observations                         |
              | process steps                               |
              | confidence + provenance                     |
              +----------------------+----------------------+
                                     |
                    resolution / induction / disambiguation
                                     |
      +------------------------------v-------------------------------+
      |                Enterprise semantic runtime                    |
      |---------------------------------------------------------------|
      | ontology candidates                                            |
      | contextual namespaces                                          |
      | resolved entities                                              |
      | hidden dependency graph                                        |
      | impact paths                                                   |
      | agent-facing discovery surface                                 |
      +---------------------+-------------------+----------------------+
                            |                   |
                            |                   |
              +-------------v----+    +--------v------------------+
              |   Rhai runtime   |    | Python logical executor   |
              |------------------|    |---------------------------|
              | mapping policies |    | connector interop         |
              | enrichment rules |    | document/diagram tooling  |
              | disambiguation   |    | LLM discovery workflows   |
              | impact heuristics|    | schema curation           |
              +-------------+----+    +------------+--------------+
                            |                      |
                            +-----------+----------+
                                        |
                              validated Rust host boundary
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
                                        |
                              +---------v---------+
                              | MCP / decision    |
                              | support surface   |
                              +-------------------+
```

## Semantic Layers

The runtime should separate at least four layers.

### 1. Source Layer

What was actually found.

Examples:

- file in SharePoint
- git blob
- schema DDL
- slide deck
- BPMN diagram
- note page

### 2. Evidence Layer

What was extracted from the source.

Examples:

- a quoted statement
- a table definition
- a process step
- a system name
- a role or owner
- a dependency claim

This layer must preserve provenance and confidence.

### 3. Semantic Layer

What the system believes these artifacts mean.

Examples:

- candidate concepts
- candidate equivalences
- competing definitions
- contextualized "standing data" interpretations
- inferred relationships

This is where ambiguity is modeled, not hidden.

### 4. Impact Layer

What changes imply.

Examples:

- if system A changes, which documents, processes, teams, schemas, and applications are affected
- which concepts depend on a field or policy that is defined only informally
- where terminology drift suggests hidden coupling or decision risk

## Why Context Matters

A term like `standing data` is not globally meaningful on its own.

The system must be able to say:

- `standing data` in market operations
- `standing data` in enterprise architecture
- `standing data` in records or governance

Those may overlap, conflict, or only partially align.

So the runtime must model:

- local meaning
- namespace / business context
- source provenance
- temporal validity
- confidence
- equivalence or non-equivalence with other concepts

The ontology is therefore not just a fixed taxonomy. It is a living, evidence-backed semantic model of the organization.

## Runtime Roles After The Pivot

### Rust Host

Rust remains the authority for:

- host contracts and type safety
- storage and retrieval
- MCP transport and serving
- validation boundaries
- execution limits
- provenance and replay
- orchestration and policy enforcement

### Rhai

Rhai is the embedded runtime for configurable semantic behavior inside the host process.

Its role is to support:

- source-specific mapping rules
- concept disambiguation policies
- enrichment and derived fields
- relation emission
- naming and routing logic
- impact heuristics
- MCP-facing semantic projections

Rhai is not the source of truth for the host contract. It runs behind a fixed Rust adapter.

### Python

Python remains in scope, but as a logical executor rather than the core runtime object system.

Its role is to support:

- interoperability with external LLM/agent ecosystems
- document-specific and diagram-specific tooling
- complex extractors not worth reimplementing in Rust
- ontology discovery workflows
- curation of Rhai-facing schemas and rule bundles
- offline or batch enrichment pipelines

Python should remain outside the critical in-process object runtime boundary.

## Ontology Extension Flow

```text
source artifact
  |
  +-- code / schema / note / slide / diagram / record
  |
  v
extractor pipeline
  |
  +-- native Rust extractor
  +-- Python executor
  +-- MCP-forwarded specialist worker
  |
  v
normalized evidence bundle
  |
  +-- anchors
  +-- observations
  +-- candidate entities
  +-- candidate relations
  +-- confidence + provenance
  |
  v
ontology interpretation
  |
  +-- validate against host schema
  +-- apply Rhai mapping / enrichment
  +-- resolve contextual namespaces
  +-- generate candidate ontology objects
  |
  v
correlation and dependency graph
  |
  +-- resolved entities
  +-- conflicts / overlaps
  +-- hidden dependencies
  +-- impact paths
  |
  v
persist to Neumann + snapshot to Git
  |
  v
serve through MCP for agent exploration
```

## Primary Runtime Abstractions

The system should evolve toward generic enterprise abstractions rather than code-specific ones.

Likely core objects:

- `SourceSystem`
- `Artifact`
- `Anchor`
- `Observation`
- `Claim`
- `Concept`
- `Entity`
- `Relation`
- `ContextNamespace`
- `EvidenceBundle`
- `ImpactPath`
- `DecisionSupportView`

Code symbols, database tables, process steps, and architecture components then become specializations or projections of those more general objects.

## Current Workspace

Current crates:

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
  semantic-runtime/
  storage-neumann/
  tomllm/
```

## Current Implemented Spine

Today the implemented path is still biased toward local repo and code/document ingestion, because that is the most mature slice so far.

Already present:

- [x] `mcp-server` with stdio and HTTP JSON-RPC transport
- [x] `phase2d` daemon entrypoint in `crates/cli`
- [x] `forward-mcp` transport for stdio and HTTP delegation
- [x] `provider-local` for managed local OpenAI-compatible serving
- [x] `indexer` watcher runtime with `watchexec`
- [x] `storage-neumann` as the live semantic store
- [x] ontology resources ingested at startup
- [x] DSL foundations for rule-driven ingestion
- [x] enterprise semantic domain objects such as `Artifact`, `Observation`, `Claim`, `Concept`, `Entity`, `Relation`, `ContextNamespace`, and `EvidenceBundle`
- [x] Rhai-based semantic correlation runtime over evidence bundles
- [x] directory-scoped staging source configs via `.promptexecution.toml`
- [x] staged artifact ingestion with inherited source tags and ontology references
- [x] watcher/indexer bridging that persists source metadata into the semantic store

Current startup examples:

```bash
cargo run -p cli --bin phase2d -- stdio
cargo run -p cli --bin phase2d -- http --addr 127.0.0.1:3000
cargo run -p cli --bin phase2d -- http --addr 127.0.0.1:3000 --watch .
```

## Where The Design Needs To Broaden

The next architectural step is not just "better code indexing".

It is:

- generalized connectors for multiple silos
- an artifact/evidence model that is not code-centric
- contextual ontology induction
- correlation across repositories, documents, schemas, and process artifacts
- decision-support views over discovered dependencies

## Next Direction For Configuration

The hardcoded bootstrap in `phase2d` should be replaced by a runtime bundle that can describe:

- source connectors
- extractor registrations
- Neumann/store config
- watch roots and polling scopes
- ontology registries
- schema registries
- Rhai modules and packages
- Python executor registrations
- MCP forward targets
- impact-view projections

The bundle should be loadable without recompiling the binary.

## Decision Support Surface

The system should ultimately help answer questions like:

- What else changes if this schema field changes?
- Which documents and processes depend on this concept, even if they use different words?
- Where do two teams mean different things by the same term?
- Which decisions rely on informal or weakly evidenced assumptions?
- Which systems are coupled only through undocumented process or data dependencies?

That is the actual downstream value of the runtime.

## Design Principles

1. Preserve provenance. Never lose the trail back to the source artifact.
2. Model ambiguity explicitly. Do not force early false precision.
3. Separate evidence from interpretation.
4. Keep the Rust host contract stable.
5. Use Rhai for embedded semantic behavior, not host contract mutation.
6. Keep Python at an executor boundary for interop and discovery.
7. Keep Git as the immutable semantic ledger and Neumann as the live semantic store.
8. Expose ontology and impact discovery through MCP so agents learn instead of guessing.

## Non-goals

This runtime is not trying to:

- reduce the enterprise ontology to code structure alone
- assume every source can be made semantically precise immediately
- make Python the in-process ontology runtime
- let scripts bypass host validation
- replace human curation where ambiguity is real

## Status

This branch captures the documentation pivot first.

The implementation already has a working Phase 2 runtime skeleton. The next real work is to widen that skeleton from "repo/code intelligence" into "enterprise artifact ingestion and latent ontology discovery".
