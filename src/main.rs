use std::io::{self, Write as _};

fn main() -> io::Result<()> {
    return writeln!(io::stdout().lock(), "{}", opcov::greet("world"));
}
