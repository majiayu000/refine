pub mod conversation;
pub mod error;
pub mod event;
pub mod item;
pub mod job;
pub mod query;
pub mod recommendation;

#[cfg(test)]
mod tests {
    use std::fs;
    use std::path::Path;

    const APPLICATION_FILES: [&str; 7] = [
        "conversation.rs",
        "error.rs",
        "event.rs",
        "item.rs",
        "job.rs",
        "query.rs",
        "recommendation.rs",
    ];
    const FORBIDDEN_TRANSPORT_IMPORTS: [&str; 2] = ["axum::", "tiny_http::"];

    #[test]
    fn application_layer_stays_transport_agnostic() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("src/application");

        for file in APPLICATION_FILES {
            let path = root.join(file);
            let source = fs::read_to_string(&path)
                .unwrap_or_else(|err| panic!("failed to read {}: {}", path.display(), err));

            for forbidden in FORBIDDEN_TRANSPORT_IMPORTS {
                assert!(
                    !source.contains(forbidden),
                    "{} should not depend on transport import {}",
                    path.display(),
                    forbidden
                );
            }
        }
    }
}
