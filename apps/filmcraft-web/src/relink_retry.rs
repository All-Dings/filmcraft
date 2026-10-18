//! Retry a failed relink only while asynchronous media reads are pending.

use std::future::Future;

const MAX_ATTEMPTS: usize = 2000;

/// A failed relink has not applied its edit yet. Wait for queued reads before retrying;
/// never repeat a successful edit, even if matching other clips queued more reads.
pub async fn run<T, A, AF, W, WF>(mut attempt: A, mut wait: W) -> Result<T, String>
where
    A: FnMut() -> AF,
    AF: Future<Output = (Result<T, String>, bool)>,
    W: FnMut() -> WF,
    WF: Future<Output = ()>,
{
    for n in 0..MAX_ATTEMPTS {
        let (result, pending) = attempt().await;
        if result.is_ok() || !pending {
            return result;
        }
        if n + 1 < MAX_ATTEMPTS {
            wait().await;
        }
    }
    Err("Timed out loading media for relinking".into())
}
