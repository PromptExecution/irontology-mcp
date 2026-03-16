use std::{env, net::TcpListener, path::PathBuf, time::Duration};

use provider_api::{ChatMessage, ModelProvider};
use provider_local::{LocalProvider, LocalProviderConfig, MistralRsConfig};
use tokio::time::sleep;

#[tokio::test]
async fn managed_provider_spawns_and_serves_requests() {
    let provider = LocalProvider::new(LocalProviderConfig::managed_mistral_rs(mock_config(None)));

    let health = provider.health().await.expect("health");
    assert!(health.healthy);
    assert!(provider.managed_process_id().await.is_some());

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

    assert_eq!(chat.content, "managed response");
    provider.shutdown().await.expect("shutdown");
}

#[tokio::test]
async fn managed_provider_restarts_after_shutdown() {
    let provider = LocalProvider::new(LocalProviderConfig::managed_mistral_rs(mock_config(None)));

    provider.health().await.expect("health");
    let first_pid = provider.managed_process_id().await.expect("pid");
    provider.shutdown().await.expect("shutdown");
    assert!(provider.managed_process_id().await.is_none());

    provider.health().await.expect("restart health");
    let second_pid = provider.managed_process_id().await.expect("restarted pid");

    assert_ne!(first_pid, second_pid);
    provider.shutdown().await.expect("shutdown");
}

#[tokio::test]
async fn managed_provider_recovers_after_process_exit() {
    let provider = LocalProvider::new(LocalProviderConfig::managed_mistral_rs(mock_config(Some(
        Duration::from_millis(300),
    ))));

    provider.health().await.expect("health");
    let first_pid = provider.managed_process_id().await.expect("pid");

    sleep(Duration::from_millis(700)).await;

    let embed = provider
        .embed(provider_api::EmbedRequest {
            model: String::new(),
            inputs: vec!["ping".to_string()],
            batch_size: 1,
        })
        .await
        .expect("embed after restart");
    let second_pid = provider.managed_process_id().await.expect("restarted pid");

    assert_eq!(&*embed.vectors[0], &[0.5, 0.5]);
    assert_ne!(first_pid, second_pid);
    provider.shutdown().await.expect("shutdown");
}

fn mock_config(lifetime: Option<Duration>) -> MistralRsConfig {
    let mut config = MistralRsConfig::new(free_port(), "mock-model", "/models/mock.Q4_K_M.gguf");
    config.program = mock_binary_path().to_string_lossy().into_owned();
    if let Some(lifetime) = lifetime {
        config.extra_args.extend([
            "--lifetime-ms".to_string(),
            lifetime.as_millis().to_string(),
        ]);
    }
    config.startup_timeout = Duration::from_secs(5);
    config.health_poll_interval = Duration::from_millis(50);
    config.max_restart_attempts = 3;
    config
}

fn mock_binary_path() -> PathBuf {
    if let Some(path) = env::var_os("CARGO_BIN_EXE_mock_mistralrs_server") {
        return PathBuf::from(path);
    }

    let mut path = env::current_exe().expect("current test binary path");
    path.pop();
    if path.ends_with("deps") {
        path.pop();
    }
    path.push(format!("mock_mistralrs_server{}", env::consts::EXE_SUFFIX));
    assert!(path.exists(), "mock binary path: {}", path.display());
    path
}

fn free_port() -> u16 {
    TcpListener::bind("127.0.0.1:0")
        .expect("bind")
        .local_addr()
        .expect("addr")
        .port()
}
