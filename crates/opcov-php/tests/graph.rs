#[cfg(test)]
#[path = "support/fixture.rs"]
mod fixture;

#[cfg(test)]
mod graph {
    use std::collections::BTreeMap;
    use std::fs;
    use std::path::PathBuf;
    use std::sync::OnceLock;

    use opcov_php::{Changed, FileChange, Graph, Index, Overlaid, Project, Revision};

    use crate::fixture::project_copy;

    const REPORT: &str = "app/Services/Report.php";
    const ROUTES: &str = "app/routes.php";
    const OVERRIDING: &str = "app/Uses/Overriding.php";
    const RENDER: &str = "    public function render(): string\n    {\n        return '';\n    }\n";
    const HEAD_RENDER: &str =
        "    public function render(): string\n    {\n        return 'head';\n    }\n";
    const RENDERER: &str =
        "\n    public function renderer(): string\n    {\n        return $this->render();\n    }\n";
    const OVERRIDE: &str =
        "    public function render(): string\n    {\n        return 'own';\n    }\n";
    const ROUTES_SOURCE: &str =
        "<?php\n\nuse App\\Services\\Report;\n\n(new Report())->renderer();\n";
    const OVERRIDING_SOURCE: &str = "<?php\n\nnamespace App\\Uses;\n\nuse App\\Services\\OverridingReport;\n\nfinal class Overriding\n{\n    public function run(OverridingReport $report): string\n    {\n        return $report->render();\n    }\n}\n\nfinal class Caller\n{\n    public function call(Overriding $overriding, OverridingReport $report): string\n    {\n        return $overriding->run($report);\n    }\n}\n";

    fn replaced(source: &str, from: &str, to: &str) -> String {
        assert!(source.contains(from), "the source holds {from:?}");
        return source.replacen(from, to, 1);
    }

    fn line_of(source: &str, needle: &str) -> u32 {
        let offset = source.find(needle).unwrap();
        let before = source.get(..offset).unwrap();
        return u32::try_from(before.matches('\n').count().checked_add(1).unwrap()).unwrap();
    }

    fn span(source: &str, block: &str) -> (u32, u32) {
        let first = line_of(source, block);
        let last = first
            .checked_add(u32::try_from(block.trim_end_matches('\n').matches('\n').count()).unwrap())
            .unwrap();
        return (first, last);
    }

    fn parts(path: &str, ranges: Vec<(u32, u32)>, deletions: Vec<u32>) -> FileChange {
        return FileChange {
            path: path.to_owned(),
            changed: Changed::Parts { ranges, deletions },
        };
    }

    const RUNS: &str = "app/Actions/Runs.php";

    struct Scenario {
        root: PathBuf,
        base_report: String,
        head_report: String,
    }

    fn scenario() -> Scenario {
        let root = project_copy("graph-project");
        let original = fs::read_to_string(root.join(REPORT)).unwrap();
        let head_report = replaced(&original, RENDER, HEAD_RENDER);
        fs::write(root.join(REPORT), &head_report).unwrap();
        fs::write(root.join(OVERRIDING), OVERRIDING_SOURCE).unwrap();
        let base_report = replaced(
            &replaced(&original, RENDER, &format!("{RENDER}{RENDERER}")),
            OVERRIDE,
            "",
        );
        return Scenario {
            root,
            base_report,
            head_report,
        };
    }

    fn graph() -> &'static serde_json::Value {
        static GRAPH: OnceLock<serde_json::Value> = OnceLock::new();
        return GRAPH.get_or_init(walked);
    }

    fn walked() -> serde_json::Value {
        let Scenario {
            root,
            base_report,
            head_report,
        } = scenario();
        let base_project = Project::open(&root).unwrap().overlaid(BTreeMap::from([
            (
                PathBuf::from(REPORT),
                Overlaid::Contents(base_report.clone().into_bytes()),
            ),
            (
                PathBuf::from(ROUTES),
                Overlaid::Contents(ROUTES_SOURCE.as_bytes().to_vec()),
            ),
        ]));
        let base_index = Index::build(&base_project).unwrap();
        let head_index = Index::build(&Project::open(&root).unwrap()).unwrap();
        let (render_body, _) = span(&base_report, "        return '';");
        let base_changes = [
            parts(
                REPORT,
                vec![(render_body, render_body), span(&base_report, RENDERER)],
                vec![
                    line_of(&base_report, "class Unrelated")
                        .checked_sub(2)
                        .unwrap(),
                ],
            ),
            parts(ROUTES, vec![(1, 5)], vec![]),
        ];
        let (head_body, _) = span(&head_report, "        return 'head';");
        let head_changes = [parts(
            REPORT,
            vec![(head_body, head_body), span(&head_report, OVERRIDE)],
            vec![head_body.checked_add(2).unwrap()],
        )];
        let graph = Graph::walk(
            Revision {
                index: &base_index,
                changes: &base_changes,
            },
            Revision {
                index: &head_index,
                changes: &head_changes,
            },
            &[],
            2,
        )
        .unwrap();
        return serde_json::to_value(graph).unwrap();
    }

    fn place(location: &serde_json::Value) -> String {
        if location.is_null() {
            return "-".to_owned();
        }
        return format!(
            "{}:{}-{}",
            location["path"].as_str().unwrap(),
            location["first"],
            location["last"]
        );
    }

    fn lines(sites: &serde_json::Value) -> String {
        return sites
            .as_array()
            .unwrap()
            .iter()
            .map(|site| return site["line"].to_string())
            .collect::<Vec<_>>()
            .join(",");
    }

    fn described(graph: &serde_json::Value) -> Vec<String> {
        let nodes = graph["nodes"].as_array().unwrap().iter().map(|node| {
            return format!(
                "{} {} {} {} | {} | {}",
                node["depth"],
                node["change"].as_str().unwrap(),
                node["kind"].as_str().unwrap(),
                node["id"].as_str().unwrap(),
                place(&node["base"]),
                place(&node["head"]),
            );
        });
        let edges = graph["edges"].as_array().unwrap().iter().map(|edge| {
            return format!(
                "{} {} -> {} | {} | {}",
                edge["change"].as_str().unwrap(),
                edge["from"].as_str().unwrap(),
                edge["to"].as_str().unwrap(),
                lines(&edge["base"]),
                lines(&edge["head"]),
            );
        });
        return nodes.chain(edges).collect();
    }

    #[test]
    fn both_sides_are_searched_for_every_target_so_edges_compare() {
        assert_eq!(
            described(graph()),
            [
                "0 added method method App\\Services\\OverridingReport::render | - | app/Services/Report.php:32-35",
                "0 modified method method App\\Services\\Report::render | app/Services/Report.php:20-23 | app/Services/Report.php:20-23",
                "0 removed method method App\\Services\\Report::renderer | app/Services/Report.php:25-28 | -",
                "1 unchanged method method App\\Uses\\Lookalike::run | app/Uses/Lookalike.php:9-14 | app/Uses/Lookalike.php:9-14",
                "1 unchanged method method App\\Uses\\Overriding::run | app/Uses/Overriding.php:9-12 | app/Uses/Overriding.php:9-12",
                "1 unchanged method method App\\Uses\\Receivers::run | app/Uses/Receivers.php:13-24 | app/Uses/Receivers.php:13-24",
                "1 removed script script app/routes.php | app/routes.php:5-5 | -",
                "2 unchanged method method App\\Uses\\Caller::call | app/Uses/Overriding.php:17-20 | app/Uses/Overriding.php:17-20",
                "removed method App\\Services\\Report::renderer -> method App\\Services\\OverridingReport::render | 27 | ",
                "removed method App\\Services\\Report::renderer -> method App\\Services\\Report::render | 27 | ",
                "kept method App\\Uses\\Caller::call -> method App\\Uses\\Overriding::run | 19 | 19",
                "kept method App\\Uses\\Lookalike::run -> method App\\Services\\Report::renderer | 11 | 11",
                "kept method App\\Uses\\Overriding::run -> method App\\Services\\OverridingReport::render | 11 | 11",
                "no_longer_resolves method App\\Uses\\Overriding::run -> method App\\Services\\Report::render | 11 | ",
                "kept method App\\Uses\\Receivers::run -> method App\\Services\\OverridingReport::render | 15,16,18,19,21 | 15,16,18,19,21",
                "kept method App\\Uses\\Receivers::run -> method App\\Services\\Report::render | 15,16,17,18,19,21 | 15,16,17,19,21",
                "removed script app/routes.php -> method App\\Services\\Report::renderer | 5 | ",
            ],
            "an override added at head takes the unchanged call in Overriding::run and line 18 of Receivers::run from Report::render; \
             the removed renderer keeps its unchanged caller and loses the base-only script; the caller of a consumer is the next level"
        );
    }

    #[test]
    fn the_document_names_its_version_depth_and_both_indexes() {
        let graph = graph();
        assert_eq!(
            (
                graph["version"].clone(),
                graph["depth"].clone(),
                graph["index"]["base"]["sources"].clone(),
                graph["index"]["head"]["sources"].clone()
            ),
            (
                serde_json::json!(1),
                serde_json::json!(2),
                serde_json::json!(24),
                serde_json::json!(23)
            ),
            "the base index holds the overlaid routes.php besides the head's sources"
        );
    }

    #[test]
    fn a_call_dropped_from_a_trait_method_is_removed_from_each_using_class() {
        let root = project_copy("graph-trait");
        let original = fs::read_to_string(root.join(RUNS)).unwrap();
        let head = replaced(
            &replaced(&original, "        (new static())->handle();\n", ""),
            "    public function handle(): void\n    {\n    }\n",
            "    public function handle(): void\n    {\n        echo 'published';\n    }\n",
        );
        fs::write(root.join(RUNS), &head).unwrap();
        let base_index = Index::build(&Project::open(&root).unwrap().overlaid(BTreeMap::from([(
            PathBuf::from(RUNS),
            Overlaid::Contents(original.clone().into_bytes()),
        )])))
        .unwrap();
        let head_index = Index::build(&Project::open(&root).unwrap()).unwrap();
        let (dropped, _) = span(&original, "        (new static())->handle();");
        let (echo, _) = span(&head, "        echo 'published';");
        let graph = serde_json::to_value(
            Graph::walk(
                Revision {
                    index: &base_index,
                    changes: &[parts(
                        RUNS,
                        vec![(dropped, dropped)],
                        vec![echo.checked_add(2).unwrap()],
                    )],
                },
                Revision {
                    index: &head_index,
                    changes: &[parts(RUNS, vec![(echo, echo)], vec![dropped])],
                },
                &[],
                1,
            )
            .unwrap(),
        )
        .unwrap();
        let edges: Vec<String> = described(&graph)
            .into_iter()
            .filter(|line| return line.contains(" -> method App\\Actions\\Publish::handle"))
            .collect();
        assert_eq!(
            edges,
            [
                "removed method App\\Actions\\Publish::run -> method App\\Actions\\Publish::handle | 9 | "
            ],
            "the call left Runs::run, whose lines changed, so Publish's copy of it lost the call rather than stopped resolving it"
        );
    }

    const MAKER: &str = "app/Sharing/Maker.php";
    const FIRST: &str = "app/Sharing/First.php";
    const SECOND: &str = "app/Sharing/Second.php";
    const CHAIN: &str = "app/Sharing/Chain.php";
    const BYSTANDER: &str = "app/Sharing/Bystander.php";
    const REPUBLISH: &str = "app/Actions/Republish.php";

    fn maker(returned: &str) -> String {
        return format!(
            "<?php\n\nnamespace App\\Sharing;\n\nfinal class Maker\n{{\n    public function make(): {returned}\n    {{\n        return new {returned}();\n    }}\n}}\n"
        );
    }

    fn runner(class: &str, header: &str) -> String {
        return format!(
            "<?php\n\nnamespace App\\Sharing;\n\n{header}final class {class}\n{{\n    public function run(): string\n    {{\n        return '{class}';\n    }}\n}}\n"
        );
    }

    const CHAIN_SOURCE: &str = "<?php\n\nnamespace App\\Sharing;\n\nfinal class Chain\n{\n    public function go(Maker $maker): string\n    {\n        return $maker->make()->run();\n    }\n}\n";

    fn whole(path: &str) -> FileChange {
        return FileChange {
            path: path.to_owned(),
            changed: Changed::Whole,
        };
    }

    struct Sides<'a> {
        name: &'a str,
        head: &'a [(&'a str, String)],
        base: &'a [(&'a str, Option<String>)],
        base_changes: &'a [FileChange],
        head_changes: &'a [FileChange],
    }

    fn walk(base: &Index, head: &Index, sides: &Sides<'_>) -> serde_json::Value {
        let graph = Graph::walk(
            Revision {
                index: base,
                changes: sides.base_changes,
            },
            Revision {
                index: head,
                changes: sides.head_changes,
            },
            &[],
            2,
        )
        .unwrap();
        return serde_json::to_value(graph).unwrap();
    }

    fn shared_and_independent(sides: &Sides<'_>) -> (usize, serde_json::Value, serde_json::Value) {
        let root = project_copy(sides.name);
        for (path, contents) in sides.head {
            let target = root.join(path);
            fs::create_dir_all(target.parent().unwrap()).unwrap();
            fs::write(target, contents).unwrap();
        }
        let overlay: BTreeMap<PathBuf, Overlaid> = sides
            .base
            .iter()
            .map(|(path, contents)| {
                let overlaid = contents.as_ref().map_or(Overlaid::Absent, |contents| {
                    return Overlaid::Contents(contents.clone().into_bytes());
                });
                return (PathBuf::from(path), overlaid);
            })
            .collect();
        let head = || return Index::build(&Project::open(&root).unwrap()).unwrap();
        let base = || {
            return Index::build(&Project::open(&root).unwrap().overlaid(overlay.clone())).unwrap();
        };
        let paths: Vec<&str> = sides
            .base_changes
            .iter()
            .chain(sides.head_changes)
            .map(|change| return change.path.as_str())
            .collect();
        let (mut sharing, donor) = (base(), head());
        let shared = sharing.share_unaffected(&donor, &paths);
        let with = walk(&sharing, &donor, sides);
        let without = walk(&base(), &head(), sides);
        return (shared, with, without);
    }

    fn resolving(graph: &serde_json::Value, needle: &str) -> Vec<String> {
        return described(graph)
            .into_iter()
            .filter(|line| return line.contains(needle))
            .collect();
    }

    fn changed_return_type(
        name: &str,
        bystander: &str,
    ) -> (usize, serde_json::Value, serde_json::Value) {
        let changes = [whole(MAKER), whole(FIRST), whole(SECOND)];
        return shared_and_independent(&Sides {
            name,
            head: &[
                (MAKER, maker("Second")),
                (FIRST, runner("First", "")),
                (SECOND, runner("Second", "")),
                (CHAIN, CHAIN_SOURCE.to_owned()),
                (BYSTANDER, bystander.to_owned()),
            ],
            base: &[(MAKER, Some(maker("First")))],
            base_changes: &changes,
            head_changes: &changes,
        });
    }

    #[test]
    fn a_file_using_a_member_whose_signature_changed_is_analysed_on_each_side() {
        let (shared, with, without) = changed_return_type("graph-share-signature", "<?php\n");
        assert!(shared > 0, "the files the change cannot affect are shared");
        assert_eq!(
            with, without,
            "sharing analyses changes nothing in the graph"
        );
        assert_eq!(
            resolving(&with, "Chain::go -> method App\\Sharing\\"),
            [
                "no_longer_resolves method App\\Sharing\\Chain::go -> method App\\Sharing\\First::run | 9 | ",
                "kept method App\\Sharing\\Chain::go -> method App\\Sharing\\Maker::make | 9 | 9",
                "now_resolves method App\\Sharing\\Chain::go -> method App\\Sharing\\Second::run |  | 9",
            ],
            "the unchanged Chain names make, whose return type changed, so each side resolves run itself"
        );
    }

    #[test]
    fn a_name_used_only_as_a_variable_does_not_stop_sharing() {
        let (by_variable, ..) = changed_return_type(
            "graph-share-variable",
            "<?php\n\nnamespace App\\Sharing;\n\nfunction bystander(): string\n{\n    $make = 'unrelated';\n    return $make;\n}\n",
        );
        let (by_call, ..) = changed_return_type(
            "graph-share-call",
            "<?php\n\nnamespace App\\Sharing;\n\nfunction bystander(Maker $maker): void\n{\n    $maker->make();\n}\n",
        );
        assert_eq!(
            by_call.checked_add(1),
            Some(by_variable),
            "a $make variable is no site of make, a ->make() call is"
        );
    }

    #[test]
    fn a_changed_class_header_shares_nothing() {
        let changes = [whole(FIRST)];
        let (shared, with, without) = shared_and_independent(&Sides {
            name: "graph-share-header",
            head: &[
                (FIRST, runner("First", "/** @deprecated */\n")),
                (CHAIN, CHAIN_SOURCE.to_owned()),
                (MAKER, maker("First")),
            ],
            base: &[(FIRST, Some(runner("First", "")))],
            base_changes: &changes,
            head_changes: &changes,
        });
        assert_eq!(
            shared, 0,
            "a header change can retype code that never names the class"
        );
        assert_eq!(with, without, "the graph is that of independent analyses");
    }

    #[test]
    fn a_user_of_a_trait_deleted_at_head_keeps_the_trait_file_unshared() {
        let changes = [whole(REPUBLISH)];
        let (shared, with, without) = shared_and_independent(&Sides {
            name: "graph-share-trait",
            head: &[],
            base: &[(
                REPUBLISH,
                Some(
                    "<?php\n\nnamespace App\\Actions;\n\nclass Republish\n{\n    use Runs;\n\n    public function handle(): void\n    {\n    }\n}\n"
                        .to_owned(),
                ),
            )],
            base_changes: &changes,
            head_changes: &[],
        });
        assert!(shared > 0, "files that do not hold the trait are shared");
        assert_eq!(
            with, without,
            "sharing analyses changes nothing in the graph"
        );
        assert_eq!(
            resolving(&with, " -> method App\\Actions\\Republish::handle"),
            [
                "no_longer_resolves method App\\Actions\\Republish::run -> method App\\Actions\\Republish::handle | 9 | "
            ],
            "the unchanged trait method resolved handle in its copy for the class deleted at head"
        );
    }
}
