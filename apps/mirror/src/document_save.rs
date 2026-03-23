use anyhow::{Context, Result};
use refine_core::knowledge::{Document, DocumentId, DocumentRepository};
use std::sync::Arc;

pub struct SaveDocumentOptions {
    pub source: &'static str,
    pub title_prefix: &'static str,
    pub url_scheme: &'static str,
    pub save_error_context: &'static str,
}

pub async fn save_report_to_document(
    doc_repo: &Arc<dyn DocumentRepository>,
    report: &str,
    options: SaveDocumentOptions,
) -> Result<DocumentId> {
    let mut doc = Document::new(options.source, report);
    let created_at = doc.created_at();
    let title = format!("{} {}", options.title_prefix, created_at.format("%Y-%m-%d"));
    doc.set_title(&title);
    doc.set_url(&format!(
        "{}://{}",
        options.url_scheme,
        created_at.to_rfc3339()
    ));
    doc_repo
        .save(&doc)
        .await
        .context(options.save_error_context)?;

    Ok(doc.id().clone())
}

#[cfg(test)]
mod tests {
    use super::*;
    use refine_core::infra::SqliteStore;
    use refine_core::knowledge::DocumentRepository;

    #[tokio::test]
    async fn test_save_report_to_document_sets_weekly_metadata() {
        let store = Arc::new(SqliteStore::in_memory().unwrap());
        let repo: Arc<dyn DocumentRepository> = store.clone();

        let doc_id = save_report_to_document(
            &repo,
            "weekly report body",
            SaveDocumentOptions {
                source: "mirror-weekly",
                title_prefix: "Mirror Weekly",
                url_scheme: "mirror-weekly",
                save_error_context: "Failed to save weekly report",
            },
        )
        .await
        .unwrap();

        let saved = store.find_by_id(&doc_id).await.unwrap().unwrap();
        let expected_title = format!("Mirror Weekly {}", saved.created_at().format("%Y-%m-%d"));
        let expected_url = format!("mirror-weekly://{}", saved.created_at().to_rfc3339());
        assert_eq!(saved.source(), "mirror-weekly");
        assert_eq!(saved.raw_content(), "weekly report body");
        assert_eq!(saved.title(), Some(expected_title.as_str()));
        assert_eq!(saved.url(), expected_url.as_str());
    }

    #[tokio::test]
    async fn test_save_report_to_document_sets_profile_metadata() {
        let store = Arc::new(SqliteStore::in_memory().unwrap());
        let repo: Arc<dyn DocumentRepository> = store.clone();

        let doc_id = save_report_to_document(
            &repo,
            "profile narrative",
            SaveDocumentOptions {
                source: "mirror-profile",
                title_prefix: "Mirror Profile",
                url_scheme: "mirror-profile",
                save_error_context: "Failed to save profile",
            },
        )
        .await
        .unwrap();

        let saved = store.find_by_id(&doc_id).await.unwrap().unwrap();
        let expected_title = format!("Mirror Profile {}", saved.created_at().format("%Y-%m-%d"));
        let expected_url = format!("mirror-profile://{}", saved.created_at().to_rfc3339());
        assert_eq!(saved.source(), "mirror-profile");
        assert_eq!(saved.raw_content(), "profile narrative");
        assert_eq!(saved.title(), Some(expected_title.as_str()));
        assert_eq!(saved.url(), expected_url.as_str());
    }
}
