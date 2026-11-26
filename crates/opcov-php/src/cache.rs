use std::collections::BTreeMap;
use std::fmt;

use bincode::config::standard;
use mago_database::file::FileId;
use serde::{Deserialize, Serialize};

use crate::canonical;
use crate::index::Scanned;
use crate::references::Reach;

const MAGIC: &[u8] = b"opcov scan cache 1\n";
const FORMAT: &[u8] = b"opcov scan cache format 3";
const DIGEST_BYTES: usize = 32;
const FILE_ID_PROBE: &[u8] = b"opcov";

pub(crate) type Digest = [u8; DIGEST_BYTES];

pub(crate) fn digest(bytes: &[u8]) -> Digest {
    return *blake3::hash(bytes).as_bytes();
}

#[derive(Debug)]
pub enum CacheError {
    Stale,
    Corrupt(String),
    Encode(String),
}

impl fmt::Display for CacheError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        return match self {
            Self::Stale => formatter.write_str("it was written by another opcov build"),
            Self::Corrupt(reason) => write!(formatter, "it is corrupt: {reason}"),
            Self::Encode(error) => write!(formatter, "cannot encode the scan cache: {error}"),
        };
    }
}

impl std::error::Error for CacheError {}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub(crate) enum Stamp {
    Content(Digest),
    Vendor,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub(crate) struct Key {
    pub(crate) name: String,
    pub(crate) reach: Reach,
    pub(crate) stamp: Stamp,
}

#[derive(Serialize, Deserialize)]
struct Header {
    vendor: Option<Digest>,
    entries: Vec<(Key, u64)>,
}

pub struct ScanCache {
    identity: Digest,
    pub(crate) vendor: Option<Digest>,
    pub(crate) entries: BTreeMap<Key, Vec<u8>>,
}

impl fmt::Debug for ScanCache {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        return formatter
            .debug_struct("ScanCache")
            .field("vendor", &self.vendor.is_some())
            .field("entries", &self.entries.len())
            .finish_non_exhaustive();
    }
}

fn identity(build: &[u8]) -> Digest {
    let mut hasher = blake3::Hasher::new();
    hasher.update(FORMAT);
    hasher.update(&FileId::new(FILE_ID_PROBE).as_u64().to_le_bytes());
    hasher.update(build);
    return *hasher.finalize().as_bytes();
}

fn corrupt(reason: impl fmt::Display) -> CacheError {
    return CacheError::Corrupt(reason.to_string());
}

fn split(bytes: &[u8], at: usize) -> Result<(&[u8], &[u8]), CacheError> {
    return bytes
        .split_at_checked(at)
        .ok_or_else(|| return corrupt("it ends early"));
}

impl ScanCache {
    #[must_use]
    pub fn new(build: &[u8]) -> Self {
        return Self {
            identity: identity(build),
            vendor: None,
            entries: BTreeMap::new(),
        };
    }

    pub(crate) const fn emptied(&self, vendor: Option<Digest>) -> Self {
        return Self {
            identity: self.identity,
            vendor,
            entries: BTreeMap::new(),
        };
    }

    pub fn decode(build: &[u8], bytes: &[u8]) -> Result<Self, CacheError> {
        let identity = identity(build);
        let (magic, rest) = split(bytes, MAGIC.len())?;
        if magic != MAGIC {
            return Err(corrupt("it is not an opcov scan cache"));
        }
        let (written, rest) = split(rest, DIGEST_BYTES)?;
        if written != identity {
            return Err(CacheError::Stale);
        }
        let (checksum, body) = split(rest, DIGEST_BYTES)?;
        if checksum != digest(body) {
            return Err(corrupt("its checksum does not match its content"));
        }
        let (header, read): (Header, usize) =
            bincode::serde::decode_from_slice(body, standard()).map_err(corrupt)?;
        let (_, mut payloads) = split(body, read)?;
        let mut entries = BTreeMap::new();
        for (key, length) in header.entries {
            let length = usize::try_from(length).map_err(corrupt)?;
            let (payload, rest) = split(payloads, length)?;
            entries.insert(key, payload.to_vec());
            payloads = rest;
        }
        if !payloads.is_empty() {
            return Err(corrupt("it has bytes after its last entry"));
        }
        return Ok(Self {
            identity,
            vendor: header.vendor,
            entries,
        });
    }

    pub fn encode(&self) -> Result<Vec<u8>, CacheError> {
        let header = Header {
            vendor: self.vendor,
            entries: self
                .entries
                .iter()
                .map(|(key, payload)| {
                    let length = u64::try_from(payload.len())
                        .map_err(|error| return CacheError::Encode(error.to_string()))?;
                    return Ok((key.clone(), length));
                })
                .collect::<Result<_, CacheError>>()?,
        };
        let mut body = bincode::serde::encode_to_vec(&header, standard())
            .map_err(|error| return CacheError::Encode(error.to_string()))?;
        for payload in self.entries.values() {
            body.extend_from_slice(payload);
        }
        let mut bytes = Vec::with_capacity(
            MAGIC
                .len()
                .saturating_add(DIGEST_BYTES)
                .saturating_add(DIGEST_BYTES)
                .saturating_add(body.len()),
        );
        bytes.extend_from_slice(MAGIC);
        bytes.extend_from_slice(&self.identity);
        bytes.extend_from_slice(&digest(&body));
        bytes.extend_from_slice(&body);
        return Ok(bytes);
    }
}

pub(crate) fn encode_scanned(scanned: &Scanned) -> Result<Vec<u8>, CacheError> {
    let encoded =
        canonical::encode(scanned).map_err(|error| return CacheError::Encode(error.to_string()))?;
    return Ok(lz4_flex::block::compress_prepend_size(&encoded));
}

pub(crate) fn decode_scanned(compressed: &[u8]) -> Option<Scanned> {
    let bytes = lz4_flex::block::decompress_size_prepended(compressed).ok()?;
    let bytes = bytes.as_slice();
    let (words, read): (Vec<mago_word::Word>, usize) =
        bincode::serde::decode_from_slice(bytes, standard()).ok()?;
    let rest = bytes.get(read..)?;
    let (scanned, used): (Scanned, usize) = mago_word::table::decode(&words, || {
        return bincode::serde::decode_from_slice(rest, standard());
    })
    .ok()?;
    return (used == rest.len()).then_some(scanned);
}
