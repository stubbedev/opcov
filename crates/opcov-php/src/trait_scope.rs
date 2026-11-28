use mago_analyzer::plugin::context::HookContext;
use mago_analyzer::plugin::hook::{ExpressionHook, HookResult};
use mago_analyzer::plugin::provider::{Provider, ProviderMeta};
use mago_codex::metadata::CodebaseMetadata;
use mago_codex::ttype::combine_union_types;
use mago_codex::ttype::combiner::CombinerOptions;
use mago_codex::ttype::expander::{StaticClassType, TypeExpansionOptions, expand_union};
use mago_codex::ttype::union::TUnion;
use mago_syntax::cst::{Access, Call, ClassLikeMemberSelector, Expression, Variable};
use mago_word::{Word, word};

use crate::hierarchy::TraitUsers;

const THIS: &[u8] = b"$this";

static META: ProviderMeta = ProviderMeta::new(
    "opcov-trait-scope",
    "Trait members of the using classes",
    "Types $this->member and self::/static::member in a trait by the classes that use it",
);

#[derive(Debug)]
pub(crate) struct TraitScope {
    users: TraitUsers,
}

impl TraitScope {
    pub(crate) const fn new(users: TraitUsers) -> Self {
        return Self { users };
    }
}

enum Member<'a> {
    Method(&'a [u8]),
    Property(&'a [u8]),
}

fn is_this(expression: &Expression<'_>) -> bool {
    return matches!(expression, Expression::Variable(Variable::Direct(variable)) if variable.name == THIS);
}

const fn named<'a>(selector: &ClassLikeMemberSelector<'a>) -> Option<&'a [u8]> {
    return match selector {
        ClassLikeMemberSelector::Identifier(identifier) => Some(identifier.value),
        ClassLikeMemberSelector::Variable(_)
        | ClassLikeMemberSelector::Expression(_)
        | ClassLikeMemberSelector::Missing(_) => None,
    };
}

fn member<'a>(expression: &Expression<'a>) -> Option<Member<'a>> {
    return match expression {
        Expression::Call(Call::Method(call)) if is_this(call.object) => {
            named(&call.method).map(Member::Method)
        }
        Expression::Call(Call::NullSafeMethod(call)) if is_this(call.object) => {
            named(&call.method).map(Member::Method)
        }
        Expression::Call(Call::StaticMethod(call))
            if matches!(call.class, Expression::Self_(_) | Expression::Static(_)) =>
        {
            named(&call.method).map(Member::Method)
        }
        Expression::Access(Access::Property(access)) if is_this(access.object) => {
            named(&access.property).map(Member::Property)
        }
        Expression::Access(Access::NullSafeProperty(access)) if is_this(access.object) => {
            named(&access.property).map(Member::Property)
        }
        _ => None,
    };
}

fn property_name(property: &[u8]) -> Vec<u8> {
    let mut name = b"$".to_vec();
    name.extend_from_slice(property);
    return name;
}

fn declares(codebase: &CodebaseMetadata, class: &[u8], member: &Member<'_>) -> bool {
    return match member {
        Member::Method(method) => codebase.get_method(class, method).is_some(),
        Member::Property(property) => codebase
            .get_property(class, &property_name(property))
            .is_some(),
    };
}

fn declared_by(codebase: &CodebaseMetadata, class: &[u8], member: &Member<'_>) -> Option<TUnion> {
    let declared = match member {
        Member::Method(method) => {
            let metadata = codebase.get_method(class, method)?;
            metadata
                .return_type_metadata
                .as_ref()
                .or(metadata.return_type_declaration_metadata.as_ref())?
                .type_union
                .clone()
        }
        Member::Property(property) => {
            let metadata = codebase.get_property(class, &property_name(property))?;
            metadata
                .type_metadata
                .as_ref()
                .or(metadata.type_declaration_metadata.as_ref())?
                .type_union
                .clone()
        }
    };
    let user = word(class);
    let mut expanded = declared;
    expand_union(
        codebase,
        &mut expanded,
        &TypeExpansionOptions {
            self_class: Some(user),
            static_class_type: StaticClassType::Name(user),
            ..TypeExpansionOptions::default()
        },
    );
    return Some(expanded);
}

impl TraitScope {
    fn users_type(
        &self,
        codebase: &CodebaseMetadata,
        used: Word,
        member: &Member<'_>,
    ) -> Option<TUnion> {
        let users = self.users.get(&used.to_string().to_ascii_lowercase())?;
        return users
            .iter()
            .filter_map(|user| return declared_by(codebase, user.as_bytes(), member))
            .reduce(|combined, next| {
                return combine_union_types(&combined, &next, codebase, CombinerOptions::default());
            });
    }
}

impl Provider for TraitScope {
    fn meta() -> &'static ProviderMeta {
        return &META;
    }
}

impl ExpressionHook for TraitScope {
    fn after_expression(
        &self,
        expression: &Expression<'_>,
        context: &mut HookContext<'_, '_>,
    ) -> HookResult<()> {
        let Some(member) = member(expression) else {
            return Ok(());
        };
        let Some(used) = context.current_class_name() else {
            return Ok(());
        };
        let codebase = context.codebase();
        let in_trait = codebase
            .get_class_like(used.as_bytes())
            .is_some_and(|metadata| return metadata.kind.is_trait());
        if !in_trait || declares(codebase, used.as_bytes(), &member) {
            return Ok(());
        }
        if context
            .get_expression_type(expression)
            .is_some_and(|recorded| return !recorded.is_mixed())
        {
            return Ok(());
        }
        if let Some(typed) = self.users_type(codebase, used, &member) {
            context.set_expression_type(expression, typed);
        }
        return Ok(());
    }
}
