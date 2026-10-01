use std::collections::hash_map::RandomState;
use std::hash::BuildHasher;
use std::io::Read;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

pub const SEED_BYTES: usize = 64;

static JOBS: AtomicU64 = AtomicU64::new(0);

pub fn seed_generator() {
    let seed = fresh_seed();
    let _ = convert_pdfdoc::seed_random(&seed);
}

pub fn fresh_seed() -> [u8; SEED_BYTES] {
    from_the_system().unwrap_or_else(|| from_the_hasher(JOBS.fetch_add(1, Ordering::Relaxed)))
}

fn from_the_system() -> Option<[u8; SEED_BYTES]> {
    let mut seed = [0_u8; SEED_BYTES];
    std::fs::File::open("/dev/urandom")
        .and_then(|mut source| source.read_exact(&mut seed))
        .ok()?;
    Some(seed)
}

pub fn from_the_hasher(job: u64) -> [u8; SEED_BYTES] {
    let state = RandomState::new();
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |since| since.as_nanos());
    let mut seed = [0_u8; SEED_BYTES];
    let (words, _) = seed.as_chunks_mut::<8>();
    for (position, word) in (0_u64..).zip(words) {
        *word = state.hash_one((job, position, nanos)).to_le_bytes();
    }
    seed
}

pub fn owner_password() -> String {
    use std::fmt::Write as _;
    fresh_seed()
        .iter()
        .take(16)
        .fold(String::with_capacity(32), |mut text, byte| {
            let _ = write!(text, "{byte:02x}");
            text
        })
}
