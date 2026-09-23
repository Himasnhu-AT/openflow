//! Every SelfTest.swift rule check, one test per area (the cases themselves live in `selftest.rs` so `--selftest`
//! runs exactly the same ones).

use super::selftest::{self, Checker};

fn run(area: fn(&mut Checker)) {
    let mut c = Checker::default();
    area(&mut c);
    assert!(c.failures.is_empty(), "{} passed, failures:\n{}", c.passed, c.failures.join("\n"));
    assert!(c.passed > 0);
}

#[test]
fn fillers() {
    run(selftest::fillers);
}

#[test]
fn commands() {
    run(selftest::commands);
}

#[test]
fn emoji() {
    run(selftest::emoji);
}

#[test]
fn guardrail() {
    run(selftest::guardrail);
}

#[test]
fn lists() {
    run(selftest::lists);
}

#[test]
fn numbers() {
    run(selftest::numbers);
}

#[test]
fn dictionary() {
    run(selftest::dictionary);
}

#[test]
fn learning() {
    run(selftest::learning);
}

#[test]
fn meetings() {
    run(selftest::meetings);
}

#[test]
fn whisper() {
    run(selftest::whisper);
}

#[test]
fn layout() {
    run(selftest::layout);
}

#[test]
fn wer() {
    run(selftest::wer);
}

#[test]
fn self_test_all_pass() {
    let (passed, failures) = selftest::self_test();
    assert!(failures.is_empty(), "{failures:#?}");
    assert_eq!(passed, 106); // SelfTest.swift has 110; the 2 Segmenter and 2 Meeting checks belong to team B
}

#[test]
fn benchmark_runs() {
    assert!(selftest::rules_benchmark_ms() > 0.0);
}
