mod auto_lister;
mod commands;
mod game;
mod hover;
mod icons;
mod input_lock;
mod market_view;
mod network;
mod product;
mod quests_view;
mod stash_view;
mod sorter_view;
mod state;
mod training;
mod tray;

use tauri::{Manager, WindowEvent};
use tauri_plugin_window_state::StateFlags;

use state::AppState;

/// The log file keeps a little history for bug reports; older lines roll into one backup.
const LOG_FILE_BYTES: u128 = 2_000_000;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let result = tauri::Builder::default()
        // First, so a second launch only brings the running app forward.
        .plugin(tauri_plugin_single_instance::init(|app, _args, _cwd| tray::show_main(app)))
        .plugin(logger())
        .plugin(tauri_plugin_opener::init())
        // The main window reopens where it was (always shown: quitting from the tray leaves it
        // hidden); the overlay card places itself.
        .plugin(
            tauri_plugin_window_state::Builder::default()
                .with_denylist(&["card"])
                .with_state_flags(StateFlags::all() - StateFlags::VISIBLE)
                .build(),
        )
        .plugin(tauri_plugin_updater::Builder::new().build())
        .plugin(tauri_plugin_process::init())
        .register_uri_scheme_protocol(icons::SCHEME, icons::serve)
        .setup(|app| {
            let assets = app.path().resource_dir()?.join("assets");
            let state = AppState::load(&assets).inspect_err(|err| {
                log::error!("could not start: {err}");
                fatal(&format!("DaD Companion could not start:\n\n{err}"));
            })?;
            app.manage(state);
            app.manage(auto_lister::runtime::ListerRuntime::new(app.handle()));
            app.manage(hover::overlay::Overlay::default());
            tray::create(app.handle())?;
            hover::overlay::create(app.handle())?;
            hover::live::start(app.handle());
            network::start(app.handle());
            Ok(())
        })
        .on_window_event(|window, event| {
            if let WindowEvent::CloseRequested { api, .. } = event {
                let to_tray = window.app_handle().try_state::<AppState>().is_some_and(|s| s.close_to_tray());
                if window.label() == "main" && to_tray {
                    api.prevent_close();
                    let _ = window.hide();
                }
            }
        })
        .invoke_handler(tauri::generate_handler![
            commands::app_status,
            commands::get_settings,
            commands::set_setting,
            commands::search_items,
            commands::get_item,
            market_view::item_market,
            market_view::open_listing_counts,
            market_view::market_overview,
            stash_view::characters,
            stash_view::stash_view,
            stash_view::stash_item_card,
            stash_view::wealth,
            hover::preview_card,
            hover::demo_overlay,
            hover::overlay::card_rendered,
            network::live_status,
            sorter_view::sorter_options,
            sorter_view::sorter_save_options,
            sorter_view::sorter_preview,
            sorter_view::sorter_start,
            sorter_view::sorter_stop,
            sorter_view::sorter_status,
            sorter_view::sorter_feedback,
            quests_view::quests_overview,
            quests_view::quests_merchant,
            quests_view::quests_items,
            quests_view::quests_set_objective,
            quests_view::quests_set_done,
            auto_lister::lister_rules,
            auto_lister::save_lister_rules,
            auto_lister::lister_sources,
            auto_lister::lister_build_plan,
            auto_lister::run::lister_status,
            auto_lister::run::lister_price_from_game,
            auto_lister::run::lister_start,
            auto_lister::run::lister_stop,
            auto_lister::run::lister_collect,
            auto_lister::run::lister_crawl,
            auto_lister::run::lister_hover_test,
            auto_lister::run::lister_sell_to_merchant,
            auto_lister::run::lister_merchant_plan,
            auto_lister::run::market_data_summary,
            auto_lister::run::worth_info,
            auto_lister::run::train_worth,
        ])
        .run(tauri::generate_context!());
    if let Err(err) = result {
        log::error!("the app stopped: {err}");
        fatal(&format!("DaD Companion stopped:\n\n{err}"));
    }
}

/// Logs to `logs\app.log` in the app's data folder (and the console while developing).
fn logger<R: tauri::Runtime>() -> tauri::plugin::TauriPlugin<R> {
    use tauri_plugin_log::{RotationStrategy, Target, TargetKind};
    let mut targets = vec![Target::new(TargetKind::Stdout)];
    if let Ok(data) = appdata::DataDir::for_product(product::DATA_FOLDER) {
        targets.push(Target::new(TargetKind::Folder { path: data.root().join("logs"), file_name: Some("app".into()) }));
    }
    tauri_plugin_log::Builder::new()
        .clear_targets()
        .targets(targets)
        .level(if cfg!(debug_assertions) { log::LevelFilter::Debug } else { log::LevelFilter::Info })
        // Libraries only say what matters.
        .level_for("tao", log::LevelFilter::Warn)
        .level_for("wry", log::LevelFilter::Warn)
        .max_file_size(LOG_FILE_BYTES)
        .rotation_strategy(RotationStrategy::KeepOne)
        .build()
}

/// Tells the user why the app can't run: release builds have no console to print to.
fn fatal(message: &str) {
    #[cfg(windows)]
    {
        use windows_sys::Win32::UI::WindowsAndMessaging::{MessageBoxW, MB_ICONERROR, MB_OK};
        let wide = |s: &str| s.encode_utf16().chain(std::iter::once(0)).collect::<Vec<u16>>();
        let (text, title) = (wide(message), wide(product::NAME));
        // SAFETY: both strings are null-terminated UTF-16 that outlive the call; no owner window.
        unsafe {
            MessageBoxW(std::ptr::null_mut(), text.as_ptr(), title.as_ptr(), MB_OK | MB_ICONERROR);
        }
    }
    #[cfg(not(windows))]
    eprintln!("{message}");
}
