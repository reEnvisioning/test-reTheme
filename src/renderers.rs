use std::{
    env, fs,
    path::{Path, PathBuf},
    process::Command,
};

use crate::core::write_atomic;

fn home() -> Option<PathBuf> {
    env::var_os("HOME").map(PathBuf::from)
}

fn config_dir_from(xdg: Option<&Path>, home: Option<&Path>) -> Option<PathBuf> {
    xdg.map(Path::to_path_buf)
        .or_else(|| home.map(|home| home.join(".config")))
}

fn config_dir() -> std::io::Result<Option<PathBuf>> {
    let xdg = env::var_os("XDG_CONFIG_HOME").map(PathBuf::from);
    let home = home();
    for (name, path) in [("XDG_CONFIG_HOME", xdg.as_ref()), ("HOME", home.as_ref())] {
        if let Some(path) = path {
            if path.as_os_str().is_empty() || !path.is_absolute() {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::InvalidInput,
                    format!("{name} must be a non-empty absolute path"),
                ));
            }
        }
    }
    Ok(config_dir_from(xdg.as_deref(), home.as_deref()))
}

pub(crate) fn render_fixed(root: &Path, c: &[String; 16], dark: bool) -> std::io::Result<()> {
    let Some(config) = config_dir()? else {
        eprintln!("warning: HOME and XDG_CONFIG_HOME are unset; skipping optional renderers");
        return Ok(());
    };
    let home = home();
    let mut files = Vec::new();
    let gtk = gtk_colors(c);
    optional(&config, "gtk-3.0", "gtk-3.0/gtk.css", &gtk, &mut files)?;
    optional(&config, "gtk-4.0", "gtk-4.0/gtk.css", &gtk, &mut files)?;
    let qt_colors = qt_colors(c);
    for qtver in ["qt5ct", "qt6ct"] {
        optional_qt(&config, qtver, &qt_colors, &mut files)?;
    }
    optional(
        &config,
        "kitty",
        "kitty/retheme-base16.conf",
        &kitty_colors(c),
        &mut files,
    )?;
    optional(
        &config,
        "btop",
        "btop/themes/retheme-base16.theme",
        &btop_colors(c),
        &mut files,
    )?;
    optional(
        &config,
        "nvim",
        "nvim/colors/retheme-base16.lua",
        &nvim_colors(c, dark),
        &mut files,
    )?;
    optional(
        &config,
        "foot",
        "foot/retheme.ini",
        &foot_colors(c),
        &mut files,
    )?;
    optional(
        &config,
        "alacritty",
        "alacritty/retheme.toml",
        &alacritty_colors(c),
        &mut files,
    )?;
    let chromium = config.join("chromium");
    if path_is_symlinked(&chromium)? {
        eprintln!("warning: Chromium config path is symlinked; skipping renderer");
    } else {
        let chromium_exists = match fs::symlink_metadata(&chromium) {
            Ok(metadata) if !metadata.is_dir() => {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::InvalidInput,
                    "Chromium config path is not a regular directory",
                ));
            }
            Ok(_) => true,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => false,
            Err(error) => return Err(error),
        };
        if chromium_exists {
            let theme_dir = chromium.join("retheme-theme");
            if path_is_symlinked(&theme_dir)? {
                eprintln!("warning: Chromium theme path is symlinked; skipping renderer");
            } else {
                match fs::symlink_metadata(&theme_dir) {
                    Ok(metadata) if !metadata.is_dir() => {
                        return Err(std::io::Error::new(
                            std::io::ErrorKind::InvalidInput,
                            "Chromium generated theme must be a regular directory",
                        ));
                    }
                    Ok(_) => {
                        let mut symlinked_entry = false;
                        let mut unexpected = false;
                        for entry in fs::read_dir(&theme_dir)? {
                            let entry = entry?;
                            let file_type = entry.file_type()?;
                            if file_type.is_symlink() {
                                symlinked_entry = true;
                            } else {
                                unexpected |=
                                    entry.file_name() != "manifest.json" || !file_type.is_file();
                            }
                        }
                        if unexpected {
                            return Err(std::io::Error::new(
                                std::io::ErrorKind::InvalidInput,
                                "Chromium generated theme contains an unexpected entry",
                            ));
                        }
                        if symlinked_entry {
                            eprintln!("warning: Chromium generated theme contains a symlink; skipping renderer");
                        } else {
                            files.push((theme_dir.join("manifest.json"), chromium_manifest(c)));
                        }
                    }
                    Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                        files.push((theme_dir.join("manifest.json"), chromium_manifest(c)));
                    }
                    Err(error) => return Err(error),
                }
            }
        } else {
            eprintln!("warning: Chromium config directory unavailable; skipping renderer");
        }
    }
    let firefox = config.join("mozilla/firefox");
    browser_prefs(&firefox, dark, &mut files)?;
    browser_prefs(&config.join("librewolf"), dark, &mut files)?;
    if let Some(home) = &home {
        let home_firefox = home.join(".mozilla/firefox");
        if home_firefox != firefox {
            browser_prefs(&home_firefox, dark, &mut files)?;
        }
        browser_prefs(&home.join(".librewolf"), dark, &mut files)?;
    }

    let mut safe_files = Vec::with_capacity(files.len());
    for (destination, content) in files {
        if path_is_symlinked(&destination)? {
            eprintln!(
                "warning: renderer destination is symlinked; skipping {}",
                destination.display()
            );
        } else {
            ensure_safe_destination(&destination)?;
            safe_files.push((destination, content));
        }
    }
    let files = safe_files;
    let originals = files
        .iter()
        .map(|(destination, _)| {
            let original = match fs::symlink_metadata(destination) {
                Ok(metadata) if metadata.is_file() => Some(fs::read_to_string(destination)?),
                Ok(_) => {
                    return Err(std::io::Error::new(
                        std::io::ErrorKind::InvalidInput,
                        format!(
                            "renderer destination is not a regular file: {}",
                            destination.display()
                        ),
                    ))
                }
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => None,
                Err(error) => return Err(error),
            };
            Ok((destination.clone(), original))
        })
        .collect::<std::io::Result<Vec<_>>>()?;
    let stage = root
        .join("cache")
        .join(format!("retheme-render-{}", std::process::id()));
    if fs::symlink_metadata(&stage).is_ok() {
        crate::core::remove_path(&stage)?;
    }
    fs::create_dir_all(&stage)?;
    let result: std::io::Result<()> = (|| {
        for (index, (_, content)) in files.iter().enumerate() {
            fs::write(stage.join(index.to_string()), content)?;
        }
        for (index, (destination, _)) in files.iter().enumerate() {
            let content = fs::read_to_string(stage.join(index.to_string()))?;
            write_atomic(destination, &content)?;
        }
        Ok(())
    })();
    let cleanup = fs::remove_dir_all(&stage);
    match (result, cleanup) {
        (Err(error), cleanup) => {
            let mut rollback = Vec::new();
            for (destination, original) in &originals {
                let restored = match original {
                    Some(content) => write_atomic(destination, content),
                    None => crate::core::remove_path(destination),
                };
                if let Err(restore) = restored {
                    rollback.push(format!("{}: {restore}", destination.display()));
                }
            }
            let mut message = error.to_string();
            if let Err(cleanup) = cleanup {
                message.push_str(&format!("; renderer staging cleanup failed: {cleanup}"));
            }
            if !rollback.is_empty() {
                message.push_str(&format!(
                    "; renderer rollback failed: {}",
                    rollback.join(", ")
                ));
            }
            Err(std::io::Error::other(message))
        }
        (Ok(()), Ok(())) => Ok(()),
        (Ok(()), Err(error)) => {
            eprintln!("warning: renderer staging cleanup failed: {error}");
            Ok(())
        }
    }
}

fn optional_qt(
    config: &Path,
    qtver: &str,
    colors: &str,
    files: &mut Vec<(PathBuf, String)>,
) -> std::io::Result<bool> {
    let dir = config.join(qtver);
    let selected = optional(
        config,
        qtver,
        &format!("{qtver}/colors/reTheme.colors"),
        colors,
        files,
    )?;
    if selected {
        let colors_path = dir.join("colors/reTheme.colors");
        optional(
            config,
            qtver,
            &format!("{qtver}/{qtver}.conf"),
            &format!("[Appearance]\ncolor_scheme_path={}\ncustom_palette=true\nicon_theme=Adwaita\nstyle=Adwaita\n", colors_path.display()),
            files,
        )?;
    }
    Ok(selected)
}

fn path_is_symlinked(path: &Path) -> std::io::Result<bool> {
    let mut current = PathBuf::new();
    for component in path.components() {
        current.push(component.as_os_str());
        match fs::symlink_metadata(&current) {
            Ok(metadata) if metadata.file_type().is_symlink() => return Ok(true),
            Ok(_) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => break,
            Err(error) => return Err(error),
        }
    }
    Ok(false)
}

fn ensure_safe_destination(path: &Path) -> std::io::Result<()> {
    if path_is_symlinked(path)? {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            format!("renderer path is symlinked: {}", path.display()),
        ));
    }
    Ok(())
}

fn optional(
    base: &Path,
    label: &str,
    relative: &str,
    content: &str,
    files: &mut Vec<(PathBuf, String)>,
) -> std::io::Result<bool> {
    let app = base.join(label);
    if path_is_symlinked(&app)? {
        eprintln!("warning: {label} config path is symlinked; skipping renderer");
        return Ok(false);
    }
    match fs::symlink_metadata(&app) {
        Ok(metadata) if !metadata.is_dir() => {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                format!("{label} config path is not a regular directory"),
            ));
        }
        Ok(_) => {}
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            eprintln!("warning: {label} config directory unavailable; skipping renderer");
            return Ok(false);
        }
        Err(error) => return Err(error),
    }
    let destination = base.join(relative);
    let parent = destination
        .parent()
        .expect("renderer destination has a parent");
    if path_is_symlinked(parent)? || path_is_symlinked(&destination)? {
        eprintln!("warning: {label} renderer path is symlinked; skipping renderer");
        return Ok(false);
    }
    match fs::symlink_metadata(parent) {
        Ok(metadata) if !metadata.is_dir() => {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                format!(
                    "renderer parent is not a regular directory: {}",
                    parent.display()
                ),
            ));
        }
        Ok(_) => {}
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            eprintln!("warning: {label} config directory unavailable; skipping renderer");
            return Ok(false);
        }
        Err(error) => return Err(error),
    }
    files.push((destination, content.into()));
    Ok(true)
}

fn gtk_colors(c: &[String; 16]) -> String {
    format!("@define-color base00 {};\n@define-color base01 {};\n@define-color base02 {};\n@define-color base03 {};\n@define-color base04 {};\n@define-color base05 {};\n@define-color base08 {};\n@define-color base0A {};\n@define-color base0D {};\nwindow, dialog {{ background-color: @base00; color: @base05; }}\nheaderbar, toolbar {{ background-color: @base01; color: @base04; }}\nbutton, entry {{ background-color: @base02; color: @base05; border-color: @base03; }}\nselection {{ background-color: @base02; color: @base05; }}\n*:focus {{ outline-color: @base0A; }}\n.error {{ color: @base08; }}\nlink {{ color: @base0D; }}\n", c[0], c[1], c[2], c[3], c[4], c[5], c[8], c[10], c[13])
}

fn nvim_colors(c: &[String; 16], dark: bool) -> String {
    format!("local c = {{ bg = '{}', surface = '{}', selection = '{}', muted = '{}', dim = '{}', fg = '{}', red = '{}', orange = '{}', yellow = '{}', green = '{}', cyan = '{}', blue = '{}', magenta = '{}' }}\nvim.cmd('hi clear')\nvim.o.background = '{}'\nvim.api.nvim_set_hl(0, 'Normal', {{ fg = c.fg, bg = c.bg }})\nvim.api.nvim_set_hl(0, 'NormalFloat', {{ fg = c.fg, bg = c.surface }})\nvim.api.nvim_set_hl(0, 'Visual', {{ bg = c.selection }})\nvim.api.nvim_set_hl(0, 'LineNr', {{ fg = c.muted }})\nvim.api.nvim_set_hl(0, 'StatusLine', {{ fg = c.dim, bg = c.surface }})\nvim.api.nvim_set_hl(0, 'Comment', {{ fg = c.muted }})\nvim.api.nvim_set_hl(0, 'String', {{ fg = c.green }})\nvim.api.nvim_set_hl(0, 'Function', {{ fg = c.blue }})\nvim.api.nvim_set_hl(0, 'Keyword', {{ fg = c.magenta }})\nvim.api.nvim_set_hl(0, 'Number', {{ fg = c.orange }})\nvim.api.nvim_set_hl(0, 'Type', {{ fg = c.yellow }})\nvim.api.nvim_set_hl(0, 'DiagnosticError', {{ fg = c.red }})\n", c[0], c[1], c[2], c[3], c[4], c[5], c[8], c[9], c[10], c[11], c[12], c[13], c[14], if dark { "dark" } else { "light" })
}

fn terminal_colors(c: &[String; 16], prefix: &str, suffix: &str) -> String {
    let ansi = [0, 8, 11, 10, 13, 14, 12, 5, 3, 8, 11, 10, 13, 14, 12, 7];
    ansi.iter()
        .enumerate()
        .map(|(i, index)| format!("{prefix}{i}{suffix}{}\n", c[*index]))
        .collect()
}

fn kitty_colors(c: &[String; 16]) -> String {
    format!("# generated by retheme\nforeground {}\nbackground {}\nselection_foreground {}\nselection_background {}\ncursor {}\ncursor_text_color {}\n{}color16 {}\ncolor17 {}\n", c[5], c[0], c[5], c[2], c[4], c[0], terminal_colors(c, "color", " "), c[9], c[15])
}

fn btop_colors(c: &[String; 16]) -> String {
    format!("# generated by retheme\nmain_bg = \"{}\"\nmain_fg = \"{}\"\ntitle = \"{}\"\nhi_fg = \"{}\"\nselected_bg = \"{}\"\nselected_fg = \"{}\"\ninactive_fg = \"{}\"\ncpu_box = \"{}\"\nmem_box = \"{}\"\nnet_box = \"{}\"\nproc_box = \"{}\"\ntemp_start = \"{}\"\ntemp_mid = \"{}\"\ntemp_end = \"{}\"\n", c[0], c[5], c[4], c[14], c[2], c[5], c[3], c[13], c[11], c[12], c[10], c[11], c[10], c[8])
}

fn foot_colors(c: &[String; 16]) -> String {
    format!("# generated by retheme\n[colors]\nforeground={}\nbackground={}\nregular0={}\nregular1={}\nregular2={}\nregular3={}\nregular4={}\nregular5={}\nregular6={}\nregular7={}\nbright0={}\nbright1={}\nbright2={}\nbright3={}\nbright4={}\nbright5={}\nbright6={}\nbright7={}\n16={}\n17={}\nselection-foreground={}\nselection-background={}\n", &c[5][1..], &c[0][1..], &c[0][1..], &c[8][1..], &c[11][1..], &c[10][1..], &c[13][1..], &c[14][1..], &c[12][1..], &c[5][1..], &c[3][1..], &c[8][1..], &c[11][1..], &c[10][1..], &c[13][1..], &c[14][1..], &c[12][1..], &c[7][1..], &c[9][1..], &c[15][1..], &c[5][1..], &c[2][1..])
}

fn alacritty_colors(c: &[String; 16]) -> String {
    format!("# generated by retheme\n[colors.primary]\nbackground = \"{}\"\nforeground = \"{}\"\n[colors.selection]\nbackground = \"{}\"\nforeground = \"{}\"\n[[colors.indexed_colors]]\nindex = 16\ncolor = \"{}\"\n[[colors.indexed_colors]]\nindex = 17\ncolor = \"{}\"\n[colors.normal]\nblack = \"{}\"\nred = \"{}\"\ngreen = \"{}\"\nyellow = \"{}\"\nblue = \"{}\"\nmagenta = \"{}\"\ncyan = \"{}\"\nwhite = \"{}\"\n[colors.bright]\nblack = \"{}\"\nred = \"{}\"\ngreen = \"{}\"\nyellow = \"{}\"\nblue = \"{}\"\nmagenta = \"{}\"\ncyan = \"{}\"\nwhite = \"{}\"\n", c[0], c[5], c[2], c[5], c[9], c[15], c[0], c[8], c[11], c[10], c[13], c[14], c[12], c[5], c[3], c[8], c[11], c[10], c[13], c[14], c[12], c[7])
}

fn chromium_manifest(c: &[String; 16]) -> String {
    format!("{{\n  \"manifest_version\": 2,\n  \"version\": \"1.0\",\n  \"name\": \"reTheme generated theme\",\n  \"theme\": {{\n    \"colors\": {{\n      \"frame\": [ {}, {}, {} ],\n      \"toolbar\": [ {}, {}, {} ],\n      \"tab_text\": [ {}, {}, {} ]\n    }}\n  }}\n}}\n", u8::from_str_radix(&c[0][1..3],16).unwrap(), u8::from_str_radix(&c[0][3..5],16).unwrap(), u8::from_str_radix(&c[0][5..7],16).unwrap(), u8::from_str_radix(&c[2][1..3],16).unwrap(), u8::from_str_radix(&c[2][3..5],16).unwrap(), u8::from_str_radix(&c[2][5..7],16).unwrap(), u8::from_str_radix(&c[5][1..3],16).unwrap(), u8::from_str_radix(&c[5][3..5],16).unwrap(), u8::from_str_radix(&c[5][5..7],16).unwrap())
}

fn qt_colors(c: &[String; 16]) -> String {
    let indexes = [
        5, 1, 2, 3, 0, 1, 5, 8, 5, 0, 0, 3, 2, 5, 13, 10, 1, 3, 1, 5, 4,
    ];
    let active = indexes
        .iter()
        .map(|index| format!("#ff{}", &c[*index][1..]))
        .collect::<Vec<_>>()
        .join(", ");
    format!("[ColorScheme]\nactive_colors={active}\ndisabled_colors={active}\ninactive_colors={active}\n")
}

fn browser_prefs(
    root: &Path,
    dark: bool,
    files: &mut Vec<(PathBuf, String)>,
) -> std::io::Result<()> {
    if path_is_symlinked(root)? {
        eprintln!("warning: browser config root is symlinked; skipping renderer");
        return Ok(());
    }
    let profiles = root.join("profiles.ini");
    if path_is_symlinked(&profiles)? {
        eprintln!("warning: browser profiles.ini is symlinked; skipping renderer");
        return Ok(());
    }
    let text = match fs::read_to_string(&profiles) {
        Ok(text) => text,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            eprintln!("warning: browser config directory unavailable; skipping renderer");
            return Ok(());
        }
        Err(error) => return Err(error),
    };
    let canonical_root = root.canonicalize()?;
    let mut browser_files = Vec::new();
    let mut skipped_profile = false;
    for line in text.lines().filter_map(|l| l.strip_prefix("Path=")) {
        let relative = Path::new(line);
        if relative.is_absolute()
            || relative
                .components()
                .any(|c| matches!(c, std::path::Component::ParentDir))
        {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                "browser profile path must be relative and contained",
            ));
        }
        let profile = root.join(relative);
        if path_is_symlinked(&profile)? {
            eprintln!("warning: browser profile path is symlinked; skipping profile");
            skipped_profile = true;
            continue;
        }
        let profile = match profile.canonicalize() {
            Ok(profile) => profile,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => continue,
            Err(error) => return Err(error),
        };
        if !profile.starts_with(&canonical_root) || !profile.is_dir() {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                "browser profile outside config root",
            ));
        }
        let prefs = [
            ("ui.systemUsesDarkTheme", if dark { "1" } else { "0" }),
            ("browser.theme.toolbar-theme", if dark { "0" } else { "2" }),
            ("browser.theme.content-theme", if dark { "0" } else { "2" }),
        ];
        let path = profile.join("user.js");
        if path_is_symlinked(&path)? {
            eprintln!("warning: browser user.js path is symlinked; skipping profile");
            skipped_profile = true;
            continue;
        }
        let old = match fs::read_to_string(&path) {
            Ok(old) => old,
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => String::new(),
            Err(err) => return Err(err),
        };
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
        browser_files.push((path, out.join("\n") + "\n"));
    }
    if !skipped_profile {
        files.extend(browser_files);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn xdg_config_works_without_home() {
        assert_eq!(
            config_dir_from(Some(Path::new("/tmp/config")), None),
            Some(PathBuf::from("/tmp/config"))
        );
        assert_eq!(config_dir_from(None, None), None);
    }

    #[test]
    fn optional_targets_and_generated_helpers_are_selected() {
        let root = env::temp_dir().join(format!("retheme-renderers-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        for path in ["gtk-3.0", "qt5ct/colors", "kitty", "nvim/colors"] {
            fs::create_dir_all(root.join(path)).unwrap();
        }
        let colors: [String; 16] = std::array::from_fn(|index| format!("#{index:06X}"));
        let mut files = Vec::new();
        let gtk = gtk_colors(&colors);
        optional(&root, "gtk-3.0", "gtk-3.0/gtk.css", &gtk, &mut files).unwrap();
        let qt = qt_colors(&colors);
        optional_qt(&root, "qt5ct", &qt, &mut files).unwrap();
        let kitty_config = kitty_colors(&colors);
        optional(
            &root,
            "kitty",
            "kitty/retheme-base16.conf",
            &kitty_config,
            &mut files,
        )
        .unwrap();
        let nvim = nvim_colors(&colors, true);
        optional(
            &root,
            "nvim",
            "nvim/colors/retheme-base16.lua",
            &nvim,
            &mut files,
        )
        .unwrap();

        assert_eq!(
            files.iter().map(|(path, _)| path).collect::<Vec<_>>(),
            vec![
                &root.join("gtk-3.0/gtk.css"),
                &root.join("qt5ct/colors/reTheme.colors"),
                &root.join("qt5ct/qt5ct.conf"),
                &root.join("kitty/retheme-base16.conf"),
                &root.join("nvim/colors/retheme-base16.lua"),
            ]
        );
        assert!(files[0].1.contains("@define-color base00 #000000;"));
        assert!(files[0].1.contains("*:focus { outline-color: @base0A; }"));
        assert!(files[1]
            .1
            .contains("active_colors=#ff000005, #ff000001, #ff000002"));
        assert!(files[1].1.contains("active_colors=#ff000005, #ff000001, #ff000002, #ff000003, #ff000000, #ff000001, #ff000005, #ff000008, #ff000005, #ff000000, #ff000000, #ff000003, #ff000002, #ff000005, #ff00000D, #ff00000A, #ff000001, #ff000003, #ff000001, #ff000005, #ff000004"));
        assert!(files[4].1.contains("vim.o.background = 'dark'"));
        assert!(!files[4].1.contains("nvim."));
        assert!(files[4]
            .1
            .contains("vim.api.nvim_set_hl(0, 'DiagnosticError', { fg = c.red })"));
        let kitty = files
            .iter()
            .find(|(path, _)| path.ends_with("kitty/retheme-base16.conf"))
            .unwrap();
        assert!(kitty.1.contains("selection_foreground #000005"));
        assert!(kitty.1.contains("color16 #000009"));
        assert!(kitty.1.contains("color17 #00000F"));
        assert!(btop_colors(&colors).contains("main_bg = \"#000000\""));
        assert!(btop_colors(&colors).contains("hi_fg = \"#00000E\""));
        assert!(foot_colors(&colors).contains("16=000009\n17=00000F\n"));
        assert_eq!(foot_colors(&colors), "# generated by retheme\n[colors]\nforeground=000005\nbackground=000000\nregular0=000000\nregular1=000008\nregular2=00000B\nregular3=00000A\nregular4=00000D\nregular5=00000E\nregular6=00000C\nregular7=000005\nbright0=000003\nbright1=000008\nbright2=00000B\nbright3=00000A\nbright4=00000D\nbright5=00000E\nbright6=00000C\nbright7=000007\n16=000009\n17=00000F\nselection-foreground=000005\nselection-background=000002\n");
        assert_eq!(alacritty_colors(&colors), "# generated by retheme\n[colors.primary]\nbackground = \"#000000\"\nforeground = \"#000005\"\n[colors.selection]\nbackground = \"#000002\"\nforeground = \"#000005\"\n[[colors.indexed_colors]]\nindex = 16\ncolor = \"#000009\"\n[[colors.indexed_colors]]\nindex = 17\ncolor = \"#00000F\"\n[colors.normal]\nblack = \"#000000\"\nred = \"#000008\"\ngreen = \"#00000B\"\nyellow = \"#00000A\"\nblue = \"#00000D\"\nmagenta = \"#00000E\"\ncyan = \"#00000C\"\nwhite = \"#000005\"\n[colors.bright]\nblack = \"#000003\"\nred = \"#000008\"\ngreen = \"#00000B\"\nyellow = \"#00000A\"\nblue = \"#00000D\"\nmagenta = \"#00000E\"\ncyan = \"#00000C\"\nwhite = \"#000007\"\n");
        assert!(chromium_manifest(&colors).contains("\"manifest_version\": 2"));

        fs::create_dir_all(root.join("qt6ct")).unwrap();
        let mut files = Vec::new();
        optional_qt(&root, "qt6ct", &qt, &mut files).unwrap();
        assert!(files.is_empty());
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn optional_missing_is_skipped_but_wrong_type_is_fatal() {
        let root = env::temp_dir().join(format!("retheme-optional-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).unwrap();
        let mut files = Vec::new();
        assert!(!optional(&root, "missing", "missing/file", "content", &mut files).unwrap());
        fs::write(root.join("wrong"), "not a directory").unwrap();
        assert!(optional(&root, "wrong", "wrong/file", "content", &mut files).is_err());
        let _ = fs::remove_dir_all(root);
    }

    #[cfg(unix)]
    #[test]
    fn optional_symlink_paths_are_skipped() {
        use std::os::unix::fs::symlink;

        let root = env::temp_dir().join(format!("retheme-optional-link-{}", std::process::id()));
        let target = root.join("target");
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(target.join("gtk-3.0")).unwrap();
        symlink(&target, root.join("config")).unwrap();
        let mut files = Vec::new();
        assert!(!optional(
            &root.join("config"),
            "gtk-3.0",
            "gtk-3.0/gtk.css",
            "content",
            &mut files
        )
        .unwrap());

        symlink(&target.join("gtk-3.0"), target.join("destination")).unwrap();
        assert!(!optional(
            &target,
            "gtk-3.0",
            "destination/gtk.css",
            "content",
            &mut files
        )
        .unwrap());
        assert!(files.is_empty());
        let _ = fs::remove_dir_all(root);
    }

    #[cfg(unix)]
    #[test]
    fn render_fixed_skips_symlinked_chromium_theme_entries() {
        use std::os::unix::fs::symlink;

        let root = env::temp_dir().join(format!("retheme-chromium-link-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        let config = root.join("config");
        let theme = config.join("chromium/retheme-theme");
        fs::create_dir_all(&theme).unwrap();
        symlink(root.join("manifest-target"), theme.join("manifest.json")).unwrap();
        fs::write(root.join("manifest-target"), "keep").unwrap();

        let old_xdg = env::var_os("XDG_CONFIG_HOME");
        let old_home = env::var_os("HOME");
        env::set_var("XDG_CONFIG_HOME", &config);
        env::remove_var("HOME");
        let colors: [String; 16] = std::array::from_fn(|index| format!("#{index:06X}"));
        render_fixed(&root, &colors, true).unwrap();
        assert_eq!(
            fs::read_to_string(root.join("manifest-target")).unwrap(),
            "keep"
        );
        assert!(fs::symlink_metadata(theme.join("manifest.json"))
            .unwrap()
            .file_type()
            .is_symlink());

        fs::remove_file(theme.join("manifest.json")).unwrap();
        symlink(
            root.join("unexpected-target"),
            theme.join("unexpected-link"),
        )
        .unwrap();
        render_fixed(&root, &colors, true).unwrap();
        assert!(fs::symlink_metadata(theme.join("unexpected-link"))
            .unwrap()
            .file_type()
            .is_symlink());

        match old_xdg {
            Some(value) => env::set_var("XDG_CONFIG_HOME", value),
            None => env::remove_var("XDG_CONFIG_HOME"),
        }
        match old_home {
            Some(value) => env::set_var("HOME", value),
            None => env::remove_var("HOME"),
        }
        let _ = fs::remove_dir_all(root);
    }

    #[cfg(unix)]
    #[test]
    fn browser_symlink_paths_are_skipped() {
        use std::os::unix::fs::symlink;

        let root = env::temp_dir().join(format!("retheme-browser-links-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(root.join("real/profile")).unwrap();
        fs::write(root.join("real/profiles.ini"), "Path=profile\n").unwrap();

        symlink(root.join("real"), root.join("root-link")).unwrap();
        let mut files = Vec::new();
        browser_prefs(&root.join("root-link"), true, &mut files).unwrap();
        assert!(files.is_empty());

        fs::copy(
            root.join("real/profiles.ini"),
            root.join("profiles.ini.target"),
        )
        .unwrap();
        let mut files = Vec::new();
        browser_prefs(&root.join("real"), true, &mut files).unwrap();
        assert_eq!(files.len(), 1);

        symlink(root.join("real/profile"), root.join("real/profile-link")).unwrap();
        fs::write(
            root.join("real/profiles.ini"),
            "Path=profile\nPath=profile-link\n",
        )
        .unwrap();
        let existing = (root.join("existing"), String::from("keep"));
        let mut files = vec![existing.clone()];
        browser_prefs(&root.join("real"), true, &mut files).unwrap();
        assert_eq!(files, vec![existing]);

        fs::remove_file(root.join("real/profiles.ini")).unwrap();
        symlink(
            root.join("profiles.ini.target"),
            root.join("real/profiles.ini"),
        )
        .unwrap();
        let mut files = Vec::new();
        browser_prefs(&root.join("real"), true, &mut files).unwrap();
        assert!(files.is_empty());

        fs::remove_file(root.join("real/profiles.ini")).unwrap();
        fs::write(root.join("real/profiles.ini"), "Path=profile-link\n").unwrap();
        let mut files = Vec::new();
        browser_prefs(&root.join("real"), true, &mut files).unwrap();
        assert!(files.is_empty());

        fs::remove_file(root.join("real/profiles.ini")).unwrap();
        fs::write(root.join("real/profiles.ini"), "Path=profile\n").unwrap();
        fs::write(root.join("user.js.target"), "user_pref(\"x\", true);\n").unwrap();
        symlink(
            root.join("user.js.target"),
            root.join("real/profile/user.js"),
        )
        .unwrap();
        let mut files = Vec::new();
        browser_prefs(&root.join("real"), true, &mut files).unwrap();
        assert!(files.is_empty());
        let _ = fs::remove_dir_all(root);
    }

    #[cfg(unix)]
    #[test]
    fn browser_symlinked_profile_still_validates_later_profiles() {
        use std::os::unix::fs::symlink;

        let root = env::temp_dir().join(format!(
            "retheme-browser-link-validation-{}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(root.join("profile")).unwrap();
        symlink(root.join("profile"), root.join("profile-link")).unwrap();
        fs::write(
            root.join("profiles.ini"),
            "Path=profile-link\nPath=../outside\n",
        )
        .unwrap();
        let existing = (root.join("existing"), String::from("keep"));
        let mut files = vec![existing.clone()];

        assert!(browser_prefs(&root, true, &mut files).is_err());
        assert_eq!(files, vec![existing]);
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn browser_preflight_errors_propagate() {
        let root = env::temp_dir().join(format!("retheme-browser-invalid-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).unwrap();
        fs::write(root.join("profiles.ini"), "Path=../outside\n").unwrap();
        let mut files = Vec::new();
        assert!(browser_prefs(&root, true, &mut files).is_err());
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn kitty_pid_requires_strict_decimal_nonzero_values() {
        for value in ["1", "42", "2147483647"] {
            assert!(kitty_pid(std::ffi::OsStr::new(value)).is_ok());
        }
        for value in [
            "",
            "0",
            "+1",
            " 1",
            "1 ",
            "1.0",
            "-1",
            "2147483648",
            "4294967295",
        ] {
            assert!(kitty_pid(std::ffi::OsStr::new(value)).is_err());
        }
    }

    #[test]
    fn wallpaper_backend_names_are_explicit() {
        use std::ffi::OsStr;
        #[cfg(unix)]
        use std::os::unix::ffi::OsStrExt;

        assert_eq!(wallpaper_backend_name(None).unwrap(), "auto");
        for name in [
            "auto",
            "rewallpaper",
            "sway",
            "hyprpaper",
            "swww",
            "swaybg",
            "none",
        ] {
            assert_eq!(
                wallpaper_backend_name(Some(OsStr::new(name))).unwrap(),
                name
            );
        }
        assert!(wallpaper_backend_name(Some(OsStr::new("unknown-backend"))).is_err());
        #[cfg(unix)]
        assert!(wallpaper_backend_name(Some(std::ffi::OsStr::from_bytes(b"bad\xff"))).is_err());
        let all = AutoAvailability {
            rewallpaper: true,
            sway: true,
            hyprpaper: true,
            swww: true,
            swaybg: true,
        };
        assert_eq!(resolve_auto(all), Some(WallpaperBackend::Rewallpaper));
        assert_eq!(
            resolve_auto(AutoAvailability {
                rewallpaper: false,
                ..all
            }),
            Some(WallpaperBackend::Sway)
        );
        assert_eq!(
            resolve_auto(AutoAvailability {
                rewallpaper: false,
                sway: false,
                ..all
            }),
            Some(WallpaperBackend::Hyprpaper)
        );
        assert_eq!(
            resolve_auto(AutoAvailability {
                rewallpaper: false,
                sway: false,
                hyprpaper: false,
                ..all
            }),
            Some(WallpaperBackend::Swww)
        );
        assert_eq!(
            resolve_auto(AutoAvailability {
                rewallpaper: false,
                sway: false,
                hyprpaper: false,
                swww: false,
                ..all
            }),
            Some(WallpaperBackend::Swaybg)
        );
        assert_eq!(
            resolve_auto(AutoAvailability {
                rewallpaper: false,
                sway: false,
                hyprpaper: false,
                swww: false,
                swaybg: false
            }),
            None
        );
        assert_eq!(
            command_spec(WallpaperBackend::Swaybg, Path::new("/tmp/a b.png")),
            (
                "swaybg",
                vec![
                    "-i".into(),
                    "/tmp/a b.png".into(),
                    "-m".into(),
                    "fill".into()
                ]
            )
        );
        assert!(parse_swaybg_state("pid=12\nstart_time=34\n").is_some());
        assert!(parse_swaybg_state("pid=12\npid=13\nstart_time=34\n").is_none());
    }

    #[test]
    fn browser_preserves_unmanaged_preferences() {
        let root = env::temp_dir().join(format!("retheme-browser-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        let profile = root.join("profile");
        fs::create_dir_all(&profile).unwrap();
        fs::write(root.join("profiles.ini"), "Path=profile\n").unwrap();
        fs::write(profile.join("user.js"), "user_pref(\"unmanaged\", true);\n").unwrap();
        let mut files = Vec::new();
        browser_prefs(&root, true, &mut files).unwrap();
        assert_eq!(files.len(), 1);
        assert!(files[0].1.contains("unmanaged"));
        assert!(files[0].1.contains("ui.systemUsesDarkTheme"));
        let _ = fs::remove_dir_all(root);
    }
}

pub(crate) fn apply_renderers(dark: bool) {
    optional_command(
        "gsettings",
        [
            "set",
            "org.gnome.desktop.interface",
            "color-scheme",
            if dark { "prefer-dark" } else { "default" },
        ],
    );
    optional_command(
        "gsettings",
        ["set", "org.gnome.desktop.interface", "gtk-theme", "Adwaita"],
    );
    reload_kitty();
    optional_command("pkill", ["-SIGUSR2", "btop"]);
}

fn kitty_pid(value: &std::ffi::OsStr) -> Result<i32, &'static str> {
    let value = value.to_str().ok_or("must be valid UTF-8")?;
    if value.is_empty() || !value.bytes().all(|byte| byte.is_ascii_digit()) {
        return Err("must be strictly decimal");
    }
    let pid = value.parse::<i32>().map_err(|_| "is out of range")?;
    (pid != 0).then_some(pid).ok_or("must be nonzero")
}

fn reload_kitty() {
    match env::var_os("KITTY_PID") {
        Some(value) => match kitty_pid(&value) {
            Ok(pid) => match Command::new("kill")
                .args(["-SIGUSR1", &pid.to_string()])
                .status()
            {
                Ok(status) if status.success() => {}
                Ok(status) => eprintln!("warning: kill failed for Kitty PID {pid} ({status})"),
                Err(err) => eprintln!("warning: kill unavailable for Kitty PID {pid}: {err}"),
            },
            Err(error) => eprintln!("warning: invalid KITTY_PID ({error}); skipping Kitty reload"),
        },
        None => optional_command("pkill", ["-SIGUSR1", "-x", "kitty"]),
    }
}

fn optional_command<const N: usize>(program: &str, args: [&str; N]) {
    match Command::new(program).args(args).status() {
        Ok(status) if status.success() => {}
        Ok(status) => eprintln!("warning: {program} failed ({status})"),
        Err(err) => eprintln!("warning: {program} unavailable: {err}"),
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum WallpaperBackend {
    Rewallpaper,
    Sway,
    Hyprpaper,
    Swww,
    Swaybg,
    None,
}

fn wallpaper_backend() -> std::io::Result<&'static str> {
    wallpaper_backend_name(env::var_os("RETHEME_WALLPAPER_BACKEND").as_deref())
}

pub(crate) fn validate_wallpaper_backend() -> std::io::Result<()> {
    wallpaper_backend().map(|_| ())
}

fn wallpaper_backend_name(value: Option<&std::ffi::OsStr>) -> std::io::Result<&'static str> {
    let value = match value {
        None => "auto",
        Some(value) => value.to_str().ok_or_else(|| {
            std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                "RETHEME_WALLPAPER_BACKEND must be valid UTF-8",
            )
        })?,
    };
    match value {
        "auto" => Ok("auto"),
        "rewallpaper" => Ok("rewallpaper"),
        "sway" => Ok("sway"),
        "hyprpaper" => Ok("hyprpaper"),
        "swww" => Ok("swww"),
        "swaybg" => Ok("swaybg"),
        "none" => Ok("none"),
        _ => Err(std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            "RETHEME_WALLPAPER_BACKEND must be auto, rewallpaper, sway, hyprpaper, swww, swaybg, or none",
        )),
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct AutoAvailability {
    rewallpaper: bool,
    sway: bool,
    hyprpaper: bool,
    swww: bool,
    swaybg: bool,
}

fn resolve_auto(available: AutoAvailability) -> Option<WallpaperBackend> {
    [
        (available.rewallpaper, WallpaperBackend::Rewallpaper),
        (available.sway, WallpaperBackend::Sway),
        (available.hyprpaper, WallpaperBackend::Hyprpaper),
        (available.swww, WallpaperBackend::Swww),
        (available.swaybg, WallpaperBackend::Swaybg),
    ]
    .into_iter()
    .find_map(|(available, backend)| available.then_some(backend))
}

fn executable(name: &str) -> bool {
    let Some(path) = env::var_os("PATH") else {
        return false;
    };
    env::split_paths(&path).any(|dir| {
        let path = dir.join(name);
        fs::metadata(path).is_ok_and(|m| {
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                m.is_file() && m.permissions().mode() & 0o111 != 0
            }
            #[cfg(not(unix))]
            {
                m.is_file()
            }
        })
    })
}

fn auto_backend() -> Option<WallpaperBackend> {
    resolve_auto(AutoAvailability {
        rewallpaper: rewallpaper_available_quiet(),
        sway: !env::var_os("SWAYSOCK").is_none_or(|v| v.is_empty()) && executable("swaymsg"),
        hyprpaper: !env::var_os("HYPRLAND_INSTANCE_SIGNATURE").is_none_or(|v| v.is_empty())
            && executable("hyprctl"),
        swww: executable("swww"),
        swaybg: !env::var_os("WAYLAND_DISPLAY").is_none_or(|v| v.is_empty())
            && executable("swaybg"),
    })
}

fn command_spec(backend: WallpaperBackend, path: &Path) -> (&'static str, Vec<String>) {
    let path = path.display().to_string();
    match backend {
        WallpaperBackend::Rewallpaper => ("rewallpaper", vec!["apply".into(), path]),
        WallpaperBackend::Sway => (
            "swaymsg",
            vec![
                "output".into(),
                "*".into(),
                "bg".into(),
                path,
                "fill".into(),
            ],
        ),
        WallpaperBackend::Hyprpaper => (
            "hyprctl",
            vec![
                "hyprpaper".into(),
                "wallpaper".into(),
                format!(",{path},cover"),
            ],
        ),
        WallpaperBackend::Swww => ("swww", vec!["img".into(), path]),
        WallpaperBackend::Swaybg => (
            "swaybg",
            vec!["-i".into(), path, "-m".into(), "fill".into()],
        ),
        WallpaperBackend::None => unreachable!(),
    }
}

pub(crate) fn apply_wallpaper(root: &Path, path: &Path) -> std::io::Result<()> {
    let configured = wallpaper_backend()?;
    let backend = match configured {
        "auto" => auto_backend().unwrap_or_else(|| {
            eprintln!("warning: no usable wallpaper backend found; skipping wallpaper apply");
            WallpaperBackend::None
        }),
        "rewallpaper" => WallpaperBackend::Rewallpaper,
        "sway" => WallpaperBackend::Sway,
        "hyprpaper" => WallpaperBackend::Hyprpaper,
        "swww" => WallpaperBackend::Swww,
        "swaybg" => WallpaperBackend::Swaybg,
        "none" => WallpaperBackend::None,
        _ => unreachable!(),
    };
    if backend != WallpaperBackend::Swaybg {
        if let Err(error) = stop_tracked_swaybg(root) {
            eprintln!("warning: could not stop tracked swaybg: {error}");
            return Ok(());
        }
    }
    if backend == WallpaperBackend::None {
        return Ok(());
    }
    if backend == WallpaperBackend::Rewallpaper && !rewallpaper_available() {
        return Ok(());
    }
    if backend == WallpaperBackend::Swaybg {
        return apply_swaybg(root, path);
    }
    let (program, args) = command_spec(backend, path);
    match Command::new(program).args(&args).status() {
        Ok(status) if status.success() => {}
        Ok(status) => eprintln!("warning: {program} failed ({status})"),
        Err(err) => eprintln!("warning: {program} unavailable: {err}"),
    }
    Ok(())
}

fn rewallpaper_available() -> bool {
    match Command::new("rewallpaper").arg("available").status() {
        Ok(status) if status.success() => true,
        Ok(status) => {
            eprintln!("warning: rewallpaper availability check failed ({status}); skipping wallpaper apply");
            false
        }
        Err(err) => {
            eprintln!("warning: rewallpaper unavailable: {err}; skipping wallpaper apply");
            false
        }
    }
}

fn rewallpaper_available_quiet() -> bool {
    Command::new("rewallpaper")
        .arg("available")
        .status()
        .is_ok_and(|status| status.success())
}

fn swaybg_pid_path(root: &Path) -> PathBuf {
    root.join("active/wallpaper.pid")
}

fn parse_swaybg_state(text: &str) -> Option<(u32, u64)> {
    let mut pid = None;
    let mut start = None;
    for line in text.lines() {
        let (key, value) = line.split_once('=')?;
        match key {
            "pid" if pid.is_none() => pid = Some(value.parse().ok()?),
            "start_time" if start.is_none() => start = Some(value.parse().ok()?),
            _ => return None,
        }
    }
    let pid: u32 = pid?;
    let start: u64 = start?;
    (pid != 0 && start != 0).then_some((pid, start))
}

#[cfg(target_os = "linux")]
fn proc_start_time(pid: u32) -> Option<u64> {
    let text = fs::read_to_string(format!("/proc/{pid}/stat")).ok()?;
    let end = text.rfind(") ")?;
    text[end + 2..].split_whitespace().nth(19)?.parse().ok()
}

#[cfg(target_os = "linux")]
fn is_owned_swaybg(pid: u32, start_time: u64) -> bool {
    if proc_start_time(pid) != Some(start_time) {
        return false;
    }
    let executable = fs::read_link(format!("/proc/{pid}/exe"))
        .ok()
        .and_then(|path| path.file_name().map(|name| name == "swaybg"))
        .unwrap_or(false);
    let cmdline = match fs::read(format!("/proc/{pid}/cmdline")) {
        Ok(cmdline) => cmdline,
        Err(_) => return false,
    };
    let program = cmdline.split(|byte| *byte == 0).next().unwrap_or_default();
    executable
        && Path::new(std::str::from_utf8(program).unwrap_or_default())
            .file_name()
            .is_some_and(|name| name == "swaybg")
}

pub(crate) fn stop_tracked_swaybg(root: &Path) -> std::io::Result<()> {
    #[cfg(not(target_os = "linux"))]
    {
        let _ = root;
        return Ok(());
    }
    #[cfg(target_os = "linux")]
    {
        let pid_path = swaybg_pid_path(root);
        let text = match fs::read_to_string(&pid_path) {
            Ok(text) => text,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(()),
            Err(error) => return Err(error),
        };
        if let Some((pid, start)) = parse_swaybg_state(&text) {
            if is_owned_swaybg(pid, start) {
                let status = Command::new("kill").arg(pid.to_string()).status()?;
                if !status.success() {
                    return Err(std::io::Error::other(format!(
                        "could not stop owned swaybg PID {pid} ({status})"
                    )));
                }
            }
        }
        crate::core::remove_path(&pid_path)
    }
}

fn apply_swaybg(root: &Path, path: &Path) -> std::io::Result<()> {
    #[cfg(not(target_os = "linux"))]
    {
        let _ = (root, path);
        return Ok(());
    }
    #[cfg(target_os = "linux")]
    {
        let pid_path = swaybg_pid_path(root);
        if let Ok(text) = fs::read_to_string(&pid_path) {
            if let Some((pid, start)) = parse_swaybg_state(&text) {
                if is_owned_swaybg(pid, start) {
                    match Command::new("kill").arg(pid.to_string()).status() {
                        Ok(status) if status.success() => {}
                        Ok(status) => {
                            eprintln!("warning: could not stop owned swaybg PID {pid} ({status}); skipping replacement");
                            return Ok(());
                        }
                        Err(error) => {
                            eprintln!("warning: could not stop owned swaybg PID {pid}: {error}; skipping replacement");
                            return Ok(());
                        }
                    }
                    for _ in 0..100 {
                        if !is_owned_swaybg(pid, start) {
                            break;
                        }
                        std::thread::sleep(std::time::Duration::from_millis(10));
                    }
                    if is_owned_swaybg(pid, start) {
                        eprintln!(
                            "warning: owned swaybg PID {pid} did not exit; skipping replacement"
                        );
                        return Ok(());
                    }
                    crate::core::remove_path(&pid_path)?;
                }
            }
        }
        let (_, args) = command_spec(WallpaperBackend::Swaybg, path);
        let mut child = match Command::new("swaybg").args(&args).spawn() {
            Ok(child) => child,
            Err(error) => {
                eprintln!("warning: swaybg unavailable: {error}");
                return Ok(());
            }
        };
        let start = match proc_start_time(child.id()) {
            Some(start) => start,
            None => {
                let _ = child.kill();
                let _ = child.wait();
                eprintln!("warning: cannot identify spawned swaybg; skipping wallpaper apply");
                return Ok(());
            }
        };
        if let Err(error) = write_atomic(
            &pid_path,
            &format!("pid={}\nstart_time={start}\n", child.id()),
        ) {
            let _ = child.kill();
            let _ = child.wait();
            eprintln!("warning: cannot record swaybg ownership: {error}");
        }
        Ok(())
    }
}
