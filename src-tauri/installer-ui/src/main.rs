#![cfg_attr(windows, windows_subsystem = "windows")]

#[cfg(windows)]
mod windows;

#[cfg(windows)]
fn main() {
    if let Err(error) = windows::run() {
        eprintln!("{error}");
        std::process::exit(2);
    }
}

#[cfg(not(windows))]
fn main() {
    eprintln!("QookiXInstallerUI is only available on Windows");
}
