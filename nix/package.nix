{
  inputs,
  self,
  ...
}: {
  flake.overlays.default = final: _: let
    opcov = import ./build.nix {inherit (inputs) crane rust-overlay;} final;
  in {
    opcov-cli = opcov.cli;
    mkOpcovExtension = php: opcov.extension php [];
  };

  perSystem = {
    lib,
    opcov,
    phpBuilds,
    phpOfficialImages,
    phpShell,
    phpVmBuilds,
    phpZts,
    pkgs,
    ...
  }: let
    inherit (pkgs.stdenv) hostPlatform;
    inherit (opcov) commonArgs craneLib extension phpEnv phptEnv repository version;

    justRecipe = recipe: args:
      craneLib.mkCargoDerivation (commonArgs
        // phpEnv phpShell
        // {
          src = lib.fileset.toSource {
            root = ../.;
            fileset = lib.fileset.unions [opcov.sources ../justfile];
          };
          pnameSuffix = "-${lib.head (lib.splitString " " recipe)}";
          cargoArtifacts = craneLib.buildDepsOnly (commonArgs // {CARGO_PROFILE = "dev";});
          doInstallCargoArtifacts = false;
          buildPhaseCargoCommand = "just ${recipe}";
        }
        // args
        // {nativeBuildInputs = commonArgs.nativeBuildInputs ++ [pkgs.just] ++ args.nativeBuildInputs or [];});

    suite = lib.fileset.toSource {
      root = ../.;
      fileset = lib.fileset.unions [../Cargo.toml ../justfile ../LICENSE ../tests/phpt ../crates/opcov-core/tests/golden];
    };

    phptWith = env: name: php: let
      release = (extension php []).overrideAttrs env;
      selftest = (extension php ["engine-selftest"]).overrideAttrs env;
    in
      pkgs.runCommand "opcov-phpt-${name}" (phptEnv php
        // {
          OPCOV_CLI = lib.getExe opcov.cli;
          nativeBuildInputs = [pkgs.just];
        }) ''
        cp -r --no-preserve=mode ${suite} source
        cd source
        just phpt-run ${selftest}/lib/php/extensions/opcov.so tests/phpt
        just phpt-run ${release}/lib/php/extensions/opcov.so tests/phpt/smoke
        touch "$out"
      '';

    phpt = phptWith {};

    glibcBaseline = "2.34";

    libc =
      if hostPlatform.isDarwin
      then "bsdlibc"
      else if hostPlatform.isMusl
      then "musl"
      else "glibc";

    darwinRelink = file: ''
      otool -L ${file} | tail -n +2 | while read -r library _; do
        case "$library" in
          ${file} | /usr/lib/* | /System/Library/*) ;;
          /nix/store/*/lib/libiconv.2.dylib) install_name_tool -change "$library" /usr/lib/libiconv.2.dylib ${file} ;;
          *) echo "${file} links $library, which exists only inside Nix" >&2; exit 1 ;;
        esac
      done
      codesign --force --sign - ${file}
    '';

    portability =
      if hostPlatform.isDarwin
      then ''
        install_name_tool -id opcov.so opcov.so
        ${darwinRelink "opcov.so"}
      ''
      else ''
        patchelf --print-needed opcov.so | while read -r library; do
          case "$library" in
            libc.so.6 | libm.so.6 | libgcc_s.so.1 | ld-linux-*.so.*) ;;
            *) echo "opcov.so needs $library, which a PHP installation outside Nix may lack" >&2; exit 1 ;;
          esac
        done
        glibc="$(objdump -T opcov.so | sed -n 's/.*GLIBC_\([0-9.]*\).*/\1/p' | sort -uV | tail -n 1)"
        if [ "$(printf '%s\n' "$glibc" ${glibcBaseline} | sort -V | tail -n 1)" != ${glibcBaseline} ]; then
          echo "opcov.so needs glibc $glibc, newer than the supported baseline ${glibcBaseline}" >&2
          exit 1
        fi
      '';

    dist = name: php:
      pkgs.runCommand "opcov-dist-${name}" (phpEnv php
        // {
          nativeBuildInputs =
            [pkgs.just]
            ++ lib.optionals hostPlatform.isLinux [pkgs.binutils pkgs.patchelf]
            ++ lib.optionals hostPlatform.isDarwin [pkgs.cctools pkgs.darwin.sigtool];
          suitePassed = phpt name php;
        }) ''
        test -e "$suitePassed"
        cp -r --no-preserve=mode ${suite} source
        cd source
        install -m 0644 ${extension php []}/lib/php/extensions/opcov.so opcov.so
        ${portability}
        just dist opcov.so ${version} ${libc} "$out"
      '';

    cliDist =
      pkgs.runCommand "opcov-cli-dist-${version}" {
        nativeBuildInputs = [pkgs.just pkgs.cctools pkgs.darwin.sigtool];
      } ''
        cp -r --no-preserve=mode ${suite} source
        cd source
        install -m 0755 ${lib.getExe opcov.cli} opcov
        ${darwinRelink "opcov"}
        just dist-cli ./opcov ${version} arm64-darwin-bsdlibc "$out"
      '';

    dockerImage = php: base: let
      tag = "${version}-php${lib.versions.majorMinor php.version}${lib.optionalString php.ztsSupport "-zts"}";
      root =
        pkgs.runCommand "opcov-docker-root-${tag}" {
          nativeBuildInputs = [pkgs.nukeReferences pkgs.patchelf];
          allowedReferences = [];
        } ''
          api="$(sed -n 's/^#define ZEND_MODULE_API_NO \([0-9]\{8\}\)$/\1/p' ${lib.getDev php.unwrapped}/include/php/Zend/zend_modules.h)"
          if [ -z "$api" ]; then
            echo "no ZEND_MODULE_API_NO in ${lib.getDev php.unwrapped}/include/php/Zend/zend_modules.h" >&2
            exit 1
          fi
          so="$out/usr/local/lib/php/extensions/no-debug-${
            if php.ztsSupport
            then "zts"
            else "non-zts"
          }-$api/opcov.so"
          install -D -m 0644 ${extension php []}/lib/php/extensions/opcov.so "$so"
          patchelf --remove-rpath "$so"
          nuke-refs "$so"
          mkdir -p "$out/usr/local/etc/php/conf.d"
          echo "extension=opcov" > "$out/usr/local/etc/php/conf.d/docker-php-ext-opcov.ini"
        '';
    in
      pkgs.dockerTools.buildImage {
        name = "ghcr.io/${lib.toLower (lib.removePrefix "https://github.com/" repository)}";
        inherit tag;
        copyToRoot = root;
        config.Labels = {
          "org.opencontainers.image.source" = repository;
          "org.opencontainers.image.version" = version;
          "org.opencontainers.image.licenses" = "MIT";
          "org.opencontainers.image.description" = "opcov.so laid out for the official Debian-based php:${base} image";
        };
      };

    phpWithOpcov = php: php.withExtensions ({enabled, ...}: enabled ++ [(extension php [])]);

    phpFromSource = src:
      pkgs.php86.override {
        phpSrc = src;
        version = lib.head (builtins.match ''.*#define PHP_VERSION "([^"]+)".*'' (builtins.readFile "${src}/main/php_version.h"));
      };

    dockerImages = lib.mapAttrs (name: dockerImage phpBuilds.${name}) phpOfficialImages;

    versionCheck = php: ''
      ${php} -r 'if (\opcov\version() !== $argv[1]) { fwrite(STDERR, "opcov reports " . \opcov\version() . ", expected " . $argv[1] . "\n"); exit(1); }' ${version}
    '';

    overlaid = pkgs.extend self.overlays.default;
  in {
    packages =
      {
        default = opcov.cli;
        opcov-cli = opcov.cli;
        release = pkgs.symlinkJoin {
          name = "opcov-release-${version}";
          paths =
            lib.attrValues (lib.mapAttrs dist phpBuilds)
            ++ lib.optional (hostPlatform.isDarwin && hostPlatform.isAarch64) cliDist;
        };
      }
      // lib.mapAttrs' (name: php: lib.nameValuePair "opcov-${name}" (extension php [])) phpBuilds
      // lib.mapAttrs' (name: php: lib.nameValuePair "${name}-with-opcov" (phpWithOpcov php)) phpBuilds
      // lib.optionalAttrs hostPlatform.isLinux (lib.mapAttrs' (name: lib.nameValuePair "docker-${name}") dockerImages);

    legacyPackages = {
      releaseImages =
        lib.mapAttrsToList (name: base: {
          attr = "docker-${name}";
          tag = dockerImages.${name}.imageTag;
          debian = "php:${base}";
          alpine = "php:${base}-alpine";
        })
        phpOfficialImages;
      rustupToolchain = lib.head (builtins.match "[0-9.]+-(nightly-[0-9]{4}-[0-9]{2}-[0-9]{2})" opcov.rustToolchain.version);
      phptForPhpSrc = src: let
        phptUnreleased = phptWith {OPCOV_ALLOW_UNRELEASED_PHP = "1";};
      in {
        nts = phptUnreleased "php-src-nts" (phpFromSource src);
        zts = phptUnreleased "php-src-zts" (phpZts (phpFromSource src));
      };
    };

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
        overlay = pkgs.runCommand "opcov-overlay" {} ''
          ${versionCheck (lib.getExe (phpShell.withExtensions ({enabled, ...}: enabled ++ [(overlaid.mkOpcovExtension phpShell)])))}
          touch "$out"
        '';
      }
      // lib.optionalAttrs (hostPlatform.isDarwin && hostPlatform.isAarch64) {cli-dist = cliDist;}
      // lib.mapAttrs' (name: php: lib.nameValuePair "phpt-${name}" (phpt name php)) (phpBuilds // phpVmBuilds);
  };
}
