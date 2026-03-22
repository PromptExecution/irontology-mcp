#[derive(Debug, Clone)]
pub struct NeumannConfig {
    pub endpoint: String,
    pub namespace: String,
    /// Optional path for sled persistence. None = in-memory only (data lost on restart).
    /// b00t convention: ~/.b00t/neumann/{namespace}/
    pub data_dir: Option<String>,
}

impl Default for NeumannConfig {
    fn default() -> Self {
        Self {
            endpoint: "http://localhost:7777".to_string(),
            namespace: "default".to_string(),
            data_dir: None,
        }
    }
}
