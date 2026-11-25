#[cfg(test)]
mod live {
    use std::fs;
    use std::path::{Path, PathBuf};

    use opcov_php::{Index, Project, Refresh, Request};

    const REQUESTS: [&str; 6] = [
        r#"{"targets": ["method App\\Services\\Report::render", "method App\\Services\\Report::__construct", "constant App\\Services\\Report::LIMIT", "property App\\Services\\Report::title"]}"#,
        r#"{"targets": ["method App\\Actions\\Publish::handle", "method App\\Jobs\\SendDigest::handle", "method App\\Jobs\\SendDigest::failed", "method App\\Jobs\\PlainHandler::handle", "method App\\Listeners\\PostSubscriber::onPublished"]}"#,
        r#"{"targets": ["method App\\Models\\Post::publish", "method App\\Models\\Comment::approve", "method App\\Observers\\PostObserver::created", "method App\\Models\\MakesSlug::bootMakesSlug", "method App\\Listeners\\CommentSubscriber::onCreated"]}"#,
        r#"{"targets": ["method App\\Models\\PostBuilder::published", "method App\\Models\\Comment::scopeVisible"]}"#,
        r#"{"targets": ["method App\\Services\\Clock::now", "method App\\Console\\Reindex::handle"]}"#,
        r#"{"targets": ["method App\\Uses\\Receivers::run", "method App\\Models\\Comment::approve"]}"#,
    ];

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

    fn project(name: &str) -> PathBuf {
        let fixture = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/project");
        let root = Path::new(env!("CARGO_TARGET_TMPDIR")).join(format!("live-{name}"));
        if root.exists() {
            fs::remove_dir_all(&root).unwrap();
        }
        copy(&fixture, &root);
        return root;
    }

    fn edit(root: &Path, relative: &str, from: &str, to: &str) {
        let path = root.join(relative);
        let source = fs::read_to_string(&path).unwrap();
        assert!(source.contains(from), "{relative} contains {from}");
        fs::write(&path, source.replacen(from, to, 1)).unwrap();
    }

    fn results(index: &Index) -> Vec<Vec<String>> {
        return REQUESTS
            .iter()
            .map(|request| {
                let request: Request = serde_json::from_str(request).unwrap();
                let search = index.uses(&request);
                return search
                    .uses
                    .into_iter()
                    .map(|found| return format!("{}:{} {}", found.path, found.line, found.target))
                    .chain(
                        search
                            .untyped
                            .into_iter()
                            .map(|untyped| return format!("untyped {}", untyped.path)),
                    )
                    .collect();
            })
            .collect();
    }

    fn refreshed(root: &Path, edits: &[(&str, &str, &str)]) -> (Refresh, Index) {
        let project = Project::open(root).unwrap();
        let mut index = Index::build(&project).unwrap();
        results(&index);
        for (relative, from, to) in edits {
            edit(root, relative, from, to);
        }
        let changed: Vec<PathBuf> = edits
            .iter()
            .map(|(relative, _, _)| return root.join(relative))
            .collect();
        let changed: Vec<&Path> = changed.iter().map(PathBuf::as_path).collect();
        let refresh = index.refresh(&project, &changed).unwrap();
        return (refresh, index);
    }

    fn fresh(root: &Path) -> Index {
        return Index::build(&Project::open(root).unwrap()).unwrap();
    }

    #[test]
    fn a_body_edit_updates_the_index_in_place_with_the_results_of_a_fresh_build() {
        let root = project("body");
        let (refresh, index) = refreshed(
            &root,
            &[
                (
                    "app/Uses/Receivers.php",
                    "        $report->render();\n",
                    "        $comment = fn (Report $again) => $again->render();\n        $report->render();\n        $report->render();\n",
                ),
                (
                    "app/Models/Comment.php",
                    "    public function approve(): void\n    {\n",
                    "    public function approve(): void\n    {\n        $this->approve();\n",
                ),
            ],
        );
        assert_eq!(
            refresh,
            Refresh::Updated(vec![
                "app/Models/Comment.php".to_owned(),
                "app/Uses/Receivers.php".to_owned()
            ])
        );
        assert_eq!(results(&index), results(&fresh(&root)));
    }

    #[test]
    fn an_unchanged_file_leaves_the_index_alone() {
        let root = project("unchanged");
        let (refresh, _) = refreshed(
            &root,
            &[(
                "app/Uses/Receivers.php",
                "        new Report();\n",
                "        new Report();\n",
            )],
        );
        assert_eq!(refresh, Refresh::Unchanged);
    }

    #[test]
    fn a_declaration_change_asks_for_a_rebuild() {
        let root = project("declaration");
        let (refresh, _) = refreshed(
            &root,
            &[(
                "app/Models/Comment.php",
                "    public function approve(): void",
                "    public function approve(int $times = 1): void",
            )],
        );
        assert_eq!(
            refresh,
            Refresh::Rebuild("app/Models/Comment.php changed a declaration".to_owned())
        );
    }

    #[test]
    fn a_deleted_file_asks_for_a_rebuild() {
        let root = project("deleted");
        let project = Project::open(&root).unwrap();
        let mut index = Index::build(&project).unwrap();
        let path = root.join("app/Uses/Receivers.php");
        fs::remove_file(&path).unwrap();
        assert_eq!(
            index.refresh(&project, &[path.as_path()]).unwrap(),
            Refresh::Rebuild("app/Uses/Receivers.php was deleted".to_owned())
        );
    }
}
