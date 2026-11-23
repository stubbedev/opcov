#![no_main]

use libfuzzer_sys::fuzz_target;
use opcov_core::report::Report;

fuzz_target!(|data: &[u8]| {
    let Ok(report) = Report::read(data) else {
        return;
    };
    let written = report
        .write(Vec::new())
        .expect("writing to memory succeeds");
    let reread = Report::read(written.as_slice()).expect("a written report reads back");
    assert_eq!(reread, report, "a report round-trips");
    let again = reread
        .write(Vec::new())
        .expect("writing to memory succeeds");
    assert_eq!(again, written, "writing is canonical");
});
