use std::{
    env, fs, io,
    path::{Path, PathBuf},
    process::{Command, Stdio},
    time::{Duration, Instant},
};

mod core;
mod renderers;

use core::{
    available_themes, discover_themes, prepare_switch, prepare_wallpaper_command, publish_switch,
    validate_name,
};

const THEME_FILE: &str = "theme.toml";
const BASE16_FILE: &str = "base16.yaml";

fn main() {
    if let Err(err) = run(env::args().skip(1)) {
        if let Some(reason) = err.strip_prefix("UNAVAILABLE: ") {
            eprintln!("unavailable: {reason}");
        } else {
            eprintln!("error: {err}");
        }
        std::process::exit(1);
    }
}
fn run(args: impl IntoIterator<Item = String>) -> Result<(), String> {
    let mut args = args.into_iter();
    match (args.next().as_deref(), args.next(), args.next()) {
        (Some("switch"), Some(name), None) => mutate(|root| switch_command(root, &name)),
        (Some("wallpaper"), Some(selection), None) => mutate(|root| wallpaper_command(root, &selection)),
        (Some("available"), None, None) => available_command()
            .map_err(|err| io::Error::other(format!("UNAVAILABLE: {err}")) ),
        (Some("list"), None, None) => list_themes(&root_dir()?),
        (Some("install"), Some(repo), None) => mutate(|root| install_repo(root, &repo)),
        _ => Err(io::Error::new(io::ErrorKind::InvalidInput, "usage: retheme available | retheme list | retheme switch <name> | retheme wallpaper <restore|next|prev|INDEX> | retheme install <repository-url>")),
    }.map_err(|e| e.to_string())
}

fn available_command() -> io::Result<()> {
    let root = root_dir().map_err(io::Error::other)?;
    let count = available_themes(&root)?;
    println!("available: {count} valid theme pack(s)");
    Ok(())
}

fn root_dir() -> Result<PathBuf, String> {
    let root = env::var_os("RETHEME_ROOT").map(|value| absolute_path("RETHEME_ROOT", value));
    let config =
        env::var_os("XDG_CONFIG_HOME").map(|value| absolute_path("XDG_CONFIG_HOME", value));
    let home = env::var_os("HOME").map(|value| absolute_path("HOME", value));
    let root = root.transpose()?;
    let config = config.transpose()?;
    let home = home.transpose()?;
    if let Some(root) = root {
        return Ok(root);
    }
    if let Some(config) = config {
        return Ok(config.join("reEnvisioning"));
    }
    Ok(home
        .ok_or("set RETHEME_ROOT, XDG_CONFIG_HOME, or HOME")?
        .join(".config/reEnvisioning"))
}

fn absolute_path(name: &str, value: std::ffi::OsString) -> Result<PathBuf, String> {
    let path = PathBuf::from(value);
    if path.as_os_str().is_empty() || !path.is_absolute() {
        return Err(format!("{name} must be a non-empty absolute path"));
    }
    Ok(path)
}

fn mutate(operation: impl FnOnce(&Path) -> io::Result<()>) -> io::Result<()> {
    let root = root_dir().map_err(io::Error::other)?;
    fs::create_dir_all(&root)?;
    let _lock = RuntimeLock::acquire(&root)?;
    operation(&root)
}

struct RuntimeLock {
    _file: fs::File,
}
impl RuntimeLock {
    fn acquire(root: &Path) -> io::Result<Self> {
        let path = root.join(".lock");
        if let Ok(metadata) = fs::symlink_metadata(&path) {
            if metadata.file_type().is_symlink() || !metadata.is_file() {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidInput,
                    "runtime lock must be a regular file",
                ));
            }
        }
        let file = fs::OpenOptions::new()
            .create(true)
            .read(true)
            .write(true)
            .open(path)?;
        #[cfg(target_os = "linux")]
        unsafe {
            if flock(std::os::fd::AsRawFd::as_raw_fd(&file), 2 | 4) != 0 {
                return Err(io::Error::new(
                    io::ErrorKind::WouldBlock,
                    "another install/switch/wallpaper operation is running",
                ));
            }
        }
        Ok(Self { _file: file })
    }
}
#[cfg(target_os = "linux")]
extern "C" {
    fn flock(fd: i32, operation: i32) -> i32;
}

fn switch_command(root: &Path, name: &str) -> io::Result<()> {
    renderers::validate_wallpaper_backend()?;
    let prepared = prepare_switch(root, name)?;
    core::validate_active(root)?;
    let wallpaper = prepared.wallpaper_state.clone().map(|state| state.1);
    let colors = prepared.colors.clone();
    let dark = prepared.metadata.dark;
    publish_switch(root, prepared)?;
    renderers::render_fixed(root, &colors, dark)?;
    if let Some(path) = wallpaper {
        renderers::apply_wallpaper(&path)?;
    }
    renderers::apply_renderers(dark);
    println!("switched to {name}");
    Ok(())
}

fn wallpaper_command(root: &Path, argument: &str) -> io::Result<()> {
    renderers::validate_wallpaper_backend()?;
    let path = prepare_wallpaper_command(root, argument)?;
    renderers::apply_wallpaper(&path)?;
    Ok(())
}

fn list_themes(root: &Path) -> io::Result<()> {
    for name in discover_themes(root)? {
        println!("{name}");
    }
    Ok(())
}
fn install_repo(root: &Path, repo: &str) -> io::Result<()> {
    let name = repo_name_from_url(repo)?;
    let cache = root.join("cache");
    fs::create_dir_all(&cache)?;
    let tmp = unique_path(&cache, "clone")?;
    let mut child = Command::new("git")
        .args([
            "clone",
            "--depth",
            "1",
            "--single-branch",
            "--no-tags",
            "--",
            repo,
        ])
        .arg(&tmp)
        .env("GIT_TERMINAL_PROMPT", "0")
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env("GIT_SSH_COMMAND", "ssh -oBatchMode=yes")
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::inherit())
        .spawn()?;
    let deadline = Instant::now() + Duration::from_secs(30);
    let status = loop {
        if let Some(status) = child.try_wait()? {
            break status;
        }
        if Instant::now() >= deadline {
            let _ = child.kill();
            let _ = child.wait();
            let cleanup = remove_path(&tmp);
            return match cleanup {
                Ok(()) => Err(io::Error::new(
                    io::ErrorKind::TimedOut,
                    "git clone timed out",
                )),
                Err(error) => Err(io::Error::other(format!(
                    "git clone timed out; cleanup failed: {error}"
                ))),
            };
        }
        std::thread::sleep(Duration::from_millis(25));
    };
    if !status.success() {
        let cleanup = remove_path(&tmp);
        return match cleanup {
            Ok(()) => Err(io::Error::other("git clone failed")),
            Err(error) => Err(io::Error::other(format!(
                "git clone failed; cleanup failed: {error}"
            ))),
        };
    }
    let result = install_from_dir(root, &tmp, &name);
    let cleanup = remove_path(&tmp);
    match (result, cleanup) {
        (Err(error), Ok(())) => Err(error),
        (Err(error), Err(cleanup)) => Err(io::Error::other(format!(
            "{error}; clone cleanup failed: {cleanup}"
        ))),
        (Ok(_), Ok(())) => Ok(()),
        (Ok(installed), Err(error)) => {
            let mut rollback = Vec::new();
            for name in installed {
                if let Err(restore) = remove_path(&root.join("themes").join(name)) {
                    rollback.push(restore.to_string());
                }
            }
            if rollback.is_empty() {
                Err(error)
            } else {
                Err(io::Error::other(format!(
                    "clone cleanup failed: {error}; install rollback failed: {}",
                    rollback.join(", ")
                )))
            }
        }
    }
}
fn install_from_dir(root: &Path, source: &Path, fallback: &str) -> io::Result<Vec<String>> {
    let found: Vec<(PathBuf, String)> =
        if source.join(THEME_FILE).exists() || source.join(BASE16_FILE).exists() {
            validate_name(fallback)?;
            core::validate_pack_dir_for_install(source)?;
            vec![(source.to_path_buf(), fallback.into())]
        } else {
            let mut found = Vec::new();
            for entry in fs::read_dir(source)? {
                let entry = entry?;
                let name = entry.file_name();
                let file_type = entry.file_type()?;
                if file_type.is_symlink() || (!file_type.is_file() && !file_type.is_dir()) {
                    return Err(io::Error::new(
                        io::ErrorKind::InvalidData,
                        "unsupported repository entry",
                    ));
                }
                if name == ".git" {
                    if !file_type.is_dir() {
                        return Err(io::Error::new(
                            io::ErrorKind::InvalidData,
                            ".git must be a directory",
                        ));
                    }
                    continue;
                }
                if file_type.is_dir() {
                    let name = name.to_str().ok_or_else(|| {
                        io::Error::new(io::ErrorKind::InvalidData, "theme name is not UTF-8")
                    })?;
                    validate_name(name)?;
                    core::validate_pack_dir(&entry.path())?;
                    found.push((entry.path(), name.into()));
                }
            }
            found
        };
    if found.is_empty() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "repository contains no complete theme packs".to_string(),
        ));
    }
    for (source, name) in &found {
        core::parse_theme_metadata(&fs::read_to_string(source.join(THEME_FILE))?, name)?;
        core::parse_base16(&fs::read_to_string(source.join(BASE16_FILE))?)?;
        core::read_wallpapers(source)?;
        preflight_dir(source)?;
    }
    let themes = root.join("themes");
    for (_, name) in &found {
        if themes.join(name).exists() {
            return Err(io::Error::new(
                io::ErrorKind::AlreadyExists,
                format!("theme already exists: {}", themes.join(name).display()),
            ));
        }
    }
    fs::create_dir_all(&themes)?;
    let stage = unique_path(&themes, "install")?;
    fs::create_dir(&stage)?;
    let names: Vec<String> = found.iter().map(|(_, name)| name.clone()).collect();
    let mut installed = Vec::new();
    let result = (|| {
        for (src, name) in found {
            copy_dir(&src, &stage.join(&name))?;
        }
        for name in &names {
            core::validate_staged_pack(&stage.join(name), name)?;
        }
        for name in &names {
            rename_noreplace(&stage.join(name), &themes.join(name))?;
            installed.push(name.clone());
        }
        installed.sort();
        println!("installed {}", installed.join(", "));
        Ok(installed.clone())
    })();
    let cleanup = remove_path(&stage);
    let installed = match result {
        Ok(installed) => installed,
        Err(error) => {
            let mut rollback_errors = Vec::new();
            for name in &installed {
                if let Err(rollback) = remove_path(&themes.join(name)) {
                    rollback_errors.push(format!("{}: {rollback}", themes.join(name).display()));
                }
            }
            return match (rollback_errors.is_empty(), cleanup) {
                (true, Ok(())) => Err(error),
                (true, Err(cleanup)) => Err(io::Error::other(format!(
                    "{error}; install staging cleanup failed: {cleanup}"
                ))),
                (false, Ok(())) => Err(io::Error::other(format!(
                    "{error}; install rollback failed: {}",
                    rollback_errors.join(", ")
                ))),
                (false, Err(cleanup)) => Err(io::Error::other(format!(
                    "{error}; install rollback failed: {}; staging cleanup failed: {cleanup}",
                    rollback_errors.join(", ")
                ))),
            };
        }
    };
    match cleanup {
        Ok(()) => Ok(installed),
        Err(error) => {
            let mut rollback_errors = Vec::new();
            for name in &installed {
                if let Err(rollback) = remove_path(&themes.join(name)) {
                    rollback_errors.push(format!("{}: {rollback}", themes.join(name).display()));
                }
            }
            if rollback_errors.is_empty() {
                Err(error)
            } else {
                Err(io::Error::other(format!(
                    "install cleanup failed: {error}; rollback failed: {}",
                    rollback_errors.join(", ")
                )))
            }
        }
    }
}
fn preflight_dir(dir: &Path) -> io::Result<()> {
    for entry in fs::read_dir(dir)? {
        let entry = entry?;
        if entry.file_name() == ".git" {
            continue;
        }
        let ty = entry.file_type()?;
        if ty.is_symlink() || (!ty.is_file() && !ty.is_dir()) {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "unsupported repository entry",
            ));
        }
        if ty.is_dir() {
            preflight_dir(&entry.path())?;
        }
    }
    Ok(())
}
fn unique_path(parent: &Path, kind: &str) -> io::Result<PathBuf> {
    for n in 0..100 {
        let candidate = parent.join(format!(".retheme-{kind}-{}-{n}", std::process::id()));
        if fs::symlink_metadata(&candidate).is_err() {
            return Ok(candidate);
        }
    }
    Err(io::Error::new(
        io::ErrorKind::AlreadyExists,
        "temporary path unavailable",
    ))
}

#[cfg(target_os = "linux")]
fn rename_noreplace(source: &Path, destination: &Path) -> io::Result<()> {
    use std::{ffi::CString, os::unix::ffi::OsStrExt};
    unsafe extern "C" {
        fn renameat2(
            old_dirfd: i32,
            old: *const i8,
            new_dirfd: i32,
            new: *const i8,
            flags: u32,
        ) -> i32;
    }
    let source = CString::new(source.as_os_str().as_bytes())
        .map_err(|_| io::Error::other("source contains NUL"))?;
    let destination = CString::new(destination.as_os_str().as_bytes())
        .map_err(|_| io::Error::other("destination contains NUL"))?;
    if unsafe { renameat2(-100, source.as_ptr(), -100, destination.as_ptr(), 1) } == 0 {
        return Ok(());
    }
    let error = io::Error::last_os_error();
    if matches!(error.raw_os_error(), Some(18) | Some(22)) {
        return Err(io::Error::new(
            io::ErrorKind::Unsupported,
            "atomic no-replace rename is unavailable",
        ));
    }
    Err(error)
}

#[cfg(not(target_os = "linux"))]
fn rename_noreplace(source: &Path, destination: &Path) -> io::Result<()> {
    if fs::symlink_metadata(destination).is_ok() {
        return Err(io::Error::new(
            io::ErrorKind::AlreadyExists,
            "destination already exists",
        ));
    }
    fs::rename(source, destination)
}

fn remove_path(path: &Path) -> io::Result<()> {
    match fs::symlink_metadata(path) {
        Ok(metadata) if metadata.is_dir() => fs::remove_dir_all(path),
        Ok(_) => fs::remove_file(path),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(error),
    }
}

fn copy_dir(src: &Path, dest: &Path) -> io::Result<()> {
    copy_dir_inner(src, dest, true)
}

fn copy_dir_inner(src: &Path, dest: &Path, skip_git: bool) -> io::Result<()> {
    fs::create_dir_all(dest)?;
    for entry in fs::read_dir(src)? {
        let entry = entry?;
        if skip_git && entry.file_name() == ".git" {
            continue;
        }
        let target = dest.join(entry.file_name());
        if entry.file_type()?.is_dir() {
            copy_dir_inner(&entry.path(), &target, false)?;
        } else if entry.file_type()?.is_file() {
            fs::copy(entry.path(), target)?;
        } else {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "unsupported repository entry",
            ));
        }
    }
    Ok(())
}
fn repo_name_from_url(repo: &str) -> io::Result<String> {
    let path = if let Some((scheme, rest)) = repo.split_once("://") {
        match scheme.to_ascii_lowercase().as_str() {
            "file" => rest.strip_prefix('/').filter(|p| !p.is_empty()),
            "https" | "ssh" | "git" => rest
                .split_once('/')
                .and_then(|(h, p)| (!h.is_empty() && !p.is_empty()).then_some(p)),
            _ => None,
        }
        .ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::InvalidInput,
                "expected complete repository URL",
            )
        })?
    } else if let Some((host, path)) = repo.split_once(':') {
        let host = host.rsplit_once('@').map_or(host, |(_, h)| h);
        if host.is_empty() || host.contains('/') || path.is_empty() {
            return bad_url();
        }
        path
    } else {
        return bad_url();
    };
    let name = path
        .split(['?', '#'])
        .next()
        .unwrap_or("")
        .trim_end_matches('/')
        .rsplit('/')
        .next()
        .unwrap_or("")
        .strip_suffix(".git")
        .unwrap_or("");
    validate_name(name)?;
    Ok(name.into())
}
fn bad_url<T>() -> io::Result<T> {
    Err(io::Error::new(
        io::ErrorKind::InvalidInput,
        "expected complete repository URL",
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn runtime_paths_must_be_absolute() {
        assert!(absolute_path("HOME", "relative".into()).is_err());
        assert!(absolute_path("HOME", "".into()).is_err());
        assert_eq!(
            absolute_path("HOME", "/tmp/home".into()).unwrap(),
            PathBuf::from("/tmp/home")
        );
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn runtime_lock_rejects_second_holder() {
        let root = env::temp_dir().join(format!("retheme-lock-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).unwrap();
        let first = RuntimeLock::acquire(&root).unwrap();
        assert!(matches!(
            RuntimeLock::acquire(&root),
            Err(error) if error.kind() == io::ErrorKind::WouldBlock
        ));
        drop(first);
        let _ = fs::remove_dir_all(root);
    }
}
