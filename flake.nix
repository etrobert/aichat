{
  description = "aichat, with a client type backed by a local command";

  inputs.nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";

  outputs =
    { self, nixpkgs }:
    let
      systems = [
        "x86_64-linux"
        "aarch64-linux"
        "aarch64-darwin"
      ];

      forAllSystems = f: nixpkgs.lib.genAttrs systems (system: f nixpkgs.legacyPackages.${system});
    in
    {
      packages = forAllSystems (pkgs: rec {
        # cargoLock reads the lock file in tree, so no vendor hash to refresh
        # when dependencies move.
        aichat = pkgs.rustPlatform.buildRustPackage {
          pname = "aichat";
          version = (builtins.fromTOML (builtins.readFile ./Cargo.toml)).package.version;
          src = self;

          cargoLock.lockFile = ./Cargo.lock;

          nativeBuildInputs = [
            pkgs.pkg-config
            pkgs.installShellFiles
          ];

          buildInputs = [ pkgs.oniguruma ];

          postInstall = ''
            installShellCompletion --cmd aichat \
              --bash scripts/completions/aichat.bash \
              --fish scripts/completions/aichat.fish \
              --zsh scripts/completions/aichat.zsh
          '';

          meta = {
            description = "All-in-one LLM CLI tool";
            homepage = "https://github.com/etrobert/aichat";
            license = pkgs.lib.licenses.mit;
            mainProgram = "aichat";
          };
        };

        default = aichat;
      });
    };
}
