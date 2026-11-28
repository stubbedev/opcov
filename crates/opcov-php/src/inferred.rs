use std::collections::{BTreeMap, BTreeSet};

use mago_allocator::LocalArena;
use mago_codex::metadata::CodebaseMetadata;
use mago_codex::metadata::ttype::TypeMetadata;
use mago_codex::ttype::atomic::TAtomic;
use mago_codex::ttype::atomic::object::TObject;
use mago_codex::ttype::atomic::object::named::TNamedObject;
use mago_codex::ttype::union::TUnion;
use mago_codex::visibility::Visibility;
use mago_database::file::{File, FileId, HasFileId};
use mago_names::ResolvedNames;
use mago_names::resolver::NameResolver;
use mago_syntax::cst::{
    Access, AnonymousClass, Argument, ArgumentList, ArrayElement, ArrowFunction, Assignment,
    AssignmentOperator, Call, Class, ClassLikeMemberSelector, Closure, ClosureUseClauseVariable,
    Enum, Expression, Foreach, ForeachTarget, Function, Global, Instantiation, Literal, Method,
    Static, StaticItem, Trait, TryCatchClause, UnaryPostfix, UnaryPrefix, UnaryPrefixOperator,
    Unset, Variable,
};
use mago_syntax::parser::parse_file;
use mago_syntax::walker::Walker;
use mago_word::{Word, ascii_lowercase_word, word};
use rayon::prelude::{IntoParallelRefIterator, ParallelIterator};

const THIS: &[u8] = b"$this";
const CONSTRUCTOR: &str = "__construct";
const EVERY_VARIABLE: &str = "*";

#[derive(Debug, Default)]
struct ByReference {
    methods: BTreeMap<String, Vec<Parameter>>,
    functions: BTreeMap<String, Vec<Parameter>>,
}

#[derive(Debug, Clone)]
struct Parameter {
    position: usize,
    name: String,
    variadic: bool,
}

impl ByReference {
    fn of(metadata: &CodebaseMetadata) -> Self {
        let mut found = Self::default();
        #[expect(
            clippy::iter_over_hash_type,
            reason = "the parameters only answer whether any by-reference parameter matches, which no order changes"
        )]
        for ((class, name), function) in &metadata.function_likes {
            let parameters: Vec<Parameter> = function
                .parameters
                .iter()
                .enumerate()
                .filter(|(_, parameter)| return parameter.flags.is_by_reference())
                .map(|(position, parameter)| {
                    return Parameter {
                        position,
                        name: String::from_utf8_lossy(parameter.name.0.as_bytes())
                            .trim_start_matches('$')
                            .to_ascii_lowercase(),
                        variadic: parameter.flags.is_variadic(),
                    };
                })
                .collect();
            if parameters.is_empty() {
                continue;
            }
            let into = if class.is_empty() {
                &mut found.functions
            } else {
                &mut found.methods
            };
            into.entry(name.to_string().to_ascii_lowercase())
                .or_default()
                .extend(parameters);
        }
        return found;
    }

    fn writes(parameters: Option<&Vec<Parameter>>, position: usize, named: Option<&str>) -> bool {
        return parameters.into_iter().flatten().any(|parameter| {
            return named.map_or_else(
                || {
                    return parameter.position == position
                        || parameter.variadic && parameter.position <= position;
                },
                |name| return parameter.name == name,
            );
        });
    }
}

#[derive(Debug, Clone)]
enum Assigned {
    Parameter { method: String, variable: String },
    New(String),
    NewOwn,
    Null,
    Other,
}

#[derive(Debug, Clone)]
enum Write {
    Assigned(Assigned),
    Other,
}

#[derive(Debug, Default)]
struct Collected {
    writes: Vec<(String, String, Write)>,
    dynamic: BTreeSet<String>,
    written: BTreeSet<(String, String, String)>,
    scopes: Vec<Option<String>>,
    frames: Vec<Option<String>>,
}

impl Collected {
    fn scope(&self) -> Option<&String> {
        return self.scopes.last().and_then(Option::as_ref);
    }

    fn method(&self) -> Option<String> {
        return self.frames.iter().rev().find_map(Clone::clone);
    }

    fn innermost_method(&self) -> Option<&String> {
        return self.frames.last().and_then(Option::as_ref);
    }

    fn written(&mut self, variable: &str) {
        let (Some(scope), Some(method)) = (self.scope().cloned(), self.method()) else {
            return;
        };
        self.written.insert((scope, method, variable.to_owned()));
    }

    fn property(&mut self, property: &str, write: Write) {
        if let Some(scope) = self.scope().cloned() {
            self.writes.push((scope, property.to_owned(), write));
        }
    }

    fn dynamic(&mut self) {
        if let Some(scope) = self.scope().cloned() {
            self.dynamic.insert(scope);
        }
    }
}

fn text(bytes: &[u8]) -> String {
    return String::from_utf8_lossy(bytes).into_owned();
}

fn variable_name(variable: &[u8]) -> String {
    return text(variable.strip_prefix(b"$").unwrap_or(variable));
}

fn is_this(expression: &Expression<'_>) -> bool {
    return matches!(expression, Expression::Variable(Variable::Direct(variable)) if variable.name == THIS);
}

enum Target {
    Property(String),
    Dynamic,
}

struct Reader<'n, 'arena> {
    names: &'n ResolvedNames<'arena>,
    by_reference: &'n ByReference,
}

impl Reader<'_, '_> {
    fn this_property(expression: &Expression<'_>) -> Option<Target> {
        let (object, property) = match expression {
            Expression::Access(Access::Property(access)) => (access.object, &access.property),
            _ => return None,
        };
        if !is_this(object) {
            return None;
        }
        return Some(match property {
            ClassLikeMemberSelector::Identifier(identifier) => {
                Target::Property(text(identifier.value))
            }
            _ => Target::Dynamic,
        });
    }

    fn mark(&self, expression: &Expression<'_>, collected: &mut Collected) {
        if let Some(target) = Self::this_property(expression) {
            match target {
                Target::Property(property) => collected.property(&property, Write::Other),
                Target::Dynamic => collected.dynamic(),
            }
            return;
        }
        match expression {
            Expression::Variable(Variable::Direct(variable)) => {
                collected.written(&variable_name(variable.name));
            }
            Expression::Variable(_) => collected.written(EVERY_VARIABLE),
            Expression::ArrayAccess(access) => self.mark(access.array, collected),
            Expression::ArrayAppend(append) => self.mark(append.array, collected),
            Expression::Array(array) => self.elements(array.elements.iter(), collected),
            Expression::LegacyArray(array) => self.elements(array.elements.iter(), collected),
            Expression::List(list) => self.elements(list.elements.iter(), collected),
            Expression::Parenthesized(inner) => self.mark(inner.expression, collected),
            _ => {}
        }
    }

    fn elements<'a, 'arena: 'a>(
        &self,
        elements: impl Iterator<Item = &'a ArrayElement<'arena>>,
        collected: &mut Collected,
    ) {
        for element in elements {
            match element {
                ArrayElement::KeyValue(pair) => self.mark(pair.value, collected),
                ArrayElement::Value(value) => self.mark(value.value, collected),
                ArrayElement::Variadic(variadic) => self.mark(variadic.value, collected),
                ArrayElement::Missing(_) => {}
            }
        }
    }

    fn assigned(&self, value: &Expression<'_>, collected: &Collected) -> Assigned {
        return match value {
            Expression::Variable(Variable::Direct(variable)) => collected
                .innermost_method()
                .map_or(Assigned::Other, |method| {
                    return Assigned::Parameter {
                        method: method.clone(),
                        variable: variable_name(variable.name),
                    };
                }),
            Expression::Instantiation(Instantiation { class, .. }) => match class {
                Expression::Identifier(identifier) => {
                    Assigned::New(text(self.names.get(identifier)))
                }
                Expression::Self_(_) | Expression::Static(_) => Assigned::NewOwn,
                _ => Assigned::Other,
            },
            Expression::Literal(Literal::Null(_)) => Assigned::Null,
            _ => Assigned::Other,
        };
    }

    fn arguments(
        &self,
        arguments: &ArgumentList<'_>,
        parameters: Option<&Vec<Parameter>>,
        unknown_callee: bool,
        collected: &mut Collected,
    ) {
        for (position, argument) in arguments.arguments.iter().enumerate() {
            let (value, named) = match argument {
                Argument::Positional(positional) => (positional.value, None),
                Argument::Named(named) => (
                    named.value,
                    Some(text(named.name.value).to_ascii_lowercase()),
                ),
            };
            if unknown_callee || ByReference::writes(parameters, position, named.as_deref()) {
                self.mark(value, collected);
            }
        }
    }

    fn method_parameters(
        &self,
        method: &ClassLikeMemberSelector<'_>,
    ) -> (Option<&Vec<Parameter>>, bool) {
        return match method {
            ClassLikeMemberSelector::Identifier(identifier) => (
                self.by_reference
                    .methods
                    .get(&text(identifier.value).to_ascii_lowercase()),
                false,
            ),
            _ => (None, true),
        };
    }
}

impl<'ast, 'arena> Walker<'ast, 'arena, Collected> for Reader<'_, 'arena> {
    fn walk_in_class(&self, class: &'ast Class<'arena>, collected: &mut Collected) {
        collected
            .scopes
            .push(Some(text(self.names.get(&class.name)).to_ascii_lowercase()));
    }

    fn walk_out_class(&self, _: &'ast Class<'arena>, collected: &mut Collected) {
        collected.scopes.pop();
    }

    fn walk_in_trait(&self, used: &'ast Trait<'arena>, collected: &mut Collected) {
        collected
            .scopes
            .push(Some(text(self.names.get(&used.name)).to_ascii_lowercase()));
    }

    fn walk_out_trait(&self, _: &'ast Trait<'arena>, collected: &mut Collected) {
        collected.scopes.pop();
    }

    fn walk_in_enum(&self, enumeration: &'ast Enum<'arena>, collected: &mut Collected) {
        collected.scopes.push(Some(
            text(self.names.get(&enumeration.name)).to_ascii_lowercase(),
        ));
    }

    fn walk_out_enum(&self, _: &'ast Enum<'arena>, collected: &mut Collected) {
        collected.scopes.pop();
    }

    fn walk_in_anonymous_class(&self, _: &'ast AnonymousClass<'arena>, collected: &mut Collected) {
        collected.scopes.push(None);
        collected.frames.push(None);
    }

    fn walk_out_anonymous_class(&self, _: &'ast AnonymousClass<'arena>, collected: &mut Collected) {
        collected.scopes.pop();
        collected.frames.pop();
    }

    fn walk_in_method(&self, method: &'ast Method<'arena>, collected: &mut Collected) {
        collected
            .frames
            .push(Some(text(method.name.value).to_ascii_lowercase()));
    }

    fn walk_out_method(&self, _: &'ast Method<'arena>, collected: &mut Collected) {
        collected.frames.pop();
    }

    fn walk_in_closure(&self, _: &'ast Closure<'arena>, collected: &mut Collected) {
        collected.frames.push(None);
    }

    fn walk_out_closure(&self, _: &'ast Closure<'arena>, collected: &mut Collected) {
        collected.frames.pop();
    }

    fn walk_in_arrow_function(&self, _: &'ast ArrowFunction<'arena>, collected: &mut Collected) {
        collected.frames.push(None);
    }

    fn walk_out_arrow_function(&self, _: &'ast ArrowFunction<'arena>, collected: &mut Collected) {
        collected.frames.pop();
    }

    fn walk_in_function(&self, _: &'ast Function<'arena>, collected: &mut Collected) {
        collected.scopes.push(None);
        collected.frames.push(None);
    }

    fn walk_out_function(&self, _: &'ast Function<'arena>, collected: &mut Collected) {
        collected.scopes.pop();
        collected.frames.pop();
    }

    fn walk_in_assignment(&self, assignment: &'ast Assignment<'arena>, collected: &mut Collected) {
        match Self::this_property(assignment.lhs) {
            Some(Target::Property(property)) => {
                let write = if matches!(assignment.operator, AssignmentOperator::Assign(_)) {
                    Write::Assigned(self.assigned(assignment.rhs, collected))
                } else {
                    Write::Other
                };
                collected.property(&property, write);
            }
            Some(Target::Dynamic) => collected.dynamic(),
            None => self.mark(assignment.lhs, collected),
        }
    }

    fn walk_in_unary_prefix(&self, unary: &'ast UnaryPrefix<'arena>, collected: &mut Collected) {
        if matches!(
            unary.operator,
            UnaryPrefixOperator::Reference(_)
                | UnaryPrefixOperator::PreIncrement(_)
                | UnaryPrefixOperator::PreDecrement(_)
        ) {
            self.mark(unary.operand, collected);
        }
    }

    fn walk_in_unary_postfix(&self, unary: &'ast UnaryPostfix<'arena>, collected: &mut Collected) {
        self.mark(unary.operand, collected);
    }

    fn walk_in_foreach(&self, foreach: &'ast Foreach<'arena>, collected: &mut Collected) {
        match &foreach.target {
            ForeachTarget::Value(target) => self.mark(target.value, collected),
            ForeachTarget::KeyValue(target) => {
                self.mark(target.key, collected);
                self.mark(target.value, collected);
            }
        }
    }

    fn walk_in_try_catch_clause(
        &self,
        clause: &'ast TryCatchClause<'arena>,
        collected: &mut Collected,
    ) {
        if let Some(variable) = &clause.variable {
            collected.written(&variable_name(variable.name));
        }
    }

    fn walk_in_global(&self, global: &'ast Global<'arena>, collected: &mut Collected) {
        for variable in &global.variables {
            match variable {
                Variable::Direct(direct) => collected.written(&variable_name(direct.name)),
                _ => collected.written(EVERY_VARIABLE),
            }
        }
    }

    fn walk_in_static(&self, statics: &'ast Static<'arena>, collected: &mut Collected) {
        for item in &statics.items {
            let variable = match item {
                StaticItem::Abstract(item) => &item.variable,
                StaticItem::Concrete(item) => &item.variable,
            };
            collected.written(&variable_name(variable.name));
        }
    }

    fn walk_in_closure_use_clause_variable(
        &self,
        variable: &'ast ClosureUseClauseVariable<'arena>,
        collected: &mut Collected,
    ) {
        if variable.ampersand.is_some() {
            collected.written(&variable_name(variable.variable.name));
        }
    }

    fn walk_in_unset(&self, unset: &'ast Unset<'arena>, collected: &mut Collected) {
        for value in &unset.values {
            self.mark(value, collected);
        }
    }

    fn walk_in_call(&self, call: &'ast Call<'arena>, collected: &mut Collected) {
        match call {
            Call::Function(function) => {
                let (parameters, unknown) = match function.function {
                    Expression::Identifier(identifier) => {
                        let resolved = text(self.names.get(identifier)).to_ascii_lowercase();
                        let local = text(identifier.value()).to_ascii_lowercase();
                        (
                            self.by_reference
                                .functions
                                .get(&resolved)
                                .or_else(|| return self.by_reference.functions.get(&local)),
                            false,
                        )
                    }
                    _ => (None, true),
                };
                self.arguments(&function.argument_list, parameters, unknown, collected);
            }
            Call::Method(method) => {
                let (parameters, unknown) = self.method_parameters(&method.method);
                self.arguments(&method.argument_list, parameters, unknown, collected);
            }
            Call::NullSafeMethod(method) => {
                let (parameters, unknown) = self.method_parameters(&method.method);
                self.arguments(&method.argument_list, parameters, unknown, collected);
            }
            Call::StaticMethod(method) => {
                let (parameters, unknown) = self.method_parameters(&method.method);
                self.arguments(&method.argument_list, parameters, unknown, collected);
            }
        }
    }

    fn walk_in_instantiation(&self, new: &'ast Instantiation<'arena>, collected: &mut Collected) {
        if let Some(arguments) = &new.argument_list {
            let parameters = self.by_reference.methods.get(CONSTRUCTOR);
            self.arguments(arguments, parameters, false, collected);
        }
    }
}

struct Candidate {
    class: Word,
    property: Word,
    protected: bool,
}

fn candidates(metadata: &CodebaseMetadata) -> Vec<Candidate> {
    let mut found = Vec::new();
    #[expect(
        clippy::iter_over_hash_type,
        reason = "the candidates are sorted before they are returned"
    )]
    for (class, class_like) in &metadata.class_likes {
        if !class_like.kind.is_class() {
            continue;
        }
        #[expect(
            clippy::iter_over_hash_type,
            reason = "the candidates are sorted before they are returned"
        )]
        for (name, property) in &class_like.properties {
            let flags = property.flags;
            let hidden = |visibility: Visibility| {
                return matches!(visibility, Visibility::Private | Visibility::Protected);
            };
            let null_default = property
                .default_type_metadata
                .as_ref()
                .is_none_or(|default| {
                    return default.type_union.is_null();
                });
            if property.type_declaration_metadata.is_some()
                || property.type_metadata.is_some()
                || flags.is_promoted_property()
                || flags.is_static()
                || !property.hooks.is_empty()
                || !hidden(property.read_visibility)
                || !hidden(property.write_visibility)
                || flags.has_default() && !null_default
            {
                continue;
            }
            found.push(Candidate {
                class: *class,
                property: *name,
                protected: property.read_visibility == Visibility::Protected
                    || property.write_visibility == Visibility::Protected,
            });
        }
    }
    found.sort_by(|left, right| {
        return (left.class, left.property).cmp(&(right.class, right.property));
    });
    return found;
}

fn traits_of(metadata: &CodebaseMetadata, class: Word, into: &mut BTreeSet<Word>) {
    let Some(class_like) = metadata.class_likes.get(&class) else {
        return;
    };
    #[expect(
        clippy::iter_over_hash_type,
        reason = "the traits are collected into a set, which no order changes"
    )]
    for used in &class_like.used_traits {
        if into.insert(*used) {
            traits_of(metadata, *used, into);
        }
    }
}

fn children(metadata: &CodebaseMetadata) -> BTreeMap<Word, Vec<Word>> {
    let mut children: BTreeMap<Word, Vec<Word>> = BTreeMap::new();
    #[expect(
        clippy::iter_over_hash_type,
        reason = "the children only feed a set of scopes, which no order changes"
    )]
    for (name, class_like) in &metadata.class_likes {
        if let Some(parent) = class_like.direct_parent_class {
            children
                .entry(ascii_lowercase_word(parent.as_bytes()))
                .or_default()
                .push(*name);
        }
    }
    return children;
}

fn scopes(
    metadata: &CodebaseMetadata,
    children: &BTreeMap<Word, Vec<Word>>,
    candidate: &Candidate,
) -> BTreeSet<Word> {
    let mut classes = vec![candidate.class];
    if candidate.protected {
        let mut pending = vec![candidate.class];
        while let Some(class) = pending.pop() {
            for child in children.get(&class).into_iter().flatten() {
                classes.push(*child);
                pending.push(*child);
            }
        }
    }
    let mut scopes: BTreeSet<Word> = classes.iter().copied().collect();
    for class in classes {
        traits_of(metadata, class, &mut scopes);
    }
    return scopes;
}

fn read(file: &File, by_reference: &ByReference) -> Collected {
    let arena = LocalArena::new();
    let program = parse_file(&arena, file);
    let names = NameResolver::new(&arena).resolve(program);
    let mut collected = Collected::default();
    Reader {
        names: &names,
        by_reference,
    }
    .walk_program(program, &mut collected);
    return collected;
}

struct Writes {
    by_property: BTreeMap<(String, String), Vec<Write>>,
    dynamic: BTreeSet<String>,
    written: BTreeSet<(String, String, String)>,
}

fn assigned_type(
    metadata: &CodebaseMetadata,
    writes: &Writes,
    scope: &str,
    assigned: &Assigned,
) -> Option<Vec<TAtomic>> {
    return match assigned {
        Assigned::Parameter { method, variable } => {
            let changed = |name: &str| {
                return writes.written.contains(&(
                    scope.to_owned(),
                    method.clone(),
                    name.to_owned(),
                ));
            };
            if changed(variable) || changed(EVERY_VARIABLE) {
                return None;
            }
            let function = metadata
                .function_likes
                .get(&(word(scope.as_bytes()), word(method.as_bytes())))?;
            let parameter = function.parameters.iter().find(|parameter| {
                return variable_name(parameter.name.0.as_bytes()).eq_ignore_ascii_case(variable);
            })?;
            if parameter.flags.is_by_reference() || parameter.flags.is_variadic() {
                return None;
            }
            let declared = parameter
                .type_declaration_metadata
                .as_ref()
                .or(parameter.type_metadata.as_ref())?;
            Some(declared.type_union.types.to_vec())
        }
        Assigned::New(class) => {
            let found = metadata
                .class_likes
                .get(&ascii_lowercase_word(class.as_bytes()))?;
            Some(vec![TAtomic::Object(TObject::Named(TNamedObject::new(
                found.original_name,
            )))])
        }
        Assigned::NewOwn => {
            let found = metadata.class_likes.get(&word(scope.as_bytes()))?;
            found.kind.is_class().then(|| {
                return vec![TAtomic::Object(TObject::Named(TNamedObject::new(
                    found.original_name,
                )))];
            })
        }
        Assigned::Null => Some(vec![TAtomic::Null]),
        Assigned::Other => None,
    };
}

fn inferred_type(
    metadata: &CodebaseMetadata,
    writes: &Writes,
    scopes: &BTreeSet<Word>,
    property: &str,
) -> Option<TUnion> {
    let scope_names: Vec<String> = scopes
        .iter()
        .map(|scope| return scope.to_string())
        .collect();
    if scope_names
        .iter()
        .any(|scope| return writes.dynamic.contains(scope))
    {
        return None;
    }
    let mut atomics = vec![TAtomic::Null];
    let mut seen = false;
    for scope in &scope_names {
        for write in writes
            .by_property
            .get(&(scope.clone(), property.to_owned()))
            .into_iter()
            .flatten()
        {
            let Write::Assigned(assigned) = write else {
                return None;
            };
            atomics.extend(assigned_type(metadata, writes, scope, assigned)?);
            seen = true;
        }
    }
    return seen.then(|| return TUnion::from_vec(atomics));
}

pub(crate) type Inferred = BTreeSet<(Word, Word)>;

pub(crate) fn infer_property_types(metadata: &mut CodebaseMetadata, files: &[&File]) -> Inferred {
    let candidates = candidates(metadata);
    if candidates.is_empty() {
        return Inferred::new();
    }
    let by_id: BTreeMap<FileId, &File> = files
        .iter()
        .map(|file| return (file.file_id(), *file))
        .collect();
    let children = children(metadata);
    let by_reference = ByReference::of(metadata);
    let planned: Vec<(Candidate, BTreeSet<Word>, BTreeSet<FileId>)> = candidates
        .into_iter()
        .filter_map(|candidate| {
            let scopes = scopes(metadata, &children, &candidate);
            let files: Option<BTreeSet<FileId>> = scopes
                .iter()
                .map(|scope| {
                    let file = metadata.class_likes.get(scope)?.span.file_id;
                    return by_id.contains_key(&file).then_some(file);
                })
                .collect();
            let files = files?;
            return Some((candidate, scopes, files));
        })
        .collect();
    let needed: BTreeSet<FileId> = planned
        .iter()
        .flat_map(|(_, _, files)| return files.iter().copied())
        .collect();
    let mut writes = Writes {
        by_property: BTreeMap::new(),
        dynamic: BTreeSet::new(),
        written: BTreeSet::new(),
    };
    let read_files: Vec<&File> = needed
        .iter()
        .filter_map(|id| return by_id.get(id).copied())
        .collect();
    let collections: Vec<Collected> = read_files
        .par_iter()
        .map(|file| return read(file, &by_reference))
        .collect();
    for collected in collections {
        for (scope, property, write) in collected.writes {
            writes
                .by_property
                .entry((scope, property))
                .or_default()
                .push(write);
        }
        writes.dynamic.extend(collected.dynamic);
        writes.written.extend(collected.written);
    }
    let inferred: Vec<(Word, Word, TUnion)> = planned
        .iter()
        .filter_map(|(candidate, scopes, _)| {
            let property = variable_name(candidate.property.as_bytes());
            let typed = inferred_type(metadata, &writes, scopes, &property)?;
            return Some((candidate.class, candidate.property, typed));
        })
        .collect();
    let mut typed_properties = Inferred::new();
    for (class, property, typed) in inferred {
        let Some(class_like) = metadata.class_likes.get_mut(&class) else {
            continue;
        };
        typed_properties.insert((class, property));
        let span = class_like.span;
        let Some(metadata) = class_like.properties.get_mut(&property) else {
            continue;
        };
        let mut declared = TypeMetadata::new(typed, metadata.span.unwrap_or(span));
        declared.inferred = true;
        metadata.type_metadata = Some(declared);
    }
    return typed_properties;
}
