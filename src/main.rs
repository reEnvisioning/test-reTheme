use std::{
    env, fs, io,
    path::{Path, PathBuf},
    process::Command,
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
        (Some("switch"), Some(name), None) => switch_theme(&root_dir()?, &name),
        (Some("list"), None, None) => list_themes(&root_dir()?),
        (Some("install"), Some(repo), None) => install_repo(&root_dir()?, &repo),
        _ => Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "usage: retheme list | retheme switch <name> | retheme install <repository-url>",
        )),
    }
    .map_err(|e| e.to_string())
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

fn list_themes(root: &Path) -> io::Result<()> {
    for name in discover_themes(root)? {
        println!("{name}");
    }
    Ok(())
}
fn discover_themes(root: &Path) -> io::Result<Vec<String>> {
    let mut names = Vec::new();
    if root.join("themes").is_dir() {
        for entry in fs::read_dir(root.join("themes"))? {
            let entry = entry?;
            let name = entry.file_name();
            let Some(name) = name.to_str() else { continue };
            if entry.file_type()?.is_dir()
                && validate_name(name).is_ok()
                && entry.path().join(THEME_FILE).is_file()
            {
                names.push(name.into());
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
fn install_from_dir(root: &Path, source: &Path, fallback: &str) -> io::Result<()> {
    let found = if source.join(THEME_FILE).is_file() {
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
            format!("repository contains no {THEME_FILE} files"),
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

fn switch_theme(root: &Path, name: &str) -> io::Result<()> {
    validate_name(name)?;
    let theme_dir = root.join("themes").join(name);
    let theme_file = theme_dir.join(THEME_FILE);
    let base16_file = theme_dir.join(BASE16_FILE);
    if !theme_file.is_file() {
        return Err(io::Error::new(
            io::ErrorKind::NotFound,
            format!("missing {}", theme_file.display()),
        ));
    }
    let colors = parse_base16(&fs::read_to_string(&base16_file)?)?;
    let dark = theme_mode(&fs::read_to_string(&theme_file)?) == "dark";
    let active = root.join("active");
    fs::create_dir_all(&active)?;
    replace_symlink(
        &active.join("theme"),
        Path::new("..").join("themes").join(name),
    )?;
    copy_atomic(&theme_file, &active.join(THEME_FILE))?;
    copy_atomic(&base16_file, &active.join(BASE16_FILE))?;
    replace_apps(&theme_dir.join("apps"), &active.join("apps"))?;
    write_atomic(&active.join("current-theme"), &format!("{name}\n"))?;
    render_fixed(&colors, dark)?;
    println!("switched to {name}");
    Ok(())
}

fn validate_name(name: &str) -> io::Result<()> {
    if name.is_empty()
        || Path::new(name).components().count() != 1
        || matches!(name, "." | "..")
        || name.contains(['/', '\\'])
    {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "theme name must be one path component",
        ));
    }
    Ok(())
}

fn parse_base16(text: &str) -> io::Result<[String; 16]> {
    let mut values: [Option<String>; 16] = Default::default();
    for line in text.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let (key, value) = line
            .split_once(':')
            .ok_or_else(|| invalid_base16("expected key: value"))?;
        let key = key.trim();
        const KEYS: [&str; 16] = [
            "base00", "base01", "base02", "base03", "base04", "base05", "base06", "base07",
            "base08", "base09", "base0A", "base0B", "base0C", "base0D", "base0E", "base0F",
        ];
        let index = KEYS
            .iter()
            .position(|candidate| *candidate == key)
            .ok_or_else(|| invalid_base16("keys must be base00..base0F"))?;
        let value = value.trim();
        let value = value
            .strip_prefix('"')
            .and_then(|v| v.strip_suffix('"'))
            .or_else(|| value.strip_prefix('\'').and_then(|v| v.strip_suffix('\'')))
            .ok_or_else(|| invalid_base16("colors must be quoted"))?;
        if value.len() != 7
            || !value.starts_with('#')
            || !value[1..].chars().all(|c| c.is_ascii_hexdigit())
            || values[index].is_some()
        {
            return Err(invalid_base16("duplicate or invalid six-digit color"));
        }
        values[index] = Some(value.to_ascii_uppercase());
    }
    if values.iter().any(Option::is_none) {
        return Err(invalid_base16("all base00..base0F colors are required"));
    }
    Ok(values.map(Option::unwrap))
}
fn invalid_base16(message: &str) -> io::Error {
    io::Error::new(
        io::ErrorKind::InvalidData,
        format!("invalid base16.yaml: {message}"),
    )
}

fn home() -> Option<PathBuf> {
    env::var_os("HOME").map(PathBuf::from)
}
fn render_fixed(c: &[String; 16], dark: bool) -> io::Result<()> {
    let Some(home) = home() else { return Ok(()) };
    let config = env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| home.join(".config"));
    let gtk = format!("@define-color base00 {};\n@define-color base05 {};\n@define-color base0D {};\nwindow, dialog {{ background-color: @base00; color: @base05; }}\n", c[0], c[5], c[13]);
    write_atomic(&config.join("gtk-3.0/gtk.css"), &gtk)?;
    write_atomic(&config.join("gtk-4.0/gtk.css"), &gtk)?;
    let _ = Command::new("gsettings")
        .args([
            "set",
            "org.gnome.desktop.interface",
            "color-scheme",
            if dark { "prefer-dark" } else { "default" },
        ])
        .status();
    let _ = Command::new("gsettings")
        .args(["set", "org.gnome.desktop.interface", "gtk-theme", "Adwaita"])
        .status();
    let qt_colors = qt_colors(c);
    for qtver in ["qt5ct", "qt6ct"] {
        let colors_path = config.join(format!("{qtver}/colors/reTheme.colors"));
        write_atomic(&colors_path, &qt_colors)?;
        let qt = format!(
            "[Appearance]\ncolor_scheme_path={}\ncustom_palette=true\nicon_theme=Adwaita\nstyle=Adwaita\n",
            colors_path.display()
        );
        write_atomic(&config.join(format!("{qtver}/{qtver}.conf")), &qt)?;
    }
    let kitty = format!("# generated by retheme\nforeground {}\nbackground {}\nselection_foreground {}\nselection_background {}\n", c[5], c[0], c[0], c[2]);
    write_atomic(&config.join("kitty/retheme-base16.conf"), &kitty)?;
    let btop = format!("# generated by retheme\nmain_bg = \"{}\"\nmain_fg = \"{}\"\nhi_fg = \"{}\"\nselected_bg = \"{}\"\nselected_fg = \"{}\"\n", c[0], c[5], c[13], c[2], c[5]);
    write_atomic(&config.join("btop/themes/retheme-base16.theme"), &btop)?;
    write_atomic(
        &config.join("nvim/colors/retheme-base16.lua"),
        &nvim_colors(c, dark),
    )?;
    browser_prefs(&config.join("mozilla/firefox"), dark);
    browser_prefs(&config.join("librewolf"), dark);
    browser_prefs(&home.join(".librewolf"), dark);
    let _ = Command::new("pkill").args(["-SIGUSR1", "kitty"]).status();
    let _ = Command::new("pkill").args(["-SIGUSR2", "btop"]).status();
    Ok(())
}
fn theme_mode(text: &str) -> String {
    text.lines()
        .find_map(|line| {
            let (key, value) = line.split_once('=')?;
            (key.trim() == "mode").then(|| value.trim().trim_matches('"').to_string())
        })
        .unwrap_or_else(|| "dark".into())
}
fn nvim_colors(c: &[String; 16], dark: bool) -> String {
    format!("local c = {{ bg = '{}', fg = '{}', red = '{}', green = '{}', yellow = '{}', blue = '{}', magenta = '{}', cyan = '{}' }}\nvim.cmd('hi clear')\nvim.o.background = '{}'\nfor _, group in ipairs({{'Normal', 'NormalFloat'}}) do vim.api.nvim_set_hl(0, group, {{ fg = c.fg, bg = c.bg }}) end\nvim.api.nvim_set_hl(0, 'Comment', {{ fg = '{}' }})\nvim.api.nvim_set_hl(0, 'String', {{ fg = c.green }})\nvim.api.nvim_set_hl(0, 'Function', {{ fg = c.blue }})\nvim.api.nvim_set_hl(0, 'Keyword', {{ fg = c.magenta }})\n", c[0], c[5], c[8], c[11], c[10], c[13], c[14], c[12], if dark { "dark" } else { "light" }, c[3])
}

fn qt_colors(c: &[String; 16]) -> String {
    let indexes = [
        5, 0, 0, 3, 4, 5, 13, 7, 3, 8, 9, 10, 11, 12, 13, 14, 15, 4, 3, 13, 3,
    ];
    let active = indexes
        .iter()
        .map(|index| format!("#ff{}", &c[*index][1..]))
        .collect::<Vec<_>>()
        .join(", ");
    let disabled = active.clone();
    format!("[ColorScheme]\nactive_colors={active}\ndisabled_colors={disabled}\ninactive_colors={active}\n")
}

fn browser_prefs(root: &Path, dark: bool) {
    let ini = root.join("profiles.ini");
    let Ok(text) = fs::read_to_string(ini) else {
        return;
    };
    for line in text.lines().filter_map(|l| l.strip_prefix("Path=")) {
        let profile = root.join(line);
        if !profile.is_dir() || line.contains("..") {
            continue;
        }
        let prefs = [
            ("ui.systemUsesDarkTheme", if dark { "1" } else { "0" }),
            ("browser.theme.toolbar-theme", if dark { "0" } else { "2" }),
            ("browser.theme.content-theme", if dark { "0" } else { "2" }),
        ];
        let path = profile.join("user.js");
        let old = fs::read_to_string(&path).unwrap_or_default();
        let mut out: Vec<String> = old
            .lines()
            .filter(|line| {
                !prefs
                    .iter()
                    .any(|(key, _)| line.starts_with(&format!("user_pref(\"{key}\",")))
            })
            .map(str::to_string)
            .collect();
        for (key, value) in prefs {
            out.push(format!("user_pref(\"{key}\", {value});"));
        }
        let _ = write_atomic(&path, &(out.join("\n") + "\n"));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn base16_requires_canonical_keys() {
        let valid = (0..16)
            .map(|i| format!("base{:02X}: \"#000000\"", i))
            .collect::<Vec<_>>()
            .join("\n");
        assert!(parse_base16(&valid).is_ok());
        assert!(parse_base16(&valid.replace("base00", "base0")).is_err());
        assert!(parse_base16(&valid.replace("base0A", "base0a")).is_err());
    }
}

#[cfg(unix)]
fn replace_symlink(link: &Path, target: PathBuf) -> io::Result<()> {
    use std::os::unix::fs::symlink;
    let tmp = link.with_extension("tmp");
    let _ = fs::remove_file(&tmp);
    symlink(target, &tmp)?;
    if link.exists() || link.symlink_metadata().is_ok() {
        fs::remove_file(link)?;
    }
    fs::rename(tmp, link)
}
fn copy_atomic(src: &Path, dest: &Path) -> io::Result<()> {
    if let Some(parent) = dest.parent() {
        fs::create_dir_all(parent)?;
    }
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
