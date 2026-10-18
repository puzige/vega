//! Headless token accounting, strict pricing catalogs, and checked USD costs.

mod catalog;
mod error;
mod persistence;

pub use catalog::{
    ModelPricingSpec, PricingCatalog, PricingProfile, PricingQuote, RateSpec, UsageCounts,
    WeeklyScheduleSpec, WeeklyWindowSpec,
};
pub use error::PricingError;
pub use persistence::{
    CatalogLoad, CatalogSnapshot, load_catalog, load_catalog_snapshot, load_or_seed_catalog,
    save_catalog_atomic,
};

#[cfg(test)]
mod tests;
