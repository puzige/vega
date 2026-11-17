use super::*;
use crate::sidebar::{SelectedProject, VegaStore};
use crate::text_input::TextInput;

mod host;
mod view;

const UTILITY_CHIP_LEFT_INSET: f32 = Layout::CONTENT_PADDING
    + Layout::COMPOSER_UTILITY_BAR_INSET
    + Layout::COMPOSER_UTILITY_CHIP_INSET;

const PROJECT_MENU_VIEWPORT_MARGIN: f32 = 8.0;

fn project_menu_width(viewport: Pixels) -> Pixels {
    let room = viewport - px(UTILITY_CHIP_LEFT_INSET + PROJECT_MENU_VIEWPORT_MARGIN);
    px(Layout::MENU_MAX_WIDTH).min(room.max(px(1.0)))
}

impl ConversationStream {
    pub(crate) fn utility_bar_visible(&self, cx: &App) -> bool {
        if !self.entries.is_empty() {
            return false;
        }
        if self.draft_route {
            return true;
        }
        let Some(binding) = self.thread.project_binding() else {
            return false;
        };
        cx.try_global::<SelectedProject>()
            .and_then(|selected| selected.0.as_deref())
            == Some(binding)
    }

    pub(crate) fn render_composer_utility_bar(
        &self,
        _: &Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        host::Bar {
            owner: cx.weak_entity(),
        }
        .into_any_element()
    }

    pub(crate) fn render_nsh_file_dropdown(&self, cx: &mut Context<Self>) -> AnyElement {
        host::Files {
            owner: cx.weak_entity(),
        }
        .into_any_element()
    }

    fn toggle_utility_projects_menu(&mut self, cx: &mut Context<Self>) {
        let open = !self.utility_projects_open;
        if open {
            self.branch_selector.update(cx, |selector, cx| {
                let _ = selector.request_close(cx);
            });
        }
        self.close_composer_popovers(cx);
        self.utility_projects_open = open;
        if open {
            self.utility_projects = Self::load_utility_projects(cx);
            self.utility_project_query.clear();
            self.utility_project_search
                .update(cx, |input, cx| input.clear(cx));
        }
        cx.notify();
    }

    pub(crate) fn sync_utility_project_query(
        &mut self,
        input: &Entity<TextInput>,
        cx: &mut Context<Self>,
    ) {
        let query = input.read(cx).text().to_owned();
        if self.utility_project_query != query {
            self.utility_project_query = query;
            cx.notify();
        }
    }

    fn load_utility_projects(cx: &App) -> Vec<(String, String)> {
        let Some(VegaStore(Ok(store))) = cx.try_global::<VegaStore>() else {
            return Vec::new();
        };
        let sort = vega_store::projects::ProjectSort::RecentlyOpened;
        match vega_store::projects::list(store.conn(), sort) {
            Ok(projects) => projects
                .into_iter()
                .map(|project| (project.id, project.name))
                .collect(),
            Err(error) => {
                tracing::error!(%error, "failed to read projects for the composer utility bar");
                Vec::new()
            }
        }
    }

    fn select_utility_project(
        &mut self,
        project_id: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        cx.emit(DraftProjectSelectionRequested {
            thread_id: self.thread.id.clone(),
            project: Some(project_id.to_owned()),
        });
        cx.set_global(SelectedProject(Some(project_id.to_string())));
        self.utility_projects_open = false;
        self.focus_composer(window, cx);
        cx.refresh_windows();
        cx.notify();
    }

    fn detach_utility_project(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        cx.emit(DraftProjectSelectionRequested {
            thread_id: self.thread.id.clone(),
            project: None,
        });
        cx.set_global(SelectedProject(None));
        self.utility_projects_open = false;
        self.focus_composer(window, cx);
        cx.refresh_windows();
        cx.notify();
    }
}
