use super::*;

#[test]
fn given_workers_when_partitions_are_split_then_should_have_exactly_one_owner_each_partition() {
    let owners: Vec<Vec<u32>> = (1..=3)
        .map(|worker| {
            owned_partitions(
                8,
                3,
                &ReaderSpec {
                    worker,
                    ..ReaderSpec::filtered("maintenance")
                },
            )
        })
        .collect();
    assert_eq!(owners, vec![vec![0, 3, 6], vec![1, 4, 7], vec![2, 5]]);
    assert_eq!(
        owned_partitions(4, 1, &ReaderSpec::filtered("maintenance")),
        vec![0, 1, 2, 3]
    );
    assert_eq!(
        owned_partitions(4, 3, &ReaderSpec::baseline("maintenance")),
        vec![0, 1, 2, 3]
    );
    assert!(
        owned_partitions(
            2,
            4,
            &ReaderSpec {
                worker: 4,
                ..ReaderSpec::filtered("maintenance")
            }
        )
        .is_empty()
    );
}
