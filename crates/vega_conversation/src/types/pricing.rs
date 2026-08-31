#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PricingSettingsErrorCode {
    /// The pricing file could not be safely read or written.
    Io,
    /// The document is malformed or violates the strict pricing schema.
    MalformedCatalog,
    /// The catalog omits or forges a locked built-in profile.
    LockedProfile,
    /// A model id or decimal input is invalid.
    InvalidInput,
    /// The exact model is absent from current authority.
    ModelNotPriced,
    /// The pricing target is unsafe or changed during save.
    TargetChanged,
    /// A committed save could not be reconciled authoritatively.
    RecoveryRequired,
    /// Another pricing operation already owns the controller.
    Busy,
    /// A checked sequence or retained limit was exceeded.
    LimitExceeded,
}

/// Four exact USD-per-million decimal strings safe for Settings projection.
#[derive(Clone, PartialEq, Eq)]
pub struct PricingRateInputs {
    pub input_usd_per_million: String,
    pub output_usd_per_million: String,
    pub cache_read_usd_per_million: String,
    pub cache_write_usd_per_million: String,
}

impl std::fmt::Debug for PricingRateInputs {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("PricingRateInputs { <redacted> }")
    }
}

/// Policy-owned entry kind exposed to Settings without generic metadata.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PricingEntryKind {
    BuiltInStatic,
    BuiltInCapped,
    BuiltInScheduled,
    CustomStatic,
}

/// Bounded, safe representation of one validated pricing entry.
#[derive(Clone, PartialEq, Eq)]
pub struct PricingEntryProjection {
    pub model: String,
    pub kind: PricingEntryKind,
    pub base: PricingRateInputs,
    pub peak: Option<PricingRateInputs>,
}

impl std::fmt::Debug for PricingEntryProjection {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("PricingEntryProjection")
            .field("kind", &self.kind)
            .field("has_peak", &self.peak.is_some())
            .finish()
    }
}

/// Non-authoritative Settings mutation; the headless policy reconstructs all metadata.
#[derive(Clone, PartialEq, Eq)]
pub enum PricingMutation {
    AddCustom {
        model: String,
        rates: PricingRateInputs,
    },
    UpdateCustom {
        model: String,
        rates: PricingRateInputs,
    },
    UpdateBuiltinBase {
        model: String,
        rates: PricingRateInputs,
    },
    UpdateDeepSeek {
        model: String,
        base: PricingRateInputs,
        peak: PricingRateInputs,
    },
    ResetBuiltin {
        model: String,
    },
    DeleteCustom {
        model: String,
    },
}

impl std::fmt::Debug for PricingMutation {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(match self {
            Self::AddCustom { .. } => "PricingMutation::AddCustom(<redacted>)",
            Self::UpdateCustom { .. } => "PricingMutation::UpdateCustom(<redacted>)",
            Self::UpdateBuiltinBase { .. } => "PricingMutation::UpdateBuiltinBase(<redacted>)",
            Self::UpdateDeepSeek { .. } => "PricingMutation::UpdateDeepSeek(<redacted>)",
            Self::ResetBuiltin { .. } => "PricingMutation::ResetBuiltin(<redacted>)",
            Self::DeleteCustom { .. } => "PricingMutation::DeleteCustom(<redacted>)",
        })
    }
}

/// Persistent non-error notice attached to current Ready authority.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PricingNotice {
    DurabilityUnknownReconciled,
    ExternalWinnerAdopted,
}

/// Why a controller-owned desired plan remains pending in Ready state.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PricingDraftReason {
    /// No bytes committed; retry may attempt the original plan.
    RetryPending,
    /// A valid external winner became authority while the original plan stayed dirty.
    ExternalConflict,
}

/// App-owned pricing controller projection consumed by Settings.
#[derive(Clone, PartialEq, Eq)]
pub enum PricingSettingsProjection {
    Loading,
    Ready {
        generation: u64,
        entries: Vec<PricingEntryProjection>,
        notice: Option<PricingNotice>,
        draft_reason: Option<PricingDraftReason>,
        error: Option<PricingSettingsErrorCode>,
    },
    Saving {
        generation: u64,
        entries: Vec<PricingEntryProjection>,
    },
    Reloading,
    Invalid(PricingSettingsErrorCode),
}

impl std::fmt::Debug for PricingSettingsProjection {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Loading => formatter.write_str("PricingSettingsProjection::Loading"),
            Self::Ready {
                generation,
                entries,
                notice,
                draft_reason,
                error,
            } => formatter
                .debug_struct("PricingSettingsProjection::Ready")
                .field("generation", generation)
                .field("entry_count", &entries.len())
                .field("notice", notice)
                .field("draft_reason", draft_reason)
                .field("error", error)
                .finish(),
            Self::Saving {
                generation,
                entries,
            } => formatter
                .debug_struct("PricingSettingsProjection::Saving")
                .field("generation", generation)
                .field("entry_count", &entries.len())
                .finish(),
            Self::Reloading => formatter.write_str("PricingSettingsProjection::Reloading"),
            Self::Invalid(code) => formatter
                .debug_tuple("PricingSettingsProjection::Invalid")
                .field(code)
                .finish(),
        }
    }
}
