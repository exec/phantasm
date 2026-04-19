use anyhow::{Context, Result};
use std::io::Read;

#[derive(Debug, Default, Clone)]
pub struct PassphraseSource {
    pub direct: Option<String>,
    pub env_var: Option<String>,
    pub fd: Option<i32>,
}

impl PassphraseSource {
    pub fn is_empty(&self) -> bool {
        self.direct.is_none() && self.env_var.is_none() && self.fd.is_none()
    }

    pub fn count_set(&self) -> usize {
        [
            self.direct.is_some(),
            self.env_var.is_some(),
            self.fd.is_some(),
        ]
        .iter()
        .filter(|b| **b)
        .count()
    }

    pub fn resolve(&self) -> Result<String> {
        if self.count_set() > 1 {
            anyhow::bail!(
                "--passphrase, --passphrase-env, and --passphrase-fd are mutually exclusive"
            );
        }
        if let Some(direct) = &self.direct {
            return Ok(direct.clone());
        }
        if let Some(var) = &self.env_var {
            let val = std::env::var(var).with_context(|| {
                format!(
                    "failed to read passphrase from environment variable {}",
                    var
                )
            })?;
            if val.is_empty() {
                anyhow::bail!("environment variable {} is empty", var);
            }
            return Ok(val);
        }
        if let Some(fd) = self.fd {
            return read_from_fd(fd);
        }
        anyhow::bail!("no passphrase source provided");
    }
}

#[cfg(unix)]
fn read_from_fd(fd: i32) -> Result<String> {
    use std::os::fd::FromRawFd;
    if fd < 0 {
        anyhow::bail!(
            "--passphrase-fd requires a non-negative file descriptor (got {})",
            fd
        );
    }
    // SAFETY: caller explicitly passed this fd. We take ownership so the File's
    // Drop closes it. Standard pattern for --password-fd flags.
    let mut file = unsafe { std::fs::File::from_raw_fd(fd) };
    let mut buf = String::new();
    file.read_to_string(&mut buf)
        .with_context(|| format!("failed to read passphrase from fd {}", fd))?;
    if buf.ends_with('\n') {
        buf.pop();
        if buf.ends_with('\r') {
            buf.pop();
        }
    }
    if buf.is_empty() {
        anyhow::bail!("passphrase read from fd {} is empty", fd);
    }
    Ok(buf)
}

#[cfg(not(unix))]
fn read_from_fd(_fd: i32) -> Result<String> {
    anyhow::bail!("--passphrase-fd is only supported on Unix platforms")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resolve_direct() {
        let src = PassphraseSource {
            direct: Some("hunter2".into()),
            ..Default::default()
        };
        assert_eq!(src.resolve().unwrap(), "hunter2");
    }

    #[test]
    fn resolve_env() {
        let var = "PHANTASM_TEST_PASSPHRASE_ENV_OK";
        // SAFETY: test-only; unique var name avoids cross-test races.
        unsafe { std::env::set_var(var, "from-env-value") };
        let src = PassphraseSource {
            env_var: Some(var.into()),
            ..Default::default()
        };
        let got = src.resolve();
        unsafe { std::env::remove_var(var) };
        assert_eq!(got.unwrap(), "from-env-value");
    }

    #[test]
    fn resolve_env_missing_errors() {
        let src = PassphraseSource {
            env_var: Some("PHANTASM_TEST_DEFINITELY_NOT_SET_ZZZZ".into()),
            ..Default::default()
        };
        assert!(src.resolve().is_err());
    }

    #[test]
    fn resolve_env_empty_errors() {
        let var = "PHANTASM_TEST_PASSPHRASE_ENV_EMPTY";
        unsafe { std::env::set_var(var, "") };
        let src = PassphraseSource {
            env_var: Some(var.into()),
            ..Default::default()
        };
        let err = src.resolve();
        unsafe { std::env::remove_var(var) };
        assert!(err.unwrap_err().to_string().contains("empty"));
    }

    #[test]
    fn mutual_exclusion() {
        let src = PassphraseSource {
            direct: Some("x".into()),
            env_var: Some("Y".into()),
            fd: None,
        };
        let err = src.resolve().unwrap_err();
        assert!(err.to_string().contains("mutually exclusive"));
    }

    #[test]
    fn empty_source_errors() {
        let src = PassphraseSource::default();
        assert!(src.resolve().is_err());
    }

    #[cfg(unix)]
    #[test]
    fn resolve_fd_reads_and_trims_newline() {
        use std::io::Write;
        use std::os::fd::FromRawFd;

        let (read_fd, write_fd) = make_pipe();
        // SAFETY: we own write_fd; File::Drop will close it.
        let mut writer = unsafe { std::fs::File::from_raw_fd(write_fd) };
        writer.write_all(b"secret\n").unwrap();
        drop(writer); // close write end so read_to_string returns

        let src = PassphraseSource {
            fd: Some(read_fd),
            ..Default::default()
        };
        assert_eq!(src.resolve().unwrap(), "secret");
    }

    #[cfg(unix)]
    #[test]
    fn resolve_fd_no_trailing_newline() {
        use std::io::Write;
        use std::os::fd::FromRawFd;

        let (read_fd, write_fd) = make_pipe();
        let mut writer = unsafe { std::fs::File::from_raw_fd(write_fd) };
        writer.write_all(b"no-nl").unwrap();
        drop(writer);

        let src = PassphraseSource {
            fd: Some(read_fd),
            ..Default::default()
        };
        assert_eq!(src.resolve().unwrap(), "no-nl");
    }

    #[cfg(unix)]
    #[test]
    fn resolve_fd_crlf_trimmed() {
        use std::io::Write;
        use std::os::fd::FromRawFd;

        let (read_fd, write_fd) = make_pipe();
        let mut writer = unsafe { std::fs::File::from_raw_fd(write_fd) };
        writer.write_all(b"windows\r\n").unwrap();
        drop(writer);

        let src = PassphraseSource {
            fd: Some(read_fd),
            ..Default::default()
        };
        assert_eq!(src.resolve().unwrap(), "windows");
    }

    #[cfg(unix)]
    #[test]
    fn resolve_fd_empty_errors() {
        use std::os::fd::FromRawFd;

        let (read_fd, write_fd) = make_pipe();
        // Close writer with no data -> reader sees EOF -> empty string.
        let writer = unsafe { std::fs::File::from_raw_fd(write_fd) };
        drop(writer);

        let src = PassphraseSource {
            fd: Some(read_fd),
            ..Default::default()
        };
        let err = src.resolve().unwrap_err();
        assert!(err.to_string().contains("empty"));
    }

    #[cfg(unix)]
    #[test]
    fn resolve_fd_negative_errors() {
        let src = PassphraseSource {
            fd: Some(-1),
            ..Default::default()
        };
        assert!(src.resolve().is_err());
    }

    #[cfg(unix)]
    fn make_pipe() -> (i32, i32) {
        extern "C" {
            fn pipe(fds: *mut i32) -> i32;
        }
        let mut fds = [0i32; 2];
        // SAFETY: valid pointer to 2-element array.
        let rc = unsafe { pipe(fds.as_mut_ptr()) };
        assert_eq!(rc, 0, "pipe() failed");
        (fds[0], fds[1])
    }
}
