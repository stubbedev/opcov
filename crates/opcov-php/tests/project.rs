#[cfg(test)]
mod project {
    use std::collections::BTreeMap;
    use std::fs;
    use std::path::{Path, PathBuf};

    use opcov_php::{Overlaid, Project};

    const EMPTY_MAP: &str = "<?php\n\n$vendorDir = dirname(__DIR__);\n$baseDir = dirname($vendorDir);\n\nreturn array(\n);\n";

    #[test]
    fn empty_autoload_maps_are_read_as_empty_and_autoload_dev_counts_as_project_code() {
        let root = Path::new(env!("CARGO_TARGET_TMPDIR")).join("project-empty-maps");
        if root.exists() {
            fs::remove_dir_all(&root).unwrap();
        }
        let composer = root.join("vendor/composer");
        fs::create_dir_all(&composer).unwrap();
        fs::create_dir_all(root.join("src")).unwrap();
        fs::create_dir_all(root.join("tests")).unwrap();
        fs::write(root.join("src/Calc.php"), "<?php\n").unwrap();
        fs::write(root.join("tests/CalcTest.php"), "<?php\n").unwrap();
        fs::write(
            root.join("composer.json"),
            r#"{"autoload": {"psr-4": {"App\\": "src/"}}, "autoload-dev": {"psr-4": {"Tests\\": "tests/"}}}"#,
        )
        .unwrap();
        for map in [
            "autoload_classmap.php",
            "autoload_files.php",
            "autoload_psr4.php",
        ] {
            fs::write(composer.join(map), EMPTY_MAP).unwrap();
        }
        let project = Project::open(&root).unwrap();
        assert_eq!(project.source_files(), [root.join("src/Calc.php")]);
        assert_eq!(
            project.all_files(),
            [root.join("src/Calc.php"), root.join("tests/CalcTest.php")]
        );
    }

    #[test]
    fn an_overlay_replaces_removes_and_adds_project_sources() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/project");
        let plain = Project::open(&root).unwrap();
        let overlaid = plain.clone().overlaid(BTreeMap::from([
            (PathBuf::from("app/Services/Clock.php"), Overlaid::Absent),
            (
                PathBuf::from("app/Services/Calendar.php"),
                Overlaid::Contents(b"<?php\n".to_vec()),
            ),
            (
                PathBuf::from("app/notes.txt"),
                Overlaid::Contents(b"not php".to_vec()),
            ),
            (
                PathBuf::from("config/extra.php"),
                Overlaid::Contents(b"<?php\n".to_vec()),
            ),
        ]));
        let mut expected = plain.source_files();
        expected.retain(|path| return *path != root.join("app/Services/Clock.php"));
        expected.push(root.join("app/Services/Calendar.php"));
        expected.sort();
        assert_eq!(
            overlaid.source_files(),
            expected,
            "an absent file leaves, a PHP file inside the autoload scope joins, other files do not"
        );
        assert_eq!(
            (
                overlaid.class_file("App\\Services\\Clock"),
                overlaid.class_file("App\\Services\\Calendar"),
                plain.class_file("App\\Services\\Calendar"),
            ),
            (None, Some(root.join("app/Services/Calendar.php")), None),
            "PSR-4 lookups see the overlay"
        );
    }
}
