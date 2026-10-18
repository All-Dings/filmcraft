//! Native tests of the web relink retry loop; no browser or media fixtures needed.

#[path = "../src/relink_retry.rs"]
mod relink_retry;

use std::cell::Cell;
use std::future::{Future, ready};
use std::task::{Context, Poll, Waker};

fn finish<T>(f: impl Future<Output = T>) -> T {
    // Test operations resolve immediately; their explicit wait callback models data arrival.
    let mut f = std::pin::pin!(f);
    match f.as_mut().poll(&mut Context::from_waker(Waker::noop())) {
        Poll::Ready(v) => v,
        Poll::Pending => panic!("test operation unexpectedly suspended"),
    }
}

#[test]
fn waits_for_missing_ranges_then_applies_exactly_one_edit() {
    let loaded_ranges = Cell::new(0);
    let attempts = Cell::new(0);
    let edits = Cell::new(0);
    let result = finish(relink_retry::run(
        || {
            attempts.set(attempts.get() + 1);
            ready(if loaded_ranges.get() < 3 {
                (Err("media data is still loading".into()), true)
            } else {
                edits.set(edits.get() + 1);
                (Ok("linked"), false)
            })
        },
        || {
            loaded_ranges.set(loaded_ranges.get() + 1);
            ready(())
        },
    ));
    assert_eq!(result, Ok("linked"));
    assert_eq!(attempts.get(), 4);
    assert_eq!(edits.get(), 1);
}

#[test]
fn permanent_errors_are_returned_without_waiting_or_retrying() {
    let attempts = Cell::new(0);
    let result = finish(relink_retry::run(
        || {
            attempts.set(attempts.get() + 1);
            ready((Err::<(), _>("fingerprint differs".into()), false))
        },
        || async { panic!("a permanent error must not wait for reads") },
    ));
    assert_eq!(result, Err("fingerprint differs".into()));
    assert_eq!(attempts.get(), 1);
}

#[test]
fn successful_edit_is_not_repeated_when_other_reads_are_pending() {
    let edits = Cell::new(0);
    let result = finish(relink_retry::run(
        || {
            edits.set(edits.get() + 1);
            ready((Ok("linked"), true))
        },
        || async { panic!("a successful edit must not be retried") },
    ));
    assert_eq!(result, Ok("linked"));
    assert_eq!(edits.get(), 1);
}

#[test]
fn repeated_pending_reads_have_a_bounded_number_of_attempts() {
    let attempts = Cell::new(0);
    let waits = Cell::new(0);
    let result = finish(relink_retry::run(
        || {
            attempts.set(attempts.get() + 1);
            ready((Err::<(), _>("media data is still loading".into()), true))
        },
        || {
            waits.set(waits.get() + 1);
            ready(())
        },
    ));
    assert_eq!(result, Err("Timed out loading media for relinking".into()));
    assert_eq!(attempts.get(), 2000);
    assert_eq!(waits.get(), 1999);
}
