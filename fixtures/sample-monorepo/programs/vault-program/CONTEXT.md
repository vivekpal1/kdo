# vault-program

> Solana vault program. Handles deposits, withdrawals, fee accrual.

## Public API

### Functions

- `pub fn initialize_vault(admin: [u8; 32], fee_bps: u16) -> Result<(), ProgramError>`
- `pub fn deposit(amount: u64) -> Result<(), ProgramError>`
- `pub fn withdraw(shares: u64) -> Result<(), ProgramError>`
- `pub fn calculate_fees(position: &UserPosition, vault: &VaultState) -> u64`

### Structs

- `pub struct VaultState {`
- `pub struct UserPosition {`

### Enums

- `pub enum ProgramError {`

## Dependencies

- `common-lib`

