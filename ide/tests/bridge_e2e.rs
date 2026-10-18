use std::time::Duration;

use gray::ide::UiCommand;
use gray_ide::bridge::{self, IdeCommand, IdeEvent};

fn recv_until<T>(
    events: &flume::Receiver<IdeEvent>,
    timeout: Duration,
    mut hit: impl FnMut(&IdeEvent) -> Option<T>,
) -> T {
    let deadline = std::time::Instant::now() + timeout;
    loop {
        let left = deadline.saturating_duration_since(std::time::Instant::now());
        assert!(!left.is_zero(), "timed out waiting for expected event");
        let ev = events.recv_timeout(left).expect("channel open");
        if let Some(t) = hit(&ev) {
            return t;
        }
    }
}

#[test]
fn bridge_e2e() {
    let home = tempfile::tempdir().expect("temp home");
    unsafe {
        std::env::set_var("GRAY_HOME", home.path());
        std::env::set_var("GRAY_USER_HOME", home.path());
        std::env::remove_var("GRAY_MODEL");
        std::env::remove_var("GRAY_API_KEY");
        std::env::remove_var("GRAY_BASE_URL");
        std::env::remove_var("GRAY_PROVIDER");
    }

    let (handle, events) = bridge::spawn(std::env::current_dir().expect("cwd"));

    recv_until(&events, Duration::from_secs(15), |ev| match ev {
        IdeEvent::Ready { label } => Some(label.clone()),
        _ => None,
    });
    recv_until(&events, Duration::from_secs(15), |ev| match ev {
        IdeEvent::Sessions(_) => Some(()),
        _ => None,
    });

    handle.submit("/help".into());
    let mut help = String::new();
    recv_until(&events, Duration::from_secs(30), |ev| match ev {
        IdeEvent::Output(line) => {
            help.push_str(line);
            help.push('\n');
            None
        }
        IdeEvent::TurnDone { .. } => Some(()),
        _ => None,
    });
    assert!(help.contains("/models"), "expected help text, got: {help}");

    handle.submit("/models".into());
    recv_until(&events, Duration::from_secs(30), |ev| match ev {
        IdeEvent::Ui(UiCommand::ModelPicker) => Some(()),
        _ => None,
    });

    handle.send(IdeCommand::ListModels);
    recv_until(&events, Duration::from_secs(15), |ev| match ev {
        IdeEvent::Models(rows) => Some(rows.clone()),
        _ => None,
    });

    handle.submit("hello".into());
    let echoed = recv_until(&events, Duration::from_secs(30), |ev| match ev {
        IdeEvent::UserEcho(text) => Some(text.clone()),
        _ => None,
    });
    assert_eq!(echoed, "hello");
    recv_until(&events, Duration::from_secs(30), |ev| match ev {
        IdeEvent::Ui(UiCommand::SetupWizard) => Some(()),
        _ => None,
    });

    handle.submit("/quit".into());
    recv_until(&events, Duration::from_secs(30), |ev| match ev {
        IdeEvent::Quit => Some(()),
        _ => None,
    });
}
