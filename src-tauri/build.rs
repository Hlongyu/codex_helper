fn main() {
    let app_version = std::env::var("CODEX_HELPER_VERSION")
        .or_else(|_| std::env::var("GITHUB_REF_NAME"))
        .ok()
        .filter(|value| value.starts_with('v'))
        .unwrap_or_else(|| format!("v{}", env!("CARGO_PKG_VERSION")));

    println!("cargo:rustc-env=CODEX_HELPER_VERSION={app_version}");
    println!("cargo:rerun-if-env-changed=CODEX_HELPER_VERSION");
    println!("cargo:rerun-if-env-changed=GITHUB_REF_NAME");

    tauri_build::build()
}
