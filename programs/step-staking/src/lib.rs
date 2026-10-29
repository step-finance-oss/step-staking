///A Solana version of the xSushi contract for STEP
/// https://github.com/sushiswap/sushiswap/blob/master/contracts/SushiBar.sol
use anchor_lang::prelude::*;
use anchor_spl::{
    associated_token::AssociatedToken,
    token::{self, spl_token::instruction::AuthorityType, Mint, Token, TokenAccount},
};
mod math;
pub use math::price_for_totals;
use math::{quote_stake, quote_unstake, token_authority};

#[cfg(not(feature = "test-id"))]
declare_id!("Stk5NCWomVN3itaFjLu382u9ibb5jMSHEsh6CuhaGjB");
#[cfg(feature = "test-id")]
declare_id!("TesT35sGptoswsVkcLpUUe6U2iTJZE59on1Jno8Vdpg");

#[cfg(not(feature = "local-testing"))]
pub mod constants {
    pub const STEP_TOKEN_MINT_PUBKEY: &str = "StepAscQoEioFxxWGnh2sLBDFp9d8rvKz2Yp39iDpyT";
    pub const X_STEP_TOKEN_MINT_PUBKEY: &str = "xStpgUCss9piqeFUk2iLVcvJEGhAdJxJQuwLkXP555G";
}

#[cfg(feature = "local-testing")]
pub mod constants {
    pub const STEP_TOKEN_MINT_PUBKEY: &str = "teST1ieLrLdr4MJPZ7i8mgSCLQ7rTrPRjNnyFdHFaz9";
    pub const X_STEP_TOKEN_MINT_PUBKEY: &str = "TestZ4qmw6fCo1uK9oJbobWDgj1sME6hR1ssWQnyjxM";
}

#[program]
pub mod step_staking {
    use super::*;

    pub fn initialize(_ctx: Context<Initialize>, _nonce: u8) -> Result<()> {
        Ok(())
    }

    /// In the case that somehow someone sends token to the vaults address but
    /// uses an ATA and the funds are at the ATA of the vault, this crank pulls
    /// it back out and into the vault.
    /// No authority or signer is required for this call.  It is a crank which
    /// always does the exact same thing regardless of caller.
    pub fn withdraw_nested(ctx: Context<WithdrawNested>) -> Result<()> {
        //compute vault signer seeds
        let token_mint_key = ctx.accounts.token_mint.key();
        let seeds = &[token_mint_key.as_ref(), &[ctx.bumps.token_vault]];
        let signer = &[&seeds[..]];

        //transfer from vault ata to vault
        let cpi_ctx = CpiContext::new_with_signer(
            token::ID,
            token::Transfer {
                from: ctx.accounts.token_vault_nested_ata.to_account_info(),
                to: ctx.accounts.token_vault.to_account_info(),
                authority: ctx.accounts.token_vault.to_account_info(),
            },
            signer,
        );
        token::transfer(cpi_ctx, ctx.accounts.token_vault_nested_ata.amount)?;

        //close the token account
        let cpi_ctx = CpiContext::new_with_signer(
            token::ID,
            token::CloseAccount {
                account: ctx.accounts.token_vault_nested_ata.to_account_info(),
                destination: ctx.accounts.refundee.to_account_info(),
                authority: ctx.accounts.token_vault.to_account_info(),
            },
            signer,
        );
        token::close_account(cpi_ctx)?;

        Ok(())
    }

    /// Set the mint authority of xSTEP to the mint authority of the STEP token
    /// This would be used for some rescue type operation, or deprecation of this program
    /// New deposits stop until the receipt mint authority is restored to the vault
    /// PDA. Existing holders can still redeem. The reclaimed authority is trusted:
    /// minting unbacked receipts would dilute every remaining holder.
    pub fn reclaim_mint_authority(ctx: Context<ReclaimMintAuthority>, nonce: u8) -> Result<()> {
        let authority = Option::<Pubkey>::from(ctx.accounts.token_mint.mint_authority)
            .ok_or(error!(StakingError::MissingMintAuthority))?;
        require_keys_eq!(
            authority,
            ctx.accounts.authority.key(),
            StakingError::InvalidAuthority
        );
        let token_mint_key = ctx.accounts.token_mint.key();
        let seeds = &[token_mint_key.as_ref(), &[nonce]];
        let signer = [&seeds[..]];

        let cpi_ctx = CpiContext::new_with_signer(
            token::ID,
            token::SetAuthority {
                current_authority: ctx.accounts.token_vault.to_account_info(),
                account_or_mint: ctx.accounts.x_token_mint.to_account_info(),
            },
            &signer,
        );
        token::set_authority(cpi_ctx, AuthorityType::MintTokens, Some(authority))?;
        Ok(())
    }

    pub fn stake(ctx: Context<Stake>, nonce: u8, amount: u64, minimum_shares: u64) -> Result<()> {
        let total_token = ctx.accounts.token_vault.amount;
        let total_x_token = ctx.accounts.x_token_mint.supply;
        token_authority(
            ctx.accounts.token_from.owner,
            Option::<Pubkey>::from(ctx.accounts.token_from.delegate),
            ctx.accounts.token_from.delegated_amount,
            ctx.accounts.token_from_authority.key(),
            amount,
        )?;
        let shares = quote_stake(amount, total_token, total_x_token, minimum_shares)?;
        let old_price = get_price(&ctx.accounts.token_vault, &ctx.accounts.x_token_mint);

        let token_mint_key = ctx.accounts.token_mint.key();
        let seeds = &[token_mint_key.as_ref(), &[nonce]];
        let signer = [&seeds[..]];

        let cpi_ctx = CpiContext::new_with_signer(
            token::ID,
            token::MintTo {
                mint: ctx.accounts.x_token_mint.to_account_info(),
                to: ctx.accounts.x_token_to.to_account_info(),
                authority: ctx.accounts.token_vault.to_account_info(),
            },
            &signer,
        );
        token::mint_to(cpi_ctx, shares)?;

        //transfer the users tokens to the vault
        let cpi_ctx = CpiContext::new(
            token::ID,
            token::Transfer {
                from: ctx.accounts.token_from.to_account_info(),
                to: ctx.accounts.token_vault.to_account_info(),
                authority: ctx.accounts.token_from_authority.to_account_info(),
            },
        );
        token::transfer(cpi_ctx, amount)?;

        ctx.accounts.token_vault.reload()?;
        ctx.accounts.x_token_mint.reload()?;

        let new_price = get_price(&ctx.accounts.token_vault, &ctx.accounts.x_token_mint);

        emit!(PriceChange {
            old_step_per_xstep_e9: old_price.0,
            old_step_per_xstep: old_price.1,
            new_step_per_xstep_e9: new_price.0,
            new_step_per_xstep: new_price.1,
        });

        Ok(())
    }

    pub fn unstake(
        ctx: Context<Unstake>,
        nonce: u8,
        amount: u64,
        minimum_tokens: u64,
    ) -> Result<()> {
        let total_token = ctx.accounts.token_vault.amount;
        let total_x_token = ctx.accounts.x_token_mint.supply;
        token_authority(
            ctx.accounts.x_token_from.owner,
            Option::<Pubkey>::from(ctx.accounts.x_token_from.delegate),
            ctx.accounts.x_token_from.delegated_amount,
            ctx.accounts.x_token_from_authority.key(),
            amount,
        )?;
        let payout = quote_unstake(amount, total_token, total_x_token, minimum_tokens)?;
        let old_price = get_price(&ctx.accounts.token_vault, &ctx.accounts.x_token_mint);

        //burn what is being sent
        let cpi_ctx = CpiContext::new(
            token::ID,
            token::Burn {
                mint: ctx.accounts.x_token_mint.to_account_info(),
                from: ctx.accounts.x_token_from.to_account_info(),
                authority: ctx.accounts.x_token_from_authority.to_account_info(),
            },
        );
        token::burn(cpi_ctx, amount)?;

        //compute vault signer seeds
        let token_mint_key = ctx.accounts.token_mint.key();
        let seeds = &[token_mint_key.as_ref(), &[nonce]];
        let signer = &[&seeds[..]];

        //transfer from vault to user
        let cpi_ctx = CpiContext::new_with_signer(
            token::ID,
            token::Transfer {
                from: ctx.accounts.token_vault.to_account_info(),
                to: ctx.accounts.token_to.to_account_info(),
                authority: ctx.accounts.token_vault.to_account_info(),
            },
            signer,
        );
        token::transfer(cpi_ctx, payout)?;

        ctx.accounts.token_vault.reload()?;
        ctx.accounts.x_token_mint.reload()?;

        let new_price = get_price(&ctx.accounts.token_vault, &ctx.accounts.x_token_mint);

        emit!(PriceChange {
            old_step_per_xstep_e9: old_price.0,
            old_step_per_xstep: old_price.1,
            new_step_per_xstep_e9: new_price.0,
            new_step_per_xstep: new_price.1,
        });

        Ok(())
    }

    pub fn emit_price(ctx: Context<EmitPrice>) -> Result<()> {
        let price = get_price(&ctx.accounts.token_vault, &ctx.accounts.x_token_mint);
        emit!(Price {
            step_per_xstep_e9: price.0,
            step_per_xstep: price.1,
        });
        Ok(())
    }
}

pub fn get_price<'info>(
    vault: &Account<'info, TokenAccount>,
    mint: &Account<'info, Mint>,
) -> (u64, String) {
    price_for_totals(vault.amount, mint.supply)
}

#[derive(Accounts)]
pub struct Initialize<'info> {
    #[account(
        address = constants::STEP_TOKEN_MINT_PUBKEY.parse::<Pubkey>().unwrap(),
    )]
    pub token_mint: Box<Account<'info, Mint>>,

    #[account(
        init,
        payer = initializer,
        token::mint = token_mint,
        token::authority = token_vault, //the PDA address is both the vault account and the authority (and event the mint authority)
        seeds = [ constants::STEP_TOKEN_MINT_PUBKEY.parse::<Pubkey>().unwrap().as_ref() ],
        bump,
    )]
    ///the not-yet-created, derived token vault pubkey
    pub token_vault: Box<Account<'info, TokenAccount>>,

    #[account(mut)]
    ///pays rent on the initializing accounts
    pub initializer: Signer<'info>,

    ///used by anchor for init of the token
    pub system_program: Program<'info, System>,
    pub token_program: Program<'info, Token>,
    pub rent: Sysvar<'info, Rent>,
}

#[derive(Accounts)]
pub struct WithdrawNested<'info> {
    #[account(mut)]
    refundee: SystemAccount<'info>,

    #[account(
        address = constants::STEP_TOKEN_MINT_PUBKEY.parse::<Pubkey>().unwrap(),
    )]
    pub token_mint: Box<Account<'info, Mint>>,

    #[account(
        mut,
        seeds = [ token_mint.key().as_ref() ],
        bump,
    )]
    pub token_vault: Box<Account<'info, TokenAccount>>,

    #[account(
        mut,
        associated_token::mint = token_mint,
        associated_token::authority = token_vault,
    )]
    pub token_vault_nested_ata: Box<Account<'info, TokenAccount>>,

    pub token_program: Program<'info, Token>,
    pub associated_token_program: Program<'info, AssociatedToken>,
}

#[derive(Accounts)]
#[instruction(nonce: u8)]
pub struct ReclaimMintAuthority<'info> {
    #[account(
        address = constants::STEP_TOKEN_MINT_PUBKEY.parse::<Pubkey>().unwrap(),
    )]
    pub token_mint: Box<Account<'info, Mint>>,

    #[account(
        mut,
        address = constants::X_STEP_TOKEN_MINT_PUBKEY.parse::<Pubkey>().unwrap(),
    )]
    pub x_token_mint: Box<Account<'info, Mint>>,

    #[account(
        mut,
        seeds = [ token_mint.key().as_ref() ],
        bump = nonce,
    )]
    pub token_vault: Box<Account<'info, TokenAccount>>,

    #[account(mut)]
    ///the mint authority of the step token
    pub authority: Signer<'info>,
    pub token_program: Program<'info, Token>,
}

#[derive(Accounts)]
#[instruction(nonce: u8)]
pub struct Stake<'info> {
    #[account(
        address = constants::STEP_TOKEN_MINT_PUBKEY.parse::<Pubkey>().unwrap(),
    )]
    pub token_mint: Box<Account<'info, Mint>>,

    #[account(
        mut,
        address = constants::X_STEP_TOKEN_MINT_PUBKEY.parse::<Pubkey>().unwrap(),
    )]
    pub x_token_mint: Box<Account<'info, Mint>>,

    #[account(mut)]
    //the token account to withdraw from
    pub token_from: Box<Account<'info, TokenAccount>>,

    //the authority allowed to transfer from token_from
    pub token_from_authority: Signer<'info>,

    #[account(
        mut,
        seeds = [ token_mint.key().as_ref() ],
        bump = nonce,
    )]
    pub token_vault: Box<Account<'info, TokenAccount>>,

    #[account(mut)]
    //the token account to send xtoken
    pub x_token_to: Box<Account<'info, TokenAccount>>,

    pub token_program: Program<'info, Token>,
}

#[derive(Accounts)]
#[instruction(nonce: u8)]
pub struct Unstake<'info> {
    #[account(
        address = constants::STEP_TOKEN_MINT_PUBKEY.parse::<Pubkey>().unwrap(),
    )]
    pub token_mint: Box<Account<'info, Mint>>,

    #[account(
        mut,
        address = constants::X_STEP_TOKEN_MINT_PUBKEY.parse::<Pubkey>().unwrap(),
    )]
    pub x_token_mint: Box<Account<'info, Mint>>,

    #[account(mut)]
    //the token account to withdraw from
    pub x_token_from: Box<Account<'info, TokenAccount>>,

    //the authority allowed to transfer from x_token_from
    pub x_token_from_authority: Signer<'info>,

    #[account(
        mut,
        seeds = [ token_mint.key().as_ref() ],
        bump = nonce,
    )]
    pub token_vault: Box<Account<'info, TokenAccount>>,

    #[account(mut)]
    //the token account to send token
    pub token_to: Box<Account<'info, TokenAccount>>,

    pub token_program: Program<'info, Token>,
}

#[derive(Accounts)]
pub struct EmitPrice<'info> {
    #[account(
        address = constants::STEP_TOKEN_MINT_PUBKEY.parse::<Pubkey>().unwrap(),
    )]
    pub token_mint: Box<Account<'info, Mint>>,

    #[account(
        mut,
        address = constants::X_STEP_TOKEN_MINT_PUBKEY.parse::<Pubkey>().unwrap(),
    )]
    pub x_token_mint: Box<Account<'info, Mint>>,

    #[account(
        mut,
        seeds = [ token_mint.key().as_ref() ],
        bump,
    )]
    pub token_vault: Box<Account<'info, TokenAccount>>,
}

#[event]
pub struct PriceChange {
    pub old_step_per_xstep_e9: u64,
    pub old_step_per_xstep: String,
    pub new_step_per_xstep_e9: u64,
    pub new_step_per_xstep: String,
}

#[event]
pub struct Price {
    pub step_per_xstep_e9: u64,
    pub step_per_xstep: String,
}

#[cfg(test)]
mod tests;

#[error_code]
pub enum StakingError {
    #[msg("Amount must be positive")]
    InvalidAmount,
    #[msg("Minimum output must be positive")]
    InvalidMinimumOutput,
    #[msg("Trade would produce zero output")]
    ZeroOutput,
    #[msg("Output is below the requested minimum")]
    SlippageExceeded,
    #[msg("Arithmetic result exceeds token limits")]
    MathOverflow,
    #[msg("Outstanding shares have no backing collateral")]
    InvalidPoolState,
    #[msg("Signer is not an authorized owner or delegate")]
    InvalidTokenAuthority,
    #[msg("Delegate allowance is insufficient")]
    InsufficientDelegation,
    #[msg("Underlying mint authority has been revoked")]
    MissingMintAuthority,
    #[msg("Signer is not the underlying mint authority")]
    InvalidAuthority,
}
