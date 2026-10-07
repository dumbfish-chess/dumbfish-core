use dumbfish::search::MATE;
use dumbfish::wdl::{MODEL, parse_model};

#[test]
fn shipped_model() {
    assert!(MODEL.a > 0.0 && MODEL.b > 0.0);
    let [win, draw, loss] = MODEL.wdl(0);
    assert_eq!(win, loss, "an equal position is symmetric");
    assert!(draw > 0);
}

#[test]
fn probabilities() {
    let mut last_win = 0;
    for score in (-2000..=2000).step_by(50) {
        let [win, draw, loss] = MODEL.wdl(score);
        assert_eq!(win + draw + loss, 1000, "{score}");
        assert_eq!(MODEL.wdl(-score), [loss, draw, win], "{score}");
        assert!(win >= last_win, "wins grow with the score at {score}");
        last_win = win;
    }
    assert_eq!(MODEL.wdl(MATE - 5), [1000, 0, 0]);
    assert_eq!(MODEL.wdl(4 - MATE), [0, 0, 1000]);
}

#[test]
fn model_format() {
    let model = parse_model("# a b\n\n120 100\n").unwrap();
    assert_eq!((model.a, model.b), (120.0, 100.0));
    for (bad, error) in [
        ("", "no model"),
        ("1 2\n3 4\n", "more than one"),
        ("1 x\n", "invalid number"),
        ("1\n", "two positive"),
        ("-1 2\n", "two positive"),
    ] {
        let message = parse_model(bad).err().unwrap();
        assert!(message.contains(error), "{message:?} for {bad:?}");
    }
}
