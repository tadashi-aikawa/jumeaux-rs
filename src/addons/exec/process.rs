//! 専用プロセスグループと非ブロッキングpipe。全I/Oと終了待ちに同じ時計を使う。
use super::{Invocation, Prepared, Runtime, Stderr};
use crate::compat::{Error, Result};
#[cfg(unix)]
use std::os::{
    fd::{AsRawFd, RawFd},
    unix::process::CommandExt,
};
use std::{
    collections::VecDeque,
    io::{Read, Write},
    process::{Child, Command, Stdio},
    time::{Duration, Instant},
};

pub(super) struct Output {
    pub bytes: Vec<u8>,
    pub tail: Vec<u8>,
}
fn io_error(e: std::io::Error) -> Error {
    Error::new("ExecAddonError", e.to_string())
}
#[cfg(unix)]
fn nonblocking(fd: RawFd) -> Result<()> {
    // SAFETY: fdはこの呼び出しが所有する開いたpipe。フラグだけを変更する。
    unsafe {
        let flags = libc::fcntl(fd, libc::F_GETFL);
        if flags < 0 || libc::fcntl(fd, libc::F_SETFL, flags | libc::O_NONBLOCK) < 0 {
            return Err(io_error(std::io::Error::last_os_error()));
        }
    }
    Ok(())
}
struct OwnedChild {
    child: Child,
    armed: bool,
}
impl OwnedChild {
    #[cfg(unix)]
    fn signal(&self, signal: i32) {
        // SAFETY: spawnでprocess_group(0)を設定した、この呼び出し専用のPGID。
        unsafe {
            libc::kill(-(self.child.id() as i32), signal);
        }
    }
}
impl Drop for OwnedChild {
    fn drop(&mut self) {
        if self.armed {
            #[cfg(unix)]
            self.signal(libc::SIGTERM);
            std::thread::sleep(Duration::from_secs(1));
            #[cfg(unix)]
            self.signal(libc::SIGKILL);
            let _ = self.child.kill();
            let _ = self.child.wait();
        }
    }
}
struct Diagnostics<'a> {
    tail: VecDeque<u8>,
    pending: Vec<u8>,
    continued: bool,
    runtime: &'a Runtime,
    prepared: &'a Prepared,
    invocation: Invocation,
}
impl Diagnostics<'_> {
    fn accept(&mut self, bytes: &[u8]) {
        for &byte in bytes {
            if self.prepared.config.max_stderr_bytes > 0 {
                if self.tail.len() == self.prepared.config.max_stderr_bytes {
                    self.tail.pop_front();
                }
                self.tail.push_back(byte);
            }
            self.pending.push(byte);
            if byte == b'\n' {
                self.emit(true);
            } else if self.pending.len() >= 8192 {
                // UTF-8の途中だけを次のチャンクへ持ち越す。
                let split = match std::str::from_utf8(&self.pending) {
                    Err(e) if e.error_len().is_none() => e.valid_up_to(),
                    _ => self.pending.len(),
                };
                let rest = self.pending.split_off(split);
                self.emit(false);
                self.pending = rest;
            }
        }
    }
    fn emit(&mut self, end: bool) {
        if !self.pending.is_empty() {
            (self.runtime.observer)(&Stderr {
                layer: self.prepared.layer,
                index: self.prepared.index,
                trial_seq: self.invocation.trial_seq,
                side: self.invocation.side,
                continued: self.continued,
                text: String::from_utf8_lossy(&self.pending).into_owned(),
            });
            self.pending.clear();
        }
        self.continued = !end;
    }
}
#[cfg(unix)]
pub(super) fn run(
    runtime: &Runtime,
    prepared: &Prepared,
    invocation: Invocation,
    input: &[u8],
) -> Result<Output> {
    runtime.check_cancelled()?;
    let start = Instant::now();
    let mut command = Command::new(&prepared.executable);
    command
        .args(&prepared.config.command[1..])
        .current_dir(&prepared.cwd)
        .env_clear()
        .envs(&prepared.env)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .process_group(0);
    let mut owned = OwnedChild {
        child: command.spawn().map_err(io_error)?,
        armed: true,
    };
    let mut stdin = owned.child.stdin.take();
    let mut stdout = owned.child.stdout.take().unwrap();
    let mut stderr = owned.child.stderr.take().unwrap();
    nonblocking(stdin.as_ref().unwrap().as_raw_fd())?;
    nonblocking(stdout.as_raw_fd())?;
    nonblocking(stderr.as_raw_fd())?;
    let mut diagnostics = Diagnostics {
        tail: VecDeque::new(),
        pending: Vec::new(),
        continued: false,
        runtime,
        prepared,
        invocation,
    };
    let mut bytes = Vec::new();
    let (mut offset, mut out_eof, mut err_eof) = (0, false, false);
    let mut status = None;
    let mut write_error = None;
    let result = (|| {
        loop {
            runtime.check_cancelled()?;
            if start.elapsed().as_secs_f64() >= prepared.config.timeout_sec {
                return Err(Error::new(
                    "ExecAddonTimeout",
                    format!("timeout_sec={} exceeded", prepared.config.timeout_sec),
                ));
            }
            let mut progress = false;
            if let Some(writer) = stdin.as_mut() {
                match writer.write(&input[offset..input.len().min(offset + 8192)]) {
                    Ok(n) => {
                        offset += n;
                        progress |= n > 0;
                        if offset == input.len() {
                            stdin = None;
                        }
                    }
                    Err(e)
                        if matches!(
                            e.kind(),
                            std::io::ErrorKind::WouldBlock | std::io::ErrorKind::Interrupted
                        ) => {}
                    Err(e) => {
                        write_error = Some(io_error(e));
                        stdin = None;
                    }
                }
            }
            let mut buffer = [0; 8192];
            for (pipe, eof, is_err) in [
                (&mut stdout as &mut dyn Read, &mut out_eof, false),
                (&mut stderr as &mut dyn Read, &mut err_eof, true),
            ] {
                if *eof {
                    continue;
                }
                match pipe.read(&mut buffer) {
                    Ok(0) => {
                        *eof = true;
                        progress = true;
                    }
                    Ok(n) => {
                        progress = true;
                        if is_err {
                            diagnostics.accept(&buffer[..n]);
                        } else {
                            if n > prepared.config.max_stdout_bytes.saturating_sub(bytes.len()) {
                                return Err(super::json::protocol("$: max_stdout_bytes exceeded"));
                            }
                            bytes.extend_from_slice(&buffer[..n]);
                        }
                    }
                    Err(e)
                        if matches!(
                            e.kind(),
                            std::io::ErrorKind::WouldBlock | std::io::ErrorKind::Interrupted
                        ) => {}
                    Err(e) => return Err(io_error(e)),
                }
            }
            if status.is_none() {
                status = owned.child.try_wait().map_err(io_error)?;
            }
            if let Some(status) = status {
                if !status.success() {
                    return Err(Error::new(
                        "ExecAddonError",
                        format!("process exited with {status}"),
                    ));
                }
                if out_eof && err_eof {
                    if let Some(error) = write_error {
                        return Err(error);
                    }
                    if offset != input.len() {
                        return Err(Error::new("ExecAddonError", "stdin was not fully consumed"));
                    }
                    return Ok(());
                }
            }
            if !progress {
                std::thread::sleep(Duration::from_millis(1));
            }
        }
    })();
    if result.is_err() {
        stdin.take();
        owned.signal(libc::SIGTERM);
        let grace = Instant::now();
        // 終了猶予中もstderrを流す。子孫がpipeを保持しても1秒で打ち切る。
        while grace.elapsed() < Duration::from_secs(1) {
            let mut buffer = [0; 8192];
            match stderr.read(&mut buffer) {
                Ok(n) if n > 0 => diagnostics.accept(&buffer[..n]),
                _ => std::thread::sleep(Duration::from_millis(2)),
            }
        }
        owned.signal(libc::SIGKILL);
        let _ = owned.child.wait();
    }
    owned.armed = false;
    diagnostics.emit(false);
    let tail: Vec<_> = diagnostics.tail.into_iter().collect();
    result.map_err(|e| super::with_tail(e, &tail))?;
    Ok(Output { bytes, tail })
}
#[cfg(not(unix))]
pub(super) fn run(_: &Runtime, _: &Prepared, _: Invocation, _: &[u8]) -> Result<Output> {
    Err(Error::new(
        "ConfigError",
        "external command addons support macOS and Linux only",
    ))
}
