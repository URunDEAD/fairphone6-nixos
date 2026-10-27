//! fp6-storage-backup: Settings > Storage (usage by category, cache
//! clearing, old system generations, garbage collection, journal) and
//! Settings > Backup (age-encrypted archives, verify, preview, selective
//! restore) for Lomiri on the Fairphone 6.
//!
//! Binaries: `fp6-storaged` (root helper, system bus, polkit-guarded) and
//! `fp6-storage-agent` (session service used by the Settings pages, and the
//! scheduled backup run).

pub mod archive;
pub mod backup;
pub mod caches;
pub mod dconf;
pub mod gens;
pub mod keys;
pub mod sqlcopy;
pub mod store;
pub mod usage;
pub mod util;
pub mod wifi;
