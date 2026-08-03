# reTheme

A small Rust CLI for installing and switching reEnvisioning Base16 theme packs.

```sh
retheme install https://github.com/reEnvisioning/themes.git
retheme list
retheme switch sakura
retheme wallpaper next
```

Themes live in `$RETHEME_ROOT/themes` (or `$XDG_CONFIG_HOME/reEnvisioning/themes`, usually `~/.config/reEnvisioning/themes`). Each selected theme must contain `theme.toml` and a strict `base16.yaml` with exactly `base00` through `base0F`. Switching publishes only the canonical `active/theme` symlink to the selected pack (plus optional `active/wallpaper.toml` selection state); stale legacy active files are removed.

Switching also applies fixed GTK, Qt, Kitty, btop, browser, and Neovim renderers. Renderer files are prepared in `cache/` and committed before the active theme is published. A filesystem failure during that commit can still leave some external renderer files updated; cross-application rollback is not possible. An optional `wallpapers.toml` uses strict `[[wallpaper]]` entries with `index` and a relative `path`; a top-level `current` index is supported. Paths must resolve to regular files inside the selected pack. Switching and `wallpaper restore` select the theme's `current` wallpaper, falling back to index `0`. `wallpaper next` and `prev` wrap through sorted declared indices; `wallpaper INDEX` selects that declared index. Each command updates `active/wallpaper.toml`. If available, `rewallpaper apply <path>` is invoked after publication; missing or failing reWallpaper is nonfatal. Theme app files, handlers, targets, and arbitrary commands are never executed.
