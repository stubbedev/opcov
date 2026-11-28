#[cfg(test)]
#[path = "support/fixture.rs"]
mod fixture;

#[cfg(test)]
mod consumers {
    use std::path::Path;
    use std::sync::OnceLock;

    use opcov_php::{Index, Project, Request, Resolution};

    use crate::fixture::project_copy;

    fn index() -> &'static Index {
        static INDEX: OnceLock<Index> = OnceLock::new();
        return INDEX.get_or_init(|| {
            let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/project");
            return Index::build(&Project::open(&root).unwrap()).unwrap();
        });
    }

    fn uses(request: &str) -> Vec<String> {
        let request: Request = serde_json::from_str(request).unwrap();
        return index()
            .uses(&request)
            .uses
            .into_iter()
            .map(|found| return format!("{}:{} {}", found.path, found.line, found.target))
            .collect();
    }

    #[test]
    fn members_are_matched_by_the_receivers_type() {
        let found = uses(
            r#"{"targets": ["method App\\Services\\Report::render", "method App\\Services\\Report::__construct", "constant App\\Services\\Report::LIMIT", "property App\\Services\\Report::title"]}"#,
        );
        assert_eq!(
            found,
            [
                "app/Uses/Receivers.php:15 method App\\Services\\Report::render",
                "app/Uses/Receivers.php:16 method App\\Services\\Report::render",
                "app/Uses/Receivers.php:17 method App\\Services\\Report::render",
                "app/Uses/Receivers.php:19 method App\\Services\\Report::render",
                "app/Uses/Receivers.php:21 method App\\Services\\Report::render",
                "app/Uses/Receivers.php:22 method App\\Services\\Report::__construct",
                "app/Uses/Receivers.php:23 constant App\\Services\\Report::LIMIT",
                "app/Uses/Receivers.php:23 property App\\Services\\Report::title",
            ],
            "the target, a nullable, an inheriting class, a supertype and an unknown receiver reach it; an override and an unrelated class do not"
        );
    }

    #[test]
    fn a_file_is_analysed_only_when_it_names_a_target_as_a_whole_word() {
        let request: Request = serde_json::from_str(
            r#"{"targets": ["method App\\Services\\Report::render"], "within": ["app/Uses/Lookalike.php"]}"#,
        )
        .unwrap();
        let search = index().uses(&request);
        assert_eq!(
            (search.analysed, search.uses.len()),
            (0, 0),
            "renderer, prerender and render_all contain render but do not name it"
        );
    }

    #[test]
    fn code_the_framework_calls_is_used_where_it_is_triggered() {
        let found = uses(
            r#"{"targets": ["method App\\Actions\\Publish::handle", "method App\\Jobs\\SendDigest::handle", "method App\\Jobs\\SendDigest::failed", "method App\\Jobs\\PlainHandler::handle", "method App\\Listeners\\PostSubscriber::onPublished"]}"#,
        );
        assert_eq!(
            found,
            [
                "app/Actions/Runs.php:9 method App\\Actions\\Publish::handle via App\\Actions\\Publish",
                "app/Uses/Framework.php:17 method App\\Jobs\\SendDigest::failed",
                "app/Uses/Framework.php:17 method App\\Jobs\\SendDigest::handle",
                "app/Uses/Framework.php:18 method App\\Jobs\\SendDigest::failed",
                "app/Uses/Framework.php:18 method App\\Jobs\\SendDigest::handle",
                "app/Uses/Framework.php:20 method App\\Listeners\\PostSubscriber::onPublished",
                "app/Uses/Framework.php:21 method App\\Listeners\\PostSubscriber::onPublished",
            ],
            "a trait's copy in the target's class, a dispatched job and a created event reach their handlers; a plain class with handle does not"
        );
    }

    #[test]
    fn eloquent_chains_and_model_events_are_typed_through_builders_scopes_and_relations() {
        let found = uses(
            r#"{"targets": ["method App\\Models\\Post::publish", "method App\\Models\\Comment::approve", "method App\\Observers\\PostObserver::created", "method App\\Models\\MakesSlug::bootMakesSlug", "method App\\Listeners\\CommentSubscriber::onCreated"]}"#,
        );
        assert_eq!(
            found,
            [
                "app/Uses/Eloquent.php:12 method App\\Models\\Post::publish",
                "app/Uses/Eloquent.php:13 method App\\Models\\Post::publish",
                "app/Uses/Eloquent.php:14 method App\\Models\\Post::publish",
                "app/Uses/Eloquent.php:15 method App\\Models\\Comment::approve",
                "app/Uses/Eloquent.php:16 method App\\Models\\Comment::approve",
                "app/Uses/Eloquent.php:17 method App\\Models\\Comment::approve",
                "app/Uses/Eloquent.php:18 method App\\Observers\\PostObserver::created",
                "app/Uses/Eloquent.php:19 method App\\Observers\\PostObserver::created",
                "app/Uses/Eloquent.php:20 method App\\Listeners\\CommentSubscriber::onCreated",
                "app/Uses/Eloquent.php:20 method App\\Models\\MakesSlug::bootMakesSlug",
            ],
            "static forwarding to a custom builder, dynamic wheres, query builder mixins, scopes, relation methods and properties all keep the model; an observer, a trait's boot closure and a dispatched event's subscriber fire on their own model's calls only"
        );
    }

    #[test]
    fn a_builder_method_is_used_through_its_models_static_forwarding() {
        let found = uses(r#"{"targets": ["method App\\Models\\PostBuilder::published"]}"#);
        assert_eq!(
            found,
            [
                "app/Uses/Eloquent.php:12 method App\\Models\\PostBuilder::published",
                "app/Uses/Eloquent.php:14 method App\\Models\\PostBuilder::published",
                "app/Uses/Eloquent.php:19 method App\\Models\\PostBuilder::published",
                "app/Uses/Forwarded.php:11 method App\\Models\\PostBuilder::published",
            ],
            "calls on the builder and a static call on the model both reach the builder's method"
        );
    }

    #[test]
    fn facades_aliases_and_commands_resolve_to_their_classes() {
        let found = uses(
            r#"{"targets": ["method App\\Services\\Clock::now", "method App\\Console\\Reindex::handle"]}"#,
        );
        assert_eq!(
            found,
            [
                "app/Uses/Facades.php:14 method App\\Services\\Clock::now",
                "app/Uses/Facades.php:15 method App\\Console\\Reindex::handle",
                "app/Uses/Facades.php:16 method App\\Console\\Reindex::handle",
                "app/Uses/Facades.php:17 method App\\Console\\Reindex::handle",
            ],
            "a facade reaches its container binding, the root alias reaches Artisan, and a command is run by name or class but not by another name"
        );
    }

    #[test]
    fn the_search_can_be_limited_to_paths() {
        let found = uses(
            r#"{"targets": ["method App\\Services\\Report::render"], "within": ["app/Actions"]}"#,
        );
        assert!(
            found.is_empty(),
            "no use of render sits under app/Actions: {found:?}"
        );
    }

    #[test]
    fn a_variable_named_like_a_target_is_no_mention_but_a_static_property_is() {
        let root = project_copy("consumers-variables");
        std::fs::write(
            root.join("app/Services/Settings.php"),
            "<?php\n\nnamespace App\\Services;\n\nfinal class Settings\n{\n    public static string $zone = 'UTC';\n}\n",
        )
        .unwrap();
        std::fs::write(
            root.join("app/Uses/Variables.php"),
            "<?php\n\nnamespace App\\Uses;\n\nfinal class Variables\n{\n    public function local(): string\n    {\n        $zone = 'local';\n        return $zone;\n    }\n}\n",
        )
        .unwrap();
        std::fs::write(
            root.join("app/Uses/StaticProperty.php"),
            "<?php\n\nnamespace App\\Uses;\n\nuse App\\Services\\Settings;\n\nfinal class StaticProperty\n{\n    public function read(): string\n    {\n        return Settings:: $zone;\n    }\n}\n",
        )
        .unwrap();
        let index = Index::build(&Project::open(&root).unwrap()).unwrap();
        let request: Request = serde_json::from_str(
            r#"{"targets": ["property App\\Services\\Settings::zone"], "within": ["app/Uses/Variables.php", "app/Uses/StaticProperty.php"]}"#,
        )
        .unwrap();
        let search = index.uses(&request);
        let found: Vec<String> = search
            .uses
            .iter()
            .map(|found| return format!("{}:{} {}", found.path, found.line, found.target))
            .collect();
        assert_eq!(
            (search.analysed, found),
            (
                1,
                vec![
                    "app/Uses/StaticProperty.php:11 property App\\Services\\Settings::zone"
                        .to_owned()
                ]
            ),
            "the $zone variable leaves Variables.php unanalysed; Settings:: $zone reads the static property"
        );
    }

    #[test]
    fn a_property_read_on_this_resolves_to_the_class_it_runs_in() {
        let root = project_copy("consumers-this-property");
        let write = |path: &str, source: &str| {
            std::fs::write(root.join(path), source).unwrap();
        };
        std::fs::create_dir_all(root.join("app/Owners")).unwrap();
        write(
            "app/Owners/Account.php",
            "<?php\n\nnamespace App\\Owners;\n\nfinal class Account\n{\n    public int $id = 1;\n}\n",
        );
        write(
            "app/Owners/Other.php",
            "<?php\n\nnamespace App\\Owners;\n\nfinal class Other\n{\n    public ?Account $account = null;\n}\n",
        );
        write(
            "app/Owners/Holder.php",
            "<?php\n\nnamespace App\\Owners;\n\nfinal class Holder\n{\n    private Account $account;\n\n    public function id(): int\n    {\n        return $this->account->id + $this->account->id;\n    }\n}\n",
        );
        let index = Index::build(&Project::open(&root).unwrap()).unwrap();
        let request: Request = serde_json::from_str(
            r#"{"targets": ["property App\\Owners\\Holder::account", "property App\\Owners\\Other::account"], "within": ["app/Owners/Holder.php"]}"#,
        )
        .unwrap();
        let found: Vec<(String, Resolution)> = index
            .uses(&request)
            .uses
            .into_iter()
            .map(|found| {
                return (
                    format!("{}:{} {}", found.path, found.line, found.target),
                    found.resolution,
                );
            })
            .collect();
        assert_eq!(
            found,
            vec![(
                "app/Owners/Holder.php:11 property App\\Owners\\Holder::account".to_owned(),
                Resolution::Typed
            )],
            "$this->account in Holder reads Holder::account, never Other::account"
        );
    }

    #[test]
    fn a_receiver_typed_partly_as_any_object_matches_the_rest_by_name() {
        let root = project_copy("consumers-open-union");
        let write = |path: &str, source: &str| {
            std::fs::write(root.join(path), source).unwrap();
        };
        std::fs::create_dir_all(root.join("app/Pings")).unwrap();
        for class in ["Account", "Other"] {
            write(
                &format!("app/Pings/{class}.php"),
                &format!(
                    "<?php\n\nnamespace App\\Pings;\n\nfinal class {class}\n{{\n    public function ping(): int\n    {{\n        return 1;\n    }}\n}}\n"
                ),
            );
        }
        write(
            "app/Pings/Caller.php",
            "<?php\n\nnamespace App\\Pings;\n\nfinal class Caller\n{\n    /**\n     * @param Account|iterable<mixed> $value\n     */\n    public function call(Account|iterable $value): int\n    {\n        return $value->ping();\n    }\n}\n",
        );
        let index = Index::build(&Project::open(&root).unwrap()).unwrap();
        let request: Request = serde_json::from_str(
            r#"{"targets": ["method App\\Pings\\Account::ping", "method App\\Pings\\Other::ping"], "within": ["app/Pings/Caller.php"]}"#,
        )
        .unwrap();
        let found: Vec<(String, Resolution)> = index
            .uses(&request)
            .uses
            .into_iter()
            .map(|found| {
                return (
                    format!("{}:{} {}", found.path, found.line, found.target),
                    found.resolution,
                );
            })
            .collect();
        assert_eq!(
            found,
            vec![
                (
                    "app/Pings/Caller.php:12 method App\\Pings\\Account::ping".to_owned(),
                    Resolution::Typed
                ),
                (
                    "app/Pings/Caller.php:12 method App\\Pings\\Other::ping".to_owned(),
                    Resolution::ByName
                ),
            ],
            "Account is proven by the type; the iterable may be any Traversable, so an Other too"
        );
    }

    #[test]
    fn a_class_named_call_binds_to_the_member_its_lookup_finds_and_static_dispatches_late() {
        let root = project_copy("consumers-binding");
        let write = |path: &str, source: &str| {
            std::fs::write(root.join(path), source).unwrap();
        };
        std::fs::create_dir_all(root.join("app/Binding")).unwrap();
        write(
            "app/Binding/Base.php",
            "<?php\n\nnamespace App\\Binding;\n\nclass Base\n{\n    public const LIMIT = 1;\n\n    public function __construct()\n    {\n    }\n\n    public static function make(): static\n    {\n        return new static();\n    }\n\n    public static function build(): self\n    {\n        return static::make();\n    }\n\n    public function limit(): int\n    {\n        return self::LIMIT;\n    }\n}\n",
        );
        write(
            "app/Binding/Child.php",
            "<?php\n\nnamespace App\\Binding;\n\nfinal class Child extends Base\n{\n    public const LIMIT = 2;\n\n    public function __construct()\n    {\n        parent::__construct();\n    }\n\n    public static function make(): static\n    {\n        return new static();\n    }\n\n    public function own(): int\n    {\n        return self::LIMIT + parent::LIMIT;\n    }\n}\n",
        );
        write(
            "app/Binding/Sibling.php",
            "<?php\n\nnamespace App\\Binding;\n\nfinal class Sibling extends Base\n{\n    public function __construct()\n    {\n        parent::__construct();\n    }\n}\n",
        );
        write(
            "app/Binding/Maker.php",
            "<?php\n\nnamespace App\\Binding;\n\nfinal class Maker\n{\n    public function plain(): Base\n    {\n        return new Base();\n    }\n}\n",
        );
        let index = Index::build(&Project::open(&root).unwrap()).unwrap();
        let request: Request = serde_json::from_str(
            r#"{"targets": [
                "method App\\Binding\\Base::__construct", "method App\\Binding\\Child::__construct", "method App\\Binding\\Sibling::__construct",
                "method App\\Binding\\Child::make", "constant App\\Binding\\Base::LIMIT", "constant App\\Binding\\Child::LIMIT"
            ], "within": ["app/Binding"]}"#,
        )
        .unwrap();
        let found: Vec<String> = index
            .uses(&request)
            .uses
            .into_iter()
            .map(|found| return format!("{}:{} {}", found.path, found.line, found.target))
            .collect();
        assert_eq!(
            found,
            [
                "app/Binding/Base.php:15 method App\\Binding\\Base::__construct",
                "app/Binding/Base.php:15 method App\\Binding\\Child::__construct",
                "app/Binding/Base.php:15 method App\\Binding\\Sibling::__construct",
                "app/Binding/Base.php:20 method App\\Binding\\Child::make",
                "app/Binding/Base.php:25 constant App\\Binding\\Base::LIMIT",
                "app/Binding/Child.php:11 method App\\Binding\\Base::__construct",
                "app/Binding/Child.php:16 method App\\Binding\\Child::__construct",
                "app/Binding/Child.php:21 constant App\\Binding\\Base::LIMIT",
                "app/Binding/Child.php:21 constant App\\Binding\\Child::LIMIT",
                "app/Binding/Maker.php:9 method App\\Binding\\Base::__construct",
                "app/Binding/Sibling.php:9 method App\\Binding\\Base::__construct",
            ],
            "parent:: and new Base bind to Base's constructor alone; new static and static::make reach the overrides; \
             self::LIMIT reads the nearest declaration, Child's own in Child and Base's in Base"
        );
    }

    #[test]
    fn a_receiver_narrowed_by_a_structural_check_stays_its_class() {
        let root = project_copy("consumers-narrowed");
        let write = |path: &str, source: &str| {
            std::fs::write(root.join(path), source).unwrap();
        };
        std::fs::create_dir_all(root.join("app/Pings")).unwrap();
        for class in ["Account", "Other"] {
            write(
                &format!("app/Pings/{class}.php"),
                &format!(
                    "<?php\n\nnamespace App\\Pings;\n\nclass {class}\n{{\n    public function ping(): int\n    {{\n        return 1;\n    }}\n}}\n"
                ),
            );
        }
        write(
            "app/Pings/Checker.php",
            "<?php\n\nnamespace App\\Pings;\n\nfinal class Checker\n{\n    public function call(Account $value): int\n    {\n        if (method_exists($value, 'extra')) {\n            return $value->ping();\n        }\n        return 0;\n    }\n}\n",
        );
        let index = Index::build(&Project::open(&root).unwrap()).unwrap();
        let request: Request = serde_json::from_str(
            r#"{"targets": ["method App\\Pings\\Account::ping", "method App\\Pings\\Other::ping"], "within": ["app/Pings/Checker.php"]}"#,
        )
        .unwrap();
        let found: Vec<(String, Resolution)> = index
            .uses(&request)
            .uses
            .into_iter()
            .map(|found| {
                return (
                    format!("{}:{} {}", found.path, found.line, found.target),
                    found.resolution,
                );
            })
            .collect();
        assert_eq!(
            found,
            vec![(
                "app/Pings/Checker.php:10 method App\\Pings\\Account::ping".to_owned(),
                Resolution::Typed
            )],
            "method_exists narrows the Account to an Account that has extra, which is still no Other"
        );
    }

    #[test]
    fn parent_in_a_trait_resolves_to_the_parent_of_each_using_class() {
        let root = project_copy("consumers-trait-parent");
        let write = |path: &str, source: &str| {
            std::fs::write(root.join(path), source).unwrap();
        };
        std::fs::create_dir_all(root.join("app/Shapes")).unwrap();
        write(
            "app/Shapes/Base.php",
            "<?php\n\nnamespace App\\Shapes;\n\nclass Base\n{\n    public function toArray(): array\n    {\n        return [];\n    }\n}\n",
        );
        write(
            "app/Shapes/Unrelated.php",
            "<?php\n\nnamespace App\\Shapes;\n\nfinal class Unrelated\n{\n    public function toArray(): array\n    {\n        return [];\n    }\n}\n",
        );
        write(
            "app/Shapes/Camel.php",
            "<?php\n\nnamespace App\\Shapes;\n\ntrait Camel\n{\n    public function toArray(): array\n    {\n        return parent::toArray();\n    }\n}\n",
        );
        write(
            "app/Shapes/Child.php",
            "<?php\n\nnamespace App\\Shapes;\n\nfinal class Child extends Base\n{\n    use Camel;\n}\n",
        );
        let index = Index::build(&Project::open(&root).unwrap()).unwrap();
        let request: Request = serde_json::from_str(
            r#"{"targets": ["method App\\Shapes\\Base::toArray", "method App\\Shapes\\Unrelated::toArray"], "within": ["app/Shapes/Camel.php"]}"#,
        )
        .unwrap();
        let found: Vec<(String, Resolution)> = index
            .uses(&request)
            .uses
            .into_iter()
            .map(|found| {
                return (
                    format!("{}:{} {}", found.path, found.line, found.target),
                    found.resolution,
                );
            })
            .collect();
        assert_eq!(
            found,
            vec![(
                "app/Shapes/Camel.php:9 method App\\Shapes\\Base::toArray via App\\Shapes\\Child"
                    .to_owned(),
                Resolution::Typed
            )],
            "parent:: in Camel is Base for its user Child, and never Unrelated"
        );
    }
}

#[cfg(test)]
mod tree {
    use std::path::Path;
    use std::sync::OnceLock;

    use opcov_php::{Changed, FileChange, Index, Project, Query, Request, Tree};

    fn index() -> &'static Index {
        static INDEX: OnceLock<Index> = OnceLock::new();
        return INDEX.get_or_init(|| {
            let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/project");
            return Index::build(&Project::open(&root).unwrap()).unwrap();
        });
    }

    fn change(ranges: Vec<(u32, u32)>, deletions: Vec<u32>) -> Vec<FileChange> {
        return vec![FileChange {
            path: "app/Actions/Runs.php".to_owned(),
            changed: Changed::Parts { ranges, deletions },
        }];
    }

    fn tree(ranges: Vec<(u32, u32)>, deletions: Vec<u32>, depth: u32) -> Tree {
        return index()
            .answer(&Query {
                changes: change(ranges, deletions),
                request: Request::default(),
                depth,
            })
            .tree;
    }

    fn changed(ranges: Vec<(u32, u32)>, deletions: Vec<u32>) -> Vec<String> {
        return tree(ranges, deletions, 0)
            .changed
            .into_iter()
            .map(|member| {
                return format!(
                    "{}:{}-{} {}",
                    member.path, member.first, member.last, member.target
                );
            })
            .collect();
    }

    #[test]
    fn a_change_names_the_members_it_touches() {
        assert_eq!(
            changed(vec![(18, 18)], vec![]),
            ["app/Actions/Runs.php:17-19 method App\\Actions\\Publish::handle"],
            "a line inside the method"
        );
        assert_eq!(
            changed(vec![], vec![18]),
            ["app/Actions/Runs.php:17-19 method App\\Actions\\Publish::handle"],
            "lines deleted inside the method"
        );
        assert!(
            changed(vec![], vec![17]).is_empty(),
            "lines deleted before the method's first line are outside it"
        );
        assert_eq!(
            changed(vec![(9, 18)], vec![]),
            [
                "app/Actions/Runs.php:7-10 method App\\Actions\\Runs::run",
                "app/Actions/Runs.php:17-19 method App\\Actions\\Publish::handle"
            ],
            "a range across two members"
        );
    }

    #[test]
    fn consumers_follow_the_call_graph_through_trait_copies() {
        let consumers: Vec<String> = tree(vec![(18, 18)], vec![], 2)
            .consumers
            .iter()
            .map(|consumer| {
                let uses: Vec<&str> = consumer.uses.iter().map(String::as_str).collect();
                return format!(
                    "{} {}:{}-{} {} uses {}",
                    consumer.depth,
                    consumer.path,
                    consumer.first,
                    consumer.last,
                    consumer.member.as_deref().unwrap_or("-"),
                    uses.join(", ")
                );
            })
            .collect();
        assert_eq!(
            consumers,
            [
                "1 app/Actions/Runs.php:7-10 method App\\Actions\\Publish::run uses method App\\Actions\\Publish::handle via App\\Actions\\Publish",
                "2 app/Uses/Framework.php:13-22 method App\\Uses\\Framework::run uses method App\\Actions\\Publish::run",
            ],
            "the trait's run is Publish's copy, and its caller is the next level"
        );
    }
}
