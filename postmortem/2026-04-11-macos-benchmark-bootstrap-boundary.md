# What happened

While pushing the terminal-agent benchmark toward autonomous multi-iteration runs, fresh benchmark-launched Con app instances on macOS frequently failed to expose a live Ghostty surface. The benchmark runner reported operator failures like "tab 1 does not expose a live, surface-ready pane with exec_visible_shell within 20.0s" even though the deeper problem was earlier in app bootstrap.

# Root cause

There were two separate issues:

1. Benchmark isolation was incomplete on macOS.
   `Session::load()` and conversation persistence used `dirs::data_dir()`, so fresh benchmark runs still inherited the real restored user session unless the app was explicitly pointed at dedicated storage paths.

2. Some subprocess-launched Con sessions in the benchmark environment could not acquire a Ghostty surface at all.
   In those cases the app log showed repeated `ghostty_surface_new returned null`, and no amount of pane-level waiting could make the benchmarked pane become `surface_ready`.

# Fix applied

- Added `CON_SESSION_PATH` support to session persistence.
- Added `CON_CONVERSATIONS_DIR` support to conversation persistence.
- Updated `iterate.py` to set those paths per iteration, alongside isolated socket/XDG paths.
- Added stronger bootstrap reassertion for newly created or newly focused terminals.
- Taught the benchmark batch runner to classify repeated Ghostty bootstrap failures as `blocked` with reason `ghostty_surface_bootstrap_unavailable` instead of misreporting them as scored product regressions.

# What we learned

- Benchmark isolation has to include every persistence path, not just socket and XDG roots.
- macOS GUI bootstrap limits are benchmark-environment facts, not product-performance facts. The benchmark needs to report that boundary honestly.
- A blocked environment run is still useful if it is classified clearly. It prevents the improvement loop from chasing fake regressions.
