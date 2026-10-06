/// The native commands and embedded HTTP API share the same service state.
pub type AppState = std::sync::Arc<refine_server::AppState>;
