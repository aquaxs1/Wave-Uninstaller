fn main() {
    // The default manifest asks for no privileges. Wave writes to HKLM and to
    // Program Files, so it needs administrator rights before it starts rather
    // than halfway through a removal — see the manifest for the reasoning.
    let windows = tauri_build::WindowsAttributes::new()
        .app_manifest(include_str!("windows-app-manifest.xml"));

    tauri_build::try_build(tauri_build::Attributes::new().windows_attributes(windows))
        .expect("the Tauri build script failed");
}
