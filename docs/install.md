# Installing opcov

opcov is a PHP extension, `opcov.so`, plus the `opcov` command-line tool for
merging and converting reports. Every channel below installs a prebuilt
extension, so none of them needs Rust. An extension only loads into the PHP it
was built for: the same PHP minor version (8.4, 8.5, 8.6), the same thread
safety (NTS or ZTS) and a non-debug build. Pick the artifact that matches
`php -v`, which prints `NTS` or `ZTS`.

The examples use version `0.1.3`; replace it with the release you install.

| Channel | Platforms | Command |
|---|---|---|
| [PIE](#pie) | Linux (glibc, musl), macOS arm64 | `pie install opcov/opcov` |
| [GitHub release archive](#github-release-archives) | Linux (glibc, musl), macOS arm64 | download, verify, copy |
| [Docker](#docker) | official Debian-based `php` images, amd64 and arm64 | `COPY --from=ghcr.io/stubbedev/opcov:0.1.3-php8.5 / /` |
| [Nix](#nix) | Linux, macOS | `nix shell github:stubbedev/opcov#php85-with-opcov` |
| [From source](#from-source) | Linux, macOS | `cargo build --release --package opcov-ext` |

Platform requirements of the prebuilt extension:

- Linux glibc: x86_64 or arm64, glibc 2.34 or newer (Debian 12, Ubuntu 22.04,
  RHEL 9 and later). The release build fails if the extension starts needing a
  newer glibc.
- Linux musl: x86_64 or arm64, built in the official `php:*-alpine` images. It
  needs `libgcc` (`apk add libgcc`), which the official images already carry.
- macOS: arm64 (Apple silicon). It links only system libraries.
- Windows is not supported.

## PIE

[PIE](https://github.com/php/pie) 1.4.0 or newer downloads the prebuilt archive
that matches the PHP it runs for, installs it into that PHP's extension
directory and enables it:

```sh
pie install opcov/opcov
php -r 'var_dump(\opcov\version());'
```

To install for another PHP on the same machine, pass its binary:
`pie install --with-php-path=/usr/bin/php8.4 opcov/opcov`.

opcov ships prebuilt binaries only (`download-url-method` is
`pre-packaged-binary` in `composer.json`). opcov is written in Rust and has no
`config.m4`, so PIE cannot build it from source: `pie build opcov/opcov` fails by
design, and `pie install` never falls back to a source build. On a combination
that has no archive it stops with the archive names it looked for, for example
for PHP 8.6 NTS on musl:

```text
Could not download opcov/opcov using pre-packaged-binary method: Could not
find release asset for opcov/opcov:v0.1.3 named one of "php_opcov-v0.1.3_php8.6-x86_64-linux-musl.zip,
php_opcov-v0.1.3_php8.6-x86_64-linux-musl.tgz, php_opcov-v0.1.3_php8.6-x86_64-linux-musl-nts.zip,
php_opcov-v0.1.3_php8.6-x86_64-linux-musl-nts.tgz, php_opcov-v0.1.3_php8.6-x86_64-linux-anylibc.zip,
php_opcov-v0.1.3_php8.6-x86_64-linux-anylibc.tgz, php_opcov-v0.1.3_php8.6-x86_64-linux-anylibc-nts.zip,
php_opcov-v0.1.3_php8.6-x86_64-linux-anylibc-nts.tgz"
```

The combinations without an archive are PHP 8.6 NTS on musl (the official
`php:8.6-*-alpine` images are ZTS only, and the musl archives are built in
them), debug PHP builds, 32-bit systems, Intel macOS and Windows. Build those
from source instead.

Before resolving the package, PIE checks for the tools a source build would
need (a C compiler, `make`, `autoconf`, ...) and, in the slim official images,
prints `The following build tools are missing: ...`. opcov needs none of them;
the warning is harmless and `--no-build-tools-check` silences it.

PIE has no checksum check for prebuilt binaries, so HTTPS from GitHub's
release-asset host is its only integrity guarantee. To check what PIE
installed, download and verify the matching release archive as described in
[Verifying an archive](#verifying-an-archive), then compare the `opcov.so` line
of the `SHA256SUMS` inside it with the installed file:

```sh
tar -xzOf "${asset}" SHA256SUMS
sha256sum "$(php -r 'echo ini_get("extension_dir");')/opcov.so"
```

## GitHub release archives

Each [release](https://github.com/stubbedev/opcov/releases) carries one archive
per PHP minor, thread safety and platform, named as PIE expects:

```text
php_opcov-v<version>_php<minor>-<arch>-<os>-<libc>-<nts|zts>.tgz
```

| Part | Values |
|---|---|
| `<minor>` | `8.4`, `8.5`, `8.6` |
| `<arch>` | `x86_64`, `arm64` |
| `<os>-<libc>` | `linux-glibc`, `linux-musl`, `darwin-bsdlibc` |

Each archive holds `opcov.so`, `LICENSE` and `SHA256SUMS` (the checksums of
those two files). The release's own `SHA256SUMS` lists every archive, and every
archive has a build-provenance attestation that ties it to the workflow run and
commit that built it.

```sh
version=0.1.3
asset="php_opcov-v${version}_php8.5-x86_64-linux-glibc-nts.tgz"
base="https://github.com/stubbedev/opcov/releases/download/v${version}"
curl -fsSLO "${base}/${asset}"
curl -fsSLO "${base}/SHA256SUMS"
sha256sum --check --ignore-missing SHA256SUMS
gh attestation verify "${asset}" --repo stubbedev/opcov
tar -xzf "${asset}" opcov.so
sudo install -m 0644 opcov.so "$(php -r 'echo ini_get("extension_dir");')/opcov.so"
echo 'extension=opcov' | sudo tee "$(php --ini | sed -n 's/^Scan for additional .ini files in: //p')/20-opcov.ini"
php -r 'var_dump(\opcov\version());'
```

On macOS use `shasum -a 256 --check --ignore-missing SHA256SUMS`. A file
downloaded with a browser carries the quarantine attribute; clear it with
`xattr -d com.apple.quarantine opcov.so` before PHP can load it. When PHP has no
scan directory (`php --ini` prints `(none)`), add `extension=opcov` to the
`php.ini` it names instead.

### Verifying an archive

The checksums only prove that the download matches the release. The
build-provenance attestation proves where the archive came from:
`gh attestation verify` looks up the attestation for the file's SHA-256
digest, checks its Sigstore signature and that it was issued to
`stubbedev/opcov`, and exits non-zero otherwise. To also pin the workflow and
the tag that built it:

```sh
gh attestation verify "${asset}" --repo stubbedev/opcov \
  --signer-workflow stubbedev/opcov/.github/workflows/release.yml \
  --source-ref "refs/tags/v${version}"
```

A file that no opcov release built fails with
`Error: HTTP 404: Not Found (https://api.github.com/repos/stubbedev/opcov/attestations/sha256:...)`.

## Docker

`ghcr.io/stubbedev/opcov` holds images with nothing but `opcov.so`, laid out
for the official Debian-based [`php`](https://hub.docker.com/_/php) images: the
extension sits in that image's extension directory and
`/usr/local/etc/php/conf.d/docker-php-ext-opcov.ini` enables it, as
`docker-php-ext-enable` would. Every image is published for `linux/amd64` and
`linux/arm64`.

```dockerfile
FROM php:8.5-cli
COPY --from=ghcr.io/stubbedev/opcov:0.1.3-php8.5 / /
```

| Official image (and its `fpm`, `apache`, `bookworm`, `trixie` siblings) | opcov image |
|---|---|
| `php:8.4-cli` | `ghcr.io/stubbedev/opcov:0.1.3-php8.4` |
| `php:8.4-zts` | `ghcr.io/stubbedev/opcov:0.1.3-php8.4-zts` |
| `php:8.5-cli` | `ghcr.io/stubbedev/opcov:0.1.3-php8.5` |
| `php:8.5-zts` | `ghcr.io/stubbedev/opcov:0.1.3-php8.5-zts` |
| `php:8.6-cli` | `ghcr.io/stubbedev/opcov:0.1.3-php8.6-zts` |

The official PHP 8.6 images are thread-safe (ZTS) and there is no NTS 8.6
image, hence `-php8.6-zts` for `php:8.6-cli`. To copy the extension without
enabling it, copy the file alone, for example
`COPY --from=ghcr.io/stubbedev/opcov:0.1.3-php8.5 /usr/local/lib/php/extensions/no-debug-non-zts-20250925/opcov.so /usr/local/lib/php/extensions/no-debug-non-zts-20250925/`.

For the Alpine images, install the musl archive:

```dockerfile
FROM php:8.5-cli-alpine
ARG OPCOV_VERSION=0.1.3
RUN arch="$(uname -m | sed 's/aarch64/arm64/')" \
    && ts="$(php -r 'echo PHP_ZTS ? "zts" : "nts";')" \
    && minor="$(php -r 'echo PHP_MAJOR_VERSION, ".", PHP_MINOR_VERSION;')" \
    && curl -fsSL "https://github.com/stubbedev/opcov/releases/download/v${OPCOV_VERSION}/php_opcov-v${OPCOV_VERSION}_php${minor}-${arch}-linux-musl-${ts}.tgz" \
       | tar -xz -C "$(php -r 'echo ini_get("extension_dir");')" opcov.so \
    && docker-php-ext-enable opcov
```

## Nix

The flake builds the extension against each nixpkgs PHP it pins and exposes:

| Output | What it is |
|---|---|
| `packages.<system>.opcov-php84`, `-php84-zts`, `-php85`, `-php85-zts`, `-php86`, `-php86-zts` | the extension, at `lib/php/extensions/opcov.so` |
| `packages.<system>.php84-with-opcov`, ... `php86-zts-with-opcov` | that PHP with its default extensions plus opcov enabled |
| `packages.<system>.opcov-cli` (also `default`) | the `opcov` command |
| `overlays.default` | adds `opcov-cli` and `mkOpcovExtension` to your nixpkgs |

```sh
nix shell github:stubbedev/opcov#php85-with-opcov --command php -r 'var_dump(\opcov\version());'
nix run github:stubbedev/opcov -- merge worker-*.json -o coverage.json
```

The extension packages carry `extensionName = "opcov"`, so they work with
`php.withExtensions`, but only for the exact PHP they were built against. For a
PHP from your own nixpkgs, build opcov against it through the overlay:

```nix
{
  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";
    opcov.url = "github:stubbedev/opcov";
  };

  outputs = {nixpkgs, opcov, ...}: let
    pkgs = import nixpkgs {
      system = "x86_64-linux";
      overlays = [opcov.overlays.default];
    };
  in {
    devShells.x86_64-linux.default = pkgs.mkShell {
      packages = [
        (pkgs.php85.withExtensions ({enabled, ...}: enabled ++ [(pkgs.mkOpcovExtension pkgs.php85)]))
        pkgs.opcov-cli
      ];
    };
  };
}
```

`mkOpcovExtension` takes a PHP package as nixpkgs provides it (the wrapped
`php85`, or one overridden with `php85.override {ztsSupport = true;}`).

## From source

Building needs nightly Rust, libclang and the headers of the target PHP
(`php-config`). The flake's dev shell provides all of them; see
[Building the extension](contributing.md#building-the-extension):

```sh
nix develop
PHP_CONFIG=/path/to/php-config cargo build --release --locked --package opcov-ext
```

The result is `target/release/libopcov_ext.so` (`.dylib` on macOS); install it
as `opcov.so` like a release archive. Building for musl additionally needs
`RUSTFLAGS="-C target-feature=-crt-static"`, without which Rust drops the
`cdylib`; `docker/musl.Dockerfile` does exactly this inside an official Alpine
image.

## The `opcov` command

The `opcov` command-line tool (`merge`, `convert`, `gate`, `impact`, `doctor`;
[cli.md](cli.md)) is a separate native binary. Every release carries it,
statically linked on Linux:

| Asset | Platform |
|---|---|
| `opcov-cli-v<version>-x86_64-linux-musl` | x86_64 Linux, any libc |
| `opcov-cli-v<version>-arm64-linux-musl` | aarch64 Linux, any libc |
| `opcov-cli-v<version>-arm64-darwin-bsdlibc` | macOS arm64 |

Each is listed in the release's `SHA256SUMS` and has a build-provenance
attestation, like the extension archives.

**With Composer.** `composer require --dev opcov/phpunit` installs
`vendor/bin/opcov`. The package is a Composer plugin: after `composer install`
and `composer update` it downloads the binary for the host from the release
that matches the package version, checks it against that release's
`SHA256SUMS`, keeps it in the user cache directory (`$XDG_CACHE_HOME/opcov`,
`~/.cache/opcov`, or `~/Library/Caches/opcov` on macOS) and places it in the
package's `bin/`. Composer asks once whether to trust the plugin; to allow it
without asking, add it to `allow-plugins`:

```sh
composer config allow-plugins.opcov/phpunit true
```

The plugin is optional: when it is not allowed, or the download fails, the
install still succeeds, and `vendor/bin/opcov` downloads the binary on its
first run instead. `vendor/bin/opcov` then replaces itself with the binary
(through `ext-pcntl`, or as a child process with the same standard streams), so
arguments, output and exit codes are the binary's. On a platform without a
prebuilt binary it fails with the platforms that have one and the
source-build command.

**Without Composer.** Download the asset and verify it as in
[Verifying an archive](#verifying-an-archive), or use the flake's `opcov-cli`
package ([Nix](#nix)), or build it from source:
`cargo install --locked --git https://github.com/stubbedev/opcov opcov-cli`.
