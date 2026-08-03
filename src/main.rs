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
        (Some("wallpaper"), Some(selection), None) => wallpaper_command(&root_dir()?, &selection),
        (Some("list"), None, None) => list_themes(&root_dir()?),
        (Some("install"), Some(repo), None) => install_repo(&root_dir()?, &repo),
        _ => Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "usage: retheme list | retheme switch <name> | retheme wallpaper <restore|next|prev|INDEX> | retheme install <repository-url>",
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
                && entry.path().join(BASE16_FILE).is_file()
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

fn load_theme(root: &Path, name: &str) -> io::Result<(PathBuf, ThemeMetadata, [String; 16])> {
    validate_name(name)?;
    let themes_root = fs::canonicalize(root.join("themes"))?;
    let theme_dir = fs::canonicalize(root.join("themes").join(name))?;
    if theme_dir.parent() != Some(themes_root.as_path())
        || theme_dir.file_name().and_then(|value| value.to_str()) != Some(name)
    {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "theme must be a direct child of themes directory",
        ));
    }
    let metadata = parse_theme_metadata(&fs::read_to_string(theme_dir.join(THEME_FILE))?, name)?;
    let colors = parse_base16(&fs::read_to_string(theme_dir.join(BASE16_FILE))?)?;
    Ok((theme_dir, metadata, colors))
}

fn switch_theme(root: &Path, name: &str) -> io::Result<()> {
    let (theme_dir, metadata, colors) = load_theme(root, name)?;

    let wallpapers = read_wallpapers(&theme_dir)?;
    // Renderer files are staged and all generated content is written before active state changes.
    render_fixed(root, &colors, metadata.dark)?;
    let wallpaper_state = prepare_wallpaper(
        &theme_dir,
        None,
        name,
        wallpapers.as_ref(),
        WallpaperSelection::Restore,
    )?;
    let active = root.join("active");
    fs::create_dir_all(&active)?;
    for old in [
        "theme.toml",
        "base16.yaml",
        "apps",
        "current-theme",
        "wallpaper.toml",
    ] {
        remove_path(&active.join(old))?;
    }
    if let Some(state) = &wallpaper_state {
        publish_wallpaper_state(&active, state)?;
    }
    replace_symlink(
        &active.join("theme"),
        Path::new("..").join("themes").join(name),
    )?;
    if let Some(state) = wallpaper_state {
        apply_wallpaper(&state.1);
    }
    apply_renderers(metadata.dark);
    println!("switched to {name}");
    Ok(())
}

fn validate_name(name: &str) -> io::Result<()> {
    if name.is_empty()
        || Path::new(name).components().count() != 1
        || matches!(name, "." | "..")
        || name.contains(['/', '\\', '"'])
        || name.chars().any(char::is_control)
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
fn render_fixed(root: &Path, c: &[String; 16], dark: bool) -> io::Result<()> {
    let Some(home) = home() else { return Ok(()) };
    let config = env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| home.join(".config"));
    let mut files = Vec::new();
    let gtk = format!("@define-color base00 {};\n@define-color base05 {};\n@define-color base0D {};\nwindow, dialog {{ background-color: @base00; color: @base05; }}\n", c[0], c[5], c[13]);
    files.push((config.join("gtk-3.0/gtk.css"), gtk.clone()));
    files.push((config.join("gtk-4.0/gtk.css"), gtk));
    let qt_colors = qt_colors(c);
    for qtver in ["qt5ct", "qt6ct"] {
        let colors_path = config.join(format!("{qtver}/colors/reTheme.colors"));
        files.push((colors_path.clone(), qt_colors.clone()));
        files.push((config.join(format!("{qtver}/{qtver}.conf")), format!("[Appearance]\ncolor_scheme_path={}\ncustom_palette=true\nicon_theme=Adwaita\nstyle=Adwaita\n", colors_path.display())));
    }
    files.push((config.join("kitty/retheme-base16.conf"), format!("# generated by retheme\nforeground {}\nbackground {}\nselection_foreground {}\nselection_background {}\n", c[5], c[0], c[0], c[2])));
    files.push((config.join("btop/themes/retheme-base16.theme"), format!("# generated by retheme\nmain_bg = \"{}\"\nmain_fg = \"{}\"\nhi_fg = \"{}\"\nselected_bg = \"{}\"\nselected_fg = \"{}\"\n", c[0], c[5], c[13], c[2], c[5])));
    files.push((
        config.join("nvim/colors/retheme-base16.lua"),
        nvim_colors(c, dark),
    ));
    browser_prefs(&config.join("mozilla/firefox"), dark, &mut files);
    browser_prefs(&config.join("librewolf"), dark, &mut files);
    browser_prefs(&home.join(".librewolf"), dark, &mut files);
    let stage = root
        .join("cache")
        .join(format!("retheme-render-{}", std::process::id()));
    let _ = fs::remove_dir_all(&stage);
    fs::create_dir_all(&stage)?;
    for (index, (_, content)) in files.iter().enumerate() {
        fs::write(stage.join(index.to_string()), content)?;
    }
    for (index, (destination, _)) in files.iter().enumerate() {
        write_atomic(
            destination,
            &fs::read_to_string(stage.join(index.to_string()))?,
        )?;
    }
    let _ = fs::remove_dir_all(stage);
    Ok(())
}
fn apply_renderers(dark: bool) {
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
    let _ = Command::new("pkill").args(["-SIGUSR1", "kitty"]).status();
    let _ = Command::new("pkill").args(["-SIGUSR2", "btop"]).status();
}
struct ThemeMetadata {
    dark: bool,
}

fn parse_theme_metadata(text: &str, expected_name: &str) -> io::Result<ThemeMetadata> {
    let mut schema = None;
    let mut name = None;
    let mut mode = None;
    for line in text.lines().map(str::trim) {
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let (key, value) = line
            .split_once('=')
            .ok_or_else(|| invalid_theme("expected key = value"))?;
        let key = key.trim();
        if !matches!(key, "schema" | "name" | "mode") {
            return Err(invalid_theme("unknown field"));
        }
        match key {
            "schema" if schema.is_none() => {
                if value.trim() != "1" {
                    return Err(invalid_theme("schema must be 1"));
                }
                schema = Some(1)
            }
            "name" if name.is_none() => name = Some(theme_string(value)?),
            "mode" if mode.is_none() => mode = Some(theme_string(value)?),
            _ => return Err(invalid_theme("duplicate field")),
        }
    }
    if schema != Some(1) || name.as_deref() != Some(expected_name) {
        return Err(invalid_theme(
            "schema must be 1 and name must match the selected directory",
        ));
    }
    match mode.as_deref() {
        Some("dark") => Ok(ThemeMetadata { dark: true }),
        Some("light") => Ok(ThemeMetadata { dark: false }),
        _ => Err(invalid_theme("mode must be exactly light or dark")),
    }
}
fn theme_string(value: &str) -> io::Result<String> {
    let value = value.trim();
    let value = value
        .strip_prefix('"')
        .and_then(|v| v.strip_suffix('"'))
        .ok_or_else(|| invalid_theme("strings must be double-quoted"))?;
    if value.contains(['"', '\\']) || value.is_empty() {
        return Err(invalid_theme("invalid string"));
    }
    Ok(value.into())
}
fn invalid_theme(message: &str) -> io::Error {
    io::Error::new(
        io::ErrorKind::InvalidData,
        format!("invalid theme.toml: {message}"),
    )
}
fn nvim_colors(c: &[String; 16], dark: bool) -> String {
    format!("local c = {{ bg = '{}', fg = '{}', red = '{}', green = '{}', yellow = '{}', blue = '{}', magenta = '{}', cyan = '{}' }}\nnvim.cmd('hi clear')\nvim.o.background = '{}'\nfor _, group in ipairs({{'Normal', 'NormalFloat'}}) do vim.api.nvim_set_hl(0, group, {{ fg = c.fg, bg = c.bg }}) end\nvim.api.nvim_set_hl(0, 'Comment', {{ fg = '{}' }})\nnvim.api.nvim_set_hl(0, 'String', {{ fg = c.green }})\nnvim.api.nvim_set_hl(0, 'Function', {{ fg = c.blue }})\nnvim.api.nvim_set_hl(0, 'Keyword', {{ fg = c.magenta }})\n", c[0], c[5], c[8], c[11], c[10], c[13], c[14], c[12], if dark { "dark" } else { "light" }, c[3])
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
    format!("[ColorScheme]\nactive_colors={active}\ndisabled_colors={active}\ninactive_colors={active}\n")
}
fn browser_prefs(root: &Path, dark: bool, files: &mut Vec<(PathBuf, String)>) {
    let Ok(text) = fs::read_to_string(root.join("profiles.ini")) else {
        return;
    };
    for line in text.lines().filter_map(|l| l.strip_prefix("Path=")) {
        let relative = Path::new(line);
        if relative.is_absolute()
            || relative
                .components()
                .any(|c| matches!(c, std::path::Component::ParentDir))
        {
            continue;
        }
        let profile = root.join(relative);
        if !profile.is_dir() {
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
        files.push((path, out.join("\n") + "\n"));
    }
}

#[derive(Debug, PartialEq, Eq)]
struct Wallpaper {
    index: usize,
    path: String,
}

#[derive(Debug, PartialEq, Eq)]
struct WallpaperPack {
    wallpapers: Vec<Wallpaper>,
    current: Option<usize>,
}

fn parse_wallpapers(text: &str) -> io::Result<WallpaperPack> {
    let mut wallpapers = Vec::new();
    let mut metadata_current: Option<usize> = None;
    let mut current = None;
    for line in text.lines().map(str::trim) {
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        if line == "[[wallpaper]]" {
            if let Some(w) = current.take() {
                wallpapers.push(w);
            }
            current = Some(Wallpaper {
                index: usize::MAX,
                path: String::new(),
            });
            continue;
        }
        let Some((key, value)) = line.split_once('=') else {
            return Err(invalid_wallpapers("expected [[wallpaper]] or key = value"));
        };
        if current.is_none() {
            if key.trim() == "current" && metadata_current.is_none() {
                metadata_current = Some(
                    parse_index(value)
                        .map_err(|_| invalid_wallpapers("current must be an integer"))?,
                );
                continue;
            }
            return Err(invalid_wallpapers(
                "keys must be inside [[wallpaper]] or be current",
            ));
        }
        let Some(w) = current.as_mut() else {
            unreachable!()
        };
        match key.trim() {
            "index" if w.index == usize::MAX => {
                w.index = parse_index(value)
                    .map_err(|_| invalid_wallpapers("index must be an integer"))?
            }
            "path" if w.path.is_empty() => {
                let value = value.trim();
                let path = value
                    .strip_prefix('"')
                    .and_then(|v| v.strip_suffix('"'))
                    .ok_or_else(|| invalid_wallpapers("path must be double-quoted"))?;
                if path.contains(['"', '\\']) || path.chars().any(char::is_control) {
                    return Err(invalid_wallpapers(
                        "path must not contain quotes, escapes, or control characters",
                    ));
                }
                w.path = path.into();
            }
            _ => return Err(invalid_wallpapers("duplicate or unknown wallpaper key")),
        }
    }
    if let Some(w) = current {
        wallpapers.push(w);
    }
    if wallpapers
        .iter()
        .any(|w| w.index == usize::MAX || w.path.is_empty())
    {
        return Err(invalid_wallpapers("each wallpaper needs index and path"));
    }
    Ok(WallpaperPack {
        wallpapers,
        current: metadata_current,
    })
}
fn parse_index(value: &str) -> io::Result<usize> {
    let value = value.trim();
    if (value.len() > 1 && value.starts_with('0')) || !value.chars().all(|c| c.is_ascii_digit()) {
        return Err(invalid_wallpapers(
            "index must be a non-negative integer without leading zeroes",
        ));
    }
    value
        .parse()
        .map_err(|_| invalid_wallpapers("index is out of range"))
}
fn invalid_wallpapers(message: &str) -> io::Error {
    io::Error::new(
        io::ErrorKind::InvalidData,
        format!("invalid wallpapers.toml: {message}"),
    )
}
fn valid_wallpaper_path(theme_dir: &Path, path: &str) -> io::Result<PathBuf> {
    let relative = Path::new(path);
    if relative.is_absolute()
        || relative
            .components()
            .any(|c| matches!(c, std::path::Component::ParentDir))
    {
        return Err(invalid_wallpapers(
            "wallpaper path must be relative and cannot traverse",
        ));
    }
    let theme = fs::canonicalize(theme_dir)?;
    let resolved = fs::canonicalize(theme_dir.join(relative))?;
    if !resolved.starts_with(theme) || !resolved.is_file() {
        return Err(invalid_wallpapers(
            "wallpaper must be a file inside the selected theme",
        ));
    }
    Ok(resolved)
}
fn remove_path(path: &Path) -> io::Result<()> {
    let Ok(metadata) = fs::symlink_metadata(path) else {
        return Ok(());
    };
    if metadata.is_dir() {
        fs::remove_dir_all(path)
    } else {
        fs::remove_file(path)
    }
}
fn active_wallpaper_index(active: &Path, theme: &str) -> Option<usize> {
    let text = fs::read_to_string(active.join("wallpaper.toml")).ok()?;
    let mut state_theme = None;
    let mut index = None;
    for line in text.lines().map(str::trim).filter(|line| !line.is_empty()) {
        let (key, value) = line.split_once('=')?;
        match key.trim() {
            "theme" => state_theme = Some(value.trim().trim_matches('"')),
            "index" => index = value.trim().parse().ok(),
            "path" => {}
            _ => return None,
        }
    }
    (state_theme == Some(theme)).then_some(index?)
}
fn read_wallpapers(theme_dir: &Path) -> io::Result<Option<WallpaperPack>> {
    let file = theme_dir.join("wallpapers.toml");
    if !file.is_file() {
        return Ok(None);
    }
    let mut pack = parse_wallpapers(&fs::read_to_string(file)?)?;
    for wallpaper in &pack.wallpapers {
        valid_wallpaper_path(theme_dir, &wallpaper.path)?;
    }
    pack.wallpapers.sort_by_key(|w| w.index);
    if pack
        .wallpapers
        .windows(2)
        .any(|pair| pair[0].index == pair[1].index)
    {
        return Err(invalid_wallpapers("wallpaper indices must be unique"));
    }
    Ok(Some(pack))
}
#[derive(Clone, Copy)]
enum WallpaperSelection {
    Restore,
    Next,
    Prev,
    Index(usize),
}

fn select_wallpaper<'a>(
    pack: &'a WallpaperPack,
    active_index: Option<usize>,
    selection: WallpaperSelection,
) -> io::Result<&'a Wallpaper> {
    let default = || pack.wallpapers.iter().find(|w| w.index == 0);
    let restored = pack
        .current
        .and_then(|index| pack.wallpapers.iter().position(|w| w.index == index));
    let navigated = active_index
        .and_then(|index| pack.wallpapers.iter().position(|w| w.index == index))
        .or_else(|| {
            pack.current
                .and_then(|index| pack.wallpapers.iter().position(|w| w.index == index))
        });
    let position = match selection {
        WallpaperSelection::Restore => restored.or_else(|| default().map(|_| 0)),
        WallpaperSelection::Index(index) => pack.wallpapers.iter().position(|w| w.index == index),
        WallpaperSelection::Next => {
            navigated.map(|position| (position + 1) % pack.wallpapers.len())
        }
        WallpaperSelection::Prev => {
            navigated.map(|position| position.checked_sub(1).unwrap_or(pack.wallpapers.len() - 1))
        }
    };
    match selection {
        WallpaperSelection::Index(index) => position
            .and_then(|position| pack.wallpapers.get(position))
            .ok_or_else(|| invalid_wallpapers(&format!("wallpaper index {index} is not declared"))),
        WallpaperSelection::Restore | WallpaperSelection::Next | WallpaperSelection::Prev => {
            position
                .and_then(|position| pack.wallpapers.get(position))
                .or_else(default)
                .ok_or_else(|| invalid_wallpapers("index 0 is required"))
        }
    }
}

fn prepare_wallpaper(
    theme_dir: &Path,
    active_index: Option<usize>,
    name: &str,
    pack: Option<&WallpaperPack>,
    selection: WallpaperSelection,
) -> io::Result<Option<(String, PathBuf)>> {
    let Some(pack) = pack else { return Ok(None) };
    let selected = select_wallpaper(pack, active_index, selection)?;
    let path = valid_wallpaper_path(theme_dir, &selected.path)?;
    let path_text = path
        .to_str()
        .ok_or_else(|| invalid_wallpapers("wallpaper path is not valid UTF-8"))?;
    Ok(Some((
        format!(
            "theme = \"{name}\"\nindex = {}\npath = \"{}\"\n",
            selected.index,
            toml_string(path_text)
        ),
        path,
    )))
}

fn publish_wallpaper_state(active: &Path, state: &(String, PathBuf)) -> io::Result<()> {
    write_atomic(&active.join("wallpaper.toml"), &state.0)
}

fn publish_wallpaper(active: &Path, state: &(String, PathBuf)) -> io::Result<()> {
    publish_wallpaper_state(active, state)?;
    apply_wallpaper(&state.1);
    Ok(())
}

fn wallpaper_command(root: &Path, argument: &str) -> io::Result<()> {
    let selection = match argument {
        "restore" => WallpaperSelection::Restore,
        "next" => WallpaperSelection::Next,
        "prev" => WallpaperSelection::Prev,
        value => WallpaperSelection::Index(parse_index(value)?),
    };
    let active = root.join("active");
    let active_theme = fs::canonicalize(active.join("theme"))?;
    let name = active_theme
        .file_name()
        .and_then(|name| name.to_str())
        .ok_or_else(|| invalid_wallpapers("active theme has no valid name"))?;
    let (validated_theme, _, _) = load_theme(root, name)?;
    if validated_theme != active_theme {
        return Err(invalid_wallpapers(
            "active theme is not the canonical selected pack",
        ));
    }
    let pack = read_wallpapers(&active_theme)?
        .ok_or_else(|| invalid_wallpapers("active theme has no wallpapers.toml"))?;
    let state = prepare_wallpaper(
        &active_theme,
        active_wallpaper_index(&active, name),
        name,
        Some(&pack),
        selection,
    )?
    .expect("validated wallpaper pack");
    publish_wallpaper(&active, &state)
}
fn toml_string(value: &str) -> String {
    let mut out = String::new();
    for ch in value.chars() {
        match ch {
            '\\' => out.push_str("\\\\"),
            '"' => out.push_str("\\\""),
            '\u{0008}' => out.push_str("\\b"),
            '\t' => out.push_str("\\t"),
            '\n' => out.push_str("\\n"),
            '\u{000C}' => out.push_str("\\f"),
            '\r' => out.push_str("\\r"),
            c if c.is_control() => out.push_str(&format!("\\u{:04X}", c as u32)),
            c => out.push(c),
        }
    }
    out
}
fn apply_wallpaper(path: &Path) {
    match Command::new("rewallpaper").arg("apply").arg(path).status() {
        Ok(status) if status.success() => {}
        Ok(status) => eprintln!("warning: rewallpaper apply failed ({status})"),
        Err(err) => eprintln!("warning: rewallpaper unavailable: {err}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn metadata_requires_exact_schema_name_and_mode() {
        let valid = "schema = 1\nname = \"sakura\"\nmode = \"light\"\n";
        assert!(!parse_theme_metadata(valid, "sakura").unwrap().dark);
        assert!(parse_theme_metadata("name = \"sakura\"\nmode = \"dark\"", "sakura").is_err());
        assert!(parse_theme_metadata(&valid.replace("sakura", "other"), "sakura").is_err());
        assert!(parse_theme_metadata(&valid.replace("light", "Dark"), "sakura").is_err());
        assert!(parse_theme_metadata(
            "schema = 1\nname = \"sakura\"\nmode = \"light\"\nextra = 1",
            "sakura"
        )
        .is_err());
    }

    #[test]
    fn valid_wallpaper_path_is_contained_file() {
        let root = env::temp_dir().join(format!("retheme-wallpaper-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(root.join("assets")).unwrap();
        fs::write(root.join("assets/wall.png"), b"image").unwrap();
        assert_eq!(
            valid_wallpaper_path(&root, "assets/wall.png").unwrap(),
            root.join("assets/wall.png").canonicalize().unwrap()
        );
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn wallpaper_parser_rejects_traversal() {
        assert!(parse_wallpapers("[[wallpaper]]\nindex = 0\npath = \"../outside.png\"\n").is_ok());
        assert!(parse_wallpapers("[[wallpaper]]\nindex = 0\npath = \"bad\"tail\"\n").is_err());
        assert!(valid_wallpaper_path(Path::new("/tmp/theme"), "../outside.png").is_err());
        assert!(valid_wallpaper_path(Path::new("/tmp/theme"), "/tmp/outside.png").is_err());
    }

    #[test]
    fn wallpaper_pack_selects_declared_current_then_default() {
        let pack = parse_wallpapers(
            "current = 1\n[[wallpaper]]\nindex = 0\npath = \"default.png\"\n[[wallpaper]]\nindex = 1\npath = \"current.png\"\n[[wallpaper]]\nindex = 2\npath = \"last.png\"\n",
        )
        .unwrap();
        assert_eq!(pack.current, Some(1));
        assert_eq!(pack.wallpapers[1].path, "current.png");
        assert_eq!(
            select_wallpaper(&pack, Some(1), WallpaperSelection::Restore)
                .unwrap()
                .index,
            1
        );
        assert_eq!(
            select_wallpaper(&pack, Some(1), WallpaperSelection::Next)
                .unwrap()
                .index,
            2
        );
        assert_eq!(
            select_wallpaper(&pack, Some(1), WallpaperSelection::Prev)
                .unwrap()
                .index,
            0
        );
        assert_eq!(
            select_wallpaper(&pack, Some(1), WallpaperSelection::Index(2))
                .unwrap()
                .index,
            2
        );
        assert!(select_wallpaper(&pack, Some(1), WallpaperSelection::Index(9)).is_err());
    }

    #[test]
    fn wallpaper_restore_falls_back_to_default() {
        let pack = parse_wallpapers(
            "[[wallpaper]]\nindex = 0\npath = \"default.png\"\n[[wallpaper]]\nindex = 2\npath = \"other.png\"\n",
        )
        .unwrap();
        assert_eq!(
            select_wallpaper(&pack, Some(2), WallpaperSelection::Restore)
                .unwrap()
                .index,
            0
        );
    }

    #[test]
    fn list_requires_minimal_pack_files() {
        let root = env::temp_dir().join(format!("retheme-test-{}", std::process::id()));
        let themes = root.join("themes");
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(themes.join("minimal")).unwrap();
        fs::write(themes.join("minimal/theme.toml"), "").unwrap();
        fs::create_dir_all(themes.join("incomplete")).unwrap();
        fs::write(themes.join("incomplete/theme.toml"), "").unwrap();
        fs::write(themes.join("incomplete/base16.yaml"), "").unwrap();
        assert_eq!(discover_themes(&root).unwrap(), vec!["incomplete"]);
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn switching_minimal_and_detailed_packs_publishes_selected_directory() {
        let root = env::temp_dir().join(format!("retheme-switch-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        for name in ["minimal", "detailed"] {
            let dir = root.join("themes").join(name);
            fs::create_dir_all(&dir).unwrap();
            fs::write(
                dir.join(THEME_FILE),
                format!("schema = 1\nname = \"{name}\"\nmode = \"dark\"\n"),
            )
            .unwrap();
            fs::write(
                dir.join(BASE16_FILE),
                (0..16)
                    .map(|i| format!("base{:02X}: \"#000000\"\n", i))
                    .collect::<String>(),
            )
            .unwrap();
            if name == "detailed" {
                fs::create_dir_all(dir.join("apps")).unwrap();
                fs::write(dir.join("apps/example.toml"), "enabled = true\n").unwrap();
            }
        }
        let config = root.join("config");
        env::set_var("RETHEME_ROOT", &root);
        env::set_var("HOME", &root.join("home"));
        env::set_var("XDG_CONFIG_HOME", &config);
        env::set_var("PATH", root.join("empty-bin"));
        switch_theme(&root, "minimal").unwrap();
        assert_eq!(
            fs::read_link(root.join("active/theme")).unwrap(),
            PathBuf::from("../themes/minimal")
        );
        switch_theme(&root, "detailed").unwrap();
        assert_eq!(
            fs::read_link(root.join("active/theme")).unwrap(),
            PathBuf::from("../themes/detailed")
        );
        assert!(root.join("active/theme/apps/example.toml").is_file());
        let _ = fs::remove_dir_all(root);
    }

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
    fs::rename(tmp, link)
}
fn write_atomic(dest: &Path, content: &str) -> io::Result<()> {
    if let Some(parent) = dest.parent() {
        fs::create_dir_all(parent)?;
    }
    let tmp = dest.with_extension("tmp");
    fs::write(&tmp, content)?;
    fs::rename(tmp, dest)
}
