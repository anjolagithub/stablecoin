// programs/stablecoin/src/lib.rs
use anchor_lang::prelude::*;
use anchor_spl::token::{self, Mint, Token, TokenAccount};
use switchboard_v2::{AggregatorAccountData, SwitchboardDecimal};
use std::convert::TryFrom;

declare_id!("22txojBd5nbZtHsvfCkCAzV9RQmFC3kT8ziK2ZPozKRP");

#[program]
pub mod stablecoin {
    use super::*;

    pub fn initialize(
        ctx: Context<Initialize>,
        name: String,
        symbol: String,
        icon_uri: String,
        target_currency: String,
    ) -> Result<()> {
        let stablecoin_config = &mut ctx.accounts.stablecoin_config;
        stablecoin_config.authority = ctx.accounts.authority.key();
        stablecoin_config.mint = ctx.accounts.mint.key();
        stablecoin_config.name = name;
        stablecoin_config.symbol = symbol;
        stablecoin_config.icon_uri = icon_uri;
        stablecoin_config.target_currency = target_currency;
        stablecoin_config.paused = false;
        Ok(())
    }

    pub fn mint_tokens(
        ctx: Context<MintTokens>,
        amount_fiat: u64,
    ) -> Result<()> {
        require!(!ctx.accounts.stablecoin_config.paused, StablecoinError::ProgramPaused);

        // Get the latest price from Switchboard oracle
        let oracle_acc = ctx.accounts.oracle.load()?;
        let sb_decimal: SwitchboardDecimal = oracle_acc.get_result()?.try_into()
            .map_err(|_| error!(StablecoinError::InvalidOracleData))?;
        
        // Convert Switchboard decimal to f64
        let oracle_price = sb_decimal.try_into_f64()
            .map_err(|_| error!(StablecoinError::InvalidOracleData))?;
        
        require!(oracle_price > 0.0, StablecoinError::InvalidOraclePrice);
        
        // Calculate token amount based on oracle price
        let token_amount = ((amount_fiat as f64) / oracle_price) as u64;
        require!(token_amount > 0, StablecoinError::InvalidTokenAmount);

        let seeds = &[
            b"mint".as_ref(),
            &[*ctx.bumps.get("mint_authority").unwrap()],
        ];
        let signer = &[&seeds[..]];

        token::mint_to(
            CpiContext::new_with_signer(
                ctx.accounts.token_program.to_account_info(),
                token::MintTo {
                    mint: ctx.accounts.mint.to_account_info(),
                    to: ctx.accounts.user_token_account.to_account_info(),
                    authority: ctx.accounts.mint_authority.to_account_info(),
                },
                signer,
            ),
            token_amount,
        )?;

        Ok(())
    }

    pub fn redeem_tokens(
        ctx: Context<RedeemTokens>,
        token_amount: u64,
    ) -> Result<()> {
        require!(!ctx.accounts.stablecoin_config.paused, StablecoinError::ProgramPaused);

        token::burn(
            CpiContext::new(
                ctx.accounts.token_program.to_account_info(),
                token::Burn {
                    mint: ctx.accounts.mint.to_account_info(),
                    from: ctx.accounts.user_token_account.to_account_info(),
                    authority: ctx.accounts.user.to_account_info(),
                },
            ),
            token_amount,
        )?;

        Ok(())
    }

    pub fn pause(ctx: Context<AdminFunction>) -> Result<()> {
        let stablecoin_config = &mut ctx.accounts.stablecoin_config;
        require!(
            ctx.accounts.authority.key() == stablecoin_config.authority,
            StablecoinError::Unauthorized
        );
        stablecoin_config.paused = true;
        Ok(())
    }

    pub fn unpause(ctx: Context<AdminFunction>) -> Result<()> {
        let stablecoin_config = &mut ctx.accounts.stablecoin_config;
        require!(
            ctx.accounts.authority.key() == stablecoin_config.authority,
            StablecoinError::Unauthorized
        );
        stablecoin_config.paused = false;
        Ok(())
    }
}

#[derive(Accounts)]
pub struct Initialize<'info> {
    #[account(mut)]
    pub authority: Signer<'info>,
    
    #[account(
        init,
        payer = authority,
        space = StablecoinConfig::LEN
    )]
    pub stablecoin_config: Account<'info, StablecoinConfig>,
    
    #[account(
        init,
        payer = authority,
        mint::decimals = 6,
        mint::authority = mint_authority.key(),
    )]
    pub mint: Account<'info, Mint>,
    
    /// CHECK: PDA for mint authority
    #[account(
        seeds = [b"mint"],
        bump,
    )]
    pub mint_authority: AccountInfo<'info>,
    
    pub system_program: Program<'info, System>,
    pub token_program: Program<'info, Token>,
    pub rent: Sysvar<'info, Rent>,
}

#[derive(Accounts)]
pub struct MintTokens<'info> {
    pub user: Signer<'info>,
    
    #[account(mut)]
    pub stablecoin_config: Account<'info, StablecoinConfig>,
    
    #[account(mut)]
    pub mint: Account<'info, Mint>,
    
    /// CHECK: PDA for mint authority
    #[account(
        seeds = [b"mint"],
        bump,
    )]
    pub mint_authority: AccountInfo<'info>,
    
    #[account(
        mut,
        associated_token::mint = mint,
        associated_token::authority = user,
    )]
    pub user_token_account: Account<'info, TokenAccount>,
    
    #[account(
        constraint = 
            oracle.load()?.latest_confirmed_round.is_some()
            @ StablecoinError::OracleNotInitialized
    )]
    pub oracle: AccountLoader<'info, AggregatorAccountData>,
    
    pub token_program: Program<'info, Token>,
}

#[derive(Accounts)]
pub struct RedeemTokens<'info> {
    pub user: Signer<'info>,
    
    #[account(mut)]
    pub stablecoin_config: Account<'info, StablecoinConfig>,
    
    #[account(mut)]
    pub mint: Account<'info, Mint>,
    
    #[account(
        mut,
        associated_token::mint = mint,
        associated_token::authority = user,
    )]
    pub user_token_account: Account<'info, TokenAccount>,
    
    pub token_program: Program<'info, Token>,
}

#[derive(Accounts)]
pub struct AdminFunction<'info> {
    pub authority: Signer<'info>,
    #[account(mut)]
    pub stablecoin_config: Account<'info, StablecoinConfig>,
}

#[account]
pub struct StablecoinConfig {
    pub authority: Pubkey,
    pub mint: Pubkey,
    pub name: String,
    pub symbol: String,
    pub icon_uri: String,
    pub target_currency: String,
    pub paused: bool,
}

impl StablecoinConfig {
    pub const LEN: usize = 8 + // discriminator
        32 + // authority
        32 + // mint
        64 + // name
        16 + // symbol
        128 + // icon_uri
        16 + // target_currency
        1; // paused
}

#[error_code]
pub enum StablecoinError {
    #[msg("Program is paused")]
    ProgramPaused,
    #[msg("Unauthorized")]
    Unauthorized,
    #[msg("Invalid oracle data")]
    InvalidOracleData,
    #[msg("Invalid oracle price")]
    InvalidOraclePrice,
    #[msg("Invalid token amount")]
    InvalidTokenAmount,
    #[msg("Oracle not initialized")]
    OracleNotInitialized,
}