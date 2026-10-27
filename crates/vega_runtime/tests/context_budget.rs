use vega_runtime::{ContextBudget, ContextCheck, ContextEstimate};

fn estimate(input_tokens: u64) -> ContextEstimate {
    ContextEstimate {
        input_tokens,
        message_count: 1,
        tool_count: 0,
        image_count: 0,
    }
}

#[test]
fn policy_trigger_does_not_replace_hard_capacity() -> Result<(), Box<dyn std::error::Error>> {
    let budget = ContextBudget::new(1_000_000, 20_000, true)?.with_policy_input_limit(180_000)?;
    assert_eq!(budget.trigger_tokens()?, 167_000);
    assert_eq!(budget.warning_tokens()?, 147_000);
    assert_eq!(budget.blocking_tokens()?, 977_000);
    assert_eq!(budget.precompute_tokens()?, 144_000);
    assert_eq!(budget.check(estimate(166_999))?, ContextCheck::Within);
    assert_eq!(budget.check(estimate(167_000))?, ContextCheck::Triggered);
    assert_eq!(budget.check(estimate(976_999))?, ContextCheck::Triggered);
    assert_eq!(budget.check(estimate(977_000))?, ContextCheck::OverLimit);
    Ok(())
}

#[test]
fn summary_cap_does_not_reduce_primary_response_capacity() -> Result<(), Box<dyn std::error::Error>>
{
    let budget = ContextBudget::new(428_000, 128_000, true)?
        .with_model_limits(Some(1_000_000), Some(32_000))?;
    assert_eq!(budget.input_budget(), 300_000);
    assert_eq!(budget.output_reserve(), 32_000);
    assert_eq!(budget.summary_output_reserve(), 20_000);
    assert_eq!(budget.total_limit(), 332_000);
    Ok(())
}

#[test]
fn frozen_model_window_bounds_configured_input_and_is_idempotent()
-> Result<(), Box<dyn std::error::Error>> {
    let budget = ContextBudget::new(428_000, 128_000, true)?
        .with_model_limits(Some(200_000), Some(32_000))?;
    assert_eq!(budget.input_budget(), 168_000);
    assert_eq!(budget.policy_input_limit(), 168_000);
    assert_eq!(budget.trigger_tokens()?, 155_000);
    assert_eq!(budget.blocking_tokens()?, 165_000);
    assert_eq!(
        budget.with_model_limits(Some(200_000), Some(32_000))?,
        budget
    );
    Ok(())
}

#[test]
fn disabling_auto_preserves_hard_block_and_moves_warning() -> Result<(), Box<dyn std::error::Error>>
{
    let budget = ContextBudget::new(1_000_000, 20_000, true)?
        .with_policy_input_limit(180_000)?
        .without_automatic_compaction();
    assert_eq!(budget.warning_tokens()?, 160_000);
    assert_eq!(budget.check(estimate(167_000))?, ContextCheck::Within);
    assert_eq!(budget.check(estimate(977_000))?, ContextCheck::OverLimit);
    Ok(())
}

#[test]
fn unknown_model_capacity_preserves_explicit_budget() -> Result<(), Box<dyn std::error::Error>> {
    let budget = ContextBudget::new(100_000, 10_000, true)?;
    assert_eq!(budget.with_model_limits(None, None)?, budget);
    assert_eq!(budget.with_model_limits(Some(0), Some(0))?, budget);
    assert!(budget.with_model_limits(Some(10_000), None).is_err());
    assert!(budget.with_policy_input_limit(90_001).is_err());
    assert!(budget.with_policy_input_limit(0).is_err());
    Ok(())
}

#[test]
fn small_windows_have_nonzero_thresholds_and_install_headroom()
-> Result<(), Box<dyn std::error::Error>> {
    let mut previous_trigger = 0;
    let mut previous_block = 0;
    for input in [
        1, 2, 10, 100, 2_999, 3_000, 3_001, 12_999, 13_000, 13_001, 59_999, 60_000, 60_001, 64_999,
        65_000, 65_001,
    ] {
        let budget = ContextBudget::new(input + 1, 1, true)?;
        assert!(budget.trigger_tokens()? >= previous_trigger);
        assert!(budget.blocking_tokens()? >= previous_block);
        previous_trigger = budget.trigger_tokens()?;
        previous_block = budget.blocking_tokens()?;
        assert!(budget.trigger_tokens()? > 0);
        assert!(budget.blocking_tokens()? > 0);
        assert!(budget.target_tokens()? < budget.trigger_tokens()?);
        assert!(budget.target_tokens()? < budget.blocking_tokens()?);
        assert!(budget.precompute_tokens()? <= budget.trigger_tokens()?);
        assert_eq!(budget.check(estimate(input))?, ContextCheck::OverLimit);
    }
    Ok(())
}
