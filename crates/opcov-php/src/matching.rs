use std::collections::{BTreeMap, BTreeSet};

use mago_analyzer::artifacts::AnalysisArtifacts;
use mago_codex::metadata::CodebaseMetadata;
use mago_codex::ttype::atomic::TAtomic;
use mago_codex::ttype::atomic::object::TObject;
use mago_codex::ttype::union::TUnion;
use mago_span::Span;
use mago_word::{ascii_lowercase_word, word};

use crate::laravel::Forwards;
use crate::sites::{ClassReference, FirstArgument, Site};
use crate::target::{Command, Kind, ModelEvent, Request, Target};
use crate::types::{Classes, Reading, classes_of, literal_strings};

const BUILDER: &str = "Illuminate\\Database\\Eloquent\\Builder";
const RELATION: &str = "Illuminate\\Database\\Eloquent\\Relations\\Relation";
const MODEL: &str = "Illuminate\\Database\\Eloquent\\Model";
const BUILDER_TEMPLATE: &str = "TModel";
const RELATION_TEMPLATE: &str = "TRelatedModel";
const SHOULD_QUEUE: &str = "Illuminate\\Contracts\\Queue\\ShouldQueue";
const BUS_DISPATCHABLE: &str = "Illuminate\\Foundation\\Bus\\Dispatchable";
const EVENT_DISPATCHABLE: &str = "Illuminate\\Foundation\\Events\\Dispatchable";
const SUBSCRIBE: &str = "subscribe";
const CONSTRUCTOR: &str = "__construct";
const NEW: &str = "new";
const VIA: &str = " via ";
const COMMAND_CALLERS: [(&str, &[&str]); 5] = [
    ("Illuminate\\Support\\Facades\\Artisan", &["call", "queue"]),
    ("Illuminate\\Support\\Facades\\Schedule", &["command"]),
    (
        "Illuminate\\Console\\Command",
        &["call", "callSilent", "callSilently"],
    ),
    ("Illuminate\\Contracts\\Console\\Kernel", &["call", "queue"]),
    ("Illuminate\\Console\\Scheduling\\Schedule", &["command"]),
];

fn lower(text: &str) -> String {
    return text.to_ascii_lowercase();
}

struct Creation {
    class: String,
    method: String,
    overridable: bool,
}

pub(crate) struct Matcher<'c> {
    codebase: &'c CodebaseMetadata,
    forwards: &'c Forwards,
    members: BTreeMap<(Kind, String), Vec<Target>>,
    globals: BTreeMap<(Kind, String), Target>,
    creations: BTreeMap<String, Vec<Creation>>,
    model_events: Vec<ModelEvent>,
    commands: Vec<Command>,
    trait_users: BTreeMap<String, Vec<String>>,
}

impl<'c> Matcher<'c> {
    pub(crate) fn new(
        codebase: &'c CodebaseMetadata,
        forwards: &'c Forwards,
        request: &Request,
    ) -> Self {
        let mut matcher = Self {
            codebase,
            forwards,
            members: BTreeMap::new(),
            globals: BTreeMap::new(),
            creations: BTreeMap::new(),
            model_events: request.model_event_targets.clone(),
            commands: request.command_targets.clone(),
            trait_users: BTreeMap::new(),
        };
        let mut classes: Vec<_> = codebase.class_likes.values().collect();
        classes.sort_by_key(|class| return class.name);
        for class in classes {
            let mut used: Vec<String> = class
                .used_traits
                .iter()
                .map(|used| return lower(&used.to_string()))
                .collect();
            used.sort();
            for trait_name in used {
                matcher
                    .trait_users
                    .entry(trait_name)
                    .or_default()
                    .push(class.original_name.to_string());
            }
        }
        let queue_entry_methods: BTreeSet<String> = request
            .queue_entry_methods
            .iter()
            .map(|method| return lower(method))
            .collect();
        for target in &request.targets {
            let key = (target.kind, target.kind.member_key(&target.member));
            let Some(class) = &target.class else {
                matcher.globals.insert(key, target.clone());
                continue;
            };
            matcher.members.entry(key).or_default().push(target.clone());
            if target.kind != Kind::Method {
                continue;
            }
            if queue_entry_methods.contains(&lower(&target.member)) && matcher.bus_dispatched(class)
            {
                matcher
                    .creations
                    .entry(class.clone())
                    .or_default()
                    .push(Creation {
                        class: class.clone(),
                        method: target.member.clone(),
                        overridable: true,
                    });
            }
            for event in matcher.subscribed_events(class, &target.member) {
                matcher.creations.entry(event).or_default().push(Creation {
                    class: class.clone(),
                    method: target.member.clone(),
                    overridable: false,
                });
            }
        }
        return matcher;
    }

    fn exists(&self, class: &str) -> bool {
        return self.codebase.class_like_exists(class.as_bytes());
    }

    fn is(&self, child: &str, parent: &str) -> bool {
        return child.eq_ignore_ascii_case(parent)
            || self
                .codebase
                .is_instance_of(child.as_bytes(), parent.as_bytes());
    }

    fn is_trait(&self, class: &str) -> bool {
        return self.codebase.trait_exists(class.as_bytes());
    }

    fn uses_trait(&self, class: &str, used: &str) -> bool {
        if self
            .codebase
            .class_uses_trait(class.as_bytes(), used.as_bytes())
        {
            return true;
        }
        return self
            .codebase
            .get_class_like(class.as_bytes())
            .is_some_and(|metadata| {
                return metadata.all_parent_classes.iter().any(|parent| {
                    return self
                        .codebase
                        .class_uses_trait(parent.as_bytes(), used.as_bytes());
                });
            });
    }

    fn bus_dispatched(&self, class: &str) -> bool {
        return self
            .codebase
            .class_implements(class.as_bytes(), SHOULD_QUEUE.as_bytes())
            || self.uses_trait(class, BUS_DISPATCHABLE);
    }

    fn subscribed_events(&self, class: &str, method: &str) -> Vec<String> {
        if self
            .codebase
            .get_method(class.as_bytes(), SUBSCRIBE.as_bytes())
            .is_none()
        {
            return Vec::new();
        }
        let parameter = self
            .codebase
            .get_method(class.as_bytes(), method.as_bytes())
            .and_then(|metadata| return metadata.parameters.first());
        let declared = parameter.and_then(|parameter| {
            return parameter
                .type_metadata
                .as_ref()
                .or(parameter.type_declaration_metadata.as_ref());
        });
        return match declared
            .map(|declared| return classes_of(&declared.type_union, None, Reading::Objects))
        {
            Some(Classes::Known(classes)) => classes,
            Some(Classes::Unknown) | None => Vec::new(),
        };
    }

    fn declaring(&self, kind: Kind, receiver: &str, member: &str) -> Option<String> {
        return match kind {
            Kind::Method => self
                .codebase
                .get_declaring_method_class(receiver.as_bytes(), member.as_bytes())
                .map(|class| return class.to_string()),
            Kind::Property => self
                .codebase
                .get_declaring_property_class(receiver.as_bytes(), format!("${member}").as_bytes())
                .map(|class| return class.to_string()),
            Kind::Constant => self.declaring_constant(receiver, member),
            Kind::Function | Kind::GlobalConstant => None,
        };
    }

    fn declaring_constant(&self, receiver: &str, member: &str) -> Option<String> {
        let name = word(member.as_bytes());
        let declares = |class: &str| {
            return self
                .codebase
                .get_class_like(class.as_bytes())
                .is_some_and(|metadata| {
                    return metadata.constants.contains_key(&name)
                        || metadata.enum_cases.contains_key(&name);
                });
        };
        let metadata = self.codebase.get_class_like(receiver.as_bytes())?;
        if !declares(receiver) {
            return None;
        }
        let topmost = metadata
            .all_parent_classes
            .iter()
            .map(|parent| return parent.to_string())
            .filter(|parent| return declares(parent))
            .max_by_key(|parent| {
                return self
                    .codebase
                    .get_class_like(parent.as_bytes())
                    .map_or(0, |ancestor| return ancestor.all_parent_classes.len());
            });
        return Some(topmost.unwrap_or_else(|| return metadata.original_name.to_string()));
    }

    fn receiver_reaches(&self, kind: Kind, receiver: &str, target: &str, member: &str) -> bool {
        if self.is_trait(target) {
            return self.uses_trait(receiver, target);
        }
        if self.is(target, receiver) {
            return true;
        }
        if !self.is(receiver, target) {
            return false;
        }
        return self
            .declaring(kind, receiver, member)
            .is_some_and(|declaring| {
                return self.is(target, &declaring)
                    || self.is_trait(&declaring) && self.uses_trait(target, &declaring);
            });
    }

    fn with_forwards(&self, receivers: Classes) -> Classes {
        let Classes::Known(mut classes) = receivers else {
            return Classes::Unknown;
        };
        let forwarded: Vec<String> = classes
            .iter()
            .filter_map(|class| return self.forwards.get(&lower(class)))
            .flatten()
            .cloned()
            .collect();
        classes.extend(forwarded);
        classes.sort();
        classes.dedup();
        return Classes::Known(classes);
    }

    fn member_use(&self, kind: Kind, member: &str, receivers: &Classes) -> Vec<String> {
        let candidates = self
            .members
            .get(&(kind, kind.member_key(member)))
            .into_iter()
            .flatten();
        return candidates
            .filter(|target| {
                let Some(class) = &target.class else {
                    return false;
                };
                return match receivers {
                    Classes::Unknown => true,
                    Classes::Known(receivers) => {
                        self.exists(class)
                            && receivers.iter().any(|receiver| {
                                return self.exists(receiver)
                                    && self.receiver_reaches(kind, receiver, class, member);
                            })
                    }
                };
            })
            .map(ToString::to_string)
            .collect();
    }

    fn global_use(&self, kind: Kind, candidates: &[String]) -> Vec<String> {
        return candidates
            .iter()
            .find_map(|candidate| return self.globals.get(&(kind, kind.member_key(candidate))))
            .map_or_else(Vec::new, |target| return vec![target.to_string()]);
    }

    fn any_is(&self, classes: &Classes, class: &str) -> bool {
        let Classes::Known(classes) = classes else {
            return true;
        };
        if !self.exists(class) {
            return false;
        }
        let target_is_trait = self.is_trait(class);
        return classes
            .iter()
            .filter(|candidate| return self.exists(candidate))
            .any(|candidate| {
                return if target_is_trait {
                    self.uses_trait(candidate, class)
                } else {
                    self.is(candidate, class)
                };
            });
    }

    fn model_event_use(
        &self,
        method: &str,
        instance_of: &Classes,
        query_of: &Classes,
    ) -> Vec<String> {
        let fires = |names: &[String]| {
            return names
                .iter()
                .any(|name| return name.eq_ignore_ascii_case(method));
        };
        return self
            .model_events
            .iter()
            .filter(|entry| {
                return (fires(&entry.instance) && self.any_is(instance_of, &entry.model))
                    || (fires(&entry.query) && self.any_is(query_of, &entry.model));
            })
            .map(|entry| return entry.target.to_string())
            .collect();
    }

    fn command_use(&self, method: &str, receivers: &Classes, names: &[String]) -> Vec<String> {
        let calls_commands = COMMAND_CALLERS.iter().any(|(caller, methods)| {
            return methods
                .iter()
                .any(|name| return name.eq_ignore_ascii_case(method))
                && self.any_is(receivers, caller);
        });
        if !calls_commands {
            return Vec::new();
        }
        return self
            .commands
            .iter()
            .filter(|entry| {
                return names.iter().any(|name| {
                    return name.eq_ignore_ascii_case(&entry.class) || *name == entry.name;
                });
            })
            .map(|entry| return entry.target.to_string())
            .collect();
    }

    fn creates_with(&self, class: &str, method: &str) -> bool {
        let method = ascii_lowercase_word(method.as_bytes());
        return [BUS_DISPATCHABLE, EVENT_DISPATCHABLE].iter().any(|used| {
            return self.uses_trait(class, used)
                && self
                    .codebase
                    .get_class_like(used.as_bytes())
                    .is_some_and(|metadata| return metadata.methods.contains(&method));
        });
    }

    fn creation_use(&self, classes: &[String]) -> Vec<String> {
        let mut found = Vec::new();
        for created in classes.iter().filter(|class| return self.exists(class)) {
            for (entry_class, creations) in &self.creations {
                if !self.is(created, entry_class) {
                    continue;
                }
                for creation in creations {
                    let takes = !creation.overridable
                        || self
                            .declaring(Kind::Method, created, &creation.method)
                            .is_some_and(|declaring| {
                                return declaring.eq_ignore_ascii_case(&creation.class);
                            });
                    if takes {
                        found.push(format!("method {}::{}", creation.class, creation.method));
                    }
                }
            }
        }
        return found;
    }

    fn template_argument(
        &self,
        class: &str,
        parameters: Option<&[TUnion]>,
        ancestor: &str,
        template: &str,
    ) -> Option<TUnion> {
        if !self.is(class, ancestor) {
            return None;
        }
        let metadata = self.codebase.get_class_like(class.as_bytes())?;
        let own = |name: &str| {
            return metadata
                .template_types
                .get_index_of(&word(name.as_bytes()))
                .and_then(|position| return parameters?.get(position).cloned());
        };
        if let Some(bound) = own(template) {
            return Some(bound);
        }
        let resolved = metadata
            .template_extended_parameters
            .get(&ascii_lowercase_word(ancestor.as_bytes()))?
            .get(&word(template.as_bytes()))?;
        let forwarded = resolved.types.iter().find_map(|atomic| {
            return match atomic {
                TAtomic::GenericParameter(parameter) => own(&parameter.parameter_name.to_string()),
                _ => None,
            };
        });
        return Some(forwarded.unwrap_or_else(|| return resolved.clone()));
    }

    fn query_models(&self, union: &TUnion, this: Option<&str>) -> Classes {
        if classes_of(union, this, Reading::Objects) == Classes::Unknown {
            return Classes::Unknown;
        }
        let mut models = Vec::new();
        for atomic in union.types.iter() {
            let TAtomic::Object(TObject::Named(named)) = atomic else {
                continue;
            };
            for (ancestor, template) in [(BUILDER, BUILDER_TEMPLATE), (RELATION, RELATION_TEMPLATE)]
            {
                let Some(bound) = self.template_argument(
                    &named.name.to_string(),
                    named.get_type_parameters(),
                    ancestor,
                    template,
                ) else {
                    continue;
                };
                let Classes::Known(queried) = classes_of(&bound, this, Reading::Objects) else {
                    return Classes::Unknown;
                };
                let concrete: Vec<String> = queried
                    .into_iter()
                    .filter(|name| return !name.eq_ignore_ascii_case(MODEL))
                    .collect();
                if concrete.is_empty() {
                    return Classes::Unknown;
                }
                models.extend(concrete);
            }
        }
        models.sort();
        models.dedup();
        return Classes::Known(models);
    }

    pub(crate) fn evaluate(
        &self,
        site: &Site,
        artifacts: &AnalysisArtifacts,
        this: Option<&str>,
    ) -> Vec<String> {
        let type_of = |span: &Span| return artifacts.get_expression_type(span);
        let objects = |span: &Span| {
            return type_of(span).map_or(Classes::Unknown, |union| {
                return classes_of(union, this, Reading::Objects);
            });
        };
        let classes = |reference: &ClassReference| {
            return match reference {
                ClassReference::Named(name) => Classes::Known(vec![name.clone()]),
                ClassReference::Current => this.map_or(Classes::Unknown, |class| {
                    return Classes::Known(vec![class.to_owned()]);
                }),
                ClassReference::Parent(parent) => {
                    parent.as_ref().map_or(Classes::Unknown, |parent| {
                        return Classes::Known(vec![parent.clone()]);
                    })
                }
                ClassReference::Expression(span) => {
                    type_of(span).map_or(Classes::Unknown, |union| {
                        return classes_of(union, this, Reading::ClassNames);
                    })
                }
            };
        };
        let names = |argument: Option<&FirstArgument>| {
            let Some(argument) = argument else {
                return Vec::new();
            };
            let mut names = type_of(&argument.span).map_or_else(Vec::new, literal_strings);
            names.extend(argument.literal.clone());
            return names;
        };
        return match site {
            Site::Function { candidates } => self.global_use(Kind::Function, candidates),
            Site::Constant { candidates } => self.global_use(Kind::GlobalConstant, candidates),
            Site::Method { member: None, .. } | Site::Static { member: None, .. } => Vec::new(),
            Site::Method {
                receiver,
                member: Some(member),
                first_argument,
            } => {
                let receivers = objects(receiver);
                let query = type_of(receiver).map_or(Classes::Unknown, |union| {
                    return self.query_models(union, this);
                });
                let mut found =
                    self.member_use(Kind::Method, member, &self.with_forwards(receivers.clone()));
                found.extend(self.model_event_use(member, &receivers, &query));
                found.extend(self.command_use(member, &receivers, &names(first_argument.as_ref())));
                found
            }
            Site::Static {
                class,
                member: Some(member),
                first_argument,
            } => {
                let receivers = classes(class);
                let mut found =
                    self.member_use(Kind::Method, member, &self.with_forwards(receivers.clone()));
                if let Some(known) = receivers.as_known() {
                    let created: Vec<String> = known
                        .iter()
                        .filter(|class| return self.creates_with(class, member))
                        .cloned()
                        .collect();
                    found.extend(self.creation_use(&created));
                }
                found.extend(self.model_event_use(member, &Classes::Known(Vec::new()), &receivers));
                found.extend(self.command_use(member, &receivers, &names(first_argument.as_ref())));
                found
            }
            Site::New { class } => {
                let receivers = classes(class);
                let mut found = self.member_use(Kind::Method, CONSTRUCTOR, &receivers);
                found.extend(self.creation_use(receivers.as_known().unwrap_or_default()));
                found
            }
            Site::ClassConstant { class, member } => {
                self.member_use(Kind::Constant, member, &classes(class))
            }
            Site::Property { receiver, member } => {
                self.member_use(Kind::Property, member, &objects(receiver))
            }
            Site::StaticProperty { class, member } => {
                self.member_use(Kind::Property, member, &classes(class))
            }
        };
    }

    pub(crate) fn needles(&self) -> Vec<String> {
        let mut needles: Vec<String> = self
            .members
            .keys()
            .chain(self.globals.keys())
            .map(|(_, member)| return member.rsplit('\\').next().unwrap_or(member).to_owned())
            .collect();
        let constructs = self
            .members
            .contains_key(&(Kind::Method, CONSTRUCTOR.to_owned()));
        if constructs || !self.creations.is_empty() {
            needles.push(NEW.to_owned());
        }
        if !self.creations.is_empty() {
            for used in [BUS_DISPATCHABLE, EVENT_DISPATCHABLE] {
                let methods = self
                    .codebase
                    .get_class_like(used.as_bytes())
                    .into_iter()
                    .flat_map(|metadata| return metadata.methods.iter());
                needles.extend(methods.map(ToString::to_string));
            }
        }
        for entry in &self.model_events {
            needles.extend(entry.instance.iter().chain(entry.query.iter()).cloned());
        }
        if !self.commands.is_empty() {
            needles.extend(
                COMMAND_CALLERS
                    .iter()
                    .flat_map(|(_, methods)| return methods.iter())
                    .map(|method| return (*method).to_owned()),
            );
        }
        needles.retain(|needle| return !needle.is_empty());
        needles.sort();
        needles.dedup();
        return needles;
    }

    pub(crate) fn trait_users(&self, used: &str) -> &[String] {
        return self
            .trait_users
            .get(&lower(used))
            .map_or(&[], Vec::as_slice);
    }

    pub(crate) fn attributed(&self, target: String, using: Option<&str>) -> String {
        let Some(using) = using else {
            return target;
        };
        let class = target
            .split_once(' ')
            .and_then(|(_, member)| return member.split_once("::"))
            .map(|(class, _)| return class.to_owned());
        return match class {
            Some(class) if self.is(using, &class) => format!("{target}{VIA}{using}"),
            _ => target,
        };
    }
}
