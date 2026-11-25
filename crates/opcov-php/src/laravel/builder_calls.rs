use std::collections::BTreeMap;

use mago_analyzer::plugin::context::{InvocationInfo, ProviderContext};
use mago_analyzer::plugin::provider::{
    MethodReturnTypeProvider, MethodTarget, Provider, ProviderMeta,
};
use mago_codex::ttype::atomic::object::named::TNamedObject;
use mago_codex::ttype::union::TUnion;
use mago_word::{Word, ascii_lowercase_word};

use super::members::object_union;
use super::model::ELOQUENT_BUILDER;

const DYNAMIC_WHERE: &[u8] = b"where";

static META: ProviderMeta = ProviderMeta::new(
    "opcov-laravel-builder-calls",
    "Eloquent dynamic wheres",
    "Builder::__call turns where{Column}() into a where and returns the builder",
);
static TARGETS: [MethodTarget; 1] = [MethodTarget::any_class(b"where*")];

#[derive(Debug)]
pub(crate) struct DynamicWheres {
    pub(super) builders: BTreeMap<Word, TNamedObject>,
}

impl Provider for DynamicWheres {
    fn meta() -> &'static ProviderMeta {
        return &META;
    }
}

impl MethodReturnTypeProvider for DynamicWheres {
    fn targets() -> &'static [MethodTarget] {
        return &TARGETS;
    }

    fn get_return_type(
        &self,
        context: &ProviderContext<'_, '_, '_>,
        class_name: &[u8],
        method_name: &[u8],
        _: &InvocationInfo<'_, '_, '_>,
    ) -> Option<TUnion> {
        let codebase = context.codebase();
        let class = codebase.get_class_like(class_name)?;
        let method = ascii_lowercase_word(method_name);
        if method.as_bytes().len() <= DYNAMIC_WHERE.len()
            || class.declaring_method_ids.contains_key(&method)
        {
            return None;
        }
        if codebase.is_instance_of(class_name, ELOQUENT_BUILDER) {
            return Some(object_union(TNamedObject::new_this(class.original_name)));
        }
        return self
            .builders
            .get(&class.name)
            .map(|builder| return object_union(builder.clone()));
    }
}
