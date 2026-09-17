use anchor_lang::prelude::*;
use anchor_lang::system_program;

declare_id!("9nKpoMMP2ZX2bRudcXjpAS4VtSJBxiZ8wsM69LAkHikv");

#[program]
pub mod maat {
    use super::*;

    pub fn initialize(ctx: Context<Initialize>, admin: Pubkey, orchestrator: Pubkey) -> Result<()> {
        let state = &mut ctx.accounts.program_state;
        state.admin = admin;
        state.orchestrator = orchestrator;
        state.paused = false;
        Ok(())
    }

    pub fn submit_intent(
        ctx: Context<SubmitIntent>,
        amount: u64,
        destination: Pubkey,
        expiry: i64,
        slippage_bps: u16,
        source_chain_id: u64,
    ) -> Result<()> {
        require!(
            !ctx.accounts.program_state.paused,
            MeridianError::ProgramPaused
        );

        let nonce = ctx.accounts.program_state.intent_nonce;

        let intent = &mut ctx.accounts.intent;
        let clock = Clock::get()?;

        intent.owner = ctx.accounts.owner.key();
        intent.amount = amount;
        intent.destination = destination;
        intent.expiry = expiry;
        intent.slippage_bps = slippage_bps;
        intent.source_chain_id = source_chain_id;
        intent.status = IntentStatus::Pending;
        intent.zk_proof_hash = [0u8; 32];
        intent.created_at = clock.unix_timestamp;
        intent.settled_at = 0;
        intent.settled_amount = 0;
        intent.nonce = nonce;
        intent.bump = ctx.bumps.intent;

        let intent_id = intent.key();
        // Escrow the settlement amount into the Intent PDA so receive_settlement
        // has real lamports to forward to the destination.
        system_program::transfer(
            CpiContext::new(
                ctx.accounts.system_program.to_account_info(),
                system_program::Transfer {
                    from: ctx.accounts.owner.to_account_info(),
                    to: ctx.accounts.intent.to_account_info(),
                },
            ),
            amount,
        )?;

        ctx.accounts.program_state.intent_nonce = ctx
            .accounts
            .program_state
            .intent_nonce
            .checked_add(1)
            .ok_or(MeridianError::Overflow)?;

        emit!(IntentSubmitted {
            intent_id,
            owner: ctx.accounts.owner.key(),
            amount,
            destination,
            expiry,
            source_chain_id,
        });

        Ok(())
    }
    pub fn receive_settlement(
        ctx: Context<ReceiveSettlement>,
        zk_proof_hash: [u8; 32],
        settlement_amount: u64,
    ) -> Result<()> {
        require!(
            !ctx.accounts.program_state.paused,
            MeridianError::ProgramPaused
        );
        require_keys_eq!(
            ctx.accounts.orchestrator.key(),
            ctx.accounts.program_state.orchestrator,
            MeridianError::NotOrchestrator
        );

        let intent = &mut ctx.accounts.intent;
        require!(
            intent.status == IntentStatus::Pending,
            MeridianError::IntentNotPending
        );

        let clock = Clock::get()?;
        require!(
            clock.unix_timestamp < intent.expiry,
            MeridianError::IntentExpired
        );

        require_eq!(
            ctx.accounts.circuit_breaker.channel_id,
            intent.source_chain_id,
            MeridianError::ChannelMismatch
        );
        require!(
            !ctx.accounts.circuit_breaker.halted,
            MeridianError::ChannelHalted
        );

        require_keys_eq!(
            intent.destination,
            ctx.accounts.destination.key(),
            MeridianError::AccountSubstitution
        );
        require_keys_eq!(
            ctx.accounts.owner.key(),
            intent.owner,
            MeridianError::Unauthorized
        );
        let slippage_deduction = (intent.amount as u128)
            .checked_mul(intent.slippage_bps as u128)
            .ok_or(MeridianError::Overflow)?
            .checked_div(10_000)
            .ok_or(MeridianError::Overflow)? as u64;
        let min_acceptable = intent
            .amount
            .checked_sub(slippage_deduction)
            .ok_or(MeridianError::Overflow)?;
        require!(
            settlement_amount >= min_acceptable,
            MeridianError::SlippageExceeded
        );
        // settlement_amount can't exceed what the owner actually escrowed.
        require!(
            settlement_amount <= intent.amount,
            MeridianError::SettlementExceedsEscrow
        );

        intent.status = IntentStatus::Settled;
        intent.settled_at = clock.unix_timestamp;
        intent.zk_proof_hash = zk_proof_hash;
        intent.settled_amount = settlement_amount;

        let intent_info = intent.to_account_info();
        **intent_info.try_borrow_mut_lamports()? = intent_info
            .lamports()
            .checked_sub(settlement_amount)
            .ok_or(MeridianError::InsufficientFunds)?;
        **ctx.accounts.destination.try_borrow_mut_lamports()? = ctx
            .accounts
            .destination
            .lamports()
            .checked_add(settlement_amount)
            .ok_or(MeridianError::Overflow)?;

        let refund_amount = intent
            .amount
            .checked_sub(settlement_amount)
            .ok_or(MeridianError::Overflow)?;
        if refund_amount > 0 {
            let intent_info = intent.to_account_info();
            **intent_info.try_borrow_mut_lamports()? = intent_info
                .lamports()
                .checked_sub(refund_amount)
                .ok_or(MeridianError::InsufficientFunds)?;
            **ctx.accounts.owner.try_borrow_mut_lamports()? = ctx
                .accounts
                .owner
                .lamports()
                .checked_add(refund_amount)
                .ok_or(MeridianError::Overflow)?;
        }

        emit!(IntentSettled {
            intent_id: intent.key(),
            destination: ctx.accounts.destination.key(),
            amount: settlement_amount,
            zk_proof_hash,
            settled_at: intent.settled_at,
            refund_amount,
        });

        Ok(())
    }

    pub fn cancel_intent(ctx: Context<CancelIntent>) -> Result<()> {
        let intent = &mut ctx.accounts.intent;

        require_keys_eq!(
            ctx.accounts.owner.key(),
            intent.owner,
            MeridianError::NotIntentOwner
        );
        require!(
            intent.status == IntentStatus::Pending,
            MeridianError::IntentNotPending
        );

        let clock = Clock::get()?;
        require!(
            clock.unix_timestamp > intent.expiry || ctx.accounts.program_state.paused,
            MeridianError::IntentNotCancellable
        );

        let amount = intent.amount;
        intent.status = IntentStatus::Expired;

        let intent_info = intent.to_account_info();
        **intent_info.try_borrow_mut_lamports()? = intent_info
            .lamports()
            .checked_sub(amount)
            .ok_or(MeridianError::InsufficientFunds)?;
        **ctx.accounts.owner.try_borrow_mut_lamports()? = ctx
            .accounts
            .owner
            .lamports()
            .checked_add(amount)
            .ok_or(MeridianError::Overflow)?;

        emit!(IntentCancelled {
            intent: intent.key(),
            owner: intent.owner,
            amount,
            timestamp: clock.unix_timestamp,
        });

        Ok(())
    }

    pub fn set_circuit_breaker(
        ctx: Context<SetCircuitBreaker>,
        channel_id: u64,
        halted: bool,
    ) -> Result<()> {
        require_keys_eq!(
            ctx.accounts.admin.key(),
            ctx.accounts.program_state.admin,
            MeridianError::NotAdmin
        );

        let cb = &mut ctx.accounts.circuit_breaker;
        cb.channel_id = channel_id;
        cb.halted = halted;
        cb.authority = ctx.accounts.admin.key();
        cb.bump = ctx.bumps.circuit_breaker;

        emit!(CircuitBreakerUpdated { channel_id, halted });

        Ok(())
    }

    pub fn pause(ctx: Context<AdminOnly>) -> Result<()> {
        require_keys_eq!(
            ctx.accounts.admin.key(),
            ctx.accounts.program_state.admin,
            MeridianError::NotAdmin
        );
        ctx.accounts.program_state.paused = true;
        Ok(())
    }

    pub fn unpause(ctx: Context<AdminOnly>) -> Result<()> {
        require_keys_eq!(
            ctx.accounts.admin.key(),
            ctx.accounts.program_state.admin,
            MeridianError::NotAdmin
        );
        ctx.accounts.program_state.paused = false;
        Ok(())
    }
}

// ------------------- Accounts -------------------

#[derive(Accounts)]
pub struct Initialize<'info> {
    #[account(
      init,
      payer = deployer,
      space = 8 + ProgramState::INIT_SPACE,
      seeds = [b"program_state"],
      bump
)]
    pub program_state: Account<'info, ProgramState>,

    #[account(mut)]
    pub deployer: Signer<'info>,

    pub system_program: Program<'info, System>,
}

#[derive(Accounts)]
#[instruction(amount: u64, destination: Pubkey, expiry: i64, slippage_bps: u16, source_chain_id: u64)]
pub struct SubmitIntent<'info> {
    #[account(mut, seeds = [b"program_state"], bump)]
    pub program_state: Account<'info, ProgramState>,

    #[account(
init,
      payer = owner,
      space = 8 + Intent::INIT_SPACE,
      seeds = [b"intent", owner.key().as_ref(), &program_state.intent_nonce.to_le_bytes()],
      bump
)]
    pub intent: Account<'info, Intent>,

    #[account(mut)]
    pub owner: Signer<'info>,

    pub system_program: Program<'info, System>,
}

#[derive(Accounts)]
pub struct ReceiveSettlement<'info> {
    #[account(seeds = [b"program_state"], bump)]
    pub program_state: Account<'info, ProgramState>,

    #[account(
      mut,
      seeds = [b"intent", intent.owner.as_ref(), &intent.nonce.to_le_bytes()],
      bump = intent.bump
)]
    pub intent: Account<'info, Intent>,

    #[account(
      seeds = [b"cb", circuit_breaker.channel_id.to_le_bytes().as_ref()],
      bump = circuit_breaker.bump
)]
    pub circuit_breaker: Account<'info, CircuitBreaker>,
    pub orchestrator: Signer<'info>,

    /// CHECK: validated against intent.destination via require_keys_eq
    #[account(mut)]
    pub destination: UncheckedAccount<'info>,

    /// CHECK: validated against intent.owner via require_keys_eq; receives any slippage refund
    #[account(mut)]
    pub owner: UncheckedAccount<'info>,
}

#[derive(Accounts)]
pub struct CancelIntent<'info> {
    // Not in the requested account list, but required to read `paused` for
    // the cancel-while-paused escape hatch.
    #[account(seeds = [b"program_state"], bump)]
    pub program_state: Account<'info, ProgramState>,

    #[account(
      mut,
      seeds = [b"intent", intent.owner.as_ref(), &intent.nonce.to_le_bytes()],
      bump = intent.bump
)]
    pub intent: Account<'info, Intent>,

    #[account(mut)]
    pub owner: Signer<'info>,

    pub system_program: Program<'info, System>,
}

#[derive(Accounts)]
#[instruction(channel_id: u64)]
pub struct SetCircuitBreaker<'info> {
    #[account(seeds = [b"program_state"], bump)]
    pub program_state: Account<'info, ProgramState>,

    #[account(
      init_if_needed,
      payer = admin,
      space = 8 + CircuitBreaker::INIT_SPACE,
      seeds = [b"cb", channel_id.to_le_bytes().as_ref()],
      bump
)]
    pub circuit_breaker: Account<'info, CircuitBreaker>,

    #[account(mut)]
    pub admin: Signer<'info>,

    pub system_program: Program<'info, System>,
}

#[derive(Accounts)]
pub struct AdminOnly<'info> {
    #[account(mut, seeds = [b"program_state"], bump)]
    pub program_state: Account<'info, ProgramState>,

    pub admin: Signer<'info>,
}

// ------------------- State -------------------

#[account]
#[derive(InitSpace)]
pub struct ProgramState {
    pub admin: Pubkey,
    pub orchestrator: Pubkey,
    pub paused: bool,
    pub intent_nonce: u64,
}

#[account]
#[derive(InitSpace)]
pub struct Intent {
    pub owner: Pubkey,
    pub amount: u64,
    pub destination: Pubkey,
    pub expiry: i64,
    pub slippage_bps: u16,
    pub source_chain_id: u64,
    pub status: IntentStatus,
    pub zk_proof_hash: [u8; 32],
    pub created_at: i64,
    pub settled_at: i64,
    pub settled_amount: u64,
    pub nonce: u64,
    pub bump: u8,
}

#[account]
#[derive(InitSpace)]
pub struct CircuitBreaker {
    pub channel_id: u64,
    pub halted: bool,
    pub authority: Pubkey,
    pub bump: u8,
}
#[derive(AnchorSerialize, AnchorDeserialize, Clone, Copy, PartialEq, Eq, InitSpace)]
pub enum IntentStatus {
    Pending,
    Settled,
    Expired,
    Slashed,
}

// ------------------- Events -------------------

#[event]
pub struct IntentSubmitted {
    pub intent_id: Pubkey,
    pub owner: Pubkey,
    pub amount: u64,
    pub destination: Pubkey,
    pub expiry: i64,
    pub source_chain_id: u64,
}

#[event]
pub struct IntentSettled {
    pub intent_id: Pubkey,
    pub destination: Pubkey,
    pub amount: u64,
    pub zk_proof_hash: [u8; 32],
    pub settled_at: i64,
    pub refund_amount: u64,
}

#[event]
pub struct IntentCancelled {
    pub intent: Pubkey,
    pub owner: Pubkey,
    pub amount: u64,
    pub timestamp: i64,
}

#[event]
pub struct CircuitBreakerUpdated {
    pub channel_id: u64,
    pub halted: bool,
}

// ------------------- Errors -------------------

#[error_code]
pub enum MeridianError {
    #[msg("Program is paused")]
    ProgramPaused,
    #[msg("Signer is not the orchestrator")]
    NotOrchestrator,
    #[msg("Signer is not the admin")]
    NotAdmin,
    #[msg("Intent is not in Pending status")]
    IntentNotPending,
    #[msg("Intent has expired")]
    IntentExpired,
    #[msg("Channel is halted by circuit breaker")]
    ChannelHalted,
    #[msg("Circuit breaker channel does not match intent source chain")]
    ChannelMismatch,
    #[msg("Destination account does not match intent")]
    AccountSubstitution,
    #[msg("Insufficient funds in intent account")]
    InsufficientFunds,
    #[msg("Arithmetic overflow")]
    Overflow,
    #[msg("Signer is not the intent owner")]
    NotIntentOwner,
    #[msg("Intent is not yet cancellable (not expired and program not paused)")]
    IntentNotCancellable,
    #[msg("Settlement amount is below the slippage floor")]
    SlippageExceeded,
    #[msg("Settlement amount exceeds the intent's escrowed amount")]
    SettlementExceedsEscrow,
    #[msg("Unauthorized: owner account does not match intent owner")]
    Unauthorized,
}
