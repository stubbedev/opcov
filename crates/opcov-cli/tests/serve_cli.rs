#[cfg(test)]
mod serve_cli {
    use std::fs;
    use std::path::{Path, PathBuf};
    use std::process::{Child, Command, Output, Stdio};
    use std::time::{Duration, Instant};

    const BINARY: &str = env!("CARGO_BIN_EXE_opcov");
    const STARTUP: Duration = Duration::from_mins(2);
    const POLL: Duration = Duration::from_millis(50);
    const REQUEST: &str = "method App\\Services\\Report::render";

    struct Server(Child);

    impl Drop for Server {
        fn drop(&mut self) {
            if matches!(self.0.try_wait(), Ok(None)) && self.0.kill().is_ok() {
                assert!(self.0.wait().is_ok(), "opcov serve is reaped");
            }
        }
    }

    fn copy(from: &Path, to: &Path) {
        fs::create_dir_all(to).unwrap();
        for entry in fs::read_dir(from).unwrap() {
            let entry = entry.unwrap();
            let target = to.join(entry.file_name());
            if entry.file_type().unwrap().is_dir() {
                copy(&entry.path(), &target);
            } else {
                fs::copy(entry.path(), target).unwrap();
            }
        }
    }

    fn workspace() -> (PathBuf, PathBuf) {
        let base = Path::new(env!("CARGO_TARGET_TMPDIR")).join("serve");
        if base.exists() {
            fs::remove_dir_all(&base).unwrap();
        }
        let root = base.join("project");
        let runtime = base.join("run");
        fs::create_dir_all(&runtime).unwrap();
        let fixture =
            Path::new(env!("CARGO_MANIFEST_DIR")).join("../opcov-php/tests/fixtures/project");
        copy(&fixture, &root);
        return (root, runtime);
    }

    fn opcov(runtime: &Path, arguments: &[&str]) -> Output {
        return Command::new(BINARY)
            .env("XDG_RUNTIME_DIR", runtime)
            .args(arguments)
            .output()
            .unwrap();
    }

    fn consumers(runtime: &Path, root: &Path, extra: &[&str]) -> serde_json::Value {
        let root = root.to_str().unwrap();
        let mut arguments = vec![
            "consumers",
            "--root",
            root,
            "--target",
            REQUEST,
            "--format",
            "json",
        ];
        arguments.extend_from_slice(extra);
        let output = opcov(runtime, &arguments);
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        return serde_json::from_slice(&output.stdout).unwrap();
    }

    fn without_origin(mut value: serde_json::Value) -> serde_json::Value {
        let object = value.as_object_mut().unwrap();
        object.remove("served");
        object.remove("analysed");
        return value;
    }

    fn start(runtime: &Path, root: &Path) -> Server {
        let server = Command::new(BINARY)
            .env("XDG_RUNTIME_DIR", runtime)
            .args(["serve", "--root", root.to_str().unwrap()])
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .unwrap();
        let started = Instant::now();
        while fs::read_dir(runtime).unwrap().next().is_none() {
            assert!(started.elapsed() < STARTUP, "opcov serve did not listen");
            std::thread::sleep(POLL);
        }
        return Server(server);
    }

    #[test]
    fn a_running_server_answers_like_a_search_in_process_and_follows_edits() {
        let (root, runtime) = workspace();
        let mut running = start(&runtime, &root);
        let served = consumers(&runtime, &root, &[]);
        assert_eq!(served["served"], true, "the server answers");
        assert_eq!(
            without_origin(served),
            without_origin(consumers(&runtime, &root, &["--no-serve"]))
        );

        let edited = root.join("app/Uses/Receivers.php");
        let source = fs::read_to_string(&edited).unwrap();
        fs::write(
            &edited,
            source.replacen(
                "        $report->render();\n",
                "\n\n        $report->render();\n",
                1,
            ),
        )
        .unwrap();
        let after = consumers(&runtime, &root, &[]);
        assert_eq!(after["served"], true);
        assert_eq!(
            without_origin(after),
            without_origin(consumers(&runtime, &root, &["--no-serve"])),
            "the edit is seen"
        );

        let stop = opcov(
            &runtime,
            &["serve", "--root", root.to_str().unwrap(), "--stop"],
        );
        assert_eq!(
            String::from_utf8_lossy(&stop.stdout),
            "opcov serve stopped\n"
        );
        assert!(running.0.wait().unwrap().success());
        assert_eq!(
            consumers(&runtime, &root, &[])["served"],
            false,
            "without a server the search runs in process"
        );
    }
}
