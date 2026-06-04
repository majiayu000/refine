use std::sync::Arc;

use crate::application::error::ApplicationErrorCode;
use crate::models::ExtractionJobRecord;
use crate::state::AppState;

#[derive(Debug, Clone)]
pub struct ExtractionJobResult {
    pub job: ExtractionJobRecord,
}

#[derive(Debug, Clone)]
pub enum ExtractionJobError {
    NotFound(String),
    Internal(String),
}

impl ExtractionJobError {
    pub fn code(&self) -> ApplicationErrorCode {
        match self {
            Self::NotFound(_) => ApplicationErrorCode::NotFound,
            Self::Internal(_) => ApplicationErrorCode::Internal,
        }
    }

    pub fn message(&self) -> &str {
        match self {
            Self::NotFound(message) => message,
            Self::Internal(message) => message,
        }
    }
}

pub async fn get_extraction_job(
    state: Arc<AppState>,
    job_id: String,
) -> Result<ExtractionJobResult, ExtractionJobError> {
    let job = state
        .job_repo
        .find_job_by_id(&job_id)
        .await
        .map_err(|err| ExtractionJobError::Internal(err.to_string()))?
        .ok_or_else(|| ExtractionJobError::NotFound("Job not found".to_string()))?;
    Ok(ExtractionJobResult { job })
}

#[cfg(test)]
mod tests {
    use super::ExtractionJobError;
    use crate::application::error::ApplicationErrorCode;

    #[test]
    fn extraction_job_error_maps_not_found_status() {
        let err = ExtractionJobError::NotFound("Job not found".to_string());
        assert_eq!(err.code(), ApplicationErrorCode::NotFound);
        assert_eq!(err.message(), "Job not found");
    }

    #[test]
    fn extraction_job_internal_maps_to_500() {
        let err = ExtractionJobError::Internal("db error".to_string());
        assert_eq!(err.code(), ApplicationErrorCode::Internal);
        assert_eq!(err.message(), "db error");
    }
}
