// Compiled by the lifecycle tests only. All files are synthetic and live in a temp directory.
use std::{fs, io::Write, path::PathBuf, thread, time::Duration};

fn main() {
    let input = PathBuf::from(std::env::args_os().last().unwrap());
    let mode = input.file_stem().unwrap().to_str().unwrap();
    let mut options = fs::OpenOptions::new();
    options.create(true).write(true).truncate(false);
    #[cfg(windows)]
    {
        use std::os::windows::fs::OpenOptionsExt;
        options.share_mode(0);
    }
    let lock = options.open(input.parent().unwrap().join("active.lock"));
    let lock = match lock {
        Ok(lock) => lock,
        Err(_) => {
            fs::write(input.parent().unwrap().join("overlap"), b"overlap").unwrap();
            std::process::exit(93);
        }
    };
    #[cfg(unix)]
    {
        use std::os::fd::AsRawFd;
        unsafe extern "C" {
            fn flock(fd: i32, operation: i32) -> i32;
        }
        if unsafe { flock(lock.as_raw_fd(), 2 | 4) } != 0 {
            fs::write(input.parent().unwrap().join("overlap"), b"overlap").unwrap();
            std::process::exit(93);
        }
    }
    fs::write(
        input.with_extension("started"),
        std::process::id().to_string(),
    )
    .unwrap();
    if mode.starts_with("slow") {
        // A partial pipe write exercises cancellation while output readers are waiting.
        print!("{{");
        std::io::stdout().flush().unwrap();
        thread::sleep(Duration::from_secs(60));
    } else if mode == "failure" {
        std::process::exit(7);
    } else if mode == "oversized" {
        std::io::stdout()
            .write_all(&vec![b'x'; 4 * 1024 * 1024 + 1])
            .unwrap();
        thread::sleep(Duration::from_secs(60));
    } else {
        println!("{{\"format\":{{\"format_name\":\"{mode}\"}},\"streams\":[]}}");
    }
    drop(lock);
}
