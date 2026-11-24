#[cfg(test)]
mod encoding {
    use opcov_core::encoding::{decode, encode};
    use proptest::prelude::*;

    fn bytes() -> impl Strategy<Value = Vec<u8>> {
        let piece = prop_oneof![
            any::<u8>().prop_map(|byte| return vec![byte]),
            prop::sample::select(vec!["%", "%25", "%E9", "é", "€", "/", "\\", ":", "#", "a"])
                .prop_map(|text| return text.as_bytes().to_vec()),
            prop::sample::select(vec![
                vec![0xC3],
                vec![0xE2, 0x82],
                vec![0xED, 0xA0, 0x80],
                vec![0xF4, 0x90, 0x80, 0x80],
                vec![0xC0, 0xAF],
            ]),
        ];
        return prop::collection::vec(piece, 0..12).prop_map(|pieces| return pieces.concat());
    }

    proptest! {
        #[test]
        fn decoding_inverts_encoding(raw in bytes()) {
            let text = encode(&raw);
            prop_assert_eq!(&*decode(&text).unwrap(), raw.as_slice(), "decode of {:?}", text);
        }

        #[test]
        fn decoding_accepts_exactly_the_encodings(text in "[%0-9A-Fa-f\u{e9}/x]{0,24}") {
            match decode(&text) {
                Ok(raw) => prop_assert_eq!(encode(&raw), text.as_str(), "accepted text is canonical"),
                Err(error) => prop_assert_eq!(error.text(), text.as_str(), "the error names the input"),
            }
        }

        #[test]
        fn distinct_bytes_have_distinct_encodings(left in bytes(), right in bytes()) {
            prop_assert_eq!(encode(&left) == encode(&right), left == right, "injective");
        }

        #[test]
        fn ascii_separators_survive_encoding_in_place(
            left in bytes(),
            right in bytes(),
            separator in prop::sample::select(vec![b'/', b'\\', b':', b'#', b'@', b'$', b'-']),
        ) {
            let joined = [left.as_slice(), &[separator], right.as_slice()].concat();
            let expected = format!("{}{}{}", encode(&left), char::from(separator), encode(&right));
            prop_assert_eq!(encode(&joined), expected.as_str(), "encoding commutes with joining");
        }

        #[test]
        fn encodings_escape_only_with_uppercase_hex_pairs(raw in bytes()) {
            let text = encode(&raw);
            let mut rest = text.as_bytes();
            while let Some(position) = rest.iter().position(|&byte| return byte == b'%') {
                let escape = &rest[position..];
                prop_assert!(
                    escape.len() >= 3
                        && escape[1..3].iter().all(|&digit| return digit.is_ascii_digit() || (b'A'..=b'F').contains(&digit)),
                    "escape in {:?}", text
                );
                rest = &escape[3..];
            }
        }
    }
}
