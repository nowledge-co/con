use con_agent::AgentConfig;
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use tokio::runtime::Runtime;

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
    pub fn request(&self, prefix: &str, callback: impl FnOnce(String) + Send + 'static) {
        let trimmed = prefix.trim();
        if trimmed.is_empty() {
            return;
        }

        // Check cache
        if let Some(cached) = self.cache.lock().unwrap().get(trimmed) {
            callback(cached.clone());
            return;
        }

        // Update pending request
        *self.pending.lock().unwrap() = Some(trimmed.to_string());
        *self.last_request.lock().unwrap() = Some(Instant::now());

        let debounce = Duration::from_millis(self.debounce_ms);
        let pending = self.pending.clone();
        let cache = self.cache.clone();
        let last_request = self.last_request.clone();
        let config = self.config.clone();
        let prefix_owned = trimmed.to_string();

        self.runtime.spawn(async move {
            tokio::time::sleep(debounce).await;

            // Check if this is still the latest request
            let current_pending = pending.lock().unwrap().clone();
            if current_pending.as_deref() != Some(&prefix_owned) {
                return;
            }

            // Check if enough time has passed since last request
            let elapsed = last_request
                .lock()
                .unwrap()
                .map(|t| t.elapsed())
                .unwrap_or(Duration::ZERO);
            if elapsed < debounce {
                return;
            }

            // Make the completion request
            if let Some(completion) = request_completion(&config, &prefix_owned).await {
                if !completion.is_empty() {
                    cache
                        .lock()
                        .unwrap()
                        .insert(prefix_owned.clone(), completion.clone());
                    callback(completion);
                }
            }
        });
    }

    /// Clear the suggestion cache
    pub fn clear_cache(&self) {
        self.cache.lock().unwrap().clear();
    }

    /// Cancel any pending request
    pub fn cancel(&self) {
        *self.pending.lock().unwrap() = None;
    }
}

/// Lightweight completion request using the configured AI provider
async fn request_completion(config: &AgentConfig, prefix: &str) -> Option<String> {
    use con_agent::AgentProvider;

    let prompt = format!(
        "Complete this shell command. Reply with ONLY the remaining characters to complete the command, nothing else. If you cannot complete it, reply with an empty string.\n\nCommand so far: {}",
        prefix
    );

    let provider = AgentProvider::new(config.clone());
    match provider.complete(&prompt).await {
        Ok(completion) => {
            let trimmed = completion.trim().to_string();
            // Sanity check: completion should be short and not contain newlines
            if trimmed.len() <= 200 && !trimmed.contains('\n') {
                Some(trimmed)
            } else {
                None
            }
        }
        Err(e) => {
            log::debug!("Suggestion completion failed: {}", e);
            None
        }
    }
}
