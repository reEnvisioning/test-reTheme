# reTheme

A small Rust CLI for installing and switching reEnvisioning TOML theme packs.

```sh
retheme install https://github.com/reEnvisioning/themes.git
retheme list
retheme switch sakura
```

Themes live in `$RETHEME_ROOT/themes` (or `$XDG_CONFIG_HOME/reEnvisioning/themes`, usually `~/.config/reEnvisioning/themes`). Switching updates the active theme files and applies `handler = "file"` entries.

Only install themes you trust: a theme can overwrite the user-writable paths it declares. Settings and wallpaper hooks remain outside this CLI for now.
