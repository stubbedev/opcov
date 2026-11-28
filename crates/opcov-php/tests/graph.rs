#[cfg(test)]
#[path = "support/fixture.rs"]
mod fixture;

#[cfg(test)]
mod graph {
    use std::collections::BTreeMap;
    use std::fs;
    use std::path::PathBuf;
    use std::sync::OnceLock;

    use opcov_php::{Changed, FileChange, Graph, Index, Overlaid, Project, Revision, Routes};

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
            &Routes::default(),
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
                "0 body_changed method method App\\Services\\Report::render | app/Services/Report.php:20-23 | app/Services/Report.php:20-23",
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
    fn a_changed_member_carries_its_hunks_on_each_side() {
        let changed: Vec<String> = graph()["nodes"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|node| return node["depth"] == 0)
            .map(|node| return format!("{} {}", node["id"].as_str().unwrap(), node["changed"]))
            .collect();
        assert_eq!(
            changed,
            [
                r#"method App\Services\OverridingReport::render {"base":null,"head":{"ranges":[[32,35]],"removed_before":[]}}"#,
                r#"method App\Services\Report::render {"base":{"ranges":[[22,22]],"removed_before":[]},"head":{"ranges":[[22,22]],"removed_before":[]}}"#,
                r#"method App\Services\Report::renderer {"base":{"ranges":[[25,28]],"removed_before":[]},"head":null}"#,
            ],
            "render changed its body line on both sides; renderer is gone at the head and the override is new there; \
             the head's deletion after line 23 lies past render's last line"
        );
        let script = graph()["nodes"]
            .as_array()
            .unwrap()
            .iter()
            .find(|node| return node["id"] == "script app/routes.php")
            .unwrap();
        assert_eq!(
            script["changed"].to_string(),
            r#"{"base":{"ranges":[[1,5]],"removed_before":[]},"head":null}"#,
            "routes.php holds no function, so all of its changed lines are the script's"
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
                &Routes::default(),
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
            &Routes::default(),
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

    const PINGER: &str = "app/Proof/Pinger.php";

    fn proof_class(name: &str, body: &str) -> String {
        return format!("<?php\n\nnamespace App\\Proof;\n\nfinal class {name}\n{{\n{body}}}\n");
    }

    fn proof_graph(name: &str) -> serde_json::Value {
        let ping = |returned: &str| {
            return proof_class(
                "Pinger",
                &format!(
                    "    public function ping(): string\n    {{\n        return '{returned}';\n    }}\n"
                ),
            );
        };
        let changes = [whole(PINGER)];
        let sides = Sides {
            name,
            head: &[
                (PINGER, ping("head")),
                (
                    "app/Proof/Typed.php",
                    proof_class(
                        "Typed",
                        "    public function run(Pinger $pinger): string\n    {\n        return $pinger->ping();\n    }\n",
                    ),
                ),
                (
                    "app/Proof/Untyped.php",
                    proof_class(
                        "Untyped",
                        "    public function run($pinger): string\n    {\n        return $pinger->ping();\n    }\n",
                    ),
                ),
                (
                    "app/Proof/Above.php",
                    proof_class(
                        "Above",
                        "    public function run(Untyped $untyped): string\n    {\n        return $untyped->run(null);\n    }\n",
                    ),
                ),
            ],
            base: &[(PINGER, Some(ping("base")))],
            base_changes: &changes,
            head_changes: &changes,
        };
        let (_, graph, _) = shared_and_independent(&sides);
        return graph;
    }

    #[test]
    fn a_node_is_proven_only_through_a_chain_of_typed_edges() {
        let graph = proof_graph("graph-proof");
        let resolutions = |key: &str, describe: &dyn Fn(&serde_json::Value) -> String| {
            return graph[key]
                .as_array()
                .unwrap()
                .iter()
                .map(|item| {
                    return format!(
                        "{} {}",
                        item["resolution"].as_str().unwrap(),
                        describe(item)
                    );
                })
                .collect::<Vec<String>>();
        };
        assert_eq!(
            (
                resolutions("edges", &|edge| {
                    return format!(
                        "{} -> {}",
                        edge["from"].as_str().unwrap(),
                        edge["to"].as_str().unwrap()
                    );
                }),
                resolutions("nodes", &|node| return node["id"]
                    .as_str()
                    .unwrap()
                    .to_owned()),
            ),
            (
                vec![
                    "typed method App\\Proof\\Above::run -> method App\\Proof\\Untyped::run"
                        .to_owned(),
                    "typed method App\\Proof\\Typed::run -> method App\\Proof\\Pinger::ping"
                        .to_owned(),
                    "by_name method App\\Proof\\Untyped::run -> method App\\Proof\\Pinger::ping"
                        .to_owned(),
                ],
                vec![
                    "typed method App\\Proof\\Pinger::ping".to_owned(),
                    "typed method App\\Proof\\Typed::run".to_owned(),
                    "by_name method App\\Proof\\Untyped::run".to_owned(),
                    "by_name method App\\Proof\\Above::run".to_owned(),
                ]
            ),
            "Above's own call is typed, but it reaches ping only through Untyped's untyped call"
        );
    }

    #[test]
    fn an_unproven_use_is_a_risk_and_counts_apart_in_the_impact() {
        let graph = proof_graph("graph-proof-risks");
        assert_eq!(
            risks(&graph),
            ["unproven_uses method App\\Proof\\Untyped::run -> method App\\Proof\\Pinger::ping"],
            "only the untyped call reaches the changed ping without a type to prove it"
        );
        let ping = graph["nodes"]
            .as_array()
            .unwrap()
            .iter()
            .find(|node| return node["id"] == "method App\\Proof\\Pinger::ping")
            .unwrap();
        assert_eq!(
            ping["impact"],
            serde_json::json!({"callers": 3, "proven_callers": 1, "entry_points": 0}),
            "Typed, Untyped and Above reach ping; only Typed through typed calls alone"
        );
    }

    const SHAPES: &str = "app/Proof/Shapes.php";

    fn shapes(members: &str) -> String {
        return proof_class("Shapes", members);
    }

    #[test]
    fn a_modified_member_says_whether_its_declaration_or_only_its_body_changed() {
        let base = shapes(concat!(
            "    public function body(): string\n    {\n        return 'base';\n    }\n\n",
            "    public function spaced(int $a, string $b = 'x'): string\n    {\n        return 'base';\n    }\n\n",
            "    public function commented(int $a): string\n    {\n        return 'base';\n    }\n\n",
            "    public function returns(): string\n    {\n        return 'base';\n    }\n\n",
            "    /**\n     * @return list<int>\n     */\n    public function documented(): array\n    {\n        return [];\n    }\n\n",
            "    public function defaulted(string $a = 'a b'): string\n    {\n        return $a;\n    }\n\n",
            "    public function layout(): string\n    {\n        return 'same';\n    }\n\n",
            "    public const LIMIT = 1;\n",
        ));
        let head = shapes(concat!(
            "    public function body(): string\n    {\n        return 'head';\n    }\n\n",
            "    public function spaced(\n        int $a,\n        string $b = 'x',\n    ): string\n    {\n        return 'base';\n    }\n\n",
            "    public function commented(/* the count */ int $a): string\n    {\n        return 'base';\n    }\n\n",
            "    public function returns(): ?string\n    {\n        return 'base';\n    }\n\n",
            "    /**\n     * @return list<string>\n     */\n    public function documented(): array\n    {\n        return [];\n    }\n\n",
            "    public function defaulted(string $a = 'a  b'): string\n    {\n        return $a;\n    }\n\n",
            "    public function layout(): string\n    {\n        // still the same\n            return 'same';\n    }\n\n",
            "    public const LIMIT = 2;\n",
        ));
        let changes = [whole(SHAPES)];
        let (_, graph, _) = shared_and_independent(&Sides {
            name: "graph-declarations",
            head: &[(SHAPES, head)],
            base: &[(SHAPES, Some(base))],
            base_changes: &changes,
            head_changes: &changes,
        });
        let described_changes: Vec<String> = graph["nodes"]
            .as_array()
            .unwrap()
            .iter()
            .map(|node| {
                return format!(
                    "{} {}",
                    node["change"].as_str().unwrap(),
                    node["id"].as_str().unwrap()
                );
            })
            .collect();
        assert_eq!(
            described_changes,
            [
                "body_changed method App\\Proof\\Shapes::body",
                "formatting_changed method App\\Proof\\Shapes::commented",
                "signature_changed method App\\Proof\\Shapes::defaulted",
                "signature_changed method App\\Proof\\Shapes::documented",
                "formatting_changed method App\\Proof\\Shapes::layout",
                "signature_changed method App\\Proof\\Shapes::returns",
                "formatting_changed method App\\Proof\\Shapes::spaced",
                "signature_changed constant App\\Proof\\Shapes::LIMIT",
            ],
            "layout, comments and a trailing comma leave a declaration as it was; a type, a docblock type, \
             the whitespace inside a default string and a constant's value change it; a comment and \
             indentation alone change neither declaration nor body"
        );
    }

    fn risks(graph: &serde_json::Value) -> Vec<String> {
        let risks = graph["risks"].as_object().unwrap();
        return risks
            .iter()
            .filter_map(|(kind, links)| return Some((kind, links.as_array()?)))
            .flat_map(|(kind, links)| {
                return links
                    .iter()
                    .filter(|link| return link.get("from").is_some())
                    .map(move |link| {
                        return format!(
                            "{kind} {} -> {}",
                            link["from"].as_str().unwrap(),
                            link["to"].as_str().unwrap()
                        );
                    });
            })
            .collect();
    }

    #[test]
    fn the_risks_name_untouched_code_a_change_reaches_differently() {
        let (_, graph, _) = changed_return_type("graph-risks", "<?php\n");
        assert_eq!(
            risks(&graph),
            [
                "stops_resolving method App\\Sharing\\Chain::go -> method App\\Sharing\\First::run",
                "starts_resolving method App\\Sharing\\Chain::go -> method App\\Sharing\\Second::run",
                "untouched_signature_callers method App\\Sharing\\Chain::go -> method App\\Sharing\\Maker::make",
            ],
            "Chain is outside the diff: make's return type changed under it, so its run call moved from First to Second"
        );
    }

    const ENTRY_FILES: [(&str, &str); 13] = [
        (
            "composer.json",
            "{\n    \"autoload\": {\"psr-4\": {\"App\\\\\": \"app/\"}},\n    \"autoload-dev\": {\"psr-4\": {\"Tests\\\\\": \"tests/\"}}\n}\n",
        ),
        (
            "vendor/phpunit/phpunit/src/Framework/TestCase.php",
            "<?php\n\nnamespace PHPUnit\\Framework;\n\nabstract class TestCase\n{\n}\n",
        ),
        (
            "vendor/phpunit/phpunit/src/Framework/Attributes/Test.php",
            "<?php\n\nnamespace PHPUnit\\Framework\\Attributes;\n\n#[\\Attribute]\nfinal class Test\n{\n}\n",
        ),
        (
            "vendor/pestphp/pest/src/Functions.php",
            "<?php\n\nfunction it(string $description, ?\\Closure $closure = null): void\n{\n}\n\nfunction test(string $description, ?\\Closure $closure = null): void\n{\n}\n",
        ),
        (
            "vendor/composer/autoload_psr4.php",
            "<?php\n\n$vendorDir = dirname(__DIR__);\n$baseDir = dirname($vendorDir);\n\nreturn array(\n    'Illuminate\\\\' => array($vendorDir . '/laravel/framework/src/Illuminate'),\n    'PHPUnit\\\\' => array($vendorDir . '/phpunit/phpunit/src'),\n    'App\\\\' => array($baseDir . '/app'),\n    'Tests\\\\' => array($baseDir . '/tests'),\n);\n",
        ),
        (
            "vendor/composer/autoload_files.php",
            "<?php\n\n$vendorDir = dirname(__DIR__);\n$baseDir = dirname($vendorDir);\n\nreturn array(\n    'helpers' => $vendorDir . '/laravel/framework/src/Illuminate/helpers.php',\n    'pest' => $vendorDir . '/pestphp/pest/src/Functions.php',\n);\n",
        ),
        (
            "app/Http/Controllers/BaseController.php",
            "<?php\n\nnamespace App\\Http\\Controllers;\n\nclass BaseController\n{\n    public function show(): string\n    {\n        return 'base';\n    }\n}\n",
        ),
        (
            "app/Http/Controllers/PingController.php",
            "<?php\n\nnamespace App\\Http\\Controllers;\n\nfinal class PingController extends BaseController\n{\n}\n",
        ),
        (
            "app/Http/Controllers/Invoked.php",
            "<?php\n\nnamespace App\\Http\\Controllers;\n\nfinal class Invoked\n{\n    public function __invoke(): string\n    {\n        return 'invoked';\n    }\n}\n",
        ),
        (
            "tests/Unit/AbstractCase.php",
            "<?php\n\nnamespace Tests\\Unit;\n\nuse PHPUnit\\Framework\\TestCase;\n\nabstract class AbstractCase extends TestCase\n{\n    public function testShared(): void\n    {\n    }\n}\n",
        ),
        (
            "tests/Unit/FirstTest.php",
            "<?php\n\nnamespace Tests\\Unit;\n\nuse PHPUnit\\Framework\\Attributes\\Test;\n\nfinal class FirstTest extends AbstractCase\n{\n    #[Test]\n    public function pings(): void\n    {\n    }\n\n    public function helper(): void\n    {\n    }\n}\n",
        ),
        (
            "tests/Unit/SecondTest.php",
            "<?php\n\nnamespace Tests\\Unit;\n\nfinal class SecondTest extends AbstractCase\n{\n    public function testShared(): void\n    {\n    }\n}\n",
        ),
        (
            "tests/Pest/PingPest.php",
            "<?php\n\nuse App\\Http\\Controllers\\Invoked;\n\nit('pings', function (): void {\n    (new Invoked())();\n    (new Invoked())->__invoke();\n});\n",
        ),
    ];

    const ROUTE_TABLE: &str = r#"[
        {"domain": null, "method": "GET|HEAD", "uri": "ping", "name": "ping.show", "action": "App\\Http\\Controllers\\PingController@show", "middleware": ["web"]},
        {"domain": null, "method": "POST", "uri": "invoked", "name": null, "action": "App\\Http\\Controllers\\Invoked", "middleware": []},
        {"domain": null, "method": "DELETE", "uri": "gone", "name": null, "action": "App\\Http\\Controllers\\PingController@gone", "middleware": []},
        {"domain": null, "method": "GET|HEAD", "uri": "up", "name": null, "action": "Closure", "middleware": []},
        {"domain": null, "method": "POST", "uri": "login", "name": null, "action": "Laravel\\Fortify\\Http\\Controllers\\AuthenticatedSessionController@store", "middleware": []},
        {"domain": null, "method": "GET|HEAD", "uri": "deleted", "name": null, "action": "App\\Http\\Controllers\\DeletedController@index", "middleware": []}
    ]"#;

    #[test]
    fn each_node_lists_the_ways_the_framework_and_the_test_runners_enter_it() {
        let root = project_copy("graph-entries");
        for (path, contents) in ENTRY_FILES {
            let target = root.join(path);
            fs::create_dir_all(target.parent().unwrap()).unwrap();
            fs::write(target, contents).unwrap();
        }
        let changed = [
            "app/Http/Controllers/BaseController.php",
            "app/Http/Controllers/Invoked.php",
            "app/Jobs/SendDigest.php",
            "app/Console/Reindex.php",
            "app/Listeners/CommentSubscriber.php",
            "app/Observers/PostObserver.php",
            "tests/Unit/AbstractCase.php",
            "tests/Unit/FirstTest.php",
            "tests/Unit/SecondTest.php",
        ]
        .map(whole);
        let index =
            || return Index::build(&Project::open(&root).unwrap().with_development()).unwrap();
        let (base, head) = (index(), index());
        let routes = Routes::parse(ROUTE_TABLE.as_bytes())
            .unwrap()
            .located(&Project::open(&root).unwrap().with_development());
        let graph = serde_json::to_value(
            Graph::walk(
                Revision {
                    index: &base,
                    changes: &changed,
                },
                Revision {
                    index: &head,
                    changes: &changed,
                },
                &routes,
                &[],
                1,
            )
            .unwrap(),
        )
        .unwrap();
        let entries: Vec<String> = graph["nodes"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|node| return !node["entries"].as_array().unwrap().is_empty())
            .map(|node| return format!("{} {}", node["id"].as_str().unwrap(), node["entries"]))
            .collect();
        let model_hook = |model: &str| {
            return format!(
                r#"{{"kind":"model_hook","model":"App\\Models\\{model}","instance":["push","save","saveOrFail"],"query":["create","createMany","createOrFirst","firstOrCreate","forceCreate","save","saveMany","updateOrCreate"]}}"#
            );
        };
        assert_eq!(
            entries,
            [
                r#"method App\Console\Reindex::handle [{"kind":"command","name":"app:reindex"}]"#.to_owned(),
                r#"method App\Http\Controllers\BaseController::show [{"kind":"route","methods":["GET","HEAD"],"uri":"ping","name":"ping.show","domain":null}]"#.to_owned(),
                r#"method App\Http\Controllers\Invoked::__invoke [{"kind":"route","methods":["POST"],"uri":"invoked","name":null,"domain":null}]"#.to_owned(),
                r#"method App\Jobs\SendDigest::failed [{"kind":"job"}]"#.to_owned(),
                r#"method App\Jobs\SendDigest::handle [{"kind":"job"}]"#.to_owned(),
                format!(
                    r#"method App\Listeners\CommentSubscriber::onCreated [{{"kind":"listener","events":["App\\Events\\CommentCreated"]}},{}]"#,
                    model_hook("Comment")
                ),
                format!(r"method App\Observers\PostObserver::created [{}]", model_hook("Post")),
                r#"method Tests\Unit\AbstractCase::testShared [{"kind":"phpunit_test","classes":["Tests\\Unit\\FirstTest"]}]"#.to_owned(),
                r#"method Tests\Unit\FirstTest::pings [{"kind":"phpunit_test","classes":["Tests\\Unit\\FirstTest"]}]"#.to_owned(),
                r#"method Tests\Unit\SecondTest::testShared [{"kind":"phpunit_test","classes":["Tests\\Unit\\SecondTest"]}]"#.to_owned(),
                r#"script tests/Pest/PingPest.php [{"kind":"pest_test"}]"#.to_owned(),
            ],
            "a route lands on the method that runs it, inherited or invoked; SecondTest overrides testShared, \
             so only FirstTest runs AbstractCase's; helper is no test; the closure route has no method"
        );
        assert_eq!(
            graph["risks"]["broken_routes"].to_string(),
            r#"[{"route":{"methods":["DELETE"],"uri":"gone","name":null,"domain":null},"action":"App\\Http\\Controllers\\PingController@gone"},{"route":{"methods":["GET","HEAD"],"uri":"deleted","name":null,"domain":null},"action":"App\\Http\\Controllers\\DeletedController@index"}]"#,
            "PingController has no gone method and the autoload maps put the missing DeletedController in app/; \
             the Fortify controller is vendor code the index never read, so nothing says it is gone"
        );
    }

    #[test]
    fn the_markdown_summary_lists_each_risk_and_folds_callers_by_namespace() {
        let changes = [whole(MAKER), whole(FIRST), whole(SECOND)];
        let root = project_copy("graph-markdown-risks");
        for (path, contents) in [
            (MAKER, maker("Second")),
            (FIRST, runner("First", "")),
            (SECOND, runner("Second", "")),
            (CHAIN, CHAIN_SOURCE.to_owned()),
        ] {
            let target = root.join(path);
            fs::create_dir_all(target.parent().unwrap()).unwrap();
            fs::write(target, contents).unwrap();
        }
        let base = Index::build(&Project::open(&root).unwrap().overlaid(BTreeMap::from([(
            PathBuf::from(MAKER),
            Overlaid::Contents(maker("First").into_bytes()),
        )])))
        .unwrap();
        let head = Index::build(&Project::open(&root).unwrap()).unwrap();
        let graph = Graph::walk(
            Revision {
                index: &base,
                changes: &changes,
            },
            Revision {
                index: &head,
                changes: &changes,
            },
            &Routes::default(),
            &[],
            1,
        )
        .unwrap();
        let mut rendered = Vec::new();
        graph.render_markdown(&mut rendered, None).unwrap();
        assert_eq!(
            String::from_utf8(rendered).unwrap(),
            r"## Change review

3 changed members (2 formatting only, 1 signature changed); 1 unchanged members and scripts use them within depth 1.

### Risks

#### Untouched code that stops reaching a member (1)

These callers are outside the diff, and their call resolved on the base but not on the head.

- `method App\Sharing\Chain::go` → `method App\Sharing\First::run`

#### Untouched code that now reaches a member (1)

These callers are outside the diff, and their call resolves to this member only on the head.

- `method App\Sharing\Chain::go` → `method App\Sharing\Second::run`

#### Untouched callers of changed signatures (1)

These callers are outside the diff and call a member whose declaration changed.

- `method App\Sharing\Chain::go` → `method App\Sharing\Maker::make`

### Changed members, by impact

| Member | Change | Callers (proven) | Entry points |
| --- | --- | --- | --- |
| `method App\Sharing\First::run` | formatting only | 1 (1) | 0 |
| `method App\Sharing\Maker::make` | signature changed | 1 (1) | 0 |
| `method App\Sharing\Second::run` | formatting only | 1 (1) | 0 |

### Unchanged callers by namespace (1)

The unchanged members and scripts that use the change, per namespace, with how many a chain of typed calls connects to it.

- `App\Sharing`: 1 (1 proven)

",
            "each risk is its own list, the changed members are a table, and the one unchanged caller folds into its namespace"
        );
    }

    const HOLDER: &str = "app/Inferred/Holder.php";

    fn holder(accepted: &str) -> String {
        return format!(
            "<?php\n\nnamespace App\\Inferred;\n\nclass Holder\n{{\n    protected $repo;\n\n    public function setRepo({accepted} $repo): void\n    {{\n        $this->repo = $repo;\n    }}\n}}\n"
        );
    }

    #[test]
    fn a_file_reading_a_property_whose_inferred_type_changed_is_analysed_on_each_side() {
        let repository = |class: &str| {
            return format!(
                "<?php\n\nnamespace App\\Inferred;\n\nclass {class}\n{{\n    public function fetch(): int\n    {{\n        return 1;\n    }}\n}}\n"
            );
        };
        let changes = [
            whole(HOLDER),
            whole("app/Inferred/Repo.php"),
            whole("app/Inferred/OtherRepo.php"),
        ];
        let (shared, with, without) = shared_and_independent(&Sides {
            name: "graph-share-inferred",
            head: &[
                ("app/Inferred/Repo.php", repository("Repo")),
                ("app/Inferred/OtherRepo.php", repository("OtherRepo")),
                (HOLDER, holder("OtherRepo")),
                (
                    "app/Inferred/Reader.php",
                    "<?php\n\nnamespace App\\Inferred;\n\nfinal class Reader extends Holder\n{\n    public function read(): int\n    {\n        return $this->repo->fetch();\n    }\n}\n".to_owned(),
                ),
            ],
            base: &[(HOLDER, Some(holder("Repo")))],
            base_changes: &changes,
            head_changes: &changes,
        });
        assert!(shared > 0, "the files the change cannot affect are shared");
        assert_eq!(
            with, without,
            "sharing analyses changes nothing in the graph"
        );
        assert_eq!(
            resolving(&with, "Reader::read -> method"),
            [
                "now_resolves method App\\Inferred\\Reader::read -> method App\\Inferred\\OtherRepo::fetch |  | 9",
                "no_longer_resolves method App\\Inferred\\Reader::read -> method App\\Inferred\\Repo::fetch | 9 | ",
            ],
            "Reader is unchanged, but $repo is inferred from setRepo's parameter, which changed from Repo to OtherRepo"
        );
    }

    #[test]
    fn a_script_carries_the_changed_lines_outside_its_functions() {
        let script = "app/Proof/boot.php";
        let source = "<?php\n\nuse App\\Proof\\Pinger;\n\n(new Pinger())->ping();\n\nfunction helper(): string\n{\n    return (new Pinger())->ping();\n}\n\n(new Pinger())->ping();\n";
        let pinger = proof_class(
            "Pinger",
            "    public function ping(): string\n    {\n        return 'ping';\n    }\n",
        );
        let changes = [whole(script), whole(PINGER)];
        let (_, graph, _) = shared_and_independent(&Sides {
            name: "graph-script-hunks",
            head: &[(PINGER, pinger.clone()), (script, source.to_owned())],
            base: &[(PINGER, Some(pinger))],
            base_changes: &changes,
            head_changes: &changes,
        });
        let node = graph["nodes"]
            .as_array()
            .unwrap()
            .iter()
            .find(|node| return node["id"] == "script app/Proof/boot.php")
            .unwrap();
        assert_eq!(
            node["changed"]["head"].to_string(),
            r#"{"ranges":[[1,6],[11,12]],"removed_before":[]}"#,
            "helper's lines 7 to 10 belong to the function, the rest of the file to the script"
        );
    }
}
