# Report format schemas

Schemas that `opcov convert` output is validated against. The Rust tests in
`../cli/convert.rs` check the structure these schemas require with a pure-Rust
XML parser, because `xmllint` is not available inside the Nix build sandbox.
Validate by hand with `xmllint` from nixpkgs `libxml2`:

```sh
nix shell nixpkgs#libxml2 -c xmllint --noout --dtdvalid https://raw.githubusercontent.com/cobertura/cobertura/master/cobertura/src/site/htdocs/xml/coverage-04.dtd out.cobertura.xml
nix shell nixpkgs#libxml2 -c xmllint --noout --schema clover-php.xsd out.clover.xml
nix shell nixpkgs#lcov -c lcov --summary out.lcov --rc branch_coverage=1
nix shell nixpkgs#lcov -c genhtml --branch-coverage -o html out.lcov
```

Cobertura's `coverage-04.dtd` is not vendored: the Cobertura repository is
GPL-2.0, so the command above fetches it from upstream.

| File | Source | Licence |
|---|---|---|
| `clover.xsd` | OpenClover, <https://raw.githubusercontent.com/openclover/clover/master/clover-core/src/main/resources/clover.xsd> (fetched 2026-11-23, unmodified) | Apache-2.0, Copyright 2002-2017 Atlassian Pty Ltd, 2017-2026 modifications by openclover.org |
| `clover-php.xsd` | Derived from `clover.xsd` above | Apache-2.0, same copyright as `clover.xsd`, modified by the opcov authors |

## `clover-php.xsd`

OpenClover's schema describes the Java dialect of Clover XML. PHP tools
(php-code-coverage's Clover report, and the consumers that read it: Codecov,
Coveralls, SonarQube's PHP plugin) use a dialect that this schema rejects, and
opcov writes that PHP dialect. `clover-php.xsd` is `clover.xsd` with exactly
these relaxations and nothing else:

- `coverage/testproject` is optional (PHP reports have no test project);
- `project` may hold `file` elements directly, next to `package`, for files in
  the global namespace;
- `package/@name`, `file/@name` and `class/@name` are free strings, because
  PHP namespaces and class names contain `\` and `file/@name` holds the path;
- a `file` may have no `class` (a file of functions);
- `class/@namespace` and `line/@name` are allowed.

Validating opcov's Clover output against the unmodified `clover.xsd` reports
only these points.
