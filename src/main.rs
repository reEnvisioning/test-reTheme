use std::{
    env, fs, io,
    path::{Path, PathBuf},
    process::Command,
};

mod core;
mod renderers;

use core::{
    discover_themes, prepare_switch, prepare_wallpaper_command, publish_switch, validate_name,
};

const THEME_FILE: &str = "theme.toml";
const BASE16_FILE: &str = "base16.yaml";

fn main() {
    if let Err(err) = run(env::args().skip(1)) {
        eprintln!("error: {err}");
        std::process::exit(1);
    }
}
fn run(args: impl IntoIterator<Item = String>) -> Result<(), String> {
    let mut args = args.into_iter();
    match (args.next().as_deref(), args.next(), args.next()) {
        (Some("switch"), Some(name), None) => switch_command(&root_dir()?, &name),
        (Some("wallpaper"), Some(selection), None) => wallpaper_command(&root_dir()?, &selection),
        (Some("list"), None, None) => list_themes(&root_dir()?),
        (Some("install"), Some(repo), None) => install_repo(&root_dir()?, &repo),
        _ => Err(io::Error::new(io::ErrorKind::InvalidInput, "usage: retheme list | retheme switch <name> | retheme wallpaper <restore|next|prev|INDEX> | retheme install <repository-url>")),
    }.map_err(|e| e.to_string())
}

fn root_dir() -> Result<PathBuf, String> {
    if let Ok(root) = env::var("RETHEME_ROOT") {
        return Ok(root.into());
    }
    if let Ok(config) = env::var("XDG_CONFIG_HOME") {
        return Ok(PathBuf::from(config).join("reEnvisioning"));
    }
    Ok(
        PathBuf::from(env::var("HOME").map_err(|_| "set RETHEME_ROOT, XDG_CONFIG_HOME, or HOME")?)
            .join(".config/reEnvisioning"),
    )
}

fn switch_command(root: &Path, name: &str) -> io::Result<()> {
    let prepared = prepare_switch(root, name)?;
    if let Err(err) = renderers::render_fixed(root, &prepared.colors, prepared.metadata.dark) {
        eprintln!("warning: optional renderers skipped: {err}");
    }
    let wallpaper = prepared.wallpaper_state.clone().map(|state| state.1);
    let dark = prepared.metadata.dark;
    publish_switch(root, prepared)?;
    if let Some(path) = wallpaper {
        renderers::apply_wallpaper(&path);
    }
    renderers::apply_renderers(dark);
    println!("switched to {name}");
    Ok(())
}

fn wallpaper_command(root: &Path, argument: &str) -> io::Result<()> {
    let path = prepare_wallpaper_command(root, argument)?;
    renderers::apply_wallpaper(&path);
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
    let tmp = cache.join(format!("retheme-install-{}", std::process::id()));
    let _ = fs::remove_dir_all(&tmp);
    let status = Command::new("git")
        .args(["clone", "--depth", "1", "--", repo])
        .arg(&tmp)
        .status()?;
    if !status.success() {
        let _ = fs::remove_dir_all(&tmp);
        return Err(io::Error::other("git clone failed"));
    }
    let result = install_from_dir(root, &tmp, &name);
    let _ = fs::remove_dir_all(&tmp);
    result
}
fn install_from_dir(root: &Path, source: &Path, fallback: &str) -> io::Result<()> {
    let found = if source.join(THEME_FILE).is_file() && source.join(BASE16_FILE).is_file() {
        validate_name(fallback)?;
        vec![(source.to_path_buf(), fallback.into())]
    } else {
        let mut found = Vec::new();
        for entry in fs::read_dir(source)? {
            let entry = entry?;
            let name = entry.file_name();
            let Some(name) = name.to_str() else { continue };
            if name != ".git"
                && entry.file_type()?.is_dir()
                && entry.path().join(THEME_FILE).is_file()
                && entry.path().join(BASE16_FILE).is_file()
            {
                validate_name(name)?;
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
    for (source, _) in &found {
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
    let mut installed: Vec<String> = Vec::new();
    for (src, name) in found {
        copy_dir(&src, &themes.join(&name))?;
        installed.push(name);
    }
    installed.sort();
    println!("installed {}", installed.join(", "));
    Ok(())
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
fn copy_dir(src: &Path, dest: &Path) -> io::Result<()> {
    fs::create_dir_all(dest)?;
    for entry in fs::read_dir(src)? {
        let entry = entry?;
        if entry.file_name() == ".git" {
            continue;
        }
        let target = dest.join(entry.file_name());
        if entry.file_type()?.is_dir() {
            copy_dir(&entry.path(), &target)?;
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
