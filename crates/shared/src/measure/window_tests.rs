use super::*;

#[test]
fn given_two_windows_on_one_partition_when_closed_then_should_give_independent_receipts() {
    let mut accumulator = accumulator();
    accumulator
        .record(observed(0, 1, b"first", true))
        .expect("record fits");
    let first = accumulator
        .checkpoint(0, WindowId(0), 10)
        .expect("the checkpoint closes its window");
    accumulator
        .record(observed(1, 3, b"second", true))
        .expect("record fits");
    let second = accumulator
        .checkpoint(0, WindowId(1), 20)
        .expect("the checkpoint closes its window");
    assert_eq!((first.matches, first.matched_bytes), (1, 5));
    assert_eq!((second.matches, second.matched_bytes), (1, 6));
    assert_ne!(first.digest, second.digest);
    assert_eq!(second.window_id, WindowId(1));
}

#[test]
fn given_a_record_of_the_next_window_before_its_checkpoint_when_recorded_then_should_fail() {
    let mut accumulator = accumulator();
    accumulator
        .record(observed(0, 1, b"a", false))
        .expect("record fits");
    assert_eq!(
        accumulator.record(observed(1, 2, b"b", false)),
        Err(Overflow::MissedCheckpoint {
            partition_id: 0,
            open: WindowId(0),
            next: WindowId(1)
        })
    );
}

#[test]
fn given_a_redelivered_record_when_recorded_then_should_count_its_bytes_but_not_a_second_match() {
    let mut accumulator = accumulator();
    accumulator
        .record(observed(0, 4, b"ab", true))
        .expect("record fits");
    accumulator
        .record(observed(0, 4, b"ab", true))
        .expect("duplicate is not an error");
    let receipt = accumulator
        .checkpoint(0, WindowId(0), 5)
        .expect("the checkpoint closes its window");
    let mut single = RollingDigest::default();
    single.push(4, 4, b"ab");
    assert_eq!(
        (receipt.matches, receipt.matched_bytes, receipt.duplicates),
        (1, 2, 1)
    );
    assert_eq!((receipt.received_records, receipt.received_bytes), (2, 4));
    assert_eq!(receipt.digest, single.finish());
}

#[test]
fn given_an_open_window_when_another_checkpoint_arrives_then_should_reject_without_losing_counters()
{
    let mut accumulator = accumulator();
    accumulator
        .record(observed(0, 1, b"abc", true))
        .expect("record fits");
    assert_eq!(
        accumulator.checkpoint(0, WindowId(1), 2),
        Err(Overflow::MissedCheckpoint {
            partition_id: 0,
            open: WindowId(0),
            next: WindowId(1),
        })
    );
    let receipt = accumulator
        .checkpoint(0, WindowId(0), 2)
        .expect("the correct checkpoint succeeds");
    assert_eq!((receipt.matches, receipt.matched_bytes), (1, 3));
}

#[test]
fn given_rejected_records_when_recorded_then_should_count_them_as_received_only() {
    let mut accumulator = accumulator();
    accumulator
        .record(observed(0, 1, b"abc", false))
        .expect("record fits");
    let receipt = accumulator
        .checkpoint(0, WindowId(0), 2)
        .expect("the checkpoint closes its window");
    assert_eq!((receipt.received_bytes, receipt.matched_bytes), (3, 0));
}

#[test]
fn given_a_closed_window_when_its_tail_and_checkpoint_repeat_then_should_reemit_the_original_receipt()
 {
    let mut accumulator = accumulator();
    assert!(
        accumulator
            .record(observed(0, 4, b"payload", true))
            .expect("first delivery")
    );
    let original = accumulator
        .checkpoint(0, WindowId(0), 5)
        .expect("checkpoint");
    assert!(
        !accumulator
            .record(observed(0, 4, b"payload", true))
            .expect("redelivery")
    );
    assert_eq!(
        accumulator.checkpoint(0, WindowId(0), 5).expect("repeat"),
        original
    );
    assert!(
        accumulator
            .record(observed(1, 6, b"next", true))
            .expect("new window")
    );
    assert_eq!(
        accumulator
            .checkpoint(0, WindowId(0), 5)
            .expect("late checkpoint"),
        original
    );
    assert_eq!(
        accumulator
            .checkpoint(0, WindowId(1), 7)
            .expect("new checkpoint")
            .matches,
        1
    );
}

#[test]
fn given_two_closed_windows_when_an_older_checkpoint_reappears_then_should_refuse_a_conflicting_receipt()
 {
    let mut accumulator = accumulator();
    for window in 0..2 {
        accumulator
            .record(observed(window, window + 1, b"data", true))
            .expect("record");
        accumulator
            .checkpoint(0, WindowId(window), window + 2)
            .expect("checkpoint");
    }
    assert_eq!(
        accumulator.checkpoint(0, WindowId(0), 2),
        Err(Overflow::RetiredCheckpoint {
            partition_id: 0,
            checkpoint: WindowId(0),
            completed: WindowId(1),
        })
    );
    assert!(accumulator.closed(0, WindowId(0)));
}

fn accumulator() -> WindowAccumulator {
    WindowAccumulator::new("0badc0de".parse().expect("run id"), "food-safety", false, 1)
}

fn observed(window: u64, sequence: u64, payload: &[u8], matched: bool) -> Observed<'_> {
    Observed {
        partition_id: 0,
        window_id: WindowId(window),
        event_id: sequence,
        sequence: Sequence(sequence),
        payload,
        matched,
    }
}
