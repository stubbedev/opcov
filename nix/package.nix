{
  perSystem = {
    cargoEnv,
    craneLib,
    lib,
    phpBuilds,
    phpDefault,
    phpVmBuilds,
    phpEnv,
    pkgs,
    ...
  }: let
    cargoToml = lib.importTOML ../Cargo.toml;
    inherit (cargoToml.workspace.package) description repository;

    sources = lib.fileset.unions [
      (craneLib.fileset.commonCargoSources ../.)
      ../crates
      (lib.fileset.maybeMissing ../schema)
    ];

    commonArgs =
      cargoEnv
      // phpEnv phpDefault
      // {
        pname = "opcov";
        src = lib.fileset.toSource {
          root = ../.;
          fileset = sources;
        };
        strictDeps = true;
        nativeBuildInputs = [pkgs.rustPlatform.bindgenHook];
      };

    releaseArtifacts = craneLib.buildDepsOnly commonArgs;

    justRecipe = recipe: args:
      craneLib.mkCargoDerivation (commonArgs
        // {
          src = lib.fileset.toSource {
            root = ../.;
            fileset = lib.fileset.unions [sources ../justfile];
          };
          pnameSuffix = "-${lib.head (lib.splitString " " recipe)}";
          cargoArtifacts = craneLib.buildDepsOnly (commonArgs // {CARGO_PROFILE = "dev";});
          doInstallCargoArtifacts = false;
          buildPhaseCargoCommand = "just ${recipe}";
        }
        // args
        // {nativeBuildInputs = commonArgs.nativeBuildInputs ++ [pkgs.just] ++ args.nativeBuildInputs or [];});

    extension = php: features:
      craneLib.buildPackage (commonArgs
        // phpEnv php
        // {
          pname = "opcov-ext";
          cargoArtifacts = releaseArtifacts;
          cargoExtraArgs = lib.concatStringsSep " " (["--locked" "--package" "opcov-ext"] ++ map (feature: "--features ${feature}") features);
          doCheck = false;
          postInstall = ''
            mkdir -p "$out/lib/php/extensions"
            mv "$out/lib/libopcov_ext${pkgs.stdenv.hostPlatform.extensions.sharedLibrary}" "$out/lib/php/extensions/opcov.so"
          '';
          meta = {
            description = "${description} (PHP extension for PHP ${php.version}${lib.optionalString php.ztsSupport " ZTS"})";
            homepage = repository;
            license = lib.licenses.mit;
            platforms = lib.platforms.unix;
          };
        });

    phpt = name: php: let
      release = extension php [];
      selftest = extension php ["engine-selftest"];
      suite = lib.fileset.toSource {
        root = ../.;
        fileset = lib.fileset.unions [../Cargo.toml ../justfile ../tests/phpt];
      };
    in
      pkgs.runCommand "opcov-phpt-${name}" (phpEnv php // {nativeBuildInputs = [pkgs.just];}) ''
        cp -r --no-preserve=mode ${suite} source
        cd source
        just phpt-run ${selftest}/lib/php/extensions/opcov.so tests/phpt
        just phpt-run ${release}/lib/php/extensions/opcov.so tests/phpt/smoke
        touch "$out"
      '';
  in {
    packages =
      {
        default = craneLib.buildPackage (commonArgs
          // {
            cargoArtifacts = releaseArtifacts;
            cargoExtraArgs = "--locked --package opcov-cli";
            doCheck = false;

            meta = {
              inherit description;
              homepage = repository;
              license = lib.licenses.mit;
              mainProgram = "opcov";
              platforms = lib.platforms.unix;
            };
          });
      }
      // lib.mapAttrs' (name: php: lib.nameValuePair "opcov-${name}" (extension php [])) phpBuilds;

    checks =
      {
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
      }
      // lib.mapAttrs' (name: php: lib.nameValuePair "phpt-${name}" (phpt name php)) (phpBuilds // phpVmBuilds);
  };
}
