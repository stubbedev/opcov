use std::borrow::Cow;
use std::path::Path;

use mago_allocator::LocalArena;
use mago_database::file::File;
use mago_names::ResolvedNames;
use mago_span::{HasSpan, Span};
use mago_syntax::cst::{ClassLikeMemberSelector, KeyValueArrayElement, Method, StaticMethodCall};
use mago_syntax::parser::parse_file;
use mago_syntax::walker::Walker;

use crate::syntax::{class_constant, string_literal};

const CONFIG: &str = "config/app.php";
const FACADE: &str = "vendor/laravel/framework/src/Illuminate/Support/Facades/Facade.php";
const SECTION: &str = "aliases";
const DEFAULTS: &[u8] = b"defaultAliases";
const STUB: &[u8] = b"<laravel class aliases>";

#[derive(Default)]
struct Pairs {
    found: Vec<(String, String)>,
    section: Option<Span>,
    uses_defaults: bool,
    in_defaults: bool,
}

#[derive(Clone, Copy)]
enum Source {
    Config,
    Defaults,
}

struct Collector<'n, 'arena> {
    names: &'n ResolvedNames<'arena>,
    source: Source,
}

const fn contains(outer: Span, inner: Span) -> bool {
    return outer.start.offset <= inner.start.offset && inner.end.offset <= outer.end.offset;
}

impl<'ast, 'arena> Walker<'ast, 'arena, Pairs> for Collector<'_, 'arena> {
    fn walk_in_method(&self, method: &'ast Method<'arena>, pairs: &mut Pairs) {
        pairs.in_defaults =
            matches!(self.source, Source::Defaults) && method.name.value == DEFAULTS;
    }

    fn walk_out_method(&self, _: &'ast Method<'arena>, pairs: &mut Pairs) {
        pairs.in_defaults = false;
    }

    fn walk_in_key_value_array_element(
        &self,
        element: &'ast KeyValueArrayElement<'arena>,
        pairs: &mut Pairs,
    ) {
        let key = string_literal(element.key);
        if matches!(self.source, Source::Config)
            && pairs.section.is_none()
            && key.as_deref() == Some(SECTION)
        {
            pairs.section = Some(element.value.span());
            return;
        }
        let inside = pairs.in_defaults
            || pairs
                .section
                .is_some_and(|section| return contains(section, element.span()));
        if let (true, Some(alias), Some(class)) =
            (inside, key, class_constant(self.names, element.value))
        {
            pairs.found.push((alias, class));
        }
    }

    fn walk_in_static_method_call(&self, call: &'ast StaticMethodCall<'arena>, pairs: &mut Pairs) {
        let calls_defaults = matches!(&call.method, ClassLikeMemberSelector::Identifier(identifier) if identifier.value == DEFAULTS);
        if calls_defaults
            && pairs
                .section
                .is_some_and(|section| return contains(section, call.span()))
        {
            pairs.uses_defaults = true;
        }
    }
}

fn pairs_in(root: &Path, relative: &str, source: Source) -> Pairs {
    let mut pairs = Pairs::default();
    let Ok(content) = std::fs::read(root.join(relative)) else {
        return pairs;
    };
    let file = File::ephemeral(
        Cow::Owned(relative.as_bytes().to_vec()),
        Cow::Owned(content),
    );
    let arena = LocalArena::new();
    let program = parse_file(&arena, &file);
    let names = mago_names::resolver::NameResolver::new(&arena).resolve(program);
    Collector {
        names: &names,
        source,
    }
    .walk_program(program, &mut pairs);
    return pairs;
}

pub(crate) fn stub(root: &Path) -> File {
    let configured = pairs_in(root, CONFIG, Source::Config);
    let mut aliases = if configured.uses_defaults {
        pairs_in(root, FACADE, Source::Defaults).found
    } else {
        Vec::new()
    };
    aliases.extend(configured.found);
    let source: String = std::iter::once(String::from("<?php\n"))
        .chain(aliases.into_iter().map(|(alias, class)| {
            return format!("class_alias(\\{class}::class, '{alias}');\n");
        }))
        .collect();
    return File::ephemeral(Cow::Borrowed(STUB), Cow::Owned(source.into_bytes()));
}
