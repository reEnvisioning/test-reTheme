# reTheme

A small Rust CLI for installing and switching reEnvisioning Base16 theme packs.

```sh
retheme install https://github.com/reEnvisioning/themes.git
retheme list
retheme switch sakura
```

Themes live in `$RETHEME_ROOT/themes` (or `$XDG_CONFIG_HOME/reEnvisioning/themes`, usually `~/.config/reEnvisioning/themes`). Each theme owns a strict `base16.yaml` with exactly `base00` through `base0F`. Switching atomically publishes `active/base16.yaml`, keeps the metadata/apps publication, and applies only fixed user renderers (GTK, Qt, Kitty, btop, browsers, and Neovim).

Theme app files are published as data only; declared handlers and targets are never executed. Wallpaper and unsupported app hooks remain outside this CLI.
