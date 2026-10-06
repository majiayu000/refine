#[tokio::main]
async fn main() {
    if let Err(error) = dotenvy::dotenv() {
        if !error.not_found() {
            eprintln!("Warning: failed to load .env ({error})");
        }
    }
    tracing_subscriber::fmt()
        .with_env_filter("refine_server=info,refine_core=info")
        .init();
    if let Err(error) = refine_server::run().await {
        eprintln!("{error}");
        std::process::exit(1);
    }
}
