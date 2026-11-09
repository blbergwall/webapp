//! Opt-in in-memory cache of sessions and their accounts
//! (`Config::session_cache`), so `Auth` usually needs no Redis call.
//!
//! **Single-instance deployments only:** a logout or account update on one
//! server can't reach another server's cache.
//!
//! Redis stays the source of truth. Each cached session's Redis TTL was
//! refreshed within its refresh interval (see
//! `session::cache_refresh_interval`), which is shorter than the TTL, so the
//! cache never serves a session Redis has expired. Entries idle for
//! [`IDLE_CUTOFF`] are swept and reload from Redis on their next use.
//!
//! All methods are synchronous: the lock is never held across an `.await`.

use std::{
    collections::HashMap,
    sync::Mutex,
    time::{Duration, Instant},
};

use webapp_common::Account;

/// How long an unused entry stays cached.
const IDLE_CUTOFF: Duration = Duration::from_secs(60 * 60);
/// How often a lookup sweeps out idle entries.
const SWEEP_INTERVAL: Duration = Duration::from_secs(10 * 60);

pub(crate) struct SessionCache(Mutex<State>);

struct State {
    entries: HashMap<String, Entry>,
    /// Bumped by every removal and account update. A miss records it before
    /// reading Redis and only inserts if it's unchanged, so a read that
    /// raced a logout or account write can't cache stale data.
    generation: u64,
    last_sweep: Instant,
}

struct Entry {
    account: Account,
    /// When the session's Redis TTL was last reset.
    refreshed: Instant,
    used: Instant,
}

#[derive(Debug, PartialEq)]
pub(crate) enum Lookup {
    /// Cached and recently refreshed: no Redis call needed.
    Hit(Account),
    /// Cached, but the Redis TTL is due a refresh; call
    /// [`SessionCache::refreshed`] once it succeeds.
    Refresh(Account),
    /// Not cached: read Redis, then [`SessionCache::insert`] with this
    /// generation.
    Miss { generation: u64 },
}

impl SessionCache {
    pub(crate) fn new() -> Self {
        Self(Mutex::new(State {
            entries: HashMap::new(),
            generation: 0,
            last_sweep: Instant::now(),
        }))
    }

    fn state(&self) -> std::sync::MutexGuard<'_, State> {
        // A panic while holding the lock can't leave the map inconsistent
        // (every update is a single insert/remove/assignment), so recover.
        self.0.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    /// Looks up a session token, marking its entry used. `refresh_after` is
    /// the token's refresh interval. Sweeps idle entries when one is due.
    pub(crate) fn lookup(&self, token: &str, now: Instant, refresh_after: Duration) -> Lookup {
        let mut state = self.state();
        if now.saturating_duration_since(state.last_sweep) >= SWEEP_INTERVAL {
            state
                .entries
                .retain(|_, entry| now.saturating_duration_since(entry.used) < IDLE_CUTOFF);
            state.last_sweep = now;
        }
        let generation = state.generation;
        match state.entries.get_mut(token) {
            Some(entry) => {
                entry.used = now;
                if now.saturating_duration_since(entry.refreshed) >= refresh_after {
                    Lookup::Refresh(entry.account.clone())
                } else {
                    Lookup::Hit(entry.account.clone())
                }
            }
            None => Lookup::Miss { generation },
        }
    }

    /// Caches a session just read from Redis (which also refreshed its TTL),
    /// unless something was invalidated since `generation` was taken.
    pub(crate) fn insert(&self, token: &str, account: Account, generation: u64, now: Instant) {
        let mut state = self.state();
        if state.generation == generation {
            let entry = Entry {
                account,
                refreshed: now,
                used: now,
            };
            state.entries.insert(token.to_string(), entry);
        }
    }

    /// Records that the session's Redis TTL was just reset.
    pub(crate) fn refreshed(&self, token: &str, now: Instant) {
        if let Some(entry) = self.state().entries.get_mut(token) {
            entry.refreshed = now;
        }
    }

    /// Drops a session (logged out, or gone from Redis).
    pub(crate) fn remove(&self, token: &str) {
        let mut state = self.state();
        state.generation += 1;
        state.entries.remove(token);
    }

    /// Replaces the cached copy of an account in all of its sessions. Call
    /// after every framework write to its `FA` record.
    pub(crate) fn update_account(&self, account: &Account) {
        let mut state = self.state();
        state.generation += 1;
        for entry in state.entries.values_mut() {
            if entry.account.id == account.id {
                entry.account = account.clone();
            }
        }
    }

    #[cfg(test)]
    fn len(&self) -> usize {
        self.state().entries.len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const DAY: Duration = Duration::from_secs(24 * 60 * 60);
    const MINUTE: Duration = Duration::from_secs(60);

    fn account(id: &str, name: &str) -> Account {
        Account {
            id: id.to_string(),
            created: 0,
            display_name: name.to_string(),
            email: None,
            discord_id: None,
            google_id: None,
        }
    }

    fn generation(lookup: Lookup) -> u64 {
        match lookup {
            Lookup::Miss { generation } => generation,
            other => panic!("expected a miss, got {other:?}"),
        }
    }

    #[test]
    fn miss_then_hit() {
        let cache = SessionCache::new();
        let now = Instant::now();
        let generation = generation(cache.lookup("t", now, DAY));
        cache.insert("t", account("a", "Ann"), generation, now);
        assert_eq!(cache.lookup("t", now + MINUTE, DAY), Lookup::Hit(account("a", "Ann")));
    }

    #[test]
    fn refresh_due_after_interval() {
        let cache = SessionCache::new();
        let start = Instant::now();
        cache.insert("t", account("a", "Ann"), 0, start);
        // Keep the entry in use, so the sweep never drops it.
        let due = start + DAY;
        let step = IDLE_CUTOFF / 2;
        let mut now = start;
        while now + step < due {
            now += step;
            assert_eq!(cache.lookup("t", now, DAY), Lookup::Hit(account("a", "Ann")));
        }
        assert_eq!(cache.lookup("t", due, DAY), Lookup::Refresh(account("a", "Ann")));
        // Still due until the refresh is recorded.
        assert_eq!(cache.lookup("t", due, DAY), Lookup::Refresh(account("a", "Ann")));
        cache.refreshed("t", due);
        assert_eq!(cache.lookup("t", due + MINUTE, DAY), Lookup::Hit(account("a", "Ann")));
    }

    #[test]
    fn refresh_interval_is_per_lookup() {
        // Activity tokens pass a shorter interval than website tokens.
        let cache = SessionCache::new();
        let now = Instant::now();
        cache.insert("t", account("a", "Ann"), 0, now);
        let later = now + 10 * MINUTE;
        assert!(matches!(cache.lookup("t", later, DAY), Lookup::Hit(_)));
        assert!(matches!(cache.lookup("t", later, 5 * MINUTE), Lookup::Refresh(_)));
    }

    #[test]
    fn sweep_drops_only_idle_entries() {
        let cache = SessionCache::new();
        let start = Instant::now();
        cache.insert("idle", account("a", "Ann"), 0, start);
        cache.insert("busy", account("b", "Bob"), 0, start);
        assert!(matches!(cache.lookup("busy", start + IDLE_CUTOFF - MINUTE, DAY), Lookup::Hit(_)));
        // The next sweep drops "idle" but keeps "busy".
        let later = start + IDLE_CUTOFF - MINUTE + SWEEP_INTERVAL;
        assert!(matches!(cache.lookup("idle", later, DAY), Lookup::Miss { .. }));
        assert_eq!(cache.len(), 1);
        assert!(matches!(cache.lookup("busy", later, DAY), Lookup::Hit(_)));
    }

    #[test]
    fn sweep_waits_for_its_interval() {
        let cache = SessionCache::new();
        let start = Instant::now();
        cache.insert("t", account("a", "Ann"), 0, start);
        // A sweep runs here, while "t" is still fresh.
        let swept = start + IDLE_CUTOFF - MINUTE;
        assert!(matches!(cache.lookup("x", swept, DAY), Lookup::Miss { .. }));
        assert_eq!(cache.len(), 1);
        // "t" is idle now, but the next sweep isn't due yet.
        assert!(matches!(cache.lookup("x", start + IDLE_CUTOFF + MINUTE, DAY), Lookup::Miss { .. }));
        assert_eq!(cache.len(), 1);
        assert!(matches!(cache.lookup("x", swept + SWEEP_INTERVAL, DAY), Lookup::Miss { .. }));
        assert_eq!(cache.len(), 0);
    }

    #[test]
    fn remove_blocks_a_racing_insert() {
        let cache = SessionCache::new();
        let now = Instant::now();
        cache.insert("t", account("a", "Ann"), 0, now);
        // A request misses on another token and reads Redis…
        let generation = generation(cache.lookup("u", now, DAY));
        // …while "u" is logged out (it was never cached here).
        cache.remove("u");
        cache.remove("t");
        cache.insert("u", account("a", "Ann"), generation, now);
        assert!(matches!(cache.lookup("u", now, DAY), Lookup::Miss { .. }));
        assert!(matches!(cache.lookup("t", now, DAY), Lookup::Miss { .. }));
    }

    #[test]
    fn update_account_replaces_every_session_of_that_account() {
        let cache = SessionCache::new();
        let now = Instant::now();
        cache.insert("t1", account("a", "Ann"), 0, now);
        cache.insert("t2", account("a", "Ann"), 0, now);
        cache.insert("t3", account("b", "Bob"), 0, now);
        // A miss that started before the update must not cache the old copy.
        let generation = generation(cache.lookup("t4", now, DAY));
        cache.update_account(&account("a", "Ann 2"));
        cache.insert("t4", account("a", "Ann"), generation, now);

        assert_eq!(cache.lookup("t1", now, DAY), Lookup::Hit(account("a", "Ann 2")));
        assert_eq!(cache.lookup("t2", now, DAY), Lookup::Hit(account("a", "Ann 2")));
        assert_eq!(cache.lookup("t3", now, DAY), Lookup::Hit(account("b", "Bob")));
        assert!(matches!(cache.lookup("t4", now, DAY), Lookup::Miss { .. }));
    }
}
