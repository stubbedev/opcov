use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

use mago_analyzer::plugin::PluginRegistry;
use mago_analyzer::settings::Settings;
use mago_codex::metadata::CodebaseMetadata;
use mago_codex::metadata::ttype::TypeMetadata;
use mago_codex::ttype::atomic::TAtomic;
use mago_codex::ttype::atomic::generic::TGenericParameter;
use mago_codex::ttype::union::TUnion;
use mago_database::file::{File, FileId, HasFileId};
use mago_syntax::walker::Walker;
use mago_word::{Word, word};
use rayon::iter::{IntoParallelIterator, ParallelIterator};

use crate::analysis::{Context, with_analysis};
use crate::consumers::Untyped;

mod aliases;
mod builder_calls;
mod container;
mod hooks;
mod members;
mod model;

pub(crate) use builder_calls::DynamicWheres;
pub(crate) use hooks::{
    Collecting as HookCollecting, Collector as HookCollector, Hooks, ModelEvent,
};
use members::{Binding, Forwarding, Members, install, key, object};
use model::{ELOQUENT_BUILDER, MODEL, MODEL_TEMPLATE, Model};

pub(crate) type Forwards = BTreeMap<String, Vec<String>>;

pub(crate) struct Facts {
    pub(crate) provider: DynamicWheres,
    pub(crate) forwards: Forwards,
    pub(crate) models: usize,
    pub(crate) facades: usize,
    pub(crate) untyped: Vec<Untyped>,
}

pub(crate) fn aliases_stub(root: &Path) -> File {
    return aliases::stub(root);
}

fn descendants(metadata: &CodebaseMetadata, ancestor: &[u8]) -> Vec<Word> {
    let mut found: Vec<Word> = metadata
        .all_class_like_descendants
        .get(&key(ancestor))
        .into_iter()
        .flatten()
        .filter_map(|descendant| {
            return metadata
                .class_likes
                .get(descendant)
                .map(|class| return class.original_name);
        })
        .collect();
    found.sort();
    return found;
}

fn facades(context: &Context<'_>, sources: &[&File]) -> (BTreeMap<Word, Word>, Vec<Untyped>) {
    let wanted: BTreeSet<FileId> = [container::FACADE, container::SERVICE_PROVIDER]
        .into_iter()
        .flat_map(|ancestor| return descendants(context.metadata, ancestor))
        .filter_map(|class| {
            return context
                .metadata
                .class_likes
                .get(&key(class.as_bytes()))
                .map(|metadata| return metadata.span.file_id);
        })
        .collect();
    let mut bindings = container::Bindings::default();
    let mut untyped = Vec::new();
    for file in sources
        .iter()
        .filter(|file| return wanted.contains(&file.file_id()))
    {
        with_analysis(context, file, |program, names, analysed| match analysed {
            Ok(artifacts) => {
                let mut found = container::Found::default();
                container::Collector { names }.walk_program(program, &mut found);
                bindings.add(found, artifacts);
            }
            Err(error) => untyped.push(Untyped {
                path: String::from_utf8_lossy(file.name.as_ref()).into_owned(),
                error: error.to_string(),
            }),
        });
    }
    return (bindings.facades(context.metadata), untyped);
}

fn builder_forwards_to_model(metadata: &mut CodebaseMetadata) {
    let Some(builder) = metadata.class_likes.get_mut(&key(ELOQUENT_BUILDER)) else {
        return;
    };
    let Some(template) = builder.template_types.get(&word(MODEL_TEMPLATE)).cloned() else {
        return;
    };
    let parameter = TGenericParameter {
        parameter_name: word(MODEL_TEMPLATE),
        constraint: std::sync::Arc::new(template.constraint),
        defining_entity: template.defining_entity,
        intersection_types: None,
    };
    let mixin = TypeMetadata::new(
        TUnion::from_atomic(TAtomic::GenericParameter(parameter)),
        builder.span,
    );
    builder.mixins.push(mixin);
}

fn model_forwards(model: &Model) -> (String, Vec<String>) {
    return (
        model.name.to_string().to_ascii_lowercase(),
        model.forwarded().iter().map(ToString::to_string).collect(),
    );
}

const SENSITIVE: [&[u8]; 5] = [
    container::FACADE,
    container::SERVICE_PROVIDER,
    ELOQUENT_BUILDER,
    model::QUERY_BUILDER,
    model::RELATION,
];

pub(crate) fn derives_facts_from_body(metadata: &CodebaseMetadata, class: &str) -> bool {
    return SENSITIVE.iter().any(|ancestor| {
        return class.as_bytes().eq_ignore_ascii_case(ancestor)
            || metadata.is_instance_of(class.as_bytes(), ancestor);
    });
}

pub(crate) fn reinstall_models(metadata: &mut CodebaseMetadata, classes: &[String]) -> Forwards {
    let models: Vec<Model> = classes
        .iter()
        .filter(|class| return metadata.is_instance_of(class.as_bytes(), MODEL))
        .filter_map(|class| return metadata.class_likes.get(&key(class.as_bytes())))
        .map(|class| return class.original_name)
        .collect::<Vec<Word>>()
        .into_iter()
        .map(|name| return Model::of(metadata, name))
        .collect();
    let planned: Vec<(Word, Members)> = models
        .iter()
        .map(|model| return (model.name, model.members(metadata)))
        .collect();
    for (owner, members) in planned {
        install(metadata, owner, members);
    }
    return models.iter().map(model_forwards).collect();
}

pub(crate) fn apply(
    metadata: &mut CodebaseMetadata,
    registry: &PluginRegistry,
    settings: &Settings,
    sources: &[&File],
) -> Facts {
    let (facades, untyped) = facades(
        &Context {
            metadata,
            registry,
            settings,
        },
        sources,
    );
    let shared: &CodebaseMetadata = metadata;
    let (models, mut planned): (Vec<Model>, Vec<(Word, Members)>) = descendants(shared, MODEL)
        .into_par_iter()
        .map(|name| {
            let model = Model::of(shared, name);
            let members = model.members(shared);
            return (model, (name, members));
        })
        .unzip();
    let mut forwards: Forwards = models.iter().map(model_forwards).collect();
    forwards.extend(facades.iter().map(|(facade, target)| {
        return (
            facade.to_string().to_ascii_lowercase(),
            vec![target.to_string()],
        );
    }));
    for (facade, target) in &facades {
        let mut members = Members::new();
        Forwarding::new(
            vec![*target],
            Binding::for_object(metadata, object(*target, None)),
        )
        .add(metadata, *facade, &mut members);
        planned.push((*facade, members));
    }
    for (owner, members) in planned {
        install(metadata, owner, members);
    }
    builder_forwards_to_model(metadata);
    let builders = models
        .iter()
        .map(|model| return (key(model.name.as_bytes()), model.builder.clone()))
        .collect();
    return Facts {
        provider: DynamicWheres { builders },
        forwards,
        models: models.len(),
        facades: facades.len(),
        untyped,
    };
}
