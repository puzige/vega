use super::*;
use vega_conversation::types::{
    CatalogAction, CatalogIdentity, CatalogModel, ModelCatalogProjection, ModelCatalogRequested,
};

mod nsh_catalog;

pub(crate) struct CatalogUi {
    presentation: Option<nsh_catalog::Presentation>,
    version: crate::nsh::ProjectionId,
    help: bool,
    owner: Option<CatalogIdentity>,
    generation: u64,
    projection: ModelCatalogProjection,
    open: bool,
    mapping: bool,
    selected_provider: Option<String>,
    candidate: Option<CatalogIdentity>,
    message: Option<SettingsText>,
    error: Option<SettingsText>,
}
impl CatalogUi {
    pub(crate) fn new(_: &mut Context<SettingsView>) -> Self {
        Self {
            presentation: None,
            version: crate::nsh::ProjectionId::new(),
            help: false,
            owner: None,
            generation: 0,
            projection: Default::default(),
            open: false,
            mapping: false,
            selected_provider: None,
            candidate: None,
            message: None,
            error: None,
        }
    }
}

fn simple_prices(model: &CatalogModel) -> Option<[String; 4]> {
    if !model.text_pricing {
        return None;
    }
    let cost = model.cost.as_ref().filter(|cost| !cost.complex)?;
    let rates = [
        cost.input.clone()?,
        cost.output.clone()?,
        cost.cache_read.clone()?,
        cost.cache_write.clone()?,
    ];
    let representable = rates.iter().all(|rate| {
        let (whole, fraction) = rate.split_once('.').unwrap_or((rate, ""));
        if whole.is_empty()
            || !whole.bytes().all(|b| b.is_ascii_digit())
            || fraction.len() > 6
            || !fraction.bytes().all(|b| b.is_ascii_digit())
        {
            return false;
        }
        whole
            .parse::<u64>()
            .ok()
            .and_then(|n| n.checked_mul(1_000_000))
            .and_then(|n| {
                n.checked_add(
                    fraction.parse::<u64>().unwrap_or(0) * 10_u64.pow(6 - fraction.len() as u32),
                )
            })
            .is_some()
    });
    representable.then_some(rates)
}

impl SettingsView {
    pub(super) fn refresh_catalog_localization(&self, cx: &mut Context<Self>) {
        self.sync_catalog_presentation(cx);
    }

    pub(crate) fn ensure_model_catalog(&mut self, cx: &mut Context<Self>) {
        let owner = if matches!(self.section, 0 | 2 | 3) {
            self.model_settings
                .as_ref()
                .map(|(provider, model)| CatalogIdentity {
                    provider: provider.clone(),
                    model: model.clone(),
                })
        } else {
            None
        };
        if self.catalog.owner == owner {
            return;
        }
        self.catalog.owner = owner;
        self.catalog.version = crate::nsh::ProjectionId::new();
        self.catalog.help = false;
        self.catalog.projection = Default::default();
        self.catalog.error = None;
        self.catalog.open = false;
        self.catalog.mapping = false;
        self.catalog.message = None;
        self.catalog.selected_provider = None;
        self.catalog.candidate = None;
        self.request_model_catalog(CatalogAction::Load, cx);
    }

    fn request_model_catalog(&mut self, action: CatalogAction, cx: &mut Context<Self>) {
        self.catalog.generation = self.catalog.generation.saturating_add(1);
        let Some(owner) = self.catalog.owner.clone() else {
            return;
        };
        self.catalog.projection.refreshing = true;
        self.catalog.message = None;
        cx.emit(ModelCatalogRequested {
            generation: self.catalog.generation,
            owner,
            action,
        });
        cx.notify();
    }

    pub fn apply_model_catalog(
        &mut self,
        request: &ModelCatalogRequested,
        projection: ModelCatalogProjection,
        cx: &mut Context<Self>,
    ) -> bool {
        if self.catalog.owner.as_ref() != Some(&request.owner)
            || self.catalog.generation != request.generation
            || !matches!(self.section, 0 | 2 | 3)
            || self.model_settings.as_ref()
                != Some(&(request.owner.provider.clone(), request.owner.model.clone()))
        {
            return false;
        }
        if matches!(request.action, CatalogAction::Map(_)) && projection.error.is_none() {
            self.catalog.mapping = false;
            self.catalog.candidate = None;
        }
        self.catalog.error = projection.error.clone().map(|error| {
            SettingsText::from_known(
                error,
                &[
                    "app.catalog.budget_invalid",
                    "app.catalog.cache_create",
                    "app.catalog.cache_format",
                    "app.catalog.cache_integrity",
                    "app.catalog.cache_limit",
                    "app.catalog.cache_save",
                    "app.catalog.cache_unavailable",
                    "app.catalog.cache_write",
                    "app.catalog.effort_invalid",
                    "app.catalog.effort_limit",
                    "app.catalog.encoding",
                    "app.catalog.fetch_failed",
                    "app.catalog.format_invalid",
                    "app.catalog.mapping_format",
                    "app.catalog.mapping_limit",
                    "app.catalog.mapping_target",
                    "app.catalog.model_identity",
                    "app.catalog.network",
                    "app.catalog.path_invalid",
                    "app.catalog.path_unavailable",
                    "app.catalog.price_format",
                    "app.catalog.provider_count",
                    "app.catalog.provider_identity",
                    "app.catalog.reasoning_invalid",
                    "app.catalog.response_limit",
                    "app.catalog.response_read",
                    "app.catalog.service_unavailable",
                ],
            )
        });
        self.catalog.projection = projection;
        self.catalog.version = crate::nsh::ProjectionId::new();
        cx.notify();
        true
    }

    fn catalog_apply_capacity(
        &mut self,
        stamp: Option<&super::provider_management::ProviderDraftStamp>,
        input: bool,
        cx: &mut Context<Self>,
    ) {
        let Some(stamp) = stamp else {
            return;
        };
        let Some(model) = self.catalog.projection.matched_model().cloned() else {
            return;
        };
        let Some(value) = (if input {
            model.input
        } else {
            Some(model.output)
        })
        .filter(|value| *value > 0) else {
            return;
        };
        if !self.apply_catalog_capacity_observed(stamp, input, value, cx) {
            return;
        }
        self.catalog.message = Some(SettingsText::key("settings.catalog.filled"));
        cx.notify();
    }

    fn catalog_apply_prices(&mut self, cx: &mut Context<Self>) {
        let Some((_, model)) = self.model_settings.clone() else {
            return;
        };
        let Some(rates) = self
            .catalog
            .projection
            .matched_model()
            .and_then(simple_prices)
        else {
            return;
        };
        if self.settings_have_draft(cx) || !self.pricing_allows_editing() {
            return;
        }
        let entry = match &self.pricing {
            PricingSettingsProjection::Ready { entries, .. } => {
                entries.iter().find(|entry| entry.model == model).cloned()
            }
            _ => None,
        };
        if let Some(entry) = entry {
            if entry.kind != PricingEntryKind::CustomStatic {
                return;
            }
            self.begin_edit_pricing(entry, cx);
        } else {
            self.begin_add_custom(cx);
        }
        for (input, value) in self.pricing_rate_inputs[..4].iter().zip(rates) {
            input.update(cx, |input, cx| input.set_text(&value, cx));
        }
        self.reset_pricing_draft(cx);
        self.section = 3;
        cx.notify();
    }
}
