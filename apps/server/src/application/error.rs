#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ApplicationErrorCode {
    BadRequest,
    Forbidden,
    NotFound,
    Internal,
}
