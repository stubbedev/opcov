#[cfg(test)]
mod golden {
    use std::collections::{BTreeMap, BTreeSet};
    use std::fs;
    use std::path::{Path, PathBuf};

    use opcov_core::cfg::{Analysis, Edge, Function, FunctionInput};
    use opcov_core::kind::EdgeKind;
    use serde::Deserialize;

    const SUPPORTED_VERSIONS: [&str; 3] = ["8.4", "8.5", "8.6"];
    const MINIMUM_FIXTURES: usize = 30;

    #[derive(Debug, Deserialize)]
    #[serde(deny_unknown_fields)]
    struct Golden {
        php: Vec<String>,
        source: String,
        input: FunctionInput,
        expected: Analysis,
    }

    fn fixture_paths() -> Vec<PathBuf> {
        let directory = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/golden/cfg");
        let mut paths: Vec<PathBuf> = fs::read_dir(&directory)
            .unwrap_or_else(|error| panic!("cannot list {}: {error}", directory.display()))
            .map(|entry| return entry.expect("directory entry").path())
            .filter(|path| {
                return path
                    .extension()
                    .is_some_and(|extension| return extension == "json");
            })
            .collect();
        paths.sort();
        return paths;
    }

    fn load(path: &Path) -> (Golden, serde_json::Value) {
        let text = fs::read_to_string(path)
            .unwrap_or_else(|error| panic!("cannot read {}: {error}", path.display()));
        let golden: Golden = serde_json::from_str(&text)
            .unwrap_or_else(|error| panic!("cannot parse {}: {error}", path.display()));
        let raw: serde_json::Value = serde_json::from_str(&text).expect("golden is JSON");
        return (golden, raw["expected"].clone());
    }

    fn fixture_name(path: &Path) -> String {
        let file = path
            .file_name()
            .and_then(|name| return name.to_str())
            .expect("UTF-8 name");
        return file.split('.').next().expect("a file name").to_owned();
    }

    fn minor(version: &str) -> String {
        return version.splitn(3, '.').take(2).collect::<Vec<_>>().join(".");
    }

    #[test]
    fn every_fixture_has_exactly_one_golden_per_supported_php_version() {
        let mut versions: BTreeMap<String, Vec<String>> = BTreeMap::new();
        for path in fixture_paths() {
            let (golden, _) = load(&path);
            versions
                .entry(fixture_name(&path))
                .or_default()
                .extend(golden.php.iter().map(|version| return minor(version)));
        }
        assert!(
            versions.len() >= MINIMUM_FIXTURES,
            "expected at least {MINIMUM_FIXTURES} fixtures, found {}",
            versions.len()
        );
        for (name, mut found) in versions {
            found.sort();
            assert_eq!(
                found, SUPPORTED_VERSIONS,
                "{name}: the goldens of a fixture cover each supported PHP version exactly once"
            );
        }
    }

    fn terminator_name(function: &Function, edge: &Edge) -> String {
        return function
            .block(edge.from)
            .and_then(|block| return block.terminator())
            .map_or_else(
                || return String::from("<no terminator>"),
                |opcode| return format!("{opcode:?}"),
            );
    }

    fn edge_drift(expected: &Function, actual: &Function) -> String {
        let wanted: BTreeSet<Edge> = expected.edges().iter().copied().collect();
        let found: BTreeSet<Edge> = actual.edges().iter().copied().collect();
        let missing = wanted.difference(&found).map(|edge| {
            return format!(
                "  missing {edge:?} from a block ending in {}",
                terminator_name(expected, edge)
            );
        });
        let unexpected = found.difference(&wanted).map(|edge| {
            return format!(
                "  unexpected {edge:?} from a block ending in {}",
                terminator_name(actual, edge)
            );
        });
        return missing.chain(unexpected).collect::<Vec<_>>().join("\n");
    }

    #[test]
    fn golden_snapshots_classify_and_round_trip() {
        for path in fixture_paths() {
            let name = path.display();
            let (golden, expected_json) = load(&path);
            assert!(
                !golden.php.is_empty(),
                "{name}: names the PHP versions it was dumped from"
            );
            assert!(
                golden.source.starts_with("<?php"),
                "{name}: source must be the PHP that was compiled"
            );

            let actual = Analysis::try_from(golden.input.clone())
                .unwrap_or_else(|error| panic!("{name}: snapshot rejected: {error}"));
            let drift = edge_drift(&golden.expected.function, &actual.function);
            assert!(drift.is_empty(), "{name}: edge kinds drifted:\n{drift}");
            assert_eq!(
                actual, golden.expected,
                "{name}: snapshot differs from the golden"
            );
            assert!(
                actual.diagnostics.is_empty(),
                "{name}: canonical shapes classify without diagnostics"
            );
            assert!(
                actual
                    .function
                    .edges()
                    .iter()
                    .all(|edge| return edge.kind != EdgeKind::Jump),
                "{name}: canonical shapes never fall back to Jump"
            );

            let serialized = serde_json::to_value(&actual).expect("snapshot serializes");
            assert_eq!(
                serialized, expected_json,
                "{name}: serialization differs from the golden JSON"
            );
            let reparsed: Analysis =
                serde_json::from_value(serialized).expect("serialized snapshot deserializes");
            assert_eq!(reparsed, actual, "{name}: serde round trip is lossless");
        }
    }

    #[test]
    fn serialization_is_deterministic() {
        for path in fixture_paths() {
            let (golden, _) = load(&path);
            let first = serde_json::to_string(
                &Analysis::try_from(golden.input.clone()).expect("valid input"),
            )
            .expect("serializes");
            let second =
                serde_json::to_string(&Analysis::try_from(golden.input).expect("valid input"))
                    .expect("serializes");
            assert_eq!(
                first,
                second,
                "{}: two analyses of the same input serialize identically",
                path.display()
            );
        }
    }
}
