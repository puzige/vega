use std::collections::{BTreeMap, BTreeSet};

use serde::{Deserialize, Deserializer, Serialize, de};

use crate::PricingError;

pub(crate) const MAX_FILE_BYTES: usize = 1024 * 1024;
pub(crate) const MAX_MODELS: usize = 1_000;
const MAX_MODEL_ID_BYTES: usize = 200;
const MAX_WINDOWS: usize = 32;
const MICROCENTS_PER_USD: u128 = 1_000_000;
const TOKENS_PER_MILLION: u128 = 1_000_000;

pub(crate) const BUILTIN_JSON: &str = r#"{
  "schema_version": "pricing_v1",
  "currency": "USD",
  "models": [
    {
      "model": "gpt-5.6-terra",
      "input_usd_per_million": "2.00",
      "output_usd_per_million": "12.00",
      "cache_read_usd_per_million": "0.20",
      "cache_write_usd_per_million": "2.50",
      "max_standard_input_tokens": 272000
    },
    {
      "model": "gpt-5.6-luna",
      "input_usd_per_million": "0.20",
      "output_usd_per_million": "1.20",
      "cache_read_usd_per_million": "0.02",
      "cache_write_usd_per_million": "0.25",
      "max_standard_input_tokens": 272000
    },
    {
      "model": "deepseek-v4-flash",
      "input_usd_per_million": "0.22",
      "output_usd_per_million": "0.66",
      "cache_read_usd_per_million": "0.007",
      "cache_write_usd_per_million": "0",
      "schedule": {
        "kind": "utc_weekly_v1",
        "windows": [
          { "weekdays": [1, 2, 3, 4, 5], "start_minute": 60, "end_minute": 240 },
          { "weekdays": [1, 2, 3, 4, 5], "start_minute": 360, "end_minute": 600 }
        ],
        "peak": {
          "input_usd_per_million": "0.44",
          "output_usd_per_million": "1.32",
          "cache_read_usd_per_million": "0.014",
          "cache_write_usd_per_million": "0"
        }
      }
    },
    {
      "model": "deepseek-v4-pro",
      "input_usd_per_million": "0.66",
      "output_usd_per_million": "1.98",
      "cache_read_usd_per_million": "0.022",
      "cache_write_usd_per_million": "0",
      "schedule": {
        "kind": "utc_weekly_v1",
        "windows": [
          { "weekdays": [1, 2, 3, 4, 5], "start_minute": 60, "end_minute": 240 },
          { "weekdays": [1, 2, 3, 4, 5], "start_minute": 360, "end_minute": 600 }
        ],
        "peak": {
          "input_usd_per_million": "1.32",
          "output_usd_per_million": "3.96",
          "cache_read_usd_per_million": "0.044",
          "cache_write_usd_per_million": "0"
        }
      }
    },
    {
      "model": "claude-sonnet-5",
      "input_usd_per_million": "2.00",
      "output_usd_per_million": "10.00",
      "cache_read_usd_per_million": "0.20",
      "cache_write_usd_per_million": "2.50"
    }
  ]
}"#;

/// Four exact USD-per-million decimal strings.
#[derive(Clone, PartialEq, Eq)]
pub struct RateSpec {
    pub input_usd_per_million: String,
    pub output_usd_per_million: String,
    pub cache_read_usd_per_million: String,
    pub cache_write_usd_per_million: String,
}

impl std::fmt::Debug for RateSpec {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("RateSpec { <redacted> }")
    }
}

/// One half-open weekly UTC window using ISO weekdays.
#[derive(Clone, PartialEq, Eq)]
pub struct WeeklyWindowSpec {
    pub weekdays: Vec<u8>,
    pub start_minute: u16,
    pub end_minute: u16,
}

impl std::fmt::Debug for WeeklyWindowSpec {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("WeeklyWindowSpec")
            .field("weekday_count", &self.weekdays.len())
            .finish_non_exhaustive()
    }
}

/// Optional peak override for a model's base rates.
#[derive(Clone, PartialEq, Eq)]
pub struct WeeklyScheduleSpec {
    pub windows: Vec<WeeklyWindowSpec>,
    pub peak: RateSpec,
}

impl std::fmt::Debug for WeeklyScheduleSpec {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("WeeklyScheduleSpec")
            .field("window_count", &self.windows.len())
            .finish_non_exhaustive()
    }
}

/// Editable, validated source representation for one exact model id.
#[derive(Clone, PartialEq, Eq)]
pub struct ModelPricingSpec {
    pub model: String,
    pub rates: RateSpec,
    pub max_standard_input_tokens: Option<u64>,
    pub schedule: Option<WeeklyScheduleSpec>,
}

impl std::fmt::Debug for ModelPricingSpec {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("ModelPricingSpec")
            .field(
                "has_standard_input_cap",
                &self.max_standard_input_tokens.is_some(),
            )
            .field("has_schedule", &self.schedule.is_some())
            .finish()
    }
}

/// Token counts for a single provider call.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct UsageCounts {
    pub input: u64,
    pub output: u64,
    pub cache_read: u64,
    pub cache_write: u64,
}

/// The exact rate profile selected for a quote.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PricingProfile {
    Base,
    PeakUtcWeekly,
}

/// A checked cost quote for one provider call.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PricingQuote {
    pub cost_microcents: i64,
    pub pricing_version: &'static str,
    pub profile: PricingProfile,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct ParsedRates {
    input: u64,
    output: u64,
    cache_read: u64,
    cache_write: u64,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct ModelPricing {
    spec: ModelPricingSpec,
    base: ParsedRates,
    peak: Option<ParsedRates>,
}

/// Strict `pricing_v1` catalog with exact, case-sensitive model lookup.
#[derive(Clone, PartialEq, Eq)]
pub struct PricingCatalog {
    models: Vec<ModelPricing>,
    index: BTreeMap<String, usize>,
}

impl std::fmt::Debug for PricingCatalog {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("PricingCatalog")
            .field("model_count", &self.models.len())
            .finish()
    }
}

impl PricingCatalog {
    /// Decodes a strict, size-bounded `pricing_v1` document.
    pub fn decode(bytes: &[u8]) -> Result<Self, PricingError> {
        if bytes.len() > MAX_FILE_BYTES {
            return Err(PricingError::FileTooLarge);
        }
        let raw: RawCatalog =
            serde_json::from_slice(bytes).map_err(|_| PricingError::MalformedJson)?;
        if raw.schema_version != "pricing_v1" {
            return Err(PricingError::InvalidSchema {
                field: "schema_version",
            });
        }
        if raw.currency != "USD" {
            return Err(PricingError::InvalidSchema { field: "currency" });
        }
        for model in &raw.models {
            if let Some(schedule) = &model.schedule {
                schedule.validate_kind()?;
            }
        }
        let specs = raw.models.into_iter().map(ModelPricingSpec::from).collect();
        Self::from_specs(specs)
    }

    /// Returns the five source-verified built-in entries.
    pub fn built_in() -> Result<Self, PricingError> {
        Self::decode(BUILTIN_JSON.as_bytes())
    }

    /// Creates a catalog from editable source specs.
    pub fn from_specs(specs: Vec<ModelPricingSpec>) -> Result<Self, PricingError> {
        if specs.len() > MAX_MODELS {
            return Err(PricingError::TooManyModels);
        }
        let mut models = Vec::with_capacity(specs.len());
        let mut index = BTreeMap::new();
        for spec in specs {
            validate_model_id(&spec.model)?;
            if index.contains_key(&spec.model) {
                return Err(PricingError::DuplicateModel { model: spec.model });
            }
            let parsed = parse_model(spec)?;
            let position = models.len();
            index.insert(parsed.spec.model.clone(), position);
            models.push(parsed);
        }
        Ok(Self { models, index })
    }

    /// Encodes a validated catalog without JSON numeric prices.
    pub fn encode(&self) -> Result<Vec<u8>, PricingError> {
        let raw = RawCatalog {
            schema_version: "pricing_v1".to_string(),
            currency: "USD".to_string(),
            models: self
                .models
                .iter()
                .map(|model| RawModel::from(&model.spec))
                .collect(),
        };
        let bytes = serde_json::to_vec_pretty(&raw)
            .map_err(|_| PricingError::InvalidSchema { field: "catalog" })?;
        if bytes.len() > MAX_FILE_BYTES {
            return Err(PricingError::FileTooLarge);
        }
        Ok(bytes)
    }

    /// Iterates exact source specs in stable file order.
    pub fn specs(&self) -> impl ExactSizeIterator<Item = &ModelPricingSpec> {
        self.models.iter().map(|model| &model.spec)
    }

    /// Replaces an exact id or appends a new validated custom entry.
    pub fn upsert(&mut self, spec: ModelPricingSpec) -> Result<(), PricingError> {
        validate_model_id(&spec.model)?;
        let parsed = parse_model(spec)?;
        if let Some(position) = self.index.get(&parsed.spec.model).copied() {
            self.models[position] = parsed;
            return Ok(());
        }
        if self.models.len() == MAX_MODELS {
            return Err(PricingError::TooManyModels);
        }
        let position = self.models.len();
        self.index.insert(parsed.spec.model.clone(), position);
        self.models.push(parsed);
        Ok(())
    }

    /// Removes an exact id without alias or case folding.
    pub fn remove(&mut self, model: &str) -> Result<bool, PricingError> {
        validate_model_id(model)?;
        let Some(position) = self.index.remove(model) else {
            return Ok(false);
        };
        self.models.remove(position);
        self.rebuild_index();
        Ok(true)
    }

    /// Quotes one call at an explicit Unix UTC timestamp.
    pub fn quote(
        &self,
        model: &str,
        usage: UsageCounts,
        unix_utc_seconds: i64,
    ) -> Result<PricingQuote, PricingError> {
        validate_model_id(model)?;
        let position =
            self.index
                .get(model)
                .copied()
                .ok_or_else(|| PricingError::ModelNotFound {
                    model: model.to_string(),
                })?;
        let pricing = &self.models[position];
        if usage.cache_read > usage.input {
            return Err(PricingError::InvalidCacheUsage);
        }
        if let Some(max_tokens) = pricing.spec.max_standard_input_tokens
            && usage.input > max_tokens
        {
            return Err(PricingError::UnsupportedInputLimit {
                model: model.to_string(),
                max_tokens,
            });
        }
        let (rates, profile) = match (&pricing.spec.schedule, &pricing.peak) {
            (Some(schedule), Some(peak)) if schedule_matches(schedule, unix_utc_seconds) => {
                (peak, PricingProfile::PeakUtcWeekly)
            }
            _ => (&pricing.base, PricingProfile::Base),
        };
        let cost_microcents = quote_rates(rates, usage)?;
        Ok(PricingQuote {
            cost_microcents,
            pricing_version: "pricing_v1",
            profile,
        })
    }

    fn rebuild_index(&mut self) {
        self.index.clear();
        for (position, model) in self.models.iter().enumerate() {
            self.index.insert(model.spec.model.clone(), position);
        }
    }
}

fn parse_model(spec: ModelPricingSpec) -> Result<ModelPricing, PricingError> {
    if matches!(spec.max_standard_input_tokens, Some(0)) {
        return Err(PricingError::InvalidSchema {
            field: "max_standard_input_tokens",
        });
    }
    if spec.max_standard_input_tokens.is_some() && spec.schedule.is_some() {
        return Err(PricingError::InvalidSchema {
            field: "model_price_profile",
        });
    }
    validate_known_model_profile(&spec)?;
    let base = parse_rates(&spec.rates)?;
    let peak = match &spec.schedule {
        Some(schedule) => {
            validate_schedule(schedule)?;
            Some(parse_rates(&schedule.peak)?)
        }
        None => None,
    };
    Ok(ModelPricing { spec, base, peak })
}

fn validate_known_model_profile(spec: &ModelPricingSpec) -> Result<(), PricingError> {
    match spec.model.as_str() {
        "gpt-5.6-terra" | "gpt-5.6-luna" => {
            if spec.max_standard_input_tokens != Some(272_000) || spec.schedule.is_some() {
                return Err(PricingError::InvalidSchema {
                    field: "known_model_profile",
                });
            }
        }
        "deepseek-v4-flash" | "deepseek-v4-pro" => {
            let valid_schedule = spec
                .schedule
                .as_ref()
                .is_some_and(is_deepseek_weekly_schedule);
            if spec.max_standard_input_tokens.is_some() || !valid_schedule {
                return Err(PricingError::InvalidSchema {
                    field: "known_model_profile",
                });
            }
        }
        "claude-sonnet-5"
            if spec.max_standard_input_tokens.is_some() || spec.schedule.is_some() =>
        {
            return Err(PricingError::InvalidSchema {
                field: "known_model_profile",
            });
        }
        _ => {}
    }
    Ok(())
}

fn is_deepseek_weekly_schedule(schedule: &WeeklyScheduleSpec) -> bool {
    (1_u8..=7).all(|weekday| {
        let mut intervals = schedule
            .windows
            .iter()
            .filter(|window| window.weekdays.contains(&weekday))
            .map(|window| (window.start_minute, window.end_minute))
            .collect::<Vec<_>>();
        intervals.sort_unstable();
        if weekday <= 5 {
            intervals == [(60, 240), (360, 600)]
        } else {
            intervals.is_empty()
        }
    })
}

fn parse_rates(spec: &RateSpec) -> Result<ParsedRates, PricingError> {
    Ok(ParsedRates {
        input: parse_decimal(&spec.input_usd_per_million, "input_usd_per_million")?,
        output: parse_decimal(&spec.output_usd_per_million, "output_usd_per_million")?,
        cache_read: parse_decimal(
            &spec.cache_read_usd_per_million,
            "cache_read_usd_per_million",
        )?,
        cache_write: parse_decimal(
            &spec.cache_write_usd_per_million,
            "cache_write_usd_per_million",
        )?,
    })
}

fn parse_decimal(value: &str, field: &'static str) -> Result<u64, PricingError> {
    let (whole, fraction) = match value.split_once('.') {
        Some((whole, fraction)) => {
            if fraction.is_empty() || fraction.len() > 6 || fraction.contains('.') {
                return Err(PricingError::InvalidDecimal { field });
            }
            (whole, Some(fraction))
        }
        None => (value, None),
    };
    if whole.is_empty()
        || !whole.bytes().all(|byte| byte.is_ascii_digit())
        || fraction.is_some_and(|part| !part.bytes().all(|byte| byte.is_ascii_digit()))
    {
        return Err(PricingError::InvalidDecimal { field });
    }
    let whole_value = parse_ascii_u128(whole).ok_or(PricingError::Overflow)?;
    let mut microcents = whole_value
        .checked_mul(MICROCENTS_PER_USD)
        .ok_or(PricingError::Overflow)?;
    if let Some(part) = fraction {
        let fraction_value = parse_ascii_u128(part).ok_or(PricingError::Overflow)?;
        let scale = 10_u128
            .checked_pow((6 - part.len()) as u32)
            .ok_or(PricingError::Overflow)?;
        microcents = microcents
            .checked_add(
                fraction_value
                    .checked_mul(scale)
                    .ok_or(PricingError::Overflow)?,
            )
            .ok_or(PricingError::Overflow)?;
    }
    u64::try_from(microcents).map_err(|_| PricingError::Overflow)
}

fn parse_ascii_u128(value: &str) -> Option<u128> {
    let mut parsed = 0_u128;
    for byte in value.bytes() {
        parsed = parsed.checked_mul(10)?;
        parsed = parsed.checked_add(u128::from(byte - b'0'))?;
    }
    Some(parsed)
}

fn validate_model_id(model: &str) -> Result<(), PricingError> {
    let bytes = model.as_bytes();
    let first_is_alphanumeric = bytes
        .first()
        .is_some_and(|byte| byte.is_ascii_alphanumeric());
    let safe = first_is_alphanumeric
        && bytes.len() <= MAX_MODEL_ID_BYTES
        && bytes.iter().all(|byte| {
            byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b':' | b'/' | b'-')
        })
        && !model.contains("..")
        && !model.contains("//")
        && !model.ends_with('/');
    if safe {
        Ok(())
    } else {
        Err(PricingError::InvalidModelId)
    }
}

fn validate_schedule(schedule: &WeeklyScheduleSpec) -> Result<(), PricingError> {
    if schedule.windows.is_empty() || schedule.windows.len() > MAX_WINDOWS {
        return Err(PricingError::InvalidSchema { field: "windows" });
    }
    let mut intervals = BTreeMap::<u8, Vec<(u16, u16)>>::new();
    for window in &schedule.windows {
        if window.weekdays.is_empty()
            || window.weekdays.len() > 7
            || window.start_minute >= window.end_minute
            || window.end_minute > 1_440
        {
            return Err(PricingError::InvalidSchema { field: "windows" });
        }
        let mut unique = BTreeSet::new();
        for weekday in &window.weekdays {
            if !(1..=7).contains(weekday) || !unique.insert(*weekday) {
                return Err(PricingError::InvalidSchema { field: "weekdays" });
            }
            intervals
                .entry(*weekday)
                .or_default()
                .push((window.start_minute, window.end_minute));
        }
    }
    for day_intervals in intervals.values_mut() {
        day_intervals.sort_unstable();
        if day_intervals.windows(2).any(|pair| pair[1].0 < pair[0].1) {
            return Err(PricingError::InvalidSchema { field: "windows" });
        }
    }
    Ok(())
}

fn schedule_matches(schedule: &WeeklyScheduleSpec, unix_utc_seconds: i64) -> bool {
    let days = unix_utc_seconds.div_euclid(86_400);
    let weekday = ((days + 3).rem_euclid(7) + 1) as u8;
    let minute = (unix_utc_seconds.rem_euclid(86_400) / 60) as u16;
    schedule.windows.iter().any(|window| {
        window.weekdays.contains(&weekday)
            && minute >= window.start_minute
            && minute < window.end_minute
    })
}

fn quote_rates(rates: &ParsedRates, usage: UsageCounts) -> Result<i64, PricingError> {
    let uncached = usage
        .input
        .checked_sub(usage.cache_read)
        .ok_or(PricingError::InvalidCacheUsage)?;
    let terms = [
        (uncached, rates.input),
        (usage.output, rates.output),
        (usage.cache_read, rates.cache_read),
        (usage.cache_write, rates.cache_write),
    ];
    let numerator = terms
        .into_iter()
        .try_fold(0_u128, |total, (tokens, rate)| {
            let term = u128::from(tokens)
                .checked_mul(u128::from(rate))
                .ok_or(PricingError::Overflow)?;
            total.checked_add(term).ok_or(PricingError::Overflow)
        })?;
    let rounded = numerator
        .checked_add(TOKENS_PER_MILLION / 2)
        .ok_or(PricingError::Overflow)?
        / TOKENS_PER_MILLION;
    i64::try_from(rounded).map_err(|_| PricingError::Overflow)
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct RawCatalog {
    schema_version: String,
    currency: String,
    #[serde(deserialize_with = "deserialize_models")]
    models: Vec<RawModel>,
}

#[derive(Debug, Serialize)]
#[serde(deny_unknown_fields)]
struct RawModel {
    model: String,
    input_usd_per_million: String,
    output_usd_per_million: String,
    cache_read_usd_per_million: String,
    cache_write_usd_per_million: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    max_standard_input_tokens: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    schedule: Option<RawSchedule>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RawModelDecoded {
    model: String,
    input_usd_per_million: String,
    output_usd_per_million: String,
    cache_read_usd_per_million: String,
    cache_write_usd_per_million: String,
    #[serde(default, deserialize_with = "deserialize_absent_only")]
    max_standard_input_tokens: Option<u64>,
    #[serde(default, deserialize_with = "deserialize_absent_only")]
    schedule: Option<RawSchedule>,
}

fn deserialize_absent_only<'de, D, T>(deserializer: D) -> Result<Option<T>, D::Error>
where
    D: Deserializer<'de>,
    T: Deserialize<'de>,
{
    T::deserialize(deserializer).map(Some)
}

impl<'de> Deserialize<'de> for RawModel {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let decoded = RawModelDecoded::deserialize(deserializer)?;
        #[cfg(test)]
        RAW_MODEL_CONSTRUCTIONS.with(|count| count.set(count.get() + 1));
        Ok(Self {
            model: decoded.model,
            input_usd_per_million: decoded.input_usd_per_million,
            output_usd_per_million: decoded.output_usd_per_million,
            cache_read_usd_per_million: decoded.cache_read_usd_per_million,
            cache_write_usd_per_million: decoded.cache_write_usd_per_million,
            max_standard_input_tokens: decoded.max_standard_input_tokens,
            schedule: decoded.schedule,
        })
    }
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct RawSchedule {
    kind: String,
    #[serde(deserialize_with = "deserialize_windows")]
    windows: Vec<RawWindow>,
    peak: RawRates,
}

#[derive(Debug, Serialize)]
#[serde(deny_unknown_fields)]
struct RawWindow {
    weekdays: Vec<u8>,
    start_minute: u16,
    end_minute: u16,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RawWindowDecoded {
    #[serde(deserialize_with = "deserialize_weekdays")]
    weekdays: Vec<u8>,
    start_minute: u16,
    end_minute: u16,
}

impl<'de> Deserialize<'de> for RawWindow {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let decoded = RawWindowDecoded::deserialize(deserializer)?;
        #[cfg(test)]
        RAW_WINDOW_CONSTRUCTIONS.with(|count| count.set(count.get() + 1));
        Ok(Self {
            weekdays: decoded.weekdays,
            start_minute: decoded.start_minute,
            end_minute: decoded.end_minute,
        })
    }
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct RawRates {
    input_usd_per_million: String,
    output_usd_per_million: String,
    cache_read_usd_per_million: String,
    cache_write_usd_per_million: String,
}

fn deserialize_models<'de, D>(deserializer: D) -> Result<Vec<RawModel>, D::Error>
where
    D: Deserializer<'de>,
{
    deserialize_capped_sequence(deserializer, MAX_MODELS, "too many pricing models")
}

fn deserialize_windows<'de, D>(deserializer: D) -> Result<Vec<RawWindow>, D::Error>
where
    D: Deserializer<'de>,
{
    deserialize_capped_sequence(deserializer, MAX_WINDOWS, "too many pricing windows")
}

fn deserialize_weekdays<'de, D>(deserializer: D) -> Result<Vec<u8>, D::Error>
where
    D: Deserializer<'de>,
{
    deserialize_capped_sequence(deserializer, 7, "too many weekdays")
}

fn deserialize_capped_sequence<'de, D, T>(
    deserializer: D,
    limit: usize,
    message: &'static str,
) -> Result<Vec<T>, D::Error>
where
    D: Deserializer<'de>,
    T: Deserialize<'de>,
{
    struct CappedVisitor<T> {
        limit: usize,
        message: &'static str,
        marker: std::marker::PhantomData<T>,
    }

    impl<'de, T> de::Visitor<'de> for CappedVisitor<T>
    where
        T: Deserialize<'de>,
    {
        type Value = Vec<T>;

        fn expecting(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            formatter.write_str("a bounded array")
        }

        fn visit_seq<A>(self, mut sequence: A) -> Result<Self::Value, A::Error>
        where
            A: de::SeqAccess<'de>,
        {
            let mut values = Vec::with_capacity(sequence.size_hint().unwrap_or(0).min(self.limit));
            while values.len() < self.limit {
                match sequence.next_element()? {
                    Some(value) => values.push(value),
                    None => return Ok(values),
                }
            }
            if sequence.next_element::<de::IgnoredAny>()?.is_some() {
                return Err(de::Error::custom(self.message));
            }
            Ok(values)
        }
    }

    deserializer.deserialize_seq(CappedVisitor {
        limit,
        message,
        marker: std::marker::PhantomData,
    })
}

#[cfg(test)]
thread_local! {
    static RAW_MODEL_CONSTRUCTIONS: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
    static RAW_WINDOW_CONSTRUCTIONS: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

#[cfg(test)]
pub(crate) fn reset_raw_construction_counts() {
    RAW_MODEL_CONSTRUCTIONS.with(|count| count.set(0));
    RAW_WINDOW_CONSTRUCTIONS.with(|count| count.set(0));
}

#[cfg(test)]
pub(crate) fn raw_construction_counts() -> (usize, usize) {
    let models = RAW_MODEL_CONSTRUCTIONS.with(std::cell::Cell::get);
    let windows = RAW_WINDOW_CONSTRUCTIONS.with(std::cell::Cell::get);
    (models, windows)
}

impl From<RawModel> for ModelPricingSpec {
    fn from(raw: RawModel) -> Self {
        Self {
            model: raw.model,
            rates: RateSpec {
                input_usd_per_million: raw.input_usd_per_million,
                output_usd_per_million: raw.output_usd_per_million,
                cache_read_usd_per_million: raw.cache_read_usd_per_million,
                cache_write_usd_per_million: raw.cache_write_usd_per_million,
            },
            max_standard_input_tokens: raw.max_standard_input_tokens,
            schedule: raw.schedule.map(|schedule| WeeklyScheduleSpec {
                windows: schedule
                    .windows
                    .into_iter()
                    .map(|window| WeeklyWindowSpec {
                        weekdays: window.weekdays,
                        start_minute: window.start_minute,
                        end_minute: window.end_minute,
                    })
                    .collect(),
                peak: RateSpec {
                    input_usd_per_million: schedule.peak.input_usd_per_million,
                    output_usd_per_million: schedule.peak.output_usd_per_million,
                    cache_read_usd_per_million: schedule.peak.cache_read_usd_per_million,
                    cache_write_usd_per_million: schedule.peak.cache_write_usd_per_million,
                },
            }),
        }
    }
}

impl From<&ModelPricingSpec> for RawModel {
    fn from(spec: &ModelPricingSpec) -> Self {
        Self {
            model: spec.model.clone(),
            input_usd_per_million: spec.rates.input_usd_per_million.clone(),
            output_usd_per_million: spec.rates.output_usd_per_million.clone(),
            cache_read_usd_per_million: spec.rates.cache_read_usd_per_million.clone(),
            cache_write_usd_per_million: spec.rates.cache_write_usd_per_million.clone(),
            max_standard_input_tokens: spec.max_standard_input_tokens,
            schedule: spec.schedule.as_ref().map(|schedule| RawSchedule {
                kind: "utc_weekly_v1".to_string(),
                windows: schedule
                    .windows
                    .iter()
                    .map(|window| RawWindow {
                        weekdays: window.weekdays.clone(),
                        start_minute: window.start_minute,
                        end_minute: window.end_minute,
                    })
                    .collect(),
                peak: RawRates {
                    input_usd_per_million: schedule.peak.input_usd_per_million.clone(),
                    output_usd_per_million: schedule.peak.output_usd_per_million.clone(),
                    cache_read_usd_per_million: schedule.peak.cache_read_usd_per_million.clone(),
                    cache_write_usd_per_million: schedule.peak.cache_write_usd_per_million.clone(),
                },
            }),
        }
    }
}

impl RawSchedule {
    fn validate_kind(&self) -> Result<(), PricingError> {
        if self.kind == "utc_weekly_v1" {
            Ok(())
        } else {
            Err(PricingError::InvalidSchema {
                field: "schedule.kind",
            })
        }
    }
}
