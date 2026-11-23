#[cfg(test)]
#[path = "support/arbitrary_report.rs"]
mod arbitrary_report;

#[cfg(test)]
mod properties {
    use std::io;

    use opcov_core::report::{Report, merge, merge_streams};
    use proptest::prelude::*;

    use super::arbitrary_report::{Run, Universe, bytes, run, universe};

    fn merged(reports: &[Report]) -> Vec<u8> {
        return bytes(&merge(reports).unwrap());
    }

    fn streamed(reports: &[Report]) -> Vec<u8> {
        let documents: Vec<Vec<u8>> = reports.iter().map(bytes).collect();
        let inputs: Vec<&[u8]> = documents.iter().map(Vec::as_slice).collect();
        return merge_streams(&inputs, |input| return io::Result::Ok(*input), Vec::new()).unwrap();
    }

    fn partition(
        universe: &Universe,
        owners: &[u8],
        worker_count: u8,
        files: u8,
        pattern: &[u8],
    ) -> (Report, Vec<Report>) {
        let workers = (0..worker_count)
            .map(|worker| {
                let tests = owners
                    .iter()
                    .enumerate()
                    .filter(|(_, owner)| return owner.checked_rem(worker_count) == Some(worker))
                    .fold(0_u8, |tests, (position, _)| {
                        return tests | 1_u8.checked_shl(u32::try_from(position).unwrap()).unwrap();
                    });
                return universe.report(&Run {
                    tests,
                    files,
                    pattern: pattern.to_vec(),
                });
            })
            .collect();
        let serial = universe.report(&Run {
            tests: u8::MAX,
            files,
            pattern: pattern.to_vec(),
        });
        return (serial, workers);
    }

    proptest! {
        #![proptest_config(ProptestConfig::with_cases(64))]

        #[test]
        fn merge_is_commutative(universe in universe(), a in run(), b in run()) {
            let (a, b) = (universe.report(&a), universe.report(&b));
            prop_assert_eq!(merged(&[a.clone(), b.clone()]), merged(&[b, a]));
        }

        #[test]
        fn merge_is_associative(universe in universe(), a in run(), b in run(), c in run()) {
            let (a, b, c) = (universe.report(&a), universe.report(&b), universe.report(&c));
            let left = merge(&[a.clone(), b.clone()]).unwrap();
            let right = merge(&[b.clone(), c.clone()]).unwrap();
            let flat = merged(&[a.clone(), b, c.clone()]);
            prop_assert_eq!(merged(&[left, c]), flat.clone());
            prop_assert_eq!(merged(&[a, right]), flat);
        }

        #[test]
        fn merge_is_idempotent(universe in universe(), a in run(), b in run()) {
            let (a, b) = (universe.report(&a), universe.report(&b));
            prop_assert_eq!(merged(&[a.clone()]), bytes(&a));
            prop_assert_eq!(merged(&[a.clone(), a.clone()]), bytes(&a));
            let both = merge(&[a, b.clone()]).unwrap();
            prop_assert_eq!(merged(&[both.clone(), b]), bytes(&both));
        }

        #[test]
        fn streaming_merge_equals_the_in_memory_merge(
            universe in universe(),
            runs in prop::collection::vec(run(), 1..6),
        ) {
            let reports: Vec<Report> = runs.iter().map(|run| return universe.report(run)).collect();
            prop_assert_eq!(streamed(&reports), merged(&reports));
        }

        #[test]
        fn workers_merge_to_the_serial_report(
            universe in universe(),
            owners in prop::collection::vec(any::<u8>(), 8),
            files in any::<u8>(),
            pattern in prop::collection::vec(any::<u8>(), 1..16),
            worker_count in prop::sample::select(vec![1_u8, 4, 8]),
        ) {
            let (serial, workers) = partition(&universe, &owners, worker_count, files, &pattern);
            let mut reversed = workers.clone();
            reversed.reverse();
            prop_assert_eq!(merged(&workers), bytes(&serial));
            prop_assert_eq!(streamed(&reversed), bytes(&serial));
        }
    }
}
