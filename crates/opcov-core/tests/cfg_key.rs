#[cfg(test)]
mod key {
    use opcov_core::cfg::{ClassRef, FunctionKey, HookKind, Line, LineSpan};
    use proptest::prelude::*;

    fn identifier() -> impl Strategy<Value = String> {
        return "[A-Za-z_é][A-Za-z0-9_é]{0,8}";
    }

    fn qualified_name() -> impl Strategy<Value = String> {
        return prop::collection::vec(identifier(), 1..4)
            .prop_map(|segments| return segments.join("\\"));
    }

    fn path() -> impl Strategy<Value = String> {
        return "/[a-z0-9:@$_. -]{0,12}\\.php";
    }

    fn lines() -> impl Strategy<Value = LineSpan> {
        return (1_u32..100_000, 1_u32..100_000).prop_map(|(a, b)| {
            return LineSpan::new(Line::new(a.min(b)).unwrap(), Line::new(a.max(b)).unwrap())
                .unwrap();
        });
    }

    fn class() -> impl Strategy<Value = ClassRef> {
        return prop_oneof![
            qualified_name().prop_map(|name| return ClassRef::Named(name.parse().unwrap())),
            (path(), lines()).prop_map(|(file, lines)| return ClassRef::Anonymous {
                file: file.parse().unwrap(),
                lines
            }),
        ];
    }

    fn key() -> impl Strategy<Value = FunctionKey> {
        return prop_oneof![
            path().prop_map(|file| return FunctionKey::Main {
                file: file.parse().unwrap()
            }),
            qualified_name().prop_map(|name| return FunctionKey::Function {
                name: name.parse().unwrap()
            }),
            (class(), identifier()).prop_map(|(class, method)| return FunctionKey::Method {
                class,
                method: method.parse().unwrap()
            }),
            (
                class(),
                identifier(),
                prop_oneof![Just(HookKind::Get), Just(HookKind::Set)]
            )
                .prop_map(|(class, property, hook)| return FunctionKey::PropertyHook {
                    class,
                    property: property.parse().unwrap(),
                    hook,
                }),
            (path(), lines()).prop_map(|(file, lines)| return FunctionKey::Closure {
                file: file.parse().unwrap(),
                lines
            }),
            (path(), lines()).prop_map(|(file, lines)| return FunctionKey::ArrowFunction {
                file: file.parse().unwrap(),
                lines
            }),
        ];
    }

    proptest! {
        #[test]
        fn canonical_form_round_trips(key in key()) {
            let text = key.to_string();
            prop_assert_eq!(&text.parse::<FunctionKey>().unwrap(), &key, "parse of {}", text);
            let json = serde_json::to_string(&key).unwrap();
            prop_assert_eq!(serde_json::from_str::<FunctionKey>(&json).unwrap(), key, "serde of {}", json);
        }

        #[test]
        fn parsing_never_accepts_a_non_canonical_string(text in "\\PC{0,40}") {
            if let Ok(key) = text.parse::<FunctionKey>() {
                prop_assert_eq!(key.to_string(), text, "accepted strings are canonical");
            }
        }
    }
}
