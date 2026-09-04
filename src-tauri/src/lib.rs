mod accounts;
mod commands;
mod crash;
mod curseforge;
mod download;
mod install;
mod instances;
mod java;
mod launch;
mod mcmeta;
mod mcping;
mod mcmod;
mod mirror;
mod models;
mod modpack;
mod pins;
mod modrinth;
mod paths;
mod servers;
mod settings;
mod state;
mod storage;
mod terracotta;
mod updater;
mod util;

use state::AppState;
use std::collections::HashMap;
use std::sync::{Arc, Mutex, RwLock};
use tauri::{
    menu::{MenuBuilder, MenuItemBuilder},
    tray::{MouseButton, TrayIconBuilder, TrayIconEvent},
    Manager,
};

/// Restore (and focus) the main window — used by the tray icon, which is the
/// only way back once the window has been hidden by the "minimize to
/// background" close behaviour.
fn show_main_window(app: &tauri::AppHandle) {
    if let Some(win) = app.get_webview_window("main") {
        let _ = win.unminimize();
        let _ = win.show();
        let _ = win.set_focus();
    }
}

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

    let proxy_mode = loaded.proxy_mode.clone();
    let proxy = loaded.proxy.clone();
    let app_state = AppState {
        root,
        settings: RwLock::new(loaded),
        client: settings::http_client(&proxy_mode, proxy.as_deref()),
        semaphore: Arc::new(tokio::sync::Semaphore::new(8)),
        game_pids: Arc::new(Mutex::new(HashMap::new())),
        server_pids: Arc::new(Mutex::new(HashMap::new())),
        server_senders: Arc::new(Mutex::new(HashMap::new())),
        task_counter: std::sync::atomic::AtomicU64::new(1),
        install_cancel: Arc::new(std::sync::atomic::AtomicBool::new(false)),
        ms_flow: Arc::new(Mutex::new(None)),
        java_cache: Mutex::new(None),
        terracotta: Mutex::new(None),
        pending_update: Mutex::new(None),
    };

    tauri::Builder::default()
        // single-instance 必须最先注册：二次唤起（如 qookix:// 链接）时把
        // 参数转发给已运行实例，而不是开出第二个窗口。
        // 转发方式：deep-link 的 JS 端 onOpenUrl 监听 "deep-link://new-url"
        // 事件（载荷为 URL 字符串数组，见 tauri-plugin-deep-link 源码），
        // 从二次实例的 argv 中取出 qookix:// 链接原样重发该事件即可。
        .plugin(tauri_plugin_single_instance::init(|app, args, _cwd| {
            println!("[single-instance] 二次启动 args={:?}", args);
            // 官方范式：交给 deep-link 插件处理 argv——它自己找出 qookix://
            // 链接、更新内部状态并 emit 一次 deep-link://new-url。
            // （此前手写 emit 与之重复，导致同一链接被处理多次、实例连开三次）
            {
                use tauri_plugin_deep_link::DeepLinkExt;
                app.deep_link().handle_cli_arguments(args.iter());
            }
            // 无论是否带链接，二次启动都把已有窗口拉到前台
            if let Some(w) = app.get_webview_window("main") {
                let _ = w.show();
                let _ = w.unminimize();
                let _ = w.set_focus();
            }
        }))
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_process::init())
        .plugin(tauri_plugin_updater::Builder::new().build())
        .plugin(tauri_plugin_deep_link::init())
        .manage(app_state)
        .invoke_handler(tauri::generate_handler![
            // settings & java
            commands::get_settings,
            commands::set_settings,
            commands::list_mirrors,
            commands::test_mirror,
            commands::test_proxy,
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
            commands::list_instance_groups,
            commands::create_instance_group,
            commands::rename_instance_group,
            commands::delete_instance_group,
            commands::reorder_instance_groups,
            commands::install_game,
            commands::cancel_install,
            commands::launch_instance,
            commands::stop_game,
            commands::is_game_running,
            commands::open_instance_folder,
            commands::list_instance_folders,
            commands::list_instance_files,
            commands::list_instance_dir,
            commands::read_instance_file,
            commands::write_instance_file,
            commands::create_instance_entry,
            commands::delete_instance_path,
            commands::rename_instance_path,
            commands::reveal_instance_path,
            commands::import_modpack,
            commands::import_instance_image,
            commands::import_background_image,
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
            commands::fetch_image_data_url,
            commands::fetch_player_capes,
            commands::apply_skin_to_account,
            commands::apply_cape_to_account,
            commands::apply_skin_offline,
            commands::get_offline_skin,
            // multiplayer servers
            commands::list_servers,
            commands::ping_mc_server,
            // hosted game servers
            commands::list_hosted_servers,
            commands::get_hosted_server,
            commands::create_hosted_server,
            commands::update_hosted_server,
            commands::delete_hosted_server,
            commands::install_hosted_server_core,
            commands::start_hosted_server,
            commands::stop_hosted_server,
            commands::is_hosted_server_running,
            commands::read_hosted_server_log,
            commands::open_hosted_server_folder,
            commands::reveal_hosted_server_path,
            commands::list_hosted_server_folders,
            commands::list_hosted_server_files,
            commands::list_hosted_server_dir,
            commands::read_hosted_server_file,
            commands::write_hosted_server_file,
            commands::list_hosted_server_config_files,
            // terracotta (陶瓦联机)
            terracotta::terracotta_detect,
            terracotta::terracotta_download,
            terracotta::terracotta_launch,
            terracotta::terracotta_stop,
            terracotta::terracotta_status,
            terracotta::terracotta_create_room,
            terracotta::terracotta_join_room,
            terracotta::terracotta_leave,
            // storage
            commands::get_storage_stats,
            commands::refresh_storage_stats,
            commands::clear_cache,
            // crash analysis
            commands::list_crash_logs,
            commands::analyze_crash_log,
            commands::get_crash_report_content,
            // pinned items (首页 / 侧边栏)
            commands::get_pins,
            commands::set_pins,
            // news
            commands::fetch_news,
            // app self-update (dynamic update source)
            updater::check_for_update,
            updater::download_update,
            updater::apply_app_update,
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

            // System tray: when the close behaviour is set to "minimize to
            // background" the window is only hidden, so without a tray icon the
            // user has no way to bring it back. Left click opens the menu,
            // double click restores the window directly.
            let show_item = MenuItemBuilder::with_id("show", "显示主窗口").build(app.handle())?;
            let quit_item = MenuItemBuilder::with_id("quit", "退出 QookiX Launcher").build(app.handle())?;
            let menu = MenuBuilder::new(app.handle())
                .item(&show_item)
                .separator()
                .item(&quit_item)
                .build()?;

            let _ = TrayIconBuilder::with_id("main")
                .icon(tauri::include_image!("icons/32x32.png"))
                .tooltip("QookiX Launcher")
                .menu(&menu)
                .on_menu_event(|app, event| match event.id().as_ref() {
                    "show" => show_main_window(app),
                    "quit" => app.exit(0),
                    _ => {}
                })
                .on_tray_icon_event(|tray, event| {
                    // Left click is reserved for the menu; double click restores.
                    if let TrayIconEvent::DoubleClick {
                        button: MouseButton::Left,
                        ..
                    } = event
                    {
                        show_main_window(tray.app_handle());
                    }
                })
                .build(app.handle());

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
        let client = crate::settings::http_client("system", None);
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
        // modern natives come as separate `...:natives-<os>` entries。
        // 注意：按「当前平台可解析出分类器」来找条目——硬编码 natives-windows
        // 会让 macOS/Linux runner 的 CI 必挂（那个条目没有本平台的分类器）。
        let native = version
            .libraries
            .iter()
            .find(|l| {
                l.name.contains("natives-")
                    && crate::install::platform_native_classifier(l).is_some()
            })
            .expect("no natives entries applicable to this platform");
        assert!(crate::install::is_native_entry(native));
        assert!(crate::install::platform_native_classifier(native).is_some());
    }

    #[tokio::test]
    async fn fabric_meta_parses() {
        let client = crate::settings::http_client("system", None);
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
            client: crate::settings::http_client("system", None),
            semaphore: Arc::new(tokio::sync::Semaphore::new(4)),
            game_pids: Arc::new(Mutex::new(HashMap::new())),
            server_pids: Arc::new(Mutex::new(HashMap::new())),
        server_senders: Arc::new(Mutex::new(HashMap::new())),
            task_counter: std::sync::atomic::AtomicU64::new(1),
            install_cancel: Arc::new(std::sync::atomic::AtomicBool::new(false)),
            ms_flow: Arc::new(Mutex::new(None)),
            java_cache: Mutex::new(None),
            terracotta: Mutex::new(None),
            pending_update: Mutex::new(None),
        };
        let instance = Instance {
            id: "test-fabric".into(),
            name: "test".into(),
            mc_version: "1.20.1".into(),
            loader: LoaderType::Fabric,
            loader_version: Some("0.15.11".into()),
            created: 0,
            last_played: None,
            total_play_time: 0,
            alias: None,
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
            group: None,
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
            client: crate::settings::http_client("system", None),
            semaphore: Arc::new(tokio::sync::Semaphore::new(4)),
            game_pids: Arc::new(Mutex::new(HashMap::new())),
            server_pids: Arc::new(Mutex::new(HashMap::new())),
        server_senders: Arc::new(Mutex::new(HashMap::new())),
            task_counter: std::sync::atomic::AtomicU64::new(1),
            install_cancel: Arc::new(std::sync::atomic::AtomicBool::new(false)),
            ms_flow: Arc::new(Mutex::new(None)),
            java_cache: Mutex::new(None),
            terracotta: Mutex::new(None),
            pending_update: Mutex::new(None),
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
