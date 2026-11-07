use super::*;

#[derive(Clone, PartialEq, Eq)]
struct InputAuthority {
    entity: u64,
    revision: u64,
    composing: bool,
}

impl InputAuthority {
    fn read(input: &Entity<TextInput>, cx: &App) -> Self {
        Self {
            entity: input.entity_id().as_u64(),
            revision: input.read(cx).content_revision(),
            composing: input.read(cx).is_composing(),
        }
    }
}

#[derive(Clone, PartialEq, Eq)]
struct EditorAuthority {
    original: Option<String>,
    inputs: [InputAuthority; 3],
    automatic: bool,
    unknown: bool,
    source: PolicySource,
    legacy_present: bool,
    request: u64,
    saving: bool,
    loaded: Option<(Option<u64>, Option<u64>, bool)>,
    renamed_from: Option<String>,
}

#[derive(Clone, PartialEq)]
pub(in crate::settings) struct NavigationAuthority {
    generation: u64,
    operation: u64,
    next_request: u64,
    selected: Option<String>,
    saving: bool,
    form: bool,
    api: vega_conversation::types::ProviderApi,
    base: Option<ProviderConfig>,
    providers: Vec<ProviderConfig>,
    inputs: [InputAuthority; 4],
    draft: Option<ProviderDraftStamp>,
    editor: Option<EditorAuthority>,
    command: Option<(u8, Vec<String>)>,
    reopen: Option<(String, Option<String>)>,
}

#[cfg(test)]
impl NavigationAuthority {
    pub(in crate::settings) fn development_fixture() -> Self {
        Self {
            generation: 1,
            operation: 2,
            next_request: 3,
            selected: Some("provider".into()),
            saving: false,
            form: true,
            api: Default::default(),
            base: None,
            providers: vec![],
            inputs: std::array::from_fn(|index| InputAuthority {
                entity: index as u64,
                revision: 0,
                composing: false,
            }),
            draft: Some(ProviderDraftStamp::development_fixture()),
            editor: None,
            command: None,
            reopen: None,
        }
    }

    pub(in crate::settings) fn development_edit_key(&mut self) {
        self.inputs[3].revision += 1;
    }

    pub(in crate::settings) fn development_edit_draft(&mut self, composing: bool) {
        if let Some(draft) = &mut self.draft {
            draft.development_change(composing);
        }
    }
}

impl NavigationAuthority {
    pub(in crate::settings) fn draft(&self) -> Option<&ProviderDraftStamp> {
        self.draft.as_ref()
    }

    pub(in crate::settings) fn composing(&self) -> bool {
        self.draft
            .as_ref()
            .is_some_and(ProviderDraftStamp::composing)
            || self.inputs.iter().any(|input| input.composing)
            || self
                .editor
                .as_ref()
                .is_some_and(|editor| editor.inputs.iter().any(|input| input.composing))
    }
}

fn command_authority(command: &Command) -> (u8, Vec<String>) {
    match command {
        Command::Select(value) => (0, vec![value.clone()]),
        Command::Add => (1, vec![]),
        Command::Edit => (2, vec![]),
        Command::Enable => (3, vec![]),
        Command::Up => (4, vec![]),
        Command::Down => (5, vec![]),
        Command::Discover => (6, vec![]),
        Command::Test(value) => (7, vec![value.clone()]),
        Command::Cancel => (8, vec![]),
        Command::Import => (9, vec![]),
        Command::Candidate(value) => (10, vec![value.clone()]),
        Command::AddModel => (11, vec![]),
        Command::EditModel(value) => (12, vec![value.clone()]),
        Command::DeleteModel(provider, model) => (13, vec![provider.clone(), model.clone()]),
        Command::SaveModel => (14, vec![]),
        Command::ToggleModelAutomatic => (15, vec![]),
        Command::ToggleModelUnknown => (16, vec![]),
        Command::Reload => (17, vec![]),
        Command::ConfirmDelete(value) => (19, vec![value.clone()]),
        Command::BackToList => (20, vec![]),
        Command::PriceLibrary => (21, vec![]),
    }
}

impl SettingsView {
    pub(in crate::settings) fn navigation_provider_authority(
        &self,
        cx: &App,
    ) -> NavigationAuthority {
        let state = &self.provider_management;
        NavigationAuthority {
            generation: state.generation,
            operation: state.operation,
            next_request: state.next_model_context_request,
            selected: state.selected.clone(),
            saving: state.saving,
            form: state.form,
            api: state.form_api,
            base: state.form_base.clone(),
            providers: self.config.providers.clone(),
            inputs: [
                &self.name_input,
                &self.base_url_input,
                &self.models_input,
                &self.key_input,
            ]
            .map(|input| InputAuthority::read(input, cx)),
            draft: self.provider_nsh_stamp(),
            editor: state.model_editor.as_ref().map(|editor| EditorAuthority {
                original: editor.original.clone(),
                inputs: [
                    &editor.model_input,
                    &editor.input_limit,
                    &editor.output_limit,
                ]
                .map(|input| InputAuthority::read(input, cx)),
                automatic: editor.automatic,
                unknown: editor.unknown,
                source: editor.source,
                legacy_present: editor.legacy_present,
                request: editor.request_id,
                saving: editor.saving,
                loaded: editor.loaded_values,
                renamed_from: editor.renamed_from.clone(),
            }),
            command: state.pending_command.as_ref().map(command_authority),
            reopen: state.pending_model_reopen.clone(),
        }
    }
}
