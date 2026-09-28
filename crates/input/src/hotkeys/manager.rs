//! The Windows `RegisterHotKey` backend: one dedicated thread owns a message loop (Win32 requires
//! hotkey registration and its `WM_HOTKEY` delivery to happen on the same thread with a message
//! queue), and every public call is a request posted to that thread via `PostThreadMessageW`,
//! mirroring `hotkeys.py`'s `_WindowsHotkeyBackend`. Unlike the Python reference's `apply_bindings`
//! (which diffs a whole dict of label->hotkey pairs at once, to support a settings-page save), this
//! registers and unregisters one binding at a time — simpler, and sufficient for what the task
//! calls for: register with a callback, unregister explicitly or on drop, conflict errors.

use std::collections::HashMap;
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::{mpsc, Arc, Mutex};
use std::thread::{self, JoinHandle};
use std::time::Duration;

use windows::Win32::Foundation::{LPARAM, WPARAM};
use windows::Win32::System::Threading::GetCurrentThreadId;
use windows::Win32::UI::Input::KeyboardAndMouse::{RegisterHotKey, UnregisterHotKey, HOT_KEY_MODIFIERS};
use windows::Win32::UI::WindowsAndMessaging::{
    DispatchMessageW, GetMessageW, PeekMessageW, PostThreadMessageW, TranslateMessage, MSG, PM_NOREMOVE, WM_APP, WM_HOTKEY,
};

use super::parse::{parse_hotkey, ParsedHotkey};
use super::HotkeyError;

/// Our one private thread message, carrying a job id in `wParam`. Chosen to not collide with any
/// standard `WM_*` message (`WM_APP` is reserved by Windows for exactly this).
const WM_APP_JOB: u32 = WM_APP + 1;
/// How long a caller waits for the listener thread to start or to finish one job before giving up.
const THREAD_TIMEOUT: Duration = Duration::from_secs(5);

type Callback = Box<dyn Fn() + Send + 'static>;

/// A request for the listener thread to perform, posted via [`HotkeyManager::submit`].
enum Job {
    Register { label: String, hotkey: ParsedHotkey, callback: Callback },
    Unregister { label: String },
    Shutdown,
}

/// What a [`Job`] produced, sent back over its own one-shot reply channel.
enum JobOutcome {
    Registered { canonical: String },
    Unregistered,
    ShutDown,
}

struct PendingJob {
    job: Option<Job>,
    reply: mpsc::SyncSender<Result<JobOutcome, HotkeyError>>,
}

/// Registers and manages global hotkeys via `RegisterHotKey` on a dedicated message-loop thread.
/// Every binding is unregistered when either dropped explicitly via
/// [`HotkeyManager::unregister`] or when the whole manager is dropped.
pub struct HotkeyManager {
    thread: Option<JoinHandle<()>>,
    thread_id: u32,
    pending: Arc<Mutex<HashMap<u32, PendingJob>>>,
    next_job_id: AtomicU32,
}

impl HotkeyManager {
    /// Starts the listener thread. Fails only if the thread itself can't be started.
    pub fn new() -> Result<Self, HotkeyError> {
        let pending: Arc<Mutex<HashMap<u32, PendingJob>>> = Arc::new(Mutex::new(HashMap::new()));
        let (thread_id_tx, thread_id_rx) = mpsc::channel();
        let pending_for_thread = Arc::clone(&pending);

        let thread = thread::Builder::new()
            .name("HotkeyMessageLoop".to_string())
            .spawn(move || message_loop(&thread_id_tx, &pending_for_thread))
            .map_err(|e| HotkeyError::Registration(format!("failed to start hotkey listener thread: {e}")))?;

        let thread_id = thread_id_rx
            .recv_timeout(THREAD_TIMEOUT)
            .map_err(|_| HotkeyError::Registration("hotkey listener thread failed to start".to_string()))?;

        Ok(Self { thread: Some(thread), thread_id, pending, next_job_id: AtomicU32::new(1) })
    }

    /// Registers `hotkey` under `label`, calling `callback` (on the listener thread) every time it
    /// fires. Registering the same label again with the same hotkey just swaps the callback;
    /// registering it with a different hotkey moves the binding; registering a hotkey already used
    /// by a *different* label fails with [`HotkeyError::Conflict`]. Returns the canonical hotkey
    /// text (e.g. `"ctrl+f11"`).
    pub fn register(&self, label: impl Into<String>, hotkey: &str, callback: impl Fn() + Send + 'static) -> Result<String, HotkeyError> {
        let hotkey = parse_hotkey(hotkey)?;
        let job = Job::Register { label: label.into(), hotkey, callback: Box::new(callback) };
        match self.submit(job)? {
            JobOutcome::Registered { canonical } => Ok(canonical),
            JobOutcome::Unregistered | JobOutcome::ShutDown => {
                unreachable!("a Register job always yields JobOutcome::Registered")
            }
        }
    }

    /// Unregisters `label`, if it was registered. Not an error if it wasn't.
    pub fn unregister(&self, label: &str) -> Result<(), HotkeyError> {
        self.submit(Job::Unregister { label: label.to_string() }).map(|_| ())
    }

    fn submit(&self, job: Job) -> Result<JobOutcome, HotkeyError> {
        let job_id = self.next_job_id.fetch_add(1, Ordering::SeqCst);
        let (reply_tx, reply_rx) = mpsc::sync_channel(1);
        self.pending.lock().unwrap().insert(job_id, PendingJob { job: Some(job), reply: reply_tx });

        // SAFETY: `self.thread_id` names our own listener thread's message queue, kept alive at
        // least until `Drop` joins it; `WM_APP_JOB` is a private application message.
        let posted = unsafe { PostThreadMessageW(self.thread_id, WM_APP_JOB, WPARAM(job_id as usize), LPARAM(0)) };
        if posted.is_err() {
            self.pending.lock().unwrap().remove(&job_id);
            return Err(HotkeyError::Registration("failed to reach the hotkey listener thread".to_string()));
        }

        reply_rx.recv_timeout(THREAD_TIMEOUT).unwrap_or_else(|_| {
            self.pending.lock().unwrap().remove(&job_id);
            Err(HotkeyError::Registration("timed out waiting for the hotkey listener thread".to_string()))
        })
    }
}

impl Drop for HotkeyManager {
    fn drop(&mut self) {
        let _ = self.submit(Job::Shutdown);
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

/// One active binding, owned exclusively by the listener thread.
struct Binding {
    native_id: i32,
    canonical: String,
    callback: Callback,
}

/// State touched only by the listener thread — no lock needed since nothing else ever sees it.
#[derive(Default)]
struct WorkerState {
    bindings: HashMap<String, Binding>,
    id_lookup: HashMap<i32, String>,
    next_native_id: i32,
}

impl WorkerState {
    fn register(&mut self, label: String, hotkey: ParsedHotkey, callback: Callback) -> Result<JobOutcome, HotkeyError> {
        if let Some(existing) = self.bindings.get(&label) {
            if existing.canonical == hotkey.canonical {
                self.bindings.get_mut(&label).unwrap().callback = callback;
                return Ok(JobOutcome::Registered { canonical: hotkey.canonical });
            }
        }
        if let Some((other_label, _)) = self.bindings.iter().find(|(l, b)| **l != label && b.canonical == hotkey.canonical) {
            return Err(HotkeyError::Conflict(format!("Hotkey '{}' is already assigned to '{other_label}'", hotkey.canonical)));
        }

        self.next_native_id += 1;
        let native_id = self.next_native_id;
        // SAFETY: `None` registers a global hotkey (not tied to a window); `native_id` is unique
        // for the lifetime of this thread.
        unsafe { RegisterHotKey(None, native_id, HOT_KEY_MODIFIERS(hotkey.modifier_mask), u32::from(hotkey.vk_code)) }
            .map_err(|e| HotkeyError::Registration(format!("unable to register hotkey '{}': {e}", hotkey.canonical)))?;

        if let Some(old) = self.bindings.remove(&label) {
            // SAFETY: `old.native_id` was registered by this same thread and not yet unregistered.
            unsafe {
                let _ = UnregisterHotKey(None, old.native_id);
            }
            self.id_lookup.remove(&old.native_id);
        }
        self.id_lookup.insert(native_id, label.clone());
        self.bindings.insert(label, Binding { native_id, canonical: hotkey.canonical.clone(), callback });
        Ok(JobOutcome::Registered { canonical: hotkey.canonical })
    }

    fn unregister(&mut self, label: &str) {
        if let Some(binding) = self.bindings.remove(label) {
            // SAFETY: `binding.native_id` was registered by this same thread.
            unsafe {
                let _ = UnregisterHotKey(None, binding.native_id);
            }
            self.id_lookup.remove(&binding.native_id);
        }
    }

    fn teardown(&mut self) {
        for label in self.bindings.keys().cloned().collect::<Vec<_>>() {
            self.unregister(&label);
        }
    }

    fn fire(&self, native_id: i32) {
        if let Some(binding) = self.id_lookup.get(&native_id).and_then(|label| self.bindings.get(label)) {
            (binding.callback)();
        }
    }
}

/// Runs a single [`Job`] (identified by `job_id`) against `state`, replying on its channel.
/// Returns `true` once a [`Job::Shutdown`] has been processed, telling the caller to stop the loop.
fn run_job(state: &mut WorkerState, pending: &Arc<Mutex<HashMap<u32, PendingJob>>>, job_id: u32) -> bool {
    let Some(PendingJob { job: Some(job), reply }) = pending.lock().unwrap().remove(&job_id) else {
        return false;
    };
    let (result, exit) = match job {
        Job::Register { label, hotkey, callback } => (state.register(label, hotkey, callback), false),
        Job::Unregister { label } => {
            state.unregister(&label);
            (Ok(JobOutcome::Unregistered), false)
        }
        Job::Shutdown => {
            state.teardown();
            (Ok(JobOutcome::ShutDown), true)
        }
    };
    let _ = reply.send(result);
    exit
}

/// The listener thread's body: registers this thread's id, then pumps messages until told to shut
/// down or the queue itself errors out. Ports `_WindowsHotkeyBackend._message_loop`.
fn message_loop(thread_id_tx: &mpsc::Sender<u32>, pending: &Arc<Mutex<HashMap<u32, PendingJob>>>) {
    // SAFETY: reads only this thread's own id.
    let thread_id = unsafe { GetCurrentThreadId() };
    let mut msg = MSG::default();
    // SAFETY: `msg` is a valid, writable MSG. This forces Windows to create this thread's message
    // queue before we signal readiness, so a `PostThreadMessageW` from another thread right after
    // that signal can never race ahead of the queue's own existence.
    unsafe {
        let _ = PeekMessageW(&mut msg, None, 0, 0, PM_NOREMOVE);
    }
    if thread_id_tx.send(thread_id).is_err() {
        return; // the manager was dropped before this thread even finished starting
    }

    let mut state = WorkerState::default();
    loop {
        // SAFETY: `msg` is a valid, writable MSG; `None` means "this thread's queue, any window".
        let status = unsafe { GetMessageW(&mut msg, None, 0, 0) };
        if status.0 == 0 || status.0 == -1 {
            break;
        }
        match msg.message {
            WM_HOTKEY => state.fire(msg.wParam.0 as i32),
            WM_APP_JOB => {
                if run_job(&mut state, pending, msg.wParam.0 as u32) {
                    break;
                }
            }
            _ => {
                // SAFETY: `msg` was just populated by `GetMessageW` above.
                unsafe {
                    let _ = TranslateMessage(&msg);
                    DispatchMessageW(&msg);
                }
            }
        }
    }
    // A message-loop failure (`status.0 == -1`) must not leave stale registrations behind.
    state.teardown();
}
