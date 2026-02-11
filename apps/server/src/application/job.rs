use axum::http::StatusCode;
use std::sync::Arc;

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
    pub fn status_code(&self) -> StatusCode {
        match self {
            Self::NotFound(_) => StatusCode::NOT_FOUND,
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
    use axum::http::StatusCode;

    #[test]
    fn extraction_job_error_maps_not_found_status() {
        let err = ExtractionJobError::NotFound("Job not found".to_string());
        assert_eq!(err.status_code(), StatusCode::NOT_FOUND);
        assert_eq!(err.message(), "Job not found");
    }
}
