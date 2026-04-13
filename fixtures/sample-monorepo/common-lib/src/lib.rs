//! Shared math utilities for vault operations.

/// Safely compute a percentage in basis points.
pub fn bps_of(amount: u64, bps: u16) -> u64 {
    amount.saturating_mul(bps as u64) / 10_000
}

/// Convert shares to underlying amount.
pub fn shares_to_amount(shares: u64, total_shares: u64, total_amount: u64) -> u64 {
    if total_shares == 0 {
        return 0;
    }
    shares.saturating_mul(total_amount) / total_shares
}
