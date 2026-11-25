#[cfg(test)]
mod consumers {
    use std::path::Path;
    use std::sync::OnceLock;

    use opcov_php::{Index, Project, Request};

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
}

#[cfg(test)]
mod tree {
    use std::path::Path;
    use std::sync::OnceLock;

    use opcov_php::{Changed, FileChange, Index, Project};

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

    fn changed(ranges: Vec<(u32, u32)>, deletions: Vec<u32>) -> Vec<String> {
        return index()
            .changed_members(&change(ranges, deletions))
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
        let members = index().changed_members(&change(vec![(18, 18)], vec![]));
        let tree = index().consumers(members, &[], &[], 2);
        let consumers: Vec<String> = tree
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
