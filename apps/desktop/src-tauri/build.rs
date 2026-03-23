fn main() {
    // `generate_context!` requires `frontendDist` to exist at compile-time.
    // CI runs `cargo check/test` without building the UI bundle first.
    let dist_dir = std::path::Path::new("../ui/dist");
    if !dist_dir.exists() {
        std::fs::create_dir_all(dist_dir).expect("failed to create apps/desktop/ui/dist");
        let index_path = dist_dir.join("index.html");
        if !index_path.exists() {
            std::fs::write(&index_path, "<!doctype html><html><body></body></html>")
                .expect("failed to write placeholder apps/desktop/ui/dist/index.html");
        }
    }

    tauri_build::build()
}
