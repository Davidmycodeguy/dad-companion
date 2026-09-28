//! Live smoke test: loads Npcap, lists adapters, and opens the chosen one with the real capture
//! filter for about a second, then stops cleanly.
//!
//! This talks to real hardware/drivers, so it treats "Npcap isn't installed here" as a skip
//! (print a note and return) rather than a failure. A permission error opening the adapter is
//! different: some Npcap installs require Administrator to capture, and that is a genuine,
//! useful result of running this test, so it is reported plainly (this test fails, with a clear
//! message) instead of being papered over.

use std::time::Duration;

use capture::{list_adapters, Capture, Config, Error};

#[test]
fn opens_the_chosen_adapter_briefly_and_stops_cleanly() {
    let devices = match list_adapters() {
        Ok(devices) => devices,
        Err(Error::NpcapNotFound(detail)) => {
            eprintln!("note: Npcap not available, skipping live smoke test ({detail})");
            return;
        }
        Err(other) => panic!("list_adapters failed: {other}"),
    };
    println!("Npcap loaded; {} adapter(s) visible", devices.len());

    let mut capture = match Capture::start(Config::default()) {
        Ok(capture) => capture,
        Err(Error::NpcapNotFound(detail)) => {
            eprintln!("note: Npcap not available, skipping live smoke test ({detail})");
            return;
        }
        Err(Error::PermissionDenied(detail)) => panic!(
            "opening the network adapter was denied: {detail}\n\
             This Npcap install appears to require running as Administrator to capture \
             packets. That is a real environment limitation, not a bug in this test."
        ),
        Err(other) => panic!("Capture::start failed: {other}"),
    };
    println!("capturing on adapter: {}", capture.adapter_name());
    assert!(!capture.adapter_name().is_empty());

    std::thread::sleep(Duration::from_secs(1));

    // Nothing is expected to arrive (the game almost certainly isn't running during this test);
    // draining just exercises the channel end-to-end.
    let drained = capture.try_iter().count();

    let counters = capture.counters();
    println!(
        "packets seen={} kept={} dropped={} (drained {drained} from the channel)",
        counters.seen, counters.kept, counters.dropped
    );

    capture.stop();
    assert!(
        capture.fatal_error().is_none(),
        "reader thread reported a fatal error: {:?}",
        capture.fatal_error()
    );
}
