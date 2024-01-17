#![cfg_attr(
    all(not(debug_assertions), target_os = "windows"),
    windows_subsystem = "windows"
)]

use std::{
    env,
    sync::{Arc, Mutex},
    time::Duration,
};

mod constants;
mod development;
mod github;
mod mod_management;
mod northstar;
mod platform_specific;
mod repair_and_verify;
mod thunderstore;
mod util;

use serde::{Deserialize, Serialize};
#[cfg(target_os = "windows")]
use tauri::api::dialog::blocking::MessageDialogBuilder;
#[cfg(target_os = "windows")]
use tauri::api::dialog::{MessageDialogButtons, MessageDialogKind};
use tauri::Manager;
use tokio::time::sleep;
use ts_rs::TS;

#[derive(Serialize, Deserialize, Debug, Clone, TS)]
#[ts(export)]
struct NorthstarThunderstoreRelease {
    package: String,
    version: String,
}

#[derive(Serialize, Deserialize, Debug, Clone, TS)]
#[ts(export)]
pub struct NorthstarThunderstoreReleaseWrapper {
    label: String,
    value: NorthstarThunderstoreRelease,
}

#[derive(Default)]
struct Counter(Arc<Mutex<i32>>);

fn main() {
    // Setup logger
    let mut log_builder = pretty_env_logger::formatted_builder();
    log_builder.parse_filters("info");
    let logger = sentry_log::SentryLogger::with_dest(log_builder.build());

    log::set_boxed_logger(Box::new(logger)).unwrap();
    log::set_max_level(log::LevelFilter::Info);

    // Only enable Sentry crash logs on release
    #[cfg(not(debug_assertions))]
    let _guard = sentry::init((
        "https://f833732deb2240b0b2dc4abce97d0f1d@o1374052.ingest.sentry.io/6692177",
        sentry::ClientOptions {
            release: sentry::release_name!(),
            attach_stacktrace: true,
            ..Default::default()
        },
    ));

    match tauri::Builder::default()
        .plugin(tauri_plugin_store::Builder::default().build())
        .setup(|app| {
            let app_handle = app.app_handle();
            tauri::async_runtime::spawn(async move {
                loop {
                    sleep(Duration::from_millis(2000)).await;
                    // println!("sending backend ping");
                    app_handle.emit_all("backend-ping", "ping").unwrap();
                }
            });
            let app_handle = app.app_handle();
            tauri::async_runtime::spawn(async move {
                loop {
                    sleep(Duration::from_millis(2000)).await;
                    app_handle
                        .emit_all(
                            "ea-app-running-ping",
                            util::check_ea_app_or_origin_running(),
                        )
                        .unwrap();
                }
            });
            let app_handle = app.app_handle();
            tauri::async_runtime::spawn(async move {
                loop {
                    sleep(Duration::from_millis(2000)).await;
                    app_handle
                        .emit_all("northstar-running-ping", util::check_northstar_running())
                        .unwrap();
                }
            });

            // Emit updated player and server count to GUI
            let app_handle = app.app_handle();
            tauri::async_runtime::spawn(async move {
                loop {
                    sleep(constants::REFRESH_DELAY).await;
                    app_handle
                        .emit_all(
                            "northstar-statistics",
                            util::get_server_player_count().await,
                        )
                        .unwrap();
                }
            });

            Ok(())
        })
        .manage(Counter(Default::default()))
        .invoke_handler(tauri::generate_handler![
            development::install_git_main,
            github::compare_tags,
            github::get_list_of_tags,
            github::pull_requests::apply_launcher_pr,
            github::pull_requests::apply_mods_pr,
            github::pull_requests::get_launcher_download_link,
            github::pull_requests::get_pull_requests_wrapper,
            github::release_notes::check_is_flightcore_outdated,
            github::release_notes::get_newest_flightcore_version,
            github::release_notes::get_northstar_release_notes,
            mod_management::delete_northstar_mod,
            mod_management::delete_thunderstore_mod,
            mod_management::get_installed_mods_and_properties,
            mod_management::install_mod_wrapper,
            mod_management::set_mod_enabled_status,
            northstar::check_is_northstar_outdated,
            northstar::get_available_northstar_versions,
            northstar::get_northstar_version_number,
            northstar::install::find_game_install_location,
            northstar::install::install_northstar_wrapper,
            northstar::install::update_northstar,
            northstar::launch_northstar_steam,
            northstar::launch_northstar,
            northstar::profile::delete_profile,
            northstar::profile::fetch_profiles,
            northstar::profile::validate_profile,
            platform_specific::get_host_os,
            platform_specific::get_local_northstar_proton_wrapper_version,
            platform_specific::install_northstar_proton_wrapper,
            platform_specific::linux_checks,
            platform_specific::uninstall_northstar_proton_wrapper,
            repair_and_verify::clean_up_download_folder_wrapper,
            repair_and_verify::disable_all_but_core,
            repair_and_verify::get_log_list,
            repair_and_verify::verify_game_files,
            repair_and_verify::verify_install_location,
            thunderstore::query_thunderstore_packages_api,
            util::close_application,
            util::force_panic,
            util::get_flightcore_version_number,
            util::get_server_player_count,
            util::is_debug_mode,
            util::kill_northstar,
            util::open_repair_window,
        ])
        .run(tauri::generate_context!())
    {
        Ok(()) => (),
        Err(err) => {
            // Failed to launch system native web view

            // Log error on Linux
            #[cfg(not(target_os = "windows"))]
            {
                log::error!("{err}");
            }

            // On Windows we can show an error window using Windows API to show how to install WebView2
            #[cfg(target_os = "windows")]
            {
                log::error!("WebView2 not installed: {err}");
                let dialog = MessageDialogBuilder::new(
                    "WebView2 not found",
                    "FlightCore requires WebView2 to run.\n\nClick OK to open installation instructions."
                )
                .kind(MessageDialogKind::Error)
                .buttons(MessageDialogButtons::Ok);

                if dialog.show() {
                    // Open the installation instructions URL in the user's default web browser
                    open::that("https://github.com/R2NorthstarTools/FlightCore/blob/main/docs/TROUBLESHOOTING.md#flightcore-wont-launch").unwrap();
                }
            }
        }
    };
}

/// Defines how Titanfall2 was installed (Steam, Origin, ...)
#[derive(Serialize, Deserialize, Debug, Clone, TS)]
#[ts(export)]
pub enum InstallType {
    STEAM,
    ORIGIN,
    EAPLAY,
    UNKNOWN,
}

/// Object holding information of the Titanfall2 install, including
/// - Install path
/// - Active profile
/// - Type of installation (Steam, Origin, ...)
#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct GameInstall {
    pub game_path: String,
    pub profile: String,
    pub install_type: InstallType,
}

/// Object holding various information about a Northstar mod
#[derive(Serialize, Deserialize, Debug, Clone, TS)]
#[ts(export)]
pub struct NorthstarMod {
    pub name: String,
    pub version: Option<String>,
    pub thunderstore_mod_string: Option<String>,
    pub enabled: bool,
    pub directory: String,
}
