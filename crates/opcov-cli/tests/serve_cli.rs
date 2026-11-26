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

    fn workspace(name: &str) -> (PathBuf, PathBuf) {
        let base = Path::new(env!("CARGO_TARGET_TMPDIR")).join(format!("serve-{name}"));
        if base.exists() {
            fs::remove_dir_all(&base).unwrap();
        }
        let root = base.join("project");
        let runtime = std::env::temp_dir().join(format!("opcov-{name}-{}", std::process::id()));
        if runtime.exists() {
            fs::remove_dir_all(&runtime).unwrap();
        }
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

    fn start(runtime: &Path, root: &Path, log: Stdio) -> Server {
        let mut server = Command::new(BINARY)
            .env("XDG_RUNTIME_DIR", runtime)
            .args(["serve", "--root", root.to_str().unwrap()])
            .stdout(Stdio::null())
            .stderr(log)
            .spawn()
            .unwrap();
        let started = Instant::now();
        while fs::read_dir(runtime).unwrap().next().is_none() {
            assert!(started.elapsed() < STARTUP, "opcov serve did not listen");
            if let Some(status) = server.try_wait().unwrap() {
                panic!("opcov serve exited before it listened: {status}");
            }
            std::thread::sleep(POLL);
        }
        return Server(server);
    }

    #[test]
    fn a_running_server_answers_like_a_search_in_process_and_follows_edits() {
        let (root, runtime) = workspace("edits");
        let mut running = start(&runtime, &root, Stdio::null());
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
        fs::remove_dir_all(&runtime).unwrap();
    }

    fn wait_for(log: &Path, text: &str) {
        let started = Instant::now();
        while !fs::read_to_string(log).unwrap_or_default().contains(text) {
            assert!(
                started.elapsed() < STARTUP,
                "opcov serve never logged {text}"
            );
            std::thread::sleep(POLL);
        }
    }

    #[test]
    fn reading_or_touching_files_through_a_linked_root_neither_rebuilds_nor_updates_the_index() {
        let (project, runtime) = workspace("reads");
        let root = project.with_file_name("linked");
        std::os::unix::fs::symlink(&project, &root).unwrap();
        let log = project.with_file_name("serve.log");
        let mut running = start(
            &runtime,
            &root,
            Stdio::from(fs::File::create(&log).unwrap()),
        );
        wait_for(&log, "analysed");
        for path in [
            "composer.json",
            "vendor/composer/autoload_psr4.php",
            "app/Uses/Receivers.php",
        ] {
            fs::read(root.join(path)).unwrap();
        }
        fs::File::options()
            .write(true)
            .open(root.join("composer.json"))
            .unwrap()
            .set_modified(std::time::SystemTime::now())
            .unwrap();
        assert_eq!(consumers(&runtime, &root, &[])["served"], true);
        std::thread::sleep(Duration::from_secs(1));
        let logged = fs::read_to_string(&log).unwrap();
        assert!(
            !logged.contains("rebuilt") && !logged.contains("updated"),
            "{logged}"
        );
        opcov(
            &runtime,
            &["serve", "--root", root.to_str().unwrap(), "--stop"],
        );
        assert!(running.0.wait().unwrap().success());
        fs::remove_dir_all(&runtime).unwrap();
    }

    #[test]
    fn a_search_in_process_reuses_its_index_cache_and_rebuilds_a_corrupt_one() {
        let (root, runtime) = workspace("cache");
        let cache = root.join(".opcov/index.cache");
        let cold = consumers(&runtime, &root, &["--no-serve"]);
        let written = fs::read(&cache).unwrap();
        assert_eq!(
            consumers(&runtime, &root, &["--no-serve"]),
            cold,
            "a search from the cache answers like one without it"
        );
        assert_eq!(
            fs::read(&cache).unwrap(),
            written,
            "a search from the cache keeps it as it was"
        );

        fs::write(&cache, b"not an index cache").unwrap();
        let rebuilt = opcov(
            &runtime,
            &[
                "consumers",
                "--root",
                root.to_str().unwrap(),
                "--target",
                REQUEST,
                "--format",
                "json",
                "--no-serve",
            ],
        );
        assert!(rebuilt.status.success());
        assert!(
            String::from_utf8_lossy(&rebuilt.stderr).contains("rebuilding the index cache"),
            "{}",
            String::from_utf8_lossy(&rebuilt.stderr)
        );
        assert_eq!(
            serde_json::from_slice::<serde_json::Value>(&rebuilt.stdout).unwrap(),
            cold
        );
        let replaced = fs::read(&cache).unwrap();
        assert_ne!(
            replaced, b"not an index cache",
            "the corrupt cache is replaced"
        );
        assert_eq!(consumers(&runtime, &root, &["--no-serve"]), cold);
        assert_eq!(
            fs::read(&cache).unwrap(),
            replaced,
            "the replaced cache is read back"
        );
        fs::remove_dir_all(&runtime).unwrap();
    }
}
