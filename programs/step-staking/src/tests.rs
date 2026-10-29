use super::*;

fn assert_error<T: std::fmt::Debug>(result: Result<T>, name: &str) {
    match result.unwrap_err() {
        anchor_lang::error::Error::AnchorError(error) => assert_eq!(error.error_name, name),
        other => panic!("Expected {name}, got {other:?}"),
    }
}

#[test]
fn extreme_price_does_not_panic_or_misrepresent_the_decimal_ratio() {
    let price = price_for_totals(18_446_744_074, 1);
    assert_eq!(price.0, u64::MAX);
    assert_eq!(price.1, "18446744074");
}

#[test]
fn maximum_balance_price_is_still_quotable() {
    let price = price_for_totals(u64::MAX, 1);
    assert_eq!(price.0, u64::MAX);
    assert_eq!(price.1, "18446744073709551615");
}

#[test]
fn price_formats_empty_exact_fractional_and_subnanounit_ratios() {
    assert_eq!(price_for_totals(100, 0), (0, "0".into()));
    assert_eq!(price_for_totals(12, 10), (1_200_000_000, "1.2".into()));
    assert_eq!(price_for_totals(1, 3), (333_333_333, "0.333333333".into()));
    assert_eq!(price_for_totals(1, 1000), (1_000_000, "0.001".into()));
    assert_eq!(price_for_totals(1, u64::MAX), (0, "0".into()));
    assert_eq!(
        price_for_totals(u64::MAX, u64::MAX),
        (1_000_000_000, "1".into())
    );
}

#[test]
fn bootstrap_retains_unowned_rewards_without_diluting_existing_receipts() {
    assert_eq!(quote_stake(5000, 0, 0, 5000).unwrap(), 5000);
    assert_eq!(quote_stake(5000, 5000, 0, 5000).unwrap(), 5000);
    assert_eq!(quote_unstake(5000, 10_000, 5000, 10_000).unwrap(), 10_000);
    assert_eq!(quote_stake(1000, 2000, 1000, 500).unwrap(), 500);
}

#[test]
fn zero_and_insolvent_trades_fail_explicitly() {
    assert_error(quote_stake(0, 0, 0, 1), "InvalidAmount");
    assert_error(quote_unstake(0, 0, 0, 1), "InvalidAmount");
    assert_error(quote_stake(1, 0, 0, 0), "InvalidMinimumOutput");
    assert_error(quote_unstake(1, 1, 1, 0), "InvalidMinimumOutput");
    assert_error(quote_stake(1, 0, 1, 1), "InvalidPoolState");
    assert_error(quote_unstake(1, 0, 0, 1), "InvalidPoolState");
    assert_error(quote_unstake(2, 100, 1, 1), "InvalidAmount");
    assert_error(quote_unstake(1, 0, 1, 1), "ZeroOutput");
}

#[test]
fn donations_cannot_force_zero_output_or_bypass_the_callers_minimum() {
    assert_error(quote_stake(1000, 1001, 1, 1), "ZeroOutput");
    assert_error(quote_stake(1999, 1000, 1, 2), "SlippageExceeded");
    assert_eq!(quote_stake(2000, 1000, 1, 2).unwrap(), 2);
    assert_error(quote_stake(100, 0, 0, 101), "SlippageExceeded");
    assert_error(quote_unstake(1, 1000, 1000, 2), "SlippageExceeded");
    assert_error(quote_unstake(1, 1, 1000, 1), "ZeroOutput");
}

#[test]
fn wide_quotes_support_large_balances_and_reject_unrepresentable_results() {
    assert_eq!(quote_stake(u64::MAX, 0, 0, u64::MAX).unwrap(), u64::MAX);
    assert_eq!(
        quote_unstake(u64::MAX, u64::MAX, u64::MAX, u64::MAX).unwrap(),
        u64::MAX
    );
    assert_eq!(
        quote_unstake(1, 18_446_744_074, 1, 18_446_744_074).unwrap(),
        18_446_744_074
    );
    assert_error(quote_stake(2, 1, u64::MAX, 1), "MathOverflow");
    assert_error(quote_stake(1, u64::MAX, 0, 1), "MathOverflow");
    assert_error(quote_stake(1, u64::MAX, u64::MAX, 1), "MathOverflow");
}

#[test]
fn rounding_never_overissues_receipts_or_overpays_a_partial_exit() {
    for collateral in [3u64, 17, 1000, 18_446_744_074] {
        for supply in [1u64, 2, 10] {
            for amount in [1u64, 7, 1000] {
                if let Ok(shares) = quote_stake(amount, collateral, supply, 1) {
                    let paid_value = u128::from(amount) * u128::from(supply);
                    assert!(u128::from(shares) * u128::from(collateral) <= paid_value);
                    assert!(u128::from(shares + 1) * u128::from(collateral) > paid_value);
                }
            }
            for amount in 1..=supply {
                if let Ok(payout) = quote_unstake(amount, collateral, supply, 1) {
                    let redeemed_value = u128::from(amount) * u128::from(collateral);
                    assert!(u128::from(payout) * u128::from(supply) <= redeemed_value);
                    assert!(u128::from(payout + 1) * u128::from(supply) > redeemed_value);
                }
            }
            assert_eq!(
                quote_unstake(supply, collateral, supply, collateral).unwrap(),
                collateral
            );
        }
    }
}

#[test]
fn token_owner_and_approved_delegate_remain_authorized() {
    let owner = Pubkey::new_unique();
    let delegate = Pubkey::new_unique();
    token_authority(owner, None, 0, owner, 100).unwrap();
    token_authority(owner, Some(delegate), 100, delegate, 100).unwrap();
    // The owner's authority does not depend on a delegate's remaining allowance.
    token_authority(owner, Some(delegate), 0, owner, 100).unwrap();
    assert_error(
        token_authority(owner, None, 0, delegate, 1),
        "InvalidTokenAuthority",
    );
    assert_error(
        token_authority(owner, Some(delegate), 99, delegate, 100),
        "InsufficientDelegation",
    );
}

#[test]
fn permissionless_spl_burn_sinks_never_authorize_vault_redemption() {
    let signer = Pubkey::new_unique();
    for owner in [
        Pubkey::default(),
        Pubkey::from_str_const("1nc1nerator11111111111111111111111111111111"),
    ] {
        assert_error(
            token_authority(owner, None, 0, signer, 1),
            "InvalidTokenAuthority",
        );
        assert_error(
            token_authority(owner, Some(signer), 100, signer, 1),
            "InvalidTokenAuthority",
        );
    }
}
