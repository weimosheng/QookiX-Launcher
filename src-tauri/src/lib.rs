mod accounts;
mod commands;
mod curseforge;
mod download;
mod install;
mod instances;
mod java;
mod launch;
mod mcmeta;
mod mcping;
mod mcmod;
mod models;
mod modpack;
mod modrinth;
mod paths;
mod settings;
mod state;
mod util;

use state::AppState;
use std::collections::HashMap;
use std::sync::{Arc, Mutex, RwLock};
use tauri::Manager;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let default_root = std::path::PathBuf::from(settings::default_root());
    let _ = settings::ensure_layout(&default_root);

    // The installer seeds a custom data directory for fresh installs by writing
    // `settings.json` (with `data_dir`) into the default root. Honor it: all
    // launcher data (instances, libraries, assets, settings) then lives under
    // the chosen folder. Existing installs keep their current data root.
    let seeded = settings::load_settings(&default_root);
    let root = if seeded.data_dir.is_empty() {
        default_root
    } else {
        std::path::PathBuf::from(&seeded.data_dir)
    };
    let _ = settings::ensure_layout(&root);
    let loaded = settings::load_settings(&root);

    let proxy = loaded.proxy.clone();
    let app_state = AppState {
        root,
        settings: RwLock::new(loaded),
        client: settings::http_client(proxy.as_deref()),
        semaphore: Arc::new(tokio::sync::Semaphore::new(8)),
        game_pids: Arc::new(Mutex::new(HashMap::new())),
        task_counter: std::sync::atomic::AtomicU64::new(1),
        install_cancel: Arc::new(std::sync::atomic::AtomicBool::new(false)),
        ms_flow: Arc::new(Mutex::new(None)),
        java_cache: Mutex::new(None),
    };

    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_process::init())
        .plugin(tauri_plugin_updater::Builder::new().build())
        .manage(app_state)
        .invoke_handler(tauri::generate_handler![
            // settings & java
            commands::get_settings,
            commands::set_settings,
            commands::change_data_dir,
            commands::auto_detect_memory,
            commands::detect_java,
            commands::download_java,
            commands::recommend_java,
            // versions
            commands::get_version_manifest,
            commands::get_loader_versions,
            // instances
            commands::list_instances,
            commands::get_instance_info,
            commands::create_instance,
            commands::update_instance_settings,
            commands::delete_instance,
            commands::install_game,
            commands::cancel_install,
            commands::launch_instance,
            commands::stop_game,
            commands::is_game_running,
            commands::open_instance_folder,
            commands::list_instance_folders,
            commands::list_instance_files,
            commands::import_modpack,
            commands::import_instance_image,
            commands::scan_minecraft_import,
            commands::estimate_download,
            commands::estimate_import,
            commands::import_minecraft_folder,
            // accounts
            commands::list_accounts,
            commands::login_offline,
            commands::login_ms_start,
            commands::login_ms_poll,
            commands::logout_account,
            // browse & content
            commands::browse,
            commands::project_versions,
            commands::curseforge_categories,
            commands::project_info,
            commands::project_dependencies,
            commands::mc_wiki_url,
            commands::install_content,
            commands::check_updates,
            commands::apply_update,
            commands::uninstall_content,
            commands::list_content,
            commands::identify_content,
            commands::import_local_file,
            commands::toggle_content_enabled,
            commands::save_text_file,
            commands::extract_game_icons,
            // skins
            commands::list_skins,
            commands::read_skin_data_url,
            commands::save_skin_from_data,
            commands::download_skin_from_url,
            commands::delete_skin,
            commands::fetch_player_skin,
            commands::fetch_player_capes,
            commands::apply_skin_to_account,
            commands::apply_cape_to_account,
            commands::apply_skin_offline,
            // multiplayer servers
            commands::list_servers,
            commands::ping_mc_server,
        ])
        .on_window_event(|window, event| {
            use tauri::WindowEvent;
            if let WindowEvent::CloseRequested { api, .. } = event {
                let app = window.app_handle();
                let state = app.state::<AppState>();
                let behavior = {
                    let s = state.settings.read().unwrap();
                    s.close_behavior.clone()
                };
                if behavior == "minimize" {
                    // keep the app running (hidden) instead of quitting
                    api.prevent_close();
                    let _ = window.hide();
                }
            }
        })
        .setup(|app| {
            let handle = app.handle().clone();
            let root = app.state::<AppState>().root.clone();
            // Allow the asset protocol to serve any file under the launcher's
            // data root (game icons, instance/pack icons, skins, ...). The
            // static `assetProtocol.scope` in tauri.conf.json cannot express a
            // runtime/custom `data_dir` (its `$APPDATA/**` maps to the
            // identifier-based app dir, not `...\QookiX-Launcher`), which used
            // to make `convertFileSrc` images fail with HTTP 403. Extend the
            // scope at runtime so icons load regardless of the data location.
            let _ = app.asset_protocol_scope().allow_directory(&root, true);
            let runtimes = root.join("runtimes");
            tauri::async_runtime::spawn(async move {
                let now = std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .map(|d| d.as_secs())
                    .unwrap_or(0);
                // Reuse the persisted cache instead of rescanning on every start.
                let cache_root = root.clone();
                let cache_runtimes = runtimes.clone();
                let (ts, detected) = tokio::task::spawn_blocking(move || {
                    crate::java::cached_detect(&cache_root, &cache_runtimes, now, false)
                })
                .await
                .unwrap_or_else(|_| (now, Vec::new()));
                let state = handle.state::<AppState>();
                *state.java_cache.lock().unwrap() = Some((ts, detected));
            });
            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}

#[cfg(test)]
mod smoke {
    use crate::models::{maven_to_path, LoaderMetaEntry, VersionManifest, VersionJson};
    use std::collections::HashMap;

    #[tokio::test]
    async fn mojang_manifest_and_version_json_parse() {
        let client = crate::settings::http_client(None);
        let url = "https://piston-meta.mojang.com/mc/game/version_manifest_v2.json";
        let text = crate::download::get_text(&client, url).await.unwrap_or_else(|e| {
            panic!("fetch failed: {e}");
        });
        eprintln!("first 200 chars: {:?}", &text[..text.len().min(200)]);
        let manifest: VersionManifest = serde_json::from_str(&text).unwrap_or_else(|e| {
            panic!("manifest json parse failed: {e} at pos {}", e.line());
        });
        assert!(manifest.versions.len() > 100, "manifest too small");
        let entry = manifest.versions.iter().find(|v| v.id == "1.20.1").expect("1.20.1 missing");
        let version: VersionJson =
            crate::download::get_json(&client, &entry.url).await.expect("version json failed");
        assert_eq!(version.id, "1.20.1");
        assert!(version.libraries.len() > 10);
        assert!(version.asset_index.is_some());
        assert!(version.downloads.client.is_some());
        // modern natives come as separate `...:natives-windows` entries
        let native = version
            .libraries
            .iter()
            .find(|l| l.name.contains("natives-windows"))
            .expect("no modern natives entries");
        assert!(crate::install::is_native_entry(native));
        assert!(crate::install::platform_native_classifier(native).is_some());
    }

    #[tokio::test]
    async fn fabric_meta_parses() {
        let client = crate::settings::http_client(None);
        let entry: LoaderMetaEntry = crate::download::get_json(
            &client,
            "https://meta.fabricmc.net/v2/versions/loader/1.20.1/0.15.11",
        )
        .await
        .expect("fabric meta fetch failed");
        assert!(entry.launcher_meta.main_class.is_some());
        assert!(entry.launcher_meta.libraries.is_some());
    }

    #[test]
    fn maven_path_maps() {
        let p = maven_to_path("net.fabricmc:fabric-loader:0.15.11").unwrap();
        assert_eq!(
            p.to_string_lossy().replace('\\', "/"),
            "net/fabricmc/fabric-loader/0.15.11/fabric-loader-0.15.11.jar"
        );
        let p2 = maven_to_path("org.lwjgl:lwjgl:3.3.1:natives-windows").unwrap();
        assert!(p2.to_string_lossy().ends_with("lwjgl-3.3.1-natives-windows.jar"));
    }

    #[test]
    fn java_detection_report() {
        let found = crate::java::detect_java(None, None);
        eprintln!("=== detected {} Java installations ===", found.len());
        for j in &found {
            eprintln!("  {} | Java {} | {} | {}", j.vendor, j.version, j.arch, j.path);
        }
        assert!(!found.is_empty(), "expected at least one Java on this machine");
    }

    #[test]
    fn unsafe_classifier_is_regular_library() {
        use crate::models::Library;
        // `:unsafe` (and any non-natives classifier) is a normal classpath jar
        let unsafe_lib = Library {
            name: "org.lwjgl:lwjgl:3.4.1:unsafe".into(),
            url: None,
            downloads: None,
            rules: None,
            natives: None,
            extract: None,
        };
        assert!(
            !crate::install::is_native_entry(&unsafe_lib),
            ":unsafe must NOT be treated as a native entry"
        );
        let native_lib = Library {
            name: "org.lwjgl:lwjgl:3.4.1:natives-windows".into(),
            url: None,
            downloads: None,
            rules: None,
            natives: None,
            extract: None,
        };
        assert!(crate::install::is_native_entry(&native_lib));
        let p = crate::models::maven_to_path("org.lwjgl:lwjgl:3.4.1:unsafe").unwrap();
        assert!(p.to_string_lossy().ends_with("lwjgl-3.4.1-unsafe.jar"));
    }

    #[test]
    fn natives_args_normalized_like_pcl() {
        use crate::models::{ArgumentValue, Arguments, ArgumentValueInner, ArgumentRule, VersionJson};
        let mut vj = VersionJson {
            id: "t".into(),
            kind: "release".into(),
            main_class: Some("x".into()),
            minecraft_arguments: None,
            arguments: Some(Arguments {
                game: None,
                jvm: Some(vec![
                    ArgumentValue::Str("-Djava.library.path=${natives_directory}/java".into()),
                    ArgumentValue::Str("-Djna.tmpdir=${natives_directory}/jna".into()),
                    ArgumentValue::Str("-Dorg.lwjgl.system.SharedLibraryExtractPath=${natives_directory}/lwjgl".into()),
                    ArgumentValue::Str("-Dio.netty.native.workdir=${natives_directory}/netty".into()),
                    ArgumentValue::Str("-Xmx2G".into()),
                    ArgumentValue::Rule(ArgumentRule {
                        rules: vec![],
                        value: ArgumentValueInner::Str("-Djava.library.path=${natives_directory}/java".into()),
                    }),
                ]),
            }),
            asset_index: None,
            downloads: Default::default(),
            libraries: vec![],
            logging: None,
            inherits_from: None,
            java_version: None,
        };
        crate::install::normalize_natives_args(&mut vj);
        let jvm = vj.arguments.unwrap().jvm.unwrap();
        assert!(matches!(&jvm[0], ArgumentValue::Str(x) if x == "-Djava.library.path=${natives_directory}"));
        assert!(matches!(&jvm[1], ArgumentValue::Str(x) if x == "-Djna.tmpdir=${natives_directory}"));
        assert!(matches!(&jvm[2], ArgumentValue::Str(x) if x == "-Dorg.lwjgl.system.SharedLibraryExtractPath=${natives_directory}"));
        assert!(matches!(&jvm[3], ArgumentValue::Str(x) if x == "-Dio.netty.native.workdir=${natives_directory}"));
        assert!(matches!(&jvm[4], ArgumentValue::Str(x) if x == "-Xmx2G"));
        // rule-wrapped args normalized too
        match &jvm[5] {
            ArgumentValue::Rule(r) => match &r.value {
                ArgumentValueInner::Str(sv) => assert_eq!(sv, "-Djava.library.path=${natives_directory}"),
                _ => panic!("expected string value"),
            },
            _ => panic!("expected rule"),
        }
    }

    #[tokio::test]
    async fn fabric_patch_builds_valid_version_json() {
        use crate::models::{Instance, LoaderType};
        use std::sync::{Arc, Mutex, RwLock};

        let root = std::env::temp_dir().join("qookix-test-patch");
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        let state = crate::state::AppState {
            root: root.clone(),
            settings: RwLock::new(Default::default()),
            client: crate::settings::http_client(None),
            semaphore: Arc::new(tokio::sync::Semaphore::new(4)),
            game_pids: Arc::new(Mutex::new(HashMap::new())),
            task_counter: std::sync::atomic::AtomicU64::new(1),
            install_cancel: Arc::new(std::sync::atomic::AtomicBool::new(false)),
            ms_flow: Arc::new(Mutex::new(None)),
            java_cache: Mutex::new(None),
        };
        let instance = Instance {
            id: "test-fabric".into(),
            name: "test".into(),
            mc_version: "1.20.1".into(),
            loader: LoaderType::Fabric,
            loader_version: Some("0.15.11".into()),
            created: 0,
            last_played: None,
            installed: false,
            icon: None,
            max_memory_mb: None,
            memory_mode: None,
            jvm_args: None,
            game_args: None,
            java_path: None,
            account_id: None,
            resolution: None,
            mods: vec![],
            resource_packs: vec![],
            shaders: vec![],
            is_symlink: false,
            source_path: None,
        };
        let vanilla = crate::mcmeta::fetch_version_json(&state, "1.20.1").await.unwrap();
        let patched = crate::install::fabric_patch(&state, &vanilla, &instance).await.unwrap();
        // id is stamped by patch_version; fabric_patch keeps the vanilla id
        assert_eq!(patched.id, "1.20.1");
        assert!(patched.main_class.as_deref().unwrap_or("").contains("KnotClient"));
        assert!(patched.libraries.iter().any(|l| l.name.starts_with("net.fabricmc:fabric-loader")));
        assert!(patched.libraries.iter().any(|l| l.name.contains("natives-windows")));
    }

    #[tokio::test]
    async fn modrinth_search_with_facets_works() {
        use std::sync::{Arc, Mutex, RwLock};

        let root = std::env::temp_dir().join("qookix-test-mr");
        std::fs::create_dir_all(&root).unwrap();
        let state = crate::state::AppState {
            root: root.clone(),
            settings: RwLock::new(Default::default()),
            client: crate::settings::http_client(None),
            semaphore: Arc::new(tokio::sync::Semaphore::new(4)),
            game_pids: Arc::new(Mutex::new(HashMap::new())),
            task_counter: std::sync::atomic::AtomicU64::new(1),
            install_cancel: Arc::new(std::sync::atomic::AtomicBool::new(false)),
            ms_flow: Arc::new(Mutex::new(None)),
            java_cache: Mutex::new(None),
        };
        // modpack type + empty query (regression for the 400 bug)
        let res = crate::modrinth::search(&state, "", "modpack", "", "relevance", 0, 20, "", "")
            .await
            .expect("modrinth search should succeed");
        let hits = res.get("hits").and_then(|h| h.as_array()).unwrap();
        assert!(!hits.is_empty(), "expected hits for modpack browse");
        // category facet too
        let res2 = crate::modrinth::search(&state, "sodium", "mod", "fabric", "relevance", 0, 5, "", "")
            .await
            .expect("modrinth search with category should succeed");
        let hits2 = res2.get("hits").and_then(|h| h.as_array()).unwrap();
        assert!(!hits2.is_empty(), "expected hits for sodium/fabric");
    }
}
