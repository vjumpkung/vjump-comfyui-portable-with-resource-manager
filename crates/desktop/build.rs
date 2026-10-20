fn main() {
    // The signed updater exists only in builds that carry both values (see `updates.rs`).
    println!("cargo:rerun-if-env-changed=VJUMP_UPDATER_PUBKEY");
    println!("cargo:rerun-if-env-changed=VJUMP_UPDATER_ENDPOINT");

    tauri_build::try_build(tauri_build::Attributes::new().app_manifest(
        tauri_build::AppManifest::new().commands(&[
            "app_info",
            "settings_get",
            "settings_set",
            "secret_set",
            "secret_clear",
            "secret_status",
            "setup_status",
            "setup_detect_hardware",
            "setup_list_profiles",
            "setup_list_curated_nodes",
            "setup_validate_python",
            "setup_select_root",
            "setup_open_install",
            "setup_new_install",
            "setup_run",
            "setup_cancel",
            "env_info",
            "env_packages",
            "env_pip_preview",
            "env_pip_apply",
            "env_delete",
            "pick_folder",
            "pick_file",
            "comfy_status",
            "comfy_start",
            "comfy_stop",
            "comfy_restart",
            "comfy_subscribe",
            "comfy_unsubscribe",
            "comfy_get_launch_options",
            "comfy_set_launch_options",
            "comfy_list_versions",
            "comfy_update",
            "comfy_open_window",
            "comfy_open_browser",
            "comfy_read_extra_model_paths",
            "comfy_generate_extra_model_paths",
            "comfy_remove_extra_model_paths",
            "app_close_decide",
            "secret_test",
            "dl_resolve",
            "dl_enqueue",
            "dl_pause",
            "dl_resume",
            "dl_cancel",
            "dl_retry",
            "dl_subscribe",
            "dl_unsubscribe",
            "dl_import_pack",
            "dl_export",
            "model_list",
            "model_rescan",
            "model_details",
            "model_rename",
            "model_move",
            "model_delete_plan",
            "model_delete",
            "model_delete_permanently",
            "model_identify",
            "model_operation_status",
            "model_subscribe",
            "model_unsubscribe",
            "node_open_repository",
            "node_registry_search",
            "node_list",
            "node_git_status",
            "node_rescan",
            "node_prepare_registry",
            "node_prepare_git",
            "node_prepare_update",
            "node_prepare_retry_deps",
            "node_commit",
            "node_set_enabled",
            "node_uninstall",
            "node_update_all",
            "node_cancel",
            "node_operation_status",
            "node_subscribe",
            "node_unsubscribe",
            "app_update_status",
            "app_update_check",
            "app_update_install",
            "app_copy_diagnostics",
        ]),
    ))
    .expect("failed to run tauri build script");

    // tauri-build embeds the Common Controls v6 manifest into the binary only. Test
    // executables (including library unit tests) need it too. `rustc-link-arg-tests`
    // only covers integration-test targets, not the library's test executable.
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows")
        && std::env::var("CARGO_CFG_TARGET_ENV").as_deref() == Ok("msvc")
    {
        // lib.rs links the generated resource library under cfg(test).
        println!(
            "cargo:rustc-link-search=native={}",
            std::env::var("OUT_DIR").unwrap()
        );
        println!("cargo:rustc-link-arg-tests=/MANIFEST:EMBED");
        println!(
            "cargo:rustc-link-arg-tests=/MANIFESTDEPENDENCY:type='win32' name='Microsoft.Windows.Common-Controls' version='6.0.0.0' processorArchitecture='*' publicKeyToken='6595b64144ccf1df' language='*'"
        );
    }

    // Debug startup (compose -> node reconcile/rescan) and shutdown run block_on on the main
    // thread; the 1 MiB Windows default overflows. Keep in sync with tests/stack_reserve.rs.
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows") {
        match std::env::var("CARGO_CFG_TARGET_ENV").as_deref() {
            Ok("msvc") => println!("cargo:rustc-link-arg-bins=/STACK:8388608"),
            Ok("gnu") | Ok("gnullvm") => println!("cargo:rustc-link-arg-bins=-Wl,--stack,8388608"),
            _ => {}
        }
    }
}
