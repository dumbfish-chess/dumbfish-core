use std::time::Duration;

use std::time::Instant;

use dumbfish::time::{Deadline, MOVE_OVERHEAD, allocate};

fn ms(millis: u64) -> Duration {
    Duration::from_millis(millis)
}

#[test]
fn limits_stay_within_the_clock() {
    for time in [0, 10, 30, 31, 100, 1_000, 60_000, 3_600_000] {
        for inc in [0, 10, 100, 2_000] {
            for moves_to_go in [None, Some(0), Some(1), Some(5), Some(40)] {
                let (soft, hard) = allocate(ms(time), ms(inc), moves_to_go);
                let available = ms(time).saturating_sub(MOVE_OVERHEAD);
                assert!(
                    soft <= hard && hard <= available,
                    "{time}+{inc}/{moves_to_go:?}: {soft:?} {hard:?}"
                );
            }
        }
    }
}

#[test]
fn allocation() {
    assert_eq!(allocate(ms(60_000), ms(0), None), (ms(2_000), ms(6_000)));
    assert_eq!(
        allocate(ms(60_000), ms(1_000), None),
        (ms(2_750), ms(8_250))
    );
    assert_eq!(
        allocate(ms(10_000), ms(0), Some(1)),
        (ms(9_970), ms(9_970)),
        "last move before the control"
    );
    assert_eq!(
        allocate(ms(20), ms(0), None),
        (ms(0), ms(0)),
        "less than the overhead"
    );
}

#[test]
fn deadlines() {
    let deadline = Deadline::default();
    assert!(!deadline.is_set() && !deadline.passed());
    deadline.set(Some(Instant::now()));
    assert!(deadline.is_set() && deadline.passed());
    deadline.set(Some(Instant::now() + Duration::from_secs(60)));
    assert!(deadline.is_set() && !deadline.passed());
    deadline.set(None);
    assert!(!deadline.is_set() && !deadline.passed());
    assert!(Deadline::at(Instant::now()).passed());
}

#[test]
fn deadline_reports_its_moment() {
    let deadline = Deadline::default();
    assert_eq!(deadline.get(), None, "unset");
    let moment = Instant::now() + Duration::from_secs(5);
    deadline.set(Some(moment));
    let got = deadline.get().unwrap();
    let error = got.max(moment) - got.min(moment);
    assert!(error < Duration::from_micros(1), "{error:?}");
}
