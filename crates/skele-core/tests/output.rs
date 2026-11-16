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

mod log_files {
    use std::path::{Path, PathBuf};

    use serde_json::json;
    use skele_core::config::Scope;
    use skele_core::output::{log_file_name, WORKBENCH_CHANNEL};
    use skele_core::{Workbench, WorkbenchOptions};

    fn with(workspace: Option<&Path>, log_dir: Option<&Path>) -> Workbench {
        let options = WorkbenchOptions {
            workspace: workspace.map(Path::to_owned),
            log_dir: log_dir.map(Path::to_owned),
            ..Default::default()
        };
        Workbench::new(options).unwrap()
    }

    fn enable(workbench: &Workbench, dir: &str) {
        workbench.set_config("output.logDirectory", Some(json!(dir)), Scope::User).unwrap();
        workbench.set_config("output.logToFile", Some(json!(true)), Scope::User).unwrap();
    }

    fn file(run: &Path, channel: &str) -> String {
        std::fs::read_to_string(run.join(log_file_name(channel))).unwrap_or_default()
    }

    fn run_dir(workbench: &Workbench) -> PathBuf {
        workbench.log_dir().expect("logging is on")
    }

    #[test]
    fn startup_dir_writes_files() {
        let tmp = tempfile::tempdir().unwrap();
        let workbench = with(None, Some(tmp.path()));
        workbench.output("Hello", "hi\r\n");
        let run = run_dir(&workbench);
        assert_eq!(run.parent().unwrap(), tmp.path());
        assert_eq!(file(&run, "Hello"), "hi\r\n");
    }

    #[test]
    fn settings_need_both() {
        let tmp = tempfile::tempdir().unwrap();
        let workbench = with(None, None);
        workbench.set_config("output.logToFile", Some(json!(true)), Scope::User).unwrap();
        assert!(workbench.log_dir().is_none());
        assert_eq!(workbench.output_read(WORKBENCH_CHANNEL).unwrap(), "");
        workbench.set_config("output.logDirectory", Some(json!(tmp.path().to_str().unwrap())), Scope::User).unwrap();
        assert!(run_dir(&workbench).starts_with(tmp.path()));
    }

    #[test]
    fn directory_alone_is_not_enough() {
        let tmp = tempfile::tempdir().unwrap();
        let workbench = with(None, None);
        workbench.set_config("output.logDirectory", Some(json!(tmp.path().to_str().unwrap())), Scope::User).unwrap();
        assert!(workbench.log_dir().is_none());
    }

    #[test]
    fn enabling_midrun_writes_buffer_first() {
        let tmp = tempfile::tempdir().unwrap();
        let workbench = with(None, None);
        workbench.output("Hello", "a\r\n");
        enable(&workbench, tmp.path().to_str().unwrap());
        workbench.output("Hello", "b\r\n");
        assert_eq!(file(&run_dir(&workbench), "Hello"), "a\r\nb\r\n");
    }

    #[test]
    fn disable_stops_writing() {
        let tmp = tempfile::tempdir().unwrap();
        let workbench = with(None, None);
        enable(&workbench, tmp.path().to_str().unwrap());
        let run = run_dir(&workbench);
        workbench.output("Hello", "a\r\n");
        workbench.set_config("output.logToFile", Some(json!(false)), Scope::User).unwrap();
        assert!(workbench.log_dir().is_none());
        workbench.output("Hello", "b\r\n");
        assert_eq!(file(&run, "Hello"), "a\r\n");
    }

    #[test]
    fn reenabling_starts_a_new_run_folder() {
        let tmp = tempfile::tempdir().unwrap();
        let workbench = with(None, None);
        enable(&workbench, tmp.path().to_str().unwrap());
        let first = run_dir(&workbench);
        workbench.set_config("output.logToFile", Some(json!(false)), Scope::User).unwrap();
        workbench.set_config("output.logToFile", Some(json!(true)), Scope::User).unwrap();
        assert_ne!(run_dir(&workbench), first);
    }

    #[test]
    fn relative_dir_without_workspace_warns() {
        let workbench = with(None, None);
        enable(&workbench, "logs");
        assert!(workbench.log_dir().is_none());
        assert!(workbench.output_read(WORKBENCH_CHANNEL).unwrap().contains("[warning]"));
    }

    #[test]
    fn relative_dir_uses_workspace() {
        let tmp = tempfile::tempdir().unwrap();
        let workbench = with(Some(tmp.path()), None);
        enable(&workbench, "logs");
        assert!(run_dir(&workbench).starts_with(tmp.path().join("logs")));
    }

    #[test]
    fn unwritable_dir_warns() {
        let workbench = with(None, None);
        enable(&workbench, "/proc/openskele-cannot-write-here");
        assert!(workbench.log_dir().is_none());
        assert!(workbench.output_read(WORKBENCH_CHANNEL).unwrap().contains("[warning]"));
    }

    #[test]
    fn flag_wins_over_settings() {
        let (flag, other) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
        let workbench = with(None, Some(flag.path()));
        enable(&workbench, other.path().to_str().unwrap());
        workbench.set_config("output.logToFile", Some(json!(false)), Scope::User).unwrap();
        assert!(run_dir(&workbench).starts_with(flag.path()));
        assert_eq!(std::fs::read_dir(other.path()).unwrap().count(), 0);
    }

    #[test]
    fn empty_flag_dir_is_off() {
        let workbench = with(None, Some(Path::new("")));
        workbench.output("Hello", "hi\r\n");
        assert!(workbench.log_dir().is_none());
    }

    #[test]
    fn log_file_name_sanitises() {
        assert_eq!(log_file_name("a/b:c"), "a_b_c.log");
        assert_eq!(log_file_name("Runtime Info"), "Runtime Info.log");
    }
}
