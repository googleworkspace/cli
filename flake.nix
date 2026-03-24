{
  description = "Google Workspace CLI — dynamic command surface from Discovery Service";

  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";
    flake-utils.url = "github:numtide/flake-utils";
  };

  outputs = { self, nixpkgs, flake-utils }:
    flake-utils.lib.eachDefaultSystem (system:
      let
        pkgs = nixpkgs.legacyPackages.${system};

        # Extract version from CLI crate's Cargo.toml
        cargoToml = builtins.fromTOML (builtins.readFile ./crates/google-workspace-cli/Cargo.toml);
        version = cargoToml.package.version;

        # System dependencies
        # On Linux, keyring often needs libsecret
        # On macOS, it uses Security framework
        linuxDeps = with pkgs; [
          libsecret
        ];

        darwinDeps = with pkgs; [
          libiconv
          apple-sdk
        ];

        gws = pkgs.rustPlatform.buildRustPackage {
          pname = "gws";
          inherit version;

          src = ./.;

          cargoLock = {
            lockFile = ./Cargo.lock;
          };

          nativeBuildInputs = [ pkgs.pkg-config ];
          buildInputs = pkgs.lib.optionals pkgs.stdenv.isLinux linuxDeps
            ++ pkgs.lib.optionals pkgs.stdenv.isDarwin darwinDeps;

          # Tests are disabled by default in buildRustPackage if not specified, 
          # but we'll be explicit. Some tests might require network.
          doCheck = false;

          meta = with pkgs.lib; {
            description = cargoToml.package.description;
            homepage = cargoToml.package.homepage;
            license = licenses.asl20;
            maintainers = [{ name = "Justin Poehnelt"; email = "justin.poehnelt@gmail.com"; }];
            mainProgram = "gws";
          };
        };
      in
      {
        packages.default = gws;
        packages.gws = gws;

        apps.default = flake-utils.lib.mkApp {
          drv = gws;
        };

        devShells.default = pkgs.mkShell {
          inputsFrom = [ gws ];
          buildInputs = with pkgs; [
            rustc
            cargo
            rust-analyzer
            clippy
            rustfmt
          ];
        };
      }
    );
}
