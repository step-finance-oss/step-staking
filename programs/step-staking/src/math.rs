use crate::StakingError;
use anchor_lang::prelude::*;

const E9: u128 = 1_000_000_000;
const INCINERATOR: Pubkey = Pubkey::from_str_const("1nc1nerator11111111111111111111111111111111");

fn output(value: u128, minimum: u64) -> Result<u64> {
    let value = u64::try_from(value).map_err(|_| error!(StakingError::MathOverflow))?;
    require_gt!(value, 0, StakingError::ZeroOutput);
    require_gte!(value, minimum, StakingError::SlippageExceeded);
    Ok(value)
}

pub fn quote_stake(amount: u64, collateral: u64, supply: u64, minimum: u64) -> Result<u64> {
    require_gt!(amount, 0, StakingError::InvalidAmount);
    require_gt!(minimum, 0, StakingError::InvalidMinimumOutput);
    let shares = if supply == 0 {
        // Unowned rewards in an empty pool still go to its first depositor.
        amount
    } else {
        require_gt!(collateral, 0, StakingError::InvalidPoolState);
        output(
            u128::from(amount) * u128::from(supply) / u128::from(collateral),
            minimum,
        )?
    };
    require_gte!(shares, minimum, StakingError::SlippageExceeded);
    supply
        .checked_add(shares)
        .ok_or(error!(StakingError::MathOverflow))?;
    collateral
        .checked_add(amount)
        .ok_or(error!(StakingError::MathOverflow))?;
    Ok(shares)
}

pub fn quote_unstake(amount: u64, collateral: u64, supply: u64, minimum: u64) -> Result<u64> {
    require_gt!(amount, 0, StakingError::InvalidAmount);
    require_gt!(minimum, 0, StakingError::InvalidMinimumOutput);
    require_gt!(supply, 0, StakingError::InvalidPoolState);
    require_gte!(supply, amount, StakingError::InvalidAmount);
    output(
        u128::from(amount) * u128::from(collateral) / u128::from(supply),
        minimum,
    )
}

pub fn token_authority(
    owner: Pubkey,
    delegate: Option<Pubkey>,
    allowance: u64,
    signer: Pubkey,
    amount: u64,
) -> Result<()> {
    // SPL Token allows permissionless burns for these sinks. Such a burn must
    // never grant the caller a claim on collateral held by this program.
    require!(
        owner != Pubkey::default() && owner != INCINERATOR,
        StakingError::InvalidTokenAuthority
    );
    if owner != signer {
        require!(
            delegate == Some(signer),
            StakingError::InvalidTokenAuthority
        );
        require_gte!(allowance, amount, StakingError::InsufficientDelegation);
    }
    Ok(())
}

pub fn price_for_totals(collateral: u64, supply: u64) -> (u64, String) {
    if supply == 0 {
        return (0, "0".into());
    }
    let scaled = u128::from(collateral) * E9 / u128::from(supply);
    let whole = collateral / supply;
    let fraction = format!(
        "{:09}",
        u128::from(collateral % supply) * E9 / u128::from(supply)
    );
    let fraction = fraction.trim_end_matches('0');
    let decimal = if fraction.is_empty() {
        whole.to_string()
    } else {
        format!("{whole}.{fraction}")
    };
    // Preserve the legacy event's u64 layout without allowing quote logging to
    // block redemptions. The decimal field remains truthful at large ratios.
    (scaled.min(u128::from(u64::MAX)) as u64, decimal)
}
