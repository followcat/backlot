//! Host a compiled binary in a PTY: `openpty` for the geometry, a
//! `login_tty` child on the slave, and a pump that feeds the master
//! into a `hc-vt` screen. Neutral fixture code — no real sessions.

#![cfg(unix)]
#![allow(dead_code)]

use hc_vt::Vt;
use std::ffi::OsStr;
use std::os::fd::{AsRawFd, FromRawFd, OwnedFd};
use std::os::unix::process::CommandExt;
use std::path::Path;
use std::process::{Child, Command, Stdio};
use std::time::Duration;

/// Read pump cadence while waiting on a screen or a file.
pub const POLL: Duration = Duration::from_millis(40);

/// A pty pair at `cols`x`rows`: nonblocking master for the pump, slave
/// for the child's stdio. `openpty` applies the size atomically.
// Apple's openpty takes *mut winsize; the &mut is needed there and
// harmless on Linux where it coerces to *const.
#[allow(clippy::unnecessary_mut_passed)]
pub fn openpty(cols: u16, rows: u16) -> (OwnedFd, OwnedFd) {
    unsafe {
        let mut ws = libc::winsize {
            ws_col: cols,
            ws_row: rows,
            ws_xpixel: 0,
            ws_ypixel: 0,
        };
        let (mut master, mut slave) = (-1, -1);
        assert_eq!(
            libc::openpty(
                &mut master,
                &mut slave,
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                &mut ws,
            ),
            0,
            "openpty: {:?}",
            std::io::Error::last_os_error()
        );
        let flags = libc::fcntl(master, libc::F_GETFL);
        assert_eq!(
            libc::fcntl(master, libc::F_SETFL, flags | libc::O_NONBLOCK),
            0
        );
        (OwnedFd::from_raw_fd(master), OwnedFd::from_raw_fd(slave))
    }
}

/// A child hosted in a PTY: own session, controlling terminal, all
/// three stdio streams on the slave.
pub struct Pty {
    pub master: OwnedFd,
    slave: OwnedFd,
    pub child: Child,
}

impl Pty {
    /// Spawns `program` with `args` under a cleared environment holding
    /// only `envs`, cwd `work`, in a `cols`x`rows` PTY.
    pub fn spawn(
        program: &Path,
        args: &[&str],
        envs: &[(String, String)],
        work: &Path,
        cols: u16,
        rows: u16,
    ) -> Pty {
        let (master, slave) = openpty(cols, rows);
        let sfd = slave.as_raw_fd();
        let dup = || unsafe { Stdio::from_raw_fd(libc::dup(sfd)) };
        let mut command = Command::new(program);
        command
            .args(args)
            .env_clear()
            .envs(envs.iter().map(|(k, v)| (OsStr::new(k), OsStr::new(v))))
            .current_dir(work)
            .stdin(dup())
            .stdout(dup())
            .stderr(dup());
        unsafe {
            command.pre_exec(move || {
                // New session + controlling tty, matching a terminal
                // launcher: login_tty is setsid + TIOCSCTTY + stdio
                // dup on every Unix.
                if libc::login_tty(sfd) != 0 {
                    return Err(std::io::Error::last_os_error());
                }
                Ok(())
            });
        }
        let child = command
            .spawn()
            .unwrap_or_else(|e| panic!("cannot spawn {}: {e}", program.display()));
        Pty {
            master,
            slave,
            child,
        }
    }

    /// Everything the child wrote since the last pump, fed to `vt`;
    /// replies the emulated terminal owes (queries, reports) go back.
    pub fn pump(&self, vt: &mut Vt) {
        let mut buf = [0u8; 65536];
        loop {
            let n =
                unsafe { libc::read(self.master.as_raw_fd(), buf.as_mut_ptr().cast(), buf.len()) };
            if n <= 0 {
                break;
            }
            vt.feed(&buf[..n as usize]);
        }
        let replies = vt.take_replies().unwrap_or_default();
        if !replies.is_empty() {
            unsafe {
                libc::write(
                    self.master.as_raw_fd(),
                    replies.as_ptr().cast(),
                    replies.len(),
                );
            }
        }
    }

    /// Keystrokes for the hosted program.
    pub fn type_keys(&self, keys: &[u8]) {
        let mut written = 0;
        while written < keys.len() {
            let n = unsafe {
                libc::write(
                    self.master.as_raw_fd(),
                    keys[written..].as_ptr().cast(),
                    keys.len() - written,
                )
            };
            assert!(n > 0, "pty write failed");
            written += n as usize;
        }
    }

    /// The slave fd — held so a final termios check still has a
    /// handle after the child exits.
    pub fn slave_fd(&self) -> &OwnedFd {
        &self.slave
    }
}

/// The whole emulated screen as text rows.
pub fn screen(vt: &Vt) -> String {
    let (cols, rows) = vt.size().expect("vt size");
    (0..rows)
        .map(|row| vt.screen_row_text(row.into(), cols).unwrap_or_default())
        .collect::<Vec<_>>()
        .join("\n")
}
