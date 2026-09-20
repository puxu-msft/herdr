use std::io;
use std::path::PathBuf;

use serde::Deserialize;

use crate::api::schema::{
    InstalledPluginInfo, IntegrationState, PluginIntegrationInfo, PluginManifestIntegration,
    PluginPlatform,
};

use super::IntegrationStatusKind;

#[derive(Clone)]
struct PluginIntegrationProvider {
    plugin: InstalledPluginInfo,
    integration: PluginManifestIntegration,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "snake_case")]
enum StateFileState {
    NotInstalled,
    Current,
    Outdated,
}

#[derive(Debug, Deserialize)]
struct StateFile {
    state: StateFileState,
    #[serde(default)]
    available: Option<bool>,
    #[serde(default)]
    message: Option<String>,
}

pub(crate) fn plugin_integration_infos() -> Vec<PluginIntegrationInfo> {
    plugin_integration_infos_from_plugins(&load_registered_plugins())
}

pub(crate) fn plugin_integration_infos_from_plugins(
    plugins: &[InstalledPluginInfo],
) -> Vec<PluginIntegrationInfo> {
    let mut infos = providers_from_plugins(plugins)
        .into_iter()
        .map(|provider| provider_info(&provider))
        .collect::<Vec<_>>();
    infos.sort_by(|left, right| left.provider_id.cmp(&right.provider_id));
    infos
}

pub(crate) fn run_plugin_integration_operation(
    provider_id: &str,
    operation: PluginIntegrationOperation,
) -> io::Result<Vec<String>> {
    let provider = providers_from_plugins(&load_registered_plugins())
        .into_iter()
        .find(|provider| provider_id_for(provider) == provider_id)
        .ok_or_else(|| {
            io::Error::new(io::ErrorKind::NotFound, unknown_provider_error(provider_id))
        })?;
    let command = match operation {
        PluginIntegrationOperation::Install => &provider.integration.install,
        PluginIntegrationOperation::Uninstall => &provider.integration.uninstall,
    };
    if command.is_empty() {
        return Err(io::Error::new(
            io::ErrorKind::Unsupported,
            format!(
                "plugin integration provider '{provider_id}' does not support {}",
                operation.as_str()
            ),
        ));
    }

    crate::plugin_paths::ensure_plugin_user_dirs(&provider.plugin.plugin_id)?;
    let (program, args) = command
        .split_first()
        .ok_or_else(|| io::Error::other("plugin integration command must not be empty"))?;
    let mut command = crate::plugin_command::command_for_argv_in_dir(
        program,
        args,
        &PathBuf::from(&provider.plugin.plugin_root),
    );
    command.envs(provider_command_environment(&provider, operation));
    let status = command.status()?;
    if !status.success() {
        return Err(io::Error::other(format!(
            "plugin integration provider '{provider_id}' {} command exited with {status}",
            operation.as_str()
        )));
    }

    Ok(vec![format!(
        "{} {} completed",
        provider.integration.label,
        operation.as_str()
    )])
}

#[derive(Debug, Clone, Copy)]
pub(crate) enum PluginIntegrationOperation {
    Install,
    Uninstall,
}

impl PluginIntegrationOperation {
    fn as_str(self) -> &'static str {
        match self {
            Self::Install => "install",
            Self::Uninstall => "uninstall",
        }
    }
}

fn load_registered_plugins() -> Vec<InstalledPluginInfo> {
    let entries = crate::persist::plugin_registry::load();
    crate::persist::plugin_registry::reload_manifests(entries, |path, enabled| {
        crate::app::load_plugin_manifest(path, enabled).map_err(|(_, message)| message)
    })
}

fn providers_from_plugins(plugins: &[InstalledPluginInfo]) -> Vec<PluginIntegrationProvider> {
    plugins
        .iter()
        .filter(|plugin| {
            plugin.enabled
                && !plugin.warnings.iter().any(|warning| {
                    warning.starts_with(
                        crate::persist::plugin_registry::MANIFEST_UNAVAILABLE_WARNING_PREFIX,
                    )
                })
        })
        .flat_map(|plugin| {
            plugin
                .integrations
                .iter()
                .filter(|integration| provider_platform_supported(plugin, integration))
                .cloned()
                .map(|integration| PluginIntegrationProvider {
                    plugin: plugin.clone(),
                    integration,
                })
        })
        .collect()
}

fn provider_info(provider: &PluginIntegrationProvider) -> PluginIntegrationInfo {
    let (state, available, message) = provider_status(provider);
    PluginIntegrationInfo {
        provider_id: provider_id_for(provider),
        plugin_id: provider.plugin.plugin_id.clone(),
        integration_id: provider.integration.id.clone(),
        label: provider.integration.label.clone(),
        available,
        state: state_to_schema(state),
        status_file: provider_status_file(provider).display().to_string(),
        message,
        supports_install: !provider.integration.install.is_empty(),
        supports_uninstall: !provider.integration.uninstall.is_empty(),
    }
}

fn provider_status(
    provider: &PluginIntegrationProvider,
) -> (IntegrationStatusKind, bool, Option<String>) {
    let status_file = provider_status_file(provider);
    let content = match std::fs::read_to_string(&status_file) {
        Ok(content) => content,
        Err(err) if err.kind() == io::ErrorKind::NotFound => {
            return (
                IntegrationStatusKind::NotInstalled,
                provider.integration.available,
                None,
            );
        }
        Err(err) => {
            return (
                IntegrationStatusKind::Outdated,
                provider.integration.available,
                Some(format!("could not read {}: {err}", status_file.display())),
            );
        }
    };
    let parsed: StateFile = match serde_json::from_str(&content) {
        Ok(parsed) => parsed,
        Err(err) => {
            return (
                IntegrationStatusKind::Outdated,
                provider.integration.available,
                Some(format!(
                    "invalid status file {}: {err}",
                    status_file.display()
                )),
            );
        }
    };
    state_from_file(parsed, provider.integration.available)
}

fn state_from_file(
    parsed: StateFile,
    default_available: bool,
) -> (IntegrationStatusKind, bool, Option<String>) {
    let state = match parsed.state {
        StateFileState::NotInstalled => IntegrationStatusKind::NotInstalled,
        StateFileState::Current => IntegrationStatusKind::Current,
        StateFileState::Outdated => IntegrationStatusKind::Outdated,
    };
    (
        state,
        parsed.available.unwrap_or(default_available),
        parsed.message,
    )
}

fn provider_status_file(provider: &PluginIntegrationProvider) -> PathBuf {
    crate::plugin_paths::plugin_state_dir(&provider.plugin.plugin_id)
        .join(&provider.integration.status_file)
}

fn provider_id_for(provider: &PluginIntegrationProvider) -> String {
    format!("{}.{}", provider.plugin.plugin_id, provider.integration.id)
}

fn unknown_provider_error(provider_id: &str) -> String {
    format!("unknown plugin integration provider: {provider_id}")
}

fn provider_platform_supported(
    plugin: &InstalledPluginInfo,
    integration: &PluginManifestIntegration,
) -> bool {
    let platforms = integration.platforms.as_ref().or(plugin.platforms.as_ref());
    platforms.is_none_or(|platforms| platforms.contains(&current_platform()))
}

fn current_platform() -> PluginPlatform {
    if cfg!(target_os = "linux") {
        PluginPlatform::Linux
    } else if cfg!(target_os = "macos") {
        PluginPlatform::Macos
    } else {
        PluginPlatform::Windows
    }
}

fn state_to_schema(state: IntegrationStatusKind) -> IntegrationState {
    match state {
        IntegrationStatusKind::NotInstalled => IntegrationState::NotInstalled,
        IntegrationStatusKind::Current => IntegrationState::Current,
        IntegrationStatusKind::Outdated => IntegrationState::Outdated,
    }
}

fn provider_command_environment(
    provider: &PluginIntegrationProvider,
    operation: PluginIntegrationOperation,
) -> Vec<(String, String)> {
    let mut environment = vec![
        (
            "HERDR_PLUGIN_ROOT".to_string(),
            provider.plugin.plugin_root.clone(),
        ),
        (
            "HERDR_PLUGIN_CONFIG_DIR".to_string(),
            crate::plugin_paths::plugin_config_dir(&provider.plugin.plugin_id)
                .display()
                .to_string(),
        ),
        (
            "HERDR_PLUGIN_STATE_DIR".to_string(),
            crate::plugin_paths::plugin_state_dir(&provider.plugin.plugin_id)
                .display()
                .to_string(),
        ),
        ("HERDR_ENV".to_string(), "1".to_string()),
        (
            "HERDR_PLUGIN_ID".to_string(),
            provider.plugin.plugin_id.clone(),
        ),
        (
            "HERDR_PLUGIN_INTEGRATION_ID".to_string(),
            provider.integration.id.clone(),
        ),
        (
            "HERDR_PLUGIN_INTEGRATION_PROVIDER_ID".to_string(),
            provider_id_for(provider),
        ),
        (
            "HERDR_PLUGIN_INTEGRATION_OPERATION".to_string(),
            operation.as_str().to_string(),
        ),
        (
            crate::api::SOCKET_PATH_ENV_VAR.to_string(),
            crate::api::socket_path().display().to_string(),
        ),
    ];
    if let Ok(executable) = crate::platform::launch_executable() {
        environment.push((
            "HERDR_BIN_PATH".to_string(),
            executable.display().to_string(),
        ));
    }
    environment
}

#[cfg(test)]
mod tests {
    use super::*;

    fn plugin(integrations: Vec<PluginManifestIntegration>) -> InstalledPluginInfo {
        InstalledPluginInfo {
            plugin_id: "example.manager".to_string(),
            name: "Example Manager".to_string(),
            version: "0.1.0".to_string(),
            min_herdr_version: "0.1.0".to_string(),
            description: None,
            manifest_path: "/tmp/manifest.toml".to_string(),
            plugin_root: "/tmp".to_string(),
            enabled: true,
            platforms: None,
            build: Vec::new(),
            startup: Vec::new(),
            actions: Vec::new(),
            events: Vec::new(),
            integrations,
            panes: Vec::new(),
            link_handlers: Vec::new(),
            source: Default::default(),
            warnings: Vec::new(),
        }
    }

    fn integration(id: &str) -> PluginManifestIntegration {
        PluginManifestIntegration {
            id: id.to_string(),
            label: "Managed Claude".to_string(),
            status_file: format!("integrations/{id}.json"),
            available: true,
            platforms: None,
            install: vec!["manager".to_string(), "install".to_string()],
            uninstall: vec!["manager".to_string(), "uninstall".to_string()],
        }
    }

    #[test]
    fn providers_use_qualified_ids_and_static_availability_without_a_status_file() {
        let infos = plugin_integration_infos_from_plugins(&[plugin(vec![integration("claude")])]);
        assert_eq!(infos.len(), 1);
        assert_eq!(infos[0].provider_id, "example.manager.claude");
        assert_eq!(infos[0].state, IntegrationState::NotInstalled);
        assert!(infos[0].available);
        assert!(infos[0].supports_install);
        assert!(infos[0].supports_uninstall);
    }

    #[test]
    fn disabled_plugins_do_not_register_providers() {
        let mut plugin = plugin(vec![integration("claude")]);
        plugin.enabled = false;
        assert!(plugin_integration_infos_from_plugins(&[plugin]).is_empty());
    }

    #[test]
    fn status_file_can_override_static_availability_and_explain_an_outdated_provider() {
        let parsed = serde_json::from_str::<StateFile>(
            r#"{"state":"outdated","available":false,"message":"restart the manager"}"#,
        )
        .unwrap();
        let (state, available, message) = state_from_file(parsed, true);
        assert_eq!(state, IntegrationStatusKind::Outdated);
        assert!(!available);
        assert_eq!(message.as_deref(), Some("restart the manager"));
    }
}
