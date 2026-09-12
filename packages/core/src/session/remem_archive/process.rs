use super::{CommandResult, Runner};
use crate::error::InfraError;
use anyhow::{Context, Result};
use std::io::{self, Read};
use std::process::{Command, Stdio};
use std::sync::mpsc;
use std::thread;
use std::time::{Duration, Instant};

const DEFAULT_PROCESS_TIMEOUT: Duration = Duration::from_secs(30);
const DEFAULT_MAX_CAPTURE_BYTES: usize = 16 * 1024 * 1024;
const PROCESS_POLL_INTERVAL: Duration = Duration::from_millis(20);

pub(super) struct ProcessRunner;

pub fn is_missing_remem_executable(error: &anyhow::Error) -> bool {
    error.chain().any(|cause| {
        cause
            .downcast_ref::<io::Error>()
            .is_some_and(|error| error.kind() == io::ErrorKind::NotFound)
    })
}

pub fn is_remem_process_timeout(error: &anyhow::Error) -> bool {
    error.chain().any(|cause| {
        matches!(
            cause.downcast_ref::<InfraError>(),
            Some(InfraError::ProcessTimeout { .. })
        )
    })
}

pub fn is_remem_process_output_overflow(error: &anyhow::Error) -> bool {
    error.chain().any(|cause| {
        matches!(
            cause.downcast_ref::<InfraError>(),
            Some(InfraError::ProcessOutputOverflow { .. })
        )
    })
}

/// Prefer typed process-guard failures when formatting Remem hydration errors.
pub fn remem_hydration_failure_message(error: &anyhow::Error) -> String {
    for cause in error.chain() {
        if let Some(infra) = cause.downcast_ref::<InfraError>() {
            match infra {
                InfraError::ProcessTimeout { .. } | InfraError::ProcessOutputOverflow { .. } => {
                    return infra.to_string();
                }
                _ => {}
            }
        }
    }
    format!("Remem hydration failed: {error}")
}

fn remem_process_timeout() -> Duration {
    parse_positive_u64_env("REFINE_REMEM_TIMEOUT_MS")
        .map(Duration::from_millis)
        .unwrap_or(DEFAULT_PROCESS_TIMEOUT)
}

fn remem_max_capture_bytes() -> usize {
    parse_positive_u64_env("REFINE_REMEM_MAX_OUTPUT_BYTES")
        .and_then(|value| usize::try_from(value).ok())
        .filter(|value| *value > 0)
        .unwrap_or(DEFAULT_MAX_CAPTURE_BYTES)
}

fn parse_positive_u64_env(name: &str) -> Option<u64> {
    std::env::var(name)
        .ok()
        .and_then(|value| value.trim().parse::<u64>().ok())
        .filter(|value| *value > 0)
}

#[derive(Debug)]
enum CaptureOutcome {
    Bytes(Vec<u8>),
    Overflow,
    Io(io::Error),
}

fn read_bounded(mut reader: impl Read, limit: usize) -> CaptureOutcome {
    let mut buffer = Vec::new();
    let mut chunk = [0_u8; 8 * 1024];
    loop {
        match reader.read(&mut chunk) {
            Ok(0) => return CaptureOutcome::Bytes(buffer),
            Ok(n) => {
                if buffer.len().saturating_add(n) > limit {
                    return CaptureOutcome::Overflow;
                }
                buffer.extend_from_slice(&chunk[..n]);
            }
            Err(error) if error.kind() == io::ErrorKind::Interrupted => continue,
            Err(error) => return CaptureOutcome::Io(error),
        }
    }
}

impl Runner for ProcessRunner {
    fn run(&self, args: &[String]) -> Result<CommandResult> {
        let binary = std::env::var("REFINE_REMEM_BIN")
            .ok()
            .filter(|value| !value.trim().is_empty())
            .unwrap_or_else(|| "remem".to_string());
        let timeout = remem_process_timeout();
        let max_capture_bytes = remem_max_capture_bytes();
        let mut child = Command::new(&binary)
            .args(args)
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .with_context(|| format!("run remem provider binary {binary:?}"))?;

        let stdout = child
            .stdout
            .take()
            .context("remem provider stdout pipe missing")?;
        let stderr = child
            .stderr
            .take()
            .context("remem provider stderr pipe missing")?;

        let (tx, rx) = mpsc::channel();
        let stdout_tx = tx.clone();
        let stdout_limit = max_capture_bytes;
        thread::spawn(move || {
            let _ = stdout_tx.send(("stdout", read_bounded(stdout, stdout_limit)));
        });
        let stderr_limit = max_capture_bytes;
        thread::spawn(move || {
            let _ = tx.send(("stderr", read_bounded(stderr, stderr_limit)));
        });

        let deadline = Instant::now() + timeout;
        let mut timed_out = false;
        let mut stdout_bytes = None;
        let mut stderr_bytes = None;
        let status = loop {
            while let Ok((stream, outcome)) = rx.try_recv() {
                match outcome {
                    CaptureOutcome::Overflow => {
                        let _ = child.kill();
                        let _ = child.wait();
                        while rx.recv_timeout(Duration::from_millis(50)).is_ok() {}
                        return Err(InfraError::ProcessOutputOverflow {
                            stream,
                            limit_bytes: max_capture_bytes,
                        }
                        .into());
                    }
                    CaptureOutcome::Io(error) => {
                        let _ = child.kill();
                        let _ = child.wait();
                        return Err(error).with_context(|| {
                            format!("read remem provider {stream} from {binary:?}")
                        });
                    }
                    CaptureOutcome::Bytes(bytes) if stream == "stdout" => {
                        stdout_bytes = Some(bytes);
                    }
                    CaptureOutcome::Bytes(bytes) if stream == "stderr" => {
                        stderr_bytes = Some(bytes);
                    }
                    CaptureOutcome::Bytes(_) => {}
                }
            }

            match child.try_wait() {
                Ok(Some(status)) => break status,
                Ok(None) => {
                    if Instant::now() >= deadline {
                        timed_out = true;
                        let _ = child.kill();
                        break child.wait().with_context(|| {
                            format!("reap remem provider binary {binary:?} after timeout")
                        })?;
                    }
                    thread::sleep(PROCESS_POLL_INTERVAL);
                }
                Err(error) => {
                    return Err(error)
                        .with_context(|| format!("wait for remem provider binary {binary:?}"));
                }
            }
        };

        if timed_out {
            while rx.recv_timeout(Duration::from_millis(50)).is_ok() {}
            return Err(InfraError::ProcessTimeout {
                operation: format!("remem {binary}"),
                timeout_ms: timeout.as_millis() as u64,
            }
            .into());
        }

        let mut remaining =
            usize::from(stdout_bytes.is_none()) + usize::from(stderr_bytes.is_none());
        while remaining > 0 {
            match rx.recv_timeout(timeout.max(Duration::from_secs(1))) {
                Ok((stream, outcome)) => match outcome {
                    CaptureOutcome::Bytes(bytes) if stream == "stdout" => {
                        if stdout_bytes.replace(bytes).is_none() {
                            remaining -= 1;
                        }
                    }
                    CaptureOutcome::Bytes(bytes) if stream == "stderr" => {
                        if stderr_bytes.replace(bytes).is_none() {
                            remaining -= 1;
                        }
                    }
                    CaptureOutcome::Overflow => {
                        return Err(InfraError::ProcessOutputOverflow {
                            stream,
                            limit_bytes: max_capture_bytes,
                        }
                        .into());
                    }
                    CaptureOutcome::Io(error) => {
                        return Err(error).with_context(|| {
                            format!("read remem provider {stream} from {binary:?}")
                        });
                    }
                    CaptureOutcome::Bytes(_) => {}
                },
                Err(mpsc::RecvTimeoutError::Timeout) => {
                    return Err(InfraError::ProcessTimeout {
                        operation: format!("remem {binary} capture"),
                        timeout_ms: timeout.as_millis() as u64,
                    }
                    .into());
                }
                Err(mpsc::RecvTimeoutError::Disconnected) => break,
            }
        }

        Ok(CommandResult {
            success: status.success(),
            code: status.code(),
            stdout: stdout_bytes.unwrap_or_default(),
            stderr: stderr_bytes.unwrap_or_default(),
        })
    }
}
