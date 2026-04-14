## What happened

Workspace session saves were moved off the interactive path to improve split and tab responsiveness.

That removed synchronous disk I/O from the UI path, but the first implementation spawned detached background writes with no ordering guarantees.

## Root cause

Each `save_session()` call created an independent background task that wrote the full session snapshot to disk.

That meant:

- multiple saves could finish out of order
- an older snapshot could overwrite a newer one
- shutdown could exit before the latest queued save finished

## Fix applied

- Replaced detached per-save tasks with a single ordered session-save worker.
- The worker coalesces rapid save requests and writes only the latest snapshot.
- Quit and window-close paths now flush the final snapshot before teardown.

## What we learned

- Moving blocking work off the UI path is necessary, but detached writes are not a correctness strategy.
- Persistence paths need both responsiveness and ordering guarantees.
- Session save queues should be designed explicitly, not assembled from ad hoc background tasks.
