{
  perSystem = {
    cargoEnv,
    craneLib,
    lib,
    pkgs,
    ...
  }: let
    cargoToml = lib.importTOML ../Cargo.toml;
    inherit (cargoToml.workspace.package) description repository;

    commonArgs =
      cargoEnv
      // {
        pname = "opcov";
        src = craneLib.cleanCargoSource ../.;
        strictDeps = true;
      };

    justRecipe = recipe: args:
      craneLib.mkCargoDerivation (commonArgs
        // {
          src = lib.fileset.toSource {
            root = ../.;
            fileset = lib.fileset.unions [
              (craneLib.fileset.commonCargoSources ../.)
              ../justfile
            ];
          };
          pnameSuffix = "-${lib.head (lib.splitString " " recipe)}";
          cargoArtifacts = craneLib.buildDepsOnly (commonArgs // {CARGO_PROFILE = "dev";});
          doInstallCargoArtifacts = false;
          buildPhaseCargoCommand = "just ${recipe}";
        }
        // args
        // {nativeBuildInputs = [pkgs.just] ++ args.nativeBuildInputs or [];});
  in {
    packages.default = craneLib.buildPackage (commonArgs
      // {
        cargoArtifacts = craneLib.buildDepsOnly commonArgs;
        doCheck = false;

        meta = {
          inherit description;
          homepage = repository;
          license = lib.licenses.mit;
          mainProgram = "opcov";
          platforms = lib.platforms.unix;
        };
      });

    checks = {
      lint = justRecipe "lint" {};
      deny = justRecipe "deny bans licenses sources" {nativeBuildInputs = [pkgs.cargo-deny];};
      shear = justRecipe "shear" {
        cargoArtifacts = null;
        nativeBuildInputs = [pkgs.cargo-shear];
      };
      test = justRecipe "test" {
        NEXTEST_PROFILE = "ci";
        nativeBuildInputs = [pkgs.cargo-nextest];
      };
    };
  };
}
