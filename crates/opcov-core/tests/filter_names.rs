#[cfg(test)]
mod reported_names {
    use opcov_core::filter::{eval_host, stream_scheme};

    #[test]
    fn eval_code_names_yield_the_host_file() {
        assert_eq!(
            eval_host("/srv/app/x.php(12) : eval()'d code"),
            Some("/srv/app/x.php"),
            "eval"
        );
    }

    #[test]
    fn every_engine_description_yields_the_host_file() {
        for description in ["eval()'d code", "mbregex replace", "highlighted code"] {
            let reported = format!("/srv/app/x.php(7) : {description}");
            assert_eq!(eval_host(&reported), Some("/srv/app/x.php"), "{reported}");
        }
    }

    #[test]
    fn nested_eval_yields_the_outermost_file() {
        assert_eq!(
            eval_host("/srv/app/x.php(3) : eval()'d code(1) : eval()'d code"),
            Some("/srv/app/x.php"),
            "eval inside eval"
        );
    }

    #[test]
    fn host_names_containing_parentheses_are_kept_whole() {
        assert_eq!(
            eval_host("/srv/a (copy)/x(1).php(4) : eval()'d code"),
            Some("/srv/a (copy)/x(1).php"),
            "only the trailing location is split off"
        );
    }

    #[test]
    fn eval_outside_execution_names_the_unknown_host() {
        assert_eq!(eval_host("Unknown(0) : eval()'d code"), Some("Unknown"), "engine fallback");
    }

    #[test]
    fn names_without_a_host_location_are_not_eval_code() {
        for reported in [
            "Command line code",
            "Command line begin code",
            "php shell code",
            "Standard input code",
            "/srv/app/x.php",
            "/srv/app/x.php(a) : eval()'d code",
            "/srv/app/x.php() : eval()'d code",
            "(3) : eval()'d code",
            "/srv/app/x.php(3) : ",
            "/srv/app/x.php(3): eval()'d code",
        ] {
            assert_eq!(eval_host(reported), None, "{reported}");
        }
    }

    #[test]
    fn stream_wrapper_schemes() {
        for (path, scheme) in [
            ("phar:///srv/app.phar/x.php", "phar"),
            ("vfs://root/x.php", "vfs"),
            ("compress.zlib://x.php.gz", "compress.zlib"),
            ("php+x-1://y", "php+x-1"),
            ("file:///srv/app/x.php", "file"),
            ("data:text/plain,<?php", "data"),
            ("data://text/plain,<?php", "data"),
        ] {
            assert_eq!(stream_scheme(path), Some(scheme), "{path}");
        }
    }

    #[test]
    fn plain_paths_have_no_scheme() {
        for path in [
            "/srv/app/x.php",
            "relative/x.php",
            "c://x.php",
            "C:\\x.php",
            "a:b",
            "phar:/x",
            "datum:x",
            "://x",
            "ph ar://x",
            "",
        ] {
            assert_eq!(stream_scheme(path), None, "{path}");
        }
    }
}
