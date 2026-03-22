use std::path::PathBuf;

#[derive(Debug, Clone)]
pub struct NeumannConfig {
    pub endpoint: String,
    pub namespace: String,
    pub data_path: Option<PathBuf>,
}

impl Default for NeumannConfig {
    fn default() -> Self {
        Self {
            endpoint: "http://localhost:7777".to_string(),
            namespace: "default".to_string(),
            data_path: None,
        }
    }
}
