use std::path::{Path, PathBuf};
use std::sync::Arc;

use vega_token::{
    ModelPricingSpec, PricingCatalog, PricingError, RateSpec, load_catalog_snapshot,
    load_or_seed_catalog, save_catalog_atomic,
};

use crate::types::{
    PricingDraftReason, PricingEntryKind, PricingEntryProjection, PricingMutation, PricingNotice,
    PricingRateInputs, PricingSettingsErrorCode, PricingSettingsProjection,
};

/// Validated pricing capability held only by the app-owned controller.
#[derive(Clone)]
pub struct PricingAuthority {
    catalog: Arc<PricingCatalog>,
    projection: Arc<Vec<PricingEntryProjection>>,
}

impl std::fmt::Debug for PricingAuthority {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("PricingAuthority")
            .field("entry_count", &self.projection.len())
            .finish()
    }
}

impl PricingAuthority {
    /// Returns whether this authority has the exact, case-sensitive model.
    pub fn contains_exact_model(&self, model: &str) -> bool {
        self.catalog.specs().any(|spec| spec.model == model)
    }

    /// Clones the frozen catalog for run-start ownership handoff (S7-T39/C3):
    /// the app copies the immutable selection into the run and the meter's
    /// provisional estimator instead of letting later reads touch files.
    pub fn catalog(&self) -> PricingCatalog {
        (*self.catalog).clone()
    }

    /// Builds a safe controller projection without exposing catalog bytes.
    pub fn project(
        &self,
        generation: u64,
        notice: Option<PricingNotice>,
        draft_reason: Option<PricingDraftReason>,
        error: Option<PricingSettingsErrorCode>,
    ) -> PricingSettingsProjection {
        PricingSettingsProjection::Ready {
            generation,
            entries: self.projection.as_ref().clone(),
            notice,
            draft_reason,
            error,
        }
    }

    /// Safe entries used while a save is in flight.
    pub fn entries(&self) -> Vec<PricingEntryProjection> {
        self.projection.as_ref().clone()
    }
}

/// Headless save result with explicit authority ownership semantics.
pub enum PricingSaveOutcome {
    Ready {
        authority: PricingAuthority,
        notice: Option<PricingNotice>,
        dirty_conflict: bool,
    },
    PreCommitFailure(PricingSettingsErrorCode),
    RecoveryRequired,
}

impl std::fmt::Debug for PricingSaveOutcome {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Ready {
                authority,
                notice,
                dirty_conflict,
            } => formatter
                .debug_struct("PricingSaveOutcome::Ready")
                .field("authority", authority)
                .field("notice", notice)
                .field("dirty_conflict", dirty_conflict)
                .finish(),
            Self::PreCommitFailure(code) => formatter
                .debug_tuple("PricingSaveOutcome::PreCommitFailure")
                .field(code)
                .finish(),
            Self::RecoveryRequired => formatter.write_str("PricingSaveOutcome::RecoveryRequired"),
        }
    }
}

/// Initial/reload authority plus a persistent, content-free durability notice.
pub struct PricingLoadOutcome {
    pub authority: PricingAuthority,
    pub notice: Option<PricingNotice>,
}

impl std::fmt::Debug for PricingLoadOutcome {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("PricingLoadOutcome")
            .field("authority", &self.authority)
            .field("notice", &self.notice)
            .finish()
    }
}

/// Opaque desired catalog owned by the app controller throughout Saving.
#[derive(Clone)]
pub struct PricingSavePlan {
    desired: PricingCatalog,
    projection: Arc<Vec<PricingEntryProjection>>,
}

impl std::fmt::Debug for PricingSavePlan {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("PricingSavePlan")
            .field("entry_count", &self.projection.len())
            .finish()
    }
}

impl PricingSavePlan {
    /// Safe desired projection retained across Settings close/reopen.
    pub fn entries(&self) -> Vec<PricingEntryProjection> {
        self.projection.as_ref().clone()
    }
}

/// Explicit-path pricing persistence and locked-profile policy.
pub struct PricingSettingsService {
    path: PathBuf,
}

impl std::fmt::Debug for PricingSettingsService {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("PricingSettingsService { path: <redacted> }")
    }
}

impl PricingSettingsService {
    /// Binds the service to one injected pricing path.
    pub fn new(path: PathBuf) -> Self {
        Self { path }
    }

    /// Loads existing authority or seeds built-ins only when the target is missing.
    pub fn load_or_seed(&self) -> Result<PricingLoadOutcome, PricingSettingsErrorCode> {
        let loaded = load_or_seed_catalog(&self.path).map_err(map_error)?;
        Ok(PricingLoadOutcome {
            authority: authority_from_catalog(loaded.catalog)?,
            notice: loaded
                .durability_unknown
                .then_some(PricingNotice::DurabilityUnknownReconciled),
        })
    }

    /// Explicitly reloads current bytes; malformed evidence is preserved by token I/O.
    pub fn reload(&self) -> Result<PricingLoadOutcome, PricingSettingsErrorCode> {
        match load_catalog_snapshot(&self.path) {
            Ok(snapshot) => Ok(PricingLoadOutcome {
                authority: authority_from_catalog(snapshot.into_catalog())?,
                notice: None,
            }),
            Err(PricingError::Io { operation: "open" }) if !path_exists(&self.path) => {
                self.load_or_seed()
            }
            Err(error) => Err(map_error(error)),
        }
    }

    /// Reconstructs and validates a desired catalog without doing file I/O.
    pub fn prepare_save(
        &self,
        current: &PricingAuthority,
        mutation: PricingMutation,
    ) -> Result<PricingSavePlan, PricingSettingsErrorCode> {
        let desired = apply_mutation(&current.catalog, mutation)?;
        // The controller may retain this plan across Settings close/reopen;
        // prove the desired canonical document fits the file cap before the
        // plan or its safe projection becomes durable app state.
        desired.encode().map_err(map_error)?;
        let builtins = PricingCatalog::built_in().map_err(map_error)?;
        let projection = desired
            .specs()
            .map(|spec| project_spec(spec, &builtins))
            .collect::<Result<Vec<_>, _>>()?;
        Ok(PricingSavePlan {
            desired,
            projection: Arc::new(projection),
        })
    }

    /// Persists one controller-owned desired plan and reconciles committed bytes.
    pub fn save(&self, plan: &PricingSavePlan) -> PricingSaveOutcome {
        match save_catalog_atomic(&self.path, &plan.desired) {
            Ok(()) => self.reconcile_desired(&plan.desired, None),
            Err(PricingError::CommittedDurabilityUnknown) => self.reconcile_desired(
                &plan.desired,
                Some(PricingNotice::DurabilityUnknownReconciled),
            ),
            Err(PricingError::SaveTargetChanged) => self.reconcile_external_winner(),
            Err(error) => PricingSaveOutcome::PreCommitFailure(map_error(error)),
        }
    }

    fn reconcile_external_winner(&self) -> PricingSaveOutcome {
        match self.load_existing() {
            Ok(authority) => PricingSaveOutcome::Ready {
                authority,
                notice: Some(PricingNotice::ExternalWinnerAdopted),
                dirty_conflict: true,
            },
            Err(_) => PricingSaveOutcome::RecoveryRequired,
        }
    }

    /// Reconciles a started save whose worker channel disappeared.
    ///
    /// This never writes or retries the mutation. It performs one safe reload
    /// and compares it with the controller-owned desired plan.
    pub fn recover_started_save(&self, plan: &PricingSavePlan) -> PricingSaveOutcome {
        self.reconcile_desired(
            &plan.desired,
            Some(PricingNotice::DurabilityUnknownReconciled),
        )
    }

    fn load_existing(&self) -> Result<PricingAuthority, PricingSettingsErrorCode> {
        let snapshot = load_catalog_snapshot(&self.path).map_err(map_error)?;
        authority_from_catalog(snapshot.into_catalog())
    }

    fn reconcile_desired(
        &self,
        desired: &PricingCatalog,
        notice: Option<PricingNotice>,
    ) -> PricingSaveOutcome {
        match load_catalog_snapshot(&self.path) {
            Ok(snapshot) => match snapshot.exactly_matches(desired) {
                Ok(true) => match authority_from_catalog(snapshot.into_catalog()) {
                    Ok(authority) => PricingSaveOutcome::Ready {
                        authority,
                        notice,
                        dirty_conflict: false,
                    },
                    Err(_) => PricingSaveOutcome::RecoveryRequired,
                },
                Ok(false) => match authority_from_catalog(snapshot.into_catalog()) {
                    Ok(authority) => PricingSaveOutcome::Ready {
                        authority,
                        notice: Some(PricingNotice::ExternalWinnerAdopted),
                        dirty_conflict: true,
                    },
                    Err(_) => PricingSaveOutcome::RecoveryRequired,
                },
                Err(_) => PricingSaveOutcome::RecoveryRequired,
            },
            Err(_) => PricingSaveOutcome::RecoveryRequired,
        }
    }
}

fn path_exists(path: &Path) -> bool {
    std::fs::symlink_metadata(path).is_ok()
}

fn authority_from_catalog(
    catalog: PricingCatalog,
) -> Result<PricingAuthority, PricingSettingsErrorCode> {
    let builtins = PricingCatalog::built_in().map_err(map_error)?;
    validate_catalog_policy_against(&catalog, &builtins)?;
    let projection = catalog
        .specs()
        .map(|spec| project_spec(spec, &builtins))
        .collect::<Result<Vec<_>, _>>()?;
    Ok(PricingAuthority {
        catalog: Arc::new(catalog),
        projection: Arc::new(projection),
    })
}

fn validate_catalog_policy_against(
    catalog: &PricingCatalog,
    builtins: &PricingCatalog,
) -> Result<(), PricingSettingsErrorCode> {
    for frozen in builtins.specs() {
        let actual =
            find_spec(catalog, &frozen.model).ok_or(PricingSettingsErrorCode::LockedProfile)?;
        if actual.max_standard_input_tokens != frozen.max_standard_input_tokens
            || schedule_metadata_differs(actual, frozen)
        {
            return Err(PricingSettingsErrorCode::LockedProfile);
        }
    }
    for spec in catalog.specs() {
        if find_spec(builtins, &spec.model).is_none()
            && (spec.max_standard_input_tokens.is_some() || spec.schedule.is_some())
        {
            return Err(PricingSettingsErrorCode::LockedProfile);
        }
    }
    Ok(())
}

fn schedule_metadata_differs(actual: &ModelPricingSpec, frozen: &ModelPricingSpec) -> bool {
    match (&actual.schedule, &frozen.schedule) {
        (None, None) => false,
        (Some(actual), Some(frozen)) => actual.windows != frozen.windows,
        _ => true,
    }
}

fn apply_mutation(
    current: &PricingCatalog,
    mutation: PricingMutation,
) -> Result<PricingCatalog, PricingSettingsErrorCode> {
    let mut specs = current.specs().cloned().collect::<Vec<_>>();
    let builtins = PricingCatalog::built_in().map_err(map_error)?;
    match mutation {
        PricingMutation::AddCustom { model, rates } => {
            if find_spec(&builtins, &model).is_some()
                || specs.iter().any(|spec| spec.model == model)
            {
                return Err(PricingSettingsErrorCode::InvalidInput);
            }
            specs.push(static_spec(model, rates));
        }
        PricingMutation::UpdateCustom { model, rates } => {
            if find_spec(&builtins, &model).is_some() {
                return Err(PricingSettingsErrorCode::LockedProfile);
            }
            let spec = find_spec_mut(&mut specs, &model)?;
            if spec.max_standard_input_tokens.is_some() || spec.schedule.is_some() {
                return Err(PricingSettingsErrorCode::LockedProfile);
            }
            spec.rates = rates.into();
        }
        PricingMutation::UpdateBuiltinBase { model, rates } => {
            let frozen =
                find_spec(&builtins, &model).ok_or(PricingSettingsErrorCode::LockedProfile)?;
            if frozen.schedule.is_some() {
                return Err(PricingSettingsErrorCode::LockedProfile);
            }
            find_spec_mut(&mut specs, &model)?.rates = rates.into();
        }
        PricingMutation::UpdateDeepSeek { model, base, peak } => {
            let frozen =
                find_spec(&builtins, &model).ok_or(PricingSettingsErrorCode::LockedProfile)?;
            if frozen.schedule.is_none() {
                return Err(PricingSettingsErrorCode::LockedProfile);
            }
            let spec = find_spec_mut(&mut specs, &model)?;
            spec.rates = base.into();
            let schedule = spec
                .schedule
                .as_mut()
                .ok_or(PricingSettingsErrorCode::LockedProfile)?;
            schedule.peak = peak.into();
        }
        PricingMutation::ResetBuiltin { model } => {
            let frozen = find_spec(&builtins, &model)
                .cloned()
                .ok_or(PricingSettingsErrorCode::LockedProfile)?;
            *find_spec_mut(&mut specs, &model)? = frozen;
        }
        PricingMutation::DeleteCustom { model } => {
            if find_spec(&builtins, &model).is_some() {
                return Err(PricingSettingsErrorCode::LockedProfile);
            }
            let before = specs.len();
            specs.retain(|spec| spec.model != model);
            if specs.len() == before {
                return Err(PricingSettingsErrorCode::InvalidInput);
            }
        }
    }
    let catalog = PricingCatalog::from_specs(specs).map_err(map_error)?;
    validate_catalog_policy_against(&catalog, &builtins)?;
    Ok(catalog)
}

fn find_spec<'a>(catalog: &'a PricingCatalog, model: &str) -> Option<&'a ModelPricingSpec> {
    catalog.specs().find(|spec| spec.model == model)
}

fn find_spec_mut<'a>(
    specs: &'a mut [ModelPricingSpec],
    model: &str,
) -> Result<&'a mut ModelPricingSpec, PricingSettingsErrorCode> {
    specs
        .iter_mut()
        .find(|spec| spec.model == model)
        .ok_or(PricingSettingsErrorCode::InvalidInput)
}

fn static_spec(model: String, rates: PricingRateInputs) -> ModelPricingSpec {
    ModelPricingSpec {
        model,
        rates: rates.into(),
        max_standard_input_tokens: None,
        schedule: None,
    }
}

impl From<PricingRateInputs> for RateSpec {
    fn from(value: PricingRateInputs) -> Self {
        Self {
            input_usd_per_million: value.input_usd_per_million,
            output_usd_per_million: value.output_usd_per_million,
            cache_read_usd_per_million: value.cache_read_usd_per_million,
            cache_write_usd_per_million: value.cache_write_usd_per_million,
        }
    }
}

fn project_spec(
    spec: &ModelPricingSpec,
    builtins: &PricingCatalog,
) -> Result<PricingEntryProjection, PricingSettingsErrorCode> {
    let kind = match find_spec(builtins, &spec.model) {
        Some(_) if spec.schedule.is_some() => PricingEntryKind::BuiltInScheduled,
        Some(_) if spec.max_standard_input_tokens.is_some() => PricingEntryKind::BuiltInCapped,
        Some(_) => PricingEntryKind::BuiltInStatic,
        None => PricingEntryKind::CustomStatic,
    };
    let peak = spec
        .schedule
        .as_ref()
        .map(|schedule| project_rates(&schedule.peak));
    if kind == PricingEntryKind::BuiltInScheduled && peak.is_none() {
        return Err(PricingSettingsErrorCode::LockedProfile);
    }
    Ok(PricingEntryProjection {
        model: spec.model.clone(),
        kind,
        base: project_rates(&spec.rates),
        peak,
    })
}

fn project_rates(rates: &RateSpec) -> PricingRateInputs {
    PricingRateInputs {
        input_usd_per_million: rates.input_usd_per_million.clone(),
        output_usd_per_million: rates.output_usd_per_million.clone(),
        cache_read_usd_per_million: rates.cache_read_usd_per_million.clone(),
        cache_write_usd_per_million: rates.cache_write_usd_per_million.clone(),
    }
}

fn map_error(error: PricingError) -> PricingSettingsErrorCode {
    match error {
        PricingError::FileTooLarge | PricingError::TooManyModels | PricingError::Overflow => {
            PricingSettingsErrorCode::LimitExceeded
        }
        PricingError::Io { .. } => PricingSettingsErrorCode::Io,
        PricingError::MalformedJson | PricingError::InvalidSchema { .. } => {
            PricingSettingsErrorCode::MalformedCatalog
        }
        PricingError::InvalidModelId | PricingError::InvalidDecimal { .. } => {
            PricingSettingsErrorCode::InvalidInput
        }
        PricingError::DuplicateModel { .. } => PricingSettingsErrorCode::MalformedCatalog,
        PricingError::ModelNotFound { .. } => PricingSettingsErrorCode::ModelNotPriced,
        PricingError::InvalidCacheUsage | PricingError::UnsupportedInputLimit { .. } => {
            PricingSettingsErrorCode::InvalidInput
        }
        PricingError::UnsafeSaveTarget | PricingError::SaveTargetChanged => {
            PricingSettingsErrorCode::TargetChanged
        }
        PricingError::CommittedDurabilityUnknown => PricingSettingsErrorCode::RecoveryRequired,
    }
}

#[cfg(test)]
mod tests {
    use std::fs;

    use tempfile::tempdir;

    use super::*;

    fn rates(seed: &str) -> PricingRateInputs {
        PricingRateInputs {
            input_usd_per_million: seed.to_string(),
            output_usd_per_million: seed.to_string(),
            cache_read_usd_per_million: seed.to_string(),
            cache_write_usd_per_million: seed.to_string(),
        }
    }

    #[test]
    fn owned_data_root_crud_restart_and_explicit_recovery_e2e() {
        let root = tempdir().unwrap();
        let path = root.path().join("pricing.json");
        let service = PricingSettingsService::new(path.clone());
        let authority = service.load_or_seed().unwrap().authority;
        assert_eq!(authority.entries().len(), 5);

        let plan = service
            .prepare_save(
                &authority,
                PricingMutation::AddCustom {
                    model: "custom/model".to_string(),
                    rates: rates("0.1"),
                },
            )
            .unwrap();
        let added = service.save(&plan);
        let PricingSaveOutcome::Ready { authority, .. } = added else {
            panic!("custom add did not publish authority");
        };
        assert!(authority.contains_exact_model("custom/model"));

        let restarted = PricingSettingsService::new(path.clone())
            .load_or_seed()
            .unwrap()
            .authority;
        let plan = service
            .prepare_save(
                &restarted,
                PricingMutation::UpdateCustom {
                    model: "custom/model".to_string(),
                    rates: rates("0.2"),
                },
            )
            .unwrap();
        let updated = service.save(&plan);
        let PricingSaveOutcome::Ready { authority, .. } = updated else {
            panic!("custom update did not publish authority");
        };
        let plan = service
            .prepare_save(
                &authority,
                PricingMutation::DeleteCustom {
                    model: "custom/model".to_string(),
                },
            )
            .unwrap();
        let deleted = service.save(&plan);
        let PricingSaveOutcome::Ready { authority, .. } = deleted else {
            panic!("custom delete did not publish authority");
        };
        assert!(!authority.contains_exact_model("custom/model"));

        fs::write(&path, b"{malformed evidence").unwrap();
        let evidence = fs::read(&path).unwrap();
        assert!(matches!(
            service.reload(),
            Err(PricingSettingsErrorCode::MalformedCatalog)
        ));
        assert_eq!(fs::read(&path).unwrap(), evidence);
        fs::remove_file(&path).unwrap();
        assert_eq!(service.reload().unwrap().authority.entries().len(), 5);
    }

    #[test]
    fn locked_profiles_require_deepseek_eight_rates_and_static_customs() {
        let root = tempdir().unwrap();
        let path = root.path().join("pricing.json");
        let service = PricingSettingsService::new(path.clone());
        let authority = service.load_or_seed().unwrap().authority;
        let deepseek = authority
            .entries()
            .into_iter()
            .find(|entry| entry.model == "deepseek-v4-flash")
            .unwrap();
        assert_eq!(deepseek.kind, PricingEntryKind::BuiltInScheduled);
        assert!(deepseek.peak.is_some());
        assert!(matches!(
            service.prepare_save(
                &authority,
                PricingMutation::UpdateBuiltinBase {
                    model: "deepseek-v4-flash".to_string(),
                    rates: rates("1"),
                }
            ),
            Err(PricingSettingsErrorCode::LockedProfile)
        ));
        assert!(matches!(
            service.prepare_save(
                &authority,
                PricingMutation::DeleteCustom {
                    model: "gpt-5.6-terra".to_string(),
                }
            ),
            Err(PricingSettingsErrorCode::LockedProfile)
        ));

        let base = PricingRateInputs {
            input_usd_per_million: "1.01".into(),
            output_usd_per_million: "2.02".into(),
            cache_read_usd_per_million: "3.03".into(),
            cache_write_usd_per_million: "4.04".into(),
        };
        let peak = PricingRateInputs {
            input_usd_per_million: "5.05".into(),
            output_usd_per_million: "6.06".into(),
            cache_read_usd_per_million: "7.07".into(),
            cache_write_usd_per_million: "8.08".into(),
        };
        let plan = service
            .prepare_save(
                &authority,
                PricingMutation::UpdateDeepSeek {
                    model: "deepseek-v4-flash".into(),
                    base: base.clone(),
                    peak: peak.clone(),
                },
            )
            .unwrap();
        assert!(matches!(
            service.save(&plan),
            PricingSaveOutcome::Ready { .. }
        ));
        let reloaded = service.reload().unwrap().authority;
        let reloaded_entry = reloaded
            .entries()
            .into_iter()
            .find(|entry| entry.model == "deepseek-v4-flash")
            .unwrap();
        assert_eq!(reloaded_entry.base, base);
        assert_eq!(reloaded_entry.peak, Some(peak));

        let persisted = load_catalog_snapshot(&path).unwrap().into_catalog();
        let frozen = PricingCatalog::built_in().unwrap();
        let persisted_spec = find_spec(&persisted, "deepseek-v4-flash").unwrap();
        let frozen_spec = find_spec(&frozen, "deepseek-v4-flash").unwrap();
        assert_eq!(
            persisted_spec
                .schedule
                .as_ref()
                .map(|schedule| &schedule.windows),
            frozen_spec
                .schedule
                .as_ref()
                .map(|schedule| &schedule.windows)
        );
        assert_eq!(
            persisted_spec.max_standard_input_tokens,
            frozen_spec.max_standard_input_tokens
        );
    }

    #[test]
    fn policy_uses_the_frozen_catalog_membership_not_a_hardcoded_id_list() {
        let current = PricingCatalog::built_in().unwrap();
        let mut frozen_specs = current.specs().cloned().collect::<Vec<_>>();
        frozen_specs.push(ModelPricingSpec {
            model: "future-built-in".to_string(),
            rates: RateSpec {
                input_usd_per_million: "1".to_string(),
                output_usd_per_million: "1".to_string(),
                cache_read_usd_per_million: "1".to_string(),
                cache_write_usd_per_million: "1".to_string(),
            },
            max_standard_input_tokens: Some(42),
            schedule: None,
        });
        let frozen = PricingCatalog::from_specs(frozen_specs.clone()).unwrap();
        assert_eq!(
            validate_catalog_policy_against(&current, &frozen),
            Err(PricingSettingsErrorCode::LockedProfile)
        );

        let with_future = PricingCatalog::from_specs(frozen_specs).unwrap();
        validate_catalog_policy_against(&with_future, &frozen).unwrap();
        let projected =
            project_spec(find_spec(&with_future, "future-built-in").unwrap(), &frozen).unwrap();
        assert_eq!(projected.kind, PricingEntryKind::BuiltInCapped);
    }

    #[test]
    fn desired_catalog_must_fit_before_the_plan_is_published() {
        let root = tempdir().unwrap();
        let service = PricingSettingsService::new(root.path().join("pricing.json"));
        let authority = service.load_or_seed().unwrap().authority;
        let huge_zero = "0".repeat(270_000);
        assert!(matches!(
            service.prepare_save(
                &authority,
                PricingMutation::AddCustom {
                    model: "custom/too-large".to_string(),
                    rates: rates(&huge_zero),
                }
            ),
            Err(PricingSettingsErrorCode::LimitExceeded)
        ));
    }

    #[test]
    fn all_postcommit_or_ambiguous_reconcile_failures_use_recovery_required() {
        let root = tempdir().unwrap();
        let path = root.path().join("pricing.json");
        let service = PricingSettingsService::new(path.clone());
        let authority = service.load_or_seed().unwrap().authority;
        let plan = service
            .prepare_save(
                &authority,
                PricingMutation::AddCustom {
                    model: "custom/recovery".to_string(),
                    rates: rates("1"),
                },
            )
            .unwrap();

        fs::remove_file(&path).unwrap();
        assert!(matches!(
            service.recover_started_save(&plan),
            PricingSaveOutcome::RecoveryRequired
        ));

        fs::write(&path, b"{postcommit malformed").unwrap();
        assert!(matches!(
            service.reconcile_desired(
                &plan.desired,
                Some(PricingNotice::DurabilityUnknownReconciled)
            ),
            PricingSaveOutcome::RecoveryRequired
        ));
        assert!(matches!(
            service.reconcile_external_winner(),
            PricingSaveOutcome::RecoveryRequired
        ));
    }

    #[test]
    fn codec_valid_policy_forgery_is_preserved_and_never_becomes_authority() {
        let root = tempdir().unwrap();
        let path = root.path().join("pricing.json");
        let service = PricingSettingsService::new(path.clone());
        let authority = service.load_or_seed().unwrap().authority;
        let mut specs = PricingCatalog::built_in()
            .unwrap()
            .specs()
            .cloned()
            .collect::<Vec<_>>();
        let deepseek = specs
            .iter()
            .find(|spec| spec.model == "deepseek-v4-flash")
            .unwrap()
            .clone();
        specs.push(ModelPricingSpec {
            model: "custom/forged-schedule".to_string(),
            rates: deepseek.rates,
            max_standard_input_tokens: None,
            schedule: deepseek.schedule,
        });
        let forged = PricingCatalog::from_specs(specs).unwrap();
        save_catalog_atomic(&path, &forged).unwrap();
        let evidence = fs::read(&path).unwrap();

        assert!(matches!(
            service.reload(),
            Err(PricingSettingsErrorCode::LockedProfile)
        ));
        assert_eq!(fs::read(&path).unwrap(), evidence);
        assert!(matches!(
            service.reconcile_external_winner(),
            PricingSaveOutcome::RecoveryRequired
        ));
        assert!(authority.contains_exact_model("deepseek-v4-flash"));
    }

    #[test]
    fn started_disconnect_exact_reconcile_keeps_a_persistent_warning() {
        let root = tempdir().unwrap();
        let service = PricingSettingsService::new(root.path().join("pricing.json"));
        let authority = service.load_or_seed().unwrap().authority;
        let plan = service
            .prepare_save(
                &authority,
                PricingMutation::AddCustom {
                    model: "custom/disconnect".to_string(),
                    rates: rates("1"),
                },
            )
            .unwrap();
        assert!(matches!(
            service.save(&plan),
            PricingSaveOutcome::Ready { .. }
        ));
        assert!(matches!(
            service.recover_started_save(&plan),
            PricingSaveOutcome::Ready {
                notice: Some(PricingNotice::DurabilityUnknownReconciled),
                dirty_conflict: false,
                ..
            }
        ));
    }
}
