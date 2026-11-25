use std::collections::BTreeMap;

use std::collections::BTreeSet;

use mago_codex::metadata::CodebaseMetadata;
use mago_word::{ascii_lowercase_word, word};

use crate::facts::{NEW, Reference, short_name};
use crate::hierarchy::Hierarchy;
use crate::laravel::Forwards;
use crate::laravel::{Hooks, ModelEvent};
use crate::target::{Kind, Request, Target};
use crate::types::{Classes, Reading, classes_of};

const SHOULD_QUEUE: &str = "Illuminate\\Contracts\\Queue\\ShouldQueue";
const BUS_DISPATCHABLE: &str = "Illuminate\\Foundation\\Bus\\Dispatchable";
const EVENT_DISPATCHABLE: &str = "Illuminate\\Foundation\\Events\\Dispatchable";
const SUBSCRIBE: &str = "subscribe";
const QUEUE_ENTRY_METHODS: [&str; 2] = ["handle", "failed"];
const COMMAND: &str = "Illuminate\\Console\\Command";
const COMMAND_ENTRY: &str = "handle";
const CONSTRUCTOR: &str = "__construct";
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

struct Fired {
    target: String,
    event: ModelEvent,
}

struct Run {
    target: String,
    class: String,
    name: String,
}

struct Creation {
    class: String,
    method: String,
    overridable: bool,
}

pub(crate) struct Matcher<'c> {
    hierarchy: Hierarchy<'c>,
    codebase: &'c CodebaseMetadata,
    forwards: &'c Forwards,
    members: BTreeMap<(Kind, String), Vec<Target>>,
    globals: BTreeMap<(Kind, String), Target>,
    creations: BTreeMap<String, Vec<Creation>>,
    model_events: Vec<Fired>,
    commands: Vec<Run>,
}

impl<'c> Matcher<'c> {
    pub(crate) fn new(
        codebase: &'c CodebaseMetadata,
        forwards: &'c Forwards,
        hooks: &Hooks,
        request: &Request,
    ) -> Self {
        let mut matcher = Self {
            hierarchy: Hierarchy(codebase),
            codebase,
            forwards,
            members: BTreeMap::new(),
            globals: BTreeMap::new(),
            creations: BTreeMap::new(),
            model_events: Vec::new(),
            commands: Vec::new(),
        };
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
            let queue_entry = QUEUE_ENTRY_METHODS
                .iter()
                .any(|method| return method.eq_ignore_ascii_case(&target.member));
            if queue_entry && matcher.bus_dispatched(class) {
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
            let listens_to = matcher.subscribed_events(class, &target.member);
            for event in hooks.model_events(class, &target.member, &listens_to) {
                matcher.model_events.push(Fired {
                    target: target.to_string(),
                    event,
                });
            }
            if target.member.eq_ignore_ascii_case(COMMAND_ENTRY)
                && matcher.hierarchy.is(class, COMMAND)
                && let Some(name) = hooks.command(class)
            {
                matcher.commands.push(Run {
                    target: target.to_string(),
                    class: class.clone(),
                    name: name.to_owned(),
                });
            }
            for event in listens_to {
                matcher.creations.entry(event).or_default().push(Creation {
                    class: class.clone(),
                    method: target.member.clone(),
                    overridable: false,
                });
            }
        }
        return matcher;
    }

    fn bus_dispatched(&self, class: &str) -> bool {
        return self
            .codebase
            .class_implements(class.as_bytes(), SHOULD_QUEUE.as_bytes())
            || self.hierarchy.uses_trait(class, BUS_DISPATCHABLE);
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
        if self.hierarchy.is_trait(target) {
            return self.hierarchy.uses_trait(receiver, target);
        }
        if self.hierarchy.is(target, receiver) {
            return true;
        }
        if !self.hierarchy.is(receiver, target) {
            return false;
        }
        return self
            .declaring(kind, receiver, member)
            .is_some_and(|declaring| {
                return self.hierarchy.is(target, &declaring)
                    || self.hierarchy.is_trait(&declaring)
                        && self.hierarchy.uses_trait(target, &declaring);
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
                        self.hierarchy.exists(class)
                            && receivers.iter().any(|receiver| {
                                return self.hierarchy.exists(receiver)
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
        if !self.hierarchy.exists(class) {
            return false;
        }
        let target_is_trait = self.hierarchy.is_trait(class);
        return classes
            .iter()
            .filter(|candidate| return self.hierarchy.exists(candidate))
            .any(|candidate| {
                return if target_is_trait {
                    self.hierarchy.uses_trait(candidate, class)
                } else {
                    self.hierarchy.is(candidate, class)
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
                return (fires(&entry.event.instance)
                    && self.any_is(instance_of, &entry.event.model))
                    || (fires(&entry.event.query) && self.any_is(query_of, &entry.event.model));
            })
            .map(|entry| return entry.target.clone())
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
            .map(|entry| return entry.target.clone())
            .collect();
    }

    fn creates_with(&self, class: &str, method: &str) -> bool {
        let method = ascii_lowercase_word(method.as_bytes());
        return [BUS_DISPATCHABLE, EVENT_DISPATCHABLE].iter().any(|used| {
            return self.hierarchy.uses_trait(class, used)
                && self
                    .codebase
                    .get_class_like(used.as_bytes())
                    .is_some_and(|metadata| return metadata.methods.contains(&method));
        });
    }

    fn creation_use(&self, classes: &[String]) -> Vec<String> {
        let mut found = Vec::new();
        for created in classes
            .iter()
            .filter(|class| return self.hierarchy.exists(class))
        {
            for (entry_class, creations) in &self.creations {
                if !self.hierarchy.is(created, entry_class) {
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

    pub(crate) fn matches(&self, reference: &Reference) -> Vec<String> {
        return match reference {
            Reference::Function { candidates } => self.global_use(Kind::Function, candidates),
            Reference::Constant { candidates } => self.global_use(Kind::GlobalConstant, candidates),
            Reference::Method {
                member,
                receivers,
                query,
                names,
            } => {
                let mut found =
                    self.member_use(Kind::Method, member, &self.with_forwards(receivers.clone()));
                found.extend(self.model_event_use(member, receivers, query));
                found.extend(self.command_use(member, receivers, names));
                found
            }
            Reference::Static {
                member,
                receivers,
                names,
            } => {
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
                found.extend(self.model_event_use(member, &Classes::Known(Vec::new()), receivers));
                found.extend(self.command_use(member, receivers, names));
                found
            }
            Reference::New { classes } => {
                let mut found = self.member_use(Kind::Method, CONSTRUCTOR, classes);
                found.extend(self.creation_use(classes.as_known().unwrap_or_default()));
                found
            }
            Reference::ClassConstant { member, classes } => {
                self.member_use(Kind::Constant, member, classes)
            }
            Reference::Property { member, receivers } => {
                self.member_use(Kind::Property, member, receivers)
            }
        };
    }

    pub(crate) fn needles(&self) -> BTreeSet<String> {
        let constructs = self
            .members
            .contains_key(&(Kind::Method, CONSTRUCTOR.to_owned()));
        let creates = (constructs || !self.creations.is_empty()).then_some(NEW);
        let mut needles: BTreeSet<String> = self
            .members
            .keys()
            .chain(self.globals.keys())
            .map(|(_, member)| return short_name(member))
            .chain(creates)
            .map(str::to_ascii_lowercase)
            .collect();
        if !self.creations.is_empty() {
            for used in [BUS_DISPATCHABLE, EVENT_DISPATCHABLE] {
                let methods = self
                    .codebase
                    .get_class_like(used.as_bytes())
                    .into_iter()
                    .flat_map(|metadata| return metadata.methods.iter());
                needles
                    .extend(methods.map(|method| return method.to_string().to_ascii_lowercase()));
            }
        }
        for entry in &self.model_events {
            needles.extend(
                entry
                    .event
                    .instance
                    .iter()
                    .chain(entry.event.query.iter())
                    .map(|method| return method.to_ascii_lowercase()),
            );
        }
        if !self.commands.is_empty() {
            needles.extend(
                COMMAND_CALLERS
                    .iter()
                    .flat_map(|(_, methods)| return methods.iter())
                    .map(|method| return method.to_ascii_lowercase()),
            );
        }
        needles.remove("");
        return needles;
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
            Some(class) if self.hierarchy.is(using, &class) => format!("{target}{VIA}{using}"),
            _ => target,
        };
    }
}
