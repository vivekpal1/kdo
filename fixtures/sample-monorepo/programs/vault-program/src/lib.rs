//! Vault program — handles deposits, withdrawals, fee accrual.
//!
//! PDA seeds: [b"vault", admin.key().as_ref()]

/// Vault state account.
pub struct VaultState {
    /// Admin public key.
    pub admin: [u8; 32],
    /// Total deposits in the vault.
    pub total_deposits: u64,
    /// Fee in basis points.
    pub fee_bps: u16,
    /// PDA bump seed.
    pub bump: u8,
}

/// User position account.
pub struct UserPosition {
    /// Owner public key.
    pub owner: [u8; 32],
    /// Shares held.
    pub shares: u64,
    /// Timestamp of last deposit.
    pub last_deposit_ts: i64,
}

/// Initialize a new vault with the given admin and fee rate.
pub fn initialize_vault(admin: [u8; 32], fee_bps: u16) -> Result<(), ProgramError> {
    // instruction implementation
    Ok(())
}

/// Deposit tokens into the vault.
pub fn deposit(amount: u64) -> Result<(), ProgramError> {
    // instruction implementation
    Ok(())
}

/// Withdraw shares from the vault.
pub fn withdraw(shares: u64) -> Result<(), ProgramError> {
    // instruction implementation
    Ok(())
}

/// Calculate fees owed on a position.
pub fn calculate_fees(position: &UserPosition, vault: &VaultState) -> u64 {
    let base = position.shares;
    base * vault.fee_bps as u64 / 10_000
}

/// Placeholder error type.
#[derive(Debug)]
pub enum ProgramError {
    /// Insufficient funds.
    InsufficientFunds,
    /// Unauthorized.
    Unauthorized,
}
