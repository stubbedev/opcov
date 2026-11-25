#[cfg(test)]
mod project {
    use std::fs;
    use std::path::Path;

    use opcov_php::Project;

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
}
