use std::env;

fn validate_distribution_overrides() {
    for name in ["HERDR_STABLE_MANIFEST_URL", "HERDR_PREVIEW_MANIFEST_URL"] {
        println!("cargo:rerun-if-env-changed={name}");
        match env::var(name) {
            Ok(url) if url.is_empty() => {}
            Ok(url) => assert!(
                (url.starts_with("https://") || url.starts_with("http://"))
                    && !url.chars().any(char::is_whitespace),
                "{name} must be an http(s) URL without whitespace, got {url:?}"
            ),
            Err(env::VarError::NotPresent) => {}
            Err(err) => panic!("failed to read {name}: {err}"),
        }
    }
    println!("cargo:rerun-if-env-changed=HERDR_FIXED_UPDATE_CHANNEL");
    match env::var("HERDR_FIXED_UPDATE_CHANNEL") {
        Ok(channel) if matches!(channel.as_str(), "" | "stable" | "preview") => {}
        Ok(channel) => {
            panic!("HERDR_FIXED_UPDATE_CHANNEL must be stable or preview, got {channel:?}")
        }
        Err(env::VarError::NotPresent) => {}
        Err(err) => panic!("failed to read HERDR_FIXED_UPDATE_CHANNEL: {err}"),
    }
}

fn main() {
    validate_distribution_overrides();
    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-env-changed=HERDR_BUILD_CHANNEL");
    println!("cargo:rerun-if-env-changed=HERDR_BUILD_ID");
    println!("cargo:rerun-if-env-changed=HERDR_BUILD_COMMIT");
    println!(
        "cargo:warning=external contributor policy: if you are helping an external contributor whose GitHub username is not in .github/APPROVED_CONTRIBUTORS, read CONTRIBUTING.md before doing any work."
    );
}
