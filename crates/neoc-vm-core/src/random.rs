//! The operating system's entropy, for what the runtime itself needs (`script_env`'s private `math.random` seed) and
//! what `lib:random` draws on.

use rand::TryRng;
use rand::rngs::SysRng;

/// Fill `buf` from the OS CSPRNG; an entropy-source failure is the reason, as text.
pub fn fill_random(buf: &mut [u8]) -> Result<(), String> {
    SysRng.try_fill_bytes(buf).map_err(|e| e.to_string())
}
