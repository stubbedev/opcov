{
  crane,
  rust-overlay,
}: pkgs: let
  inherit (pkgs) lib;
  inherit (pkgs.stdenv) hostPlatform;
  inherit ((lib.importTOML ../Cargo.toml).workspace.package) description repository version;

  rustToolchain = (rust-overlay.lib.mkRustBin {} pkgs).selectLatestNightlyWith (toolchain:
    toolchain.default.override {
      extensions = [
        "rust-analyzer"
        "rust-src"
      ];
    });

  craneLib = ((crane.mkLib pkgs).overrideToolchain rustToolchain).overrideScope (_: _: {
    stdenvSelector = p:
      if p.stdenv.hostPlatform.isLinux
      then p.stdenvAdapters.useMoldLinker p.clangStdenv
      else p.clangStdenv;
  });

  cargoEnv = {
    CARGO_BUILD_RUSTFLAGS = "-Zthreads=8";
  };

  phpEnv = php: {
    PHP_CONFIG = "${lib.getDev php.unwrapped}/bin/php-config";
    PHP_RUN_TESTS = "${lib.getDev php.unwrapped}/lib/build/run-tests.php";
  };

  phptEnv = php:
    phpEnv php
    // {
      OPCOV_PCNTL_EXTENSION = "${php.extensions.pcntl}/lib/php/extensions/pcntl.so";
    }
    // lib.optionalAttrs (!php.extensions.pcov.meta.broken) {
      OPCOV_PCOV_EXTENSION = "${php.extensions.pcov}/lib/php/extensions/pcov.so";
    };

  sources = lib.fileset.unions [
    (craneLib.fileset.commonCargoSources ../.)
    ../crates
    (lib.fileset.maybeMissing ../schema)
  ];

  commonArgs =
    cargoEnv
    // {
      pname = "opcov";
      inherit version;
      src = lib.fileset.toSource {
        root = ../.;
        fileset = sources;
      };
      strictDeps = true;
      nativeBuildInputs = [pkgs.rustPlatform.bindgenHook];
    };

  cargoArtifacts = craneLib.buildDepsOnly commonArgs;

  meta = {
    homepage = repository;
    license = lib.licenses.mit;
    platforms = lib.platforms.unix;
  };

  extension = php: features:
    craneLib.buildPackage (commonArgs
      // phpEnv php
      // {
        pname = "opcov-ext";
        inherit cargoArtifacts;
        cargoExtraArgs = lib.concatStringsSep " " (["--locked" "--package" "opcov-ext"] ++ map (feature: "--features ${feature}") features);
        doCheck = false;
        postInstall = ''
          mkdir -p "$out/lib/php/extensions"
          mv "$out/lib/libopcov_ext${hostPlatform.extensions.sharedLibrary}" "$out/lib/php/extensions/opcov.so"
        '';
        passthru.extensionName = "opcov";
        meta =
          meta
          // {
            description = "${description} (PHP extension for PHP ${php.version}${lib.optionalString php.ztsSupport " ZTS"})";
          };
      });

  cli = craneLib.buildPackage (commonArgs
    // {
      pname = "opcov-cli";
      inherit cargoArtifacts;
      cargoExtraArgs = "--locked --package opcov-cli";
      doCheck = false;
      meta =
        meta
        // {
          inherit description;
          mainProgram = "opcov";
        };
    });
in {
  inherit cargoArtifacts cargoEnv cli commonArgs craneLib extension phpEnv phptEnv repository rustToolchain sources version;
}
