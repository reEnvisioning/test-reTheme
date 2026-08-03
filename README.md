# reTheme

A small Linux CLI for installing and switching reEnvisioning Base16 theme packs.

```sh
retheme install https://github.com/reEnvisioning/themes.git
retheme list
retheme available
retheme switch sakura
retheme wallpaper next
```

## Runtime contract

`RETHEME_ROOT`, `XDG_CONFIG_HOME`, and `HOME` must be non-empty absolute paths when set or used. The root is selected in that order, then `~/.config/reEnvisioning`. `available` and `list` are side-effect-free; mutating commands create the selected root as needed and take one Linux process lock.

Installs use noninteractive, shallow, single-branch, no-tags Git with a 30-second timeout. Repository packs are structurally and semantically revalidated after copying and before publication. Publication uses staging, no-replace renames where Linux provides them, and attempts rollback and cleanup of every destination created by a failed operation.

Switches validate the existing active state before publication. The active state is canonical: the active theme symlink and wallpaper state are assembled together in a sibling staging directory and published atomically. Renderer files are written after publication; a fatal fixed-renderer write error can leave external files partially updated, and any renderer-local rollback is best effort—there is no cross-application rollback and active state is not reverted. External session-command failures warn. Malformed Base16, pack, active state, symlinks, and special files are fatal rather than normalized. Renderer staging cleanup failures are warnings and do not turn a successful write into an error.

## Theme pack layout

Every pack has exactly these root entries:

- required regular files: `theme.toml`, `base16.yaml`
- optional regular files: `wallpapers.toml`, `typography.toml`, `spacing.toml`, `animation.toml`, `icons.toml`, `fonts.toml`
- optional directories: `apps`, `assets` (containing only regular files and directories)

Unknown root entries, symlinks, and special files are rejected. `base16.yaml` must contain canonical `base00` through `base0F` keys and uppercase six-digit colors. Wallpapers use strict unique integer indices, require index `0`, and must resolve to regular files inside the pack.

## Linux renderers

Theme files and active state are owned by reTheme. Optional GTK 3/4 (GNOME, Cinnamon, Xfce), Qt5/6 (Plasma), Kitty, btop, Neovim, Firefox, LibreWolf, foot, Alacritty, and Chromium fragments are generated only when their config directories exist. Absent optional parent directories warn and skip; existing wrong-type parents, I/O errors, unsafe symlinked paths, malformed state, and renderer read/write failures are fatal. Chromium receives an unpacked theme at `chromium/retheme-theme/manifest.json`; foot and Alacritty receive fragments and may require one-time native activation/import.

Wallpaper application is an optional stdlib process boundary controlled by `RETHEME_WALLPAPER_BACKEND`:

- `rewallpaper` (default): checks `rewallpaper available`, then runs `rewallpaper apply <path>`
- `sway`: `swaymsg output '*' bg <path> fill`
- `hyprpaper`: `hyprctl hyprpaper wallpaper ,<path>,cover`
- `swww`: `swww img <path>` for wlroots sessions
- `none`: do not invoke a backend

Backend failures warn and do not block state publication. reTheme uses no dependencies, does not invoke NixOS hooks or shell commands, and has no macOS runtime support.
