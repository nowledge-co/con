use std::sync::OnceLock;

pub(crate) fn perf_trace_enabled() -> bool {
    static ENABLED: OnceLock<bool> = OnceLock::new();
    *ENABLED.get_or_init(|| {
        std::env::var_os("CON_GHOSTTY_PROFILE").is_some_and(|v| !v.is_empty() && v != "0")
    })
}

pub(crate) fn perf_trace_verbose() -> bool {
    static VERBOSE: OnceLock<bool> = OnceLock::new();
    *VERBOSE.get_or_init(|| {
        std::env::var_os("CON_GHOSTTY_PROFILE_VERBOSE").is_some_and(|v| !v.is_empty() && v != "0")
    })
}
