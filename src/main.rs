use std::{
    env, fs, io,
    path::{Path, PathBuf},
};

fn main() {
    if let Err(err) = run(env::args().skip(1)) {
        eprintln!("error: {err}");
        std::process::exit(1);
    }
}

fn run(args: impl IntoIterator<Item = String>) -> Result<(), String> {
    let mut args = args.into_iter();
    match (args.next().as_deref(), args.next(), args.next()) {
        (Some("switch"), Some(name), None) => {
            switch_theme(&root_dir()?, &name).map_err(|e| e.to_string())
        }
        (Some("list"), None, None) => list_themes(&root_dir()?).map_err(|e| e.to_string()),
        _ => Err("usage: retheme switch <name> | retheme list".into()),
    }
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
    if !themes.is_dir() {
        return Ok(names);
    }
    for entry in fs::read_dir(themes)? {
        let entry = entry?;
        if entry.file_type()?.is_dir() && entry.path().join("theme.json").is_file() {
            if let Some(name) = entry.file_name().to_str() {
                if validate_name(name).is_ok() {
                    names.push(name.to_string());
                }
            }
        }
    }
    names.sort();
    Ok(names)
}

fn switch_theme(root: &Path, name: &str) -> io::Result<()> {
    validate_name(name)?;

    let theme_dir = root.join("themes").join(name);
    let theme_json = theme_dir.join("theme.json");
    if !theme_json.is_file() {
        return Err(io::Error::new(
            io::ErrorKind::NotFound,
            format!("missing {}", theme_json.display()),
        ));
    }

    let active = root.join("active");
    fs::create_dir_all(&active)?;

    replace_symlink(
        &active.join("theme"),
        Path::new("..").join("themes").join(name),
    )?;
    copy_atomic(&theme_json, &active.join("theme.json"))?;
    replace_apps(&theme_dir.join("apps"), &active.join("apps"))?;
    apply_file_handlers(root, &active.join("apps"))?;
    fs::write(active.join("current-theme"), name)?;

    println!("switched to {name}");
    Ok(())
}

fn validate_name(name: &str) -> io::Result<()> {
    let path = Path::new(name);
    if name.is_empty()
        || path.components().count() != 1
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
    if fs::symlink_metadata(&tmp).is_ok() {
        fs::remove_file(&tmp)?;
    }
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
    let _ = fs::remove_dir_all(&tmp);
    fs::create_dir_all(&tmp)?;

    if src.is_dir() {
        for entry in fs::read_dir(src)? {
            let entry = entry?;
            if entry.file_type()?.is_file() {
                fs::copy(entry.path(), tmp.join(entry.file_name()))?;
            }
        }
    }

    let old = dest.with_extension("old");
    let _ = fs::remove_dir_all(&old);
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn temp_root() -> PathBuf {
        let id = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        env::temp_dir().join(format!("retheme-test-{}-{id}", std::process::id()))
    }

    fn add_theme(root: &Path, name: &str, theme: &[u8], app_name: &str, app: &[u8]) {
        let apps = root.join("themes").join(name).join("apps");
        fs::create_dir_all(&apps).unwrap();
        fs::write(root.join("themes").join(name).join("theme.json"), theme).unwrap();
        fs::write(apps.join(app_name), app).unwrap();
    }

    fn fixture(name: &str) -> PathBuf {
        let root = temp_root();
        add_theme(
            &root,
            name,
            b"theme",
            "app.toml",
            b"handler = \"settings\"\n",
        );
        root
    }

    #[test]
    fn switches_theme() {
        let root = fixture("sakura");
        switch_theme(&root, "sakura").unwrap();

        assert_eq!(fs::read(root.join("active/theme.json")).unwrap(), b"theme");
        assert_eq!(
            fs::read_to_string(root.join("active/apps/app.toml")).unwrap(),
            "handler = \"settings\"\n"
        );
        assert_eq!(
            fs::read(root.join("active/current-theme")).unwrap(),
            b"sakura"
        );

        #[cfg(unix)]
        assert_eq!(
            fs::read_link(root.join("active/theme")).unwrap(),
            Path::new("../themes/sakura")
        );

        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn discovers_installed_themes() {
        let root = fixture("sakura");
        add_theme(&root, "horror", b"theme", "app.toml", b"");
        assert_eq!(discover_themes(&root).unwrap(), ["horror", "sakura"]);
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn generic_file_handler_writes_declared_target() {
        let root = temp_root();
        add_theme(
            &root,
            "sakura",
            b"theme",
            "whatever.toml",
            br#"[meta]
handler = "file"
target = "out"
filename = "generated.conf"

[content]
text = """
hello
"""
"#,
        );

        switch_theme(&root, "sakura").unwrap();
        assert_eq!(
            fs::read_to_string(root.join("out/generated.conf")).unwrap(),
            "hello\n"
        );
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn replaces_existing_theme_and_stale_apps() {
        let root = temp_root();
        add_theme(&root, "sakura", b"sakura", "old.toml", b"old");
        add_theme(&root, "horror", b"horror", "new.toml", b"new");

        switch_theme(&root, "sakura").unwrap();
        switch_theme(&root, "horror").unwrap();

        assert_eq!(fs::read(root.join("active/theme.json")).unwrap(), b"horror");
        assert_eq!(fs::read(root.join("active/apps/new.toml")).unwrap(), b"new");
        assert!(!root.join("active/apps/old.toml").exists());

        #[cfg(unix)]
        assert_eq!(
            fs::read_link(root.join("active/theme")).unwrap(),
            Path::new("../themes/horror")
        );

        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn rejects_path_traversal() {
        let root = fixture("sakura");
        switch_theme(&root, "sakura").unwrap();
        assert!(switch_theme(&root, "../sakura").is_err());

        #[cfg(unix)]
        assert_eq!(
            fs::read_link(root.join("active/theme")).unwrap(),
            Path::new("../themes/sakura")
        );

        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn rejects_separators() {
        for name in ["", ".", "..", "a/b", "a\\b"] {
            assert!(validate_name(name).is_err(), "{name:?}");
        }
        assert!(validate_name("sakura").is_ok());
    }
}
