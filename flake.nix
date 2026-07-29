{
  description = "Theme switcher for reEnvisioning";

  inputs.nixpkgs.url = "github:nixos/nixpkgs/nixos-26.05";

  outputs = { self, nixpkgs }:
    let
      system = "x86_64-linux";
      pkgs = nixpkgs.legacyPackages.${system};
      retheme = pkgs.rustPlatform.buildRustPackage {
        pname = "retheme";
        version = "0.1.0";
        src = ./.;
        cargoLock.lockFile = ./Cargo.lock;
      };
    in {
      packages.${system} = {
        default = retheme;
        retheme = retheme;
      };

      overlays.default = final: prev: {
        retheme = self.packages.${final.system}.default;
      };

      apps.${system}.default = {
        type = "app";
        program = "${retheme}/bin/retheme";
      };

      devShells.${system}.default = pkgs.mkShell {
        packages = with pkgs; [ cargo rustc rustfmt clippy rust-analyzer ];
      };

      formatter.${system} = pkgs.nixpkgs-fmt;
    };
}
