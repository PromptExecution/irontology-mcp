use std::{
    collections::BTreeMap,
    fs,
    path::{Path, PathBuf},
};

use anyhow::Result;
use serde::Deserialize;
use storage_neumann::config::NeumannConfig;

pub const DEFAULT_CONFIG_FILE: &str = "phase2d.toml";

#[derive(Debug, Clone)]
pub struct LoadedPhase2dConfig {
    pub path: Option<PathBuf>,
    pub config: Phase2dConfig,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Phase2dConfig {
    pub server: ServerSettings,
    pub neumann: NeumannSettings,
    pub provider: ProviderSettings,
    pub forwarding: ForwardingSettings,
    pub sources: Vec<SourceRootConfig>,
    pub registries: RegistrySettings,
}

impl Default for Phase2dConfig {
    fn default() -> Self {
        Self {
            server: ServerSettings::default(),
            neumann: NeumannSettings::default(),
            provider: ProviderSettings::default(),
            forwarding: ForwardingSettings::default(),
            sources: Vec::new(),
            registries: RegistrySettings::default(),
        }
    }
}

#[derive(Debug, Clone, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct ServerSettings {
    pub http_addr: String,
}

impl Default for ServerSettings {
    fn default() -> Self {
        Self {
            http_addr: "127.0.0.1:3000".to_string(),
        }
    }
}

#[derive(Debug, Clone, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct NeumannSettings {
    pub endpoint: String,
    pub namespace: String,
}

impl Default for NeumannSettings {
    fn default() -> Self {
        let defaults = NeumannConfig::default();
        Self {
            endpoint: defaults.endpoint,
            namespace: defaults.namespace,
        }
    }
}

impl From<NeumannSettings> for NeumannConfig {
    fn from(value: NeumannSettings) -> Self {
        Self {
            endpoint: value.endpoint,
            namespace: value.namespace,
        }
    }
}

#[derive(Debug, Clone, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct ForwardingSettings {
    pub transport_forwarding: bool,
}

impl Default for ForwardingSettings {
    fn default() -> Self {
        Self {
            transport_forwarding: true,
        }
    }
}

#[derive(Debug, Clone, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ProviderSettings {
    Fixture {
        model_id: String,
        chat_content: String,
        embedding_dim: usize,
    },
    OpenAi {
        base_url: String,
        model_id: String,
        api_key: Option<String>,
        retries: u8,
        retry_backoff_ms: u64,
    },
    Local {
        model_id: String,
        base_url: Option<String>,
        api_key: Option<String>,
        managed_program: Option<String>,
        managed_host: String,
        managed_port: Option<u16>,
        managed_model: Option<String>,
        extra_args: Vec<String>,
    },
}

impl Default for ProviderSettings {
    fn default() -> Self {
        Self::Fixture {
            model_id: "fixture-embed".to_string(),
            chat_content: "fixture response".to_string(),
            embedding_dim: 4,
        }
    }
}

#[derive(Debug, Clone, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct SourceRootConfig {
    pub root: String,
    pub enabled: bool,
    pub rhai_modules: Vec<String>,
}

impl Default for SourceRootConfig {
    fn default() -> Self {
        Self {
            root: String::new(),
            enabled: true,
            rhai_modules: Vec::new(),
        }
    }
}

#[derive(Debug, Clone, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct RegistrySettings {
    pub extractors: Vec<ExtractorRegistration>,
    pub executors: Vec<ExecutorRegistration>,
    pub rhai_modules: Vec<RhaiModuleRegistration>,
}

impl Default for RegistrySettings {
    fn default() -> Self {
        Self {
            extractors: vec![ExtractorRegistration {
                name: "text".to_string(),
                kind: ExtractorKind::BuiltinText,
            }],
            executors: Vec::new(),
            rhai_modules: Vec::new(),
        }
    }
}

#[derive(Debug, Clone, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct ExtractorRegistration {
    pub name: String,
    pub kind: ExtractorKind,
}

impl Default for ExtractorRegistration {
    fn default() -> Self {
        Self {
            name: "text".to_string(),
            kind: ExtractorKind::BuiltinText,
        }
    }
}

#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ExtractorKind {
    BuiltinText,
}

#[derive(Debug, Clone, Deserialize, Default)]
#[serde(default, deny_unknown_fields)]
pub struct ExecutorRegistration {
    pub name: String,
    pub target: String,
}

#[derive(Debug, Clone, Deserialize, Default)]
#[serde(default, deny_unknown_fields)]
pub struct RhaiModuleRegistration {
    pub name: String,
    pub path: String,
}

impl Phase2dConfig {
    pub fn source_roots(&self) -> Vec<String> {
        self.sources
            .iter()
            .filter(|source| source.enabled && !source.root.trim().is_empty())
            .map(|source| source.root.clone())
            .collect()
    }

    pub fn source_modules(&self) -> BTreeMap<String, Vec<String>> {
        self.sources
            .iter()
            .filter(|source| source.enabled && !source.root.trim().is_empty())
            .map(|source| (source.root.clone(), source.rhai_modules.clone()))
            .collect()
    }
}

pub fn load_phase2d_config(path: Option<&Path>) -> Result<LoadedPhase2dConfig> {
    let explicit = path.map(Path::to_path_buf);
    let candidate = explicit.or_else(default_config_path);

    if let Some(path) = candidate {
        let body = fs::read_to_string(&path)?;
        let config = toml::from_str(&body)?;
        return Ok(LoadedPhase2dConfig {
            path: Some(path),
            config,
        });
    }

    Ok(LoadedPhase2dConfig {
        path: None,
        config: Phase2dConfig::default(),
    })
}

pub fn resolve_config_relative(path: &str, config_path: Option<&Path>) -> PathBuf {
    let path = PathBuf::from(path);
    if path.is_absolute() {
        return path;
    }
    if let Some(config_path) = config_path {
        if let Some(parent) = config_path.parent() {
            return parent.join(path);
        }
    }
    path
}

fn default_config_path() -> Option<PathBuf> {
    let candidate = PathBuf::from(DEFAULT_CONFIG_FILE);
    candidate.is_file().then_some(candidate)
}

#[cfg(test)]
mod tests {
    use std::fs;

    use tempfile::tempdir;

    use crate::config::{load_phase2d_config, resolve_config_relative, ProviderSettings};

    #[test]
    fn loads_runtime_config_with_sources_and_registries() {
        let root = tempdir().expect("tempdir");
        let config_path = root.path().join("phase2d.toml");
        fs::write(
            &config_path,
            r#"
            [server]
            http_addr = "127.0.0.1:4100"

            [neumann]
            endpoint = "http://localhost:7777"
            namespace = "acme"

            [provider]
            kind = "fixture"
            model_id = "fixture-acme"
            chat_content = "demo"
            embedding_dim = 8

            [forwarding]
            transport_forwarding = false

            [[sources]]
            root = "examples/acme-corp/repo"
            rhai_modules = ["latent_dependencies"]

            [[registries.extractors]]
            name = "docs"
            kind = "builtin_text"

            [[registries.executors]]
            name = "python_curator"
            target = "stdio://child:python-worker"

            [[registries.rhai_modules]]
            name = "latent_dependencies"
            path = "rhai/latent_dependencies.rhai"
            "#,
        )
        .expect("write config");

        let loaded = load_phase2d_config(Some(&config_path)).expect("load config");
        assert_eq!(loaded.config.server.http_addr, "127.0.0.1:4100");
        assert_eq!(
            loaded.config.source_roots(),
            vec!["examples/acme-corp/repo"]
        );
        assert_eq!(
            loaded.config.source_modules()["examples/acme-corp/repo"],
            vec!["latent_dependencies".to_string()]
        );
        assert_eq!(
            loaded.config.registries.executors[0].target,
            "stdio://child:python-worker"
        );
        match loaded.config.provider {
            ProviderSettings::Fixture { model_id, .. } => assert_eq!(model_id, "fixture-acme"),
            other => panic!("unexpected provider: {other:?}"),
        }
    }

    #[test]
    fn resolves_paths_relative_to_runtime_config() {
        let root = tempdir().expect("tempdir");
        let config_path = root.path().join("phase2d.toml");
        let resolved = resolve_config_relative("rhai/acme.rhai", Some(&config_path));
        assert_eq!(resolved, root.path().join("rhai/acme.rhai"));
    }
}
