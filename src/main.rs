mod capture;
mod config;
mod editor;
mod hotkey;
mod raster;

use std::borrow::Cow;
use std::fs::{File, OpenOptions};
use std::io;
use std::os::fd::AsRawFd;
use std::path::Path;
use std::time::Duration;

const USAGE: &str = "lightshot capture\nlightshot install-hotkey\nlightshot uninstall-hotkey\nDefault hotkey <Ctrl>Print";
const NOTIFY_GRACE: Duration = Duration::from_secs(30);

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let args: Vec<&str> = args.iter().map(String::as_str).collect();
    let code = match args.as_slice() {
        [] | ["capture"] => run_capture(),
        ["install-hotkey"] => run_hotkey(hotkey::install),
        ["uninstall-hotkey"] => run_hotkey(hotkey::uninstall),
        ["--help"] | ["-h"] => {
            println!("{USAGE}");
            0
        }
        _ => {
            eprintln!("{USAGE}");
            2
        }
    };
    std::process::exit(code);
}

fn run_hotkey(f: fn() -> Result<(), String>) -> i32 {
    match f() {
        Ok(()) => 0,
        Err(e) => {
            eprintln!("lightshot: {e}");
            1
        }
    }
}

fn acquire_lock(dir: &Path) -> io::Result<Option<File>> {
    std::fs::create_dir_all(dir)?;
    let file = OpenOptions::new()
        .create(true)
        .truncate(false)
        .write(true)
        .open(dir.join("capture.lock"))?;
    if unsafe { libc::flock(file.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) } == 0 {
        return Ok(Some(file));
    }
    let err = io::Error::last_os_error();
    if err.raw_os_error() == Some(libc::EWOULDBLOCK) {
        Ok(None)
    } else {
        Err(err)
    }
}

fn run_capture() -> i32 {
    let dir = config::config_dir();
    let lock = match acquire_lock(&dir) {
        Ok(Some(lock)) => lock,
        Ok(None) => return 0,
        Err(e) => {
            eprintln!("lightshot: lock {}: {e}", dir.join("capture.lock").display());
            return 1;
        }
    };
    let cfg = config::load();
    raster::font();
    let img = match capture::capture() {
        Ok(img) => img,
        Err(e) => {
            eprintln!("lightshot: capture: {e}");
            return 1;
        }
    };
    let outcome = match editor::run(img, cfg) {
        Ok(o) => o,
        Err(e) => {
            eprintln!("lightshot: editor: {e}");
            return 1;
        }
    };
    drop(lock);
    if let Some(img) = outcome.clipboard {
        hold_clipboard(&img);
    }
    editor::wait_notifications(NOTIFY_GRACE);
    0
}

/// Linux clipboards are served by the owning process, so a forked child keeps
/// the last copied image available until another client replaces it.
fn hold_clipboard(img: &image::RgbaImage) {
    let serve = || {
        use arboard::SetExtLinux;
        if let Ok(mut cb) = arboard::Clipboard::new() {
            let _ = cb.set().wait().image(arboard::ImageData {
                width: img.width() as usize,
                height: img.height() as usize,
                bytes: Cow::Borrowed(img.as_raw()),
            });
        }
    };
    match unsafe { libc::fork() } {
        0 => {
            unsafe { libc::setsid() };
            serve();
            unsafe { libc::_exit(0) };
        }
        -1 => serve(),
        _ => {}
    }
}
