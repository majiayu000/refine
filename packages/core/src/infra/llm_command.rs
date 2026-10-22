//! 命令行 LLM 通道：把 prompt 写进外部命令的 stdin，读取 stdout 作为补全。
//!
//! 用于没有 HTTP API、只有本地 agent CLI（例如 Cursor 的 `cr`）的场景。
//! 命令本身负责鉴权和模型选择；这里只做进程、超时和错误分类。

use super::llm::LlmClient;
use crate::error::{InfraError, InfraResult};
use async_trait::async_trait;
use sha2::{Digest, Sha256};
use std::path::PathBuf;
use std::process::Stdio;
use std::time::Duration;
use tokio::io::AsyncWriteExt;
use tokio::process::Command;

const DEFAULT_COMMAND_TIMEOUT: Duration = Duration::from_secs(600);
const STDERR_TAIL_CHARS: usize = 500;

pub struct CommandLlmClient {
    program: PathBuf,
    timeout: Duration,
}

impl CommandLlmClient {
    pub fn new(program: impl Into<PathBuf>) -> Self {
        Self {
            program: program.into(),
            timeout: DEFAULT_COMMAND_TIMEOUT,
        }
    }

    pub fn with_timeout(mut self, timeout: Duration) -> Self {
        self.timeout = timeout;
        self
    }
}

/// System prompts have no separate channel on a CLI, so they lead the input.
fn command_input(prompt: &str, system: Option<&str>) -> String {
    match system.map(str::trim).filter(|system| !system.is_empty()) {
        Some(system) => format!("{system}\n\n---\n\n{prompt}"),
        None => prompt.to_string(),
    }
}

fn tail(text: &str, max_chars: usize) -> String {
    let chars: Vec<char> = text.trim().chars().collect();
    chars[chars.len().saturating_sub(max_chars)..]
        .iter()
        .collect()
}

#[async_trait]
impl LlmClient for CommandLlmClient {
    async fn complete(&self, prompt: &str, system: Option<&str>) -> InfraResult<String> {
        let mut child = Command::new(&self.program)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .kill_on_drop(true)
            .spawn()
            .map_err(|error| {
                InfraError::LlmRequest(format!(
                    "无法启动 LLM 命令 {}: {error}",
                    self.program.display()
                ))
            })?;
        let mut stdin = child
            .stdin
            .take()
            .ok_or_else(|| InfraError::LlmRequest("LLM 命令缺少 stdin".into()))?;
        let input = command_input(prompt, system);
        let run = async move {
            stdin.write_all(input.as_bytes()).await?;
            drop(stdin);
            child.wait_with_output().await
        };
        let output = tokio::time::timeout(self.timeout, run)
            .await
            .map_err(|_| InfraError::ProcessTimeout {
                operation: format!("llm command {}", self.program.display()),
                timeout_ms: self.timeout.as_millis() as u64,
            })?
            .map_err(|error| InfraError::LlmTransport(format!("LLM 命令 I/O 失败: {error}")))?;
        if !output.status.success() {
            // Agent CLIs fail transiently (network, rate limits); let the caller retry.
            return Err(InfraError::LlmTransport(format!(
                "LLM 命令退出 {}: {}",
                output.status,
                tail(&String::from_utf8_lossy(&output.stderr), STDERR_TAIL_CHARS)
            )));
        }
        let text = String::from_utf8(output.stdout)
            .map_err(|error| InfraError::LlmParse(format!("LLM 命令输出不是 UTF-8: {error}")))?;
        let text = text.trim();
        if text.is_empty() {
            return Err(InfraError::LlmTransport("LLM 命令输出为空".into()));
        }
        Ok(text.to_string())
    }

    fn min_request_timeout(&self) -> Option<Duration> {
        Some(self.timeout)
    }

    fn cache_identity(&self) -> String {
        let digest = Sha256::digest(self.program.to_string_lossy().as_bytes());
        format!("command:program-sha256:{digest:x}")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::fs::PermissionsExt;

    fn script(dir: &tempfile::TempDir, body: &str) -> PathBuf {
        let path = dir.path().join("llm");
        std::fs::write(&path, format!("#!/bin/sh\n{body}\n")).unwrap();
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
        path
    }

    #[tokio::test]
    async fn prompt_and_system_reach_the_command_on_stdin() {
        let dir = tempfile::tempdir().unwrap();
        let client = CommandLlmClient::new(script(&dir, "cat"));
        let reply = client.complete("问题", Some("你是教练")).await.unwrap();
        assert_eq!(reply, "你是教练\n\n---\n\n问题");
    }

    #[tokio::test]
    async fn failing_command_is_retryable_transport_error() {
        let dir = tempfile::tempdir().unwrap();
        let client = CommandLlmClient::new(script(&dir, "echo quota >&2; exit 3"));
        let error = client.complete("x", None).await.unwrap_err();
        assert!(
            matches!(error, InfraError::LlmTransport(ref message) if message.contains("quota"))
        );
    }

    #[tokio::test]
    async fn empty_output_is_an_error() {
        let dir = tempfile::tempdir().unwrap();
        let client = CommandLlmClient::new(script(&dir, "cat >/dev/null"));
        assert!(client.complete("x", None).await.is_err());
    }

    #[tokio::test]
    async fn slow_command_times_out() {
        let dir = tempfile::tempdir().unwrap();
        let client =
            CommandLlmClient::new(script(&dir, "sleep 5")).with_timeout(Duration::from_millis(200));
        let error = client.complete("x", None).await.unwrap_err();
        assert!(matches!(error, InfraError::ProcessTimeout { .. }));
    }

    #[tokio::test]
    async fn retry_layer_waits_for_a_slow_command() {
        let dir = tempfile::tempdir().unwrap();
        let client: std::sync::Arc<dyn LlmClient> =
            std::sync::Arc::new(CommandLlmClient::new(script(&dir, "sleep 0.5; echo done")));
        let reply = crate::infra::llm_with_retry_policy_for(
            &client,
            "test",
            "prompt",
            "system",
            crate::infra::LlmRetryPolicy {
                max_retries: 1,
                base_delay_secs: 0,
                request_timeout_millis: 100,
            },
            |_attempt, _max_retries, _delay_secs, _err| {},
        )
        .await
        .expect("command timeout must override the shorter policy timeout");
        assert_eq!(reply, "done");
    }
}
