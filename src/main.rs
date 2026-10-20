#![cfg_attr(windows, windows_subsystem = "windows")]

mod app_icon;
mod cli;
mod device_config;
mod crop;
mod hardware_monitor;
mod hidden_command;
mod kanali_assets;
mod link;
mod media;
mod paths;
mod protocol;
mod sensors;
mod service;
mod service_control;
mod settings;
mod status;
mod theme_video;
mod tray;
#[cfg(not(windows))]
mod turris;
mod ui;
#[cfg(windows)]
mod windows_process;

use clap::Parser;
use cli::{Cli, Command};
use settings::SettingsStore;
use std::process::ExitCode;
use std::time::Duration;

const HARDWARE_MONITOR_GRACE: Duration = Duration::from_secs(30);

fn main() -> ExitCode {
    #[cfg(windows)]
    if std::env::args_os().len() > 1 {
        windows_process::attach_parent_console();
    }
    paths::move_settings_from_old_name();
    let store = SettingsStore::new(paths::settings_path());
    match Cli::parse().command.unwrap_or(Command::Ui { page: ui::StartPage::Home }) {
        Command::Ui { page } => run_window(store, page),
        Command::Service { port } => run_service(store, port),
        Command::Icon { path } => print_outcome(app_icon::save_png(&path).map(|()| format!("wrote {}", path.display()))),
        settings_command => print_outcome(cli::run_settings_command(settings_command, &store)),
    }
}

fn run_window(store: SettingsStore, page: ui::StartPage) -> ExitCode {
    match ui::run(store, paths::status_path(), page) {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => print_outcome(Err(format!("cannot open window: {error}"))),
    }
}

fn run_service(store: SettingsStore, port: Option<String>) -> ExitCode {
    #[cfg(windows)]
    if let Err(error) = prepare_windows_service() {
        return print_outcome(Err(error));
    }
    let screen_link = std::thread::spawn(move || {
        let status = status::StatusWriter::new(paths::status_path());
        service::ScreenService::new(store, status, port).run_forever();
    });
    std::thread::spawn(start_hardware_monitor_after_login);
    if let Err(error) = tray::run() {
        eprintln!("canalis: {error}; running without a tray icon");
    }
    let _never_finishes = screen_link.join();
    ExitCode::SUCCESS
}

/// LibreHardwareMonitor's own login task gets the first chance to start it.
fn start_hardware_monitor_after_login() {
    std::thread::sleep(HARDWARE_MONITOR_GRACE);
    match hardware_monitor::ensure_running() {
        Ok(Some(message)) => eprintln!("canalis: {message}"),
        Ok(None) => {}
        Err(error) => eprintln!("canalis: {error}"),
    }
}

/// Only one service may talk to the screen. It runs detached from the terminal or installer that
/// started it, which would otherwise end it when closed, so its messages go to a log file.
#[cfg(windows)]
fn prepare_windows_service() -> Result<(), String> {
    if !windows_process::claim_single_instance(r"Local\CanalisService") {
        return Err("the service is already running".into());
    }
    windows_process::detach_console();
    let log_file = paths::log_path();
    windows_process::redirect_output_to(&log_file).map_err(|error| format!("cannot open log file {}: {error}", log_file.display()))
}

fn print_outcome(outcome: Result<String, String>) -> ExitCode {
    match outcome {
        Ok(message) => {
            println!("{message}");
            ExitCode::SUCCESS
        }
        Err(error) => {
            eprintln!("canalis: {error}");
            ExitCode::from(2)
        }
    }
}
