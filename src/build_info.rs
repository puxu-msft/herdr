//! Build identity helpers.

pub const BASE_VERSION: &str = env!("CARGO_PKG_VERSION");

/// Update manifests read by `herdr update` and remote installs. Builds that
/// publish their own releases (for example a fork) override them at build time.
pub const STABLE_UPDATE_MANIFEST_URL: &str = manifest_url(
    option_env!("HERDR_STABLE_MANIFEST_URL"),
    "https://herdr.dev/latest.json",
);
pub const PREVIEW_UPDATE_MANIFEST_URL: &str = manifest_url(
    option_env!("HERDR_PREVIEW_MANIFEST_URL"),
    "https://herdr.dev/preview.json",
);

const fn manifest_url(value: Option<&'static str>, default: &'static str) -> &'static str {
    match value {
        Some(url) if !url.is_empty() => url,
        _ => default,
    }
}

/// Build-time fixed update channel; `build.rs` only accepts `stable` or `preview`.
/// Builds that set it ignore the configured channel, so a distribution that
/// publishes a single channel never follows another distribution's manifests.
pub fn fixed_update_channel() -> Option<&'static str> {
    non_empty(option_env!("HERDR_FIXED_UPDATE_CHANNEL"))
}

pub fn channel() -> &'static str {
    non_empty(option_env!("HERDR_BUILD_CHANNEL")).unwrap_or("stable")
}

pub fn build_id() -> Option<&'static str> {
    non_empty(option_env!("HERDR_BUILD_ID"))
}

pub fn version() -> String {
    match channel() {
        "stable" => BASE_VERSION.to_string(),
        channel => match build_id() {
            Some(build_id) => format!("{BASE_VERSION}-{channel}.{build_id}"),
            None => format!("{BASE_VERSION}-{channel}"),
        },
    }
}

pub fn is_preview() -> bool {
    channel() == "preview"
}

fn non_empty(value: Option<&'static str>) -> Option<&'static str> {
    value.and_then(|value| {
        let trimmed = value.trim();
        if trimmed.is_empty() {
            None
        } else {
            Some(trimmed)
        }
    })
}

#[cfg(test)]
mod tests {
    #[test]
    fn stable_version_defaults_to_cargo_version() {
        assert!(!super::version().is_empty());
    }
}
