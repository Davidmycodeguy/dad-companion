//! The public [`Capture`] API: spawns a reader thread that turns Npcap frames into `Segment`s.

use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, TryIter};
use std::sync::{Arc, OnceLock};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use protocol::Segment;

use crate::device::{self, DeviceInfo};
use crate::error::Error;
use crate::handle::PcapHandle;
use crate::interface;
use crate::library::NpcapLibrary;
use crate::parse::{self, PortRange};

/// Lists every network adapter Npcap can see, e.g. to present as choices for
/// [`Config::adapter_name`]. Independent of any running [`Capture`] (loads and unloads its own
/// handle to `wpcap.dll`).
pub fn list_adapters() -> Result<Vec<DeviceInfo>, Error> {
    let lib = NpcapLibrary::load()?;
    device::list_devices(&lib)
}

/// How to select and filter the adapter to capture on.
#[derive(Debug, Clone, Default)]
pub struct Config {
    /// Capture on this adapter by its libpcap name, bypassing automatic selection.
    pub adapter_name: Option<String>,
    /// TCP ports considered "the game server". Defaults to 20200-20300.
    pub port_range: PortRange,
}

/// Packet counters updated as the reader thread runs, read via [`Capture::counters`].
#[derive(Debug, Default)]
struct Counters {
    /// Every packet libpcap delivered (it has already applied the BPF port filter).
    seen: AtomicU64,
    /// Packets that turned into a `Segment` sent over the channel.
    kept: AtomicU64,
    /// Packets libpcap delivered that this crate could not turn into a `Segment` (a fragment, a
    /// payload-less segment with no SYN/FIN/RST, or a malformed frame).
    dropped: AtomicU64,
}

impl Counters {
    fn snapshot(&self) -> CounterSnapshot {
        CounterSnapshot {
            seen: self.seen.load(Ordering::Relaxed),
            kept: self.kept.load(Ordering::Relaxed),
            dropped: self.dropped.load(Ordering::Relaxed),
        }
    }
}

/// A point-in-time copy of the running packet counters.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct CounterSnapshot {
    pub seen: u64,
    pub kept: u64,
    pub dropped: u64,
}

/// A running capture: owns a background thread that reads frames off the chosen adapter, parses
/// them, and sends [`Segment`]s over a channel. Never injects, replays or otherwise sends any
/// traffic — it only reads.
pub struct Capture {
    adapter_name: String,
    counters: Arc<Counters>,
    fatal_error: Arc<OnceLock<Error>>,
    stop_flag: Arc<AtomicBool>,
    receiver: Receiver<Segment>,
    thread: Option<JoinHandle<()>>,
}

impl Capture {
    /// Loads Npcap, chooses an adapter per `config`, and starts capturing on a background
    /// thread. Returns once the adapter is open and filtered; capturing then happens
    /// concurrently until [`Capture::stop`] is called (or `self` is dropped).
    pub fn start(config: Config) -> Result<Self, Error> {
        let lib = Arc::new(NpcapLibrary::load()?);
        let devices = device::list_devices(&lib)?;
        let local_ip = interface::local_ipv4();
        let chosen =
            interface::choose_adapter(&devices, config.adapter_name.as_deref(), local_ip)?;
        let adapter_name = chosen.name.clone();

        let bpf_filter = format!(
            "tcp portrange {}-{}",
            config.port_range.start, config.port_range.end
        );
        let pcap_handle = PcapHandle::open(Arc::clone(&lib), &adapter_name, &bpf_filter)?;

        let (sender, receiver) = mpsc::channel();
        let counters = Arc::new(Counters::default());
        let fatal_error = Arc::new(OnceLock::new());
        let stop_flag = Arc::new(AtomicBool::new(false));

        let thread = {
            let counters = Arc::clone(&counters);
            let fatal_error = Arc::clone(&fatal_error);
            let stop_flag = Arc::clone(&stop_flag);
            let port_range = config.port_range;
            std::thread::Builder::new()
                .name("capture-reader".to_string())
                .spawn(move || {
                    reader_loop(pcap_handle, port_range, sender, counters, fatal_error, stop_flag);
                })
                .map_err(|e| Error::ThreadSpawnFailed(e.to_string()))?
        };

        Ok(Capture {
            adapter_name,
            counters,
            fatal_error,
            stop_flag,
            receiver,
            thread: Some(thread),
        })
    }

    /// The libpcap name of the adapter chosen (or explicitly requested via [`Config`]).
    pub fn adapter_name(&self) -> &str {
        &self.adapter_name
    }

    /// A snapshot of the running packet counters.
    pub fn counters(&self) -> CounterSnapshot {
        self.counters.snapshot()
    }

    /// The error that stopped the background thread on its own (e.g. the adapter disappeared).
    /// `None` while still running, and also `None` after an explicit [`Capture::stop`].
    pub fn fatal_error(&self) -> Option<Error> {
        self.fatal_error.get().cloned()
    }

    /// Blocks for up to `timeout` for the next segment.
    pub fn recv_timeout(&self, timeout: Duration) -> Result<Segment, RecvTimeoutError> {
        self.receiver.recv_timeout(timeout)
    }

    /// Drains whatever segments are already queued, without blocking.
    pub fn try_iter(&self) -> TryIter<'_, Segment> {
        self.receiver.try_iter()
    }

    /// Signals the reader thread to stop and waits for it to exit. Idempotent, and also run
    /// automatically on drop.
    pub fn stop(&mut self) {
        self.stop_flag.store(true, Ordering::Relaxed);
        if let Some(thread) = self.thread.take() {
            // A panic inside the reader thread isn't this crate's failure to report; joining
            // just needs to not propagate that panic into `stop()`.
            let _ = thread.join();
        }
    }
}

impl Drop for Capture {
    fn drop(&mut self) {
        self.stop();
    }
}

/// Reads and parses frames until told to stop or libpcap reports a hard error.
fn reader_loop(
    mut pcap_handle: PcapHandle,
    port_range: PortRange,
    sender: mpsc::Sender<Segment>,
    counters: Arc<Counters>,
    fatal_error: Arc<OnceLock<Error>>,
    stop_flag: Arc<AtomicBool>,
) {
    let start = Instant::now();
    let link_type = pcap_handle.datalink();

    while !stop_flag.load(Ordering::Relaxed) {
        match pcap_handle.next_packet() {
            Ok(Some((_header, data))) => {
                counters.seen.fetch_add(1, Ordering::Relaxed);
                let time = start.elapsed().as_secs_f64();
                match parse::parse_frame(link_type, data, port_range, time) {
                    Some(segment) => {
                        counters.kept.fetch_add(1, Ordering::Relaxed);
                        // If nobody is receiving anymore, keep reading anyway: counters and
                        // `adapter_name()` should stay queryable until `stop()` is called.
                        let _ = sender.send(segment);
                    }
                    None => {
                        counters.dropped.fetch_add(1, Ordering::Relaxed);
                    }
                }
            }
            // Read timeout: the normal, frequent case. Loop back around to re-check the stop
            // flag, which is what keeps `stop()` responsive within ~`READ_TIMEOUT_MS`.
            Ok(None) => {}
            Err(error) => {
                let _ = fatal_error.set(error);
                break;
            }
        }
    }
}
