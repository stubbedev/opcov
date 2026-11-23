#[cfg(test)]
mod build {
    use opcov_core::cfg::Line;
    use opcov_core::report::{
        HitSet, ModelError, PathBase, Root, Sha256, TestIndex, TestTable, line_hits,
    };

    fn line(number: u32) -> Line {
        return Line::new(number).unwrap();
    }

    fn relative(root: &str) -> PathBase {
        return PathBase::Relative(Root::new(root.to_owned()).unwrap());
    }

    #[test]
    fn compiled_paths_are_located_relative_to_the_root() {
        let base = relative("/srv/app");
        let cases = [
            ("/srv/app/src/Foo.php", "src/Foo.php"),
            (
                "/srv/app/src/x.php(3) : eval()'d code",
                "src/x.php(3) : eval()'d code",
            ),
        ];
        for (compiled, expected) in cases {
            assert_eq!(
                base.locate(compiled).unwrap().as_str(),
                expected,
                "{compiled}"
            );
        }
        assert_eq!(
            relative("/").locate("/srv/a.php").unwrap().as_str(),
            "srv/a.php",
            "the root directory"
        );
        assert_eq!(
            PathBase::Absolute.locate("/srv/a.php").unwrap().as_str(),
            "/srv/a.php",
            "absolute mode keeps the path"
        );
    }

    #[test]
    fn paths_outside_the_root_are_refused_naming_the_file() {
        let base = relative("/srv/app");
        for compiled in ["/srv/application/a.php", "/srv/app", "/other/a.php"] {
            let error = base.locate(compiled).unwrap_err();
            assert!(
                matches!(&error, ModelError::OutsideRoot { path, .. } if path == compiled),
                "{compiled}: {error}"
            );
            assert_eq!(
                error.to_string(),
                format!("measured file {compiled} lies outside the report root /srv/app"),
                "message"
            );
        }
        assert!(
            matches!(
                PathBase::Absolute.locate("rel/a.php"),
                Err(ModelError::PathModeMismatch { .. })
            ),
            "absolute mode refuses relative paths"
        );
    }

    #[test]
    fn unordered_test_names_are_sorted_and_mapped() {
        let names = ["b", "c", "a"].map(str::to_owned);
        let (table, indexes) = TestTable::from_unordered(&names).unwrap();
        assert_eq!(table.names(), ["a", "b", "c"], "sorted table");
        assert_eq!(
            indexes,
            [1, 2, 0].map(TestIndex::new),
            "each input position maps to its sorted index"
        );
        let duplicate = ["a", "a"].map(str::to_owned);
        assert!(
            TestTable::from_unordered(&duplicate).is_err(),
            "duplicates are refused"
        );
    }

    #[test]
    fn line_hits_cover_every_executable_line_with_sorted_unique_tests() {
        let hits = [
            (line(3), TestIndex::new(2)),
            (line(3), TestIndex::new(0)),
            (line(3), TestIndex::new(2)),
            (line(5), TestIndex::new(1)),
        ];
        let lines = line_hits([line(3), line(4), line(5)], hits).unwrap();
        let expected = [
            (
                line(3),
                HitSet::new(vec![TestIndex::new(0), TestIndex::new(2)]).unwrap(),
            ),
            (line(4), HitSet::default()),
            (line(5), HitSet::new(vec![TestIndex::new(1)]).unwrap()),
        ];
        assert_eq!(lines.into_iter().collect::<Vec<_>>(), expected, "lines");
        assert!(
            matches!(
                line_hits([line(3)], [(line(9), TestIndex::new(0))]),
                Err(ModelError::HitOnNonExecutableLine { .. })
            ),
            "a hit on a line without a probe site is an error"
        );
    }

    #[test]
    fn sha256_digests_bytes() {
        assert_eq!(
            Sha256::digest(b"").to_string(),
            "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855",
            "empty input"
        );
    }
}
