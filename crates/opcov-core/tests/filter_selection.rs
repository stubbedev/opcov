#[cfg(test)]
#[expect(
    clippy::redundant_pub_crate,
    reason = "pub(super) is the intended visibility; the pub this lint suggests is denied by rustc unreachable_pub"
)]
mod support {
    use std::collections::BTreeMap;
    use std::io;
    use std::path::{Path, PathBuf};

    use opcov_core::filter::{CaseSensitivity, Selection, SelectionConfig};

    pub(super) fn no_listfiles() -> impl FnMut(&Path) -> Result<String, io::Error> {
        return |path| panic!("no listfile expected, asked for {}", path.display());
    }

    pub(super) fn identity() -> impl FnMut(&Path) -> Result<PathBuf, io::Error> {
        return |path| return Ok(path.to_path_buf());
    }

    pub(super) fn table<T: for<'a> From<&'a str>>(rows: &[(&str, &str)]) -> BTreeMap<PathBuf, T> {
        return rows
            .iter()
            .map(|&(path, value)| return (PathBuf::from(path), T::from(value)))
            .collect();
    }

    pub(super) fn lookup<T: Clone>(
        table: &BTreeMap<PathBuf, T>,
    ) -> impl FnMut(&Path) -> Result<T, io::Error> {
        return |path| {
            return table.get(path).cloned().ok_or_else(|| {
                return io::Error::new(io::ErrorKind::NotFound, "No such file or directory");
            });
        };
    }

    pub(super) fn selection_with(
        collect: &str,
        exclude: &str,
        realpath: impl FnMut(&Path) -> Result<PathBuf, io::Error>,
        case: CaseSensitivity,
    ) -> Selection {
        return SelectionConfig::parse(collect, exclude, no_listfiles())
            .unwrap()
            .resolve(realpath, case)
            .unwrap();
    }
}

#[cfg(test)]
mod ini_syntax {
    use std::io;
    use std::path::Path;

    use opcov_core::filter::{
        CaseSensitivity, FilterError, PathSetting, SelectionConfig, UnresolvedSelection,
    };

    use super::support::{identity, lookup, no_listfiles, table};

    fn parse_inline(collect: &str, exclude: &str) -> UnresolvedSelection {
        return SelectionConfig::parse(collect, exclude, no_listfiles()).unwrap();
    }

    fn entries(selection: &UnresolvedSelection) -> Vec<&str> {
        return selection
            .entries()
            .map(|path| return path.to_str().unwrap())
            .collect();
    }

    #[test]
    fn single_path() {
        assert_eq!(
            entries(&parse_inline("/srv/app", "")),
            ["/srv/app"],
            "one entry"
        );
    }

    #[test]
    fn colon_separated_paths_keep_order_collect_before_exclude() {
        let selection = parse_inline("/srv/app:/srv/lib/x.php", "/srv/app/vendor:/tmp");
        assert_eq!(
            entries(&selection),
            ["/srv/app", "/srv/lib/x.php", "/srv/app/vendor", "/tmp"],
            "collect entries, then exclude entries, each in configured order"
        );
    }

    #[test]
    fn leading_trailing_and_double_separators_are_ignored() {
        let selection = parse_inline("::/a::/b:", ":/c:");
        assert_eq!(
            entries(&selection),
            ["/a", "/b", "/c"],
            "empty segments skipped"
        );
    }

    #[test]
    fn whitespace_around_entries_is_trimmed() {
        let selection = parse_inline("  /a : /b  ", " ");
        assert_eq!(
            entries(&selection),
            ["/a", "/b"],
            "trimmed, blank exclude is empty"
        );
    }

    #[test]
    fn empty_collect_parses_and_resolves_to_an_empty_selection() {
        let selection = parse_inline("", "");
        assert_eq!(selection.entries().count(), 0, "no entries");
        let resolved = selection
            .resolve(identity(), CaseSensitivity::Sensitive)
            .unwrap();
        assert!(resolved.is_empty_collect(), "empty collect list");
    }

    #[test]
    fn listfile_with_blank_lines_and_crlf() {
        let files = table(&[
            (
                "/etc/collect.txt",
                "\r\n/srv/app\r\n\r\n   \r\n/srv/lib/x.php\r\n",
            ),
            ("/etc/exclude.txt", "/srv/app/vendor\n\n"),
        ]);
        let selection =
            SelectionConfig::parse("@/etc/collect.txt", "@ /etc/exclude.txt", lookup(&files))
                .unwrap();
        assert_eq!(
            entries(&selection),
            ["/srv/app", "/srv/lib/x.php", "/srv/app/vendor"],
            "one path per line, blank lines and CR dropped"
        );
    }

    #[test]
    fn listfile_lines_are_not_split_on_colons() {
        let files = table(&[("/l", "/a:b\n")]);
        let selection = SelectionConfig::parse("@/l", "", lookup(&files)).unwrap();
        assert_eq!(entries(&selection), ["/a:b"], "whole line is one path");
    }

    #[test]
    fn empty_listfile_is_an_error_naming_it() {
        let files = table(&[("/etc/empty.txt", "\n  \r\n")]);
        let error = SelectionConfig::parse("/a", "@/etc/empty.txt", lookup(&files)).unwrap_err();
        assert!(
            matches!(
                &error,
                FilterError::EmptyListfile { setting: PathSetting::Exclude, listfile }
                    if listfile == Path::new("/etc/empty.txt")
            ),
            "got {error:?}"
        );
        assert_eq!(
            error.to_string(),
            "opcov.exclude_paths: listfile /etc/empty.txt contains no paths",
            "message names the setting and the listfile"
        );
    }

    #[test]
    fn missing_listfile_is_an_error_naming_it_and_wrapping_the_io_error() {
        let files = table::<String>(&[]);
        let error = SelectionConfig::parse("@/etc/missing.txt", "", lookup(&files)).unwrap_err();
        assert!(
            matches!(
                &error,
                FilterError::ListfileUnreadable { setting: PathSetting::Collect, listfile, source }
                    if listfile == Path::new("/etc/missing.txt") && source.kind() == io::ErrorKind::NotFound
            ),
            "got {error:?}"
        );
        assert_eq!(
            error.to_string(),
            "opcov.collect_paths: cannot read listfile /etc/missing.txt: No such file or directory",
            "message names the setting, the listfile and the cause"
        );
        assert!(
            std::error::Error::source(&error).is_some(),
            "io error is the source"
        );
    }

    #[test]
    fn at_sign_without_a_listfile_is_an_error() {
        let error = SelectionConfig::parse(" @ ", "", no_listfiles()).unwrap_err();
        assert!(
            matches!(
                error,
                FilterError::MissingListfileName {
                    setting: PathSetting::Collect
                }
            ),
            "got {error:?}"
        );
    }

    #[test]
    fn ini_names() {
        assert_eq!(
            PathSetting::Collect.ini_name(),
            "opcov.collect_paths",
            "collect"
        );
        assert_eq!(
            PathSetting::Exclude.ini_name(),
            "opcov.exclude_paths",
            "exclude"
        );
    }
}

#[cfg(test)]
mod decisions {
    use std::io;
    use std::path::{Path, PathBuf};

    use opcov_core::filter::{
        CaseSensitivity, CompiledFile, Decision, FilterError, PathSetting, Selection,
        SelectionConfig, SkipReason,
    };

    use super::support::{identity, lookup, no_listfiles, selection_with, table};

    fn selection(collect: &str, exclude: &str) -> Selection {
        return selection_with(collect, exclude, identity(), CaseSensitivity::Sensitive);
    }

    fn decide(selection: &Selection, file: &str) -> Decision {
        return selection.decide(CompiledFile::Resolved(Path::new(file)), false);
    }

    fn excluded_by(entry: &str) -> Decision {
        return Decision::Skipped(SkipReason::ExcludedByPath(PathBuf::from(entry)));
    }

    const NOT_COLLECTED: Decision = Decision::Skipped(SkipReason::NotInCollectPaths);

    #[test]
    fn skip_reasons_describe_themselves() {
        let cases = [
            (SkipReason::NotInCollectPaths, "not in opcov.collect_paths"),
            (
                SkipReason::ExcludedByPath(PathBuf::from("/srv/app/gen")),
                "excluded by opcov.exclude_paths entry /srv/app/gen",
            ),
            (
                SkipReason::ExcludedByPattern,
                "matched by opcov.exclude_pattern",
            ),
            (
                SkipReason::StreamWrapper,
                "loaded through a stream wrapper, which is not selectable by path",
            ),
            (
                SkipReason::Unresolvable,
                "its path cannot be resolved to an existing UTF-8 file name",
            ),
        ];
        for (reason, text) in cases {
            assert_eq!(reason.to_string(), text, "{reason:?}");
        }
    }

    #[test]
    fn configured_entries_are_kept_as_written_with_listfiles_expanded() {
        let unresolved = SelectionConfig::parse(" app : src/a.php ", "@list", |_| {
            return Ok(String::from("gen\n\n  vendor \n"));
        })
        .unwrap();
        assert_eq!(
            unresolved.collect_entries(),
            ["app", "src/a.php"],
            "collect"
        );
        assert_eq!(unresolved.exclude_entries(), ["gen", "vendor"], "exclude");
    }

    #[test]
    fn directory_matches_by_prefix_with_a_separator_boundary() {
        let selection = selection("/srv/app", "");
        assert_eq!(
            decide(&selection, "/srv/app/x.php"),
            Decision::Selected,
            "direct child"
        );
        assert_eq!(
            decide(&selection, "/srv/app/a/b/c.php"),
            Decision::Selected,
            "deep child"
        );
        assert_eq!(
            decide(&selection, "/srv/application/x.php"),
            NOT_COLLECTED,
            "sibling prefix"
        );
        assert_eq!(decide(&selection, "/srv/ap"), NOT_COLLECTED, "shorter name");
        assert_eq!(
            decide(&selection, "/srv"),
            NOT_COLLECTED,
            "parent directory"
        );
    }

    #[test]
    fn trailing_separator_on_the_entry_does_not_change_matching() {
        let selection = selection("/srv/app/", "");
        assert_eq!(
            decide(&selection, "/srv/app/x.php"),
            Decision::Selected,
            "child"
        );
        assert_eq!(
            decide(&selection, "/srv/application/x.php"),
            NOT_COLLECTED,
            "sibling"
        );
    }

    #[test]
    fn file_entries_match_exactly() {
        let selection = selection("/srv/lib/x.php", "");
        assert_eq!(
            decide(&selection, "/srv/lib/x.php"),
            Decision::Selected,
            "the file"
        );
        assert_eq!(
            decide(&selection, "/srv/lib/x.php.bak"),
            NOT_COLLECTED,
            "longer name"
        );
        assert_eq!(
            decide(&selection, "/srv/lib/y.php"),
            NOT_COLLECTED,
            "other file"
        );
    }

    #[test]
    fn root_entry_selects_every_absolute_file() {
        let selection = selection("/", "");
        assert_eq!(
            decide(&selection, "/x.php"),
            Decision::Selected,
            "top level"
        );
        assert_eq!(
            decide(&selection, "/a/b/x.php"),
            Decision::Selected,
            "nested"
        );
    }

    #[test]
    fn empty_collect_selects_nothing() {
        let selection = selection("", "/srv/app/vendor");
        assert!(selection.is_empty_collect(), "empty collect");
        assert_eq!(
            decide(&selection, "/srv/app/x.php"),
            NOT_COLLECTED,
            "nothing selected"
        );
    }

    #[test]
    fn exclude_wins_over_collect_regardless_of_specificity() {
        let selection = selection("/srv/app/src/x.php:/srv/app", "/srv/app/src");
        assert_eq!(
            decide(&selection, "/srv/app/src/x.php"),
            excluded_by("/srv/app/src"),
            "a more specific collect entry does not override the exclusion"
        );
        assert_eq!(
            decide(&selection, "/srv/app/y.php"),
            Decision::Selected,
            "outside exclude"
        );
    }

    #[test]
    fn exclude_reports_the_shortest_matching_entry() {
        let selection = selection("/srv", "/srv/app/vendor:/srv/app");
        assert_eq!(
            decide(&selection, "/srv/app/vendor/x.php"),
            excluded_by("/srv/app"),
            "the shallowest exclude entry on the path"
        );
    }

    #[test]
    fn not_collected_takes_precedence_over_exclusion_reasons() {
        let selection = selection("/srv/app", "/srv/other");
        assert_eq!(
            selection.decide(CompiledFile::Resolved(Path::new("/srv/other/x.php")), true),
            NOT_COLLECTED,
            "a file outside the collect paths is reported as such"
        );
    }

    #[test]
    fn exclude_pattern_verdict_skips_collected_files() {
        let selection = selection("/srv/app", "");
        assert_eq!(
            selection.decide(CompiledFile::Resolved(Path::new("/srv/app/x.php")), true),
            Decision::Skipped(SkipReason::ExcludedByPattern),
            "pattern exclusion"
        );
    }

    #[test]
    fn path_exclusion_is_reported_before_pattern_exclusion() {
        let selection = selection("/srv/app", "/srv/app/vendor");
        assert_eq!(
            selection.decide(
                CompiledFile::Resolved(Path::new("/srv/app/vendor/x.php")),
                true
            ),
            excluded_by("/srv/app/vendor"),
            "path exclusion names its entry"
        );
    }

    #[test]
    fn eval_code_is_decided_by_its_host_file() {
        let selection = selection("/srv/app", "/srv/app/vendor");
        let decide_eval = |host: &str| {
            return selection.decide(
                CompiledFile::Eval {
                    host: Path::new(host),
                },
                false,
            );
        };
        assert_eq!(
            decide_eval("/srv/app/x.php"),
            Decision::Selected,
            "selected host"
        );
        assert_eq!(
            decide_eval("/srv/app/vendor/y.php"),
            excluded_by("/srv/app/vendor"),
            "excluded host"
        );
        assert_eq!(
            decide_eval("/srv/other.php"),
            NOT_COLLECTED,
            "unselected host"
        );
    }

    #[test]
    fn stream_wrappers_and_unresolvable_files_are_never_selected() {
        let selection = selection("/", "");
        assert_eq!(
            selection.decide(
                CompiledFile::StreamWrapper("phar:///srv/app.phar/x.php"),
                false
            ),
            Decision::Skipped(SkipReason::StreamWrapper),
            "phar"
        );
        assert_eq!(
            selection.decide(CompiledFile::StreamWrapper("vfs://root/x.php"), false),
            Decision::Skipped(SkipReason::StreamWrapper),
            "vfs"
        );
        assert_eq!(
            selection.decide(CompiledFile::Unresolvable("Command line code"), false),
            Decision::Skipped(SkipReason::Unresolvable),
            "unresolvable"
        );
    }

    #[test]
    fn case_sensitive_mode_distinguishes_case() {
        let selection = selection("/Srv/App", "");
        assert_eq!(
            decide(&selection, "/srv/app/x.php"),
            NOT_COLLECTED,
            "different case"
        );
        assert_eq!(
            decide(&selection, "/Srv/App/x.php"),
            Decision::Selected,
            "same case"
        );
    }

    #[test]
    fn case_insensitive_mode_folds_ascii_case() {
        let selection = selection_with(
            "/Srv/App",
            "/srv/app/VENDOR",
            identity(),
            CaseSensitivity::Insensitive,
        );
        assert_eq!(
            decide(&selection, "/SRV/app/X.php"),
            Decision::Selected,
            "folded collect"
        );
        assert_eq!(
            decide(&selection, "/srv/APP/Vendor/x.php"),
            excluded_by("/srv/app/VENDOR"),
            "folded exclude, reported with its resolved spelling"
        );
        assert_eq!(
            decide(&selection, "/srv/application/x.php"),
            NOT_COLLECTED,
            "boundary kept"
        );
    }

    #[test]
    fn case_insensitive_mode_does_not_fold_non_ascii() {
        let selection = selection_with("/srv/Äpp", "", identity(), CaseSensitivity::Insensitive);
        assert_eq!(
            decide(&selection, "/srv/äpp/x.php"),
            NOT_COLLECTED,
            "non-ASCII kept"
        );
        assert_eq!(
            decide(&selection, "/SRV/Äpp/x.php"),
            Decision::Selected,
            "ASCII folded"
        );
    }

    #[test]
    fn symlinks_and_parent_segments_are_resolved_through_the_callers_realpath() {
        let filesystem = table(&[
            ("/home/me/project", "/srv/app"),
            ("/srv/app/../app/vendor", "/srv/app/vendor"),
        ]);
        let selection = selection_with(
            "/home/me/project",
            "/srv/app/../app/vendor",
            lookup(&filesystem),
            CaseSensitivity::Sensitive,
        );
        assert_eq!(
            decide(&selection, "/srv/app/x.php"),
            Decision::Selected,
            "symlink target"
        );
        assert_eq!(
            decide(&selection, "/srv/app/vendor/y.php"),
            excluded_by("/srv/app/vendor"),
            "resolved exclude"
        );
        assert_eq!(
            decide(&selection, "/home/me/project/x.php"),
            NOT_COLLECTED,
            "link name itself"
        );
    }

    #[test]
    fn the_same_directory_under_two_paths_resolves_to_one_entry() {
        let filesystem = table(&[("/srv/app", "/srv/app"), ("/var/www", "/srv/app")]);
        let selection = selection_with(
            "/srv/app:/var/www",
            "",
            lookup(&filesystem),
            CaseSensitivity::Sensitive,
        );
        let deduplicated = selection_with("/srv/app", "", identity(), CaseSensitivity::Sensitive);
        assert_eq!(selection, deduplicated, "both entries collapse into one");
        assert_eq!(
            decide(&selection, "/srv/app/x.php"),
            Decision::Selected,
            "resolved file"
        );
    }

    #[test]
    fn unresolvable_entry_is_an_error_naming_it() {
        let filesystem = table(&[("/srv/app", "/srv/app")]);
        let error = SelectionConfig::parse("/srv/app", "/srv/gone", no_listfiles())
            .unwrap()
            .resolve(lookup(&filesystem), CaseSensitivity::Sensitive)
            .unwrap_err();
        assert!(
            matches!(
                &error,
                FilterError::EntryUnresolvable { setting: PathSetting::Exclude, entry, source }
                    if entry == Path::new("/srv/gone") && source.kind() == io::ErrorKind::NotFound
            ),
            "got {error:?}"
        );
        assert_eq!(
            error.to_string(),
            "opcov.exclude_paths: cannot resolve /srv/gone: No such file or directory",
            "message names the setting, the entry and the cause"
        );
    }

    #[test]
    fn realpath_returning_a_non_canonical_path_is_an_error() {
        for resolved in ["relative/app", "/srv/../app"] {
            let error = SelectionConfig::parse("app", "", no_listfiles())
                .unwrap()
                .resolve(
                    |_| return Ok(PathBuf::from(resolved)),
                    CaseSensitivity::Sensitive,
                )
                .unwrap_err();
            assert!(
                matches!(
                    &error,
                    FilterError::NotCanonical { setting: PathSetting::Collect, entry, resolved: got }
                        if entry == Path::new("app") && got == Path::new(resolved)
                ),
                "{resolved}: got {error:?}"
            );
        }
    }
}

#[cfg(test)]
mod reference_agreement {
    use std::path::{Path, PathBuf};

    use opcov_core::filter::{CaseSensitivity, CompiledFile, Decision, SkipReason};
    use proptest::prelude::*;

    use super::support::{identity, selection_with};

    fn path() -> impl Strategy<Value = String> {
        let component = prop::sample::select(vec!["a", "A", "b", "ab", "aB", "b.php", "B.PHP"]);
        return prop::collection::vec(component, 0..4)
            .prop_map(|components| return format!("/{}", components.join("/")));
    }

    fn case() -> impl Strategy<Value = CaseSensitivity> {
        return prop_oneof![
            Just(CaseSensitivity::Sensitive),
            Just(CaseSensitivity::Insensitive)
        ];
    }

    fn normalized(path: &str, case: CaseSensitivity) -> String {
        let trimmed = path.trim_end_matches('/');
        return match case {
            CaseSensitivity::Sensitive => trimmed.to_owned(),
            CaseSensitivity::Insensitive => trimmed.to_ascii_lowercase(),
        };
    }

    fn contains(entry: &str, file: &str, case: CaseSensitivity) -> bool {
        let entry = normalized(entry, case);
        let file = normalized(file, case);
        return file == entry || file.starts_with(&format!("{entry}/"));
    }

    fn depth(path: &str) -> usize {
        return path
            .split('/')
            .filter(|segment| return !segment.is_empty())
            .count();
    }

    fn reference(
        collect: &[String],
        exclude: &[String],
        file: &str,
        pattern_excluded: bool,
        case: CaseSensitivity,
    ) -> Decision {
        if !collect
            .iter()
            .any(|entry| return contains(entry, file, case))
        {
            return Decision::Skipped(SkipReason::NotInCollectPaths);
        }
        let shortest = exclude
            .iter()
            .filter(|entry| return contains(entry, file, case))
            .min_by_key(|entry| return depth(entry));
        if let Some(entry) = shortest {
            return Decision::Skipped(SkipReason::ExcludedByPath(PathBuf::from(entry)));
        }
        if pattern_excluded {
            return Decision::Skipped(SkipReason::ExcludedByPattern);
        }
        return Decision::Selected;
    }

    proptest! {
        #[test]
        fn decide_agrees_with_the_naive_reference(
            collect in prop::collection::vec(path(), 0..5),
            exclude in prop::collection::vec(path(), 0..5),
            files in prop::collection::vec(path(), 1..10),
            pattern_excluded in any::<bool>(),
            case in case(),
        ) {
            let selection = selection_with(&collect.join(":"), &exclude.join(":"), identity(), case);
            prop_assert_eq!(selection.is_empty_collect(), collect.is_empty(), "empty collect");
            for file in &files {
                prop_assert_eq!(
                    selection.decide(CompiledFile::Resolved(Path::new(file)), pattern_excluded),
                    reference(&collect, &exclude, file, pattern_excluded, case),
                    "file {}", file
                );
            }
        }
    }
}
