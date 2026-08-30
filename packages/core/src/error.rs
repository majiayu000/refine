//! 统一错误类型
//!
//! 所有模块共享的错误定义

use thiserror::Error;

/// 领域错误
#[derive(Error, Debug)]
pub enum DomainError {
    #[error("验证失败: {0}")]
    Validation(String),

    #[error("标签数量超限 (最多 20 个)")]
    TooManyTags,

    #[error("无效的对话格式")]
    InvalidConversation,

    #[error("提炼失败: {0}")]
    Extraction(String),
}

/// 基础设施错误
#[derive(Error, Debug)]
pub enum InfraError {
    #[error("数据库错误: {0}")]
    Database(String),

    #[error("未找到: {0}")]
    NotFound(String),

    #[error("序列化错误: {0}")]
    Serialization(String),

    #[error("LLM 请求失败: {0}")]
    LlmRequest(String),

    #[error("LLM HTTP 错误 ({status}): {message}")]
    LlmHttp { status: u16, message: String },

    #[error("LLM 响应解析失败: {0}")]
    LlmParse(String),

    #[error("LLM 用量账本写入失败: {0}")]
    UsageLedger(String),

    /// The provider rejected the prompt for a deterministic policy reason.
    /// Retrying the same input cannot succeed, so callers must quarantine it.
    #[error("LLM 内容被拒绝 ({code}): {message}")]
    LlmRejected { code: String, message: String },

    #[error("HTTP 错误: {0}")]
    Http(String),

    /// Quota/rate-limit exhaustion — not a transient error; callers must not retry.
    #[error("LLM 配额已耗尽 (retry_after: {retry_after_secs:?}s)")]
    RateLimited { retry_after_secs: Option<u64> },
}

/// Core 顶层错误
#[derive(Error, Debug)]
pub enum CoreError {
    #[error(transparent)]
    Domain(#[from] DomainError),

    #[error(transparent)]
    Infra(#[from] InfraError),

    #[error("配置错误: {0}")]
    Config(String),
}

/// 统一 Result 类型
pub type Result<T> = std::result::Result<T, CoreError>;
pub type AppError = CoreError;
pub type DomainResult<T> = std::result::Result<T, DomainError>;
pub type InfraResult<T> = std::result::Result<T, InfraError>;

#[cfg(test)]
mod tests {
    use super::{AppError, CoreError, DomainError, InfraError, Result};

    fn accepts_app_error(_: AppError) {}

    #[test]
    fn app_error_remains_public_alias_for_core_error() {
        let app_error: AppError = DomainError::Validation("missing title".to_string()).into();
        accepts_app_error(app_error);

        let result: Result<()> = Err(InfraError::Database("locked".to_string()).into());
        match result {
            Err(core_error) => {
                let _: CoreError = core_error;
            }
            Ok(()) => panic!("expected infra error"),
        }
    }
}
