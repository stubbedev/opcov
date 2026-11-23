use std::env;

fn main() {
    if env::var("CARGO_CFG_TARGET_OS").is_ok_and(|os| return os == "macos") {
        println!("cargo::rustc-link-arg=-undefined");
        println!("cargo::rustc-link-arg=dynamic_lookup");
    }
}
