use std::path::PathBuf;
use std::{env, fmt, fs, io};

const PRELUDE: &str = "prelude.bin";

#[derive(Debug)]
enum Error {
    OutDir,
    Encode(String),
    Write(io::Error),
}

impl fmt::Display for Error {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        return match self {
            Self::OutDir => formatter.write_str("cargo did not set OUT_DIR"),
            Self::Encode(error) => write!(formatter, "cannot encode the PHP prelude: {error}"),
            Self::Write(error) => write!(formatter, "cannot write the PHP prelude: {error}"),
        };
    }
}

impl std::error::Error for Error {}

fn main() -> Result<(), Error> {
    let out_dir = PathBuf::from(env::var_os("OUT_DIR").ok_or(Error::OutDir)?);
    let bytes = mago_prelude::Prelude::build()
        .encode()
        .map_err(|error| return Error::Encode(error.to_string()))?;
    fs::write(out_dir.join(PRELUDE), bytes).map_err(Error::Write)?;
    println!("cargo::rerun-if-changed=build.rs");
    return Ok(());
}
