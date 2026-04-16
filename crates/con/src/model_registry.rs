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
        ProviderKind::ChatGPT => &[
            "o4-mini",
            "o3",
            "o3-pro",
            "o3-mini",
            "gpt-4o",
            "gpt-4o-mini",
            "gpt-4.1",
            "gpt-4.1-mini",
            "gpt-4.1-nano",
            "gpt-5.4",
            "gpt-5.4-pro",
            "gpt-5.3-codex",
            "gpt-5.3-chat-latest",
            "gpt-5.3-instant",
        ],
        ProviderKind::GitHubCopilot => &[
            "gpt-5.4",
            "gpt-5.3-codex",
            "gpt-5.2",
            "claude-sonnet-4.5",
            "gpt-4o",
        ],
        ProviderKind::MiniMax | ProviderKind::MiniMaxAnthropic => {
            &["MiniMax-M2", "MiniMax-M2.1", "MiniMax-M2.5", "MiniMax-M2.7"]
        }
        ProviderKind::Moonshot => &[
            "kimi-for-coding",
            "kimi-k2.5",
            "kimi-k2",
            "moonshot-v1-128k",
        ],
        ProviderKind::MoonshotAnthropic => &["kimi-k2.5", "kimi-k2", "moonshot-v1-128k"],
        ProviderKind::ZAI | ProviderKind::ZAIAnthropic => {
            &["glm-4.6", "glm-4.6-air", "glm-4.5", "glm-4.5v"]
        }
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

fn pinned_models(provider: &ProviderKind) -> &'static [&'static str] {
    match provider {
        ProviderKind::Moonshot => &["kimi-for-coding"],
        _ => &[],
    }
}

fn append_missing_models(models: &mut Vec<String>, extra: &[&str]) {
    for model in extra {
        if !models.iter().any(|existing| existing == model) {
            models.push((*model).to_string());
        }
    }
}

/// Maps settings/runtime provider variants onto the canonical models.dev family.
fn canonical_models_provider(provider: &ProviderKind) -> ProviderKind {
    match provider {
        ProviderKind::MiniMaxAnthropic => ProviderKind::MiniMax,
        ProviderKind::MoonshotAnthropic => ProviderKind::Moonshot,
        ProviderKind::ZAIAnthropic => ProviderKind::ZAI,
        _ => provider.clone(),
    }
}

/// Maps models.dev provider IDs onto one or more Con provider families.
fn models_dev_id_to_providers(id: &str) -> &'static [ProviderKind] {
    match id {
        "anthropic" => &[ProviderKind::Anthropic],
        "openai" => &[ProviderKind::OpenAI, ProviderKind::ChatGPT],
        "chatgpt" => &[ProviderKind::ChatGPT],
        "github-copilot" => &[ProviderKind::GitHubCopilot],
        "minimax" => &[ProviderKind::MiniMax, ProviderKind::MiniMaxAnthropic],
        "moonshot" | "moonshotai" | "moonshotai-cn" => {
            &[ProviderKind::Moonshot, ProviderKind::MoonshotAnthropic]
        }
        "kimi-for-coding" => &[ProviderKind::Moonshot],
        "z-ai" | "zai" | "zai-coding-plan" => &[ProviderKind::ZAI, ProviderKind::ZAIAnthropic],
        "deepseek" => &[ProviderKind::DeepSeek],
        "groq" => &[ProviderKind::Groq],
        "google" => &[ProviderKind::Gemini],
        "cohere" => &[ProviderKind::Cohere],
        "ollama" | "ollama-cloud" => &[ProviderKind::Ollama],
        "openrouter" => &[ProviderKind::OpenRouter],
        "perplexity" => &[ProviderKind::Perplexity],
        "mistral" => &[ProviderKind::Mistral],
        "togetherai" => &[ProviderKind::Together],
        "xai" => &[ProviderKind::XAI],
        _ => &[],
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
        let canonical = canonical_models_provider(provider);
        let guard = self.inner.lock().unwrap();
        if let Some(entry) = guard.as_ref() {
            if let Some(models) = entry.models.get(&canonical) {
                if !models.is_empty() {
                    let mut models = models.clone();
                    append_missing_models(&mut models, pinned_models(provider));
                    return models;
                }
            }
        }
        // Fallback
        let mut models: Vec<String> = fallback_models(provider)
            .iter()
            .map(|s| s.to_string())
            .collect();
        append_missing_models(&mut models, pinned_models(provider));
        models
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

        let mut merged_models: HashMap<ProviderKind, HashMap<String, (bool, u64)>> = HashMap::new();

        for (provider_id, provider_data) in &resp {
            let targets = models_dev_id_to_providers(provider_id);
            if targets.is_empty() {
                continue;
            }

            let Some(models) = &provider_data.models else {
                continue;
            };

            for model in models.values() {
                let ctx = model
                    .limit
                    .as_ref()
                    .and_then(|limit| limit.context)
                    .unwrap_or(0);
                for target in targets {
                    let entry = merged_models.entry(target.clone()).or_default();
                    entry
                        .entry(model.id.clone())
                        .and_modify(|existing| {
                            existing.0 |= model.tool_call;
                            existing.1 = existing.1.max(ctx);
                        })
                        .or_insert((model.tool_call, ctx));
                }
            }
        }

        let models_map: HashMap<ProviderKind, Vec<String>> = merged_models
            .into_iter()
            .map(|(kind, models)| {
                let mut model_list: Vec<(String, bool, u64)> = models
                    .into_iter()
                    .map(|(id, (tool_call, ctx))| (id, tool_call, ctx))
                    .collect();
                model_list.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| b.2.cmp(&a.2)));
                let ids = model_list.into_iter().map(|(id, _, _)| id).collect();
                (kind, ids)
            })
            .collect();

        let entry = CacheEntry {
            models: models_map,
            fetched_at: Instant::now(),
        };

        *self.inner.lock().unwrap() = Some(entry);
        log::info!("Model registry updated from models.dev");
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use con_agent::ProviderKind;

    #[test]
    fn anthropic_variants_share_canonical_models_family() {
        assert_eq!(
            canonical_models_provider(&ProviderKind::MiniMaxAnthropic),
            ProviderKind::MiniMax
        );
        assert_eq!(
            canonical_models_provider(&ProviderKind::MoonshotAnthropic),
            ProviderKind::Moonshot
        );
        assert_eq!(
            canonical_models_provider(&ProviderKind::ZAIAnthropic),
            ProviderKind::ZAI
        );
    }

    #[test]
    fn models_dev_aliases_cover_live_provider_ids() {
        assert_eq!(
            models_dev_id_to_providers("openai"),
            &[ProviderKind::OpenAI, ProviderKind::ChatGPT]
        );
        assert_eq!(
            models_dev_id_to_providers("minimax"),
            &[ProviderKind::MiniMax, ProviderKind::MiniMaxAnthropic]
        );
        assert_eq!(
            models_dev_id_to_providers("moonshotai"),
            &[ProviderKind::Moonshot, ProviderKind::MoonshotAnthropic]
        );
        assert_eq!(
            models_dev_id_to_providers("kimi-for-coding"),
            &[ProviderKind::Moonshot]
        );
        assert_eq!(
            models_dev_id_to_providers("zai-coding-plan"),
            &[ProviderKind::ZAI, ProviderKind::ZAIAnthropic]
        );
    }

    #[test]
    fn pinned_models_are_merged_into_live_cache() {
        let registry = ModelRegistry::new();
        let mut models = HashMap::new();
        models.insert(ProviderKind::Moonshot, vec!["kimi-k2.5".to_string()]);
        *registry.inner.lock().unwrap() = Some(CacheEntry {
            models,
            fetched_at: Instant::now(),
        });

        assert_eq!(
            registry.models_for(&ProviderKind::Moonshot),
            vec!["kimi-k2.5".to_string(), "kimi-for-coding".to_string()]
        );
    }
}
