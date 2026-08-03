use std::{io, process::Child};

#[cfg(unix)]
use std::{
    process::Command,
    thread,
    time::{Duration, Instant},
};

/// Terminates a timed-out child and every process that inherited its group.
pub(super) fn terminate_timed_out_child(child: &mut Child, child_exited: bool) -> io::Result<()> {
    #[cfg(not(unix))]
    {
        return if child_exited { Ok(()) } else { child.kill() };
    }

    #[cfg(unix)]
    {
        let pgid = child.id();
        if pgid == 0 {
            return if child_exited { Ok(()) } else { child.kill() };
        }

        if let Err(err) = send_process_group_signal(pgid, "TERM") {
            return if child_exited { Ok(()) } else { Err(err) };
        }

        let deadline = Instant::now() + Duration::from_millis(100);
        let mut leader_exited = child_exited;
        while Instant::now() < deadline {
            if !leader_exited {
                leader_exited = child.try_wait()?.is_some();
            }
            thread::sleep(Duration::from_millis(10));
        }

        if !leader_exited {
            leader_exited = child.try_wait()?.is_some();
        }
        match send_process_group_signal(pgid, "KILL") {
            Ok(()) => Ok(()),
            Err(_) if leader_exited => Ok(()),
            Err(err) => Err(err),
        }
    }
}

#[cfg(unix)]
fn send_process_group_signal(pgid: u32, signal: &str) -> io::Result<()> {
    let output = Command::new("kill")
        .arg(format!("-{signal}"))
        .arg("--")
        .arg(format!("-{pgid}"))
        .output()?;
    if output.status.success() {
        Ok(())
    } else {
        Err(io::Error::other(format!(
            "kill {signal} process group {pgid} exited with {}",
            output.status
        )))
    }
}
