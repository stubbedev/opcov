#[cfg(test)]
#[path = "support/arbitrary_report.rs"]
mod arbitrary_report;

#[cfg(test)]
mod stream {
    use std::collections::BTreeMap;
    use std::io;

    use opcov_core::report::{
        FileCoverage, ReadError, Report, ReportReader, ReportWriter, SourcePath, WriteError,
        merge_streams,
    };
    use proptest::prelude::*;

    use super::arbitrary_report::{bytes, run, universe};

    const SHA: &str = "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855";

    fn document(files: &str) -> String {
        return format!(
            concat!(
                r#"{{"schema":"opcov-report/1","#,
                r#""generator":{{"opcov":"1.0.0","php":"8.5.12","zts":false,"vm":"HYBRID"}},"#,
                r#""settings":{{"collect_paths":["app"],"exclude_paths":[],"exclude_pattern":null}},"#,
                r#""paths":{{"mode":"relative","root":"/srv/app"}},"#,
                r#""tests":["T::a","T::b"],"files":{{{files}}}}}"#,
                "\n"
            ),
            files = files
        );
    }

    fn entry(path: &str, lines: &str) -> String {
        return format!(r#""{path}":{{"sha256":"{SHA}","lines":{{{lines}}},"functions":{{}}}}"#);
    }

    fn streamed(report: &Report) -> Vec<u8> {
        let mut writer = ReportWriter::begin(Vec::new(), report.header()).unwrap();
        for (path, file) in report.files() {
            writer.entry(path, file).unwrap();
        }
        return writer.finish().unwrap();
    }

    fn pulled(document: &[u8]) -> Report {
        let mut reader = ReportReader::open(document).unwrap();
        let mut files: BTreeMap<SourcePath, FileCoverage> = BTreeMap::new();
        while let Some(raw) = reader.next().transpose().unwrap() {
            let (path, file) = raw.decode(reader.header()).unwrap();
            files.insert(path, file);
        }
        return Report::new(reader.into_header(), files).unwrap();
    }

    fn spaced(document: &[u8]) -> Vec<u8> {
        let mut out = Vec::with_capacity(document.len().saturating_mul(2));
        let mut in_string = false;
        let mut escaped = false;
        for &byte in document {
            out.push(byte);
            if in_string {
                if escaped {
                    escaped = false;
                } else if byte == b'\\' {
                    escaped = true;
                } else if byte == b'"' {
                    in_string = false;
                }
                continue;
            }
            match byte {
                b'"' => in_string = true,
                b'{' | b'[' | b',' | b':' => out.extend_from_slice(b"\n\t "),
                _ => {}
            }
        }
        return out;
    }

    proptest! {
        #![proptest_config(ProptestConfig::with_cases(64))]

        #[test]
        fn the_streaming_writer_writes_the_bytes_of_the_in_memory_writer(
            universe in universe(),
            run in run(),
        ) {
            let report = universe.report(&run);
            prop_assert_eq!(streamed(&report), bytes(&report));
        }

        #[test]
        fn the_pull_reader_reads_canonical_and_pretty_documents(
            universe in universe(),
            run in run(),
        ) {
            let report = universe.report(&run);
            let canonical = bytes(&report);
            prop_assert_eq!(&pulled(&canonical), &report);
            prop_assert_eq!(&Report::read(spaced(&canonical).as_slice()).unwrap(), &report);
        }

        #[test]
        fn every_truncation_is_an_error(universe in universe(), run in run(), cut in any::<prop::sample::Index>()) {
            let canonical = bytes(&universe.report(&run));
            let end = canonical.len().saturating_sub(2);
            let at = cut.index(end);
            prop_assert!(Report::read(&canonical[..at]).is_err(), "a prefix of {} bytes is rejected", at);
        }
    }

    #[test]
    fn paths_with_structural_characters_are_read_back() {
        let tricky = [
            r#"app/a\"b}.php"#,
            r"app/back\\slash].php",
            r"app/x{[,:.php",
            r"app/été.php",
        ];
        let mut sorted: Vec<(String, String)> = tricky
            .iter()
            .map(|raw| {
                let decoded: String = serde_json::from_str(&format!("\"{raw}\"")).unwrap();
                return (decoded, (*raw).to_owned());
            })
            .collect();
        sorted.sort();
        let files: Vec<String> = sorted
            .iter()
            .map(|(_, raw)| return entry(raw, r#""3":[1]"#))
            .collect();
        let report = Report::read(document(&files.join(",")).as_bytes()).unwrap();
        let paths: Vec<&str> = report.files().keys().map(SourcePath::as_str).collect();
        let expected: Vec<&str> = sorted
            .iter()
            .map(|(path, _)| return path.as_str())
            .collect();
        assert_eq!(paths, expected, "every path is decoded");
    }

    #[test]
    fn the_header_is_read_without_the_files() {
        let mut broken = document(&entry("app/a.php", r#""3":[1]"#)).into_bytes();
        broken.truncate(broken.len().saturating_sub(10));
        let reader = ReportReader::open(broken.as_slice()).unwrap();
        assert_eq!(reader.header().tests.len(), 2, "the tests are read");
        assert!(
            Report::read(broken.as_slice()).is_err(),
            "the truncated files are rejected when read"
        );
    }

    #[test]
    fn an_invalid_entry_names_its_path() {
        let files = [
            entry("app/a.php", r#""3":[1]"#),
            entry("app/b.php", r#""3":[7]"#),
        ];
        let error = Report::read(document(&files.join(",")).as_bytes()).unwrap_err();
        let ReadError::Entry { path, .. } = &error else {
            panic!("expected an entry error, got {error}");
        };
        assert_eq!(path.as_str(), "app/b.php", "the entry is named");
        assert!(
            error.to_string().contains("app/b.php"),
            "{error} names the path"
        );
    }

    #[test]
    fn structural_errors_name_what_was_expected_and_where() {
        let cases = [
            (
                document("").replace(r#""files":{}"#, r#""files":[]"#),
                "`{` opening the files",
            ),
            (
                document(&entry("app/a.php", "")).replace("}}}\n", "}}\n"),
                "`}` closing the report",
            ),
            (
                format!("{} x", document("").trim_end()),
                "the end of the input",
            ),
            (
                document(&format!("{},", entry("app/a.php", ""))),
                "a string",
            ),
        ];
        for (document, expected) in cases {
            let error = Report::read(document.as_bytes()).unwrap_err();
            let ReadError::Syntax { .. } = &error else {
                panic!("expected a syntax error for {document}, got {error}");
            };
            assert!(
                error.to_string().contains(expected),
                "{error} names {expected}"
            );
        }
    }

    #[test]
    fn a_member_after_the_files_is_rejected() {
        let document = document("").replace("}}\n", r#"},"extra":1}"#);
        let error = Report::read(document.as_bytes()).unwrap_err();
        assert!(
            matches!(&error, ReadError::Trailing { member } if member == "extra"),
            "the extra member is named, got {error}"
        );
    }

    #[test]
    fn the_writer_refuses_unordered_and_invalid_entries() {
        let report = Report::read(document(&entry("app/a.php", r#""3":[1]"#)).as_bytes()).unwrap();
        let (path, file) = report.files().iter().next().unwrap();
        let mut writer = ReportWriter::begin(Vec::new(), report.header()).unwrap();
        writer.entry(path, file).unwrap();
        let error = writer.entry(path, file).unwrap_err();
        assert!(
            matches!(error, WriteError::Model(_)),
            "a repeated path is refused, got {error}"
        );
        let absolute = SourcePath::new("/srv/app/b.php".to_owned()).unwrap();
        let error = writer.entry(&absolute, file).unwrap_err();
        assert!(
            error.to_string().contains("/srv/app/b.php"),
            "an absolute path in relative mode is refused: {error}"
        );
        let mut beyond = file.clone();
        let line = *beyond.lines.keys().next().unwrap();
        beyond.lines.insert(
            line,
            opcov_core::report::HitSet::new(vec![opcov_core::report::TestIndex::new(2)]).unwrap(),
        );
        let next = SourcePath::new("app/c.php".to_owned()).unwrap();
        let error = writer.entry(&next, &beyond).unwrap_err();
        assert!(
            error.to_string().contains("app/c.php"),
            "a test index beyond the table is refused naming the file: {error}"
        );
    }

    #[test]
    fn a_failing_input_is_named_by_the_streaming_merge() {
        let good = document(&entry("app/a.php", r#""3":[1]"#));
        let bad = document(
            &[
                entry("app/a.php", r#""3":[0]"#),
                entry("app/b.php", r#""3":[9]"#),
            ]
            .join(","),
        );
        let inputs = [good.as_bytes(), bad.as_bytes()];
        let error =
            merge_streams(&inputs, |input| return io::Result::Ok(*input), Vec::new()).unwrap_err();
        let message = error.describe(&["good.json", "bad.json"]).to_string();
        assert!(
            message.contains("bad.json") && message.contains("app/b.php"),
            "{message} names the input and the entry"
        );
    }
}
