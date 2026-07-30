use std::{
    env, fs, io,
    path::{Path, PathBuf},
    process::Command,
};

const THEME_FILE: &str = "theme.toml";

fn main() {
    if let Err(err) = run(env::args().skip(1)) {
        eprintln!("error: {err}");
        std::process::exit(1);
    }
}

fn run(args: impl IntoIterator<Item = String>) -> Result<(), String> {
    let mut args = args.into_iter();
    match (args.next().as_deref(), args.next(), args.next()) {
        (Some("switch"), Some(name), None) => switch_theme(&root_dir()?, &name),
        (Some("list"), None, None) => list_themes(&root_dir()?),
        (Some("install"), Some(repo), None) => install_repo(&root_dir()?, &repo),
        _ => {
            return Err(
                "usage: retheme list | retheme switch <name> | retheme install <repository-url>"
                    .into(),
            )
        }
    }
    .map_err(|e| e.to_string())
}

fn root_dir() -> Result<PathBuf, String> {
    if let Ok(root) = env::var("RETHEME_ROOT") {
        return Ok(PathBuf::from(root));
    }
    if let Ok(config) = env::var("XDG_CONFIG_HOME") {
        return Ok(PathBuf::from(config).join("reEnvisioning"));
    }
    let home = env::var("HOME").map_err(|_| "set RETHEME_ROOT, XDG_CONFIG_HOME, or HOME")?;
    Ok(PathBuf::from(home).join(".config/reEnvisioning"))
}

fn list_themes(root: &Path) -> io::Result<()> {
    for name in discover_themes(root)? {
        println!("{name}");
    }
    Ok(())
}

fn discover_themes(root: &Path) -> io::Result<Vec<String>> {
    let themes = root.join("themes");
    let mut names = Vec::new();
    if themes.is_dir() {
        for entry in fs::read_dir(themes)? {
            let entry = entry?;
            let name = entry.file_name();
            let Some(name) = name.to_str() else { continue };
            if entry.file_type()?.is_dir()
                && validate_name(name).is_ok()
                && entry.path().join(THEME_FILE).is_file()
            {
                names.push(name.to_string());
            }
        }
    }
    names.sort();
    Ok(names)
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

fn install_from_dir(root: &Path, source: &Path, fallback_name: &str) -> io::Result<()> {
    let found = if source.join(THEME_FILE).is_file() {
        validate_name(fallback_name)?;
        vec![(source.to_path_buf(), fallback_name.to_string())]
    } else {
        let mut found = Vec::new();
        for entry in fs::read_dir(source)? {
            let entry = entry?;
            if entry.file_name() == ".git" {
                continue;
            }
            let name = entry.file_name();
            let Some(name) = name.to_str() else { continue };
            if entry.file_type()?.is_dir() && entry.path().join(THEME_FILE).is_file() {
                validate_name(name)?;
                found.push((entry.path(), name.to_string()));
            }
        }
        found
    };

    if found.is_empty() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            format!("repository contains no {THEME_FILE} files"),
        ));
    }
    for (source, _) in &found {
        preflight_dir(source)?;
    }

    let themes = root.join("themes");
    for (_, name) in &found {
        let dest = themes.join(name);
        if dest.exists() {
            return Err(io::Error::new(
                io::ErrorKind::AlreadyExists,
                format!("theme already exists: {}", dest.display()),
            ));
        }
    }
    fs::create_dir_all(&themes)?;
    let mut installed = Vec::new();
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
                format!("unsupported repository entry: {}", entry.path().display()),
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
        let ty = entry.file_type()?;
        if ty.is_dir() {
            copy_dir(&entry.path(), &target)?;
        } else if ty.is_file() {
            fs::copy(entry.path(), target)?;
        } else {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                format!("unsupported repository entry: {}", entry.path().display()),
            ));
        }
    }
    Ok(())
}

fn repo_name_from_url(repo: &str) -> io::Result<String> {
    let path = if let Some((scheme, rest)) = repo.split_once("://") {
        match scheme.to_ascii_lowercase().as_str() {
            "file" => rest.strip_prefix('/').filter(|path| !path.is_empty()),
            "https" | "ssh" | "git" => rest
                .split_once('/')
                .and_then(|(host, path)| (!host.is_empty() && !path.is_empty()).then_some(path)),
            _ => None,
        }
        .ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::InvalidInput,
                "expected complete repository URL",
            )
        })?
    } else if let Some((host, path)) = repo.split_once(':') {
        let host = host.rsplit_once('@').map_or(host, |(_, host)| host);
        if host.is_empty()
            || host.contains('/')
            || path.is_empty()
            || matches!(host, "http" | "https" | "ssh" | "git" | "file")
        {
            bad_url()?;
        }
        path
    } else {
        bad_url()?
    };

    let name = path
        .split(['?', '#'])
        .next()
        .unwrap_or("")
        .trim_end_matches('/')
        .rsplit('/')
        .next()
        .unwrap_or("");
    let name = name.strip_suffix(".git").unwrap_or(name);
    validate_name(name)?;
    Ok(name.to_string())
}

fn bad_url<T>() -> io::Result<T> {
    Err(io::Error::new(
        io::ErrorKind::InvalidInput,
        "expected complete repository URL",
    ))
}

fn switch_theme(root: &Path, name: &str) -> io::Result<()> {
    validate_name(name)?;

    let theme_dir = root.join("themes").join(name);
    let theme_file = theme_dir.join(THEME_FILE);
    if !theme_file.is_file() {
        return Err(io::Error::new(
            io::ErrorKind::NotFound,
            format!("missing {}", theme_file.display()),
        ));
    }

    let active = root.join("active");
    fs::create_dir_all(&active)?;
    replace_symlink(
        &active.join("theme"),
        Path::new("..").join("themes").join(name),
    )?;
    copy_atomic(&theme_file, &active.join(THEME_FILE))?;
    if theme_dir.join("colors.toml").is_file() {
        copy_atomic(&theme_dir.join("colors.toml"), &active.join("colors.toml"))?;
    } else if let Err(err) = fs::remove_file(active.join("colors.toml")) {
        if err.kind() != io::ErrorKind::NotFound {
            return Err(err);
        }
    }
    replace_apps(&theme_dir.join("apps"), &active.join("apps"))?;
    apply_file_handlers(root, &active.join("apps"))?;
    fs::write(active.join("current-theme"), name)?;

    println!("switched to {name}");
    Ok(())
}

fn validate_name(name: &str) -> io::Result<()> {
    if name.is_empty()
        || Path::new(name).components().count() != 1
        || name == "."
        || name == ".."
        || name.contains('/')
        || name.contains('\\')
    {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "theme name must be one path component",
        ));
    }
    Ok(())
}

#[cfg(unix)]
fn replace_symlink(link: &Path, target: PathBuf) -> io::Result<()> {
    use std::os::unix::fs::symlink;

    let tmp = link.with_extension("tmp");
    let _ = fs::remove_file(&tmp);
    symlink(target, &tmp)?;
    fs::rename(tmp, link)
}

fn copy_atomic(src: &Path, dest: &Path) -> io::Result<()> {
    let tmp = dest.with_extension("tmp");
    fs::copy(src, &tmp)?;
    fs::rename(tmp, dest)
}

fn write_atomic(dest: &Path, content: &str) -> io::Result<()> {
    if let Some(parent) = dest.parent() {
        fs::create_dir_all(parent)?;
    }
    let tmp = dest.with_extension("tmp");
    fs::write(&tmp, content)?;
    fs::rename(tmp, dest)
}

fn replace_apps(src: &Path, dest: &Path) -> io::Result<()> {
    let tmp = dest.with_extension("tmp");
    let old = dest.with_extension("old");
    let _ = fs::remove_dir_all(&tmp);
    let _ = fs::remove_dir_all(&old);
    fs::create_dir_all(&tmp)?;

    if src.is_dir() {
        for entry in fs::read_dir(src)? {
            let entry = entry?;
            if entry.file_type()?.is_file() {
                fs::copy(entry.path(), tmp.join(entry.file_name()))?;
            }
        }
    }

    if dest.exists() {
        fs::rename(dest, &old)?;
    }
    fs::rename(&tmp, dest)?;
    let _ = fs::remove_dir_all(old);
    Ok(())
}

fn apply_file_handlers(root: &Path, app_dir: &Path) -> io::Result<()> {
    if !app_dir.is_dir() {
        return Ok(());
    }
    for entry in fs::read_dir(app_dir)? {
        let entry = entry?;
        if entry.file_type()?.is_file() {
            let content = fs::read_to_string(entry.path())?;
            if toml_value(&content, "handler").as_deref() == Some("file") {
                let target = toml_value(&content, "target").ok_or_else(|| {
                    io::Error::new(io::ErrorKind::InvalidData, "file handler missing target")
                })?;
                let filename = toml_value(&content, "filename").ok_or_else(|| {
                    io::Error::new(io::ErrorKind::InvalidData, "file handler missing filename")
                })?;
                validate_name(&filename)?;
                let text = toml_multiline(&content, "text").ok_or_else(|| {
                    io::Error::new(
                        io::ErrorKind::InvalidData,
                        "file handler missing content.text",
                    )
                })?;
                write_atomic(&resolve_target(root, &target).join(filename), &text)?;
            }
        }
    }
    Ok(())
}

fn resolve_target(root: &Path, target: &str) -> PathBuf {
    if let Some(rest) = target.strip_prefix("~/") {
        if let Ok(home) = env::var("HOME") {
            return PathBuf::from(home).join(rest);
        }
    }
    let path = PathBuf::from(target);
    if path.is_absolute() {
        path
    } else {
        root.join(path)
    }
}

fn toml_value(text: &str, key: &str) -> Option<String> {
    let prefix = format!("{key} = ");
    text.lines().find_map(|line| {
        let value = line.trim().strip_prefix(&prefix)?.trim();
        Some(value.trim_matches('"').to_string())
    })
}

fn toml_multiline(text: &str, key: &str) -> Option<String> {
    let start = format!("{key} = \"\"\"");
    let mut out = String::new();
    let mut active = false;
    for line in text.lines() {
        if active {
            if line.trim() == "\"\"\"" {
                return Some(out);
            }
            out.push_str(line);
            out.push('\n');
        } else if line.trim() == start {
            active = true;
        }
    }
    None
}
