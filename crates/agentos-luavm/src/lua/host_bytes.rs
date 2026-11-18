//! One byte budget for everything the host builds on the Rust side from a script's input.
//!
//! The VM's memory limit covers the Lua heap only. A function implemented in Rust that builds a buffer first
//! (`string.gsub`'s output, `string.rep`'s result) allocates outside it, and a script can nest such calls: a
//! `gsub` whose replacement function runs another `gsub`, and so on, keeps one buffer per level alive at once.
//! A cap per call is therefore no cap at all (sixty levels of a 16 MiB cap are a gigabyte).
//!
//! A [`ByteBudget`] is shared by every such call of one VM. A [`Reservation`] charges it as a buffer grows and
//! gives the bytes back when the buffer is dropped (however the call ends), and a charge is refused when the
//! bytes reserved, plus the Lua heap in use, would pass the VM's memory limit: host buffers and the Lua heap
//! are one pool, so nothing the script can nest or repeat holds more than the limit it was given.

use std::sync::{
    atomic::{AtomicUsize, Ordering},
    Arc,
};

use mlua::Lua;

/// The bytes the host may hold for one VM's script at once (see the module docs).
#[derive(Debug)]
pub struct ByteBudget {
    limit: usize,
    used: AtomicUsize,
    peak: AtomicUsize,
}

impl ByteBudget {
    /// A budget of `limit` bytes (the VM's memory limit).
    #[must_use]
    pub fn new(limit: usize) -> Arc<Self> {
        Arc::new(Self { limit, used: AtomicUsize::new(0), peak: AtomicUsize::new(0) })
    }

    /// Bytes reserved right now.
    #[must_use]
    pub fn used(&self) -> usize {
        self.used.load(Ordering::SeqCst)
    }

    /// The most bytes that were ever reserved at once.
    #[must_use]
    pub fn peak(&self) -> usize {
        self.peak.load(Ordering::SeqCst)
    }

    /// The limit.
    #[must_use]
    pub fn limit(&self) -> usize {
        self.limit
    }

    /// An empty reservation against this budget.
    #[must_use]
    pub fn reservation(self: &Arc<Self>) -> Reservation {
        Reservation { budget: self.clone(), held: 0 }
    }

    fn grow(&self, lua: &Lua, by: usize) -> mlua::Result<()> {
        let heap = lua.used_memory();
        let mut used = self.used.load(Ordering::SeqCst);
        loop {
            let after = used
                .checked_add(by)
                .filter(|after| after.saturating_add(heap) <= self.limit)
                .ok_or_else(|| mlua::Error::MemoryError("not enough memory".into()))?;
            match self.used.compare_exchange(used, after, Ordering::SeqCst, Ordering::SeqCst) {
                Ok(_) => {
                    self.peak.fetch_max(after, Ordering::SeqCst);
                    return Ok(());
                }
                Err(now) => used = now,
            }
        }
    }

    fn shrink(&self, by: usize) {
        // saturating: a release can never take the count below zero
        let _ =
            self.used.fetch_update(Ordering::SeqCst, Ordering::SeqCst, |used| Some(used.saturating_sub(by)));
    }
}

/// Bytes held against a [`ByteBudget`] by one host-side buffer; released when it is dropped.
#[derive(Debug)]
pub struct Reservation {
    budget: Arc<ByteBudget>,
    held: usize,
}

impl Reservation {
    /// Hold `total` bytes (more or fewer than before): charged when it grows, refused with a Lua memory error
    /// when the pool would pass its limit.
    ///
    /// # Errors
    /// A [`mlua::Error::MemoryError`] when the budget cannot cover the growth; the reservation is unchanged.
    pub fn hold(&mut self, lua: &Lua, total: usize) -> mlua::Result<()> {
        if total > self.held {
            self.budget.grow(lua, total - self.held)?;
        } else {
            self.budget.shrink(self.held - total);
        }
        self.held = total;
        Ok(())
    }

    /// Bytes held.
    #[must_use]
    pub fn held(&self) -> usize {
        self.held
    }
}

impl Drop for Reservation {
    fn drop(&mut self) {
        self.budget.shrink(self.held);
    }
}

/// A growing output buffer whose capacity is held against a [`ByteBudget`] (the real allocation, not the
/// length: a `Vec` that doubles holds up to twice what it was asked for).
#[derive(Debug)]
pub struct Buffer {
    bytes: Vec<u8>,
    reservation: Reservation,
}

impl Buffer {
    /// An empty buffer.
    #[must_use]
    pub fn new(budget: &Arc<ByteBudget>) -> Self {
        Self { bytes: Vec::new(), reservation: budget.reservation() }
    }

    /// The bytes so far.
    #[must_use]
    pub fn as_slice(&self) -> &[u8] {
        &self.bytes
    }

    /// Append `data`, charging the growth first.
    ///
    /// # Errors
    /// A memory error when the pool is spent; nothing is appended.
    pub fn extend(&mut self, lua: &Lua, data: &[u8]) -> mlua::Result<()> {
        let needed = self.bytes.len().checked_add(data.len()).ok_or_else(too_big)?;
        if needed > self.bytes.capacity() {
            // what `Vec` will allocate: at least double, at least what is needed
            let capacity = needed.max(self.bytes.capacity().saturating_mul(2)).max(8);
            self.reservation.hold(lua, capacity)?;
            self.bytes.try_reserve_exact(capacity - self.bytes.len()).map_err(|_| too_big())?;
        }
        self.bytes.extend_from_slice(data);
        Ok(())
    }

    /// Append one byte.
    ///
    /// # Errors
    /// As [`Buffer::extend`].
    pub fn push(&mut self, lua: &Lua, byte: u8) -> mlua::Result<()> {
        self.extend(lua, &[byte])
    }
}

fn too_big() -> mlua::Error {
    mlua::Error::MemoryError("not enough memory".into())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn lua() -> Lua {
        Lua::new_with(mlua::StdLib::NONE, mlua::LuaOptions::new()).expect("lua")
    }

    #[test]
    fn a_reservation_is_charged_while_held_and_released_when_dropped() {
        let (lua, budget) = (lua(), ByteBudget::new(10_000_000));
        {
            let mut a = budget.reservation();
            let mut b = budget.reservation();
            a.hold(&lua, 1000).unwrap();
            b.hold(&lua, 500).unwrap();
            assert_eq!(budget.used(), 1500);
            a.hold(&lua, 200).unwrap();
            assert_eq!(budget.used(), 700, "holding less gives bytes back");
        }
        assert_eq!(budget.used(), 0);
        assert_eq!(budget.peak(), 1500);
    }

    #[test]
    fn a_charge_past_the_limit_is_a_memory_error_and_changes_nothing() {
        let (lua, budget) = (lua(), ByteBudget::new(1_000_000));
        let mut first = budget.reservation();
        first.hold(&lua, 600_000).unwrap();
        let mut second = budget.reservation();
        let err = second.hold(&lua, 600_000).unwrap_err();
        assert!(matches!(err, mlua::Error::MemoryError(_)), "{err}");
        assert_eq!((second.held(), budget.used()), (0, 600_000));
        drop(first);
        second.hold(&lua, 600_000).expect("the first one's bytes are free again");
    }

    #[test]
    fn the_lua_heap_in_use_counts_against_the_same_pool() {
        let lua = lua();
        let heap = lua.used_memory();
        let budget = ByteBudget::new(heap + 1000);
        let mut r = budget.reservation();
        assert!(r.hold(&lua, 5000).is_err());
        assert!(r.hold(&lua, 500).is_ok());
    }

    #[test]
    fn a_buffer_holds_its_capacity_and_releases_it_on_drop_even_when_it_fails() {
        let (lua, budget) = (lua(), ByteBudget::new(1 << 20));
        let mut buffer = Buffer::new(&budget);
        let chunk = vec![7u8; 1000];
        let mut appended = 0;
        let failure = loop {
            match buffer.extend(&lua, &chunk) {
                Ok(()) => appended += chunk.len(),
                Err(e) => break e,
            }
        };
        assert!(matches!(failure, mlua::Error::MemoryError(_)));
        assert_eq!(buffer.as_slice().len(), appended);
        assert!(budget.used() >= appended && budget.used() <= budget.limit(), "{}", budget.used());
        drop(buffer);
        assert_eq!(budget.used(), 0);
    }

    #[test]
    fn many_live_buffers_share_one_limit() {
        let (lua, budget) = (lua(), ByteBudget::new(4 << 20));
        let mut live = Vec::new();
        let mut levels = 0;
        loop {
            let mut buffer = Buffer::new(&budget);
            if buffer.extend(&lua, &vec![1u8; 1 << 20]).is_err() {
                break;
            }
            live.push(buffer);
            levels += 1;
            assert!(levels < 100, "sixty levels of a per-call cap would all have succeeded");
        }
        assert!(levels <= 4, "{levels}");
        assert!(budget.peak() <= budget.limit());
    }
}
