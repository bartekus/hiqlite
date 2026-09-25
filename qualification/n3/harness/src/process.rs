//! Node processes: spawn, signal, observe, and the control-socket client.

use n3_proto::{CtlRequest, CtlResponse, NodeStatus, SHUTDOWN_FILE, STATUS_FILE, ShutdownRecord};
use serde::Serialize;
use std::os::unix::process::ExitStatusExt;
use std::path::{Path, PathBuf};
use std::process::{ExitStatus, Stdio};
use std::time::Duration;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::TcpStream;
use tokio::process::{Child, Command};

#[derive(Debug, Clone, Serialize)]
pub struct ExitInfo {
    pub code: Option<i32>,
    pub signal: Option<i32>,
}

impl ExitInfo {
    pub fn from_status(s: ExitStatus) -> Self {
        Self {
            code: s.code(),
            signal: s.signal(),
        }
    }
}

impl std::fmt::Display for ExitInfo {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match (self.code, self.signal) {
            (Some(c), _) => write!(f, "exit code {c}"),
            (None, Some(s)) => write!(f, "signal {s}"),
            _ => f.write_str("unknown exit"),
        }
    }
}

pub struct NodeProc {
    pub key: usize,
    pub name: String,
    pub pod: usize,
    pub dir: PathBuf,
    pub bin: PathBuf,
    pub rust_log: String,
    pub ctl_addr: String,
    pub child: Option<Child>,
    pub pid: Option<u32>,
    /// Number of times this node has been spawned in the run.
    pub incarnation: u32,
    pub last_exit: Option<ExitInfo>,
}

impl NodeProc {
    pub fn launch_path(&self) -> PathBuf {
        self.dir.join(n3_proto::LAUNCH_FILE)
    }

    fn status_path(&self) -> PathBuf {
        self.dir.join(STATUS_FILE)
    }

    fn shutdown_path(&self) -> PathBuf {
        self.dir.join(SHUTDOWN_FILE)
    }

    /// Starts a new incarnation. The previous incarnation's status and shutdown files are kept
    /// under a numbered name, so the evidence of every stop survives the restart.
    pub fn spawn(&mut self) -> Result<u32, String> {
        if self.child.is_some() {
            return Err(format!("{} is already running", self.name));
        }
        if self.incarnation > 0 {
            for (p, stem) in [
                (self.status_path(), "status"),
                (self.shutdown_path(), "shutdown"),
            ] {
                if p.exists() {
                    let to = self.dir.join(format!("{stem}.{}.json", self.incarnation));
                    std::fs::rename(&p, &to).map_err(|e| format!("archiving {p:?}: {e}"))?;
                }
            }
        }
        self.incarnation += 1;
        let log_path = self.dir.join(format!("node.{}.log", self.incarnation));
        let log = std::fs::File::create(&log_path).map_err(|e| format!("{log_path:?}: {e}"))?;
        let log_err = log.try_clone().map_err(|e| e.to_string())?;
        let child = Command::new(&self.bin)
            .arg("--launch")
            .arg(self.launch_path())
            .env("RUST_LOG", &self.rust_log)
            .current_dir(&self.dir)
            .stdin(Stdio::null())
            .stdout(Stdio::from(log))
            .stderr(Stdio::from(log_err))
            // A harness that is itself dropped does not leave its nodes behind.
            .kill_on_drop(true)
            .spawn()
            .map_err(|e| format!("spawning {}: {e}", self.name))?;
        let pid = child.id().ok_or("spawned child has no pid")?;
        self.pid = Some(pid);
        self.child = Some(child);
        self.last_exit = None;
        Ok(pid)
    }

    pub fn is_running(&self) -> bool {
        self.child.is_some()
    }

    /// Reaps the child if it exited. Returns the exit when it happened now.
    pub fn poll_exit(&mut self) -> Option<ExitInfo> {
        let child = self.child.as_mut()?;
        match child.try_wait() {
            Ok(Some(status)) => {
                let info = ExitInfo::from_status(status);
                self.child = None;
                self.last_exit = Some(info.clone());
                Some(info)
            }
            _ => None,
        }
    }

    /// `Err` when a node the caller expects to be running is not.
    pub fn check_alive(&mut self) -> Result<(), String> {
        if let Some(exit) = self.poll_exit() {
            return Err(format!("{} exited unexpectedly: {exit}", self.name));
        }
        if self.child.is_none() {
            return Err(format!("{} is not running", self.name));
        }
        Ok(())
    }

    /// Sends a signal to this harness's own child, never to any other process.
    pub fn signal(&self, sig: i32) -> Result<(), String> {
        let (Some(_), Some(pid)) = (&self.child, self.pid) else {
            return Err(format!("{} has no running process to signal", self.name));
        };
        let r = unsafe { libc::kill(pid as libc::pid_t, sig) };
        if r != 0 {
            return Err(format!(
                "kill({pid}, {sig}) for {}: {}",
                self.name,
                std::io::Error::last_os_error()
            ));
        }
        Ok(())
    }

    /// The node's last status, only if it was written by the current incarnation's process.
    pub fn status(&self) -> Option<NodeStatus> {
        let s: NodeStatus = read_json(&self.status_path())?;
        (Some(s.pid) == self.pid).then_some(s)
    }

    /// The status file as it stands, whichever process wrote it.
    pub fn status_any(&self) -> Option<NodeStatus> {
        read_json(&self.status_path())
    }

    pub fn shutdown_record(&self) -> Option<ShutdownRecord> {
        read_json(&self.shutdown_path())
    }

    pub async fn ctl(&self, req: &CtlRequest, bound: Duration) -> Result<CtlResponse, String> {
        let fut = async {
            let mut s = TcpStream::connect(&self.ctl_addr)
                .await
                .map_err(|e| format!("connect {}: {e}", self.ctl_addr))?;
            let mut line = serde_json::to_string(req).map_err(|e| e.to_string())?;
            line.push('\n');
            s.write_all(line.as_bytes())
                .await
                .map_err(|e| e.to_string())?;
            let mut lines = BufReader::new(s).lines();
            let resp = lines
                .next_line()
                .await
                .map_err(|e| e.to_string())?
                .ok_or("control connection closed without an answer")?;
            serde_json::from_str::<CtlResponse>(&resp).map_err(|e| format!("bad answer: {e}"))
        };
        match tokio::time::timeout(bound, fut).await {
            Ok(r) => r.map_err(|e| format!("{}: {e}", self.name)),
            Err(_) => Err(format!(
                "{}: control request {req:?} did not answer within {bound:?}",
                self.name
            )),
        }
    }
}

fn read_json<T: serde::de::DeserializeOwned>(p: &Path) -> Option<T> {
    serde_json::from_slice(&std::fs::read(p).ok()?).ok()
}
