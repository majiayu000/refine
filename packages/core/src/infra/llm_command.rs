//! 命令行 LLM 通道：把 prompt 写进外部命令的 stdin，读取 stdout 作为补全。
//!
//! 用于没有 HTTP API、只有本地 agent CLI（例如 Cursor 的 `cr`）的场景。
//! 命令本身负责鉴权和模型选择；这里只做进程、超时和错误分类。

use super::llm::LlmClient;
use crate::error::{InfraError, InfraResult};
use async_trait::async_trait;
use sha2::{Digest, Sha256};
use std::ffi::OsString;
use std::path::PathBuf;
use std::process::Stdio;
use std::time::Duration;
use tokio::io::AsyncWriteExt;
use tokio::process::Command;

const DEFAULT_COMMAND_TIMEOUT: Duration = Duration::from_secs(600);
const STDERR_TAIL_CHARS: usize = 500;

pub struct CommandLlmClient {
    program: PathBuf,
    program_contents: Result<Vec<u8>, String>,
    environment: Vec<(&'static str, Option<OsString>)>,
    timeout: Duration,
}

impl CommandLlmClient {
    pub fn new(program: impl Into<PathBuf>) -> Self {
        let program = resolve_command_program(program.into(), std::env::var_os("PATH"));
        let program_contents = std::fs::read(&program).map_err(|error| error.to_string());
        Self {
            program,
            program_contents,
            environment: [
                "REFINE_CURSOR_MODEL",
                "REFINE_CURSOR_BIN",
                "REFINE_CURSOR_WORKDIR",
            ]
            .into_iter()
            .map(|key| (key, std::env::var_os(key)))
            .collect(),
            timeout: DEFAULT_COMMAND_TIMEOUT,
        }
    }

    pub fn with_timeout(mut self, timeout: Duration) -> Self {
        self.timeout = timeout;
        self
    }
}

fn resolve_command_program(program: PathBuf, search_path: Option<OsString>) -> PathBuf {
    if program.components().count() == 1 {
        search_path
            .and_then(|path| {
                std::env::split_paths(&path)
                    .map(|dir| dir.join(&program))
                    .find(|path| {
                        if !path.is_file() {
                            return false;
                        }
                        #[cfg(unix)]
                        {
                            use std::os::unix::ffi::OsStrExt;
                            std::ffi::CString::new(path.as_os_str().as_bytes()).is_ok_and(|path| {
                                // SAFETY: path is NUL-terminated and valid for this call.
                                unsafe { libc::access(path.as_ptr(), libc::X_OK) == 0 }
                            })
                        }
                        #[cfg(not(unix))]
                        {
                            true
                        }
                    })
            })
            .unwrap_or(program)
    } else {
        program
    }
}

/// The command and its descendants share this group. Dropping a request future
/// must terminate all of them, including when the caller cancels the future.
#[cfg(unix)]
struct CommandProcessGroup(u32);

#[cfg(unix)]
impl Drop for CommandProcessGroup {
    fn drop(&mut self) {
        // SAFETY: spawn created a separate group with this child's PID; a
        // negative PID addresses only that group, never our caller's group.
        unsafe {
            libc::kill(-(self.0 as libc::pid_t), libc::SIGKILL);
        }
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
        let expected_contents = self.program_contents.as_ref().map_err(|error| {
            InfraError::LlmRequest(format!(
                "无法读取 LLM 命令 {} 的缓存身份，请重建客户端后重试: {error}",
                self.program.display()
            ))
        })?;
        let mut command = Command::new(&self.program);
        for (key, value) in &self.environment {
            match value {
                Some(value) => {
                    command.env(key, value);
                }
                None => {
                    command.env_remove(key);
                }
            }
        }
        #[cfg(unix)]
        command.process_group(0);
        let current_contents = std::fs::read(&self.program).map_err(|error| {
            InfraError::LlmRequest(format!(
                "无法读取 LLM 命令 {}，请重建客户端后重试: {error}",
                self.program.display()
            ))
        })?;
        if &current_contents != expected_contents {
            return Err(InfraError::LlmRequest(format!(
                "LLM 命令 {} 的内容已变化，请重建客户端后重试",
                self.program.display()
            )));
        }
        let mut child = command
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
        #[cfg(unix)]
        let _process_group =
            CommandProcessGroup(child.id().expect("newly spawned child has a PID"));
        let mut stdin = child
            .stdin
            .take()
            .ok_or_else(|| InfraError::LlmRequest("LLM 命令缺少 stdin".into()))?;
        let input = command_input(prompt, system);
        let run = async move {
            // Feed stdin while draining output so a large prompt cannot deadlock
            // against a command that writes before reading everything.
            let writer = async move {
                let written = stdin.write_all(input.as_bytes()).await;
                // The wrapper reads until EOF before starting its agent.
                drop(stdin);
                match written {
                    // A command may exit without reading its input; its exit
                    // status and stderr explain why, so do not mask them.
                    Err(error) if error.kind() != std::io::ErrorKind::BrokenPipe => Err(error),
                    _ => Ok(()),
                }
            };
            let (written, output) = tokio::join!(writer, child.wait_with_output());
            written?;
            output
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
        let mut digest = Sha256::new();
        digest.update(self.program.as_os_str().as_encoded_bytes());
        match &self.program_contents {
            Ok(contents) => {
                digest.update(b"\0contents\0");
                digest.update(contents);
            }
            Err(error) => {
                digest.update(b"\0read-error\0");
                digest.update(error.as_bytes());
            }
        }
        for (key, value) in &self.environment {
            digest.update(b"\0");
            digest.update(key.as_bytes());
            if let Some(value) = value {
                digest.update(b"\0set\0");
                digest.update(value.as_encoded_bytes());
            } else {
                digest.update(b"\0unset");
            }
        }
        format!("command:sha256:{:x}", digest.finalize())
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

    #[test]
    fn cache_identity_changes_with_command_content_and_cursor_configuration() {
        let dir = tempfile::tempdir().unwrap();
        let path = script(&dir, "echo first");
        let first = CommandLlmClient::new(&path).cache_identity();
        script(&dir, "echo second");
        let second = CommandLlmClient::new(&path).cache_identity();
        assert_ne!(first, second);
        for key in [
            "REFINE_CURSOR_MODEL",
            "REFINE_CURSOR_BIN",
            "REFINE_CURSOR_WORKDIR",
        ] {
            let mut client = CommandLlmClient::new(&path);
            let before = client.cache_identity();
            client
                .environment
                .iter_mut()
                .find(|(name, _)| *name == key)
                .unwrap()
                .1 = Some("changed".into());
            assert_ne!(before, client.cache_identity(), "{key}");
        }
    }

    #[tokio::test]
    async fn command_uses_the_configuration_in_its_cache_identity() {
        let dir = tempfile::tempdir().unwrap();
        let mut client =
            CommandLlmClient::new(script(&dir, "printf '%s' \"$REFINE_CURSOR_MODEL\""));
        client
            .environment
            .iter_mut()
            .find(|(name, _)| *name == "REFINE_CURSOR_MODEL")
            .unwrap()
            .1 = Some("test-model".into());
        assert_eq!(client.complete("x", None).await.unwrap(), "test-model");
    }

    #[tokio::test]
    async fn unreadable_command_reports_identity_error_before_execution() {
        let dir = tempfile::tempdir().unwrap();
        let client = CommandLlmClient::new(dir.path().join("missing"));
        assert!(matches!(client.complete("x", None).await.unwrap_err(),
            InfraError::LlmRequest(message) if message.contains("缓存身份")));
    }

    #[tokio::test]
    async fn changed_command_requires_rebuilding_the_client_before_execution() {
        let dir = tempfile::tempdir().unwrap();
        let path = script(&dir, "echo first");
        let client = CommandLlmClient::new(&path);
        let identity = client.cache_identity();
        assert_eq!(client.complete("x", None).await.unwrap(), "first");
        let marker = dir.path().join("executed");
        script(&dir, &format!("touch '{}'; echo second", marker.display()));
        assert!(matches!(client.complete("x", None).await.unwrap_err(),
            InfraError::LlmRequest(message) if message.contains("重建")));
        assert!(!marker.exists(), "changed command must not execute");
        assert_eq!(identity, client.cache_identity());
        let fresh = CommandLlmClient::new(&path);
        assert_ne!(identity, fresh.cache_identity());
        assert_eq!(fresh.complete("x", None).await.unwrap(), "second");
        std::fs::remove_file(&path).unwrap();
        assert!(matches!(fresh.complete("x", None).await.unwrap_err(),
            InfraError::LlmRequest(message) if message.contains("重建")));
    }

    #[tokio::test]
    async fn bare_command_skips_non_executable_path_entry() {
        let first = tempfile::tempdir().unwrap();
        let second = tempfile::tempdir().unwrap();
        let blocked = script(&first, "echo blocked");
        std::fs::set_permissions(&blocked, std::fs::Permissions::from_mode(0o644)).unwrap();
        let executable = script(&second, "echo executable");
        let search_path = std::env::join_paths([first.path(), second.path()]).unwrap();
        let resolved = resolve_command_program("llm".into(), Some(search_path));
        assert_eq!(resolved, executable);
        let client = CommandLlmClient::new(resolved);
        assert_eq!(client.complete("x", None).await.unwrap(), "executable");
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

    async fn assert_agent_cleaned_up(cancel: bool) {
        let dir = tempfile::tempdir().unwrap();
        let agent = dir.path().join("agent");
        let pid_file = dir.path().join("agent.pid");
        std::fs::write(
            &agent,
            format!(
                r#"#!/bin/sh
sleep 60 &
printf '%s,%s' "$$" "$!" > {:?}
trap '' PIPE
while :; do
 printf '%s' '{{"type":"progress"}}
' 2>/dev/null || true
 sleep 0.02
done
"#,
                pid_file.to_str().unwrap()
            ),
        )
        .unwrap();
        std::fs::set_permissions(&agent, std::fs::Permissions::from_mode(0o755)).unwrap();
        let wrapper = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../scripts/examples/weekly-llm-cursor.py");
        let program = script(
            &dir,
            &format!(
                "export REFINE_CURSOR_BIN='{}' REFINE_CURSOR_WORKDIR='{}'
exec python3 '{}'",
                agent.display(),
                dir.path().display(),
                wrapper.display()
            ),
        );
        let client = CommandLlmClient::new(program).with_timeout(Duration::from_secs(60));
        let task = tokio::spawn(async move { client.complete("prompt", None).await });
        let pids = tokio::time::timeout(Duration::from_secs(20), async {
            loop {
                if let Ok(pid) = std::fs::read_to_string(&pid_file) {
                    if let Ok(pids) = pid
                        .split(',')
                        .map(str::parse::<u32>)
                        .collect::<Result<Vec<_>, _>>()
                    {
                        if pids.len() == 2 {
                            break pids;
                        }
                    }
                }
                tokio::time::sleep(Duration::from_millis(20)).await;
            }
        })
        .await;
        let pids = match pids {
            Ok(pids) => pids,
            Err(error) => {
                task.abort();
                panic!("fake agent starts: {error}; command={:?}", task.await);
            }
        };
        if cancel {
            task.abort();
            assert!(task.await.unwrap_err().is_cancelled());
        } else {
            // Trigger the real timeout cleanup only after both real processes
            // exist, without coupling the test to interpreter cold-start time.
            tokio::time::pause();
            tokio::time::advance(Duration::from_secs(61)).await;
            tokio::time::resume();
            assert!(matches!(
                task.await.unwrap().unwrap_err(),
                InfraError::ProcessTimeout { .. }
            ));
        }
        for pid in pids {
            let gone = tokio::time::timeout(Duration::from_secs(3), async {
                loop {
                    let status = std::process::Command::new("kill")
                        .args(["-0", &pid.to_string()])
                        .stdout(Stdio::null())
                        .stderr(Stdio::null())
                        .status()
                        .unwrap();
                    if !status.success() {
                        break;
                    }
                    tokio::time::sleep(Duration::from_millis(25)).await;
                }
            })
            .await;
            // Do not leak the intentionally reproduced orphan on the red run.
            if gone.is_err() {
                let _ = std::process::Command::new("kill")
                    .args(["-KILL", &pid.to_string()])
                    .status();
            }
            assert!(
                gone.is_ok(),
                "fake Cursor agent PID {pid} survived cleanup (cancel={cancel})"
            );
            println!("fake Cursor PID {pid} is gone after cancel={cancel}");
        }
    }

    #[tokio::test]
    async fn timeout_cleans_up_cursor_agent() {
        assert_agent_cleaned_up(false).await;
    }

    #[tokio::test]
    async fn cancellation_cleans_up_cursor_agent() {
        assert_agent_cleaned_up(true).await;
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

    #[tokio::test]
    async fn command_that_ignores_stdin_still_reports_its_stderr() {
        let dir = tempfile::tempdir().unwrap();
        let client = CommandLlmClient::new(script(&dir, "echo quota >&2; exit 3"));
        // Larger than a pipe buffer, so the write hits a closed pipe on every platform.
        let prompt = "x".repeat(1 << 20);
        let error = client.complete(&prompt, None).await.unwrap_err();
        assert!(
            matches!(error, InfraError::LlmTransport(ref message) if message.contains("quota"))
        );
    }

    #[tokio::test]
    async fn large_prompt_round_trips_without_deadlock() {
        let dir = tempfile::tempdir().unwrap();
        let client = CommandLlmClient::new(script(&dir, "cat"));
        let prompt = "y".repeat(1 << 20);
        assert_eq!(
            client.complete(&prompt, None).await.unwrap().len(),
            prompt.len()
        );
    }
}
