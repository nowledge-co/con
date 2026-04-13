use con_agent::AgentConfig;
use parking_lot::Mutex;
use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::runtime::Runtime;

const SUGGESTION_MAX_LEN: usize = 200;

#[derive(Debug, Clone, Default)]
pub struct SuggestionContext {
    pub cwd: Option<String>,
    pub recent_commands: Vec<String>,
}

/// Debounced shell command suggestion engine.
///
/// When the user types at a shell prompt, this engine debounces input,
/// requests a lightweight AI completion, and caches results.
pub struct SuggestionEngine {
    cache: Arc<Mutex<HashMap<String, String>>>,
    last_request: Arc<Mutex<Option<Instant>>>,
    pending: Arc<Mutex<Option<String>>>,
    debounce_ms: u64,
    config: AgentConfig,
    runtime: Arc<Runtime>,
}

impl SuggestionEngine {
    pub fn new(config: AgentConfig, runtime: Arc<Runtime>, debounce_ms: u64) -> Self {
        Self {
            cache: Arc::new(Mutex::new(HashMap::new())),
            last_request: Arc::new(Mutex::new(None)),
            pending: Arc::new(Mutex::new(None)),
            debounce_ms,
            config,
            runtime,
        }
    }

    /// Request a suggestion for the given command prefix.
    /// Returns a cached result immediately if available.
    /// Otherwise, starts a debounced async request.
    pub fn request(
        &self,
        prefix: &str,
        context: SuggestionContext,
        callback: impl FnOnce(String) + Send + 'static,
    ) {
        if prefix.trim().is_empty() {
            log::debug!(target: "con_core::suggestions", "skip ai suggestion: empty prefix");
            return;
        }

        let cache_key = match context.cwd.as_deref() {
            Some(cwd) if !cwd.is_empty() => format!("{cwd}\n{prefix}"),
            _ => prefix.to_string(),
        };

        // Check cache
        if let Some(cached) = self.cache.lock().get(&cache_key) {
            log::debug!(
                target: "con_core::suggestions",
                "ai suggestion cache hit prefix={:?} completion={:?}",
                prefix,
                cached
            );
            callback(cached.clone());
            return;
        }

        log::debug!(
            target: "con_core::suggestions",
            "queue ai suggestion prefix={:?} cwd={:?} recent_commands={}",
            prefix,
            context.cwd,
            context.recent_commands.len()
        );

        // Update pending request
        *self.pending.lock() = Some(prefix.to_string());
        *self.last_request.lock() = Some(Instant::now());

        let debounce = Duration::from_millis(self.debounce_ms);
        let pending = self.pending.clone();
        let cache = self.cache.clone();
        let last_request = self.last_request.clone();
        let config = self.config.clone();
        let prefix_owned = prefix.to_string();
        let cache_key_owned = cache_key;
        let context_owned = context;

        self.runtime.spawn(async move {
            tokio::time::sleep(debounce).await;

            // Check if this is still the latest request
            let current_pending = pending.lock().clone();
            if current_pending.as_deref() != Some(&prefix_owned) {
                log::debug!(
                    target: "con_core::suggestions",
                    "drop ai suggestion prefix={:?}: superseded by {:?}",
                    prefix_owned,
                    current_pending
                );
                return;
            }

            // Check if enough time has passed since last request
            let elapsed = last_request
                .lock()
                .map(|t| t.elapsed())
                .unwrap_or(Duration::ZERO);
            if elapsed < debounce {
                log::debug!(
                    target: "con_core::suggestions",
                    "drop ai suggestion prefix={:?}: debounce not elapsed ({:?})",
                    prefix_owned,
                    elapsed
                );
                return;
            }

            // Make the completion request
            log::debug!(
                target: "con_core::suggestions",
                "dispatch ai suggestion prefix={:?}",
                prefix_owned
            );
            if let Some(completion) =
                request_completion(&config, &prefix_owned, &context_owned).await
            {
                if !completion.is_empty() {
                    cache.lock().insert(cache_key_owned, completion.clone());
                    log::debug!(
                        target: "con_core::suggestions",
                        "ai suggestion result prefix={:?} completion={:?}",
                        prefix_owned,
                        completion
                    );
                    callback(completion);
                } else {
                    log::debug!(
                        target: "con_core::suggestions",
                        "ai suggestion empty result prefix={:?}",
                        prefix_owned
                    );
                }
            }
        });
    }

    /// Clear the suggestion cache
    pub fn clear_cache(&self) {
        self.cache.lock().clear();
    }

    /// Cancel any pending request
    pub fn cancel(&self) {
        log::debug!(target: "con_core::suggestions", "cancel ai suggestion pending request");
        *self.pending.lock() = None;
    }
}

/// Lightweight completion request using the configured AI provider
async fn request_completion(
    config: &AgentConfig,
    prefix: &str,
    context: &SuggestionContext,
) -> Option<String> {
    use con_agent::AgentProvider;

    let recent_commands = if context.recent_commands.is_empty() {
        "none".to_string()
    } else {
        context
            .recent_commands
            .iter()
            .take(3)
            .map(|cmd| format!("- {cmd}"))
            .collect::<Vec<_>>()
            .join("\n")
    };
    let prompt = format!(
        "Complete this shell command suffix.\n\
         Return only the missing trailing characters.\n\
         No explanations. No newline. No shell chaining. No destructive extras.\n\
         Preserve quoting and spaces.\n\
         If the best completion is identical to the prefix, return empty.\n\
         If unsure, return empty.\n\n\
         cwd: {}\n\
         recent:\n{}\n\n\
         prefix: {}",
        context.cwd.as_deref().unwrap_or("(unknown)"),
        recent_commands,
        prefix,
    );

    let provider = AgentProvider::new(config.clone());
    log::debug!(
        target: "con_core::suggestions",
        "request ai completion provider={:?} model={} prefix={:?}",
        config.provider,
        config.effective_model(&config.provider),
        prefix
    );
    match provider.complete(&prompt).await {
        Ok(completion) => {
            let cleaned = completion
                .trim_matches(|c| matches!(c, '\n' | '\r' | '\t'))
                .to_string();
            if cleaned.len() <= SUGGESTION_MAX_LEN && !cleaned.contains('\n') {
                Some(cleaned)
            } else {
                None
            }
        }
        Err(e) => {
            log::debug!(target: "con_core::suggestions", "Suggestion completion failed: {}", e);
            None
        }
    }
}
