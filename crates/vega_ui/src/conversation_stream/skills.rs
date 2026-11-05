//! Composer Skills are presentation and user intent only. Catalog scans,
//! consent CAS and root reads happen in the app/controller worker.

use super::*;
use vega_conversation::types::{SkillComposerCandidate, SkillComposerMutation};

impl ConversationStream {
    pub(crate) fn clear_skill_route_state(&mut self) {
        self.skill_projection_generation = self.skill_projection_generation.wrapping_add(1);
        self.skill_projection = None;
        self.skill_mutation_pending = false;
        self.actions.skill_projection_requested = false;
    }

    /// Settings may change consent while a catalog/mutation worker is still
    /// running. Retire its presentation owner; keep a draft's user intent so
    /// first send must still revalidate instead of silently omitting it.
    pub fn suspend_skill_projection(&mut self, cx: &mut Context<Self>) {
        self.skill_projection_generation = self.skill_projection_generation.wrapping_add(1);
        self.skill_projection = None;
        self.skill_mutation_pending = false;
        self.actions.skill_projection_requested = false;
        cx.notify();
    }

    /// Route-fenced read request. A draft has no Store row and no write is
    /// initiated by opening this picker.
    pub fn request_skill_projection(&mut self, cx: &mut Context<Self>) {
        self.skill_projection_generation = self.skill_projection_generation.wrapping_add(1);
        cx.emit(SkillComposerProjectionRequested {
            thread_id: self.thread.id.clone(),
            project_id: self.thread.project_id.clone(),
            draft: self.draft_route,
            generation: self.skill_projection_generation,
        });
        cx.notify();
    }

    pub fn skill_projection_generation(&self) -> u64 {
        self.skill_projection_generation
    }

    pub fn apply_skill_projection(
        &mut self,
        generation: u64,
        result: Result<SkillComposerProjection, &'static str>,
        cx: &mut Context<Self>,
    ) {
        if generation != self.skill_projection_generation {
            return;
        }
        self.actions.menu_generation = self.actions.menu_generation.wrapping_add(1);
        self.actions.skill_projection_requested = false;
        match result {
            Ok(projection) if projection.thread_id == self.thread.id => {
                self.skill_projection = Some(projection);
            }
            _ => {
                self.skill_projection = None;
                self.controller_runtime_error = None;
                self.controller_error = Some(crate::localized_copy::LocalizedCopy::new(
                    "ui.skill_sources_changed_refresh_and_try_again",
                ));
            }
        }
        cx.notify();
    }

    pub fn choose_skill_candidate(
        &mut self,
        candidate: &SkillComposerCandidate,
        cx: &mut Context<Self>,
    ) {
        if self.skill_mutation_pending
            || self.composer_submit_pending
            || self.input.read(cx).is_composing()
        {
            return;
        }
        let Some(projection) = self.skill_projection.as_ref() else {
            return;
        };
        let expected_generation = projection.consent_generation;
        if !projection.candidates.iter().any(|item| {
            item.source_id == candidate.source_id
                && item.name == candidate.name
                && item.content_sha256 == candidate.content_sha256
                && item.target_binding == candidate.target_binding
        }) {
            return;
        }
        let Some((range, _)) = self.skill_mention_query(cx) else {
            return;
        };
        let intent = SkillSelectionIntent {
            source_id: candidate.source_id.clone(),
            name: candidate.name.clone(),
            content_sha256: candidate.content_sha256.clone(),
            target_binding: candidate.target_binding.clone(),
            expected_consent_generation: expected_generation,
        };
        let text = format!("${} [{}]", candidate.name, candidate.source_label);
        self.close_composer_popovers(cx);
        self.input.update(cx, |input, cx| {
            input.insert_skill_mention(range, &text, intent, cx)
        });
        cx.notify();
    }

    pub fn remove_skill_pin(&mut self, name: String, cx: &mut Context<Self>) {
        if self.skill_mutation_pending || self.draft_route {
            return;
        }
        let Some(projection) = self.skill_projection.as_ref() else {
            return;
        };
        if !projection.pins.iter().any(|pin| pin.name == name) {
            return;
        }
        self.skill_mutation_pending = true;
        cx.emit(SkillComposerMutationRequested {
            thread_id: self.thread.id.clone(),
            project_id: self.thread.project_id.clone(),
            owner_generation: self.skill_projection_generation,
            expected_generation: projection.consent_generation,
            mutation: SkillComposerMutation::Unpin { name },
        });
        cx.notify();
    }

    pub fn finish_skill_mutation(&mut self, success: bool, cx: &mut Context<Self>) {
        self.skill_mutation_pending = false;
        if success {
            self.controller_runtime_error = None;
            self.controller_error = None;
            self.request_skill_projection(cx);
        } else {
            self.controller_runtime_error = None;
            self.controller_error = Some(crate::localized_copy::LocalizedCopy::new(
                "ui.skill_permissions_or_files_changed_refresh_and_review",
            ));
            cx.notify();
        }
    }

    pub fn finish_submitted_skill_pin(&mut self, success: bool, cx: &mut Context<Self>) {
        if success {
            self.controller_runtime_error = None;
            self.controller_error = None;
            self.request_skill_projection(cx);
        } else {
            self.reject_composer_submission(cx);
            self.controller_runtime_error = None;
            self.controller_error = Some(crate::localized_copy::LocalizedCopy::new(
                "ui.skill_permissions_or_files_changed_refresh_and_review_en",
            ));
            cx.notify();
        }
    }
}
