#![no_main]

use libfuzzer_sys::fuzz_target;
use opcov_core::report::{Report, merge};

fuzz_target!(|data: &[u8]| {
    let Some(split) = data.iter().position(|&byte| byte == 0) else {
        return;
    };
    let (left, right) = data.split_at(split);
    let (Ok(first), Ok(second)) = (Report::read(left), Report::read(&right[1..])) else {
        return;
    };
    let Ok(merged) = merge(&[first.clone(), second.clone()]) else {
        return;
    };
    let swapped = merge(&[second, first]).expect("merging is symmetric in what it accepts");
    assert!(
        merged.files().keys().eq(swapped.files().keys()),
        "both orders see the same files"
    );
    let written = merged
        .write(Vec::new())
        .expect("writing to memory succeeds");
    let reread = Report::read(written.as_slice()).expect("a merged report reads back");
    assert_eq!(reread, merged, "a merged report round-trips");
});
