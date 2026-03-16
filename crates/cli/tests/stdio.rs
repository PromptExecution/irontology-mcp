use std::{path::PathBuf, process::Stdio};

use serde_json::json;
use tokio::{
    io::{AsyncBufReadExt, AsyncWriteExt, BufReader},
    process::Command,
};

#[tokio::test]
async fn cli_stdio_serves_tools_list() {
    let binary = binary_path("phase2d");
    let mut child = Command::new(binary)
        .arg("stdio")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .expect("spawn phase2d");

    let mut stdin = child.stdin.take().expect("stdin");
    let stdout = child.stdout.take().expect("stdout");
    let request = json!({
        "jsonrpc": "2.0",
        "id": 1,
        "method": "tools/list",
        "params": {}
    });

    stdin
        .write_all(format!("{request}\n").as_bytes())
        .await
        .expect("write request");
    stdin.shutdown().await.expect("close stdin");
    drop(stdin);

    let mut lines = BufReader::new(stdout).lines();
    let response = lines
        .next_line()
        .await
        .expect("read line")
        .expect("response");
    let json: serde_json::Value = serde_json::from_str(&response).expect("parse response");

    assert!(json["result"]["tools"]
        .as_array()
        .expect("tool array")
        .iter()
        .any(|tool| tool["name"] == "repo.search"));

    let status = tokio::time::timeout(std::time::Duration::from_secs(10), child.wait())
        .await
        .expect("child exit timeout")
        .expect("wait for child");
    assert!(status.success());
}

#[tokio::test]
async fn cli_stdio_loads_demo_runtime_config() {
    let binary = binary_path("phase2d");
    let config = workspace_root().join("examples/acme-corp/phase2d.toml");
    let mut child = Command::new(binary)
        .arg("stdio")
        .arg("--config")
        .arg(&config)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .expect("spawn phase2d");

    let mut stdin = child.stdin.take().expect("stdin");
    let stdout = child.stdout.take().expect("stdout");
    let request = json!({
        "jsonrpc": "2.0",
        "id": 2,
        "method": "resources/list",
        "params": {}
    });

    stdin
        .write_all(format!("{request}\n").as_bytes())
        .await
        .expect("write request");
    stdin.shutdown().await.expect("close stdin");
    drop(stdin);

    let mut lines = BufReader::new(stdout).lines();
    let response = lines
        .next_line()
        .await
        .expect("read line")
        .expect("response");
    let json: serde_json::Value = serde_json::from_str(&response).expect("parse response");

    assert!(json["result"]["resources"]
        .as_array()
        .expect("resources array")
        .iter()
        .any(|resource| resource["uri"] == "ontology://naming_conventions"));

    let status = tokio::time::timeout(std::time::Duration::from_secs(10), child.wait())
        .await
        .expect("child exit timeout")
        .expect("wait for child");
    assert!(status.success());
}

#[tokio::test]
async fn cli_stdio_runs_agent_executor() {
    let binary = binary_path("phase2d");
    let config = workspace_root().join("examples/acme-corp/phase2d.toml");
    let mut child = Command::new(binary)
        .arg("stdio")
        .arg("--config")
        .arg(&config)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .expect("spawn phase2d");

    let mut stdin = child.stdin.take().expect("stdin");
    let stdout = child.stdout.take().expect("stdout");
    let request = json!({
        "jsonrpc": "2.0",
        "id": 3,
        "method": "tools/call",
        "params": {
            "name": "agent.run",
            "arguments": {
                "task": "Find latent dependencies across the Acme corpus",
                "model": "fixture-acme",
                "max_turns": 4,
                "budget_tokens": 8000,
                "context": ["repo://tree", "ontology://classes"]
            }
        }
    });

    stdin
        .write_all(format!("{request}\n").as_bytes())
        .await
        .expect("write request");
    stdin.shutdown().await.expect("close stdin");
    drop(stdin);

    let mut lines = BufReader::new(stdout).lines();
    let response = lines
        .next_line()
        .await
        .expect("read line")
        .expect("response");
    let json: serde_json::Value = serde_json::from_str(&response).expect("parse response");

    assert_eq!(
        json["result"]["content"][0]["json"]["answer"],
        "acme fixture response"
    );
    assert!(!json["result"]["content"][0]["json"]["run_id"]
        .as_str()
        .expect("run id")
        .is_empty());

    let status = tokio::time::timeout(std::time::Duration::from_secs(10), child.wait())
        .await
        .expect("child exit timeout")
        .expect("wait for child");
    assert!(status.success());
}

fn binary_path(name: &str) -> PathBuf {
    if let Ok(path) = std::env::var(format!("CARGO_BIN_EXE_{name}")) {
        return PathBuf::from(path);
    }

    std::env::current_exe()
        .expect("current exe")
        .parent()
        .and_then(|dir| dir.parent())
        .map(|dir| dir.join(name))
        .expect("derive cargo bin path")
}

fn workspace_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(|dir| dir.parent())
        .expect("workspace root")
        .to_path_buf()
}
