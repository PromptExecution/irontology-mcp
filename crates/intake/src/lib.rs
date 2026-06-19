use std::{
    collections::BTreeMap,
    fs,
    path::{Path, PathBuf},
    sync::Arc,
};

use anyhow::{anyhow, Result};
use async_trait::async_trait;
use classifier::{ClassMatch, Classifier};
use domain::{Artifact, ArtifactKind, EvidenceBundle, SourceSystemKind};
use handlers::{Extraction, HandlerRegistry, IntakeFile};
use naming::{NamingPolicy, StoragePlan};
use serde::Deserialize;
pub mod paper;
pub use paper::{ArXivConnector, HuggingFacePapersConnector, PdfExtractor};


pub const DIRECTORY_CONFIG_FILE: &str = ".promptexecution.toml";

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum IntakeOutcome {
    Classified,
    ClassifiedLowConfidence,
    Unclassified,
    FailedExtraction,
}

#[derive(Debug, Clone, PartialEq)]
pub struct IntakeDecision {
    pub handler_name: String,
    pub extraction: Extraction,
    pub class_match: Option<ClassMatch>,
    pub storage_plan: Option<StoragePlan>,
    pub outcome: IntakeOutcome,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SourceSystem {
    pub id: String,
    pub kind: SourceSystemKind,
    pub locator: String,
}

#[async_trait]
pub trait ArtifactConnector: Send + Sync {
    fn name(&self) -> &str;
    fn supports(&self, source: &SourceSystem) -> bool;
    async fn list_artifacts(&self, source: &SourceSystem) -> Result<Vec<Artifact>>;
}

#[async_trait]
pub trait EvidenceExtractor: Send + Sync {
    fn name(&self) -> &str;
    fn supports(&self, artifact: &Artifact) -> bool;
    async fn extract(&self, artifact: &Artifact) -> Result<EvidenceBundle>;
}

pub struct ConnectorRegistry {
    connectors: Vec<Arc<dyn ArtifactConnector>>,
}

impl ConnectorRegistry {
    pub fn new(connectors: Vec<Arc<dyn ArtifactConnector>>) -> Self {
        Self { connectors }
    }

    pub fn connectors_for(&self, source: &SourceSystem) -> Vec<Arc<dyn ArtifactConnector>> {
        self.connectors
            .iter()
            .filter(|connector| connector.supports(source))
            .cloned()
            .collect()
    }
}

pub struct ExtractorRegistry {
    extractors: Vec<Arc<dyn EvidenceExtractor>>,
}

impl ExtractorRegistry {
    pub fn new(extractors: Vec<Arc<dyn EvidenceExtractor>>) -> Self {
        Self { extractors }
    }

    pub fn select(&self, artifact: &Artifact) -> Option<Arc<dyn EvidenceExtractor>> {
        self.extractors
            .iter()
            .find(|extractor| extractor.supports(artifact))
            .cloned()
    }
}

pub async fn ingest_source(
    source: &SourceSystem,
    connectors: &ConnectorRegistry,
    extractors: &ExtractorRegistry,
) -> Result<Vec<EvidenceBundle>> {
    let mut bundles = Vec::new();
    for connector in connectors.connectors_for(source) {
        for artifact in connector.list_artifacts(source).await? {
            if let Some(extractor) = extractors.select(&artifact) {
                bundles.push(extractor.extract(&artifact).await?);
            }
        }
    }

    Ok(bundles)
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PollMode {
    Watch,
    Interval,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PollConfig {
    pub mode: PollMode,
    pub interval_seconds: Option<u64>,
}

impl Default for PollConfig {
    fn default() -> Self {
        Self {
            mode: PollMode::Watch,
            interval_seconds: None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct AdapterSet {
    pub extractors: Vec<String>,
    pub executors: Vec<String>,
    pub error_handlers: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ErrorPolicy {
    pub retry_limit: u32,
    pub quarantine_dir: Option<String>,
    pub stop_on_error: bool,
}

impl Default for ErrorPolicy {
    fn default() -> Self {
        Self {
            retry_limit: 0,
            quarantine_dir: None,
            stop_on_error: false,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DirectorySource {
    pub root_dir: PathBuf,
    pub source: SourceSystem,
    pub metadata_tags: BTreeMap<String, String>,
    pub ontology_refs: Vec<String>,
    pub poll: PollConfig,
    pub adapters: AdapterSet,
    pub error_policy: ErrorPolicy,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StagedArtifact {
    pub source: DirectorySource,
    pub artifact: Artifact,
    pub absolute_path: PathBuf,
    pub relative_path: PathBuf,
    pub content_hash: String,
}

impl DirectorySource {
    pub fn from_root(root_dir: impl Into<PathBuf>) -> Result<Self> {
        let root_dir = root_dir.into();
        let canonical_root = root_dir.canonicalize().unwrap_or(root_dir.clone());
        let config_path = canonical_root.join(DIRECTORY_CONFIG_FILE);
        if config_path.is_file() {
            Self::load(&canonical_root)
        } else {
            let root_label = canonical_root
                .file_name()
                .and_then(|value| value.to_str())
                .unwrap_or("staging");
            Ok(Self {
                root_dir: canonical_root.clone(),
                source: SourceSystem {
                    id: format!("dir://{}", canonical_root.display()),
                    kind: SourceSystemKind::DocumentSilo,
                    locator: canonical_root.display().to_string(),
                },
                metadata_tags: BTreeMap::from([("source_dir".to_string(), root_label.to_string())]),
                ontology_refs: Vec::new(),
                poll: PollConfig::default(),
                adapters: AdapterSet::default(),
                error_policy: ErrorPolicy::default(),
            })
        }
    }

    pub fn load(root_dir: impl AsRef<Path>) -> Result<Self> {
        let root_dir = root_dir.as_ref();
        let config_path = root_dir.join(DIRECTORY_CONFIG_FILE);
        let raw = fs::read_to_string(&config_path)?;
        let doc: DirectorySourceDocument = toml::from_str(&raw)?;
        doc.validate()?;

        Ok(Self {
            root_dir: root_dir
                .canonicalize()
                .unwrap_or_else(|_| root_dir.to_path_buf()),
            source: SourceSystem {
                id: doc.source.id,
                kind: parse_source_kind(&doc.source.kind),
                locator: doc
                    .source
                    .locator
                    .unwrap_or_else(|| root_dir.display().to_string()),
            },
            metadata_tags: doc.metadata.tags,
            ontology_refs: doc.ontologies.refs,
            poll: PollConfig {
                mode: if doc.poll.mode == "interval" {
                    PollMode::Interval
                } else {
                    PollMode::Watch
                },
                interval_seconds: doc.poll.interval_seconds,
            },
            adapters: AdapterSet {
                extractors: doc.adapters.extractors,
                executors: doc.adapters.executors,
                error_handlers: doc.adapters.error_handlers,
            },
            error_policy: ErrorPolicy {
                retry_limit: doc.errors.retry_limit,
                quarantine_dir: doc.errors.quarantine_dir,
                stop_on_error: doc.errors.stop_on_error,
            },
        })
    }

    pub fn contains_path(&self, path: &Path) -> bool {
        let absolute = path.canonicalize().unwrap_or_else(|_| path.to_path_buf());
        absolute.starts_with(&self.root_dir)
    }

    pub fn stage_artifact(&self, path: impl AsRef<Path>) -> Result<StagedArtifact> {
        let path = path.as_ref();
        let absolute_path = path.canonicalize().unwrap_or_else(|_| path.to_path_buf());
        if !absolute_path.starts_with(&self.root_dir) {
            return Err(anyhow!(
                "path {} does not belong to source root {}",
                absolute_path.display(),
                self.root_dir.display()
            ));
        }
        if absolute_path.file_name().and_then(|name| name.to_str()) == Some(DIRECTORY_CONFIG_FILE) {
            return Err(anyhow!("directory config is not a staged artifact"));
        }

        let relative_path = absolute_path
            .strip_prefix(&self.root_dir)
            .unwrap_or(path)
            .to_path_buf();
        let bytes = fs::read(&absolute_path)?;
        let content_hash = blake3::hash(&bytes).to_hex().to_string();
        let locator = relative_path.display().to_string();

        Ok(StagedArtifact {
            source: self.clone(),
            artifact: Artifact {
                id: format!("artifact:{}:{content_hash}", self.source.id),
                source_id: self.source.id.clone(),
                source_kind: self.source.kind.clone(),
                kind: infer_artifact_kind(&relative_path),
                title: relative_path
                    .file_name()
                    .and_then(|value| value.to_str())
                    .map(ToOwned::to_owned),
                locator,
                media_type: infer_media_type(&absolute_path).map(ToOwned::to_owned),
                tags: self.metadata_tags.clone(),
                valid_at: None,
                observed_at: None,
            },
            absolute_path,
            relative_path,
            content_hash,
        })
    }
}

impl StagedArtifact {
    pub fn to_handler_file(&self) -> Result<IntakeFile> {
        let bytes = fs::read(&self.absolute_path)?;
        let digest = blake3::hash(&bytes);
        Ok(IntakeFile {
            sha256: *digest.as_bytes(),
            bytes: bytes.into(),
            path_hint: Some(self.relative_path.display().to_string()),
            media_type: self.artifact.media_type.clone(),
        })
    }
}

pub fn load_directory_sources(
    roots: impl IntoIterator<Item = impl AsRef<Path>>,
) -> Result<Vec<DirectorySource>> {
    roots
        .into_iter()
        .map(|root| DirectorySource::from_root(root.as_ref().to_path_buf()))
        .collect()
}

pub async fn ingest_directory_source(
    source: &DirectorySource,
    extractors: &ExtractorRegistry,
) -> Result<Vec<EvidenceBundle>> {
    let mut bundles = Vec::new();
    for path in collect_artifact_paths(&source.root_dir)? {
        let staged = source.stage_artifact(&path)?;
        if let Some(extractor) = extractors.select(&staged.artifact) {
            bundles.push(extractor.extract(&staged.artifact).await?);
        }
    }
    Ok(bundles)
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct DirectorySourceDocument {
    source: DirectorySourceSection,
    #[serde(default)]
    metadata: MetadataSection,
    #[serde(default)]
    ontologies: OntologySection,
    #[serde(default)]
    poll: PollSection,
    #[serde(default)]
    adapters: AdapterSection,
    #[serde(default)]
    errors: ErrorSection,
}

impl DirectorySourceDocument {
    fn validate(&self) -> Result<()> {
        if self.source.id.trim().is_empty() {
            return Err(anyhow!("source.id is required"));
        }
        if self.source.kind.trim().is_empty() {
            return Err(anyhow!("source.kind is required"));
        }
        if self.poll.mode == "interval" && self.poll.interval_seconds.is_none() {
            return Err(anyhow!(
                "poll.interval_seconds is required for interval mode"
            ));
        }
        Ok(())
    }
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct DirectorySourceSection {
    id: String,
    kind: String,
    locator: Option<String>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(default, deny_unknown_fields)]
struct MetadataSection {
    tags: BTreeMap<String, String>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(default, deny_unknown_fields)]
struct OntologySection {
    refs: Vec<String>,
}

#[derive(Debug, Deserialize)]
#[serde(default, deny_unknown_fields)]
struct PollSection {
    mode: String,
    interval_seconds: Option<u64>,
}

impl Default for PollSection {
    fn default() -> Self {
        Self {
            mode: "watch".to_string(),
            interval_seconds: None,
        }
    }
}

#[derive(Debug, Default, Deserialize)]
#[serde(default, deny_unknown_fields)]
struct AdapterSection {
    extractors: Vec<String>,
    executors: Vec<String>,
    error_handlers: Vec<String>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(default, deny_unknown_fields)]
struct ErrorSection {
    retry_limit: u32,
    quarantine_dir: Option<String>,
    stop_on_error: bool,
}

fn parse_source_kind(value: &str) -> SourceSystemKind {
    match value.trim().to_ascii_lowercase().as_str() {
        "git" | "git_repository" => SourceSystemKind::GitRepository,
        "sharepoint" => SourceSystemKind::SharePoint,
        "database_schema" | "schema" => SourceSystemKind::DatabaseSchema,
        "document_silo" | "documents" => SourceSystemKind::DocumentSilo,
        "process_catalog" => SourceSystemKind::ProcessCatalog,
        other => SourceSystemKind::Other(other.to_string()),
    }
}

fn infer_artifact_kind(path: &Path) -> ArtifactKind {
    let name = path
        .file_name()
        .and_then(|value| value.to_str())
        .unwrap_or_default()
        .to_ascii_lowercase();
    let extension = path
        .extension()
        .and_then(|value| value.to_str())
        .unwrap_or_default()
        .to_ascii_lowercase();

    if matches!(
        extension.as_str(),
        "rs" | "py" | "js" | "ts" | "tsx" | "java" | "go" | "kt" | "c" | "h" | "cpp"
    ) {
        ArtifactKind::SourceCode
    } else if matches!(extension.as_str(), "ppt" | "pptx" | "odp") {
        ArtifactKind::Presentation
    } else if matches!(extension.as_str(), "drawio" | "vsdx" | "mmd" | "puml") {
        ArtifactKind::Diagram
    } else if matches!(extension.as_str(), "sql" | "dbml") {
        ArtifactKind::DatabaseSchema
    } else if matches!(extension.as_str(), "csv" | "xlsx" | "xls") {
        ArtifactKind::Spreadsheet
    } else if name.contains("meeting") || name.contains("minutes") || name.contains("standup") {
        ArtifactKind::MeetingNotes
    } else if name.contains("plan") || name.contains("roadmap") {
        ArtifactKind::ProjectPlan
    } else {
        ArtifactKind::ArchitectureDocument
    }
}

fn infer_media_type(path: &Path) -> Option<&'static str> {
    match path
        .extension()
        .and_then(|value| value.to_str())
        .unwrap_or_default()
    {
        "csv" => Some("text/csv"),
        "docx" => Some("application/vnd.openxmlformats-officedocument.wordprocessingml.document"),
        "json" => Some("application/json"),
        "md" | "txt" | "toml" | "yaml" | "yml" => Some("text/plain"),
        "pdf" => Some("application/pdf"),
        "png" => Some("image/png"),
        "jpg" | "jpeg" => Some("image/jpeg"),
        "pptx" => Some("application/vnd.openxmlformats-officedocument.presentationml.presentation"),
        "rs" | "py" | "sql" => Some("text/plain"),
        _ => None,
    }
}

fn collect_artifact_paths(root: &Path) -> Result<Vec<PathBuf>> {
    let mut paths = Vec::new();
    let mut stack = vec![root.to_path_buf()];
    while let Some(current) = stack.pop() {
        for entry in fs::read_dir(&current)? {
            let entry = entry?;
            let path = entry.path();
            if path.is_dir() {
                stack.push(path);
                continue;
            }
            if path.file_name().and_then(|value| value.to_str()) == Some(DIRECTORY_CONFIG_FILE) {
                continue;
            }
            paths.push(path);
        }
    }
    Ok(paths)
}

pub async fn plan_file(
    file: &IntakeFile,
    registry: &HandlerRegistry,
    classifier: &dyn Classifier,
    naming: &dyn NamingPolicy,
) -> Result<IntakeDecision> {
    let handler = registry
        .select(file)
        .ok_or_else(|| anyhow!("no handler available"))?;
    let extraction = handler.extract(file).await?;
    let matches = classifier.classify(&extraction).await?;
    let class_match = matches.into_iter().next();

    match class_match {
        Some(class_match) => {
            let plan = naming.derive(&extraction, &class_match)?;
            let outcome = if class_match.confidence >= 0.9 {
                IntakeOutcome::Classified
            } else {
                IntakeOutcome::ClassifiedLowConfidence
            };
            Ok(IntakeDecision {
                handler_name: handler.name().to_string(),
                extraction,
                class_match: Some(class_match),
                storage_plan: Some(plan),
                outcome,
            })
        }
        None => Ok(IntakeDecision {
            handler_name: handler.name().to_string(),
            extraction,
            class_match: None,
            storage_plan: None,
            outcome: IntakeOutcome::Unclassified,
        }),
    }
}

#[cfg(test)]
mod tests {
    use std::{fs, sync::Arc};

    use anyhow::Result;
    use async_trait::async_trait;
    use bytes::Bytes;
    use classifier::{ShapeRule, ShapeRuleClassifier};
    use domain::{Artifact, ArtifactKind, EvidenceBundle, SourceSystemKind};
    use handlers::{
        Extraction, FileHandler, HandlerRegistry, HandlerScore, IntakeFile, MoneyValue,
        TemporalValue,
    };
    use naming::DslNamingPolicy;
    use serde_json::json;
    use tempfile::tempdir;

    use crate::{
        ingest_directory_source, ingest_source, load_directory_sources, plan_file,
        ArtifactConnector, ConnectorRegistry, DirectorySource, EvidenceExtractor,
        ExtractorRegistry, IntakeOutcome, PollMode, SourceSystem, DIRECTORY_CONFIG_FILE,
    };

    struct ReceiptHandler;

    #[async_trait]
    impl FileHandler for ReceiptHandler {
        fn name(&self) -> &str {
            "pdf_receipt"
        }

        fn score(&self, file: &IntakeFile) -> HandlerScore {
            if file.media_type.as_deref() == Some("application/pdf") {
                HandlerScore(0.9)
            } else {
                HandlerScore(0.0)
            }
        }

        async fn extract(&self, _file: &IntakeFile) -> Result<Extraction> {
            Ok(Extraction {
                detected_kind: "receipt".to_string(),
                text: Some("Officeworks".to_string()),
                fields: std::collections::BTreeMap::from([
                    ("vendor".to_string(), json!("Officeworks")),
                    ("date".to_string(), json!("2026-03-07")),
                    ("currency".to_string(), json!("AUD")),
                    ("total".to_string(), json!(148.95)),
                ]),
                dates: vec![TemporalValue {
                    label: "issue_date".to_string(),
                    value: "2026-03-07".to_string(),
                }],
                amounts: vec![MoneyValue {
                    label: "total".to_string(),
                    amount_minor: 14895,
                    currency: "AUD".to_string(),
                }],
                entities: vec![],
            })
        }
    }

    #[tokio::test]
    async fn plans_receipt_intake_end_to_end() {
        let registry = HandlerRegistry::new(vec![Arc::new(ReceiptHandler)]);
        let classifier = ShapeRuleClassifier::new(vec![ShapeRule {
            class: "doc:Receipt".to_string(),
            shape: "shape:ReceiptShape".to_string(),
            required_fields: vec![
                "vendor".to_string(),
                "date".to_string(),
                "total".to_string(),
            ],
            detected_kind: Some("receipt".to_string()),
        }]);
        let policy = DslNamingPolicy::new(vec![dsl::compile_rule(
            r#"
            rule receipt_naming
            when
              class == "doc:Receipt" and shape == "shape:ReceiptShape"
            then
              bucket = "finance-docs-au"
              prefix = "financial/receipt/{vendor_slug}/{yyyy}/{mm}/"
              filename = "{date}_{vendor_slug}_{total_minor}_{currency}_receipt.pdf"
              tags = { "vendor" : "{vendor_slug}" }
            "#,
        )
        .expect("rule")]);

        let decision = plan_file(
            &IntakeFile {
                sha256: [3; 32],
                bytes: Bytes::from_static(b"fake receipt"),
                path_hint: Some("receipt.pdf".to_string()),
                media_type: Some("application/pdf".to_string()),
            },
            &registry,
            &classifier,
            &policy,
        )
        .await
        .expect("plan");

        assert_eq!(decision.handler_name, "pdf_receipt");
        assert_eq!(decision.outcome, IntakeOutcome::Classified);
        assert_eq!(
            decision.storage_plan.as_ref().expect("plan").filename,
            "2026-03-07_officeworks_14895_aud_receipt.pdf"
        );
    }

    struct StaticConnector {
        name: &'static str,
        source_kind: SourceSystemKind,
        artifacts: Vec<Artifact>,
    }

    #[async_trait]
    impl ArtifactConnector for StaticConnector {
        fn name(&self) -> &str {
            self.name
        }

        fn supports(&self, source: &SourceSystem) -> bool {
            source.kind == self.source_kind
        }

        async fn list_artifacts(&self, _source: &SourceSystem) -> Result<Vec<Artifact>> {
            Ok(self.artifacts.clone())
        }
    }

    struct StaticExtractor {
        name: &'static str,
        artifact_kind: ArtifactKind,
    }

    #[async_trait]
    impl EvidenceExtractor for StaticExtractor {
        fn name(&self) -> &str {
            self.name
        }

        fn supports(&self, artifact: &Artifact) -> bool {
            artifact.kind == self.artifact_kind
        }

        async fn extract(&self, artifact: &Artifact) -> Result<EvidenceBundle> {
            Ok(EvidenceBundle {
                artifact: artifact.clone(),
                namespaces: vec![],
                anchors: vec![],
                observations: vec![],
                claims: vec![],
                concepts: vec![],
                entities: vec![],
                relations: vec![],
            })
        }
    }

    #[tokio::test]
    async fn ingests_artifacts_from_git_sharepoint_and_schema_connectors() {
        let connectors = ConnectorRegistry::new(vec![
            Arc::new(StaticConnector {
                name: "git",
                source_kind: SourceSystemKind::GitRepository,
                artifacts: vec![Artifact {
                    id: "artifact:git:src-lib".to_string(),
                    source_id: "git://repo-a".to_string(),
                    source_kind: SourceSystemKind::GitRepository,
                    kind: ArtifactKind::SourceCode,
                    title: Some("src/lib.rs".to_string()),
                    locator: "src/lib.rs".to_string(),
                    media_type: Some("text/plain".to_string()),
                    tags: std::collections::BTreeMap::new(),
                    valid_at: None,
                    observed_at: None,
                }],
            }),
            Arc::new(StaticConnector {
                name: "sharepoint",
                source_kind: SourceSystemKind::SharePoint,
                artifacts: vec![Artifact {
                    id: "artifact:sp:meeting-notes".to_string(),
                    source_id: "sharepoint://delivery".to_string(),
                    source_kind: SourceSystemKind::SharePoint,
                    kind: ArtifactKind::MeetingNotes,
                    title: Some("Standup notes".to_string()),
                    locator: "/Shared Documents/notes.docx".to_string(),
                    media_type: Some(
                        "application/vnd.openxmlformats-officedocument.wordprocessingml.document"
                            .to_string(),
                    ),
                    tags: std::collections::BTreeMap::new(),
                    valid_at: None,
                    observed_at: None,
                }],
            }),
            Arc::new(StaticConnector {
                name: "schema",
                source_kind: SourceSystemKind::DatabaseSchema,
                artifacts: vec![Artifact {
                    id: "artifact:db:schema".to_string(),
                    source_id: "jdbc://warehouse".to_string(),
                    source_kind: SourceSystemKind::DatabaseSchema,
                    kind: ArtifactKind::DatabaseSchema,
                    title: Some("core schema".to_string()),
                    locator: "warehouse.core".to_string(),
                    media_type: Some("application/sql".to_string()),
                    tags: std::collections::BTreeMap::new(),
                    valid_at: None,
                    observed_at: None,
                }],
            }),
        ]);
        let extractors = ExtractorRegistry::new(vec![
            Arc::new(StaticExtractor {
                name: "code",
                artifact_kind: ArtifactKind::SourceCode,
            }),
            Arc::new(StaticExtractor {
                name: "notes",
                artifact_kind: ArtifactKind::MeetingNotes,
            }),
            Arc::new(StaticExtractor {
                name: "schema",
                artifact_kind: ArtifactKind::DatabaseSchema,
            }),
        ]);

        let git_bundles = ingest_source(
            &SourceSystem {
                id: "git://repo-a".to_string(),
                kind: SourceSystemKind::GitRepository,
                locator: ".".to_string(),
            },
            &connectors,
            &extractors,
        )
        .await
        .expect("git ingest");
        let sharepoint_bundles = ingest_source(
            &SourceSystem {
                id: "sharepoint://delivery".to_string(),
                kind: SourceSystemKind::SharePoint,
                locator: "/Shared Documents".to_string(),
            },
            &connectors,
            &extractors,
        )
        .await
        .expect("sharepoint ingest");
        let schema_bundles = ingest_source(
            &SourceSystem {
                id: "jdbc://warehouse".to_string(),
                kind: SourceSystemKind::DatabaseSchema,
                locator: "warehouse.core".to_string(),
            },
            &connectors,
            &extractors,
        )
        .await
        .expect("schema ingest");

        assert_eq!(git_bundles.len(), 1);
        assert_eq!(sharepoint_bundles.len(), 1);
        assert_eq!(schema_bundles.len(), 1);
        assert_eq!(git_bundles[0].artifact.kind, ArtifactKind::SourceCode);
        assert_eq!(
            sharepoint_bundles[0].artifact.kind,
            ArtifactKind::MeetingNotes
        );
        assert_eq!(
            schema_bundles[0].artifact.kind,
            ArtifactKind::DatabaseSchema
        );
    }

    #[test]
    fn loads_directory_source_config_and_validates_polling() {
        let root = tempdir().expect("tempdir");
        fs::write(
            root.path().join(DIRECTORY_CONFIG_FILE),
            r#"
            [source]
            id = "sharepoint://delivery"
            kind = "sharepoint"
            locator = "/Shared Documents"

            [metadata]
            tags = { portfolio = "delivery", silo = "sharepoint" }

            [ontologies]
            refs = ["ontology://delivery", "ontology://program"]

            [poll]
            mode = "interval"
            interval_seconds = 300

            [adapters]
            extractors = ["pdf", "docx"]
            executors = ["python://curator"]
            error_handlers = ["quarantine"]

            [errors]
            retry_limit = 3
            quarantine_dir = ".quarantine"
            stop_on_error = true
            "#,
        )
        .expect("write config");

        let source = DirectorySource::from_root(root.path()).expect("load source");
        assert_eq!(source.source.id, "sharepoint://delivery");
        assert_eq!(source.source.kind, SourceSystemKind::SharePoint);
        assert_eq!(source.metadata_tags["portfolio"], "delivery");
        assert_eq!(
            source.ontology_refs,
            vec![
                "ontology://delivery".to_string(),
                "ontology://program".to_string()
            ]
        );
        assert_eq!(source.poll.mode, PollMode::Interval);
        assert_eq!(source.poll.interval_seconds, Some(300));
        assert_eq!(
            source.adapters.executors,
            vec!["python://curator".to_string()]
        );
        assert_eq!(source.error_policy.retry_limit, 3);
        assert_eq!(
            source.error_policy.quarantine_dir.as_deref(),
            Some(".quarantine")
        );
        assert!(source.error_policy.stop_on_error);
    }

    #[tokio::test]
    async fn staged_artifact_inherits_source_metadata_and_flows_through_extractor() {
        let root = tempdir().expect("tempdir");
        fs::write(
            root.path().join(DIRECTORY_CONFIG_FILE),
            r#"
            [source]
            id = "git://workspace"
            kind = "git"

            [metadata]
            tags = { domain = "payments", sensitivity = "internal" }

            [ontologies]
            refs = ["ontology://payments"]
            "#,
        )
        .expect("write config");
        let staged_path = root.path().join("roadmap.md");
        fs::write(&staged_path, "# roadmap\n").expect("write staged file");

        let source = load_directory_sources([root.path()]).expect("load sources");
        let source = source.into_iter().next().expect("one source");
        let staged = source.stage_artifact(&staged_path).expect("stage artifact");
        let handler_file = staged.to_handler_file().expect("handler file");

        assert_eq!(staged.artifact.source_id, "git://workspace");
        assert_eq!(staged.artifact.tags["domain"], "payments");
        assert_eq!(staged.relative_path.display().to_string(), "roadmap.md");
        assert_eq!(handler_file.path_hint.as_deref(), Some("roadmap.md"));
        assert_eq!(handler_file.media_type.as_deref(), Some("text/plain"));

        let bundles = ingest_directory_source(
            &source,
            &ExtractorRegistry::new(vec![Arc::new(StaticExtractor {
                name: "documents",
                artifact_kind: ArtifactKind::ProjectPlan,
            })]),
        )
        .await
        .expect("ingest directory source");

        assert_eq!(bundles.len(), 1);
        assert_eq!(bundles[0].artifact.source_id, "git://workspace");
        assert_eq!(bundles[0].artifact.tags["sensitivity"], "internal");
    }
}
