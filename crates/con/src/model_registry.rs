//! Live model registry fetched from models.dev.
//!
//! Provides up-to-date model lists per provider by fetching from the
//! models.dev community API. Falls back to hardcoded defaults when
//! the network is unavailable.

use con_agent::ProviderKind;
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

const API_URL: &str = "https://models.dev/api.json";
const CACHE_TTL: Duration = Duration::from_secs(3600); // 1 hour

// ── Fallback model lists (used when API is unreachable) ──────────────

fn fallback_models(provider: &ProviderKind) -> &'static [&'static str] {
    match provider {
        ProviderKind::Anthropic => &[
            "claude-opus-4-6",
            "claude-sonnet-4-6",
            "claude-opus-4-5",
            "claude-sonnet-4-5",
            "claude-haiku-4-5",
        ],
        ProviderKind::OpenAI => &[
            "o4-mini",
            "o3",
            "o3-pro",
            "o3-mini",
            "gpt-4o",
            "gpt-4o-mini",
            "gpt-4.1",
            "gpt-4.1-mini",
            "gpt-4.1-nano",
        ],
        ProviderKind::DeepSeek => &["deepseek-chat", "deepseek-reasoner"],
        ProviderKind::Groq => &[
            "llama-3.3-70b-versatile",
            "llama-3.1-8b-instant",
            "deepseek-r1-distill-llama-70b",
            "qwen-qwq-32b",
        ],
        ProviderKind::Gemini => &[
            "gemini-2.5-pro",
            "gemini-2.5-flash",
            "gemini-2.5-flash-lite",
            "gemini-2.0-flash",
        ],
        ProviderKind::Mistral => &[
            "mistral-large-latest",
            "mistral-medium-latest",
            "mistral-small-latest",
            "codestral-latest",
        ],
        ProviderKind::Cohere => &[
            "command-a-03-2025",
            "command-r-plus-08-2024",
            "command-r-08-2024",
        ],
        ProviderKind::Perplexity => &["sonar-pro", "sonar-reasoning-pro", "sonar"],
        ProviderKind::XAI => &["grok-4", "grok-3", "grok-3-mini", "grok-2"],
        ProviderKind::Ollama => &[
            "llama3.2",
            "llama3.1",
            "qwen2.5-coder",
            "deepseek-v3",
            "mistral",
            "gemma3",
        ],
        _ => &[],
    }
}

/// Maps models.dev provider IDs to our `ProviderKind`.
fn models_dev_id_to_provider(id: &str) -> Option<ProviderKind> {
    match id {
        "anthropic" => Some(ProviderKind::Anthropic),
        "openai" => Some(ProviderKind::OpenAI),
        "deepseek" => Some(ProviderKind::DeepSeek),
        "groq" => Some(ProviderKind::Groq),
        "google" => Some(ProviderKind::Gemini),
        "cohere" => Some(ProviderKind::Cohere),
        "ollama" | "ollama-cloud" => Some(ProviderKind::Ollama),
        "openrouter" => Some(ProviderKind::OpenRouter),
        "perplexity" => Some(ProviderKind::Perplexity),
        "mistral" => Some(ProviderKind::Mistral),
        "togetherai" => Some(ProviderKind::Together),
        "xai" => Some(ProviderKind::XAI),
        _ => None,
    }
}

// ── API response types ───────────────────────────────────────────────

#[derive(serde::Deserialize)]
struct ApiProvider {
    models: Option<HashMap<String, ApiModel>>,
}

#[derive(serde::Deserialize)]
struct ApiModel {
    id: String,
    #[serde(default)]
    tool_call: bool,
    limit: Option<ApiLimit>,
}

#[derive(serde::Deserialize)]
struct ApiLimit {
    context: Option<u64>,
}

// ── Registry ─────────────────────────────────────────────────────────

#[derive(Clone)]
struct CacheEntry {
    models: HashMap<ProviderKind, Vec<String>>,
    fetched_at: Instant,
}

/// Shared, thread-safe model registry with background fetching.
#[derive(Clone)]
pub struct ModelRegistry {
    inner: Arc<Mutex<Option<CacheEntry>>>,
}

impl ModelRegistry {
    pub fn new() -> Self {
        Self {
            inner: Arc::new(Mutex::new(None)),
        }
    }

    /// Returns the cached model list for a provider.
    /// Falls back to hardcoded defaults if the cache is empty.
    pub fn models_for(&self, provider: &ProviderKind) -> Vec<String> {
        let guard = self.inner.lock().unwrap();
        if let Some(entry) = guard.as_ref() {
            if let Some(models) = entry.models.get(provider) {
                if !models.is_empty() {
                    return models.clone();
                }
            }
        }
        // Fallback
        fallback_models(provider)
            .iter()
            .map(|s| s.to_string())
            .collect()
    }

    /// Whether the cache is stale or empty and needs a refresh.
    pub fn needs_refresh(&self) -> bool {
        let guard = self.inner.lock().unwrap();
        match guard.as_ref() {
            None => true,
            Some(entry) => entry.fetched_at.elapsed() > CACHE_TTL,
        }
    }

    /// Fetch model data from models.dev and update the cache.
    /// Intended to be called from a background task.
    pub async fn fetch(&self) -> anyhow::Result<()> {
        log::info!("Fetching model registry from {API_URL}");

        let client = reqwest::Client::builder()
            .timeout(Duration::from_secs(15))
            .build()?;

        let resp: HashMap<String, ApiProvider> = client.get(API_URL).send().await?.json().await?;

        let mut models_map: HashMap<ProviderKind, Vec<String>> = HashMap::new();

        for (provider_id, provider_data) in &resp {
            let Some(kind) = models_dev_id_to_provider(provider_id) else {
                continue;
            };

            let Some(models) = &provider_data.models else {
                continue;
            };

            // Collect model IDs, preferring models with tool_call support
            // and larger context windows first.
            let mut model_list: Vec<(String, bool, u64)> = models
                .values()
                .map(|m| {
                    let ctx = m.limit.as_ref().and_then(|l| l.context).unwrap_or(0);
                    (m.id.clone(), m.tool_call, ctx)
                })
                .collect();

            // Sort: tool_call capable first, then by context window descending
            model_list.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| b.2.cmp(&a.2)));

            let ids: Vec<String> = model_list.into_iter().map(|(id, _, _)| id).collect();
            models_map.insert(kind, ids);
        }

        let entry = CacheEntry {
            models: models_map,
            fetched_at: Instant::now(),
        };

        *self.inner.lock().unwrap() = Some(entry);
        log::info!("Model registry updated from models.dev");
        Ok(())
    }
}
