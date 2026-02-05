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

    #[error("LLM 响应解析失败: {0}")]
    LlmParse(String),

    #[error("HTTP 错误: {0}")]
    Http(String),
}

/// 应用层错误
#[derive(Error, Debug)]
pub enum AppError {
    #[error(transparent)]
    Domain(#[from] DomainError),

    #[error(transparent)]
    Infra(#[from] InfraError),

    #[error("配置错误: {0}")]
    Config(String),
}

/// 统一 Result 类型
pub type Result<T> = std::result::Result<T, AppError>;
pub type DomainResult<T> = std::result::Result<T, DomainError>;
pub type InfraResult<T> = std::result::Result<T, InfraError>;
