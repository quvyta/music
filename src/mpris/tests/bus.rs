//! Starting where there is no session bus to start on.

use crate::mpris::Server;
use crate::testing::Scratch;

/// What tells the test in the child process that its part is the one to do.
const CHILD: &str = "QMUS_MPRIS_WITHOUT_A_BUS";

/// The name of this test, as the test binary knows it. The child runs this very test again, with
/// the bus taken away from it.
const NAME: &str = "mpris::tests::bus::starting_where_there_is_no_session_bus_gives_nothing_and_says_nothing";

#[test]
fn starting_where_there_is_no_session_bus_gives_nothing_and_says_nothing() {
    if std::env::var(CHILD).is_ok() {
        assert!(Server::start(|_| {}).is_none(), "there is no bus here to be found");
        return;
    }
    // The address of the session bus belongs to the process, and this process is running the
    // whole test suite; taking it away here would take it away from the other tests too. The
    // child process is the one that gets an address with no bus behind it, which is the state a
    // qmus over SSH is in.
    let scratch = Scratch::new("mpris-no-bus");
    let address = format!("unix:path={}", scratch.path("no-bus-here").display());
    let output = std::process::Command::new(std::env::current_exe().expect("the test binary"))
        .args(["--exact", NAME, "--nocapture"])
        .env(CHILD, "1")
        .env("DBUS_SESSION_BUS_ADDRESS", address)
        .output()
        .expect("the child runs this test again");
    let said = String::from_utf8_lossy(&output.stdout);
    assert!(said.contains(&format!("test {NAME} ... ok\n")), "the child said:\n{said}");
    assert!(output.status.success(), "the child failed:\n{}", String::from_utf8_lossy(&output.stderr));
}
