# vault_program

## Public API

### Functions

- `pub fn initialize_vault(admin: [u8; 32], fee_bps: u16) -> Result<(), ProgramError>`
- `pub fn deposit(amount: u64) -> Result<(), ProgramError>`
- `pub fn withdraw(shares: u64) -> Result<(), ProgramError>`
- `pub fn calculate_fees(position: &UserPosition, vault: &VaultState) -> u64`
- `pub fn bps_of(amount: u64, bps: u16) -> u64`
- `pub fn shares_to_amount(shares: u64, total_shares: u64, total_amount: u64) -> u64`

### Structs

- `pub struct VaultState {`
- `pub struct UserPosition {`

### Enums

- `pub enum ProgramError {`

