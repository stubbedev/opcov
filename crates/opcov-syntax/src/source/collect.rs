use std::mem;

use mago_span::HasSpan;
use mago_syntax::cst::{
    AnonymousClass, ArrowFunction, Assignment, AssignmentOperator, AttributeList, Binary,
    BinaryOperator, Break, Class, ClassLikeConstant, Closure, Conditional, Constant, Continue,
    DoWhile, Enum, EnumCase, Expression, For, Foreach, ForeachTarget, Function,
    FunctionLikeParameter, FunctionLikeParameterDefaultValue, Goto, Hint, HookedProperty,
    Identifier, If, IfBody, Interface, Literal, Match, MatchArm, Method, Namespace,
    NullSafeMethodCall, NullSafePropertyAccess, Program, PropertyConcreteItem, PropertyHook,
    PropertyItem, Return, Sequence, Statement, Switch, SwitchBody, SwitchCase, Trait, Try, While,
};
use mago_syntax::walker::{
    MutWalker, walk_attribute_list_mut, walk_class_like_constant_mut, walk_constant_mut,
    walk_enum_case_mut, walk_function_like_parameter_default_value_mut,
    walk_property_concrete_item_mut,
};
use opcov_core::kind::LoopKind;

use super::construct::{BodyPos, BranchKind, Clause, Detail, Shape, SwitchSubject};
use super::matching::{ConstructId, Site, SiteClass};
use super::position::ByteSpan;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum RawClass {
    Named(Vec<u8>),
    Anonymous(ByteSpan),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum RawKey {
    Main,
    Function {
        name: Vec<u8>,
    },
    Method {
        class: Option<RawClass>,
        method: Vec<u8>,
        at: u32,
    },
    Hook {
        class: Option<RawClass>,
        property: Option<Vec<u8>>,
        hook: Vec<u8>,
        at: u32,
    },
    Closure {
        start: u32,
        end: u32,
    },
    Arrow {
        start: u32,
        body_end: u32,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct RawConstruct {
    pub(super) span: ByteSpan,
    pub(super) shape: Shape<ByteSpan>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct RawFunction {
    pub(super) key: RawKey,
    pub(super) span: ByteSpan,
    pub(super) constructs: Vec<RawConstruct>,
    pub(super) sites: Vec<Site<ByteSpan, ByteSpan>>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct Collected {
    pub(super) functions: Vec<RawFunction>,
    pub(super) anonymous_classes: Vec<ByteSpan>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Unwind {
    Loop,
    Finally(ConstructId<ByteSpan>),
}

#[derive(Debug)]
struct Scope {
    function: RawFunction,
    unwind: Vec<Unwind>,
    constant: bool,
}

impl Scope {
    const fn new(key: RawKey, span: ByteSpan) -> Self {
        return Self {
            function: RawFunction {
                key,
                span,
                constructs: Vec::new(),
                sites: Vec::new(),
            },
            unwind: Vec::new(),
            constant: false,
        };
    }
}

#[derive(Debug)]
struct Collector {
    namespace: Option<Vec<u8>>,
    classes: Vec<RawClass>,
    properties: Vec<Vec<u8>>,
    current: Scope,
    parents: Vec<Scope>,
    finished: Vec<RawFunction>,
    anonymous_classes: Vec<ByteSpan>,
}

pub(super) fn collect(program: &Program<'_>, length: u32) -> Collected {
    let mut collector = Collector {
        namespace: None,
        classes: Vec::new(),
        properties: Vec::new(),
        current: Scope::new(RawKey::Main, ByteSpan::new(0, length)),
        parents: Vec::new(),
        finished: Vec::new(),
        anonymous_classes: Vec::new(),
    };
    collector.walk_program(program, &mut ());
    let mut functions = collector.finished;
    functions.push(collector.current.function);
    return Collected {
        functions,
        anonymous_classes: collector.anonymous_classes,
    };
}

const fn identifier_bytes<'a>(identifier: &Identifier<'a>) -> &'a [u8] {
    return match identifier {
        Identifier::Local(local) => local.value,
        Identifier::Qualified(qualified) => qualified.value,
        Identifier::FullyQualified(fully) => fully.value,
    };
}

fn variable_name(name: &[u8]) -> Vec<u8> {
    return name.strip_prefix(b"$").unwrap_or(name).to_vec();
}

const fn property_name<'a>(item: &PropertyItem<'a>) -> &'a [u8] {
    return match item {
        PropertyItem::Abstract(item) => item.variable.name,
        PropertyItem::Concrete(item) => item.variable.name,
    };
}

const fn switch_subject(expression: &Expression<'_>) -> SwitchSubject {
    return match expression {
        Expression::Parenthesized(inner) => switch_subject(inner.expression),
        Expression::Literal(Literal::True(_)) => SwitchSubject::True,
        Expression::Literal(Literal::False(_)) => SwitchSubject::False,
        _ => SwitchSubject::Other,
    };
}

fn is_executable(statement: &Statement<'_>) -> bool {
    return match statement {
        Statement::Noop(_)
        | Statement::Label(_)
        | Statement::OpeningTag(_)
        | Statement::ClosingTag(_) => false,
        Statement::Block(block) => block.statements.iter().any(is_executable),
        _ => true,
    };
}

const fn switch_cases<'ast, 'arena>(body: &'ast SwitchBody<'arena>) -> &'ast [SwitchCase<'arena>] {
    return match body {
        SwitchBody::BraceDelimited(body) => body.cases.nodes,
        SwitchBody::ColonDelimited(body) => body.cases.nodes,
    };
}

const fn case_statements<'ast, 'arena>(
    case: &'ast SwitchCase<'arena>,
) -> &'ast Sequence<'arena, Statement<'arena>> {
    return match case {
        SwitchCase::Expression(case) => &case.statements,
        SwitchCase::Default(case) => &case.statements,
    };
}

fn clause_shapes(cases: &[SwitchCase<'_>]) -> Vec<Clause<ByteSpan>> {
    let mut next = BodyPos::End;
    let mut clauses: Vec<Clause<ByteSpan>> = cases
        .iter()
        .enumerate()
        .rev()
        .map(|(index, case)| {
            if case_statements(case).iter().any(is_executable) {
                next = BodyPos::Clause(index);
            }
            return Clause {
                span: ByteSpan::of(case.span()),
                is_default: matches!(case, SwitchCase::Default(_)),
                body: next,
            };
        })
        .collect();
    clauses.reverse();
    return clauses;
}

fn catch_types(hint: &Hint<'_>, types: &mut Vec<ByteSpan>) {
    if let Hint::Union(union) = hint {
        catch_types(union.left, types);
        catch_types(union.right, types);
        return;
    }
    types.push(ByteSpan::of(hint.span()));
}

const fn loop_levels(level: Option<&Expression<'_>>) -> Option<u64> {
    let Some(level) = level else {
        return Some(1);
    };
    return match level {
        Expression::Literal(Literal::Integer(integer)) => integer.value,
        _ => None,
    };
}

const fn is_destructuring(expression: &Expression<'_>) -> bool {
    return matches!(
        expression,
        Expression::Array(_) | Expression::List(_) | Expression::LegacyArray(_)
    );
}

impl Collector {
    fn qualify(&self, name: &[u8]) -> Vec<u8> {
        let Some(namespace) = &self.namespace else {
            return name.to_vec();
        };
        return [namespace.as_slice(), b"\\", name].concat();
    }

    fn enter(&mut self, key: RawKey, span: mago_span::Span) {
        let parent = mem::replace(&mut self.current, Scope::new(key, ByteSpan::of(span)));
        self.parents.push(parent);
    }

    fn leave(&mut self) {
        if let Some(parent) = self.parents.pop() {
            let done = mem::replace(&mut self.current, parent);
            self.finished.push(done.function);
        }
    }

    fn constant(&mut self, walk: impl FnOnce(&mut Self)) {
        let saved = mem::replace(&mut self.current.constant, true);
        walk(self);
        self.current.constant = saved;
    }

    fn record(&mut self, span: ByteSpan, shape: Shape<ByteSpan>) -> Option<ConstructId<ByteSpan>> {
        if self.current.constant {
            return None;
        }
        let id = ConstructId {
            span,
            kind: shape.kind(),
        };
        self.current
            .function
            .constructs
            .push(RawConstruct { span, shape });
        return Some(id);
    }

    fn site(
        &mut self,
        construct: Option<ConstructId<ByteSpan>>,
        class: SiteClass,
        claim: Vec<ByteSpan>,
        detail: Detail,
    ) {
        if let Some(construct) = construct {
            self.current.function.sites.push(Site {
                class,
                claim,
                construct,
                detail,
            });
        }
    }

    fn branch(
        &mut self,
        span: mago_span::Span,
        kind: BranchKind,
        class: SiteClass,
        claim: mago_span::Span,
    ) {
        let id = self.record(ByteSpan::of(span), Shape::Branch(kind));
        self.site(id, class, vec![ByteSpan::of(claim)], Detail::Whole);
    }

    fn condition(&mut self, condition: &Expression<'_>, kind: BranchKind) {
        self.walk_expression(condition, &mut ());
        self.branch(
            condition.span(),
            kind,
            SiteClass::Condition,
            condition.span(),
        );
    }

    fn statements(&mut self, statements: &Sequence<'_, Statement<'_>>) {
        for statement in statements {
            self.walk_statement(statement, &mut ());
        }
    }

    fn looped(&mut self, walk: impl FnOnce(&mut Self)) {
        self.current.unwind.push(Unwind::Loop);
        walk(self);
        self.current.unwind.pop();
    }

    fn unwind_finally(&mut self, claim: ByteSpan, levels: Option<u64>) {
        let mut loops_left = levels;
        let mut finals = Vec::new();
        for frame in self.current.unwind.iter().rev() {
            match frame {
                Unwind::Finally(id) => finals.push(*id),
                Unwind::Loop => match loops_left {
                    Some(1) => break,
                    Some(count) => loops_left = Some(count.saturating_sub(1)),
                    None => {}
                },
            }
        }
        for id in finals {
            self.site(Some(id), SiteClass::Finally, vec![claim], Detail::Whole);
        }
    }

    fn located_loop(&mut self, span: mago_span::Span, kind: LoopKind, claim: ByteSpan) {
        let id = self.record(ByteSpan::of(span), Shape::Loop(kind));
        self.site(id, SiteClass::LoopTest, vec![claim], Detail::Whole);
    }

    fn enter_class(&mut self, name: &[u8]) {
        let class = RawClass::Named(self.qualify(name));
        self.classes.push(class);
    }

    fn leave_class(&mut self) {
        self.classes.pop();
    }
}

impl<'ast, 'arena> MutWalker<'ast, 'arena, ()> for Collector {
    fn walk_in_namespace(&mut self, namespace: &'ast Namespace<'arena>, (): &mut ()) {
        self.namespace = namespace
            .name
            .as_ref()
            .map(|name| return identifier_bytes(name).to_vec());
    }

    fn walk_out_namespace(&mut self, _: &'ast Namespace<'arena>, (): &mut ()) {
        self.namespace = None;
    }

    fn walk_in_class(&mut self, class: &'ast Class<'arena>, (): &mut ()) {
        self.enter_class(class.name.value);
    }

    fn walk_out_class(&mut self, _: &'ast Class<'arena>, (): &mut ()) {
        self.leave_class();
    }

    fn walk_in_interface(&mut self, interface: &'ast Interface<'arena>, (): &mut ()) {
        self.enter_class(interface.name.value);
    }

    fn walk_out_interface(&mut self, _: &'ast Interface<'arena>, (): &mut ()) {
        self.leave_class();
    }

    fn walk_in_trait(&mut self, r#trait: &'ast Trait<'arena>, (): &mut ()) {
        self.enter_class(r#trait.name.value);
    }

    fn walk_out_trait(&mut self, _: &'ast Trait<'arena>, (): &mut ()) {
        self.leave_class();
    }

    fn walk_in_enum(&mut self, r#enum: &'ast Enum<'arena>, (): &mut ()) {
        self.enter_class(r#enum.name.value);
    }

    fn walk_out_enum(&mut self, _: &'ast Enum<'arena>, (): &mut ()) {
        self.leave_class();
    }

    fn walk_anonymous_class(&mut self, class: &'ast AnonymousClass<'arena>, context: &mut ()) {
        if let Some(arguments) = &class.argument_list {
            self.walk_partial_argument_list(arguments, context);
        }
        let span = ByteSpan::new(
            class.class.span.start.offset,
            class.right_brace.start.offset,
        );
        self.anonymous_classes.push(span);
        self.classes.push(RawClass::Anonymous(span));
        for member in &class.members {
            self.walk_class_like_member(member, context);
        }
        self.leave_class();
    }

    fn walk_in_function(&mut self, function: &'ast Function<'arena>, (): &mut ()) {
        let name = self.qualify(function.name.value);
        self.enter(RawKey::Function { name }, function.span());
    }

    fn walk_out_function(&mut self, _: &'ast Function<'arena>, (): &mut ()) {
        self.leave();
    }

    fn walk_in_method(&mut self, method: &'ast Method<'arena>, (): &mut ()) {
        let key = RawKey::Method {
            class: self.classes.last().cloned(),
            method: method.name.value.to_vec(),
            at: method.name.span.start.offset,
        };
        self.enter(key, method.span());
    }

    fn walk_out_method(&mut self, _: &'ast Method<'arena>, (): &mut ()) {
        self.leave();
    }

    fn walk_in_closure(&mut self, closure: &'ast Closure<'arena>, (): &mut ()) {
        let key = RawKey::Closure {
            start: closure.function.span.start.offset,
            end: closure.body.right_brace.start.offset,
        };
        self.enter(key, closure.span());
    }

    fn walk_out_closure(&mut self, _: &'ast Closure<'arena>, (): &mut ()) {
        self.leave();
    }

    fn walk_in_arrow_function(&mut self, arrow: &'ast ArrowFunction<'arena>, (): &mut ()) {
        let key = RawKey::Arrow {
            start: arrow.r#fn.span.start.offset,
            body_end: arrow.expression.span().end.offset,
        };
        self.enter(key, arrow.span());
    }

    fn walk_out_arrow_function(&mut self, _: &'ast ArrowFunction<'arena>, (): &mut ()) {
        self.leave();
    }

    fn walk_in_hooked_property(&mut self, property: &'ast HookedProperty<'arena>, (): &mut ()) {
        self.properties
            .push(variable_name(property_name(&property.item)));
    }

    fn walk_out_hooked_property(&mut self, _: &'ast HookedProperty<'arena>, (): &mut ()) {
        self.properties.pop();
    }

    fn walk_in_function_like_parameter(
        &mut self,
        parameter: &'ast FunctionLikeParameter<'arena>,
        (): &mut (),
    ) {
        if parameter.hooks.is_some() {
            self.properties.push(variable_name(parameter.variable.name));
        }
    }

    fn walk_out_function_like_parameter(
        &mut self,
        parameter: &'ast FunctionLikeParameter<'arena>,
        (): &mut (),
    ) {
        if parameter.hooks.is_some() {
            self.properties.pop();
        }
    }

    fn walk_in_property_hook(&mut self, hook: &'ast PropertyHook<'arena>, (): &mut ()) {
        let key = RawKey::Hook {
            class: self.classes.last().cloned(),
            property: self.properties.last().cloned(),
            hook: hook.name.value.to_vec(),
            at: hook.name.span.start.offset,
        };
        self.enter(key, hook.span());
    }

    fn walk_out_property_hook(&mut self, _: &'ast PropertyHook<'arena>, (): &mut ()) {
        self.leave();
    }

    fn walk_attribute_list(&mut self, list: &'ast AttributeList<'arena>, context: &mut ()) {
        self.constant(|walker| return walk_attribute_list_mut(walker, list, context));
    }

    fn walk_function_like_parameter_default_value(
        &mut self,
        value: &'ast FunctionLikeParameterDefaultValue<'arena>,
        context: &mut (),
    ) {
        self.constant(|walker| {
            return walk_function_like_parameter_default_value_mut(walker, value, context);
        });
    }

    fn walk_class_like_constant(
        &mut self,
        constant: &'ast ClassLikeConstant<'arena>,
        context: &mut (),
    ) {
        self.constant(|walker| return walk_class_like_constant_mut(walker, constant, context));
    }

    fn walk_enum_case(&mut self, case: &'ast EnumCase<'arena>, context: &mut ()) {
        self.constant(|walker| return walk_enum_case_mut(walker, case, context));
    }

    fn walk_property_concrete_item(
        &mut self,
        item: &'ast PropertyConcreteItem<'arena>,
        context: &mut (),
    ) {
        self.constant(|walker| return walk_property_concrete_item_mut(walker, item, context));
    }

    fn walk_constant(&mut self, constant: &'ast Constant<'arena>, context: &mut ()) {
        self.constant(|walker| return walk_constant_mut(walker, constant, context));
    }

    fn walk_if(&mut self, r#if: &'ast If<'arena>, context: &mut ()) {
        self.condition(r#if.condition, BranchKind::If);
        match &r#if.body {
            IfBody::Statement(body) => {
                self.walk_statement(body.statement, context);
                for clause in &body.else_if_clauses {
                    self.condition(clause.condition, BranchKind::ElseIf);
                    self.walk_statement(clause.statement, context);
                }
                if let Some(clause) = &body.else_clause {
                    self.walk_statement(clause.statement, context);
                }
            }
            IfBody::ColonDelimited(body) => {
                self.statements(&body.statements);
                for clause in &body.else_if_clauses {
                    self.condition(clause.condition, BranchKind::ElseIf);
                    self.statements(&clause.statements);
                }
                if let Some(clause) = &body.else_clause {
                    self.statements(&clause.statements);
                }
            }
        }
    }

    fn walk_conditional(&mut self, conditional: &'ast Conditional<'arena>, context: &mut ()) {
        self.walk_expression(conditional.condition, context);
        let claim = conditional.condition.span();
        if let Some(then) = conditional.then {
            self.branch(
                conditional.span(),
                BranchKind::Ternary,
                SiteClass::Condition,
                claim,
            );
            self.walk_expression(then, context);
        } else {
            self.branch(
                conditional.span(),
                BranchKind::ShortTernary,
                SiteClass::ShortTernary,
                claim,
            );
        }
        self.walk_expression(conditional.r#else, context);
    }

    fn walk_binary(&mut self, binary: &'ast Binary<'arena>, context: &mut ()) {
        let branch = match binary.operator {
            BinaryOperator::NullCoalesce(_) => Some((BranchKind::Coalesce, SiteClass::Coalesce)),
            BinaryOperator::And(_) => Some((BranchKind::And, SiteClass::ShortCircuit)),
            BinaryOperator::Or(_) => Some((BranchKind::Or, SiteClass::ShortCircuit)),
            BinaryOperator::LowAnd(_) => Some((BranchKind::LogicalAnd, SiteClass::ShortCircuit)),
            BinaryOperator::LowOr(_) => Some((BranchKind::LogicalOr, SiteClass::ShortCircuit)),
            _ => None,
        };
        self.walk_expression(binary.lhs, context);
        if let Some((kind, class)) = branch {
            self.branch(binary.span(), kind, class, binary.lhs.span());
        }
        self.walk_expression(binary.rhs, context);
    }

    fn walk_assignment(&mut self, assignment: &'ast Assignment<'arena>, context: &mut ()) {
        if matches!(assignment.operator, AssignmentOperator::Coalesce(_)) {
            self.walk_expression(assignment.lhs, context);
            self.branch(
                assignment.span(),
                BranchKind::CoalesceAssign,
                SiteClass::Coalesce,
                assignment.lhs.span(),
            );
            self.walk_expression(assignment.rhs, context);
            return;
        }
        if is_destructuring(assignment.lhs) {
            self.walk_expression(assignment.rhs, context);
            self.walk_expression(assignment.lhs, context);
            return;
        }
        self.walk_expression(assignment.lhs, context);
        self.walk_expression(assignment.rhs, context);
    }

    fn walk_null_safe_property_access(
        &mut self,
        access: &'ast NullSafePropertyAccess<'arena>,
        context: &mut (),
    ) {
        self.walk_expression(access.object, context);
        self.branch(
            access.span(),
            BranchKind::NullsafeProperty,
            SiteClass::Nullsafe,
            access.object.span(),
        );
        self.walk_class_like_member_selector(&access.property, context);
    }

    fn walk_null_safe_method_call(
        &mut self,
        call: &'ast NullSafeMethodCall<'arena>,
        context: &mut (),
    ) {
        self.walk_expression(call.object, context);
        self.branch(
            call.span(),
            BranchKind::NullsafeMethodCall,
            SiteClass::Nullsafe,
            call.object.span(),
        );
        self.walk_class_like_member_selector(&call.method, context);
        self.walk_argument_list(&call.argument_list, context);
    }

    fn walk_match(&mut self, r#match: &'ast Match<'arena>, context: &mut ()) {
        self.walk_expression(r#match.expression, context);
        let mut claim = vec![ByteSpan::of(r#match.expression.span())];
        for arm in &r#match.arms {
            if let MatchArm::Expression(arm) = arm {
                for condition in &arm.conditions {
                    self.walk_expression(condition, context);
                    claim.push(ByteSpan::of(condition.span()));
                }
            }
        }
        for arm in &r#match.arms {
            let body = match arm {
                MatchArm::Expression(arm) => arm.expression,
                MatchArm::Default(arm) => arm.expression,
            };
            self.walk_expression(body, context);
        }
        let shape = Shape::Match {
            arms: r#match
                .arms
                .iter()
                .map(|arm| return ByteSpan::of(arm.span()))
                .collect(),
            default: r#match
                .arms
                .iter()
                .position(|arm| return matches!(arm, MatchArm::Default(_))),
        };
        let id = self.record(ByteSpan::of(r#match.span()), shape);
        self.site(id, SiteClass::Arms, claim, Detail::Whole);
    }

    fn walk_switch(&mut self, switch: &'ast Switch<'arena>, context: &mut ()) {
        self.walk_expression(switch.expression, context);
        let cases = switch_cases(&switch.body);
        let subject = switch_subject(switch.expression);
        let shape = Shape::Switch {
            subject,
            clauses: clause_shapes(cases),
        };
        let id = self.record(ByteSpan::of(switch.span()), shape);
        let mut claim = vec![ByteSpan::of(switch.expression.span())];
        for (index, case) in cases.iter().enumerate() {
            if let SwitchCase::Expression(case) = case {
                self.walk_expression(case.expression, context);
                let condition = ByteSpan::of(case.expression.span());
                claim.push(condition);
                if subject == SwitchSubject::False {
                    self.site(
                        id,
                        SiteClass::Condition,
                        vec![condition],
                        Detail::Case(index),
                    );
                }
            }
        }
        self.looped(|walker| {
            for case in cases {
                walker.statements(case_statements(case));
            }
        });
        self.site(id, SiteClass::Arms, claim, Detail::Whole);
    }

    fn walk_foreach(&mut self, foreach: &'ast Foreach<'arena>, context: &mut ()) {
        self.walk_expression(foreach.expression, context);
        let claim = ByteSpan::of(foreach.expression.span());
        let id = self.record(ByteSpan::of(foreach.span()), Shape::Loop(LoopKind::Foreach));
        self.site(id, SiteClass::LoopEntry, vec![claim], Detail::Whole);
        self.site(id, SiteClass::LoopTest, vec![claim], Detail::Whole);
        self.looped(|walker| {
            match &foreach.target {
                ForeachTarget::Value(target) => walker.walk_expression(target.value, &mut ()),
                ForeachTarget::KeyValue(target) => {
                    walker.walk_expression(target.value, &mut ());
                    walker.walk_expression(target.key, &mut ());
                }
            }
            walker.walk_foreach_body(&foreach.body, &mut ());
        });
    }

    fn walk_for(&mut self, r#for: &'ast For<'arena>, context: &mut ()) {
        for initialization in &r#for.initializations {
            self.walk_expression(initialization, context);
        }
        self.looped(|walker| return walker.walk_for_body(&r#for.body, &mut ()));
        for increment in &r#for.increments {
            self.walk_expression(increment, context);
        }
        for condition in &r#for.conditions {
            self.walk_expression(condition, context);
        }
        let claim = match (r#for.conditions.first(), r#for.conditions.last()) {
            (Some(first), Some(last)) => ByteSpan::between(first.span(), last.span()),
            _ => ByteSpan::of(r#for.span()),
        };
        self.located_loop(r#for.span(), LoopKind::For, claim);
    }

    fn walk_while(&mut self, r#while: &'ast While<'arena>, context: &mut ()) {
        self.looped(|walker| return walker.walk_while_body(&r#while.body, &mut ()));
        self.walk_expression(r#while.condition, context);
        self.located_loop(
            r#while.span(),
            LoopKind::While,
            ByteSpan::of(r#while.condition.span()),
        );
    }

    fn walk_do_while(&mut self, do_while: &'ast DoWhile<'arena>, context: &mut ()) {
        self.looped(|walker| return walker.walk_statement(do_while.statement, &mut ()));
        self.walk_expression(do_while.condition, context);
        self.located_loop(
            do_while.span(),
            LoopKind::DoWhile,
            ByteSpan::of(do_while.condition.span()),
        );
    }

    fn walk_try(&mut self, r#try: &'ast Try<'arena>, context: &mut ()) {
        let finally = r#try.finally_clause.as_ref().and_then(|clause| {
            let shape = Shape::Finally {
                clause: ByteSpan::of(clause.span()),
            };
            return self
                .record(ByteSpan::of(r#try.span()), shape)
                .map(|id| return (id, clause));
        });
        if let Some((id, _)) = finally {
            self.current.unwind.push(Unwind::Finally(id));
        }
        self.walk_block(&r#try.block, context);
        for clause in &r#try.catch_clauses {
            let mut types = Vec::new();
            catch_types(&clause.hint, &mut types);
            let first = types.first().copied();
            let count = types.len();
            let id = self.record(ByteSpan::of(clause.span()), Shape::Catch { types });
            if let Some(first) = first {
                for index in 0..count {
                    self.site(id, SiteClass::Catch, vec![first], Detail::CatchType(index));
                }
            }
            self.walk_block(&clause.block, context);
        }
        if let Some((id, clause)) = finally {
            self.current.unwind.pop();
            let claim = ByteSpan::between(clause.finally.span, clause.block.left_brace);
            self.site(Some(id), SiteClass::Finally, vec![claim], Detail::Whole);
            self.site(
                Some(id),
                SiteClass::FinallyEntry,
                vec![ByteSpan::of(clause.block.span())],
                Detail::Whole,
            );
            self.walk_block(&clause.block, context);
        }
    }

    fn walk_return(&mut self, r#return: &'ast Return<'arena>, context: &mut ()) {
        if let Some(value) = r#return.value {
            self.walk_expression(value, context);
        }
        self.unwind_finally(ByteSpan::of(r#return.span()), None);
    }

    fn walk_break(&mut self, r#break: &'ast Break<'arena>, (): &mut ()) {
        self.unwind_finally(ByteSpan::of(r#break.span()), loop_levels(r#break.level));
    }

    fn walk_continue(&mut self, r#continue: &'ast Continue<'arena>, (): &mut ()) {
        self.unwind_finally(
            ByteSpan::of(r#continue.span()),
            loop_levels(r#continue.level),
        );
    }

    fn walk_goto(&mut self, goto: &'ast Goto<'arena>, (): &mut ()) {
        self.unwind_finally(ByteSpan::of(goto.span()), None);
    }
}
