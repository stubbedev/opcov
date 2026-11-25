#[cfg(test)]
mod outline {
    use opcov_core::cfg::Line;
    use opcov_syntax::{Outline, SourceFile};

    const BASE: &str = "<?php\n\nnamespace App;\n\nclass Foo\n{\n    public const LIMIT = 3;\n\n    public function bar(int $x): int\n    {\n        return $x + 1;\n    }\n\n    public function baz(): array\n    {\n        return [fn () => 1];\n    }\n}\n";

    fn outline(source: &str) -> Outline {
        let (_, outline) =
            SourceFile::parse_outlined("app/Foo.php".parse().unwrap(), source.as_bytes()).unwrap();
        return outline;
    }

    fn line(number: u32) -> Option<Line> {
        return Line::new(number);
    }

    #[test]
    fn function_bodies_do_not_count() {
        let changed = BASE.replace("return $x + 1;", "$y = $x * 2;\n        return $y;");
        assert_eq!(outline(BASE), outline(&changed), "a body edit");
    }

    #[test]
    fn whitespace_and_comments_do_not_count() {
        let changed = BASE
            .replace("class Foo\n{", "// the foo\nclass   Foo\n{\n    # note\n")
            .replace("public const", "/* limit */ public const");
        assert_eq!(outline(BASE), outline(&changed), "trivia only");
    }

    #[test]
    fn a_shifted_closure_keeps_its_place() {
        let changed = BASE.replace("return [fn () => 1];", "\n\n        return [fn () => 2];");
        assert_eq!(outline(BASE), outline(&changed), "closure lines move");
    }

    #[test]
    fn doc_comments_count() {
        let changed = BASE.replace(
            "    public function bar",
            "    /** @return int */\n    public function bar",
        );
        let divergence = outline(BASE).diverges_from(&outline(&changed)).unwrap();
        assert_eq!(
            divergence.here(),
            line(9),
            "the method placeholder on the base"
        );
        assert_eq!(divergence.there(), line(9), "the doc comment on the change");
    }

    #[test]
    fn a_constant_counts() {
        let changed = BASE.replace("LIMIT = 3", "LIMIT = 4");
        let divergence = outline(BASE).diverges_from(&outline(&changed)).unwrap();
        assert_eq!(divergence.here(), line(7), "the value");
        assert_eq!(divergence.there(), line(7), "the value");
    }

    #[test]
    fn an_added_method_counts() {
        let changed = BASE.replace(
            "\n}\n",
            "\n\n    public function qux(): void\n    {\n    }\n}\n",
        );
        let divergence = outline(BASE).diverges_from(&outline(&changed)).unwrap();
        assert_eq!(divergence.here(), line(18), "the closing brace on the base");
        assert_eq!(divergence.there(), line(19), "the new method");
    }

    #[test]
    fn a_renamed_method_counts() {
        let changed = BASE.replace("function baz", "function qux");
        assert_ne!(
            outline(BASE),
            outline(&changed),
            "the placeholder names the key"
        );
    }
}
