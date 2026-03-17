use std::{
    collections::BTreeMap,
    path::PathBuf,
    process::Stdio,
    sync::Arc,
    time::{Duration, Instant},
};

use anyhow::{anyhow, Result};
use async_trait::async_trait;
use provider_api::{
    ChatRequest, ChatResponse, EmbedRequest, EmbedResponse, ModelProvider, ProviderHealth,
};
use provider_openai::{OpenAiCompatConfig, OpenAiCompatProvider};
use tokio::{
    process::{Child, Command},
    sync::Mutex,
    time::sleep,
};

#[derive(Debug, Clone)]
pub struct LocalCommand {
    pub program: String,
    pub args: Vec<String>,
    pub env: BTreeMap<String, String>,
    pub current_dir: Option<PathBuf>,
}

impl LocalCommand {
    pub fn new(program: impl Into<String>) -> Self {
        Self {
            program: program.into(),
            args: Vec::new(),
            env: BTreeMap::new(),
            current_dir: None,
        }
    }

    pub fn with_args(mut self, args: impl IntoIterator<Item = impl Into<String>>) -> Self {
        self.args = args.into_iter().map(Into::into).collect();
        self
    }

    pub fn push_arg(mut self, arg: impl Into<String>) -> Self {
        self.args.push(arg.into());
        self
    }

    pub fn with_env(mut self, key: impl Into<String>, value: impl Into<String>) -> Self {
        self.env.insert(key.into(), value.into());
        self
    }
}

#[derive(Debug, Clone)]
pub struct ManagedLocalModel {
    pub base_url: String,
    pub api_key: Option<String>,
    pub command: LocalCommand,
    pub startup_timeout: Duration,
    pub health_poll_interval: Duration,
    pub max_restart_attempts: usize,
}

impl ManagedLocalModel {
    pub fn new(base_url: impl Into<String>, command: LocalCommand) -> Self {
        Self {
            base_url: base_url.into(),
            api_key: None,
            command,
            startup_timeout: Duration::from_secs(30),
            health_poll_interval: Duration::from_millis(200),
            max_restart_attempts: 2,
        }
    }
}

#[derive(Debug, Clone)]
pub struct MistralRsConfig {
    pub program: String,
    pub host: String,
    pub port: u16,
    pub model_id: String,
    pub model: String,
    pub api_key: Option<String>,
    pub extra_args: Vec<String>,
    pub startup_timeout: Duration,
    pub health_poll_interval: Duration,
    pub max_restart_attempts: usize,
}

impl MistralRsConfig {
    pub fn new(port: u16, model_id: impl Into<String>, model: impl Into<String>) -> Self {
        Self {
            program: "mistralrs-server".to_string(),
            host: "127.0.0.1".to_string(),
            port,
            model_id: model_id.into(),
            model: model.into(),
            api_key: None,
            extra_args: Vec::new(),
            startup_timeout: Duration::from_secs(30),
            health_poll_interval: Duration::from_millis(200),
            max_restart_attempts: 2,
        }
    }

    pub fn base_url(&self) -> String {
        format!("http://{}:{}", self.host, self.port)
    }

    pub fn command(&self) -> LocalCommand {
        let mut args = vec![
            "--port".to_string(),
            self.port.to_string(),
            "run".to_string(),
            "-m".to_string(),
            self.model.clone(),
        ];
        args.extend(self.extra_args.clone());
        LocalCommand::new(self.program.clone()).with_args(args)
    }

    pub fn into_managed(self) -> ManagedLocalModel {
        let mut managed = ManagedLocalModel::new(self.base_url(), self.command());
        managed.api_key = self.api_key;
        managed.startup_timeout = self.startup_timeout;
        managed.health_poll_interval = self.health_poll_interval;
        managed.max_restart_attempts = self.max_restart_attempts;
        managed
    }
}

#[derive(Debug, Clone)]
pub struct LocalProviderConfig {
    pub model_id: String,
    pub base_url: String,
    pub api_key: Option<String>,
    pub managed: Option<ManagedLocalModel>,
}

impl LocalProviderConfig {
    pub fn new(base_url: impl Into<String>, model_id: impl Into<String>) -> Self {
        Self {
            model_id: model_id.into(),
            base_url: base_url.into(),
            api_key: None,
            managed: None,
        }
    }

    pub fn managed_mistral_rs(config: MistralRsConfig) -> Self {
        Self {
            model_id: config.model_id.clone(),
            base_url: config.base_url(),
            api_key: config.api_key.clone(),
            managed: Some(config.into_managed()),
        }
    }

    pub fn with_managed_command(mut self, managed: ManagedLocalModel) -> Self {
        self.base_url = managed.base_url.clone();
        self.api_key = managed.api_key.clone();
        self.managed = Some(managed);
        self
    }

    pub fn with_api_key(mut self, api_key: impl Into<String>) -> Self {
        self.api_key = Some(api_key.into());
        self
    }
}

struct ManagedChild {
    child: Child,
}

pub struct LocalModelManager {
    managed: Option<ManagedLocalModel>,
    probe: OpenAiCompatProvider,
    child: Mutex<Option<ManagedChild>>,
}

impl LocalModelManager {
    pub fn new(config: &LocalProviderConfig) -> Self {
        let mut probe = OpenAiCompatConfig::new(config.base_url.clone(), config.model_id.clone());
        probe.api_key = config.api_key.clone();

        Self {
            managed: config.managed.clone(),
            probe: OpenAiCompatProvider::new(probe),
            child: Mutex::new(None),
        }
    }

    pub async fn ensure_ready(&self) -> Result<()> {
        let Some(managed) = &self.managed else {
            return Ok(());
        };

        let attempts = managed.max_restart_attempts.max(1);
        for _ in 0..attempts {
            self.ensure_process_running().await?;
            match self.wait_until_ready().await {
                Ok(()) => return Ok(()),
                Err(err) => {
                    if self.process_exited().await? {
                        continue;
                    }
                    return Err(err);
                }
            }
        }

        Err(anyhow!("managed local model failed to become ready"))
    }

    pub async fn restart(&self) -> Result<()> {
        self.shutdown().await?;
        self.ensure_ready().await
    }

    pub async fn shutdown(&self) -> Result<()> {
        let mut slot = self.child.lock().await;
        if let Some(mut managed) = slot.take() {
            let _ = managed.child.kill().await;
            let _ = managed.child.wait().await;
        }
        Ok(())
    }

    pub async fn managed_process_id(&self) -> Option<u32> {
        let slot = self.child.lock().await;
        slot.as_ref().and_then(|managed| managed.child.id())
    }

    async fn ensure_process_running(&self) -> Result<()> {
        let Some(managed) = &self.managed else {
            return Ok(());
        };

        let mut slot = self.child.lock().await;
        if let Some(existing) = slot.as_mut() {
            if existing.child.try_wait()?.is_none() {
                return Ok(());
            }
            *slot = None;
        }

        let mut command = Command::new(&managed.command.program);
        command.args(&managed.command.args);
        command.stdout(Stdio::null());
        command.stderr(Stdio::null());
        if let Some(current_dir) = &managed.command.current_dir {
            command.current_dir(current_dir);
        }
        for (key, value) in &managed.command.env {
            command.env(key, value);
        }

        let child = command.spawn()?;
        *slot = Some(ManagedChild { child });
        Ok(())
    }

    async fn wait_until_ready(&self) -> Result<()> {
        let Some(managed) = &self.managed else {
            return Ok(());
        };

        let deadline = Instant::now() + managed.startup_timeout;
        loop {
            if let Ok(health) = self.probe.health().await {
                if health.healthy {
                    return Ok(());
                }
            }

            if self.process_exited().await? {
                return Err(anyhow!("managed local model exited before it became ready"));
            }

            if Instant::now() >= deadline {
                return Err(anyhow!(
                    "managed local model did not become ready within {:?}",
                    managed.startup_timeout
                ));
            }

            sleep(managed.health_poll_interval).await;
        }
    }

    async fn process_exited(&self) -> Result<bool> {
        let mut slot = self.child.lock().await;
        let Some(managed) = slot.as_mut() else {
            return Ok(false);
        };

        if managed.child.try_wait()?.is_some() {
            *slot = None;
            Ok(true)
        } else {
            Ok(false)
        }
    }
}

pub struct LocalProvider {
    inner: OpenAiCompatProvider,
    manager: Arc<LocalModelManager>,
    model_id: String,
}

impl LocalProvider {
    pub fn new(config: LocalProviderConfig) -> Self {
        let mut openai = OpenAiCompatConfig::new(config.base_url.clone(), config.model_id.clone());
        openai.api_key = config.api_key.clone();

        Self {
            inner: OpenAiCompatProvider::new(openai),
            manager: Arc::new(LocalModelManager::new(&config)),
            model_id: config.model_id,
        }
    }

    pub async fn restart(&self) -> Result<()> {
        self.manager.restart().await
    }

    pub async fn shutdown(&self) -> Result<()> {
        self.manager.shutdown().await
    }

    pub async fn managed_process_id(&self) -> Option<u32> {
        self.manager.managed_process_id().await
    }
}

#[async_trait]
impl ModelProvider for LocalProvider {
    async fn chat(&self, req: ChatRequest) -> Result<ChatResponse> {
        self.manager.ensure_ready().await?;
        match self.inner.chat(req.clone()).await {
            Ok(response) => Ok(response),
            Err(err) => {
                if self.manager.process_exited().await? {
                    self.manager.ensure_ready().await?;
                    return self.inner.chat(req).await;
                }
                Err(err)
            }
        }
    }

    async fn embed(&self, req: EmbedRequest) -> Result<EmbedResponse> {
        self.manager.ensure_ready().await?;
        match self.inner.embed(req.clone()).await {
            Ok(response) => Ok(response),
            Err(err) => {
                if self.manager.process_exited().await? {
                    self.manager.ensure_ready().await?;
                    return self.inner.embed(req).await;
                }
                Err(err)
            }
        }
    }

    async fn health(&self) -> Result<ProviderHealth> {
        self.manager.ensure_ready().await?;
        self.inner.health().await
    }

    fn model_id(&self) -> &str {
        &self.model_id
    }
}

#[cfg(test)]
mod tests {
    use std::net::SocketAddr;

    use axum::{http::StatusCode, response::IntoResponse, routing::get, Json, Router};
    use provider_api::{ChatMessage, ModelProvider};
    use serde_json::json;
    use tokio::net::TcpListener;

    use crate::{LocalProvider, LocalProviderConfig};

    #[tokio::test]
    async fn local_provider_health_check() {
        let base_url = spawn_local_server().await;
        let provider = LocalProvider::new(LocalProviderConfig::new(base_url, "local-code"));

        let health = provider.health().await.expect("health");
        assert!(health.healthy);
    }

    #[tokio::test]
    async fn local_provider_delegates_chat_and_embed() {
        let base_url = spawn_local_server().await;
        let provider = LocalProvider::new(LocalProviderConfig::new(base_url, "local-code"));

        let chat = provider
            .chat(provider_api::ChatRequest {
                model: String::new(),
                messages: vec![ChatMessage {
                    role: "user".to_string(),
                    content: "ping".to_string(),
                }],
                max_tokens: None,
                stream: false,
                params: Default::default(),
            })
            .await
            .expect("chat");
        let embed = provider
            .embed(provider_api::EmbedRequest {
                model: String::new(),
                inputs: vec!["ping".to_string()],
                batch_size: 1,
            })
            .await
            .expect("embed");

        assert_eq!(chat.content, "local response");
        assert_eq!(&*embed.vectors[0], &[0.25, 0.75]);
    }

    async fn spawn_local_server() -> String {
        let app = Router::new()
            .route("/health", get(|| async { StatusCode::OK }))
            .route(
                "/v1/chat/completions",
                axum::routing::post(|| async {
                    (
                        StatusCode::OK,
                        Json(json!({
                            "model": "local-code",
                            "choices": [{ "message": { "content": "local response" } }],
                            "usage": { "prompt_tokens": 1, "completion_tokens": 1, "total_tokens": 2 }
                        })),
                    )
                }),
            )
            .route(
                "/v1/embeddings",
                axum::routing::post(|| async {
                    (
                        StatusCode::OK,
                        Json(json!({
                            "model": "local-code",
                            "data": [{ "embedding": [0.25, 0.75] }],
                            "usage": { "prompt_tokens": 1, "completion_tokens": 0, "total_tokens": 1 }
                        })),
                    )
                }),
            )
            .route("/v1/models", get(models));

        let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind");
        let addr: SocketAddr = listener.local_addr().expect("addr");
        tokio::spawn(async move {
            axum::serve(listener, app).await.expect("server");
        });
        format!("http://{addr}")
    }

    async fn models() -> impl IntoResponse {
        Json(json!({ "data": [{ "id": "local-code" }] }))
    }
}
