
  Overview

  Build the knowledge layer with incremental code indexing, symbol graph construction, fusion retrieval (vector + graph + lexical + ontology), and
  DSL-driven rule engine for file processing.

  Target Timeline: Weeks 5-8 (Phase 2)
  Status: Not Started
  Depends On: Phase 1 Foundation (#1)

  Problem Statement / Motivation

  Agents need semantic understanding of codebases to answer questions, trace call graphs, and discover implementation patterns. Current approaches
  suffer from:

  - Context Loss: Symbol relationships not preserved across file boundaries
  - Stale Indexes: Manual re-indexing required after code changes
  - Single-Modality Search: Vector-only search misses structural relationships
  - Hardcoded Rules: File processing logic embedded in code rather than user-configurable

  This phase delivers queryable knowledge graphs where agents discover patterns via MCP resources, not heuristics.

  Proposed Solution

  Implement 6 knowledge-layer crates following TDD workflow:

  1. codegraph - tree-sitter AST → petgraph symbol graph
  2. indexer - watchexec-driven incremental re-indexing pipeline
  3. retrieval - Fusion retrieval across vector/graph/lexical/ontology
  4. storage-neumann - NeumannStore implementation (production storage)
  5. dsl - DSL grammar, AST, compiler, rule store
  6. mcp-server - Add repo.search, ontology.list_classes tools

  Technical Approach

  Architecture

  ┌─────────────────────────────────────────────────────┐
  │            watchexec file events                     │
  └─────────────────────┬───────────────────────────────┘
                        │
           ┌────────────▼────────────┐
           │  git-ledger: blob_id()  │
           │  (skip if unchanged)    │
           └────────────┬────────────┘
                        │
           ┌────────────▼────────────┐
           │  Rule Matcher (DSL)     │
           │  Select handler+pipeline │
           └────────────┬────────────┘
                        │
           ┌────────────▼────────────┐
           │  Handler.extract()      │
           │  (tree-sitter parsing)  │
           └────────────┬────────────┘
                        │
           ┌────────────▼────────────┐
           │  codegraph.update()     │
           │  (symbol graph delta)   │
           └────────────┬────────────┘
                        │
           ┌────────────▼────────────┐
           │  embed_pipeline         │
           │  chunk → embed → record │
           └────────────┬────────────┘
                        │
           ┌────────────▼────────────┐
           │  storage-neumann        │
           │  upsert facts/edges/emb │
           └────────────┬────────────┘
                        │
           ┌────────────▼────────────┐
           │  git-ledger             │
           │  attach_triples (notes) │
           └─────────────────────────┘

  Implementation Phases

  Phase 2.1: Code Graph (Week 5)

  Files to Create:
  - crates/codegraph/src/lib.rs
  - crates/codegraph/src/symbol_node.rs - SymbolNode, SymbolKind enums
  - crates/codegraph/src/graph.rs - SymbolGraph wrapper around petgraph
  - crates/codegraph/src/parsers/rust.rs - tree-sitter Rust parser
  - crates/codegraph/src/parsers/python.rs - tree-sitter Python parser
  - crates/codegraph/src/extractors/mod.rs - Symbol extraction logic
  - crates/codegraph/tests/rust_parser.rs - RED tests for Rust symbols

  Symbol Graph Structure:
  use petgraph::Graph;

  pub struct SymbolGraph(Graph<SymbolNode, EdgeKind>);

  pub struct SymbolNode {
      pub id: NodeUri,          // git:blob:hash:SymbolName
      pub kind: SymbolKind,     // Function | Type | Module | Test | Doc
      pub doctext: Option<String>,
      pub span: Span,
      pub signature: Option<String>,
  }

  pub enum EdgeKind {
      Calls,
      Imports,
      Defines,
      Tests,
      Implements,
      DependsOn,
  }

  Tree-sitter Query Example:
  // Extract Rust functions
  let query = r#"
      (function_item
        name: (identifier) @fn.name
        parameters: (parameters) @fn.params
        body: (block) @fn.body)
  "#;

  let parsed = parser.parse(source_code, None)?;
  let mut cursor = QueryCursor::new();

  for match_ in cursor.matches(&query, parsed.root_node(), source_code.as_bytes()) {
      let fn_name = extract_text(&match_, "fn.name", source_code);
      let node = SymbolNode {
          id: NodeUri::new(&blob_id, fn_name),
          kind: SymbolKind::Function,
          // ... other fields
      };
      graph.add_node(node);
  }

  Phase 2.2: Incremental Indexer (Week 5-6)

  Files to Create:
  - crates/indexer/src/lib.rs
  - crates/indexer/src/watcher.rs - watchexec integration
  - crates/indexer/src/pipeline.rs - Index pipeline orchestration
  - crates/indexer/src/chunking.rs - Text chunking strategies
  - crates/indexer/src/embedding.rs - Embedding routing by modality
  - crates/indexer/tests/unchanged_skip.rs - Skip unchanged files test

  Indexing Pipeline:
  pub async fn index_file(
      path: &Path,
      git_ledger: &GitLedger,
      rules: &RuleMatcher,
      store: &dyn KnowledgeStore,
      provider: &dyn ModelProvider,
  ) -> Result<()> {
      // 1. Compute blob_id
      let blob_id = git_ledger.blob_id(path)?;

      // 2. Check if already indexed (compare with stored hash)
      if store.has_blob(blob_id).await? {
          return Ok(()); // Skip unchanged
      }

      // 3. Match DSL rules
      let matched_rules = rules.match_file(&IntakeFile::from_path(path)?);
      let handler = select_handler(&matched_rules)?;

      // 4. Extract (AST parsing)
      let extraction = handler.extract(&file).await?;

      // 5. Update code graph
      if extraction.symbols.is_some() {
          let symbols = extraction.symbols.unwrap();
          codegraph.update_symbols(blob_id, symbols)?;
      }

      // 6. Chunk and embed
      let chunks = chunker.chunk(&extraction.text)?;
      let embeddings = provider.embed(EmbedRequest {
          inputs: chunks.clone(),
          ..Default::default()
      }).await?;

      // 7. Route to appropriate sinks
      for (chunk, embedding) in chunks.iter().zip(embeddings.vectors) {
          let record = EmbeddingRecord {
              id: Uuid::new_v4(),
              vector: embedding,
              modality: determine_modality(&extraction),
              source_blob: blob_id,
              // ... other fields
          };

          match record.modality {
              Modality::CodeSymbol => {
                  tokio::join!(
                      store.upsert_embeddings(vec![record.clone()]),
                      codegraph.store_embedding(record)
                  );
              }
              Modality::DocChunk => {
                  tokio::join!(
                      store.upsert_embeddings(vec![record.clone()]),
                      lexical_index.add(record)
                  );
              }
              _ => {}
          }
      }

      // 8. Attach triples to git notes
      let triples = extraction_to_triples(&extraction, blob_id)?;
      git_ledger.attach_triples(blob_id, triples)?;

      Ok(())
  }

  Phase 2.3: DSL Compiler (Week 6)

  Files to Create:
  - crates/dsl/src/lib.rs
  - crates/dsl/src/grammar.lalrpop - LALRPOP grammar definition
  - crates/dsl/src/ast.rs - AST node types
  - crates/dsl/src/compiler.rs - LLM-assisted prompt → DSL
  - crates/dsl/src/matcher.rs - Rule matching engine
  - crates/dsl/rules/rust.tomllm - Built-in Rust rule
  - crates/dsl/tests/compiler.rs - Deterministic compilation tests

  DSL Grammar (LALRPOP):
  // crates/dsl/src/grammar.lalrpop
  use crate::ast::*;

  grammar;

  pub Rule: Rule = {
      "rule" <name:Id> <when_clause:WhenClause> <then_clause:ThenClause> => {
          Rule { name, when_clause, then_clause }
      }
  };

  WhenClause: WhenClause = {
      "when" <conditions:Condition+> => WhenClause { conditions }
  };

  Condition: Condition = {
      "extension" "==" <ext:String> => Condition::Extension(ext),
      "media_type" "==" <mime:String> => Condition::MediaType(mime),
      "contains_field" "(" <field:String> ")" => Condition::ContainsField(field),
      <lhs:Condition> "and" <rhs:Condition> => {
          Condition::And(Box::new(lhs), Box::new(rhs))
      },
  };

  ThenClause: ThenClause = {
      "then" <actions:Action+> => ThenClause { actions }
  };

  Action: Action = {
      "handler" "=" <h:String> => Action::Handler(h),
      "extract" "=" "[" <fields:Comma<String>> "]" => Action::Extract(fields),
      "bucket" "=" <b:String> => Action::Bucket(b),
      "prefix" "=" <p:String> => Action::Prefix(p),
  };

  Built-in Rule Example:
  # crates/dsl/rules/rust.tomllm
  # This comment is a hint - never allocated, never parsed
  # Expected handler: tree_sitter_rust
  # Expected output: symbols, call graph, imports

  rule rust_code
    when
      extension == ".rs"
      and media_type == "text/plain"
    then
      handler = tree_sitter_rust
      extract = [symbols, calls, imports]
      embed = [functions, docblocks]
      ontology = rust_code_graph
      bucket = "{repo_slug}/code"
      prefix = "{module_path}/"
      filename = "{blob_hash}_{symbol}.rs"

  Phase 2.4: Fusion Retrieval (Week 7)

  Files to Create:
  - crates/retrieval/src/lib.rs
  - crates/retrieval/src/fusion.rs - Fusion scoring algorithm
  - crates/retrieval/src/vector.rs - Vector similarity search
  - crates/retrieval/src/graph.rs - Graph neighborhood expansion
  - crates/retrieval/src/lexical.rs - BM25 lexical search
  - crates/retrieval/src/ontology.rs - SPARQL ontology queries
  - crates/retrieval/tests/determinism.rs - Score determinism tests

  Fusion Algorithm:
  pub struct FusionWeights {
      pub vector: f32,   // default 0.35
      pub graph: f32,    // default 0.30
      pub lexical: f32,  // default 0.20
      pub ontology: f32, // default 0.15
  }

  pub async fn fusion_search(
      query: &str,
      top_k: usize,
      weights: FusionWeights,
      store: &dyn KnowledgeStore,
      provider: &dyn ModelProvider,
  ) -> Result<Vec<SearchResult>> {
      // 1. Generate query embedding
      let query_emb = provider.embed(EmbedRequest {
          inputs: vec![query.to_string()],
          ..Default::default()
      }).await?.vectors[0].clone();

      // 2. Parallel search across modalities
      let (vector_results, graph_results, lexical_results, ontology_results) = tokio::join!(
          vector_search(&query_emb, top_k * 3, store),
          graph_search(query, top_k * 3, store),
          lexical_search(query, top_k * 3, store),
          ontology_search(query, top_k * 3, store),
      );

      // 3. Fuse scores
      let mut combined: HashMap<NodeUri, f32> = HashMap::new();

      for result in vector_results? {
          *combined.entry(result.id).or_insert(0.0) += result.score * weights.vector;
      }
      for result in graph_results? {
          *combined.entry(result.id).or_insert(0.0) += result.score * weights.graph;
      }
      for result in lexical_results? {
          *combined.entry(result.id).or_insert(0.0) += result.score * weights.lexical;
      }
      for result in ontology_results? {
          *combined.entry(result.id).or_insert(0.0) += result.score * weights.ontology;
      }

      // 4. Sort and return top-k
      let mut results: Vec<_> = combined.into_iter()
          .map(|(id, score)| SearchResult { id, score })
          .collect();
      results.sort_by(|a, b| b.score.partial_cmp(&a.score).unwrap());
      results.truncate(top_k);

      Ok(results)
  }

  Phase 2.5: Neumann Store Production (Week 7-8)

  Files to Create:
  - crates/storage-neumann/src/neumann.rs - NeumannStore implementation
  - crates/storage-neumann/src/config.rs - Connection configuration
  - crates/storage-neumann/tests/contract.rs - Same tests as MemoryStore

  NeumannStore Implementation:
  pub struct NeumannStore {
      config: NeumannConfig,
      // Internal Neumann client
  }

  #[async_trait]
  impl KnowledgeStore for NeumannStore {
      async fn upsert_file(&self, file: FileRecord) -> Result<()> {
          // Neumann upsert logic
      }

      async fn upsert_facts(&self, facts: Vec<FactRecord>) -> Result<()> {
          // Batch insert to Neumann
      }

      async fn upsert_edges(&self, edges: Vec<EdgeRecord>) -> Result<()> {
          // Graph edge insertion
      }

      async fn upsert_embeddings(&self, e: Vec<EmbeddingRecord>) -> Result<()> {
          // Vector storage in Neumann
      }

      async fn query(&self, q: SemanticQuery) -> Result<QueryResult> {
          match q {
              SemanticQuery::Vector { embedding, top_k, filter } => {
                  // Vector similarity search in Neumann
              }
              SemanticQuery::Graph { sparql } => {
                  // SPARQL query translation
              }
              SemanticQuery::Hybrid { .. } => {
                  // Fusion query
              }
              _ => todo!(),
          }
      }
  }

  Phase 2.6: MCP Tools Enhancement (Week 8)

  Files to Create:
  - crates/mcp-server/src/tools/repo_search.rs - Fusion search MCP tool
  - crates/mcp-server/src/tools/repo_read_symbol.rs - Symbol lookup
  - crates/mcp-server/src/resources/ontology.rs - Ontology resources
  - crates/mcp-server/tests/tool_registry.rs - Tool discovery tests

  MCP Tool: repo.search
  #[derive(Debug)]
  struct RepoSearchTool {
      retrieval: Arc<FusionRetrieval>,
  }

  #[async_trait]
  impl Tool for RepoSearchTool {
      fn name(&self) -> &str { "repo.search" }

      fn description(&self) -> &str {
          "Search code repository with fusion retrieval (vector+graph+lexical+ontology)"
      }

      fn input_schema(&self) -> serde_json::Value {
          json!({
              "type": "object",
              "properties": {
                  "query": {
                      "type": "string",
                      "description": "Natural language or code search query"
                  },
                  "top_k": {
                      "type": "integer",
                      "default": 10,
                      "description": "Number of results to return"
                  },
                  "expand": {
                      "type": "boolean",
                      "default": false,
                      "description": "Expand graph neighborhood"
                  }
              },
              "required": ["query"]
          })
      }

      async fn call(&self, params: serde_json::Value) -> Result<serde_json::Value> {
          let query: String = params["query"].as_str().unwrap().to_string();
          let top_k = params.get("top_k").and_then(|v| v.as_u64()).unwrap_or(10) as usize;

          let results = self.retrieval.search(&query, top_k).await?;

          Ok(json!({
              "results": results.iter().map(|r| json!({
                  "id": r.id.to_string(),
                  "score": r.score,
                  "content": r.content,
                  "location": r.location,
                  "symbol_kind": r.symbol_kind,
              })).collect::<Vec<_>>()
          }))
      }
  }

  Technical Considerations

  Incremental Indexing Performance

  - Blob Hash Comparison: O(1) check prevents redundant re-indexing
  - Delta Updates: Only modified files trigger graph updates
  - Parallel Embedding: Fan-out embeddings to multiple sinks concurrently
  - Expected Throughput: 1000+ files/minute on modern hardware

  Graph Storage Tradeoffs

  - Petgraph In-Memory: Fast for <100K symbols, requires serialization
  - Neumann Persistent: Scales to millions of symbols, slightly slower queries
  - Hybrid Approach: Hot symbols in petgraph, cold storage in Neumann

  Embedding Modalities

  pub enum Modality {
      CodeSymbol,      // Functions, types → graph + vector
      DocChunk,        // Documentation → vector + lexical
      OntologyNode,    // RDF triples → graph only
      TestCase,        // Tests → graph (traces to code)
  }

  DSL Safety

  - Sandboxed Execution: Rules cannot execute arbitrary code
  - Type-Safe AST: LALRPOP generates compile-time checked parsers
  - Validation: All rules validated before storage

  Acceptance Criteria

  Functional Requirements

  - Code Graph: Rust symbol graph built from crates/domain; call edges present
  - Incremental Indexing: watchexec triggers re-index on file change; unchanged files skipped (verified via blob hash)
  - Tree-sitter: Function definitions, imports, and call sites extracted correctly
  - DSL Compilation: Receipt DSL rule compiles; file matched correctly with deterministic output
  - Fusion Retrieval: Hybrid query returns plausible results for known symbol name (deterministic scoring)
  - Neumann Store: NeumannStore passes all MemoryStore contract tests
  - MCP Tools: repo.search returns ranked results; ontology.list_classes works

  Quality Gates

  - Index Idempotency: Re-indexing same file produces identical graph state (bit-identical blob hashes)
  - Embedding Fan-out: Verified each modality reaches correct sinks (vector/graph/lexical)
  - No Raw Files in Store: Only blob IDs, hashes, and extracted facts stored (never raw file bytes)
  - Symbols as Primary Unit: Embeddings generated per-symbol, not per-file
  - Deterministic Fusion: Identical inputs produce identical ranking (CI-safe)
  - DSL Rules in Files: All processing logic in .tomllm files, zero hardcoded paths in Rust

  Performance Targets

  - Index Speed: ≥500 files/minute on 4-core CPU
  - Query Latency: Fusion search <500ms for top-10 results
  - Graph Traversal: BFS to depth 3 completes in <100ms for 10K-node graph

  Success Metrics

  - ✅ examples/code-rag resolves a symbol and traces call graph via MCP
  - ✅ Unchanged file skip verified: re-indexing identical file takes <1ms
  - ✅ Fusion retrieval ranks known symbols in top-5 for semantic queries
  - ✅ DSL compiler produces deterministic output (same prompt → same AST)

  Dependencies & Risks

  Prerequisites

  - Phase 1 Foundation completed (#1)
  - Tree-sitter grammars installed (Rust, Python, TS, Go)
  - Neumann database access configured

  Risks

  - Tree-sitter Grammar Versioning: Grammar updates may break parsing
    - Mitigation: Pin grammar versions, comprehensive regression tests
  - Embedding Cost: Large codebases expensive to embed initially
    - Mitigation: Incremental embedding, cache embeddings, use local models
  - Fusion Weight Tuning: Default weights may not suit all repositories
    - Mitigation: Make weights configurable, provide auto-tuning tool
  - Neumann Availability: Production store may have downtime
    - Mitigation: Graceful degradation to MemoryStore, queue failed writes

  References & Research

  Internal References

  - Knowledge Layer Architecture: /mnt/d/onedrive/tbd/README.md §10
  - Retrieval Fusion: /mnt/d/onedrive/tbd/README.md §12
  - DSL Design: /mnt/d/onedrive/tbd/README.md §8
  - Phase 2 DoD: /mnt/d/onedrive/tbd/README.md §18 (Weeks 5-8)

  External References

  - https://tree-sitter.github.io/tree-sitter/
  - https://docs.rs/petgraph/
  - https://lalrpop.github.io/lalrpop/
  - https://www.pinecone.io/learn/hybrid-search/
  - https://github.com/watchexec/watchexec

  Related Work

  - See framework-docs-researcher report for tree-sitter, petgraph, and embedding routing patterns

  ---
  Issue Type: 🎯 Enhancement
  Labels: phase-2, knowledge-layer, code-graph, retrieval, dsl, high-priority
  Assignees: TBD
  Milestone: Phase 2 - Knowledge Layer
  Depends On: #1 (Phase 1 Foundation)


  Overview

  Build end-to-end file intake pipeline with automatic classification, SHACL shape matching, OWL ontology inference, and DSL-driven naming policies
  for S3 storage.

  Target Timeline: Weeks 9-12 (Phase 3)
  Status: Not Started
  Depends On: Phase 2 Knowledge Layer (#2)

  Problem Statement / Motivation

  Organizations accumulate unstructured files (receipts, invoices, contracts, reports) with inconsistent naming and scattered storage. Current
  approaches suffer from:

  - Guesswork Naming: File paths assigned by heuristics rather than ontology-driven policies
  - Lost Metadata: Document type, vendor, date not captured in structured form
  - Manual Classification: Humans categorize documents, expensive and error-prone
  - No Provenance: Cannot trace why file was placed at specific path

  This phase delivers semantic intake where every file is classified via SHACL shapes, named via DSL rules, and tagged with ontology metadata.

  Proposed Solution

  Implement 5 intake-layer crates following TDD workflow:

  1. handlers - FileHandler trait + PDF/image/docx/csv implementations
  2. classifier - SHACL shape matching + OWL class inference
  3. naming - NamingPolicy trait + DSL-driven StoragePlan generation
  4. intake - End-to-end orchestration pipeline
  5. mcp-server - Add intake.submit, intake.status tools

  Technical Approach

  Architecture

  ┌─────────────────────────────────────────────────────┐
  │              File Upload/Drop                        │
  └─────────────────────┬───────────────────────────────┘
                        │
           ┌────────────▼────────────┐
           │  intake.submit (MCP)    │
           │  → IntakeFile           │
           └────────────┬────────────┘
                        │
           ┌────────────▼────────────┐
           │  Handler Selection      │
           │  (score-based routing)  │
           └────────────┬────────────┘
                        │
           ┌────────────▼────────────┐
           │  Handler.extract()      │
           │  → Extraction           │
           └────────────┬────────────┘
                        │
           ┌────────────▼────────────┐
           │  Classifier.classify()  │
           │  (SHACL + OWL)          │
           │  → ClassMatch[]         │
           └────────────┬────────────┘
                        │
           ┌────────────▼────────────┐
           │  NamingPolicy.derive()  │
           │  (DSL rules)            │
           │  → StoragePlan          │
           └────────────┬────────────┘
                        │
           ┌────────────▼────────────┐
           │  S3 Upload + Tagging    │
           │  (bucket/prefix/tags)   │
           └────────────┬────────────┘
                        │
           ┌────────────▼────────────┐
           │  Knowledge Store Update │
           │  (provenance tracking)  │
           └─────────────────────────┘

  Implementation Phases

  Phase 3.1: File Handlers (Week 9)

  Files to Create:
  - crates/handlers/src/lib.rs
  - crates/handlers/src/trait.rs - FileHandler trait
  - crates/handlers/src/pdf_text.rs - Born-digital PDF handler
  - crates/handlers/src/pdf_scan.rs - Scanned PDF → OCR handler
  - crates/handlers/src/image_doc.rs - Image layout analysis
  - crates/handlers/src/docx.rs - DOCX extraction
  - crates/handlers/src/csv.rs - CSV tabular extraction
  - crates/handlers/tests/receipt_pdf.rs - Receipt classification test

  FileHandler Trait:
  pub struct IntakeFile {
      pub sha256: [u8; 32],
      pub bytes: Bytes,
      pub path_hint: Option<String>,
      pub media_type: Option<String>,
  }

  #[async_trait]
  pub trait FileHandler: Send + Sync {
      /// Score 0.0-1.0, not boolean (allows handler competition)
      fn score(&self, file: &IntakeFile) -> HandlerScore;

      /// Extract structured data from file
      async fn extract(&self, file: &IntakeFile) -> Result<Extraction>;
  }

  pub struct Extraction {
      pub detected_kind: String,
      pub text: Option<String>,
      pub fields: BTreeMap<String, Value>,
      pub dates: Vec<TemporalValue>,
      pub amounts: Vec<MoneyValue>,
      pub entities: Vec<Entity>,
  }

  pub struct Entity {
      pub kind: EntityKind,  // Person | Organization | Location
      pub name: String,
      pub confidence: f32,
  }

  Handler Selection (Score-Based):
  pub fn select_handler(
      file: &IntakeFile,
      handlers: &[Box<dyn FileHandler>],
  ) -> Result<&dyn FileHandler> {
      let mut scored: Vec<_> = handlers
          .iter()
          .map(|h| (h.as_ref(), h.score(file)))
          .collect();

      scored.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap());

      let (best_handler, best_score) = scored[0];

      if best_score < 0.3 {
          return Ok(&GenericBinaryHandler); // Fallback
      }

      Ok(best_handler)
  }

  PDF Text Handler Example:
  pub struct PdfTextHandler;

  impl FileHandler for PdfTextHandler {
      fn score(&self, file: &IntakeFile) -> f32 {
          if file.media_type != Some("application/pdf") {
              return 0.0;
          }

          // Check if born-digital (has text layer)
          match pdf_has_text_layer(&file.bytes) {
              Ok(true) => 0.95,
              Ok(false) => 0.1, // Low score, let pdf_scan handler take it
              Err(_) => 0.0,
          }
      }

      async fn extract(&self, file: &IntakeFile) -> Result<Extraction> {
          let pdf = PdfDocument::load(&file.bytes)?;

          let mut text = String::new();
          let mut dates = Vec::new();
          let mut amounts = Vec::new();

          for page in pdf.pages() {
              text.push_str(&page.extract_text()?);

              // Extract dates with regex
              for date_match in DATE_REGEX.captures_iter(&text) {
                  dates.push(parse_date(&date_match[0])?);
              }

              // Extract monetary amounts
              for amount_match in MONEY_REGEX.captures_iter(&text) {
                  amounts.push(parse_money(&amount_match[0])?);
              }
          }

          Ok(Extraction {
              detected_kind: "document/pdf/text".to_string(),
              text: Some(text),
              fields: BTreeMap::new(),
              dates,
              amounts,
              entities: Vec::new(), // TODO: NER
          })
      }
  }

  Phase 3.2: SHACL/OWL Classifier (Week 9-10)

  Files to Create:
  - crates/classifier/src/lib.rs
  - crates/classifier/src/trait.rs - Classifier trait
  - crates/classifier/src/shacl.rs - SHACL shape matcher
  - crates/classifier/src/owl.rs - OWL class inference
  - crates/classifier/src/shapes/receipt.ttl - Receipt SHACL shape
  - crates/classifier/src/ontology.ttl - Document ontology (OWL)
  - crates/classifier/tests/receipt_classification.rs - >0.90 confidence test

  SHACL Shape for Receipt:
  @prefix sh: <http://www.w3.org/ns/shacl#> .
  @prefix doc: <http://example.org/doc#> .
  @prefix xsd: <http://www.w3.org/2001/XMLSchema#> .

  doc:ReceiptShape a sh:NodeShape ;
      sh:targetClass doc:Receipt ;
      sh:property [
          sh:path doc:vendor ;
          sh:datatype xsd:string ;
          sh:minCount 1 ;
          sh:message "Receipt must have vendor name" ;
      ] ;
      sh:property [
          sh:path doc:totalAmount ;
          sh:datatype doc:MoneyAmount ;
          sh:minCount 1 ;
          sh:message "Receipt must have total amount" ;
      ] ;
      sh:property [
          sh:path doc:date ;
          sh:datatype xsd:date ;
          sh:minCount 1 ;
          sh:message "Receipt must have transaction date" ;
      ] ;
      sh:property [
          sh:path doc:currency ;
          sh:in ( "USD" "EUR" "GBP" "AUD" ) ;
          sh:minCount 1 ;
      ] .

  OWL Ontology for Documents:
  @prefix owl: <http://www.w3.org/2002/07/owl#> .
  @prefix rdf: <http://www.w3.org/1999/02/22-rdf-syntax-ns#> .
  @prefix rdfs: <http://www.w3.org/2000/01/rdf-schema#> .
  @prefix doc: <http://example.org/doc#> .

  doc:Document a owl:Class ;
      rdfs:label "Document" .

  doc:FinancialDocument a owl:Class ;
      rdfs:subClassOf doc:Document ;
      rdfs:label "Financial Document" .

  doc:Receipt a owl:Class ;
      rdfs:subClassOf doc:FinancialDocument ;
      rdfs:label "Receipt" .

  doc:Invoice a owl:Class ;
      rdfs:subClassOf doc:FinancialDocument ;
      rdfs:label "Invoice" .

  doc:Contract a owl:Class ;
      rdfs:subClassOf doc:Document ;
      rdfs:label "Contract" .

  Classifier Implementation:
  pub struct ClassMatch {
      pub class: OntologyUri,       // "doc:Receipt"
      pub shape: ShapeUri,           // "shape:ReceiptShape"
      pub confidence: f32,
      pub matched_by: Vec<String>,   // Field names that satisfied shape
  }

  #[async_trait]
  pub trait Classifier: Send + Sync {
      async fn classify(&self, ext: &Extraction) -> Result<Vec<ClassMatch>>;
  }

  pub struct ShaclClassifier {
      shapes: HashMap<ShapeUri, ShaclShape>,
      ontology: OwlOntology,
  }

  impl Classifier for ShaclClassifier {
      async fn classify(&self, ext: &Extraction) -> Result<Vec<ClassMatch>> {
          let mut matches = Vec::new();

          // Convert Extraction to RDF graph
          let data_graph = extraction_to_rdf(ext)?;

          // Test each shape
          for (shape_uri, shape) in &self.shapes {
              let report = validate_shape(&data_graph, shape)?;

              if report.conforms {
                  let class_uri = shape.target_class.clone();
                  let confidence = calculate_confidence(&report);

                  matches.push(ClassMatch {
                      class: class_uri,
                      shape: shape_uri.clone(),
                      confidence,
                      matched_by: report.satisfied_constraints,
                  });
              }
          }

          // Sort by confidence
          matches.sort_by(|a, b| b.confidence.partial_cmp(&a.confidence).unwrap());

          // Apply OWL inference (subclass relationships)
          matches = self.ontology.infer_supertypes(matches);

          Ok(matches)
      }
  }

  Phase 3.3: Naming Policy (Week 10-11)

  Files to Create:
  - crates/naming/src/lib.rs
  - crates/naming/src/trait.rs - NamingPolicy trait
  - crates/naming/src/dsl_policy.rs - DSL-driven policy
  - crates/naming/src/template.rs - Template expansion
  - crates/naming/rules/receipt.tomllm - Receipt naming rule
  - crates/naming/tests/canonical_path.rs - Path generation test

  StoragePlan:
  pub struct StoragePlan {
      pub bucket: String,
      pub prefix: String,
      pub filename: String,
      pub tags: BTreeMap<String, String>,
      pub ontology_class: OntologyUri,
      pub shape: ShapeUri,
  }

  pub trait NamingPolicy: Send + Sync {
      fn derive(&self, ext: &Extraction, class: &ClassMatch) -> Result<StoragePlan>;
  }

  DSL Naming Rule:
  # crates/naming/rules/receipt.tomllm
  # Receipt naming policy
  # Bucket: finance-docs-au
  # Prefix structure: financial/receipt/{vendor_slug}/{yyyy}/{mm}/

  rule receipt_naming
    when
      class == "doc:Receipt"
      and shape == "shape:ReceiptShape"
    then
      bucket    = "finance-docs-au"
      prefix    = "financial/receipt/{vendor_slug}/{yyyy}/{mm}/"
      filename  = "{date}_{vendor_slug}_{total_minor}_{currency}_receipt.pdf"
      tags      = {
          "ontology_class": "doc:Receipt",
          "shacl_shape": "shape:ReceiptShape",
          "vendor": "{vendor}",
          "total": "{total}",
          "currency": "{currency}",
          "date": "{date}"
      }

  Template Expansion:
  pub fn expand_template(
      template: &str,
      ext: &Extraction,
      class: &ClassMatch,
  ) -> Result<String> {
      let mut result = template.to_string();

      // Extract variables from template
      for var in extract_vars(template) {
          let value = match var.as_str() {
              "vendor_slug" => slugify(ext.fields.get("vendor")?),
              "yyyy" => ext.dates[0].year().to_string(),
              "mm" => format!("{:02}", ext.dates[0].month()),
              "date" => ext.dates[0].format("%Y-%m-%d").to_string(),
              "total_minor" => ext.amounts[0].minor_units().to_string(),
              "currency" => ext.amounts[0].currency.to_string(),
              _ => return Err(NamingError::UnknownVariable(var)),
          };

          result = result.replace(&format!("{{{}}}", var), &value);
      }

      Ok(result)
  }

  Phase 3.4: End-to-End Intake (Week 11-12)

  Files to Create:
  - crates/intake/src/lib.rs
  - crates/intake/src/pipeline.rs - Orchestration pipeline
  - crates/intake/src/quarantine.rs - Unclassified file handling
  - crates/intake/src/s3.rs - S3 upload with metadata tagging
  - crates/intake/tests/receipt_e2e.rs - End-to-end test with mock S3

  Intake Pipeline:
  pub enum IntakeOutcome {
      Classified {
          plan: StoragePlan,
          s3_url: String,
      },
      ClassifiedLowConfidence {
          plan: StoragePlan,
          s3_url: String,
          confidence: f32,
      },
      Unclassified {
          quarantine_url: String,
          reason: String,
      },
      FailedExtraction {
          error: String,
      },
  }

  pub async fn intake_file(
      file: IntakeFile,
      handlers: &[Box<dyn FileHandler>],
      classifier: &dyn Classifier,
      policy: &dyn NamingPolicy,
      s3_client: &S3Client,
  ) -> Result<IntakeOutcome> {
      // 1. Select and run handler
      let handler = select_handler(&file, handlers)?;
      let extraction = match handler.extract(&file).await {
          Ok(ext) => ext,
          Err(e) => return Ok(IntakeOutcome::FailedExtraction {
              error: e.to_string()
          }),
      };

      // 2. Classify
      let matches = classifier.classify(&extraction).await?;

      if matches.is_empty() {
          // Unclassified → quarantine
          l quarantine_url = upload_to_quarantine(&file, s3_client).await?;
          return Ok(IntakeOutcome::Unclassified {
              quarantine_url,
              reason: "No matching SHACL shapes".to_string(),
          });
      }

      let best_match = &matches[0];

      // 3. Generate storage plan
      let plan = policy.derive(&extraction, best_match)?;

      // 4. Upload to S3 with metadata
      let s3_url = upload_with_plan(&file, &plan, s3_client).await?;

      // 5. Return outcome
      if best_match.confidence < 0.75 {
          Ok(IntakeOutcome::ClassifiedLowConfidence {
              plan,
              s3_url,
              confidence: best_match.confidence,
          })
      } else {
          Ok(IntakeOutcome::Classified { plan, s3_url })
      }
  }

  S3 Upload with Metadata:
  async fn upload_with_plan(
      file: &IntakeFile,
      plan: &StoragePlan,
      s3: &S3Client,
  ) -> Result<String> {
      let key = format!("{}{}", plan.prefix, plan.filename);

      s3.put_object()
          .bucket(&plan.bucket)
          .key(&key)
          .body(ByteStream::from(file.bytes.clone()))
          .content_type(file.media_type.as_deref().unwrap_or("application/octet-stream"))
          .metadata("ontology_class", plan.ontology_class.to_string())
          .metadata("shacl_shape", plan.shape.to_string())
          .tagging(
              plan.tags
                  .iter()
                  .map(|(k, v)| format!("{}={}", k, v))
                  .collect::<Vec<_>>()
                  .join("&")
          )
          .send()
          .await?;

      Ok(format!("s3://{}/{}", plan.bucket, key))
  }

  Phase 3.5: MCP Integration (Week 12)

  Files to Create:
  - crates/mcp-server/src/tools/intake_submit.rs - File submission tool
  - crates/mcp-server/src/tools/intake_status.rs - Status tracking
  - examples/receipt-intake/src/main.rs - DoD smoke test

  MCP Tool: intake.submit
  #[async_trait]
  impl Tool for IntakeSubmitTool {
      fn name(&self) -> &str { "intake.submit" }

      fn input_schema(&self) -> serde_json::Value {
          json!({
              "type": "object",
              "properties": {
                  "file_path": { "type": "string" },
                  "media_type": { "type": "string" }
              },
              "required": ["file_path"]
          })
      }

      async fn call(&self, params: serde_json::Value) -> Result<serde_json::Value> {
          let path = params["file_path"].as_str().unwrap();
          let bytes = tokio::fs::read(path).await?;

          let file = IntakeFile {
              sha256: compute_sha256(&bytes),
              bytes: Bytes::from(bytes),
              path_hint: Some(path.to_string()),
              media_type: params.get("media_type").and_then(|v| v.as_str()).map(String::from),
          };

          let outcome = intake_file(file, &self.handlers, &self.classifier, &self.policy, &self.s3).await?;

          Ok(json!({
              "outcome": match outcome {
                  IntakeOutcome::Classified { plan, s3_url } => json!({
                      "status": "classified",
                      "s3_url": s3_url,
                      "ontology_class": plan.ontology_class,
                      "shape": plan.shape,
                  }),
                  IntakeOutcome::Unclassified { quarantine_url, reason } => json!({
                      "status": "unclassified",
                      "quarantine_url": quarantine_url,
                      "reason": reason,
                  }),
                  _ => json!({"status": "error"}),
              }
          }))
      }
  }

  Technical Considerations

  Handler Selection Strategy

  - Score-Based Competition: Multiple handlers can claim same file, highest score wins
  - Fallback Handler: Generic binary handler ensures every file processable
  - External Handlers via MCP: Future support for delegating to specialized MCP servers

  SHACL vs OWL Roles

  - SHACL: Data validation (closed-world assumption) - "Does this extraction satisfy Receipt constraints?"
  - OWL: Knowledge inference (open-world assumption) - "Receipt is a FinancialDocument"
  - Combined: SHACL validates structure, OWL infers supertypes

  Quarantine Strategy

  Unclassified files → s3://intake-quarantine/unclassified/{yyyy-mm-dd}/{sha256}.{ext}
  Failed extraction → s3://intake-quarantine/failed/{yyyy-mm-dd}/{sha256}.{ext}
  Low confidence (<0.75) → s3://intake-quarantine/review/{yyyy-mm-dd}/{sha256}.{ext}

  Naming Collisions

  - Unique Filename: Include SHA256 suffix if collision detected
  - Versioning: S3 versioning enabled for overwrites
  - Audit Trail: Every upload logged to knowledge store

  Acceptance Criteria

  Functional Requirements

  - Receipt Classification: Receipt fixture classified as doc:Receipt with confidence >0.90
  - Unknown File Quarantine: Unknown file type routes to quarantine, not error
  - Storage Plan Metadata: StoragePlan tags include ontology_class and shape on every S3 object
  - External MCP Handler: Contract tested with provider-test fixture (delegation works)
  - DSL Naming: All naming logic in DSL rules (.tomllm files), zero hardcoded paths in Rust
  - End-to-End: examples/receipt-intake runs against mock S3 successfully

  Quality Gates

  - No Hardcoded Paths: grep -r 's3://.*' crates/intake/src/ returns zero literal S3 paths
  - Idempotent Upload: Re-uploading identical file produces same S3 key (deterministic naming)
  - Metadata Completeness: Every uploaded object has ontology_class and shacl_shape tags
  - Fallback Coverage: 100% of files reach terminal state (classified, quarantined, or failed)
  - Shape Validation: All built-in SHACL shapes validate correctly in tests

  Performance Targets

  - Throughput: ≥50 files/minute single-threaded, ≥200 files/minute parallel
  - Latency: PDF extraction + classification <2 seconds per file
  - S3 Upload: Batch uploads use multipart for files >5MB

  Success Metrics

  - ✅ examples/receipt-intake runs end-to-end against mock S3
  - ✅ Receipt PDF classified correctly with confidence >0.90
  - ✅ Generated S3 path matches expected pattern from DSL rule
  - ✅ Ontology metadata tags present on uploaded object

  Dependencies & Risks

  Prerequisites

  - Phase 2 Knowledge Layer completed (#2)
  - S3-compatible storage configured (AWS S3, MinIO, or LocalStack)
  - SHACL/OWL reasoner integrated (oxigraph or rudof)

  Risks

  - OCR Accuracy: Scanned PDFs may have poor text extraction
    - Mitigation: Use high-quality OCR (Tesseract 5+), validate extracted fields
  - Shape Coverage: Not all document types have SHACL shapes initially
    - Mitigation: Graceful fallback to quarantine, incremental shape authoring
  - Naming Collisions: Multiple files with same vendor/date/amount
    - Mitigation: Append SHA256 suffix for uniqueness
  - S3 Costs: Large file uploads expensive
    - Mitigation: Compress before upload, use S3 Intelligent-Tiering

  References & Research

  Internal References

  - Intake Pipeline: /mnt/d/onedrive/tbd/README.md §13
  - SHACL Classification: /mnt/d/onedrive/tbd/README.md §13.3
  - Naming Policy: /mnt/d/onedrive/tbd/README.md §13.4
  - Phase 3 DoD: /mnt/d/onedrive/tbd/README.md §18 (Weeks 9-12)

  External References

  - https://www.w3.org/TR/shacl/
  - https://www.w3.org/TR/owl2-primer/
  - https://rudof-project.github.io/rudof/
  - https://docs.aws.amazon.com/AmazonS3/latest/userguide/object-tagging.html
  - https://poppler.freedesktop.org/

  Related Work

  - See best-practices-researcher report for SHACL vs OWL comparison and classification patterns

  ---
  Issue Type: 🎯 Enhancement
  Labels: phase-3, intake, shacl, owl, classification, high-priority
  Assignees: TBD
  Milestone: Phase 3 - Intake Pipeline
  Depends On: #2 (Phase 2 Knowledge Layer)


  Let me continue with Phase 4 and Phase 5 issues.

  Overview

  Implement full agent orchestration with bounded agent loops, async serial job queue, cross-agent delegation via forward-MCP, and policy
  enforcement (DRTW/NRtW/TRIZ).

  Target Timeline: Weeks 13-16 (Phase 4)
  Status: Not Started
  Depends On: Phase 3 Intake Pipeline (#3)

  Problem Statement / Motivation

  Agent systems fail when:
  - Unbounded Execution: Agents loop infinitely, exhausting budgets
  - No Evidence Requirement: Agents answer without retrieving facts (hallucination)
  - Monolithic Design: Single agent handles all tasks, no specialization
  - No Auditability: Cannot replay execution or understand decision reasoning

  This phase delivers provably bounded agents with checkpointing, policy-enforced evidence retrieval, and auditable delegation to specialized
  sub-agents.

  Proposed Solution

  Implement 4 orchestration-layer crates following TDD workflow:

  1. orchestrator - Agent loop, async serial job queue, checkpoint/replay
  2. policy - DRTW/NRtW/TRIZ enforcement, budget tracking, stop conditions
  3. forward-mcp - MCP client bridge for cross-agent delegation
  4. mcp-server - Add agent.run, agent.delegate, agent.forward_mcp tools

  Technical Approach

  Architecture

  ┌─────────────────────────────────────────────────────┐
  │              Agent Entry Point                       │
  │         (MCP tool: agent.run)                        │
  └─────────────────────┬───────────────────────────────┘
                        │
           ┌────────────▼────────────┐
           │  Job Queue (Serial)     │
           │  TaskId → JobSteps[]    │
           └────────────┬────────────┘
                        │
           ┌────────────▼────────────┐
           │  Agent Loop             │
           │  (for turn in 0..N)     │
           └────────────┬────────────┘
                        │
      ┌─────────────────┼─────────────────┐
      │                 │                 │
      ▼                 ▼                 ▼
  ┌───────────┐   ┌─────────┐    ┌──────────────┐
  │  Plan     │   │ Retrieve│    │  Choose      │
  │           │   │ Evidence│    │  Action      │
  └─────┬─────┘   └────┬────┘    └──────┬───────┘
        │              │                 │
        │              │                 │
        └──────────────┴─────────────────┘
                       │
           ┌───────────▼────────────┐
           │  Policy Check          │
           │  (DRTW/Budget/Turns)   │
           └───────────┬────────────┘
                       │
           ┌───────────▼────────────┐
           │  Execute Action        │
           │  CallTool | Delegate   │
           └───────────┬────────────┘
                       │
           ┌───────────▼────────────┐
           │  Checkpoint State      │
           │  (libSQL persistence)  │
           └────────────────────────┘

  Implementation Phases

  Phase 4.1: Job Model & Queue (Week 13)

  Files to Create:
  - crates/orchestrator/src/lib.rs
  - crates/orchestrator/src/job.rs - Job, JobMode, JobStep types
  - crates/orchestrator/src/queue.rs - Async serial job queue
  - crates/orchestrator/src/checkpoint.rs - State persistence
  - crates/orchestrator/tests/serial_execution.rs - Steps execute serially test

  Job Model:
  pub struct Job {
      pub id: Uuid,
      pub mode: JobMode,
      pub steps: Vec<JobStep>,
      pub created_at: DateTime<Utc>,
      pub updated_at: DateTime<Utc>,
  }

  pub enum JobMode {
      Serial,   // Steps execute sequentially within job
      Batch,    // Multiple jobs run in parallel
  }

  pub enum JobStep {
      Plan {
          objective: String,
          context: Vec<EvidenceRef>,
      },
      Retrieve {
          query: String,
          filters: Vec<Filter>,
      },
      CallTool {
          tool: String,
          args: serde_json::Value,
      },
      Delegate {
          target: McpTarget,
          task: String,
          allowed_tools: Vec<String>,
      },
      Synthesize {
          inputs: Vec<ArtifactRef>,
      },
      Persist {
          artifact: Artifact,
      },
  }

  Serial Job Queue:
  pub struct JobQueue {
      queue: Arc<Mutex<VecDeque<Job>>>,
      store: Arc<dyn StateStore>,
  }

  impl JobQueue {
      pub async fn run(&self) {
          while let Some(job) = self.queue.lock().await.pop_front() {
              for (step_idx, step) in job.steps.iter().enumerate() {
                  // Execute step
                  let result = self.exec_step(step, &job).await?;

                  // Checkpoint after each step
                  self.store.save_checkpoint(&job.id, step_idx, &result).await?;

                  // Handle wait conditions
                  if result.requires_wait() {
                      result.wait_handle.await?;
                  }
              }
          }
      }

      async fn exec_step(&self, step: &JobStep, ctx: &Job) -> Result<StepResult> {
          match step {
              JobStep::Plan { objective, context } => {
                  // Generate execution plan
              }
              JobStep::Retrieve { query, filters } => {
                  // Fusion retrieval
              }
              JobStep::CallTool { tool, args } => {
                  // MCP tool invocation
              }
              JobStep::Delegate { target, task, .. } => {
                  // Forward to child agent
              }
              _ => todo!(),
          }
      }
  }

  Phase 4.2: Agent Loop (Week 13-14)

  Files to Create:
  - crates/orchestrator/src/agent.rs - AgentState, agent_loop function
  - crates/orchestrator/src/planner.rs - Task planning logic
  - crates/orchestrator/src/chooser.rs - Action selection
  - crates/orchestrator/tests/max_turns.rs - Agent stops at max turns test
  - crates/orchestrator/tests/budget.rs - Agent stops at budget exhaustion test

  Agent State:
  pub struct AgentState {
      pub task: String,
      pub turn: u32,
      pub evidence: Vec<EvidenceRef>,
      pub scratch: Vec<ThoughtRecord>,
      pub artifacts: Vec<ArtifactRef>,
      pub budget: BudgetState,
      pub policy: PolicyState,
  }

  pub struct BudgetState {
      pub tokens_used: u32,
      pub tokens_limit: u32,
      pub delegations_used: u8,
      pub delegations_limit: u8,
  }

  pub struct PolicyState {
      pub drtw: bool,              // Do Right Then Write
      pub nrtw: bool,              // No Rush Then Write
      pub triz_enabled: bool,
      pub require_evidence: bool,
      pub max_turns: u32,
  }

  Agent Loop:
  pub async fn agent_loop(
      mut state: AgentState,
      planner: &dyn Planner,
      retriever: &dyn Retriever,
      chooser: &dyn ActionChooser,
      executor: &dyn Executor,
  ) -> Result<Answer> {
      for turn in 0..state.policy.max_turns {
          state.turn = turn;

          // 1. Plan next action
          let plan = planner.plan(&state).await?;

          // 2. Retrieve evidence (if needed)
          let evidence = if plan.requires_evidence {
              retriever.retrieve(&plan).await?
          } else {
              Vec::new()
          };

          // 3. Choose action
          let action = chooser.choose(&plan, &evidence).await?;

          // 4. Policy enforcement
          if let Err(e) = state.policy.check(&action, &state) {
              return Err(PolicyViolation(e).into());
          }

          // 5. Execute action
          match action {
              Action::CallTool(t) => {
                  let result = executor.call_tool(&t.name, &t.args).await?;
                  state.evidence.push(EvidenceRef::ToolResult(result));
                  state.budget.tokens_used += result.tokens_used;
              }
              Action::Delegate(d) => {
                  let result = executor.delegate(&d).await?;
                  state.artifacts.push(ArtifactRef::DelegatedResult(result));
                  state.budget.delegations_used += 1;
                  state.budget.tokens_used += result.tokens_used;
              }
              Action::Answer(a) => {
                  // Agent is done
                  return Ok(a);
              }
              Action::Stop => {
                  // Explicit stop
                  break;
              }
          }

          // 6. Update state
          state.update(evidence, action)?;

          // 7. Check budget
          if state.budget.exhausted() {
              return Err(BudgetExhausted.into());
          }
      }

      Err(MaxTurnsExceeded.into())
  }

  Phase 4.3: Policy Module (Week 14-15)

  Files to Create:
  - crates/policy/src/lib.rs
  - crates/policy/src/drtw.rs - Do Right Then Write enforcement
  - crates/policy/src/nrtw.rs - No Rush Then Write enforcement
  - crates/policy/src/triz.rs - TRIZ heuristics (contradiction detection)
  - crates/policy/src/budget.rs - Token/delegation tracking
  - crates/policy/tests/drtw_violation.rs - DRTW test fails without evidence

  DRTW Enforcement:
  pub struct DrtwPolicy {
      require_evidence_before_answer: bool,
  }

  impl Policy for DrtwPolicy {
      fn check(&self, action: &Action, state: &AgentState) -> Result<()> {
          match action {
              Action::Answer(_) => {
                  if self.require_evidence_before_answer && state.evidence.is_empty() {
                      return Err(PolicyError::DrtwViolation(
                          "Cannot answer without retrieving evidence first".to_string()
                      ));
                  }
                  Ok(())
              }
              _ => Ok(()),
          }
      }
  }

  TRIZ Heuristics:
  pub struct TrizPolicy {
      enabled: bool,
  }

  impl TrizPolicy {
      /// Detect contradictions in plan
      pub fn detect_contradiction(&self, plan: &Plan) -> Option<Contradiction> {
          // Example: "Need to process quickly" vs "Need high accuracy"
          let requires_speed = plan.constraints.contains(&Constraint::FastProcessing);
          let requires_accuracy = plan.constraints.contains(&Constraint::HighAccuracy);

          if requires_speed && requires_accuracy {
              Some(Contradiction {
                  principle1: "Fast processing".to_string(),
                  principle2: "High accuracy".to_string(),
                  suggested_resolution: "Apply segmentation: fast first pass, detailed second pass".to_string(),
              })
          } else {
              None
          }
      }

      /// Apply TRIZ principle: Inversion
      pub fn apply_inversion(&self, plan: &Plan) -> Option<Plan> {
          // Instead of "add X", consider "remove not-X"
          // Instead of "optimize process", consider "eliminate bottleneck"
          None // Placeholder
      }

      /// Apply TRIZ principle: Prior Action
      pub fn apply_prior_action(&self, plan: &Plan) -> Option<Plan> {
          // Move required action earlier in sequence
          None // Placeholder
      }
  }

  Budget Tracking:
  pub struct BudgetPolicy {
      pub max_tokens: u32,
      pub max_delegations: u8,
  }

  impl Policy for BudgetPolicy {
      fn check(&self, action: &Action, state: &AgentState) -> Result<()> {
          match action {
              Action::CallTool(t) => {
                  let estimated_tokens = estimate_tokens(&t.args);
                  if state.budget.tokens_used + estimated_tokens > self.max_tokens {
                      return Err(PolicyError::BudgetExceeded {
                          used: state.budget.tokens_used,
                          limit: self.max_tokens,
                      });
                  }
                  Ok(())
              }
              Action::Delegate(_) => {
                  if state.budget.delegations_used >= self.max_delegations {
                      return Err(PolicyError::DelegationLimitExceeded {
                          used: state.budget.delegations_used,
                          limit: self.max_delegations,
                      });
                  }
                  Ok(())
              }
              _ => Ok(()),
          }
      }
  }

  Phase 4.4: Forward-MCP (Week 15-16)

  Files to Create:
  - crates/forward-mcp/src/lib.rs
  - crates/forward-mcp/src/client.rs - MCP client for delegation
  - crates/forward-mcp/src/context_bundle.rs - Context packaging
  - crates/forward-mcp/src/audit.rs - Delegation audit logging
  - crates/forward-mcp/tests/delegation.rs - Cross-agent delegation test

  Forward Request:
  pub struct ForwardRequest {
      pub target: McpTarget,           // stdio://child:path or http://host
      pub task: String,
      pub allowed_tools: Vec<String>,  // Capability restriction
      pub context_bundle: ContextBundle,
      pub budget_tokens: u32,
      pub return_mode: ReturnMode,
  }

  pub enum McpTarget {
      Stdio { command: String, args: Vec<String> },
      Http { url: String, headers: HashMap<String, String> },
  }

  pub struct ContextBundle {
      pub evidence: Vec<EvidenceRef>,
      pub ontology_snapshot: Option<Vec<Triple>>,
      pub relevant_symbols: Vec<SymbolRef>,
  }

  pub enum ReturnMode {
      FinalOnly,         // Just the answer
      FinalWithTrace,    // Answer + execution trace
  }

  Delegation Implementation:
  pub async fn forward_to_agent(req: ForwardRequest) -> Result<DelegationResult> {
      // 1. Establish MCP connection
      let client = match req.target {
          McpTarget::Stdio { command, args } => {
              McpClient::connect_stdio(&command, &args).await?
          }
          McpTarget::Http { url, headers } => {
              McpClient::connect_http(&url, headers).await?
          }
      };

      // 2. Send context bundle as resources
      for evidence in &req.context_bundle.evidence {
          client.provide_resource(evidence.to_mcp_resource()).await?;
      }

      // 3. Invoke agent with restricted tools
      let response = client.call_tool("agent.run", json!({
          "task": req.task,
          "allowed_tools": req.allowed_tools,
          "budget_tokens": req.budget_tokens,
      })).await?;

      // 4. Audit log
      audit_delegation(&req, &response).await?;

      // 5. Return result
      Ok(DelegationResult {
          answer: response["answer"].as_str().unwrap().to_string(),
          tokens_used: response["tokens_used"].as_u64().unwrap() as u32,
          trace: if matches!(req.return_mode, ReturnMode::FinalWithTrace) {
              Some(response["trace"].clone())
          } else {
              None
          },
      })
  }

  Phase 4.5: MCP Integration (Week 16)

  Files to Create:
  - crates/mcp-server/src/tools/agent_run.rs - Agent execution tool
  - crates/mcp-server/src/tools/agent_delegate.rs - Delegation tool
  - examples/forward-mcp/src/main.rs - DoD smoke test

  MCP Tool: agent.run
  #[async_trait]
  impl Tool for AgentRunTool {
      fn name(&self) -> &str { "agent.run" }

      fn input_schema(&self) -> serde_json::Value {
          json!({
              "type": "object",
              "properties": {
                  "task": { "type": "string" },
                  "budget_tokens": { "type": "integer", "default": 10000 },
                  "max_turns": { "type": "integer", "default": 10 }
              },
              "required": ["task"]
          })
      }

      async fn call(&self, params: serde_json::Value) -> Result<serde_json::Value> {
          let task = params["task"].as_str().unwrap().to_string();
          let budget = params.get("budget_tokens").and_then(|v| v.as_u64()).unwrap_or(10000) as u32;
          let max_turns = params.get("max_turns").and_then(|v| v.as_u64()).unwrap_or(10) as u32;

          let state = AgentState {
              task,
              turn: 0,
              evidence: Vec::new(),
              scratch: Vec::new(),
              artifacts: Vec::new(),
              budget: BudgetState {
                  tokens_used: 0,
                  tokens_limit: budget,
                  delegations_used: 0,
                  delegations_limit: 3,
              },
              policy: PolicyState {
                  drtw: true,
                  nrtw: true,
                  triz_enabled: true,
                  require_evidence: true,
                  max_turns,
              },
          };

          let answer = agent_loop(
              state,
              &self.planner,
              &self.retriever,
              &self.chooser,
              &self.executor,
          ).await?;

          Ok(json!({
              "answer": answer.text,
              "tokens_used": answer.tokens_used,
              "turns": answer.turns,
          }))
      }
  }

  Technical Considerations

  Checkpoint/Replay

  - State Persistence: After each step, serialize AgentState to libSQL
  - Replay: Load checkpoint by job ID, resume from last completed step
  - Determinism: Same inputs + same checkpoint → same execution path

  Policy Composition

  pub struct PolicySet {
      policies: Vec<Box<dyn Policy>>,
  }

  impl PolicySet {
      pub fn check(&self, action: &Action, state: &AgentState) -> Result<()> {
          for policy in &self.policies {
              policy.check(action, state)?;
          }
          Ok(())
      }
  }

  // Usage
  let policies = PolicySet {
      policies: vec![
          Box::new(DrtwPolicy { require_evidence_before_answer: true }),
          Box::new(BudgetPolicy { max_tokens: 50000, max_delegations: 5 }),
          Box::new(TrizPolicy { enabled: true }),
      ],
  };

  Audit Trail

  Every action logged:
  pub struct AuditEntry {
      pub job_id: Uuid,
      pub turn: u32,
      pub action: Action,
      pub timestamp: DateTime<Utc>,
      pub tokens_used: u32,
      pub result: ActionResult,
  }

  Acceptance Criteria

  Functional Requirements

  - Job Replayability: Jobs replayable from checkpoint state (deterministic re-execution verified)
  - Max Turns Enforcement: Agent loop provably bounded (test fails after max_turns exceeded)
  - Budget Enforcement: Agent stops when token budget exhausted (test verifies stop condition)
  - DRTW Policy: drtw: true causes test failure when answer produced without evidence retrieval
  - Forward-MCP: Agent delegates to child agent; result returned with trace
  - MCP Tools Operational: agent.run, agent.delegate, agent.forward_mcp all return correct results

  Quality Gates

  - Checkpoint After Every Step: Test verifies state persisted after each JobStep
  - Policy Violations Structured: Errors are typed enums, not panics
  - Tool Audit Logging: Every tool call logged before execution (name + args recorded)
  - Delegation Tracing: Child agent invocations traceable via audit log
  - No Infinite Loops: Static analysis + tests confirm bounded execution

  Performance Targets

  - Checkpoint Overhead: <10ms per checkpoint write
  - Policy Check Overhead: <1ms per action
  - Delegation Latency: <200ms overhead (excluding child agent execution)

  Success Metrics

  - ✅ examples/forward-mcp demonstrates cross-agent delegation with audit log
  - ✅ Agent loop stops at max_turns (does not run indefinitely)
  - ✅ DRTW test fails when answer precedes evidence retrieval
  - ✅ Job resumable from checkpoint after crash/restart

  Dependencies & Risks

  Prerequisites

  - Phase 3 Intake Pipeline completed (#3)
  - libSQL database configured for state persistence
  - MCP client library available (stdio + HTTP transports)

  Risks

  - Checkpoint Performance: Frequent writes may slow execution
    - Mitigation: Async writes, batch checkpoints, in-memory buffer
  - Delegation Overhead: Cross-agent calls add latency
    - Mitigation: Parallel delegation when independent, cache sub-agent results
  - Policy Complexity: Too many policies may conflict
    - Mitigation: Policy composition testing, clear precedence rules
  - TRIZ Heuristics: May suggest incorrect resolutions
    - Mitigation: User confirmation required for TRIZ suggestions

  References & Research

  Internal References

  - Agent Loop: /mnt/d/onedrive/tbd/README.md §15.2
  - Policy Module: /mnt/d/onedrive/tbd/README.md §15.3
  - Forward-MCP: /mnt/d/onedrive/tbd/README.md §15.4
  - Phase 4 DoD: /mnt/d/onedrive/tbd/README.md §18 (Weeks 13-16)

  External References

  - https://github.com/modelcontextprotocol/rust-sdk
  - https://www.triz.org/principles
  - https://tokio.rs/tokio/topics/tracing
  - https://github.com/libsql/libsql
  - https://arxiv.org/abs/2310.03714

  Related Work

  - See repo-research-analyst report for agent orchestration patterns and bounded execution strategies

  ---
  Issue Type: 🎯 Enhancement
  Labels: phase-4, orchestration, agent-loop, policy, delegation, high-priority
  Assignees: TBD
  Milestone: Phase 4 - Agent Orchestration
  Depends On: #3 (Phase 3 Intake Pipeline)


  Now let me create the final Phase 5 issue:

  Overview

  Connect all three projections (operational Rust types, semantic RDF triples, architectural EA records) via shared URIs, enabling cross-domain
  queries and unified governance.

  Target Timeline: Weeks 17-20 (Phase 5)
  Status: Not Started
  Depends On: Phase 4 Agent Orchestration (#4)

  Problem Statement / Motivation

  Business objects exist in silos:
  - Operational: NMI/DUID as Rust types in code
  - Semantic: Same objects as unlinked RDF triples in graph store
  - Architectural: Ghost records in iServer365 EA repository

  The same DUID: BBTHREE1 appears three times with no binding. Queries cannot span domains:
  - "Show me all BESS units with their EA capability mappings" → impossible today
  - "Which dispatch units are in NSW1 region and interface with SPARTAN?" → requires manual joins
  - "Has NMI 6305299307 been architecturally approved?" → different systems, no link

  This phase delivers the standing data spine: every business object has a stable git-anchored URI that unifies all three views.

  Proposed Solution

  Implement standing data spine with 5 integration points:

  1. Shared URI Scheme - Every domain type → canonical nem: URI
  2. RDF Binding - Rust types → Turtle triples queryable via SPARQL
  3. SHACL Shapes for Arrow - Validate operational data against semantic shapes
  4. EA Repository Linkage - iServer365 record URIs embedded as ea:hasRecord edges
  5. SharePoint Replacement - Standing data queryable via SPARQL + git replay

  Technical Approach

  Architecture

  ┌──────────────────────────────────────────────────────┐
  │           Business Object: ColocatedBessSolar        │
  │                  NMI: 4XXXXXXX01                     │
  ├──────────────────┬──────────────────┬────────────────┤
  │ Operational      │ Semantic         │ Architectural  │
  │ (§5 domain)      │ (§9/§11 graph)   │ (EA repo)      │
  │                  │                  │                │
  │ ColocatedBessSolar│ rdf:type        │ iServer365     │
  │   .nmi           │  nem:BessUnit    │ capability:    │
  │   .bess_duid     │ nem:coLocatedWith│  DER_Dispatch  │
  │   .solar_duid    │  nem:SolarFarm   │ system:        │
  │   .region        │ nem:region       │  SPARTAN        │
  │   .bess_mwh      │  nem:NSW1        │ interface:     │
  │                  │ ea:hasRecord     │  DISPATCHLOAD  │
  │                  │  <iserver365/>   │                │
  └──────────────────┴──────────────────┴────────────────┘
           all three views share: git:blob:<hash> URI

  Implementation Phases

  Phase 5.1: Shared URI Scheme (Week 17)

  Files to Create:
  - crates/domain/src/uri.rs - Canonical URI generation
  - crates/domain/src/namespace.rs - NEM namespace definitions
  - crates/domain/tests/uri_stability.rs - URI determinism tests
  - docs/URI_SCHEME.md - URI scheme documentation

  URI Scheme:
  nem:nmi:<10-char-nmi>                     # NMI identifier
  nem:duid:<duid>                           # DUID identifier
  nem:region:<region-code>                  # Region (NSW1, QLD1, etc.)
  nem:participant:<participant-id>          # Participant
  nem:asset-class:<class-name>              # Asset class enum
  nem:unit:<duid>                           # Dispatch unit (aggregation)

  git:blob:<sha1-hash>                      # File content identity
  git:tree:<sha1-hash>                      # Directory identity
  git:commit:<sha1-hash>                    # Snapshot identity
  symbol:git:blob:<sha1>:<symbol-name>      # Code symbol identity

  ea:record:<iserver365-id>                 # EA repository record
  ea:capability:<capability-name>           # TOGAF capability
  ea:system:<system-name>                   # System
  ea:interface:<interface-name>             # Interface

  URI Generation:
  impl Nmi {
      pub fn to_uri(&self) -> OntologyUri {
          OntologyUri::new(&format!("nem:nmi:{}", self.0))
      }
  }

  impl Duid {
      pub fn to_uri(&self) -> OntologyUri {
          OntologyUri::new(&format!("nem:duid:{}", self.0))
      }
  }

  impl DuDetail {
      pub fn to_uri(&self) -> OntologyUri {
          // Aggregate URI based on DUID
          self.duid.to_uri()
      }

      pub fn to_triples(&self) -> Vec<Triple> {
          let subject = self.to_uri();

          vec![
              Triple::new(subject.clone(), rdf_type(), nem_dispatch_unit()),
              Triple::new(subject.clone(), nem_duid(), self.duid.to_uri()),
              Triple::new(subject.clone(), nem_region(), self.region.to_uri()),
              Triple::new(subject.clone(), nem_tlf(), literal_f64(self.tlf.value())),
              Triple::new(subject.clone(), nem_dlf(), literal_f64(self.dlf.value())),
              // ... other triples
          ]
      }
  }

  Phase 5.2: RDF Binding (Week 17-18)

  Files to Create:
  - crates/domain/src/rdf.rs - Rust → RDF conversion
  - crates/domain/src/ontology.ttl - NEM ontology (OWL)
  - crates/domain/src/shapes.ttl - SHACL shapes for domain types
  - crates/domain/tests/rdf_round_trip.rs - Serialization tests

  NEM Ontology (OWL):
  @prefix nem: <http://example.org/nem#> .
  @prefix owl: <http://www.w3.org/2002/07/owl#> .
  @prefix rdf: <http://www.w3.org/1999/02/22-rdf-syntax-ns#> .
  @prefix rdfs: <http://www.w3.org/2000/01/rdf-schema#> .
  @prefix xsd: <http://www.w3.org/2001/XMLSchema#> .

  # Ontology declaration
  nem:Ontology a owl:Ontology ;
      rdfs:label "National Electricity Market Ontology" ;
      rdfs:comment "Operational, semantic, and architectural model of NEM entities" .

  # Classes
  nem:DispatchUnit a owl:Class ;
      rdfs:label "Dispatch Unit" ;
      rdfs:comment "A dispatchable generation or load unit" .

  nem:BessUnit a owl:Class ;
      rdfs:subClassOf nem:DispatchUnit ;
      rdfs:label "Battery Energy Storage System" .

  nem:SolarFarm a owl:Class ;
      rdfs:subClassOf nem:DispatchUnit ;
      rdfs:label "Solar Farm" .

  nem:ColocatedBessSolar a owl:Class ;
      rdfs:label "Colocated BESS + Solar" ;
      owl:equivalentClass [
          a owl:Class ;
          owl:intersectionOf (
              [ a owl:Restriction ;
                owl:onProperty nem:coLocatedWith ;
                owl:someValuesFrom nem:SolarFarm ]
              [ a owl:Restriction ;
                owl:onProperty rdf:type ;
                owl:hasValue nem:BessUnit ]
          )
      ] .

  # Properties
  nem:duid a owl:DatatypeProperty ;
      rdfs:domain nem:DispatchUnit ;
      rdfs:range xsd:string ;
      rdfs:label "DUID" .

  nem:nmi a owl:DatatypeProperty ;
      rdfs:domain nem:DispatchUnit ;
      rdfs:range xsd:string ;
      rdfs:label "NMI" .

  nem:region a owl:ObjectProperty ;
      rdfs:domain nem:DispatchUnit ;
      rdfs:range nem:Region ;
      rdfs:label "Region" .

  nem:coLocatedWith a owl:ObjectProperty ;
      rdfs:domain nem:BessUnit ;
      rdfs:range nem:SolarFarm ;
      rdfs:label "Co-located with" .

  nem:tlf a owl:DatatypeProperty ;
      rdfs:domain nem:DispatchUnit ;
      rdfs:range xsd:double ;
      rdfs:label "Transmission Loss Factor" .

  nem:dlf a owl:DatatypeProperty ;
      rdfs:domain nem:DispatchUnit ;
      rdfs:range xsd:double ;
      rdfs:label "Distribution Loss Factor" .

  # EA Repository Linkage
  ea:hasRecord a owl:ObjectProperty ;
      rdfs:domain nem:DispatchUnit ;
      rdfs:range ea:Record ;
      rdfs:label "Has EA Repository Record" .

  SHACL Shapes for Validation:
  @prefix sh: <http://www.w3.org/ns/shacl#> .
  @prefix nem: <http://example.org/nem#> .
  @prefix xsd: <http://www.w3.org/2001/XMLSchema#> .

  nem:DispatchUnitShape a sh:NodeShape ;
      sh:targetClass nem:DispatchUnit ;
      sh:property [
          sh:path nem:duid ;
          sh:datatype xsd:string ;
          sh:minCount 1 ;
          sh:maxCount 1 ;
          sh:pattern "^[A-Z0-9]{1,20}$" ;
          sh:message "DUID must be 1-20 uppercase alphanumeric characters" ;
      ] ;
      sh:property [
          sh:path nem:region ;
          sh:class nem:Region ;
          sh:minCount 1 ;
          sh:maxCount 1 ;
      ] ;
      sh:property [
          sh:path nem:tlf ;
          sh:datatype xsd:double ;
          sh:minInclusive 0.0 ;
          sh:maxInclusive 2.0 ;
          sh:message "TLF must be between 0.0 and 2.0" ;
      ] .

  nem:ColocatedBessSolarShape a sh:NodeShape ;
      sh:targetClass nem:ColocatedBessSolar ;
      sh:property [
          sh:path nem:coLocatedWith ;
          sh:class nem:SolarFarm ;
          sh:minCount 1 ;
          sh:maxCount 1 ;
          sh:message "Colocated BESS must have exactly one solar farm" ;
      ] .

  Phase 5.3: Arrow Schema Validation (Week 18)

  Files to Create:
  - crates/domain/src/arrow_shacl.rs - Arrow schema → SHACL converter
  - crates/domain/tests/arrow_validation.rs - Schema validation tests

  Arrow Schema → SHACL:
  pub fn arrow_schema_to_shacl(schema: &arrow2::datatypes::Schema) -> String {
      let mut shapes = String::new();

      for field in &schema.fields {
          let shape = match field.data_type() {
              DataType::Utf8 => {
                  let required = if field.is_nullable { "0" } else { "1" };
                  format!(
                      "sh:property [ sh:path nem:{} ; sh:datatype xsd:string ; sh:minCount {} ; sh:maxCount 1 ] ;",
                      field.name(), required
                  )
              }
              DataType::Int64 => {
                  format!(
                      "sh:property [ sh:path nem:{} ; sh:datatype xsd:long ; sh:minCount 1 ; sh:maxCount 1 ] ;",
                      field.name()
                  )
              }
              DataType::Float64 => {
                  format!(
                      "sh:property [ sh:path nem:{} ; sh:datatype xsd:double ; sh:minCount 1 ; sh:maxCount 1 ] ;",
                      field.name()
                  )
              }
              _ => String::new(),
          };
          shapes.push_str(&shape);
      }

      format!(
          "@prefix sh: <http://www.w3.org/ns/shacl#> .\n\
           @prefix nem: <http://example.org/nem#> .\n\
           @prefix xsd: <http://www.w3.org/2001/XMLSchema#> .\n\n\
           nem:ArrowSchemaShape a sh:NodeShape ;\n\
               {}\n\
           .",
          shapes
      )
  }

  Phase 5.4: EA Repository Linkage (Week 19)

  Files to Create:
  - crates/domain/src/ea_link.rs - iServer365 record linking
  - crates/domain/tests/ea_integration.rs - EA record resolution tests
  - docs/EA_INTEGRATION.md - iServer365 integration guide

  EA Record Linkage:
  pub struct EaRecordLink {
      pub dispatch_unit_uri: OntologyUri,
      pub ea_record_uri: OntologyUri,
      pub capability: String,
      pub system: String,
      pub interfaces: Vec<String>,
  }

  impl DuDetail {
      pub fn link_ea_record(&self, ea_record_id: &str) -> Vec<Triple> {
          let subject = self.to_uri();
          let ea_uri = OntologyUri::new(&format!("ea:record:{}", ea_record_id));

          vec![
              Triple::new(subject.clone(), ea_has_record(), ea_uri.clone()),
              Triple::new(ea_uri.clone(), ea_capability(), literal_str("DER_Dispatch")),
              Triple::new(ea_uri.clone(), ea_system(), literal_str("SPARTAN")),
              Triple::new(ea_uri, ea_interface(), literal_str("DISPATCHLOAD")),
          ]
      }
  }

  SPARQL Query Example (Unified Query):
  PREFIX nem: <http://example.org/nem#>
  PREFIX ea: <http://example.org/ea#>
  PREFIX rdf: <http://www.w3.org/1999/02/22-rdf-syntax-ns#>

  SELECT ?duid ?region ?capability ?system
  WHERE {
      ?unit rdf:type nem:BessUnit ;
            nem:duid ?duid ;
            nem:region nem:NSW1 ;
            ea:hasRecord ?eaRecord .

      ?eaRecord ea:capability ?capability ;
                ea:system ?system .
  }

  Phase 5.5: SharePoint Replacement Query Interface (Week 19-20)

  Files to Create:
  - crates/mcp-server/src/tools/sparql_query.rs - SPARQL query tool
  - crates/mcp-server/src/tools/temporal_query.rs - Time-travel queries
  - examples/standing-data-query/src/main.rs - DoD query examples

  SPARQL Query Tool:
  #[async_trait]
  impl Tool for SparqlQueryTool {
      fn name(&self) -> &str { "standing_data.query" }

      fn input_schema(&self) -> serde_json::Value {
          json!({
              "type": "object",
              "properties": {
                  "query": { "type": "string", "description": "SPARQL query" },
                  "valid_time": {
                      "type": "string",
                      "description": "ISO 8601 timestamp for time-travel query (optional)"
                  }
              },
              "required": ["query"]
          })
      }

      async fn call(&self, params: serde_json::Value) -> Result<serde_json::Value> {
          let query = params["query"].as_str().unwrap();

          let results = if let Some(valid_time_str) = params.get("valid_time") {
              // Time-travel query
              let valid_time = DateTime::parse_from_rfc3339(valid_time_str.as_str().unwrap())?;
              self.git_ledger.temporal_query(query, valid_time.into()).await?
          } else {
              // Current query
              self.store.query_sparql(query).await?
          };

          Ok(json!({
              "results": results.iter().map(|row| {
                  row.iter()
                      .map(|(var, value)| (var.clone(), value.to_string()))
                      .collect::<serde_json::Map<_, _>>()
              }).collect::<Vec<_>>()
          }))
      }
  }

  Time-Travel Query:
  impl GitLedger {
      pub async fn temporal_query(
          &self,
          sparql: &str,
          valid_time: DateTime<Utc>,
      ) -> Result<Vec<QueryRow>> {
          // 1. Find commit at valid_time
          let commits = self.commits_before(valid_time)?;
          let commit = commits.first().ok_or(QueryError::NoCommitFound)?;

          // 2. Replay state
          let frozen = self.replay(*commit)?;

          // 3. Reconstruct ontology graph
          let mut graph = Graph::new();
          for triple in frozen.manifest.triples {
              graph.insert(triple);
          }

          // 4. Execute SPARQL against historical state
          graph.query(sparql)
      }
  }

  Technical Considerations

  URI Stability

  - Git Blob Hashes: Content-addressed, stable across platforms
  - NMI/DUID URIs: Deterministic from business object values
  - EA Record URIs: External system IDs embedded directly

  RDF Triple Volume

  - DuDetail Record: ~20 triples (properties + edges)
  - 10K Dispatch Units: ~200K triples
  - Oxigraph Performance: Handles millions of triples efficiently

  Bitemporal Queries

  // Query "as it was known on 2025-03-01"
  let results = temporal_query(
      "SELECT ?duid ?region WHERE { ?unit nem:duid ?duid ; nem:region ?region }",
      DateTime::parse_from_rfc3339("2025-03-01T00:00:00Z")?,
  ).await?;

  EA Repository Integration

  - iServer365 API: REST API for record lookup
  - Cache Strategy: Cache EA record metadata in Neumann
  - Sync Frequency: Daily batch sync, on-demand refresh

  Acceptance Criteria

  Functional Requirements

  - Canonical URIs: Every domain type has canonical nem: URI (documented in URI_SCHEME.md)
  - RDF Binding: DuDetail → Turtle triples queryable via ontology.run_sparql
  - SHACL Validation: NmiRecord shape validates Arrow output (Arrow schema → SHACL tested)
  - EA Linkage: EA record URIs embedded in ontology as ea:hasRecord edges
  - Time-Travel Query: SPARQL query SELECT ?duid WHERE { ?duid a nem:BidirectionalUnit } returns results at historical timestamp

  Quality Gates

  - URI Determinism: Same input produces same URI across platforms (tested with 1000 random inputs)
  - Triple Round-Trip: Rust → RDF → SPARQL query → results match original data
  - No Orphan URIs: Every URI resolves to at least one triple (no dangling references)
  - Cross-Domain Queries: SPARQL queries can join operational, semantic, and architectural data
  - Temporal Consistency: Historical queries produce stable results (replay determinism)

  Performance Targets

  - URI Generation: <1µs per URI
  - RDF Serialization: 10K DuDetail records → Turtle in <5 seconds
  - SPARQL Query: Simple SELECT query <100ms, complex JOIN <1 second
  - Time-Travel Overhead: Replay + query <2x current query time

  Success Metrics

  - ✅ SPARQL query successfully joins operational (DuDetail), semantic (BessUnit), and architectural (EA record) data
  - ✅ Time-travel query returns historical state correctly (verified against known snapshot)
  - ✅ Arrow schema validates against SHACL shape (no violations)
  - ✅ EA record URIs resolve to capability/system/interface metadata

  Dependencies & Risks

  Prerequisites

  - Phase 4 Agent Orchestration completed (#4)
  - iServer365 API access credentials
  - Oxigraph or equivalent SPARQL store configured
  - Git ledger with bitemporal snapshots operational

  Risks

  - iServer365 API Rate Limits: EA repository queries may be throttled
    - Mitigation: Aggressive caching, batch API calls, nightly sync
  - URI Scheme Changes: Changing URIs breaks historical queries
    - Mitigation: URI scheme versioning, redirect mappings for deprecated URIs
  - Triple Volume: Large ontology graphs may slow queries
    - Mitigation: Selective indexing, partitioning by region/asset class
  - EA Data Freshness: Cached EA data may be stale
    - Mitigation: TTL-based cache invalidation, manual refresh command

  References & Research

  Internal References

  - Standing Data Spine: /mnt/d/onedrive/tbd/README.md §17
  - URI Scheme: /mnt/d/onedrive/tbd/README.md §9.1
  - Bitemporal Model: /mnt/d/onedrive/tbd/README.md §9.3
  - Phase 5 DoD: /mnt/d/onedrive/tbd/README.md §18 (Weeks 17-20)

  External References

  - https://www.w3.org/TR/sparql11-query/
  - https://www.w3.org/TR/shacl/
  - https://www.w3.org/TR/owl2-overview/
  - https://en.wikipedia.org/wiki/Bitemporal_modeling
  - https://pubs.opengroup.org/architecture/togaf9-doc/arch/chap34.html

  Related Work

  - See framework-docs-researcher report for Oxigraph, SHACL validation, and RDF serialization patterns

  ---
  Issue Type: 🎯 Enhancement
  Labels: phase-5, standing-data, rdf, sparql, ea-integration, high-priority
  Assignees: TBD
  Milestone: Phase 5 - Standing Data Spine
  Depends On: #4 (Phase 4 Agent Orchestration)


  ---
  Summary

  I've created 5 comprehensive GitHub issues covering all phases of your promptexecution project:

  ✅ Created Issues:

  1. Phase 1: Foundation (Weeks 1-4) - Runnable daemon with parse boundaries, git ledger, MCP server
  2. Phase 2: Knowledge Layer (Weeks 5-8) - Code graph, incremental indexing, fusion retrieval, DSL compiler
  3. Phase 3: Intake Pipeline (Weeks 9-12) - File handlers, SHACL/OWL classification, DSL-driven naming, S3 storage
  4. Phase 4: Agent Orchestration (Weeks 13-16) - Agent loop, policy enforcement (DRTW/NRtW/TRIZ), forward-MCP delegation
  5. Phase 5: Standing Data Spine (Weeks 17-20) - URI scheme, RDF binding, EA repository linkage, time-travel queries

  📋 Each Issue Includes:

  - Comprehensive Overview with problem statement and motivation
  - Detailed Technical Approach with architecture diagrams
  - Phase-by-Phase Implementation with specific files to create
  - Code Examples for key patterns and implementations
  - Acceptance Criteria (functional, quality gates, performance)
  - Success Metrics with measurable DoD
  - Risk Analysis with mitigation strategies
  - Extensive References (internal PRD sections + external docs)

  All issues follow the "📋 MORE - Standard Issue" template suitable for complex technical projects requiring team collaboration.

  You can now:
  1. Review these issues
  2. Create them in GitHub with gh issue create
  3. Begin Phase 1 implementation following the TDD RED/GREEN workflow

  Would you like me to:
  - Create additional issues for specific sub-tasks?
  - Generate CLAUDE.md and CONTRIBUTING.md files?
  - Set up the initial workspace structure?

