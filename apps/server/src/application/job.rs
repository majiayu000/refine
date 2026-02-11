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
}

impl ExtractionJobError {
    pub fn code(&self) -> ApplicationErrorCode {
        match self {
            Self::NotFound(_) => ApplicationErrorCode::NotFound,
        }
    }

    pub fn message(&self) -> &str {
        match self {
            Self::NotFound(message) => message,
        }
    }
}

pub async fn get_extraction_job(
    state: Arc<AppState>,
    job_id: String,
) -> Result<ExtractionJobResult, ExtractionJobError> {
    let jobs = state.runtime.jobs.read().await;
    let Some(job) = jobs.get(&job_id).cloned() else {
        return Err(ExtractionJobError::NotFound("Job not found".to_string()));
    };
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
}
