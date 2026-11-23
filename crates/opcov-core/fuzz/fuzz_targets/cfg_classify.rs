#![no_main]

use libfuzzer_sys::fuzz_target;
use opcov_core::cfg::{Analysis, FunctionInput, Plan};

fuzz_target!(|data: &[u8]| {
    let Ok(input) = serde_json::from_slice::<FunctionInput>(data) else {
        return;
    };
    let Ok(analysis) = Analysis::try_from(input) else {
        return;
    };
    let plan = Plan::of(&analysis.function);
    let encoded = serde_json::to_vec(&analysis).expect("an analysis serialises");
    let decoded: Analysis = serde_json::from_slice(&encoded).expect("an analysis reads back");
    assert_eq!(decoded, analysis, "an analysis round-trips through JSON");
    assert_eq!(
        Plan::of(&decoded.function).outcomes(),
        plan.outcomes(),
        "the plan depends on the analysis only"
    );
});
