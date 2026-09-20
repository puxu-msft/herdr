use crate::api::schema::{
    IntegrationInfo, IntegrationInstallResult, IntegrationState, IntegrationUninstallResult,
    PluginIntegrationOperationParams, ResponseResult,
};
use crate::app::App;

use super::responses::{encode_error, encode_success};

impl App {
    pub(super) fn handle_integration_list(&self, id: String) -> String {
        let integrations = crate::integration::integration_recommendations()
            .into_iter()
            .map(|recommendation| IntegrationInfo {
                target: recommendation.target,
                label: recommendation.label.to_owned(),
                command: recommendation.command.to_owned(),
                available: recommendation.available,
                state: match recommendation.state {
                    crate::integration::IntegrationStatusKind::NotInstalled => {
                        IntegrationState::NotInstalled
                    }
                    crate::integration::IntegrationStatusKind::Current => IntegrationState::Current,
                    crate::integration::IntegrationStatusKind::Outdated => {
                        IntegrationState::Outdated
                    }
                },
            })
            .collect();
        encode_success(id, ResponseResult::IntegrationList { integrations })
    }

    pub(super) fn handle_integration_provider_list(&self, id: String) -> String {
        encode_success(
            id,
            ResponseResult::IntegrationProviderList {
                integrations: crate::integration::plugin_integration_infos(),
            },
        )
    }

    pub(super) fn handle_integration_provider_install(
        &mut self,
        id: String,
        params: PluginIntegrationOperationParams,
    ) -> String {
        let provider_id = params.provider_id;
        let messages = match crate::integration::run_plugin_integration_operation(
            &provider_id,
            crate::integration::PluginIntegrationOperation::Install,
        ) {
            Ok(messages) => messages,
            Err(err) => {
                return encode_error(id, "integration_provider_install_failed", err.to_string());
            }
        };
        encode_success(
            id,
            ResponseResult::IntegrationProviderInstall {
                provider_id,
                details: IntegrationInstallResult { messages },
            },
        )
    }

    pub(super) fn handle_integration_provider_uninstall(
        &mut self,
        id: String,
        params: PluginIntegrationOperationParams,
    ) -> String {
        let provider_id = params.provider_id;
        let messages = match crate::integration::run_plugin_integration_operation(
            &provider_id,
            crate::integration::PluginIntegrationOperation::Uninstall,
        ) {
            Ok(messages) => messages,
            Err(err) => {
                return encode_error(id, "integration_provider_uninstall_failed", err.to_string());
            }
        };
        encode_success(
            id,
            ResponseResult::IntegrationProviderUninstall {
                provider_id,
                details: IntegrationUninstallResult { messages },
            },
        )
    }

    pub(super) fn handle_integration_install(
        &mut self,
        id: String,
        params: crate::api::schema::IntegrationInstallParams,
    ) -> String {
        let target = params.target;
        let messages = match crate::integration::install_target(target) {
            Ok(messages) => messages,
            Err(err) => return encode_error(id, "integration_install_failed", err.to_string()),
        };
        self.state.integration_recommendations = crate::integration::integration_recommendations();

        encode_success(
            id,
            ResponseResult::IntegrationInstall {
                target,
                details: IntegrationInstallResult { messages },
            },
        )
    }

    pub(super) fn handle_integration_uninstall(
        &mut self,
        id: String,
        params: crate::api::schema::IntegrationUninstallParams,
    ) -> String {
        let target = params.target;
        let messages = match crate::integration::uninstall_target(target) {
            Ok(messages) => messages,
            Err(err) => return encode_error(id, "integration_uninstall_failed", err.to_string()),
        };
        self.state.integration_recommendations = crate::integration::integration_recommendations();

        encode_success(
            id,
            ResponseResult::IntegrationUninstall {
                target,
                details: IntegrationUninstallResult { messages },
            },
        )
    }
}
