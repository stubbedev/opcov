use std::cell::RefCell;
use std::cmp::Reverse;
use std::collections::HashMap;
use std::fmt;
use std::ops::Range;

use bincode::config::standard;
use bincode::enc::write::Writer;
use bincode::error::EncodeError;
use foldhash::fast::FixedState;
use mago_word::Word;
use serde::ser::{
    self, Impossible, Serialize, SerializeMap, SerializeSeq, SerializeStruct,
    SerializeStructVariant, SerializeTuple, SerializeTupleStruct, SerializeTupleVariant,
    Serializer,
};

fn unordered_fields(name: &str) -> &'static [&'static str] {
    return match name {
        "CodebaseMetadata" => &[
            "function_likes",
            "safe_symbol_members",
            "file_signatures",
            "patch_function_likes",
        ],
        "TraitMethodAdaptations" => &["excluded_methods", "modifiers"],
        _ => &[],
    };
}

#[derive(Debug)]
pub(crate) struct Error(String);

impl fmt::Display for Error {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        return formatter.write_str(&self.0);
    }
}

impl std::error::Error for Error {}

impl ser::Error for Error {
    fn custom<T: fmt::Display>(message: T) -> Self {
        return Self(message.to_string());
    }
}

impl From<EncodeError> for Error {
    fn from(error: EncodeError) -> Self {
        return Self(error.to_string());
    }
}

type Counts = HashMap<Box<[u8]>, u32, FixedState>;
type Indices<'a> = HashMap<&'a [u8], u32, FixedState>;

#[derive(Clone, Copy)]
enum Mode<'a> {
    Count(&'a RefCell<Counts>),
    Write {
        indices: &'a Indices<'a>,
        out: &'a RefCell<Vec<u8>>,
    },
}

struct Discard;

impl Writer for Discard {
    fn write(&mut self, _bytes: &[u8]) -> Result<(), EncodeError> {
        return Ok(());
    }
}

struct Shared<'a>(&'a RefCell<Vec<u8>>);

impl Writer for Shared<'_> {
    fn write(&mut self, bytes: &[u8]) -> Result<(), EncodeError> {
        self.0.borrow_mut().extend_from_slice(bytes);
        return Ok(());
    }
}

struct Wrapped<'a, T: ?Sized>(&'a T, Mode<'a>);

impl<T: ?Sized + Serialize> Serialize for Wrapped<'_, T> {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        return self.0.serialize(Forward {
            inner: serializer,
            mode: self.1,
        });
    }
}

fn encode_into<T: ?Sized + Serialize>(
    value: &T,
    indices: &Indices<'_>,
    out: &RefCell<Vec<u8>>,
) -> Result<Range<usize>, Error> {
    let start = out.borrow().len();
    let mode = Mode::Write { indices, out };
    bincode::serde::encode_into_writer(Wrapped(value, mode), Shared(out), standard())?;
    return Ok(start..out.borrow().len());
}

enum Parts {
    Entries(Vec<(Range<usize>, Range<usize>)>),
    Elements(Vec<Range<usize>>),
}

struct Collected {
    arena: Vec<u8>,
    parts: Parts,
}

impl Collected {
    fn splice(self, out: &RefCell<Vec<u8>>) -> Result<(), Error> {
        let arena = self.arena.as_slice();
        let bytes = |range: &Range<usize>| {
            return arena
                .get(range.clone())
                .ok_or_else(|| return Error("a fragment lies outside its arena".to_owned()));
        };
        let mut ordered: Vec<(&[u8], Option<&[u8]>)> = match &self.parts {
            Parts::Entries(entries) => entries
                .iter()
                .map(|(key, value)| return Ok((bytes(key)?, Some(bytes(value)?))))
                .collect::<Result<_, Error>>()?,
            Parts::Elements(elements) => elements
                .iter()
                .map(|element| return Ok((bytes(element)?, None)))
                .collect::<Result<_, Error>>()?,
        };
        ordered.sort_unstable();
        let mut out = out.borrow_mut();
        out.reserve(arena.len());
        out.extend(bincode::encode_to_vec(ordered.len(), standard())?);
        for (first, second) in ordered {
            out.extend_from_slice(first);
            out.extend_from_slice(second.unwrap_or_default());
        }
        return Ok(());
    }
}

struct Collect<'a> {
    indices: &'a Indices<'a>,
}

struct Collecting<'a> {
    indices: &'a Indices<'a>,
    arena: RefCell<Vec<u8>>,
    key: Option<Range<usize>>,
    parts: Parts,
}

impl<'a> Collecting<'a> {
    const fn new(indices: &'a Indices<'a>, parts: Parts) -> Self {
        return Self {
            indices,
            arena: RefCell::new(Vec::new()),
            key: None,
            parts,
        };
    }

    fn finish(self) -> Collected {
        return Collected {
            arena: self.arena.into_inner(),
            parts: self.parts,
        };
    }
}

fn unordered() -> Error {
    return Error("an unordered field is neither a map nor a set".to_owned());
}

impl SerializeSeq for Collecting<'_> {
    type Ok = Collected;
    type Error = Error;

    fn serialize_element<T: ?Sized + Serialize>(&mut self, value: &T) -> Result<(), Error> {
        let range = encode_into(value, self.indices, &self.arena)?;
        let Parts::Elements(elements) = &mut self.parts else {
            return Err(unordered());
        };
        elements.push(range);
        return Ok(());
    }

    fn end(self) -> Result<Collected, Error> {
        return Ok(self.finish());
    }
}

impl SerializeMap for Collecting<'_> {
    type Ok = Collected;
    type Error = Error;

    fn serialize_key<T: ?Sized + Serialize>(&mut self, key: &T) -> Result<(), Error> {
        self.key = Some(encode_into(key, self.indices, &self.arena)?);
        return Ok(());
    }

    fn serialize_value<T: ?Sized + Serialize>(&mut self, value: &T) -> Result<(), Error> {
        let key = self
            .key
            .take()
            .ok_or_else(|| return Error("a map value came before its key".to_owned()))?;
        let range = encode_into(value, self.indices, &self.arena)?;
        let Parts::Entries(entries) = &mut self.parts else {
            return Err(unordered());
        };
        entries.push((key, range));
        return Ok(());
    }

    fn end(self) -> Result<Collected, Error> {
        return Ok(self.finish());
    }
}

macro_rules! unordered_scalars {
    ($($method:ident($type:ty)),* $(,)?) => {
        $(
            fn $method(self, _value: $type) -> Result<Collected, Error> {
                return Err(unordered());
            }
        )*
    };
}

impl<'a> Serializer for Collect<'a> {
    type Ok = Collected;
    type Error = Error;
    type SerializeSeq = Collecting<'a>;
    type SerializeTuple = Impossible<Collected, Error>;
    type SerializeTupleStruct = Impossible<Collected, Error>;
    type SerializeTupleVariant = Impossible<Collected, Error>;
    type SerializeMap = Collecting<'a>;
    type SerializeStruct = Impossible<Collected, Error>;
    type SerializeStructVariant = Impossible<Collected, Error>;

    unordered_scalars!(
        serialize_bool(bool),
        serialize_i8(i8),
        serialize_i16(i16),
        serialize_i32(i32),
        serialize_i64(i64),
        serialize_i128(i128),
        serialize_u8(u8),
        serialize_u16(u16),
        serialize_u32(u32),
        serialize_u64(u64),
        serialize_u128(u128),
        serialize_f32(f32),
        serialize_f64(f64),
        serialize_char(char),
        serialize_str(&str),
        serialize_bytes(&[u8]),
        serialize_unit_struct(&'static str),
    );

    fn is_human_readable(&self) -> bool {
        return false;
    }

    fn serialize_none(self) -> Result<Collected, Error> {
        return Err(unordered());
    }

    fn serialize_some<T: ?Sized + Serialize>(self, _value: &T) -> Result<Collected, Error> {
        return Err(unordered());
    }

    fn serialize_unit(self) -> Result<Collected, Error> {
        return Err(unordered());
    }

    fn serialize_unit_variant(
        self,
        _name: &'static str,
        _index: u32,
        _variant: &'static str,
    ) -> Result<Collected, Error> {
        return Err(unordered());
    }

    fn serialize_newtype_struct<T: ?Sized + Serialize>(
        self,
        _name: &'static str,
        _value: &T,
    ) -> Result<Collected, Error> {
        return Err(unordered());
    }

    fn serialize_newtype_variant<T: ?Sized + Serialize>(
        self,
        _name: &'static str,
        _index: u32,
        _variant: &'static str,
        _value: &T,
    ) -> Result<Collected, Error> {
        return Err(unordered());
    }

    fn serialize_seq(self, length: Option<usize>) -> Result<Collecting<'a>, Error> {
        return Ok(Collecting::new(
            self.indices,
            Parts::Elements(Vec::with_capacity(length.unwrap_or_default())),
        ));
    }

    fn serialize_tuple(self, _length: usize) -> Result<Self::SerializeTuple, Error> {
        return Err(unordered());
    }

    fn serialize_tuple_struct(
        self,
        _name: &'static str,
        _length: usize,
    ) -> Result<Self::SerializeTupleStruct, Error> {
        return Err(unordered());
    }

    fn serialize_tuple_variant(
        self,
        _name: &'static str,
        _index: u32,
        _variant: &'static str,
        _length: usize,
    ) -> Result<Self::SerializeTupleVariant, Error> {
        return Err(unordered());
    }

    fn serialize_map(self, length: Option<usize>) -> Result<Collecting<'a>, Error> {
        return Ok(Collecting::new(
            self.indices,
            Parts::Entries(Vec::with_capacity(length.unwrap_or_default())),
        ));
    }

    fn serialize_struct(
        self,
        _name: &'static str,
        _length: usize,
    ) -> Result<Self::SerializeStruct, Error> {
        return Err(unordered());
    }

    fn serialize_struct_variant(
        self,
        _name: &'static str,
        _index: u32,
        _variant: &'static str,
        _length: usize,
    ) -> Result<Self::SerializeStructVariant, Error> {
        return Err(unordered());
    }
}

struct Forward<'a, S> {
    inner: S,
    mode: Mode<'a>,
}

struct Compound<'a, C> {
    inner: C,
    mode: Mode<'a>,
    name: &'static str,
    unordered: &'static [&'static str],
}

impl<'a, C> Compound<'a, C> {
    fn new(inner: C, mode: Mode<'a>, name: &'static str) -> Self {
        return Self {
            inner,
            mode,
            name,
            unordered: unordered_fields(name),
        };
    }

    fn spliced<T: ?Sized + Serialize, E: ser::Error>(
        &self,
        key: &'static str,
        value: &T,
    ) -> Result<bool, E> {
        if self.unordered.is_empty() || !self.unordered.contains(&key) {
            return Ok(false);
        }
        let Mode::Write { indices, out } = self.mode else {
            return Ok(false);
        };
        value
            .serialize(Collect { indices })
            .and_then(|collected| return collected.splice(out))
            .map_err(|error| return E::custom(format!("{}.{key}: {error}", self.name)))?;
        return Ok(true);
    }
}

impl<C: SerializeSeq> SerializeSeq for Compound<'_, C> {
    type Ok = C::Ok;
    type Error = C::Error;

    fn serialize_element<T: ?Sized + Serialize>(&mut self, value: &T) -> Result<(), C::Error> {
        return self.inner.serialize_element(&Wrapped(value, self.mode));
    }

    fn end(self) -> Result<C::Ok, C::Error> {
        return self.inner.end();
    }
}

impl<C: SerializeTuple> SerializeTuple for Compound<'_, C> {
    type Ok = C::Ok;
    type Error = C::Error;

    fn serialize_element<T: ?Sized + Serialize>(&mut self, value: &T) -> Result<(), C::Error> {
        return self.inner.serialize_element(&Wrapped(value, self.mode));
    }

    fn end(self) -> Result<C::Ok, C::Error> {
        return self.inner.end();
    }
}

impl<C: SerializeTupleStruct> SerializeTupleStruct for Compound<'_, C> {
    type Ok = C::Ok;
    type Error = C::Error;

    fn serialize_field<T: ?Sized + Serialize>(&mut self, value: &T) -> Result<(), C::Error> {
        return self.inner.serialize_field(&Wrapped(value, self.mode));
    }

    fn end(self) -> Result<C::Ok, C::Error> {
        return self.inner.end();
    }
}

impl<C: SerializeTupleVariant> SerializeTupleVariant for Compound<'_, C> {
    type Ok = C::Ok;
    type Error = C::Error;

    fn serialize_field<T: ?Sized + Serialize>(&mut self, value: &T) -> Result<(), C::Error> {
        return self.inner.serialize_field(&Wrapped(value, self.mode));
    }

    fn end(self) -> Result<C::Ok, C::Error> {
        return self.inner.end();
    }
}

impl<C: SerializeMap> SerializeMap for Compound<'_, C> {
    type Ok = C::Ok;
    type Error = C::Error;

    fn serialize_key<T: ?Sized + Serialize>(&mut self, key: &T) -> Result<(), C::Error> {
        return self.inner.serialize_key(&Wrapped(key, self.mode));
    }

    fn serialize_value<T: ?Sized + Serialize>(&mut self, value: &T) -> Result<(), C::Error> {
        return self.inner.serialize_value(&Wrapped(value, self.mode));
    }

    fn end(self) -> Result<C::Ok, C::Error> {
        return self.inner.end();
    }
}

impl<C: SerializeStruct> SerializeStruct for Compound<'_, C> {
    type Ok = C::Ok;
    type Error = C::Error;

    fn serialize_field<T: ?Sized + Serialize>(
        &mut self,
        key: &'static str,
        value: &T,
    ) -> Result<(), C::Error> {
        if self.spliced(key, value)? {
            return Ok(());
        }
        return self.inner.serialize_field(key, &Wrapped(value, self.mode));
    }

    fn end(self) -> Result<C::Ok, C::Error> {
        return self.inner.end();
    }
}

impl<C: SerializeStructVariant> SerializeStructVariant for Compound<'_, C> {
    type Ok = C::Ok;
    type Error = C::Error;

    fn serialize_field<T: ?Sized + Serialize>(
        &mut self,
        key: &'static str,
        value: &T,
    ) -> Result<(), C::Error> {
        if self.spliced(key, value)? {
            return Ok(());
        }
        return self.inner.serialize_field(key, &Wrapped(value, self.mode));
    }

    fn end(self) -> Result<C::Ok, C::Error> {
        return self.inner.end();
    }
}

macro_rules! forwarded_scalars {
    ($($method:ident($type:ty)),* $(,)?) => {
        $(
            fn $method(self, value: $type) -> Result<S::Ok, S::Error> {
                return self.inner.$method(value);
            }
        )*
    };
}

impl<'a, S: Serializer> Serializer for Forward<'a, S> {
    type Ok = S::Ok;
    type Error = S::Error;
    type SerializeSeq = Compound<'a, S::SerializeSeq>;
    type SerializeTuple = Compound<'a, S::SerializeTuple>;
    type SerializeTupleStruct = Compound<'a, S::SerializeTupleStruct>;
    type SerializeTupleVariant = Compound<'a, S::SerializeTupleVariant>;
    type SerializeMap = Compound<'a, S::SerializeMap>;
    type SerializeStruct = Compound<'a, S::SerializeStruct>;
    type SerializeStructVariant = Compound<'a, S::SerializeStructVariant>;

    forwarded_scalars!(
        serialize_bool(bool),
        serialize_i8(i8),
        serialize_i16(i16),
        serialize_i32(i32),
        serialize_i64(i64),
        serialize_i128(i128),
        serialize_u8(u8),
        serialize_u16(u16),
        serialize_u32(u32),
        serialize_u64(u64),
        serialize_u128(u128),
        serialize_f32(f32),
        serialize_f64(f64),
        serialize_char(char),
        serialize_str(&str),
        serialize_unit_struct(&'static str),
    );

    fn is_human_readable(&self) -> bool {
        return self.inner.is_human_readable();
    }

    fn serialize_bytes(self, word: &[u8]) -> Result<S::Ok, S::Error> {
        let index = match self.mode {
            Mode::Count(counts) => {
                let mut counts = counts.borrow_mut();
                if let Some(count) = counts.get_mut(word) {
                    *count = count.saturating_add(1);
                } else {
                    counts.insert(word.into(), 1);
                }
                0
            }
            Mode::Write { indices, .. } => *indices.get(word).ok_or_else(|| {
                return ser::Error::custom("a word is missing from the word table");
            })?,
        };
        return self.inner.serialize_u32(index);
    }

    fn serialize_none(self) -> Result<S::Ok, S::Error> {
        return self.inner.serialize_none();
    }

    fn serialize_some<T: ?Sized + Serialize>(self, value: &T) -> Result<S::Ok, S::Error> {
        return self.inner.serialize_some(&Wrapped(value, self.mode));
    }

    fn serialize_unit(self) -> Result<S::Ok, S::Error> {
        return self.inner.serialize_unit();
    }

    fn serialize_unit_variant(
        self,
        name: &'static str,
        index: u32,
        variant: &'static str,
    ) -> Result<S::Ok, S::Error> {
        return self.inner.serialize_unit_variant(name, index, variant);
    }

    fn serialize_newtype_struct<T: ?Sized + Serialize>(
        self,
        name: &'static str,
        value: &T,
    ) -> Result<S::Ok, S::Error> {
        return self
            .inner
            .serialize_newtype_struct(name, &Wrapped(value, self.mode));
    }

    fn serialize_newtype_variant<T: ?Sized + Serialize>(
        self,
        name: &'static str,
        index: u32,
        variant: &'static str,
        value: &T,
    ) -> Result<S::Ok, S::Error> {
        return self.inner.serialize_newtype_variant(
            name,
            index,
            variant,
            &Wrapped(value, self.mode),
        );
    }

    fn serialize_seq(self, length: Option<usize>) -> Result<Self::SerializeSeq, S::Error> {
        let inner = self.inner.serialize_seq(length)?;
        return Ok(Compound::new(inner, self.mode, ""));
    }

    fn serialize_tuple(self, length: usize) -> Result<Self::SerializeTuple, S::Error> {
        let inner = self.inner.serialize_tuple(length)?;
        return Ok(Compound::new(inner, self.mode, ""));
    }

    fn serialize_tuple_struct(
        self,
        name: &'static str,
        length: usize,
    ) -> Result<Self::SerializeTupleStruct, S::Error> {
        let inner = self.inner.serialize_tuple_struct(name, length)?;
        return Ok(Compound::new(inner, self.mode, name));
    }

    fn serialize_tuple_variant(
        self,
        name: &'static str,
        index: u32,
        variant: &'static str,
        length: usize,
    ) -> Result<Self::SerializeTupleVariant, S::Error> {
        let inner = self
            .inner
            .serialize_tuple_variant(name, index, variant, length)?;
        return Ok(Compound::new(inner, self.mode, name));
    }

    fn serialize_map(self, length: Option<usize>) -> Result<Self::SerializeMap, S::Error> {
        let inner = self.inner.serialize_map(length)?;
        return Ok(Compound::new(inner, self.mode, ""));
    }

    fn serialize_struct(
        self,
        name: &'static str,
        length: usize,
    ) -> Result<Self::SerializeStruct, S::Error> {
        let inner = self.inner.serialize_struct(name, length)?;
        return Ok(Compound::new(inner, self.mode, name));
    }

    fn serialize_struct_variant(
        self,
        name: &'static str,
        index: u32,
        variant: &'static str,
        length: usize,
    ) -> Result<Self::SerializeStructVariant, S::Error> {
        let inner = self
            .inner
            .serialize_struct_variant(name, index, variant, length)?;
        return Ok(Compound::new(inner, self.mode, name));
    }
}

pub(crate) fn encode<T: Serialize>(value: &T) -> Result<Vec<u8>, Error> {
    let counts = RefCell::new(Counts::default());
    bincode::serde::encode_into_writer(Wrapped(value, Mode::Count(&counts)), Discard, standard())?;
    let counts = counts.into_inner();
    let mut ranked: Vec<(&[u8], u32)> = counts
        .iter()
        .map(|(word, count)| return (word.as_ref(), *count))
        .collect();
    ranked.sort_unstable_by_key(|(word, count)| return (Reverse(*count), *word));
    let mut indices = Indices::with_capacity_and_hasher(ranked.len(), FixedState::default());
    let mut words = Vec::with_capacity(ranked.len());
    for (index, (word, _)) in ranked.into_iter().enumerate() {
        indices.insert(
            word,
            u32::try_from(index).map_err(<Error as ser::Error>::custom)?,
        );
        words.push(Word::new(word));
    }
    let out = RefCell::new(bincode::serde::encode_to_vec(&words, standard())?);
    let mode = Mode::Write {
        indices: &indices,
        out: &out,
    };
    bincode::serde::encode_into_writer(Wrapped(value, mode), Shared(&out), standard())?;
    return Ok(out.into_inner());
}
