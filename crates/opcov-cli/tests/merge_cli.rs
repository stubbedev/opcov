#[cfg(test)]
mod merge_command {
    use std::fs;
    use std::path::{Path, PathBuf};
    use std::process::{Command, Output};

    const BINARY: &str = env!("CARGO_BIN_EXE_opcov");

    fn report(test: &str, line_hits: &str, version: &str) -> String {
        return [
            r#"{"schema":"opcov-report/1","#,
            r#""generator":{"opcov":""#,
            version,
            r#"","php":"8.5.12","zts":true,"vm":"HYBRID"},"#,
            r#""settings":{"collect_paths":["app"],"exclude_paths":[],"exclude_pattern":null,"strict":true},"#,
            r#""paths":{"mode":"relative","root":"/srv/app"},"#,
            r#""tests":[""#,
            test,
            r#""],"files":{"app/Foo.php":{"#,
            r#""sha256":"e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855","#,
            r#""lines":{"12":"#,
            line_hits,
            r#","13":[]},"functions":{}}}}"#,
            "\n",
        ]
        .concat();
    }

    fn workspace(name: &str) -> PathBuf {
        let directory = Path::new(env!("CARGO_TARGET_TMPDIR")).join(name);
        if let Err(error) = fs::remove_dir_all(&directory)
            && error.kind() != std::io::ErrorKind::NotFound
        {
            panic!("cannot clear {}: {error}", directory.display());
        }
        fs::create_dir_all(&directory).unwrap();
        return directory;
    }

    fn run(arguments: &[&Path]) -> Output {
        return Command::new(BINARY)
            .arg("merge")
            .args(arguments)
            .output()
            .unwrap();
    }

    fn stderr(output: &Output) -> String {
        return String::from_utf8(output.stderr.clone()).unwrap();
    }

    #[test]
    fn merges_reports_into_the_output_atomically() {
        let directory = workspace("merges_reports");
        let first = directory.join("a.json");
        let second = directory.join("b.json");
        let out = directory.join("out.json");
        fs::write(&first, report("ATest::test_a", "[0]", "1.0.0")).unwrap();
        fs::write(&second, report("BTest::test_b", "[0]", "1.0.0")).unwrap();
        let output = run(&[&second, &first, Path::new("-o"), &out]);
        assert_eq!(
            output.status.code(),
            Some(0),
            "merge succeeds: {}",
            stderr(&output)
        );
        let merged = fs::read_to_string(&out).unwrap();
        let expected = report("ATest::test_a\",\"BTest::test_b", "[0,1]", "1.0.0");
        assert_eq!(
            merged, expected,
            "tests are deduplicated, re-indexed and sorted"
        );
        let leftovers: Vec<_> = fs::read_dir(&directory)
            .unwrap()
            .map(|entry| return entry.unwrap().file_name())
            .filter(|name| return name.to_string_lossy().ends_with(".tmp"))
            .collect();
        assert!(
            leftovers.is_empty(),
            "no temporary file is left: {leftovers:?}"
        );
    }

    #[test]
    fn refuses_mismatching_versions_with_exit_code_one() {
        let directory = workspace("refuses_versions");
        let first = directory.join("a.json");
        let second = directory.join("b.json");
        let out = directory.join("out.json");
        fs::write(&first, report("ATest::test_a", "[0]", "1.0.0")).unwrap();
        fs::write(&second, report("BTest::test_b", "[0]", "1.1.0")).unwrap();
        let output = run(&[&first, &second, Path::new("-o"), &out]);
        assert_eq!(output.status.code(), Some(1), "a mismatch is a refusal");
        let message = stderr(&output);
        assert!(
            message.contains("generator.opcov"),
            "{message} names the field"
        );
        assert!(
            message.contains(r#""1.0.0""#) && message.contains(r#""1.1.0""#),
            "{message} names both values"
        );
        assert!(
            message.contains(&second.display().to_string()),
            "{message} names the input"
        );
        assert!(!out.exists(), "no output is written on refusal");
    }

    #[test]
    fn reports_a_missing_input_with_exit_code_two() {
        let directory = workspace("missing_input");
        let missing = directory.join("missing.json");
        let out = directory.join("out.json");
        let output = run(&[&missing, Path::new("-o"), &out]);
        assert_eq!(
            output.status.code(),
            Some(2),
            "an unreadable input is an IO error"
        );
        let message = stderr(&output);
        assert!(
            message.contains(&missing.display().to_string()),
            "{message} names the input"
        );
    }

    #[test]
    fn rejects_usage_errors_with_exit_code_two() {
        let output = Command::new(BINARY).arg("merge").output().unwrap();
        assert_eq!(
            output.status.code(),
            Some(2),
            "missing arguments are a usage error"
        );
        assert!(!stderr(&output).is_empty(), "the usage error is explained");
    }
}
