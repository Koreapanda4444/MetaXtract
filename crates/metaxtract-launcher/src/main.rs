#![cfg_attr(target_os = "windows", windows_subsystem = "windows")]
#![forbid(unsafe_code)]
fn main() -> std::process::ExitCode {
    let result = (|| -> std::io::Result<_> {
        let executable = std::env::current_exe()?;
        let root = executable
            .parent()
            .ok_or_else(|| std::io::Error::other("Application directory unavailable"))?;
        let mut args = std::env::args_os().skip(1).peekable();
        let cli = args.peek().is_some_and(|arg| arg == "--cli");
        if cli {
            args.next();
        }
        let name = if cli { "metaxtract" } else { "metaxtract-gui" };
        let name = format!("{name}{}", std::env::consts::EXE_SUFFIX);
        let target = root.join("runtime/bin").join(name);
        if !std::fs::symlink_metadata(&target)?.is_file() {
            return Err(std::io::Error::other(
                "Bundled application is not a regular file",
            ));
        }
        std::process::Command::new(target).args(args).status()
    })();
    match result {
        Ok(status) => std::process::ExitCode::from(status.code().unwrap_or(1).clamp(0, 255) as u8),
        Err(error) => {
            eprintln!("MetaXtract could not start: {error}");
            std::process::ExitCode::FAILURE
        }
    }
}
