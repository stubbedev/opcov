use mago_names::ResolvedNames;
use mago_span::{HasSpan, Span};
use mago_syntax::cst::{
    AnonymousClass, Class, ClassLikeMember, Constant, Enum, EnumCaseItem, Function, Interface,
    LocalIdentifier, Property, PropertyItem, Trait,
};
use mago_syntax::walker::Walker;
use serde::Serialize;

use crate::target::{Kind, Target};

const CONSTRUCTOR: &[u8] = b"__construct";

#[derive(Debug, Clone)]
pub(crate) struct Lines(Vec<u32>);

impl Lines {
    pub(crate) fn of(source: &[u8]) -> Self {
        let starts = source
            .iter()
            .enumerate()
            .filter(|(_, byte)| return **byte == b'\n')
            .filter_map(|(offset, _)| return u32::try_from(offset).ok())
            .collect();
        return Self(starts);
    }

    pub(crate) fn line(&self, offset: u32) -> u32 {
        let before = self.0.partition_point(|newline| return *newline < offset);
        return u32::try_from(before).unwrap_or(u32::MAX).saturating_add(1);
    }
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize)]
pub struct Member {
    pub path: String,
    pub first: u32,
    pub last: u32,
    #[serde(serialize_with = "as_string")]
    pub target: Target,
}

fn as_string<S: serde::Serializer>(target: &Target, serializer: S) -> Result<S::Ok, S::Error> {
    return serializer.collect_str(target);
}

impl Member {
    pub(crate) const fn is_callable(&self) -> bool {
        return matches!(self.target.kind, Kind::Method | Kind::Function);
    }

    pub(crate) const fn contains(&self, line: u32) -> bool {
        return self.first <= line && line <= self.last;
    }

    pub(crate) const fn width(&self) -> u32 {
        return self.last.saturating_sub(self.first);
    }
}

pub(crate) struct Collector<'n, 'arena> {
    pub(crate) names: &'n ResolvedNames<'arena>,
    pub(crate) lines: &'n Lines,
    pub(crate) path: &'n str,
}

#[derive(Default)]
pub(crate) struct Members {
    classes: Vec<Option<String>>,
    pub(crate) found: Vec<Member>,
}

fn text(bytes: &[u8]) -> String {
    return String::from_utf8_lossy(bytes).into_owned();
}

impl Collector<'_, '_> {
    fn push(
        &self,
        members: &mut Members,
        kind: Kind,
        class: Option<String>,
        name: String,
        span: Span,
    ) {
        members.found.push(Member {
            path: self.path.to_owned(),
            first: self.lines.line(span.start.offset),
            last: self.lines.line(span.end.offset),
            target: Target {
                kind,
                class,
                member: name,
            },
        });
    }

    fn enter(&self, name: &LocalIdentifier<'_>, members: &mut Members) {
        members
            .classes
            .push(Some(text(self.names.resolve(name).unwrap_or(name.value))));
    }

    fn property_items<'a>(property: &'a Property<'a>) -> Vec<&'a PropertyItem<'a>> {
        return match property {
            Property::Plain(plain) => plain.items.iter().collect(),
            Property::Hooked(hooked) => vec![&hooked.item],
        };
    }

    fn class_members(
        &self,
        class: &str,
        list: &mago_syntax::cst::Sequence<'_, ClassLikeMember<'_>>,
        members: &mut Members,
    ) {
        for member in list {
            match member {
                ClassLikeMember::Method(method) => {
                    self.push(
                        members,
                        Kind::Method,
                        Some(class.to_owned()),
                        text(method.name.value),
                        method.span(),
                    );
                    if method.name.value.eq_ignore_ascii_case(CONSTRUCTOR) {
                        for parameter in method
                            .parameter_list
                            .parameters
                            .iter()
                            .filter(|parameter| return !parameter.modifiers.is_empty())
                        {
                            let name = text(parameter.variable.name)
                                .trim_start_matches('$')
                                .to_owned();
                            self.push(
                                members,
                                Kind::Property,
                                Some(class.to_owned()),
                                name,
                                parameter.span(),
                            );
                        }
                    }
                }
                ClassLikeMember::Constant(constant) => {
                    for item in &constant.items {
                        self.push(
                            members,
                            Kind::Constant,
                            Some(class.to_owned()),
                            text(item.name.value),
                            constant.span(),
                        );
                    }
                }
                ClassLikeMember::EnumCase(case) => {
                    let name = match &case.item {
                        EnumCaseItem::Unit(unit) => unit.name.value,
                        EnumCaseItem::Backed(backed) => backed.name.value,
                    };
                    self.push(
                        members,
                        Kind::Constant,
                        Some(class.to_owned()),
                        text(name),
                        case.span(),
                    );
                }
                ClassLikeMember::Property(property) => {
                    for item in Self::property_items(property) {
                        let variable = match item {
                            PropertyItem::Abstract(abstract_item) => abstract_item.variable.name,
                            PropertyItem::Concrete(concrete) => concrete.variable.name,
                        };
                        let name = text(variable).trim_start_matches('$').to_owned();
                        self.push(
                            members,
                            Kind::Property,
                            Some(class.to_owned()),
                            name,
                            property.span(),
                        );
                    }
                }
                ClassLikeMember::TraitUse(_) => {}
            }
        }
    }

    fn class_like(
        &self,
        name: &LocalIdentifier<'_>,
        list: &mago_syntax::cst::Sequence<'_, ClassLikeMember<'_>>,
        members: &mut Members,
    ) {
        self.enter(name, members);
        if let Some(Some(class)) = members.classes.last().cloned() {
            self.class_members(&class, list, members);
        }
    }
}

impl<'ast, 'arena> Walker<'ast, 'arena, Members> for Collector<'_, 'arena> {
    fn walk_in_class(&self, class: &'ast Class<'arena>, members: &mut Members) {
        self.class_like(&class.name, &class.members, members);
    }

    fn walk_out_class(&self, _: &'ast Class<'arena>, members: &mut Members) {
        members.classes.pop();
    }

    fn walk_in_interface(&self, interface: &'ast Interface<'arena>, members: &mut Members) {
        self.class_like(&interface.name, &interface.members, members);
    }

    fn walk_out_interface(&self, _: &'ast Interface<'arena>, members: &mut Members) {
        members.classes.pop();
    }

    fn walk_in_trait(&self, used: &'ast Trait<'arena>, members: &mut Members) {
        self.class_like(&used.name, &used.members, members);
    }

    fn walk_out_trait(&self, _: &'ast Trait<'arena>, members: &mut Members) {
        members.classes.pop();
    }

    fn walk_in_enum(&self, enumeration: &'ast Enum<'arena>, members: &mut Members) {
        self.class_like(&enumeration.name, &enumeration.members, members);
    }

    fn walk_out_enum(&self, _: &'ast Enum<'arena>, members: &mut Members) {
        members.classes.pop();
    }

    fn walk_in_anonymous_class(&self, _: &'ast AnonymousClass<'arena>, members: &mut Members) {
        members.classes.push(None);
    }

    fn walk_out_anonymous_class(&self, _: &'ast AnonymousClass<'arena>, members: &mut Members) {
        members.classes.pop();
    }

    fn walk_in_function(&self, function: &'ast Function<'arena>, members: &mut Members) {
        let name = text(
            self.names
                .resolve(&function.name)
                .unwrap_or(function.name.value),
        );
        self.push(members, Kind::Function, None, name, function.span());
    }

    fn walk_in_constant(&self, constant: &'ast Constant<'arena>, members: &mut Members) {
        for item in &constant.items {
            let name = text(self.names.resolve(&item.name).unwrap_or(item.name.value));
            self.push(members, Kind::GlobalConstant, None, name, constant.span());
        }
    }
}

pub(crate) fn innermost_callable(members: &[Member], line: u32) -> Option<&Member> {
    return members
        .iter()
        .filter(|member| return member.is_callable() && member.contains(line))
        .min_by_key(|member| return member.width());
}
