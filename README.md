# reTheme

Minimal reEnvisioning theme switcher.

```sh
retheme install reEnvisioning/themes
retheme list
retheme switch sakura
```

Only install themes you trust. Theme metadata can write user config files declared by the theme when you switch to it.

## NixOS flake import

After the repository is published, add reTheme as an input:

```nix
inputs.retheme = {
  url = "github:reEnvisioning/reTheme";
  inputs.nixpkgs.follows = "nixpkgs";
};
```

Expose the package to modules:

```nix
outputs = { self, nixpkgs, retheme, ... }:
let
  system = "x86_64-linux";
  rethemePackage = retheme.packages.${system}.default;
in {
  nixosConfigurations.host = nixpkgs.lib.nixosSystem {
    inherit system;
    specialArgs = { inherit rethemePackage; };
    modules = [ ./theme/appearance.nix ];
  };
}
```

`projects/NixOS/theme/appearance.nix` already accepts `rethemePackage ? null` and installs it when provided.

For local testing before publication, temporarily use `url = "path:/absolute/path/to/projects/reTheme"` in the consuming flake, then remove it before publishing.
