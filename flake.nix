{
  inputs = {
    nixpkgs.url = "github:nixos/nixpkgs/nixpkgs-unstable";
    utils.url = "github:numtide/flake-utils";
    naersk.url = "github:nix-community/naersk";
  };

  outputs = {
    nixpkgs,
    utils,
    naersk,
    ...
  }:
    utils.lib.eachDefaultSystem (
      system: let
        name = "canvas-cli";
        pkgs = import nixpkgs {
          inherit system;
        };
        naersk-lib = naersk.lib."${system}";
        deps = with pkgs; [
          perl
        ];
      in rec {
        packages.${name} = naersk-lib.buildPackage {
          pname = "${name}";
          root = ./.;
          doCheck = true;
          copyLibs = true;
          buildInputs = deps;
            overrideMain = old: {
            postInstall = ''
              installShellCompletion --cmd canvas-cli \
                --bash <($out/bin/canvas-cli completions bash) \
                --fish <($out/bin/canvas-cli completions fish) \
                --zsh <($out/bin/canvas-cli completions zsh)
              '';
          };
          nativeBuildInputs = with pkgs; [ installShellFiles ];
        };
        packages.default = packages.${name};

        apps.${name} = utils.lib.mkApp {
          inherit name;
          drv = packages.${name};
        };
        apps.default = apps.${name};

        devShells.default = pkgs.mkShell {
          name = "${name}-devshell";
          packages = with pkgs;
            [
              rustc
              cargo
              clippy
              rustfmt
              rust-analyzer
              alejandra
              vhs
            ]
            ++ deps;

          RUST_SRC_PATH = "${pkgs.rust.packages.stable.rustPlatform.rustLibSrc}";
        };
      }
    );
}
