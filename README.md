# reTheme

A small Rust CLI for installing and switching reEnvisioning Base16 theme packs.

```sh
retheme install https://github.com/reEnvisioning/themes.git
retheme list
retheme switch sakura
retheme wallpaper next
```

Themes live in `$RETHEME_ROOT/themes` (or `$XDG_CONFIG_HOME/reEnvisioning/themes`, usually `~/.config/reEnvisioning/themes`). Each selected theme must contain `theme.toml` and a strict `base16.yaml` with exactly `base00` through `base0F`. Switching publishes only the canonical `active/theme` symlink to the selected pack (plus optional `active/wallpaper.toml` selection state); stale legacy active files are removed.

## Optional Linux renderers

The core owns theme parsing, validation, selection, generated state, and publication. The renderer adapter module owns fixed generated files and best-effort desktop notifications. It targets existing GTK 3/4, Qt5/6, Kitty, btop, Neovim, Firefox, and LibreWolf configuration/profile directories. Paths use `$XDG_CONFIG_HOME` and fall back to `$HOME/.config`; a missing `HOME`, application directory, browser root/profile, or optional executable skips that adapter without blocking the switch. Unsafe profiles are skipped; readable-profile failures produce warnings. Renderer files are staged in `cache/` and committed before active theme state is published.

NixOS can package or configure reTheme, but reTheme does not invoke NixOS, depend on Nix-owned state, or run shell commands. The only process calls are direct optional executable invocations (`git` for install, desktop notification tools, and `rewallpaper apply`). Theme app files, handlers, targets, and arbitrary commands are never executed.

An optional `wallpapers.toml` uses strict `[[wallpaper]]` entries with `index` and a relative `path`; a top-level `current` index is supported. Paths must resolve to regular files inside the selected pack. Switching and `wallpaper restore` select the theme's `current` wallpaper, falling back to index `0`. `wallpaper next` and `prev` wrap through sorted declared indices; `wallpaper INDEX` selects that declared index. Each command updates `active/wallpaper.toml`. If available, `rewallpaper apply <path>` is invoked after publication; missing or failing reWallpaper is nonfatal.
