use std::{
    fs, io,
    path::{Path, PathBuf},
};

use crate::{BASE16_FILE, THEME_FILE};

pub(crate) struct ThemeMetadata {
    pub(crate) dark: bool,
}

const ROOT_FILES: [&str; 8] = [
    THEME_FILE,
    BASE16_FILE,
    "wallpapers.toml",
    "typography.toml",
    "spacing.toml",
    "animation.toml",
    "icons.toml",
    "fonts.toml",
];

pub(crate) fn validate_pack_dir(dir: &Path) -> io::Result<()> {
    validate_pack_dir_inner(dir, false)
}

pub(crate) fn validate_pack_dir_for_install(dir: &Path) -> io::Result<()> {
    validate_pack_dir_inner(dir, true)
}

pub(crate) fn validate_staged_pack(dir: &Path, name: &str) -> io::Result<()> {
    validate_pack_dir(dir)?;
    parse_theme_metadata(&fs::read_to_string(dir.join(THEME_FILE))?, name)?;
    parse_base16(&fs::read_to_string(dir.join(BASE16_FILE))?)?;
    read_wallpapers(dir)?;
    Ok(())
}

fn validate_pack_dir_inner(dir: &Path, allow_git: bool) -> io::Result<()> {
    let mut required = [false; 2];
    for entry in fs::read_dir(dir)? {
        let entry = entry?;
        let name = entry.file_name();
        let name = name
            .to_str()
            .ok_or_else(|| invalid_pack("non-UTF-8 root entry"))?;
        let metadata = fs::symlink_metadata(entry.path())?;
        if metadata.file_type().is_symlink() || (!metadata.is_file() && !metadata.is_dir()) {
            return Err(invalid_pack("symlinks and special files are not allowed"));
        }
        if allow_git && name == ".git" && metadata.is_dir() {
            continue;
        }
        if let Some(index) = ROOT_FILES.iter().position(|file| *file == name) {
            if !metadata.is_file() {
                return Err(invalid_pack("root schema files must be regular files"));
            }
            if index < 2 {
                required[index] = true;
            }
        } else if matches!(name, "apps" | "assets") && metadata.is_dir() {
            validate_tree(&entry.path())?;
        } else {
            return Err(invalid_pack("unknown root entry"));
        }
    }
    if required.iter().any(|present| !present) {
        return Err(invalid_pack("theme.toml and base16.yaml are required"));
    }
    Ok(())
}

fn validate_tree(dir: &Path) -> io::Result<()> {
    for entry in fs::read_dir(dir)? {
        let entry = entry?;
        let metadata = fs::symlink_metadata(entry.path())?;
        if metadata.file_type().is_symlink() || (!metadata.is_file() && !metadata.is_dir()) {
            return Err(invalid_pack("symlinks and special files are not allowed"));
        }
        if metadata.is_dir() {
            validate_tree(&entry.path())?;
        }
    }
    Ok(())
}

fn invalid_pack(message: &str) -> io::Error {
    io::Error::new(
        io::ErrorKind::InvalidData,
        format!("invalid theme pack: {message}"),
    )
}

pub(crate) fn available_themes(root: &Path) -> io::Result<usize> {
    let themes = root.join("themes");
    if !themes.is_dir() {
        return Err(io::Error::new(
            io::ErrorKind::NotFound,
            "configured root has no themes directory",
        ));
    }
    let mut count = 0;
    for entry in fs::read_dir(themes)? {
        let entry = entry?;
        let name_os = entry.file_name();
        let name = name_os
            .to_str()
            .ok_or_else(|| invalid_pack("theme name is not UTF-8"))?;
        if !entry.file_type()?.is_dir() {
            return Err(invalid_pack("themes contains a non-directory"));
        }
        validate_name(name)?;
        load_theme(root, name)?;
        count += 1;
    }
    if count == 0 {
        return Err(io::Error::new(
            io::ErrorKind::NotFound,
            "configured root has no valid theme packs",
        ));
    }
    Ok(count)
}

pub(crate) fn discover_themes(root: &Path) -> io::Result<Vec<String>> {
    let mut names = Vec::new();
    if root.join("themes").is_dir() {
        for entry in fs::read_dir(root.join("themes"))? {
            let entry = entry?;
            let name = entry.file_name();
            let name = name
                .to_str()
                .ok_or_else(|| invalid_pack("theme name is not UTF-8"))?;
            if !entry.file_type()?.is_dir() {
                return Err(invalid_pack("themes contains a non-directory"));
            }
            validate_name(name)?;
            validate_pack_dir(&entry.path())?;
            load_theme(root, name)?;
            names.push(name.into());
        }
    }
    names.sort();
    Ok(names)
}

pub(crate) fn load_theme(
    root: &Path,
    name: &str,
) -> io::Result<(PathBuf, ThemeMetadata, [String; 16])> {
    validate_name(name)?;
    validate_pack_dir(&root.join("themes").join(name))?;
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
    read_wallpapers(&theme_dir)?;
    Ok((theme_dir, metadata, colors))
}

pub(crate) fn validate_name(name: &str) -> io::Result<()> {
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

pub(crate) fn parse_base16(text: &str) -> io::Result<[String; 16]> {
    let mut values: [Option<String>; 16] = Default::default();
    const KEYS: [&str; 16] = [
        "base00", "base01", "base02", "base03", "base04", "base05", "base06", "base07", "base08",
        "base09", "base0A", "base0B", "base0C", "base0D", "base0E", "base0F",
    ];
    for line in text.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let (key, value) = line
            .split_once(':')
            .ok_or_else(|| invalid_base16("expected key: value"))?;
        let index = KEYS
            .iter()
            .position(|candidate| *candidate == key.trim())
            .ok_or_else(|| invalid_base16("keys must be base00..base0F"))?;
        let value = value
            .trim()
            .strip_prefix('"')
            .and_then(|v| v.strip_suffix('"'))
            .or_else(|| {
                value
                    .trim()
                    .strip_prefix('\'')
                    .and_then(|v| v.strip_suffix('\''))
            })
            .ok_or_else(|| invalid_base16("colors must be quoted"))?;
        if value.len() != 7
            || !value.starts_with('#')
            || !value[1..].chars().all(|c| c.is_ascii_hexdigit())
            || value != value.to_ascii_uppercase()
            || values[index].is_some()
        {
            return Err(invalid_base16("duplicate or non-canonical six-digit color"));
        }
        values[index] = Some(value.into());
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

pub(crate) fn parse_theme_metadata(text: &str, expected_name: &str) -> io::Result<ThemeMetadata> {
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
    let value = value
        .trim()
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

#[derive(Debug, PartialEq, Eq)]
pub(crate) struct Wallpaper {
    pub(crate) index: usize,
    pub(crate) path: String,
}
#[derive(Debug, PartialEq, Eq)]
pub(crate) struct WallpaperPack {
    pub(crate) wallpapers: Vec<Wallpaper>,
    pub(crate) current: Option<usize>,
}

pub(crate) fn parse_wallpapers(text: &str) -> io::Result<WallpaperPack> {
    let mut wallpapers = Vec::new();
    let mut metadata_current = None;
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
        let w = current.as_mut().unwrap();
        match key.trim() {
            "index" if w.index == usize::MAX => {
                w.index = parse_index(value)
                    .map_err(|_| invalid_wallpapers("index must be an integer"))?
            }
            "path" if w.path.is_empty() => {
                let path = value
                    .trim()
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
    if !wallpapers.iter().any(|wallpaper| wallpaper.index == 0) {
        return Err(invalid_wallpapers("index 0 is required"));
    }
    if let Some(current) = metadata_current {
        if !wallpapers
            .iter()
            .any(|wallpaper| wallpaper.index == current)
        {
            return Err(invalid_wallpapers("current index is not declared"));
        }
    }
    Ok(WallpaperPack {
        wallpapers,
        current: metadata_current,
    })
}
pub(crate) fn parse_index(value: &str) -> io::Result<usize> {
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
pub(crate) fn invalid_wallpapers(message: &str) -> io::Error {
    io::Error::new(
        io::ErrorKind::InvalidData,
        format!("invalid wallpapers.toml: {message}"),
    )
}

pub(crate) fn valid_wallpaper_path(theme_dir: &Path, path: &str) -> io::Result<PathBuf> {
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

pub(crate) fn read_wallpapers(theme_dir: &Path) -> io::Result<Option<WallpaperPack>> {
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
pub(crate) enum WallpaperSelection {
    Restore,
    Next,
    Prev,
    Index(usize),
}
pub(crate) fn select_wallpaper<'a>(
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
        })
        .or_else(|| default().map(|_| 0));
    let position = match selection {
        WallpaperSelection::Restore => active_index
            .and_then(|index| pack.wallpapers.iter().position(|w| w.index == index))
            .or(restored)
            .or_else(|| default().map(|_| 0)),
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
        _ => position
            .and_then(|position| pack.wallpapers.get(position))
            .or_else(default)
            .ok_or_else(|| invalid_wallpapers("index 0 is required")),
    }
}
pub(crate) fn prepare_wallpaper(
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
pub(crate) fn publish_wallpaper_state(active: &Path, state: &(String, PathBuf)) -> io::Result<()> {
    write_atomic(&active.join("wallpaper.toml"), &state.0)
}
pub(crate) fn active_wallpaper_index(
    active: &Path,
    theme_dir: &Path,
    theme: &str,
    pack: &WallpaperPack,
) -> io::Result<Option<usize>> {
    let path = active.join("wallpaper.toml");
    let metadata = match fs::symlink_metadata(&path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(error),
    };
    if metadata.file_type().is_symlink() || !metadata.is_file() {
        return Err(invalid_wallpapers(
            "active wallpaper state is not a regular file",
        ));
    }
    let text = fs::read_to_string(path)?;
    let mut state_theme = None;
    let mut index = None;
    let mut state_path = None;
    for line in text.lines().map(str::trim).filter(|line| !line.is_empty()) {
        let (key, value) = line
            .split_once('=')
            .ok_or_else(|| invalid_wallpapers("malformed active wallpaper state"))?;
        match key.trim() {
            "theme" if state_theme.is_none() => state_theme = Some(parse_state_string(value)?),
            "index" if index.is_none() => index = Some(parse_index(value)?),
            "path" if state_path.is_none() => state_path = Some(parse_state_string(value)?),
            _ => return Err(invalid_wallpapers("malformed active wallpaper state")),
        }
    }
    let index = index.ok_or_else(|| invalid_wallpapers("active wallpaper index is missing"))?;
    let state_path =
        state_path.ok_or_else(|| invalid_wallpapers("active wallpaper path is missing"))?;
    if state_theme.as_deref() != Some(theme) {
        return Err(invalid_wallpapers(
            "active wallpaper belongs to another theme",
        ));
    }
    let declared = pack
        .wallpapers
        .iter()
        .find(|wallpaper| wallpaper.index == index)
        .ok_or_else(|| invalid_wallpapers("active wallpaper index is not declared"))?;
    let state_path = fs::canonicalize(&state_path)
        .map_err(|_| invalid_wallpapers("active wallpaper path is missing"))?;
    if state_path != valid_wallpaper_path(theme_dir, &declared.path)? {
        return Err(invalid_wallpapers(
            "active wallpaper path does not match index",
        ));
    }
    Ok(Some(index))
}

fn parse_state_string(value: &str) -> io::Result<String> {
    let value = value.trim();
    let Some(value) = value.strip_prefix('"').and_then(|v| v.strip_suffix('"')) else {
        return Err(invalid_wallpapers(
            "active wallpaper strings must be quoted",
        ));
    };
    let mut out = String::new();
    let mut chars = value.chars();
    while let Some(ch) = chars.next() {
        if ch != '\\' {
            if ch == '"' || ch.is_control() {
                return Err(invalid_wallpapers(
                    "active wallpaper string has control characters",
                ));
            }
            out.push(ch);
            continue;
        }
        match chars.next() {
            Some('\\') => out.push('\\'),
            Some('"') => out.push('"'),
            Some('b') => out.push('\u{0008}'),
            Some('f') => out.push('\u{000C}'),
            Some('n') => out.push('\n'),
            Some('r') => out.push('\r'),
            Some('t') => out.push('\t'),
            Some('u') => {
                let hex: String = chars.by_ref().take(4).collect();
                if hex.len() != 4 {
                    return Err(invalid_wallpapers("invalid active wallpaper escape"));
                }
                let code = u32::from_str_radix(&hex, 16)
                    .map_err(|_| invalid_wallpapers("invalid active wallpaper escape"))?;
                let Some(ch) = char::from_u32(code) else {
                    return Err(invalid_wallpapers("invalid active wallpaper escape"));
                };
                out.push(ch);
            }
            _ => return Err(invalid_wallpapers("invalid active wallpaper escape")),
        }
    }
    if out.is_empty() {
        return Err(invalid_wallpapers(
            "active wallpaper strings must not be empty",
        ));
    }
    Ok(out)
}
pub(crate) fn remove_path(path: &Path) -> io::Result<()> {
    match fs::symlink_metadata(path) {
        Ok(metadata) if metadata.is_dir() => fs::remove_dir_all(path),
        Ok(_) => fs::remove_file(path),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(error),
    }
}
pub(crate) fn toml_string(value: &str) -> String {
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
pub(crate) fn write_atomic(dest: &Path, content: &str) -> io::Result<()> {
    use std::io::Write;

    if let Some(parent) = dest.parent() {
        fs::create_dir_all(parent)?;
    }
    let mut tmp = dest.with_extension(format!("tmp-{}", std::process::id()));
    for suffix in 0..100 {
        if suffix > 0 {
            tmp = dest.with_extension(format!("tmp-{}-{suffix}", std::process::id()));
        }
        match fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&tmp)
        {
            Ok(mut file) => {
                if let Err(err) = file.write_all(content.as_bytes()) {
                    let _ = fs::remove_file(&tmp);
                    return Err(err);
                }
                drop(file);
                let result = fs::rename(&tmp, dest);
                if result.is_err() {
                    let _ = fs::remove_file(&tmp);
                }
                return result;
            }
            Err(err) if err.kind() == io::ErrorKind::AlreadyExists => continue,
            Err(err) => return Err(err),
        }
    }
    Err(io::Error::new(
        io::ErrorKind::AlreadyExists,
        "too many temporary files",
    ))
}

#[cfg(unix)]
pub(crate) fn replace_symlink(link: &Path, target: PathBuf) -> io::Result<()> {
    use std::os::unix::fs::symlink;
    for n in 0..100 {
        let tmp = link.with_file_name(format!(".retheme-link-{}-{n}", std::process::id()));
        match symlink(&target, &tmp) {
            Ok(()) => {
                let result = fs::rename(&tmp, link);
                if result.is_err() {
                    let _ = fs::remove_file(&tmp);
                }
                return result;
            }
            Err(error) if error.kind() == io::ErrorKind::AlreadyExists => continue,
            Err(error) => return Err(error),
        }
    }
    Err(io::Error::new(
        io::ErrorKind::AlreadyExists,
        "too many temporary links",
    ))
}

pub(crate) struct PreparedTheme {
    pub(crate) name: String,
    pub(crate) metadata: ThemeMetadata,
    pub(crate) colors: [String; 16],
    pub(crate) wallpaper_state: Option<(String, PathBuf)>,
}

pub(crate) fn prepare_switch(root: &Path, name: &str) -> io::Result<PreparedTheme> {
    let (theme_dir, metadata, colors) = load_theme(root, name)?;
    let wallpapers = read_wallpapers(&theme_dir)?;
    let wallpaper_state = prepare_wallpaper(
        &theme_dir,
        None,
        name,
        wallpapers.as_ref(),
        WallpaperSelection::Restore,
    )?;
    Ok(PreparedTheme {
        name: name.into(),
        metadata,
        colors,
        wallpaper_state,
    })
}

pub(crate) fn validate_active(root: &Path) -> io::Result<()> {
    validate_active_tree(root, &root.join("active"))
}

pub(crate) fn publish_switch(root: &Path, prepared: PreparedTheme) -> io::Result<()> {
    let active = root.join("active");
    validate_active_tree(root, &active)?;
    let stage = unique_sibling(&active, "stage")?;
    fs::create_dir(&stage)?;
    let mut exchanged = false;
    let result = (|| {
        if let Some(state) = &prepared.wallpaper_state {
            publish_wallpaper_state(&stage, state)?;
        }
        replace_symlink(
            &stage.join("theme"),
            Path::new("..").join("themes").join(&prepared.name),
        )?;
        let had_active = match fs::symlink_metadata(&active) {
            Ok(_) => true,
            Err(error) if error.kind() == io::ErrorKind::NotFound => false,
            Err(error) => return Err(error),
        };
        if had_active {
            atomic_exchange(&stage, &active)?;
            exchanged = true;
            if let Err(cleanup) = remove_path(&stage) {
                return match atomic_exchange(&stage, &active) {
                    Ok(()) => {
                        let cleanup_new = remove_path(&stage);
                        Err(io::Error::other(format!(
                            "active cleanup failed: {cleanup}; rollback cleanup: {cleanup_new:?}"
                        )))
                    }
                    Err(rollback) => Err(io::Error::other(format!(
                        "active cleanup failed: {cleanup}; rollback failed: {rollback}"
                    ))),
                };
            }
        } else {
            fs::rename(&stage, &active)?;
        }
        Ok(())
    })();
    if result.is_err() && !exchanged {
        let _ = remove_path(&stage);
    }
    result
}

#[cfg(target_os = "linux")]
fn atomic_exchange(first: &Path, second: &Path) -> io::Result<()> {
    use std::{ffi::CString, os::unix::ffi::OsStrExt};
    unsafe extern "C" {
        fn renameat2(
            old_dirfd: i32,
            old_path: *const i8,
            new_dirfd: i32,
            new_path: *const i8,
            flags: u32,
        ) -> i32;
    }
    let first = CString::new(first.as_os_str().as_bytes())
        .map_err(|_| io::Error::new(io::ErrorKind::InvalidInput, "temporary path contains NUL"))?;
    let second = CString::new(second.as_os_str().as_bytes())
        .map_err(|_| io::Error::new(io::ErrorKind::InvalidInput, "active path contains NUL"))?;
    let result = unsafe { renameat2(-100, first.as_ptr(), -100, second.as_ptr(), 2) };
    if result == 0 {
        Ok(())
    } else {
        Err(io::Error::last_os_error())
    }
}

#[cfg(not(target_os = "linux"))]
fn atomic_exchange(first: &Path, second: &Path) -> io::Result<()> {
    let backup = second.with_file_name(".retheme-active-backup");
    fs::rename(second, &backup)?;
    if let Err(error) = fs::rename(first, second) {
        let _ = fs::rename(&backup, second);
        return Err(error);
    }
    remove_path(&backup)
}

fn validate_active_tree(root: &Path, active: &Path) -> io::Result<()> {
    let metadata = match fs::symlink_metadata(active) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(()),
        Err(error) => return Err(error),
    };
    if metadata.file_type().is_symlink() || !metadata.is_dir() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "active state must be a regular directory",
        ));
    }
    let theme = active.join("theme");
    let theme_metadata = fs::symlink_metadata(&theme)?;
    if !theme_metadata.file_type().is_symlink() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "active theme must be a symlink",
        ));
    }
    let theme_dir = fs::canonicalize(&theme)?;
    let name = theme_dir
        .file_name()
        .and_then(|name| name.to_str())
        .ok_or_else(|| invalid_pack("active theme has no valid name"))?;
    let (selected, _, _) = load_theme(root, name)?;
    if selected != theme_dir {
        return Err(invalid_pack("active theme is not a selected pack"));
    }
    let wallpaper = match fs::symlink_metadata(active.join("wallpaper.toml")) {
        Ok(metadata) => Some(metadata),
        Err(error) if error.kind() == io::ErrorKind::NotFound => None,
        Err(error) => return Err(error),
    };
    if let Some(metadata) = wallpaper {
        if metadata.file_type().is_symlink() || !metadata.is_file() {
            return Err(invalid_wallpapers(
                "active wallpaper state is not a regular file",
            ));
        }
        let pack = read_wallpapers(&theme_dir)?.ok_or_else(|| {
            invalid_wallpapers("active wallpaper state exists without a wallpaper pack")
        })?;
        active_wallpaper_index(active, &theme_dir, name, &pack)?;
    }
    for entry in fs::read_dir(active)? {
        let name = entry?.file_name();
        if name != "theme" && name != "wallpaper.toml" {
            return Err(invalid_pack("active state contains an unknown entry"));
        }
    }
    Ok(())
}

fn publish_wallpaper_transaction(active: &Path, state: &(String, PathBuf)) -> io::Result<()> {
    let stage = unique_sibling(active, "wallpaper")?;
    fs::create_dir(&stage)?;
    let mut exchanged = false;
    let result = (|| {
        publish_wallpaper_state(&stage, state)?;
        let theme = fs::read_link(active.join("theme"))?;
        replace_symlink(&stage.join("theme"), theme)?;
        atomic_exchange(&stage, active)?;
        exchanged = true;
        if let Err(cleanup) = remove_path(&stage) {
            return match atomic_exchange(&stage, active) {
                Ok(()) => {
                    let cleanup_new = remove_path(&stage);
                    Err(io::Error::other(format!(
                        "wallpaper cleanup failed: {cleanup}; rollback cleanup: {cleanup_new:?}"
                    )))
                }
                Err(rollback) => Err(io::Error::other(format!(
                    "wallpaper cleanup failed: {cleanup}; rollback failed: {rollback}"
                ))),
            };
        }
        Ok(())
    })();
    if result.is_err() && !exchanged {
        let _ = remove_path(&stage);
    }
    result
}

fn unique_sibling(path: &Path, kind: &str) -> io::Result<PathBuf> {
    for n in 0..100 {
        let base = path
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or("active");
        let candidate = path.with_file_name(format!(".{base}-{kind}-{}-{n}", std::process::id()));
        if !candidate.exists() {
            return Ok(candidate);
        }
    }
    Err(io::Error::new(
        io::ErrorKind::AlreadyExists,
        "temporary path unavailable",
    ))
}

pub(crate) fn prepare_wallpaper_command(root: &Path, argument: &str) -> io::Result<PathBuf> {
    let selection = match argument {
        "restore" => WallpaperSelection::Restore,
        "next" => WallpaperSelection::Next,
        "prev" => WallpaperSelection::Prev,
        value => WallpaperSelection::Index(parse_index(value)?),
    };
    let active = root.join("active");
    validate_active(root)?;
    let active_metadata = fs::symlink_metadata(&active)?;
    if active_metadata.file_type().is_symlink() || !active_metadata.is_dir() {
        return Err(invalid_wallpapers(
            "active state directory is not a regular directory",
        ));
    }
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
        active_wallpaper_index(&active, &active_theme, name, &pack)?,
        name,
        Some(&pack),
        selection,
    )?
    .expect("validated wallpaper pack");
    publish_wallpaper_transaction(&active, &state)?;
    Ok(state.1)
}

#[cfg(test)]
mod tests {
    use std::{
        env,
        ffi::OsString,
        sync::{Mutex, MutexGuard, OnceLock},
    };

    use super::*;

    static ENV_LOCK: OnceLock<Mutex<()>> = OnceLock::new();

    struct EnvGuard {
        _lock: MutexGuard<'static, ()>,
        values: [(&'static str, Option<OsString>); 4],
    }

    impl EnvGuard {
        fn lock() -> Self {
            let lock = ENV_LOCK
                .get_or_init(|| Mutex::new(()))
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            let names = ["RETHEME_ROOT", "HOME", "XDG_CONFIG_HOME", "PATH"];
            Self {
                _lock: lock,
                values: names.map(|name| (name, env::var_os(name))),
            }
        }
    }

    impl Drop for EnvGuard {
        fn drop(&mut self) {
            for (name, value) in &self.values {
                match value {
                    Some(value) => env::set_var(name, value),
                    None => env::remove_var(name),
                }
            }
        }
    }

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
        ).unwrap();
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
    fn wallpaper_restore_uses_active_or_default() {
        let pack = parse_wallpapers(
            "[[wallpaper]]\nindex = 0\npath = \"default.png\"\n[[wallpaper]]\nindex = 2\npath = \"other.png\"\n",
        ).unwrap();
        assert_eq!(
            select_wallpaper(&pack, Some(2), WallpaperSelection::Restore)
                .unwrap()
                .index,
            2
        );
        assert_eq!(
            select_wallpaper(&pack, None, WallpaperSelection::Restore)
                .unwrap()
                .index,
            0
        );
        assert_eq!(
            select_wallpaper(&pack, None, WallpaperSelection::Next)
                .unwrap()
                .index,
            2
        );
    }

    #[test]
    fn availability_requires_a_valid_theme_pack() {
        let root = env::temp_dir().join(format!("retheme-availability-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        assert!(available_themes(&root).is_err());
        let dir = root.join("themes/sakura");
        fs::create_dir_all(&dir).unwrap();
        fs::write(
            dir.join(THEME_FILE),
            "schema = 1\nname = \"sakura\"\nmode = \"dark\"\n",
        )
        .unwrap();
        fs::write(
            dir.join(BASE16_FILE),
            (0..16)
                .map(|i| format!("base{:02X}: \"#000000\"\n", i))
                .collect::<String>(),
        )
        .unwrap();
        fs::create_dir_all(root.join("themes/broken")).unwrap();
        fs::write(root.join("themes/broken/theme.toml"), "broken").unwrap();
        assert!(available_themes(&root).is_err());
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn list_rejects_missing_or_malformed_packs() {
        let root = env::temp_dir().join(format!("retheme-test-{}", std::process::id()));
        let themes = root.join("themes");
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(themes.join("incomplete")).unwrap();
        fs::write(themes.join("incomplete/theme.toml"), "").unwrap();
        fs::write(themes.join("incomplete/base16.yaml"), "").unwrap();
        assert!(discover_themes(&root).is_err());
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn wallpaper_pack_requires_zero_and_declared_current() {
        assert!(parse_wallpapers("[[wallpaper]]\nindex = 1\npath = \"one.png\"\n").is_err());
        assert!(
            parse_wallpapers("current = 2\n[[wallpaper]]\nindex = 0\npath = \"zero.png\"\n")
                .is_err()
        );
    }

    #[test]
    fn switching_minimal_and_detailed_packs_publishes_selected_directory() {
        let _env = EnvGuard::lock();
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
                fs::create_dir_all(dir.join("assets")).unwrap();
                fs::write(dir.join("assets/default.png"), b"default").unwrap();
                fs::write(dir.join("assets/next.png"), b"next").unwrap();
                fs::write(
                    dir.join("wallpapers.toml"),
                    "[[wallpaper]]\nindex = 0\npath = \"assets/default.png\"\n[[wallpaper]]\nindex = 1\npath = \"assets/next.png\"\n",
                )
                .unwrap();
            }
        }
        let config = root.join("config");
        env::set_var("RETHEME_ROOT", &root);
        env::set_var("HOME", root.join("home"));
        env::set_var("XDG_CONFIG_HOME", &config);
        env::set_var("PATH", root.join("empty-bin"));
        publish_switch(&root, prepare_switch(&root, "minimal").unwrap()).unwrap();
        assert_eq!(
            fs::read_link(root.join("active/theme")).unwrap(),
            PathBuf::from("../themes/minimal")
        );
        publish_switch(&root, prepare_switch(&root, "detailed").unwrap()).unwrap();
        assert_eq!(
            fs::read_link(root.join("active/theme")).unwrap(),
            PathBuf::from("../themes/detailed")
        );
        assert!(root.join("active/theme/apps/example.toml").is_file());
        assert_eq!(
            prepare_wallpaper_command(&root, "next").unwrap(),
            root.join("themes/detailed/assets/next.png")
                .canonicalize()
                .unwrap()
        );
        assert!(fs::read_to_string(root.join("active/wallpaper.toml"))
            .unwrap()
            .contains("index = 1"));
        assert_eq!(
            fs::read_link(root.join("active/theme")).unwrap(),
            PathBuf::from("../themes/detailed")
        );
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
        assert!(parse_base16(&valid.replace("#000000", "#a00000")).is_err());
    }
}
