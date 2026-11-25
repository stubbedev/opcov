#[cfg(test)]
#[path = "support/arbitrary_report.rs"]
mod arbitrary_report;

#[cfg(test)]
mod stitch {
    use std::collections::BTreeSet;

    use opcov_core::report::{
        HeaderField, MergeError, Report, Sha256, SourcePath, StreamMergeError, merge,
        stitch_streams,
    };
    use proptest::prelude::*;

    use super::arbitrary_report::{Run, bytes, run, universe};

    fn stitched(
        baseline: &Report,
        partial: &Report,
        replaced: u8,
        changed: &BTreeSet<SourcePath>,
    ) -> Result<Vec<u8>, StreamMergeError> {
        let names = baseline.header().tests.names().to_vec();
        let replaced = move |name: &str| {
            return names
                .iter()
                .position(|known| return known == name)
                .and_then(|position| return u32::try_from(position).ok())
                .and_then(|position| return 1_u8.checked_shl(position))
                .is_some_and(|flag| return replaced & flag != 0);
        };
        return stitch_streams(
            bytes(baseline).as_slice(),
            bytes(partial).as_slice(),
            replaced,
            |path| return changed.contains(path),
            Vec::new(),
        );
    }

    fn select(tests: u8, files: u8, pattern: &[u8]) -> Run {
        return Run {
            tests,
            files,
            pattern: pattern.to_vec(),
        };
    }

    fn changed(report: &Report, mask: u8) -> (Report, BTreeSet<SourcePath>) {
        let mut paths = BTreeSet::new();
        let mut files = report.files().clone();
        for (position, (path, file)) in files.iter_mut().enumerate() {
            let flag = u32::try_from(position)
                .ok()
                .and_then(|shift| return 1_u8.checked_shl(shift));
            if flag.is_some_and(|flag| return mask & flag != 0) {
                let mut digest = *file.sha256.as_bytes();
                digest[0] ^= 1;
                file.sha256 = Sha256::from_bytes(digest);
                paths.insert(path.clone());
            }
        }
        return (Report::new(report.header().clone(), files).unwrap(), paths);
    }

    fn reached(report: &Report, paths: &BTreeSet<SourcePath>) -> u8 {
        let mut mask = 0_u8;
        for (path, file) in report.files() {
            if !paths.contains(path) {
                continue;
            }
            for hits in file.hit_sets() {
                for index in hits.as_slice() {
                    mask |= 1_u8.checked_shl(index.get()).unwrap();
                }
            }
        }
        return mask;
    }

    proptest! {
        #![proptest_config(ProptestConfig::with_cases(64))]

        #[test]
        fn a_rerun_over_a_stale_baseline_gives_the_full_run(
            universe in universe(),
            rerun in any::<u8>(),
            removed in any::<u8>(),
            files in any::<u8>(),
            pattern in prop::collection::vec(any::<u8>(), 1..16),
            stale in run(),
        ) {
            let baseline = merge(&[
                universe.report(&select(!rerun, files, &pattern)),
                universe.report(&Run { tests: rerun, files, ..stale }),
            ])
            .unwrap();
            let partial = universe.report(&select(rerun, files, &pattern));
            let full = universe.report(&select(!removed | rerun, files, &pattern));
            let result = stitched(&baseline, &partial, removed, &BTreeSet::new()).unwrap();
            prop_assert_eq!(result, bytes(&full));
        }

        #[test]
        fn changed_files_take_the_rerun_once_every_test_that_ran_them_reruns(
            universe in universe(),
            changes in any::<u8>(),
            extra in any::<u8>(),
            pattern in prop::collection::vec(any::<u8>(), 1..16),
        ) {
            let baseline = universe.report(&select(u8::MAX, u8::MAX, &pattern));
            let (full, paths) = changed(&baseline, changes);
            let needed = reached(&baseline, &paths);
            let rerun = extra | needed;
            let (partial, _) = changed(&universe.report(&select(rerun, u8::MAX, &pattern)), changes);
            let result = stitched(&baseline, &partial, 0, &paths).unwrap();
            prop_assert_eq!(result, bytes(&full));
            if needed & !extra != 0 {
                let (partial, _) =
                    changed(&universe.report(&select(extra & !needed, u8::MAX, &pattern)), changes);
                let error = stitched(&baseline, &partial, 0, &paths).unwrap_err();
                prop_assert!(
                    matches!(&error, StreamMergeError::Merge(MergeError::Stale(_))),
                    "a changed file run by a test that is not rerun is refused: {}",
                    error
                );
            }
        }
    }

    const HEADER: &str = concat!(
        r#"{"schema":"opcov-report/1","#,
        r#""generator":{"opcov":"1.0.0","php":"8.5.12","zts":true,"vm":"HYBRID"},"#,
        r#""settings":{"collect_paths":["app"],"exclude_paths":[],"exclude_pattern":null},"#,
        r#""paths":{"mode":"relative","root":"/srv/app"},"#
    );

    fn document(tests: &str, files: &str) -> String {
        return format!(r#"{HEADER}"tests":{tests},"files":{{{files}}}}}"#);
    }

    fn entry(sha: char, lines: &str) -> String {
        let sha256: String = std::iter::repeat_n(sha, 64).collect();
        return format!(
            r#""app/Foo.php":{{"sha256":"{sha256}","lines":{{"12":{lines}}},"functions":{{}}}}"#
        );
    }

    #[test]
    fn a_stale_changed_file_is_refused_naming_file_test_and_reports() {
        let baseline = document(r#"["T::a","T::b"]"#, &entry('a', "[0,1]"));
        let partial = document(r#"["T::a"]"#, &entry('b', "[0]"));
        let changed = SourcePath::new("app/Foo.php".to_owned()).unwrap();
        let error = stitch_streams(
            baseline.as_bytes(),
            partial.as_bytes(),
            |_| return false,
            |path| return *path == changed,
            Vec::new(),
        )
        .unwrap_err();
        assert_eq!(
            error
                .describe(&["baseline.json", "partial.json"])
                .to_string(),
            "refusing to stitch app/Foo.php: it changed, but T::b ran it in baseline.json and is not rerun in partial.json",
            "the message names the file, the test and both reports"
        );
        assert!(error.is_refusal(), "a stale file is a refusal");
    }

    #[test]
    fn an_unchanged_file_with_another_hash_is_refused() {
        let baseline = document(r#"["T::a","T::b"]"#, &entry('a', "[0,1]"));
        let partial = document(r#"["T::a"]"#, &entry('b', "[0]"));
        let error = stitch_streams(
            baseline.as_bytes(),
            partial.as_bytes(),
            |_| return false,
            |_| return false,
            Vec::new(),
        )
        .unwrap_err();
        assert!(
            matches!(&error, StreamMergeError::Merge(MergeError::Conflict(_))),
            "the partial run measured another version of an unchanged file: {error}"
        );
    }

    #[test]
    fn differing_headers_are_refused() {
        let baseline = document("[]", "");
        let partial = baseline.replace(r#""php":"8.5.12""#, r#""php":"8.5.13""#);
        let error = stitch_streams(
            baseline.as_bytes(),
            partial.as_bytes(),
            |_| return false,
            |_| return false,
            Vec::new(),
        )
        .unwrap_err();
        let StreamMergeError::Merge(MergeError::Header(mismatch)) = &error else {
            panic!("expected a header mismatch, got {error}");
        };
        assert_eq!(
            mismatch.field,
            HeaderField::GeneratorPhp,
            "the field is named"
        );
    }
}
