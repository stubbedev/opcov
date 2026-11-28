use std::collections::BTreeMap;

use mago_codex::identifier::method::MethodIdentifier;
use mago_codex::metadata::CodebaseMetadata;
use mago_codex::metadata::flags::MetadataFlags;
use mago_codex::metadata::function_like::FunctionLikeMetadata;
use mago_codex::metadata::property::PropertyMetadata;
use mago_codex::metadata::ttype::TypeMetadata;
use mago_codex::misc::{GenericParent, VariableIdentifier};
use mago_codex::ttype::TType;
use mago_codex::ttype::atomic::TAtomic;
use mago_codex::ttype::atomic::object::TObject;
use mago_codex::ttype::atomic::object::named::TNamedObject;
use mago_codex::ttype::expander::{StaticClassType, TypeExpansionOptions, expand_union};
use mago_codex::ttype::template::TemplateResult;
use mago_codex::ttype::template::inferred_type_replacer;
use mago_codex::ttype::union::TUnion;
use mago_codex::visibility::Visibility;
use mago_word::{Word, ascii_lowercase_word};

pub(super) fn key(name: &[u8]) -> Word {
    return ascii_lowercase_word(name);
}

pub(super) fn object(name: Word, parameters: Option<Vec<TUnion>>) -> TNamedObject {
    return TNamedObject::new_with_type_parameters(name, parameters);
}

pub(super) fn object_union(object: TNamedObject) -> TUnion {
    return TUnion::from_atomic(TAtomic::Object(TObject::Named(object)));
}

pub(super) fn declared_return(function: &FunctionLikeMetadata) -> Option<&TUnion> {
    return function
        .return_type_metadata
        .as_ref()
        .or(function.return_type_declaration_metadata.as_ref())
        .map(|declared| return &declared.type_union);
}

pub(super) fn returned_objects(
    function: &FunctionLikeMetadata,
) -> impl Iterator<Item = &TNamedObject> {
    return declared_return(function)
        .into_iter()
        .flat_map(|union| return union.types.iter())
        .filter_map(|atomic| {
            return match atomic {
                TAtomic::Object(TObject::Named(object)) => Some(object),
                _ => None,
            };
        });
}

pub(super) struct Binding {
    receiver: TNamedObject,
    inherited: TemplateResult,
    own: TemplateResult,
}

impl Binding {
    pub(super) fn for_object(metadata: &CodebaseMetadata, receiver: TNamedObject) -> Self {
        let mut inherited = TemplateResult::default();
        let mut own = TemplateResult::default();
        if let Some(class) = metadata.class_likes.get(&key(receiver.name.as_bytes())) {
            #[expect(
                clippy::iter_over_hash_type,
                reason = "each template gets its own lower bound, so the order they are added in does not matter"
            )]
            for (defining, parameters) in &class.template_extended_parameters {
                for (parameter, bound) in parameters {
                    inherited.add_lower_bound(
                        *parameter,
                        GenericParent::ClassLike(*defining),
                        bound.clone(),
                    );
                }
            }
            for (position, bound) in receiver
                .get_type_parameters()
                .into_iter()
                .flatten()
                .enumerate()
            {
                if let Some(parameter) = class.get_template_name_for_index(position) {
                    own.add_lower_bound(
                        parameter,
                        GenericParent::ClassLike(class.name),
                        bound.clone(),
                    );
                }
            }
        }
        return Self {
            receiver,
            inherited,
            own,
        };
    }

    pub(super) const fn receiver(&self) -> &TNamedObject {
        return &self.receiver;
    }

    pub(super) fn bind(&self, metadata: &CodebaseMetadata, union: &TUnion) -> TUnion {
        let inherited = inferred_type_replacer::replace(union, &self.inherited, metadata);
        let mut bound = inferred_type_replacer::replace(&inherited, &self.own, metadata);
        let options = TypeExpansionOptions {
            self_class: Some(self.receiver.name),
            static_class_type: StaticClassType::Object(TObject::Named(self.receiver.clone())),
            preserve_parameter_dependencies: true,
            preserve_template_conditionals: true,
            ..TypeExpansionOptions::default()
        };
        expand_union(metadata, &mut bound, &options);
        return bound;
    }

    fn bind_function(&self, metadata: &CodebaseMetadata, function: &mut FunctionLikeMetadata) {
        let parameters = function.parameters.iter_mut().flat_map(|parameter| {
            return [
                &mut parameter.type_metadata,
                &mut parameter.type_declaration_metadata,
            ];
        });
        let returns = [
            &mut function.return_type_metadata,
            &mut function.return_type_declaration_metadata,
        ];
        for slot in returns.into_iter().chain(parameters) {
            if let Some(declared) = slot.as_mut().filter(|declared| {
                return declared.type_union.has_template_types()
                    || declared.type_union.is_expandable();
            }) {
                declared.type_union = self.bind(metadata, &declared.type_union);
            }
        }
    }
}

pub(super) const fn static_magic(mut function: FunctionLikeMetadata) -> FunctionLikeMetadata {
    function.flags.set(MetadataFlags::MAGIC_METHOD, true);
    function.flags.set(MetadataFlags::STATIC, true);
    if let Some(method) = function.method_metadata.as_mut() {
        method.is_static = true;
    }
    return function;
}

fn public_methods(metadata: &CodebaseMetadata, class: Word) -> Vec<(Word, MethodIdentifier)> {
    let Some(class) = metadata.class_likes.get(&key(class.as_bytes())) else {
        return Vec::new();
    };
    let mut methods: Vec<(Word, MethodIdentifier)> = class
        .declaring_method_ids
        .iter()
        .filter(|(name, _)| return !name.as_bytes().starts_with(b"__"))
        .filter(|(_, id)| {
            return metadata
                .get_method_by_id(id)
                .and_then(|function| return function.method_metadata.as_ref())
                .is_some_and(|method| return method.visibility == Visibility::Public);
        })
        .map(|(name, id)| return (*name, *id))
        .collect();
    methods.sort();
    return methods;
}

pub(super) enum Member {
    Method {
        name: Word,
        function: Box<FunctionLikeMetadata>,
    },
    Property {
        name: Word,
        value: TUnion,
    },
}

pub(super) type Members = BTreeMap<Word, Member>;

pub(super) struct Forwarding {
    targets: Vec<Word>,
    binding: Binding,
}

impl Forwarding {
    pub(super) const fn new(targets: Vec<Word>, binding: Binding) -> Self {
        return Self { targets, binding };
    }

    pub(super) fn add(&self, metadata: &CodebaseMetadata, owner: Word, members: &mut Members) {
        let Some(class) = metadata.class_likes.get(&key(owner.as_bytes())) else {
            return;
        };
        for target in &self.targets {
            for (name, id) in public_methods(metadata, *target) {
                if class.declaring_method_ids.contains_key(&name) || members.contains_key(&name) {
                    continue;
                }
                let Some(source) = metadata.get_method_by_id(&id) else {
                    continue;
                };
                let mut function = source.clone();
                self.binding.bind_function(metadata, &mut function);
                members.insert(
                    name,
                    Member::Method {
                        name,
                        function: Box::new(static_magic(function)),
                    },
                );
            }
        }
    }
}

pub(super) fn install(metadata: &mut CodebaseMetadata, owner: Word, members: Members) {
    let owner_key = key(owner.as_bytes());
    let Some(class) = metadata.class_likes.get(&owner_key) else {
        return;
    };
    let (declaring, span) = (class.name, class.span);
    let mut owners = vec![owner_key];
    owners.extend(
        metadata
            .all_class_like_descendants
            .get(&owner_key)
            .into_iter()
            .flatten()
            .copied(),
    );
    for member in members.into_values() {
        match member {
            Member::Method { name, function } => {
                let id = MethodIdentifier::new(declaring, name);
                for target in &owners {
                    let Some(class) = metadata.class_likes.get_mut(target) else {
                        continue;
                    };
                    if *target != owner_key && class.declaring_method_ids.contains_key(&name) {
                        continue;
                    }
                    class.methods.insert(name);
                    class.pseudo_methods.insert(name);
                    class.declaring_method_ids.insert(name, id);
                    class.appearing_method_ids.insert(name, id);
                    class.inheritable_method_ids.insert(name, id);
                }
                metadata.function_likes.insert((owner_key, name), function);
            }
            Member::Property { name, value } => {
                for target in &owners {
                    let Some(class) = metadata.class_likes.get_mut(target) else {
                        continue;
                    };
                    if class.properties.contains_key(&name)
                        || class.magic_properties.contains_key(&name)
                    {
                        continue;
                    }
                    let mut property =
                        PropertyMetadata::new(VariableIdentifier(name), MetadataFlags::empty());
                    property.read_visibility = Visibility::Public;
                    property.write_visibility = Visibility::Public;
                    property.type_metadata = Some(TypeMetadata::new(value.clone(), span));
                    class.magic_properties.insert(name, property);
                    class.magic_property_ids.insert(name, declaring);
                }
            }
        }
    }
}
