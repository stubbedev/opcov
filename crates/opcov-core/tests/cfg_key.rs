#[cfg(test)]
mod key {
    use opcov_core::cfg::{
        ClassRef, FilePath, FunctionKey, HookKind, Identifier, Line, LineSpan, QualifiedName,
    };
    use proptest::prelude::*;

    fn pieces(
        choices: Vec<&'static [u8]>,
        count: std::ops::Range<usize>,
    ) -> impl Strategy<Value = Vec<u8>> {
        return prop::collection::vec(prop::sample::select(choices), count)
            .prop_map(|pieces| return pieces.concat());
    }

    fn identifier_bytes() -> impl Strategy<Value = Vec<u8>> {
        let first: Vec<&'static [u8]> =
            vec![b"A", b"z", b"_", "é".as_bytes(), b"\xe9", b"\xff", b"\xc3"];
        let rest: Vec<&'static [u8]> =
            vec![b"A", b"z", b"_", b"9", "é".as_bytes(), b"\xe9", b"\x80"];
        return (pieces(first, 1..2), pieces(rest, 0..8)).prop_map(|(mut first, rest)| {
            first.extend(rest);
            return first;
        });
    }

    fn identifier() -> impl Strategy<Value = String> {
        return identifier_bytes()
            .prop_map(|name| return Identifier::from_bytes(&name).unwrap().to_string());
    }

    fn qualified_name() -> impl Strategy<Value = String> {
        return prop::collection::vec(identifier_bytes(), 1..4).prop_map(|segments| {
            return QualifiedName::from_bytes(&segments.join(&b'\\'))
                .unwrap()
                .to_string();
        });
    }

    fn path() -> impl Strategy<Value = String> {
        let piece: Vec<&'static [u8]> = vec![
            b"a",
            b"0",
            b":",
            b"@",
            b"$",
            b"#",
            b"_",
            b".",
            b"-",
            b" ",
            b"%",
            b"%E9",
            "é".as_bytes(),
            b"\xe9",
            b"\xc3",
            b"/",
        ];
        return pieces(piece, 0..12).prop_map(|name| {
            let raw = [b"/".as_slice(), &name, b".php"].concat();
            return FilePath::from_bytes(&raw).unwrap().to_string();
        });
    }

    fn lines() -> impl Strategy<Value = LineSpan> {
        return (1_u32..100_000, 1_u32..100_000).prop_map(|(a, b)| {
            return LineSpan::new(Line::new(a.min(b)).unwrap(), Line::new(a.max(b)).unwrap())
                .unwrap();
        });
    }

    fn ordinal() -> impl Strategy<Value = u32> {
        return prop_oneof![Just(0_u32), 1_u32..4, Just(u32::MAX)];
    }

    fn class() -> impl Strategy<Value = ClassRef> {
        return prop_oneof![
            qualified_name().prop_map(|name| return ClassRef::Named(name.parse().unwrap())),
            (path(), lines(), ordinal()).prop_map(|(file, lines, ordinal)| {
                return ClassRef::Anonymous {
                    file: file.parse().unwrap(),
                    lines,
                    ordinal,
                };
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
            (path(), lines(), ordinal()).prop_map(|(file, lines, ordinal)| {
                return FunctionKey::Closure {
                    file: file.parse().unwrap(),
                    lines,
                    ordinal,
                };
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
        fn parsing_never_accepts_a_non_canonical_string(
            text in prop_oneof!["\\PC{0,40}", "[%0-9A-Fa-f:@#$\\\\{}a-]{0,30}"]
        ) {
            if let Ok(key) = text.parse::<FunctionKey>() {
                prop_assert_eq!(key.to_string(), text, "accepted strings are canonical");
            }
        }
    }

    proptest! {
        #[test]
        fn a_member_key_equals_the_parsed_member_text(
            class in qualified_name(),
            member in identifier_bytes(),
            hook in prop::bool::ANY,
            kind in prop::sample::select(vec!["get", "set"]),
        ) {
            let member = Identifier::from_bytes(&member).unwrap();
            let raw = if hook {
                [b"$".as_slice(), &member.to_bytes(), b"::", kind.as_bytes()].concat()
            } else {
                member.to_bytes().into_owned()
            };
            let name = if hook { format!("${member}::{kind}") } else { member.to_string() };
            let class_ref: ClassRef = class.parse().unwrap();
            let built = FunctionKey::member(class_ref, &raw).unwrap();
            let parsed: FunctionKey = format!("{class}::{name}").parse().unwrap();
            prop_assert_eq!(built, parsed);
        }
    }
}
