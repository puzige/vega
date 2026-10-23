use super::*;

impl ArtifactService {
    pub fn in_memory_ui_fixture(
        text: &str,
    ) -> Result<(ArtifactCard, ArtifactPreviewProjection), GitWorkspaceError> {
        if text.len() > PREVIEW_BYTES
            || text.lines().count() > PREVIEW_LINES
            || text.lines().any(|line| line.len() > PREVIEW_LINE_BYTES)
        {
            return Err(workspace_error(GitWorkspaceErrorCode::ArtifactLimit));
        }
        let nonce = ARTIFACT_SERVICE_NONCE
            .fetch_update(Ordering::Relaxed, Ordering::Relaxed, |value| {
                value.checked_add(1)
            })
            .map_err(|_| workspace_error(GitWorkspaceErrorCode::ArtifactLimit))?;
        let route_epoch = nonce;
        let slot = 0;
        let id = ArtifactCardId {
            route_epoch,
            slot,
            seal: card_seal(nonce, route_epoch, slot),
        };
        let file_id = WorkspaceFileId {
            generation: nonce,
            slot,
            seal: card_seal(nonce, route_epoch.rotate_left(1), slot),
        };
        let record = ArtifactRecord {
            id,
            fingerprint: TerminalFingerprint::Write {
                path: "memory-disclosure.txt".into(),
                input_fingerprint: "memory-only-ui-fixture".into(),
                bytes_written: text.len() as u64,
            },
            path: OsString::from("memory-disclosure.txt"),
            label: "memory-disclosure.txt".into(),
            source: ArtifactSource::WorkspaceChange,
            evidence: None,
            current_file_id: Some(file_id),
            stale_disabled: false,
        };
        Ok((
            record.projection(),
            ArtifactPreviewProjection {
                card_id: id,
                file_id,
                text: text.into(),
            },
        ))
    }
}
