//! App-owned Skills Composer bridge. All Store/filesystem work is on bounded
//! workers; the stream only projects current reviewed choices and user intent.

use super::*;
use std::sync::mpsc;

const SKILL_WORKER_POLL: std::time::Duration = std::time::Duration::from_millis(4);

impl VegaWindow {
    pub(crate) fn skill_composer_service(
        &self,
        project_id: &str,
        cx: &App,
    ) -> Option<SkillSettingsService> {
        let database = self.context_database(cx)?;
        let config_root = self.composer_config_path()?.parent()?.to_path_buf();
        let selected_project = (!project_id.is_empty()).then(|| project_id.to_owned());
        Some(SkillSettingsService::new(
            database,
            config_root,
            selected_project,
        ))
    }

    fn skill_route_is_current(
        &self,
        stream: &Entity<ConversationStream>,
        thread_id: &str,
        project_id: &str,
        cx: &App,
    ) -> bool {
        !cx.global::<SettingsOpen>().0
            && self.owns_stream_request(stream, thread_id, cx)
            && stream.read(cx).route_project_id() == project_id
    }

    pub(crate) fn request_skill_composer_projection(
        &mut self,
        stream: Entity<ConversationStream>,
        request: &SkillComposerProjectionRequested,
        cx: &mut Context<Self>,
    ) {
        if !self.skill_route_is_current(&stream, &request.thread_id, &request.project_id, cx) {
            return;
        }
        let Some(service) = self.skill_composer_service(&request.project_id, cx) else {
            stream.update(cx, |stream, cx| {
                stream.apply_skill_projection(request.generation, Err("storage_failed"), cx);
            });
            return;
        };
        let thread_id = request.thread_id.clone();
        let project_id = request.project_id.clone();
        let generation = request.generation;
        let draft = request.draft;
        let (sender, receiver) = mpsc::sync_channel(1);
        let worker_thread = thread_id.clone();
        if std::thread::Builder::new()
            .name("vega-skill-catalog".into())
            .spawn(move || {
                let result = if draft {
                    service.composer_draft_projection(&worker_thread)
                } else {
                    service.composer_projection(&worker_thread)
                };
                let _ = sender.send(result);
            })
            .is_err()
        {
            stream.update(cx, |stream, cx| {
                stream.apply_skill_projection(generation, Err("worker_failed"), cx);
            });
            return;
        }
        cx.spawn(async move |this, cx| {
            loop {
                cx.background_executor().timer(SKILL_WORKER_POLL).await;
                let result = match receiver.try_recv() {
                    Ok(result) => result.map_err(|error| error.code()),
                    Err(mpsc::TryRecvError::Empty) => continue,
                    Err(mpsc::TryRecvError::Disconnected) => Err("worker_failed"),
                };
                let _ = this.update(cx, |this, cx| {
                    if this.skill_route_is_current(&stream, &thread_id, &project_id, cx) {
                        stream.update(cx, |stream, cx| {
                            stream.apply_skill_projection(generation, result, cx);
                        });
                    }
                });
                break;
            }
        })
        .detach();
    }

    pub(crate) fn request_skill_composer_mutation(
        &mut self,
        stream: Entity<ConversationStream>,
        request: &SkillComposerMutationRequested,
        cx: &mut Context<Self>,
    ) {
        if !self.skill_route_is_current(&stream, &request.thread_id, &request.project_id, cx) {
            return;
        }
        let Some(service) = self.skill_composer_service(&request.project_id, cx) else {
            stream.update(cx, |stream, cx| stream.finish_skill_mutation(false, cx));
            return;
        };
        let thread_id = request.thread_id.clone();
        let project_id = request.project_id.clone();
        let owner_generation = request.owner_generation;
        let generation = request.expected_generation;
        let mutation = request.mutation.clone();
        let (sender, receiver) = mpsc::sync_channel(1);
        let worker_thread = thread_id.clone();
        if std::thread::Builder::new()
            .name("vega-skill-selection".into())
            .spawn(move || {
                let _ = sender.send(service.apply_composer(&worker_thread, generation, mutation));
            })
            .is_err()
        {
            stream.update(cx, |stream, cx| stream.finish_skill_mutation(false, cx));
            return;
        }
        cx.spawn(async move |this, cx| {
            loop {
                cx.background_executor().timer(SKILL_WORKER_POLL).await;
                let success = match receiver.try_recv() {
                    Ok(result) => result.is_ok(),
                    Err(mpsc::TryRecvError::Empty) => continue,
                    Err(mpsc::TryRecvError::Disconnected) => false,
                };
                let _ = this.update(cx, |this, cx| {
                    if this.skill_route_is_current(&stream, &thread_id, &project_id, cx)
                        && stream.read(cx).skill_projection_generation() == owner_generation
                    {
                        stream.update(cx, |stream, cx| {
                            stream.finish_skill_mutation(success, cx);
                        });
                    }
                });
                break;
            }
        })
        .detach();
    }
}
