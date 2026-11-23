# Versioning and support policy

## What is versioned

opcov has three version numbers, each following
[Semantic Versioning 2.0.0](https://semver.org/spec/v2.0.0.html) and each moving
independently:

| Artifact | Version | Where it is defined |
|---|---|---|
| The extension (`opcov.so`) and the `opcov` CLI | `<major>.<minor>.<patch>`, one number for both | `[workspace.package] version` in `Cargo.toml`; a release is the tag `v<version>` |
| The PHP package `opcov/phpunit` (PHPUnit and Pest integration) | its own `<major>.<minor>.<patch>` | not published yet (#9) |
| The report schema | `opcov-report/<major>` | `docs/report-schema.md`, section "Version policy" |

For the extension and the CLI, a major version changes when an existing user
has to change something: a `\opcov\` function or its result shape, an INI
setting, a CLI flag or exit code, or the oldest report schema the CLI reads. A
minor version adds features, new PHP minors or platforms. A patch fixes bugs.
Removing a PHP minor that php.net no longer supports happens in a minor release.
Before 1.0.0, a minor release may also break.

The report schema records only its major version; additive changes keep it, and
readers reject a major version they do not know. The schema version does not
follow the extension's version. `docs/report-schema.md` holds the details.

The extension and the PHP package will refuse each other across major
versions. That handshake needs the PHP package (#9) and is not implemented yet.

## Supported PHP versions

opcov supports the PHP versions that php.net supports, in active support or
security support (see [php.net/supported-versions](https://www.php.net/supported-versions.php)),
both NTS and ZTS, non-debug builds.

| PHP | Supported by opcov | php.net support ends |
|---|---|---|
| 8.4 | yes | 31 Dec 2028 |
| 8.5 | yes | 31 Dec 2029 |
| 8.6 | yes | 31 Dec 2030 |

- A new PHP minor is added at its first release candidate (RC1): opcov builds
  and passes its tests against it from then on, and the first opcov release
  after its general availability ships artifacts for it.
- A PHP minor is dropped in the first opcov minor release after php.net ends
  its security support.
- php-src `master` is built every night (`.github/workflows/php-src-nightly.yml`);
  a failure opens an issue titled "Nightly build against php-src master fails",
  or comments on the open one.

## Supported platforms

| Platform | Artifacts |
|---|---|
| Linux x86_64 and arm64, glibc 2.34 or newer | release archives, PIE, Docker images, Nix |
| Linux x86_64 and arm64, musl (Alpine) | release archives, PIE |
| macOS arm64 | release archives, PIE, Nix |

Windows is not supported. `docs/install.md` lists every channel.

## Release notes

Each release's notes are its section in `CHANGELOG.md`, which is written by
hand when a change lands, plus the PHP versions it was built for and the report
schema version it writes.
