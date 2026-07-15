use std::path::PathBuf;

#[derive(Debug, Clone)]
pub struct NeumannConfig {
    pub endpoint: String,
    pub namespace: String,
    /// Optional path for sled persistence. `None` = in-memory only (data lost on restart).
    /// b00t convention: use an absolute, expanded path, e.g. `/home/alice/.b00t/neumann/{namespace}/`.
    /// Note: `~` is not expanded automatically when read from env/config; expand it before setting.
    pub data_path: Option<PathBuf>,
}

impl Default for NeumannConfig {
    fn default() -> Self {
        Self {
            endpoint: "http://localhost:5273".to_string(),
            namespace: "default".to_string(),
            data_path: None,
        }
    }
}
