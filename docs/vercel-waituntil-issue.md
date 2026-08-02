# [rust runtime] `waitUntil` futures stop executing after the response is sent, contrary to documented behavior

## Documented behavior

The Rust runtime docs (https://vercel.com/docs/functions/runtimes/rust) list `waitUntil` as supported, and `vercel_runtime` 2.4.0's own doc comments say:

`AppState::wait_until` (`src/lib.rs`):

> "Register a background future **to keep running after the response has been sent**."

`Awaiter::wait_until` (`src/awaiter.rs`):

> "The future is spawned onto the current Tokio runtime immediately **so it makes progress between requests**."

## Actual behavior

The future stops executing as soon as the response is sent. It only advances in short slices when later, unrelated requests wake the same instance; with no follow-up traffic it never completes. Wall-clock time keeps passing while it is parked, so timers and timeouts inside the future fire spuriously on the next wake-up.

Reproduction: a handler registers a 30-tick, once-per-second logger via `wait_until` (each tick logs monotonic `elapsed_ms`) and responds immediately. One trigger request, 75 s of no traffic, one `GET /health`, 40 more silent seconds, one more `GET /health`. Runtime log timeline (region `pdx1`, deployment `finstream-nap6sdy7q`, 2026-08-02 UTC):

| Time (UTC) | Event |
| --- | --- |
| 03:35:42.835 | Trigger request registers the ticker via `wait_until`; response sent in 1.4 ms |
| 03:35:43 – 03:36:57 | No traffic — no ticks, despite 1-per-second cadence |
| 03:36:58.243 | `tick=1` (due at T0+1 s) runs **75.4 s late** (`elapsed_ms=75408`), logged inside the first `/health` invocation's log group |
| 03:37:38.470 | `tick=2` (due at T0+2 s) runs at **T0+115.6 s** (`elapsed_ms=115634`), inside the second `/health` invocation |
| — | Ticks 3–30 and the completion log never executed |

Exactly one tick per wake-up, each attached to the waking request's log group rather than the registering invocation's. Note `elapsed_ms=75408` across a 1-second `tokio::time::sleep`.

The cause appears to be that the per-request `end` IPC message is sent immediately after the handler returns, without draining the `Awaiter`, so the instance is suspended with the future still pending. The SIGTERM drain doesn't cover this, since suspension is not shutdown.

I've opened https://github.com/vercel/vercel/pull/17350 with a possible fix and before/after measurements — I don't know whether that's the approach you'd want to take here, but it does fix the issue: with the `end` message deferred until the request's `waitUntil` futures settle, the same experiment completes all 30 ticks at exact 1-second cadence with unchanged response latency.
