use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;

use mago_allocator::LocalArena;
use mago_codex::metadata::CodebaseMetadata;
use mago_codex::metadata::constant::ConstantMetadata;
use mago_codex::ttype::union::TUnion;
use mago_database::file::{File, FileId};
use mago_names::ResolvedNames;
use mago_names::resolver::NameResolver;
use mago_span::{HasSpan, Span};
use mago_syntax::comments::docblock::PrecedingDocblocks;
use mago_syntax::cst::{
    Class, ClassLikeMember, Enum, EnumCaseItem, Function, Interface, LocalIdentifier, Method,
    MethodBody, Property, PropertyItem, Trait, Trivia, Use, UseItem,
};
use mago_syntax::parser::parse_file;
use mago_syntax::walker::Walker;
use mago_word::{Word, ascii_lowercase_word};
use rayon::prelude::{IntoParallelRefIterator, ParallelIterator};

use crate::consumers::Mentions;
use crate::declaration::normalized;
use crate::facts::short_name;
use crate::index::Index;
use crate::target::{Kind, Target};

const CONSTRUCTOR: &str = "__construct";
const SCOPE: &str = "scope";
const IMPLICIT: &[&str] = &[
    "__call",
    "__callstatic",
    "__get",
    "__set",
    "__isset",
    "__unset",
    "__invoke",
    "__tostring",
    "__clone",
    "__destruct",
    "__serialize",
    "__unserialize",
    "__sleep",
    "__wakeup",
    "__set_state",
    "__debuginfo",
    "offsetget",
    "offsetset",
    "offsetexists",
    "offsetunset",
    "getiterator",
    "current",
    "key",
    "next",
    "rewind",
    "valid",
    "count",
    "jsonserialize",
];

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
struct Declared {
    file: usize,
    kind: Kind,
    text: Vec<u8>,
    names: Vec<String>,
}

type Bodies = BTreeMap<(Kind, String), BTreeSet<Vec<u8>>>;

#[derive(Debug)]
struct Shape {
    file: usize,
    name: String,
    header: Vec<u8>,
    members: BTreeSet<Declared>,
    bodies: Bodies,
}

#[derive(Debug, Default)]
struct Declarations {
    imports: BTreeSet<(usize, String, Vec<u8>)>,
    classes: BTreeMap<String, Shape>,
    functions: BTreeSet<Declared>,
    function_bodies: Bodies,
    redeclared: bool,
}

#[derive(Default)]
struct Members {
    declared: BTreeSet<Declared>,
    bodies: Bodies,
}

struct Reader<'a, 'arena> {
    file: usize,
    source: &'a [u8],
    names: &'a ResolvedNames<'arena>,
    trivia: &'a [Trivia<'arena>],
}

fn lowercase(bytes: &[u8]) -> String {
    return String::from_utf8_lossy(bytes).to_ascii_lowercase();
}

fn variable(name: &[u8]) -> String {
    return lowercase(name.strip_prefix(b"$").unwrap_or(name));
}

impl Reader<'_, '_> {
    fn between(&self, start: u32, end: u32) -> &[u8] {
        let (Ok(start), Ok(end)) = (usize::try_from(start), usize::try_from(end)) else {
            return &[];
        };
        return self.source.get(start..end).unwrap_or_default();
    }

    fn documented(&self, span: Span, end: u32) -> Vec<u8> {
        return normalized(
            PrecedingDocblocks::new(self.trivia, span.start.offset)
                .map(|docblock| return docblock.value),
            self.between(span.start.offset, end),
        );
    }

    fn body(&self, kind: Kind, name: &str, span: Span, bodies: &mut Bodies) {
        let code = self.between(span.start.offset, span.end.offset);
        bodies
            .entry((kind, name.to_owned()))
            .or_default()
            .insert(normalized(std::iter::empty(), code));
    }

    fn declared(&self, kind: Kind, span: Span, end: u32, names: Vec<String>) -> Declared {
        return Declared {
            file: self.file,
            kind,
            text: self.documented(span, end),
            names,
        };
    }

    fn method(&self, method: &Method<'_>, members: &mut Members) {
        let name = lowercase(method.name.value);
        let end = match &method.body {
            MethodBody::Concrete(block) => {
                self.body(Kind::Method, &name, block.span(), &mut members.bodies);
                block.left_brace.start.offset
            }
            MethodBody::Abstract(_) => method.span().end.offset,
        };
        if name == CONSTRUCTOR {
            members.declared.extend(
                method
                    .parameter_list
                    .parameters
                    .iter()
                    .filter(|parameter| return !parameter.modifiers.is_empty())
                    .map(|parameter| {
                        return self.declared(
                            Kind::Property,
                            parameter.span(),
                            parameter.span().end.offset,
                            vec![variable(parameter.variable.name)],
                        );
                    }),
            );
        }
        members
            .declared
            .insert(self.declared(Kind::Method, method.span(), end, vec![name]));
    }

    fn member(&self, member: &ClassLikeMember<'_>, header: &mut Vec<u8>, members: &mut Members) {
        let whole = |kind: Kind, names: Vec<String>| {
            return self.declared(kind, member.span(), member.span().end.offset, names);
        };
        let declared = match member {
            ClassLikeMember::TraitUse(_) => {
                header.extend(self.documented(member.span(), member.span().end.offset));
                return;
            }
            ClassLikeMember::Method(method) => return self.method(method, members),
            ClassLikeMember::Property(Property::Plain(plain)) => whole(
                Kind::Property,
                plain
                    .items
                    .iter()
                    .map(|item| return item_name(item))
                    .collect(),
            ),
            ClassLikeMember::Property(Property::Hooked(hooked)) => {
                let name = item_name(&hooked.item);
                self.body(
                    Kind::Property,
                    &name,
                    hooked.hook_list.span(),
                    &mut members.bodies,
                );
                self.declared(
                    Kind::Property,
                    member.span(),
                    hooked.hook_list.left_brace.start.offset,
                    vec![name],
                )
            }
            ClassLikeMember::Constant(constant) => whole(
                Kind::Constant,
                constant
                    .items
                    .iter()
                    .map(|item| return lowercase(item.name.value))
                    .collect(),
            ),
            ClassLikeMember::EnumCase(case) => whole(
                Kind::Constant,
                vec![lowercase(match &case.item {
                    EnumCaseItem::Unit(unit) => unit.name.value,
                    EnumCaseItem::Backed(backed) => backed.name.value,
                })],
            ),
        };
        members.declared.insert(declared);
    }

    fn class_like<'m, 'arena: 'm>(
        &self,
        span: Span,
        name: &LocalIdentifier<'arena>,
        left_brace: Span,
        members: impl Iterator<Item = &'m ClassLikeMember<'arena>>,
        declarations: &mut Declarations,
    ) {
        let mut header = self.documented(span, left_brace.end.offset);
        let mut collected = Members::default();
        for member in members {
            self.member(member, &mut header, &mut collected);
        }
        let resolved = self.names.get(name);
        let shape = Shape {
            file: self.file,
            name: String::from_utf8_lossy(resolved).into_owned(),
            header,
            members: collected.declared,
            bodies: collected.bodies,
        };
        if declarations
            .classes
            .insert(lowercase(resolved), shape)
            .is_some()
        {
            declarations.redeclared = true;
        }
    }
}

fn item_name(item: &PropertyItem<'_>) -> String {
    return variable(match item {
        PropertyItem::Abstract(item) => item.variable.name,
        PropertyItem::Concrete(item) => item.variable.name,
    });
}

struct Collecting {
    declarations: Declarations,
    statement: Vec<u8>,
}

impl<'ast, 'arena> Walker<'ast, 'arena, Collecting> for Reader<'_, 'arena> {
    fn walk_in_use(&self, statement: &'ast Use<'arena>, collecting: &mut Collecting) {
        collecting.statement = self
            .between(statement.span().start.offset, statement.span().end.offset)
            .to_vec();
    }

    fn walk_in_use_item(&self, item: &'ast UseItem<'arena>, collecting: &mut Collecting) {
        let alias = item.alias.as_ref().map_or_else(
            || return short_name(&String::from_utf8_lossy(item.name.value())).to_ascii_lowercase(),
            |alias| return lowercase(alias.identifier.value),
        );
        collecting
            .declarations
            .imports
            .insert((self.file, alias, collecting.statement.clone()));
    }

    fn walk_in_class(&self, class: &'ast Class<'arena>, collecting: &mut Collecting) {
        self.class_like(
            class.span(),
            &class.name,
            class.left_brace,
            class.members.iter(),
            &mut collecting.declarations,
        );
    }

    fn walk_in_interface(&self, interface: &'ast Interface<'arena>, collecting: &mut Collecting) {
        self.class_like(
            interface.span(),
            &interface.name,
            interface.left_brace,
            interface.members.iter(),
            &mut collecting.declarations,
        );
    }

    fn walk_in_trait(&self, declared: &'ast Trait<'arena>, collecting: &mut Collecting) {
        self.class_like(
            declared.span(),
            &declared.name,
            declared.left_brace,
            declared.members.iter(),
            &mut collecting.declarations,
        );
    }

    fn walk_in_enum(&self, declared: &'ast Enum<'arena>, collecting: &mut Collecting) {
        self.class_like(
            declared.span(),
            &declared.name,
            declared.left_brace,
            declared.members.iter(),
            &mut collecting.declarations,
        );
    }

    fn walk_in_function(&self, function: &'ast Function<'arena>, collecting: &mut Collecting) {
        let name = short_name(&lowercase(self.names.get(&function.name))).to_owned();
        self.body(
            Kind::Function,
            &name,
            function.body.span(),
            &mut collecting.declarations.function_bodies,
        );
        let declared = self.declared(
            Kind::Function,
            function.span(),
            function.body.left_brace.start.offset,
            vec![name],
        );
        collecting.declarations.functions.insert(declared);
    }
}

impl Declarations {
    fn of(index: &Index, changed: &[&str]) -> Self {
        let mut collecting = Collecting {
            declarations: Self::default(),
            statement: Vec::new(),
        };
        for (position, path) in changed.iter().enumerate() {
            let Some(source) = index
                .positions
                .get(*path)
                .and_then(|position| return index.sources.get(*position))
            else {
                continue;
            };
            read(&source.file, position, &mut collecting);
        }
        return collecting.declarations;
    }
}

fn read(file: &File, position: usize, collecting: &mut Collecting) {
    let arena = LocalArena::new();
    let program = parse_file(&arena, file);
    let names = NameResolver::new(&arena).resolve(program);
    Reader {
        file: position,
        source: file.contents.as_ref(),
        names: &names,
        trivia: program.trivia.nodes,
    }
    .walk_program(program, collecting);
}

type SignatureKey = (Kind, String, String);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum MemberChange {
    Signature,
    Body,
    Formatting,
}

#[derive(Default)]
pub(crate) struct Signatures {
    declarations: BTreeMap<SignatureKey, BTreeSet<Vec<u8>>>,
    bodies: BTreeMap<SignatureKey, BTreeSet<Vec<u8>>>,
}

impl Signatures {
    fn key(target: &Target) -> SignatureKey {
        let member = match target.kind {
            Kind::Function | Kind::GlobalConstant => {
                short_name(&target.member).to_ascii_lowercase()
            }
            Kind::Method | Kind::Property | Kind::Constant => target.member.to_ascii_lowercase(),
        };
        let class = target
            .class
            .as_deref()
            .unwrap_or_default()
            .to_ascii_lowercase();
        return (target.kind, class, member);
    }

    pub(crate) fn change(&self, other: &Self, target: &Target) -> MemberChange {
        let key = Self::key(target);
        let declared = match (self.declarations.get(&key), other.declarations.get(&key)) {
            (Some(mine), Some(theirs)) => mine == theirs,
            _ => false,
        };
        if !declared {
            return MemberChange::Signature;
        }
        if self.bodies.get(&key) != other.bodies.get(&key) {
            return MemberChange::Body;
        }
        return MemberChange::Formatting;
    }

    fn add(&mut self, class: &str, declared: &Declared) {
        for name in &declared.names {
            self.declarations
                .entry((declared.kind, class.to_owned(), name.clone()))
                .or_default()
                .insert(declared.text.clone());
        }
    }

    fn add_bodies(&mut self, class: &str, bodies: &Bodies) {
        for ((kind, name), texts) in bodies {
            self.bodies
                .entry((*kind, class.to_owned(), name.clone()))
                .or_default()
                .extend(texts.iter().cloned());
        }
    }
}

impl Index {
    pub(crate) fn signatures(&self, paths: &[&str]) -> Signatures {
        let declarations = Declarations::of(self, paths);
        let mut signatures = Signatures::default();
        for (class, shape) in &declarations.classes {
            for declared in &shape.members {
                signatures.add(class, declared);
            }
            signatures.add_bodies(class, &shape.bodies);
        }
        for declared in &declarations.functions {
            signatures.add("", declared);
        }
        signatures.add_bodies("", &declarations.function_bodies);
        return signatures;
    }
}

struct Words(BTreeSet<String>);

impl Words {
    fn mentioned_in(&self, text: &[u8]) -> bool {
        return text
            .split(|byte| return !(byte.is_ascii_alphanumeric() || *byte == b'_' || *byte >= 0x80))
            .any(|word| return self.0.contains(&lowercase(word)));
    }
}

struct Suspects {
    aliases: BTreeMap<usize, Words>,
    absent: Words,
}

impl Suspects {
    fn in_text(&self, file: usize, text: &[u8]) -> bool {
        return self.absent.mentioned_in(text)
            || self
                .aliases
                .get(&file)
                .is_some_and(|words| return words.mentioned_in(text));
    }
}

fn changed_aliases(before: &Declarations, after: &Declarations) -> BTreeMap<usize, Words> {
    let mut aliases: BTreeMap<usize, Words> = BTreeMap::new();
    for (file, alias, _) in before.imports.symmetric_difference(&after.imports) {
        aliases
            .entry(*file)
            .or_insert_with(|| return Words(BTreeSet::new()))
            .0
            .insert(alias.clone());
    }
    return aliases;
}

struct Diff<'a> {
    base: &'a Index,
    head: &'a Index,
    names: BTreeSet<String>,
    files: BTreeSet<FileId>,
}

impl Diff<'_> {
    fn absent(&self) -> BTreeSet<String> {
        let one_sided = |from: &CodebaseMetadata, other: &CodebaseMetadata| {
            return from
                .class_likes
                .keys()
                .filter(|name| return !other.class_likes.contains_key(*name))
                .chain(
                    from.class_like_aliases
                        .keys()
                        .filter(|name| return !other.class_like_aliases.contains_key(*name)),
                )
                .map(|name| return name.to_string())
                .collect::<Vec<String>>();
        };
        let mut absent: BTreeSet<String> = one_sided(&self.base.metadata, &self.head.metadata)
            .into_iter()
            .collect();
        absent.extend(one_sided(&self.head.metadata, &self.base.metadata));
        return absent;
    }

    fn constants(&mut self) {
        let one_sided = |from: &CodebaseMetadata, other: &CodebaseMetadata| {
            return from
                .constants
                .keys()
                .filter(|name| return !other.constants.contains_key(*name))
                .map(|name| return short_name(&name.to_string()).to_ascii_lowercase())
                .collect::<Vec<String>>();
        };
        let found = one_sided(&self.base.metadata, &self.head.metadata);
        self.names.extend(found);
        let found = one_sided(&self.head.metadata, &self.base.metadata);
        self.names.extend(found);
        let retyped: Vec<String> = self
            .base
            .metadata
            .constants
            .iter()
            .filter(|(name, constant)| {
                return self
                    .head
                    .metadata
                    .constants
                    .get(*name)
                    .is_some_and(|other| return !same_constant(constant, other));
            })
            .map(|(name, _)| return short_name(&name.to_string()).to_ascii_lowercase())
            .collect();
        self.names.extend(retyped);
    }

    fn property_types(&mut self) {
        fn typed<'i>(index: &'i Index, (class, property): &(Word, Word)) -> Option<&'i TUnion> {
            return index
                .metadata
                .class_likes
                .get(class)
                .and_then(|class_like| return class_like.properties.get(property))
                .and_then(|metadata| return metadata.type_metadata.as_ref())
                .map(|metadata| return &metadata.type_union);
        }
        let retyped: Vec<String> = self
            .base
            .inferred
            .union(&self.head.inferred)
            .filter(|key| return typed(self.base, key) != typed(self.head, key))
            .map(|(_, property)| return variable(property.as_bytes()))
            .collect();
        self.names.extend(retyped);
    }

    fn referenced_unchanged(&self, absent: &BTreeSet<String>, changed: &BTreeSet<&str>) -> bool {
        return self.head.sources.par_iter().any(|source| {
            return !changed.contains(source.path.as_str())
                && source
                    .record
                    .references
                    .iter()
                    .any(|class| return absent.contains(&class.to_ascii_lowercase()));
        });
    }

    fn traits(&mut self) {
        let (base, head) = (&self.base.trait_users, &self.head.trait_users);
        let declaring: Vec<FileId> = base
            .keys()
            .chain(head.keys())
            .filter(|used| return base.get(*used) != head.get(*used))
            .flat_map(|used| {
                let name = ascii_lowercase_word(used.as_bytes());
                return [&self.base.metadata, &self.head.metadata]
                    .into_iter()
                    .filter_map(move |metadata| return metadata.class_likes.get(&name))
                    .map(|declared| return declared.span.file_id);
            })
            .collect();
        self.files.extend(declaring);
    }

    fn forwarding_differs(&self) -> bool {
        let (base, head) = (&self.base.forwards, &self.head.forwards);
        return base.iter().any(|(owner, targets)| {
            return head.get(owner).is_some_and(|other| return other != targets);
        });
    }

    fn constructed(&mut self, class: &str) {
        self.names.insert(short_name(class).to_ascii_lowercase());
        for metadata in [&self.base.metadata, &self.head.metadata] {
            let descendants = metadata
                .all_class_like_descendants
                .get(&ascii_lowercase_word(class.as_bytes()))
                .into_iter()
                .flatten()
                .map(|descendant| return short_name(&descendant.to_string()).to_ascii_lowercase())
                .collect::<Vec<String>>();
            self.names.extend(descendants);
        }
    }

    fn members<'d>(
        &mut self,
        class: &str,
        changed: impl Iterator<Item = &'d Declared>,
    ) -> Option<()> {
        for declared in changed {
            for name in &declared.names {
                if IMPLICIT.contains(&name.as_str()) {
                    return None;
                }
                if name == CONSTRUCTOR {
                    self.constructed(class);
                    continue;
                }
                if let Some(scope) = name
                    .strip_prefix(SCOPE)
                    .filter(|scope| return !scope.is_empty())
                {
                    self.names.insert(scope.to_owned());
                }
                self.names.insert(name.clone());
            }
        }
        return Some(());
    }

    fn classes(
        &mut self,
        before: &Declarations,
        after: &Declarations,
        suspects: &Suspects,
    ) -> Option<()> {
        for (key, old) in &before.classes {
            let Some(new) = after.classes.get(key) else {
                continue;
            };
            if old.header != new.header
                || [old, new]
                    .iter()
                    .any(|shape| return suspects.in_text(shape.file, &shape.header))
            {
                return None;
            }
            let changed = old.members.symmetric_difference(&new.members).chain(
                old.members
                    .iter()
                    .chain(&new.members)
                    .filter(|member| return suspects.in_text(member.file, &member.text)),
            );
            self.members(&new.name, changed)?;
        }
        return Some(());
    }

    fn functions(&mut self, before: &Declarations, after: &Declarations, suspects: &Suspects) {
        let changed: Vec<String> = before
            .functions
            .symmetric_difference(&after.functions)
            .chain(
                before
                    .functions
                    .iter()
                    .chain(&after.functions)
                    .filter(|function| return suspects.in_text(function.file, &function.text)),
            )
            .flat_map(|function| return function.names.iter().cloned())
            .collect();
        self.names.extend(changed);
    }
}

impl Index {
    pub fn share_unaffected(&mut self, donor: &Self, diff: &[&str]) -> usize {
        let Some(donors) = unaffected(self, donor, diff) else {
            return 0;
        };
        let mut shared = 0_usize;
        for (source, position) in self.sources.iter_mut().zip(donors) {
            let Some(counterpart) =
                position.and_then(|position| return donor.sources.get(position))
            else {
                continue;
            };
            source.facts = Arc::clone(&counterpart.facts);
            shared = shared.saturating_add(1);
        }
        return shared;
    }
}

fn unaffected(base: &Index, head: &Index, diff: &[&str]) -> Option<Vec<Option<usize>>> {
    let unindexed: BTreeSet<FileId> = diff
        .iter()
        .filter(|path| {
            return !base.positions.contains_key(**path) && !head.positions.contains_key(**path);
        })
        .map(|path| return FileId::new(path.as_bytes()))
        .collect();
    if declares_in(&base.metadata, &unindexed) || declares_in(&head.metadata, &unindexed) {
        return None;
    }
    let changed = changed_sources(base, head);
    let listed: Vec<&str> = changed.iter().copied().collect();
    let (before, after) = rayon::join(
        || return Declarations::of(base, &listed),
        || return Declarations::of(head, &listed),
    );
    if before.redeclared || after.redeclared {
        return None;
    }
    let mut found = Diff {
        base,
        head,
        names: BTreeSet::new(),
        files: BTreeSet::new(),
    };
    let absent = found.absent();
    if found.forwarding_differs() || found.referenced_unchanged(&absent, &changed) {
        return None;
    }
    let suspects = Suspects {
        aliases: changed_aliases(&before, &after),
        absent: Words(
            absent
                .iter()
                .map(|class| return short_name(class).to_owned())
                .collect(),
        ),
    };
    found.names.extend(suspects.absent.0.iter().cloned());
    found.constants();
    found.property_types();
    found.traits();
    found.classes(&before, &after, &suspects)?;
    found.functions(&before, &after, &suspects);
    let mentions = Mentions::new(&found.names).ok()?;
    return Some(
        base.sources
            .par_iter()
            .map(|source| {
                let position = *head.positions.get(&source.path)?;
                let donor = head.sources.get(position)?;
                let same = donor.file.contents == source.file.contents
                    && !found.files.contains(&source.file.id)
                    && !mentions.in_source(source.file.contents.as_ref());
                return same.then_some(position);
            })
            .collect(),
    );
}

fn same_constant(left: &ConstantMetadata, right: &ConstantMetadata) -> bool {
    return left.inferred_type == right.inferred_type
        && declared_type(left) == declared_type(right)
        && left.flags == right.flags;
}

fn declared_type(constant: &ConstantMetadata) -> Option<&TUnion> {
    return constant
        .type_metadata
        .as_ref()
        .map(|declared| return &declared.type_union);
}

pub(crate) fn declares_in(metadata: &CodebaseMetadata, files: &BTreeSet<FileId>) -> bool {
    if files.is_empty() {
        return false;
    }
    return metadata
        .class_likes
        .values()
        .map(|class| return class.span.file_id)
        .chain(
            metadata
                .function_likes
                .values()
                .map(|function| return function.span.file_id),
        )
        .chain(
            metadata
                .constants
                .values()
                .map(|constant| return constant.span.file_id),
        )
        .any(|file| return files.contains(&file));
}

fn changed_sources<'i>(base: &'i Index, head: &'i Index) -> BTreeSet<&'i str> {
    let differs = |from: &'i Index, other: &'i Index| {
        return from
            .sources
            .par_iter()
            .filter(|source| {
                return other
                    .positions
                    .get(&source.path)
                    .and_then(|position| return other.sources.get(*position))
                    .is_none_or(|counterpart| {
                        return counterpart.file.contents != source.file.contents;
                    });
            })
            .map(|source| return source.path.as_str())
            .collect::<Vec<&str>>();
    };
    let mut changed: BTreeSet<&str> = differs(base, head).into_iter().collect();
    changed.extend(differs(head, base));
    return changed;
}
