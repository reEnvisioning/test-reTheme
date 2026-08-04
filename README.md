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

Installs use noninteractive, shallow, single-branch, no-tags Git with a 30-second timeout. Repository packs are structurally and semantically revalidated after copying and before publication. Publication stages every pack, atomically replaces same-name packs when requested, leaves unrelated themes untouched, and rolls back replaced/new packs if publication fails. Output distinguishes `installed` from `updated` names.

Switches validate the existing active state before publication. A one-time migration accepts only the known legacy projection (`theme.toml`, `base16.yaml`, `colors.toml`, `current-theme`, `apps`, `theme.json`, and `wallpaper.toml`, with the canonical `theme` symlink optionally present); unknown active entries and malformed canonical packs remain fatal. The legacy directory is exchanged transactionally and retained under the root `cache` as a migration backup. The active state is canonical: the active theme symlink and wallpaper state are assembled together in a sibling staging directory and published atomically. Renderer files are written after publication; a fatal fixed-renderer write error can leave external files partially updated, and any renderer-local rollback is best effort—there is no cross-application rollback and active state is not reverted. External session-command failures warn. Malformed Base16, pack, active state, symlinks, and special files are fatal rather than normalized. Renderer staging cleanup failures are warnings and do not turn a successful write into an error.

## Theme pack layout

Every pack has exactly these root entries:

- required regular files: `theme.toml`, `base16.yaml`
- optional regular files: `wallpapers.toml`, `typography.toml`, `spacing.toml`, `animation.toml`, `icons.toml`, `fonts.toml`
- optional directories: `apps`, `assets` (containing only regular files and directories)

Unknown root entries, symlinks, and special files are rejected. `base16.yaml` must contain canonical `base00` through `base0F` keys and uppercase six-digit colors. Wallpapers use strict unique integer indices, require index `0`, and must resolve to regular files inside the pack.

## Linux renderers

Theme files and active state are owned by reTheme. Optional GTK 3/4 (GNOME, Cinnamon, Xfce), Qt5/6 (Plasma), Kitty, btop, Neovim, Firefox, LibreWolf, foot, Alacritty, and Chromium fragments are generated only when their config directories exist. Absent optional parent directories warn and skip; existing wrong-type parents, I/O errors, unsafe symlinked paths, malformed state, and renderer read/write failures are fatal. Chromium receives an unpacked theme at `chromium/retheme-theme/manifest.json`; foot and Alacritty receive fragments and may require one-time native activation/import. Base16 `base00`–`base07` are UI roles; terminal ANSI maps black=`base00`, red=`base08`, green=`base0B`, yellow=`base0A`, blue=`base0D`, magenta=`base0E`, cyan=`base0C`, white=`base05`; bright black=`base03`, bright white=`base07`, color16=`base09`, and color17=`base0F`.

Wallpaper application is an optional stdlib process boundary controlled by `RETHEME_WALLPAPER_BACKEND` (unset and `auto` are equivalent). Auto probes, in order, working `rewallpaper available`, Sway (`SWAYSOCK` nonempty plus `swaymsg`), Hyprland (`HYPRLAND_INSTANCE_SIGNATURE` plus `hyprctl`), `swww`, then Wayland `swaybg` (`WAYLAND_DISPLAY` nonempty plus `swaybg`). If none is usable, reTheme warns clearly and skips.

Explicit backends are `rewallpaper`, `sway`, `hyprpaper`, `swww`, `swaybg`, and `none`; they retain their direct commands. The final Wayland fallback runs `swaybg -i <path> -m fill`. Its owned PID and Linux `/proc` start-time identity are stored in `active/wallpaper.pid`; replacement stops only a tracked, still-matching `swaybg`, so stale or reused PIDs are never signaled. Active transactions preserve this state. Backend failures warn and do not block state publication. reTheme uses no dependencies, does not invoke NixOS hooks or shell commands, and has no macOS runtime support.
