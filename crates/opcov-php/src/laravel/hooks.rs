use std::collections::{BTreeMap, BTreeSet};

use mago_names::ResolvedNames;
use mago_syntax::cst::{
    Argument, ArgumentList, Array, ArrayElement, AttributeList, Class, ClassLikeMemberSelector,
    Enum, Expression, LegacyArray, Method, PartialArgument, Property, PropertyItem, Sequence,
    StaticMethodCall, Trait,
};
use mago_syntax::walker::Walker;

use crate::syntax::{class_constant, string_literal};

const OBSERVE: &[u8] = b"observe";
const OBSERVED_BY: &str = "Illuminate\\Database\\Eloquent\\Attributes\\ObservedBy";
const AS_COMMAND: &str = "Symfony\\Component\\Console\\Attribute\\AsCommand";
const DISPATCHES_EVENTS: &[u8] = b"$dispatchesEvents";
const SIGNATURE: &[u8] = b"$signature";
const NAME: &[u8] = b"$name";

const CREATING: Calls = Calls {
    instance: &["save", "saveOrFail", "push"],
    query: &[
        "create",
        "forceCreate",
        "firstOrCreate",
        "updateOrCreate",
        "createOrFirst",
        "createMany",
        "save",
        "saveMany",
    ],
};
const UPDATING: Calls = Calls {
    instance: &[
        "save",
        "saveOrFail",
        "update",
        "updateOrFail",
        "push",
        "touch",
        "increment",
        "decrement",
    ],
    query: &["updateOrCreate", "save", "saveMany"],
};
const DELETING: Calls = Calls {
    instance: &["delete", "deleteOrFail", "forceDelete"],
    query: &["destroy"],
};
const FORCE_DELETING: Calls = Calls {
    instance: &["forceDelete"],
    query: &["forceDestroy"],
};
const RESTORING: Calls = Calls {
    instance: &["restore"],
    query: &[],
};
const REPLICATING: Calls = Calls {
    instance: &["replicate"],
    query: &[],
};
const MODEL_EVENTS: [(&str, &[Calls]); 14] = [
    ("creating", &[CREATING]),
    ("created", &[CREATING]),
    ("updating", &[UPDATING]),
    ("updated", &[UPDATING]),
    ("saving", &[CREATING, UPDATING]),
    ("saved", &[CREATING, UPDATING]),
    ("deleting", &[DELETING]),
    ("deleted", &[DELETING]),
    ("trashed", &[DELETING]),
    ("forceDeleting", &[FORCE_DELETING]),
    ("forceDeleted", &[FORCE_DELETING]),
    ("restoring", &[RESTORING]),
    ("restored", &[RESTORING]),
    ("replicating", &[REPLICATING]),
];

#[derive(Debug, Clone, Copy)]
pub(crate) struct Calls {
    pub(crate) instance: &'static [&'static str],
    pub(crate) query: &'static [&'static str],
}

fn calls_of(event: &str) -> Option<&'static [Calls]> {
    return MODEL_EVENTS
        .iter()
        .find(|(name, _)| return *name == event)
        .map(|(_, calls)| return *calls);
}

#[derive(Debug, Clone, Default, serde::Serialize, serde::Deserialize)]
pub(crate) struct Hooks {
    observers: BTreeSet<(String, String)>,
    closures: BTreeMap<(String, String), BTreeSet<String>>,
    dispatched: BTreeSet<(String, String, String)>,
    commands: BTreeMap<String, String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ModelEvent {
    pub(crate) model: String,
    pub(crate) instance: Vec<String>,
    pub(crate) query: Vec<String>,
}

fn lower(text: &str) -> String {
    return text.to_ascii_lowercase();
}

impl Hooks {
    pub(crate) fn extend(&mut self, other: Self) {
        self.observers.extend(other.observers);
        for (owner, events) in other.closures {
            self.closures.entry(owner).or_default().extend(events);
        }
        self.dispatched.extend(other.dispatched);
        for (class, name) in other.commands {
            self.commands.entry(class).or_insert(name);
        }
    }

    pub(crate) fn model_events(
        &self,
        class: &str,
        method: &str,
        listens_to: &[String],
    ) -> Vec<ModelEvent> {
        let (class_key, method_key) = (lower(class), lower(method));
        let observed = self
            .observers
            .iter()
            .filter(|(observer, _)| return lower(observer) == class_key)
            .map(|(_, model)| return (model.clone(), vec![method.to_owned()]));
        let closures = self
            .closures
            .iter()
            .filter(|((owner, registering), _)| {
                return lower(owner) == class_key && lower(registering) == method_key;
            })
            .map(|((owner, _), events)| {
                return (owner.clone(), events.iter().cloned().collect::<Vec<_>>());
            });
        let dispatched = self
            .dispatched
            .iter()
            .filter(|(_, _, event_class)| {
                return listens_to
                    .iter()
                    .any(|listened| return listened.eq_ignore_ascii_case(event_class));
            })
            .map(|(model, event, _)| return (model.clone(), vec![event.clone()]));
        return observed
            .chain(closures)
            .chain(dispatched)
            .filter_map(|(model, events)| {
                let calls: Vec<Calls> = events
                    .iter()
                    .filter_map(|event| return calls_of(event))
                    .flatten()
                    .copied()
                    .collect();
                let collect = |side: fn(&Calls) -> &'static [&'static str]| {
                    let mut names: Vec<String> = calls
                        .iter()
                        .flat_map(|calls| return side(calls).iter())
                        .map(|name| return (*name).to_owned())
                        .collect();
                    names.sort();
                    names.dedup();
                    return names;
                };
                let event = ModelEvent {
                    model,
                    instance: collect(|calls| return calls.instance),
                    query: collect(|calls| return calls.query),
                };
                return (!event.instance.is_empty() || !event.query.is_empty()).then_some(event);
            })
            .collect();
    }

    pub(crate) fn command(&self, class: &str) -> Option<&str> {
        return self
            .commands
            .iter()
            .find(|(command, _)| return command.eq_ignore_ascii_case(class))
            .map(|(_, name)| return name.as_str());
    }
}

#[derive(Default)]
pub(crate) struct Collecting {
    pub(crate) hooks: Hooks,
    classes: Vec<Option<String>>,
    method: Option<String>,
}

pub(crate) struct Collector<'n, 'arena> {
    pub(crate) names: &'n ResolvedNames<'arena>,
}

fn text(bytes: &[u8]) -> String {
    return String::from_utf8_lossy(bytes).into_owned();
}

fn command_name(signature: &str) -> Option<String> {
    let name: String = signature
        .trim()
        .chars()
        .take_while(|character| return !character.is_whitespace() && *character != '{')
        .collect();
    return (!name.is_empty()).then_some(name);
}

impl Collector<'_, '_> {
    fn classes(&self, expression: &Expression<'_>, current: Option<&str>) -> Vec<String> {
        let elements = match expression {
            Expression::Array(Array { elements, .. })
            | Expression::LegacyArray(LegacyArray { elements, .. }) => elements,
            Expression::Self_(_) | Expression::Static(_) => {
                return current.map_or_else(Vec::new, |class| return vec![class.to_owned()]);
            }
            other => return class_constant(self.names, other).into_iter().collect(),
        };
        return elements
            .iter()
            .filter_map(|element| {
                return match element {
                    ArrayElement::Value(value) => Some(value.value),
                    ArrayElement::KeyValue(pair) => Some(pair.value),
                    ArrayElement::Variadic(_) | ArrayElement::Missing(_) => None,
                };
            })
            .flat_map(|value| return self.classes(value, current))
            .collect();
    }

    fn arguments<'a>(arguments: &'a ArgumentList<'a>) -> impl Iterator<Item = &'a Expression<'a>> {
        return arguments.arguments.iter().map(|argument| {
            return match argument {
                Argument::Positional(positional) => positional.value,
                Argument::Named(named) => named.value,
            };
        });
    }

    fn attributes(
        &self,
        lists: &Sequence<'_, AttributeList<'_>>,
        class: &str,
        collecting: &mut Collecting,
    ) {
        for attribute in lists.iter().flat_map(|list| return list.attributes.iter()) {
            let name = text(self.names.get(&attribute.name));
            let values = attribute
                .argument_list
                .iter()
                .flat_map(|list| return list.arguments.iter())
                .filter_map(|argument| {
                    return match argument {
                        PartialArgument::Positional(positional) => Some(positional.value),
                        PartialArgument::Named(named) => Some(named.value),
                        PartialArgument::NamedPlaceholder(_)
                        | PartialArgument::Placeholder(_)
                        | PartialArgument::VariadicPlaceholder(_) => None,
                    };
                });
            if name.eq_ignore_ascii_case(OBSERVED_BY) {
                for observer in values.flat_map(|value| return self.classes(value, Some(class))) {
                    collecting
                        .hooks
                        .observers
                        .insert((observer, class.to_owned()));
                }
            } else if name.eq_ignore_ascii_case(AS_COMMAND)
                && let Some(command) = values
                    .take(1)
                    .find_map(string_literal)
                    .and_then(|value| return command_name(&value))
            {
                collecting.hooks.commands.insert(class.to_owned(), command);
            }
        }
    }

    fn enter(
        &self,
        name: &mago_syntax::cst::LocalIdentifier<'_>,
        collecting: &mut Collecting,
    ) -> String {
        let class = text(self.names.resolve(name).unwrap_or(name.value));
        collecting.classes.push(Some(class.clone()));
        return class;
    }
}

impl<'ast, 'arena> Walker<'ast, 'arena, Collecting> for Collector<'_, 'arena> {
    fn walk_in_class(&self, class: &'ast Class<'arena>, collecting: &mut Collecting) {
        let name = self.enter(&class.name, collecting);
        self.attributes(&class.attribute_lists, &name, collecting);
    }

    fn walk_out_class(&self, _: &'ast Class<'arena>, collecting: &mut Collecting) {
        collecting.classes.pop();
    }

    fn walk_in_trait(&self, used: &'ast Trait<'arena>, collecting: &mut Collecting) {
        self.enter(&used.name, collecting);
    }

    fn walk_out_trait(&self, _: &'ast Trait<'arena>, collecting: &mut Collecting) {
        collecting.classes.pop();
    }

    fn walk_in_enum(&self, enumeration: &'ast Enum<'arena>, collecting: &mut Collecting) {
        self.enter(&enumeration.name, collecting);
    }

    fn walk_out_enum(&self, _: &'ast Enum<'arena>, collecting: &mut Collecting) {
        collecting.classes.pop();
    }

    fn walk_in_anonymous_class(
        &self,
        _: &'ast mago_syntax::cst::AnonymousClass<'arena>,
        collecting: &mut Collecting,
    ) {
        collecting.classes.push(None);
    }

    fn walk_out_anonymous_class(
        &self,
        _: &'ast mago_syntax::cst::AnonymousClass<'arena>,
        collecting: &mut Collecting,
    ) {
        collecting.classes.pop();
    }

    fn walk_in_method(&self, method: &'ast Method<'arena>, collecting: &mut Collecting) {
        collecting.method = Some(text(method.name.value));
    }

    fn walk_out_method(&self, _: &'ast Method<'arena>, collecting: &mut Collecting) {
        collecting.method = None;
    }

    fn walk_in_property(&self, property: &'ast Property<'arena>, collecting: &mut Collecting) {
        let (Property::Plain(plain), Some(Some(class))) =
            (property, collecting.classes.last().cloned())
        else {
            return;
        };
        for item in &plain.items {
            let PropertyItem::Concrete(concrete) = item else {
                continue;
            };
            let variable = concrete.variable.name;
            if variable == DISPATCHES_EVENTS {
                let (Expression::Array(Array { elements, .. })
                | Expression::LegacyArray(LegacyArray { elements, .. })) = concrete.value
                else {
                    continue;
                };
                for element in elements {
                    let ArrayElement::KeyValue(pair) = element else {
                        continue;
                    };
                    if let (Some(event), Some(event_class)) = (
                        string_literal(pair.key),
                        class_constant(self.names, pair.value),
                    ) {
                        collecting
                            .hooks
                            .dispatched
                            .insert((class.clone(), event, event_class));
                    }
                }
            } else if (variable == SIGNATURE || variable == NAME)
                && let Some(command) =
                    string_literal(concrete.value).and_then(|value| return command_name(&value))
            {
                if variable == SIGNATURE {
                    collecting.hooks.commands.insert(class.clone(), command);
                } else {
                    collecting
                        .hooks
                        .commands
                        .entry(class.clone())
                        .or_insert(command);
                }
            }
        }
    }

    fn walk_in_static_method_call(
        &self,
        call: &'ast StaticMethodCall<'arena>,
        collecting: &mut Collecting,
    ) {
        let ClassLikeMemberSelector::Identifier(selector) = &call.method else {
            return;
        };
        let current = collecting.classes.last().cloned().flatten();
        let special = matches!(call.class, Expression::Self_(_) | Expression::Static(_));
        let model = match call.class {
            Expression::Identifier(identifier) => Some(text(self.names.get(identifier))),
            Expression::Self_(_) | Expression::Static(_) => current.clone(),
            _ => None,
        };
        let Some(model) = model else {
            return;
        };
        if selector.value.eq_ignore_ascii_case(OBSERVE) {
            for observer in Self::arguments(&call.argument_list)
                .flat_map(|argument| return self.classes(argument, current.as_deref()))
            {
                collecting.hooks.observers.insert((observer, model.clone()));
            }
            return;
        }
        let event = text(selector.value);
        if let (true, Some(method), Some(_)) =
            (special, collecting.method.clone(), calls_of(&event))
        {
            collecting
                .hooks
                .closures
                .entry((model, method))
                .or_default()
                .insert(event);
        }
    }
}
