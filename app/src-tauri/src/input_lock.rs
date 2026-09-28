//! Only one feature drives the mouse at a time. A run (the stash sorter, the auto lister) takes the
//! lock for as long as it clicks in the game; while it holds it, Ctrl+F12 is registered as a global
//! hotkey that stops it, and released again afterwards so other programs get the key back.

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};

use input::hotkeys::HotkeyManager;

/// The global stop hotkey, as `input::hotkeys` spells it.
pub const STOP_HOTKEY: &str = "ctrl+f12";
const STOP_LABEL: &str = "stop";

type StopFn = Box<dyn Fn() + Send>;

struct Holder {
    id: u64,
    what: &'static str,
    on_stop: Vec<StopFn>,
}

/// The app-wide mouse lock. Cheap to clone; clones share the lock.
#[derive(Clone, Default)]
pub struct InputLock {
    slot: Arc<Mutex<Option<Holder>>>,
    /// Created on first use: a thread with a message loop that owns the hotkey registration.
    hotkeys: Arc<Mutex<Option<HotkeyManager>>>,
    next_id: Arc<AtomicU64>,
}

fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
}

impl InputLock {
    /// Takes the lock for `what` (a sentence subject such as "The stash sorter"), or says who has
    /// it: "The auto lister is running."
    pub fn try_acquire(&self, what: &'static str) -> Result<InputGuard, String> {
        let id = self.next_id.fetch_add(1, Ordering::Relaxed);
        {
            let mut slot = lock(&self.slot);
            if let Some(holder) = slot.as_ref() {
                return Err(format!("{} is running.", holder.what));
            }
            *slot = Some(Holder { id, what, on_stop: Vec::new() });
        }
        let hotkey = self.register_stop_hotkey();
        Ok(InputGuard { lock: self.clone(), id, hotkey })
    }

    /// Who drives the mouse right now, if anyone.
    pub fn holder(&self) -> Option<&'static str> {
        lock(&self.slot).as_ref().map(|holder| holder.what)
    }

    /// Stops whatever drives the mouse (what Ctrl+F12 does). `true` when something was running.
    pub fn stop(&self) -> bool {
        stop_holder(&self.slot)
    }

    /// Registers Ctrl+F12 for the current holder. A failure (another program owns the key) is
    /// logged and the run goes ahead: the Stop button and moving the mouse still stop it.
    fn register_stop_hotkey(&self) -> bool {
        if cfg!(test) {
            return false; // unit tests never take a global hotkey from the desktop
        }
        let mut hotkeys = lock(&self.hotkeys);
        if hotkeys.is_none() {
            match HotkeyManager::new() {
                Ok(manager) => *hotkeys = Some(manager),
                Err(err) => {
                    log::warn!("the stop hotkey is unavailable: {err}");
                    return false;
                }
            }
        }
        let slot = Arc::clone(&self.slot);
        let registered = hotkeys.as_ref().map(|manager| manager.register(STOP_LABEL, STOP_HOTKEY, move || {
            stop_holder(&slot);
        }));
        match registered {
            Some(Ok(_)) => true,
            Some(Err(err)) => {
                log::warn!("Ctrl+F12 could not be registered, so it won't stop this run: {err}");
                false
            }
            None => false,
        }
    }

    fn release(&self, id: u64, hotkey: bool) {
        if hotkey {
            if let Some(manager) = lock(&self.hotkeys).as_ref() {
                if let Err(err) = manager.unregister(STOP_LABEL) {
                    log::warn!("could not release Ctrl+F12: {err}");
                }
            }
        }
        let mut slot = lock(&self.slot);
        if slot.as_ref().is_some_and(|holder| holder.id == id) {
            *slot = None;
        }
    }
}

/// Runs every stop callback of the current holder.
fn stop_holder(slot: &Mutex<Option<Holder>>) -> bool {
    let slot = lock(slot);
    match slot.as_ref() {
        Some(holder) => {
            for stop in &holder.on_stop {
                stop();
            }
            true
        }
        None => false,
    }
}

/// Holding this means driving the mouse is yours. Dropping it releases the lock and Ctrl+F12.
pub struct InputGuard {
    lock: InputLock,
    id: u64,
    hotkey: bool,
}

impl InputGuard {
    /// What [`InputLock::stop`] (and so Ctrl+F12) calls while this guard is held. Keep it quick:
    /// set a cancel flag, nothing more.
    pub fn on_stop(&self, stop: impl Fn() + Send + 'static) {
        let mut slot = lock(&self.lock.slot);
        if let Some(holder) = slot.as_mut().filter(|holder| holder.id == self.id) {
            holder.on_stop.push(Box::new(stop));
        }
    }

    /// Whether Ctrl+F12 stops this run (it doesn't when another program owns the key).
    pub fn stop_hotkey_active(&self) -> bool {
        self.hotkey
    }
}

impl Drop for InputGuard {
    fn drop(&mut self) {
        self.lock.release(self.id, self.hotkey);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::AtomicBool;

    #[test]
    fn a_second_feature_is_told_who_drives_the_mouse() {
        let lock = InputLock::default();
        let guard = lock.try_acquire("The auto lister").expect("free lock");
        assert_eq!(lock.holder(), Some("The auto lister"));
        assert_eq!(lock.try_acquire("The stash sorter").err().as_deref(), Some("The auto lister is running."));
        drop(guard);
        assert_eq!(lock.holder(), None);
        assert!(lock.try_acquire("The stash sorter").is_ok());
    }

    #[test]
    fn stop_reaches_the_current_holder_only() {
        let lock = InputLock::default();
        assert!(!lock.stop(), "nothing to stop");
        let stopped = Arc::new(AtomicBool::new(false));
        let guard = lock.try_acquire("The stash sorter").expect("free lock");
        let flag = Arc::clone(&stopped);
        guard.on_stop(move || flag.store(true, Ordering::SeqCst));
        assert!(lock.stop());
        assert!(stopped.load(Ordering::SeqCst));
        drop(guard);
        assert!(!lock.stop());
    }
}
