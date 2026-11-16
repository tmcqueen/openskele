//! Output channels through the workbench: events, errors and host messages.

use skele_core::output::WORKBENCH_CHANNEL;
use skele_core::protocol::methods::MessageLevel;
use skele_core::{Error, HostEvent, Workbench};

fn workbench() -> Workbench {
    Workbench::new(Default::default()).unwrap()
}

#[test]
fn workbench_channel_exists_at_startup() {
    let workbench = workbench();
    assert_eq!(workbench.output_channels(), [WORKBENCH_CHANNEL]);
    assert_eq!(workbench.output_read(WORKBENCH_CHANNEL).unwrap(), "");
}

#[test]
fn output_emits_events() {
    let workbench = workbench();
    let mut events = workbench.bus().subscribe();
    workbench.output("Hello", "hi\r\n");
    workbench.output("Hello", "again\r\n");
    let received: Vec<_> = std::iter::from_fn(|| events.try_recv().ok()).collect();
    assert!(matches!(received[0], HostEvent::OutputChannelsChanged));
    assert!(
        matches!(&received[1], HostEvent::OutputAppended { channel, text } if channel == "Hello" && text == "hi\r\n")
    );
    assert!(matches!(&received[2], HostEvent::OutputAppended { text, .. } if text == "again\r\n"));
    assert_eq!(received.len(), 3);
    assert_eq!(workbench.output_read("Hello").unwrap(), "hi\r\nagain\r\n");
}

#[test]
fn read_unknown_channel_errors() {
    assert!(matches!(workbench().output_read("nope"), Err(Error::UnknownChannel(_))));
}

#[test]
fn clear_emits_cleared() {
    let workbench = workbench();
    workbench.output("Hello", "hi\r\n");
    let mut events = workbench.bus().subscribe();
    workbench.output_clear("Hello").unwrap();
    assert!(matches!(events.try_recv(), Ok(HostEvent::OutputCleared { channel }) if channel == "Hello"));
    assert_eq!(workbench.output_read("Hello").unwrap(), "");
    assert!(matches!(workbench.output_clear("nope"), Err(Error::UnknownChannel(_))));
}

#[test]
fn notify_logs_to_workbench() {
    let workbench = workbench();
    let mut events = workbench.bus().subscribe();
    workbench.notify(MessageLevel::Warning, "careful", Some("Hello"));
    assert!(matches!(events.try_recv(), Ok(HostEvent::Message { .. })));
    assert_eq!(workbench.output_read(WORKBENCH_CHANNEL).unwrap(), "[warning] Hello: careful\r\n");
}
