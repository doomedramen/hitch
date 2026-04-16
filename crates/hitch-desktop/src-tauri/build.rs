fn main() {
    ensure_default_icons();
    tauri_build::build()
}

fn ensure_default_icons() {
    use std::fs;
    use std::path::PathBuf;

    let manifest_dir = std::env::var("CARGO_MANIFEST_DIR").unwrap_or_else(|_| ".".to_string());
    let manifest_dir = PathBuf::from(manifest_dir);
    let icons_dir = manifest_dir.join("icons");
    let icon_png = icons_dir.join("icon.png");

    if icon_png.exists() {
        return;
    }

    let _ = fs::create_dir_all(&icons_dir);

    // 1x1 transparent RGBA PNG, used as a build-time fallback so
    // `tauri::generate_context!()` doesn't fail when the project hasn't generated real icons yet.
    const ICON_PNG: &[u8] = &[
        137, 80, 78, 71, 13, 10, 26, 10, 0, 0, 0, 13, 73, 72, 68, 82, 0, 0, 0, 1, 0, 0, 0, 1, 8, 6,
        0, 0, 0, 31, 21, 196, 137, 0, 0, 0, 13, 73, 68, 65, 84, 120, 156, 99, 96, 96, 96, 96, 0, 0,
        0, 5, 0, 1, 165, 246, 69, 64, 0, 0, 0, 0, 73, 69, 78, 68, 174, 66, 96, 130,
    ];

    let _ = fs::write(&icon_png, ICON_PNG);
}
