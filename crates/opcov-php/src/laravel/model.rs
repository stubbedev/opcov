use mago_codex::metadata::CodebaseMetadata;
use mago_codex::metadata::flags::MetadataFlags;
use mago_codex::metadata::function_like::{FunctionLikeKind, FunctionLikeMetadata};
use mago_codex::metadata::ttype::TypeMetadata;
use mago_codex::ttype::atomic::TAtomic;
use mago_codex::ttype::atomic::object::named::TNamedObject;
use mago_codex::ttype::get_int;
use mago_codex::ttype::union::TUnion;
use mago_word::{Word, word};

use super::members::{
    Binding, Forwarding, Member, Members, declared_return, key, object, object_union,
    returned_objects, static_magic,
};

pub(super) const MODEL: &[u8] = b"Illuminate\\Database\\Eloquent\\Model";
pub(super) const ELOQUENT_BUILDER: &[u8] = b"Illuminate\\Database\\Eloquent\\Builder";
pub(super) const MODEL_TEMPLATE: &[u8] = b"TModel";
pub(super) const QUERY_BUILDER: &[u8] = b"Illuminate\\Database\\Query\\Builder";
pub(super) const RELATION: &[u8] = b"Illuminate\\Database\\Eloquent\\Relations\\Relation";
const COLLECTION: &[u8] = b"Illuminate\\Database\\Eloquent\\Collection";
const SCOPE_ATTRIBUTE: &[u8] = b"Illuminate\\Database\\Eloquent\\Attributes\\Scope";
const SCOPE_PREFIX: &str = "scope";
const BUILDER_FACTORY: &[u8] = b"newEloquentBuilder";
const RELATED_TEMPLATE: &[u8] = b"TRelatedModel";
const QUERY_FACTORIES: [&[u8]; 5] = [
    b"query",
    b"newQuery",
    b"newModelQuery",
    b"newQueryWithoutScopes",
    b"newQueryWithoutRelationships",
];
const TO_MANY: [&[u8]; 5] = [
    b"Illuminate\\Database\\Eloquent\\Relations\\HasMany",
    b"Illuminate\\Database\\Eloquent\\Relations\\BelongsToMany",
    b"Illuminate\\Database\\Eloquent\\Relations\\MorphMany",
    b"Illuminate\\Database\\Eloquent\\Relations\\MorphToMany",
    b"Illuminate\\Database\\Eloquent\\Relations\\HasManyThrough",
];

pub(super) struct Model {
    pub(super) name: Word,
    pub(super) builder: TNamedObject,
}

impl Model {
    pub(super) fn of(metadata: &CodebaseMetadata, name: Word) -> Self {
        let model = object(name, None);
        let custom = metadata
            .class_likes
            .get(&key(name.as_bytes()))
            .and_then(|class| return class.declaring_method_ids.get(&key(BUILDER_FACTORY)))
            .filter(|id| return !id.get_class_name().as_bytes().eq_ignore_ascii_case(MODEL))
            .and_then(|id| return metadata.get_method_by_id(id))
            .and_then(|function| {
                return returned_objects(function)
                    .map(|returned| return returned.name)
                    .find(|class| {
                        return metadata.is_instance_of(class.as_bytes(), ELOQUENT_BUILDER);
                    });
            });
        let builder = match custom {
            Some(class) => {
                let templated = metadata
                    .class_likes
                    .get(&key(class.as_bytes()))
                    .is_some_and(|builder| {
                        return builder.template_types.contains_key(&word(MODEL_TEMPLATE));
                    });
                object(
                    class,
                    templated.then(|| return vec![object_union(model.clone())]),
                )
            }
            None => object(word(ELOQUENT_BUILDER), Some(vec![object_union(model)])),
        };
        return Self { name, builder };
    }

    pub(super) fn members(&self, metadata: &CodebaseMetadata) -> Members {
        let mut members = Members::new();
        let Some(class) = metadata.class_likes.get(&key(self.name.as_bytes())) else {
            return members;
        };
        let binding = Binding::for_object(metadata, self.builder.clone());
        for factory in QUERY_FACTORIES {
            let name = key(factory);
            let Some(source) = class
                .declaring_method_ids
                .get(&name)
                .and_then(|id| return metadata.get_method_by_id(id))
            else {
                continue;
            };
            let mut function = source.clone();
            function.return_type_metadata = Some(TypeMetadata::new(
                object_union(self.builder.clone()),
                source.span,
            ));
            function.return_type_declaration_metadata = None;
            members.insert(
                name,
                Member::Method {
                    name,
                    function: Box::new(function),
                },
            );
        }
        let own = class.declaring_method_ids.values().filter(|id| {
            return id
                .get_class_name()
                .as_bytes()
                .eq_ignore_ascii_case(self.name.as_bytes());
        });
        for function in own.filter_map(|id| return metadata.get_method_by_id(id)) {
            if let Some((name, value)) = relation_property(metadata, function) {
                members.insert(name, Member::Property { name, value });
            }
            if let Some((name, scope)) = scope_method(&binding, metadata, function)
                && !class.declaring_method_ids.contains_key(&name)
            {
                members.insert(
                    name,
                    Member::Method {
                        name,
                        function: Box::new(scope),
                    },
                );
            }
        }
        Forwarding::new(self.forwarded(), binding).add(metadata, self.name, &mut members);
        return members;
    }

    pub(super) fn forwarded(&self) -> Vec<Word> {
        return vec![self.builder.name, word(QUERY_BUILDER)];
    }
}

fn relation_property(
    metadata: &CodebaseMetadata,
    function: &FunctionLikeMetadata,
) -> Option<(Word, TUnion)> {
    let relation = returned_objects(function)
        .find(|returned| return metadata.is_instance_of(returned.name.as_bytes(), RELATION))?;
    let class = metadata.class_likes.get(&key(relation.name.as_bytes()))?;
    let position = class.template_types.get_index_of(&word(RELATED_TEMPLATE))?;
    let related = relation.get_type_parameters()?.get(position).cloned()?;
    let many = TO_MANY
        .iter()
        .any(|ancestor| return metadata.is_instance_of(relation.name.as_bytes(), ancestor));
    let value = if many {
        object_union(object(word(COLLECTION), Some(vec![get_int(), related])))
    } else {
        let mut types = related.types.to_vec();
        types.push(TAtomic::Null);
        TUnion::from_vec(types)
    };
    return Some((
        word(format!("${}", function.original_name).as_bytes()),
        value,
    ));
}

fn scope_name(function: &FunctionLikeMetadata) -> Option<String> {
    let original = function.original_name.to_string();
    if function.attributes.iter().any(|attribute| {
        return attribute
            .name
            .as_bytes()
            .eq_ignore_ascii_case(SCOPE_ATTRIBUTE);
    }) {
        return Some(original);
    }
    let rest = original.strip_prefix(SCOPE_PREFIX)?;
    let mut characters = rest.chars();
    let first = characters.next()?;
    return Some(first.to_lowercase().chain(characters).collect());
}

fn scope_method(
    binding: &Binding,
    metadata: &CodebaseMetadata,
    source: &FunctionLikeMetadata,
) -> Option<(Word, FunctionLikeMetadata)> {
    let public = scope_name(source)?;
    let name = key(public.as_bytes());
    let mut function = FunctionLikeMetadata::new(
        FunctionLikeKind::Method,
        name,
        word(public.as_bytes()),
        source.span,
        MetadataFlags::empty(),
    );
    function.parameters = source.parameters.iter().skip(1).cloned().collect();
    let returns = declared_return(source)
        .filter(|union| return !union.is_void() && !union.is_mixed())
        .map_or_else(
            || return object_union(binding.receiver().clone()),
            |union| return binding.bind(metadata, union),
        );
    function.return_type_metadata = Some(TypeMetadata::new(returns, source.span));
    return Some((name, static_magic(function)));
}
