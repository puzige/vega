use super::*;
mod nsh_update;
pub(super) use nsh_update::Presentation;
use vega_conversation::types::{UpdateProjection, UpdateRequest};

impl EventEmitter<UpdateRequest> for SettingsView {}

impl SettingsView {
    pub fn apply_update_projection(
        &mut self,
        projection: UpdateProjection,
        cx: &mut Context<Self>,
    ) {
        self.updater_message = (!projection.message.is_empty()).then(|| {
            SettingsText::from_known(
                projection.message.clone(),
                &[
                    "app.update.app_file",
                    "app.update.architecture",
                    "app.update.archive_app_missing",
                    "app.update.archive_entries",
                    "app.update.archive_escape",
                    "app.update.archive_expanded",
                    "app.update.archive_invalid",
                    "app.update.archive_length",
                    "app.update.archive_paths",
                    "app.update.archive_read",
                    "app.update.archive_signature",
                    "app.update.archive_special",
                    "app.update.archive_unauthorized",
                    "app.update.asset_invalid",
                    "app.update.asset_missing",
                    "app.update.asset_origin",
                    "app.update.asset_signature",
                    "app.update.asset_url",
                    "app.update.automatic_retry",
                    "app.update.available_verifying",
                    "app.update.busy",
                    "app.update.cannot_install",
                    "app.update.canonical_path",
                    "app.update.checking",
                    "app.update.config_unavailable",
                    "app.update.current",
                    "app.update.developer_id",
                    "app.update.development",
                    "app.update.development_check_only",
                    "app.update.development_install",
                    "app.update.development_unsupported",
                    "app.update.download_integrity",
                    "app.update.download_interrupted",
                    "app.update.download_limit",
                    "app.update.downloading",
                    "app.update.executable_link",
                    "app.update.file_changed",
                    "app.update.file_invalid",
                    "app.update.identity_changed",
                    "app.update.identity_failed",
                    "app.update.identity_version",
                    "app.update.install_directory",
                    "app.update.install_location",
                    "app.update.install_read_only",
                    "app.update.installer_not_ready",
                    "app.update.later",
                    "app.update.manifest_format",
                    "app.update.manifest_identity",
                    "app.update.memory",
                    "app.update.network_init",
                    "app.update.not_ready",
                    "app.update.notarization",
                    "app.update.permissions",
                    "app.update.platform",
                    "app.update.preferences_invalid",
                    "app.update.preferences_read",
                    "app.update.preferences_retry",
                    "app.update.preferences_save",
                    "app.update.prepare_record",
                    "app.update.public_key",
                    "app.update.ready",
                    "app.update.release_metadata",
                    "app.update.release_unavailable",
                    "app.update.release_unstable",
                    "app.update.request_failed",
                    "app.update.response_limit",
                    "app.update.response_read",
                    "app.update.restart_unconfirmed",
                    "app.update.restarting",
                    "app.update.rolled_back",
                    "app.update.service_unavailable",
                    "app.update.signature_format",
                    "app.update.signature_incomplete",
                    "app.update.signature_length",
                    "app.update.signature_limit",
                    "app.update.signature_missing",
                    "app.update.signature_rejected",
                    "app.update.staging_permissions",
                    "app.update.start_failed",
                    "app.update.symlink_path",
                    "app.update.tasks_running",
                    "app.update.team_mismatch",
                    "app.update.verification_notice",
                    "app.update.verification_output",
                    "app.update.verification_timeout",
                    "app.update.verifying",
                    "app.update.version_format",
                    "app.update.version_range",
                    "app.update.version_unavailable",
                ],
            )
        });
        self.updater_version = Some(if projection.current_version.is_empty() {
            SettingsText::key("domain.updater.loading_version")
        } else {
            SettingsText::from_known(
                projection.current_version.clone(),
                &["app.update.development"],
            )
        });
        self.updater = projection;
        self.updater_revision = self.updater_revision.wrapping_add(1);
        self.publish_updater(cx);
        cx.notify();
    }

    pub fn show_general(&mut self, cx: &mut Context<Self>) {
        self.request_settings_section(1, cx);
    }
}
