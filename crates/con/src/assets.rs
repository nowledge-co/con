use gpui::{AssetSource, Result, SharedString};
use std::borrow::Cow;

/// Embeds con's own icons (Phosphor) from `assets/icons/`.
#[derive(rust_embed::RustEmbed)]
#[folder = "../../assets/icons"]
#[include = "**/*.svg"]
struct ConIcons;

/// Asset source that serves con's icons first, then falls back to
/// gpui-component's bundled icons (Lucide).
pub struct ConAssets;

impl AssetSource for ConAssets {
    fn load(&self, path: &str) -> Result<Option<Cow<'static, [u8]>>> {
        if path.is_empty() {
            return Ok(None);
        }

        // Try con's own icons first
        if let Some(data) = ConIcons::get(path) {
            return Ok(Some(data.data));
        }

        // Fall back to gpui-component's bundled assets
        gpui_component_assets::Assets.load(path)
    }

    fn list(&self, path: &str) -> Result<Vec<SharedString>> {
        let mut results: Vec<SharedString> = ConIcons::iter()
            .filter(|p| p.starts_with(path))
            .map(|p| p.into())
            .collect();

        if let Ok(mut component_results) = gpui_component_assets::Assets.list(path) {
            results.append(&mut component_results);
        }

        Ok(results)
    }
}
