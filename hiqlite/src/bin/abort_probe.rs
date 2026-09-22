//! Runs hiqlite's expected failure paths under a `panic = "abort"` profile.
//!
//! This exists because **no test in this repository can run under abort**: Rust's test harness
//! requires unwinding, so `cargo test` always builds with `panic = "unwind"` whatever the
//! profile says. An expected failure that is a panic therefore looks identical to one that is a
//! returned error when it is run by a test, and the difference only shows up in a consumer's
//! build.
//!
//! Built with `--release`, this binary inherits the workspace's `[profile.release]
//! panic = "abort"`. Every call below is a failure hiqlite is expected to report. If any of
//! them panics, this process aborts and the exit status says so; if all of them return errors,
//! it prints them and exits `0`.
//!
//! Built only with the internal `__abort-probe` feature, so it is not part of any normal build.

fn main() {
    let mut failures = 0usize;

    // `abort` is the whole point of the binary, so say what it is running under.
    println!("probe: panic strategy is abort (release profile)");

    // A malformed split-brain interval. F-009: this was
    // `.expect("Cannot parse HQL_SPLIT_BRAIN_INTERVAL as u64")` inside a spawned task, so under
    // this profile it ended the process.
    match hiqlite::probe::split_brain_interval("not a number") {
        Ok(_) => {
            println!("FAIL: a malformed split-brain interval was accepted");
            failures += 1;
        }
        Err(err) => println!("ok: split-brain interval -> {err}"),
    }
    match hiqlite::probe::split_brain_interval("0") {
        Ok(_) => {
            println!("FAIL: a zero split-brain interval was accepted");
            failures += 1;
        }
        Err(err) => println!("ok: zero split-brain interval -> {err}"),
    }

    // An S3 configuration with the URL set and nothing else. F-061: five `expect`s and an
    // `unwrap` followed the URL reading successfully.
    #[cfg(feature = "s3")]
    {
        match hiqlite::probe::s3_config_from(&[("HQL_S3_URL", "https://s3.example.com")]) {
            Ok(_) => {
                println!("FAIL: an incomplete S3 configuration was accepted");
                failures += 1;
            }
            Err(err) => println!("ok: incomplete S3 configuration -> {err}"),
        }
    }

    // A listener address that cannot be parsed, and one that cannot be bound. F-040: the parse,
    // the bind and `serve` were all `expect`/`unwrap` inside tasks whose handles were dropped.
    let rt = match tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
    {
        Ok(rt) => rt,
        Err(err) => {
            println!("FAIL: cannot build a runtime for the probe: {err}");
            std::process::exit(1);
        }
    };
    rt.block_on(async {
        match hiqlite::probe::bind("this is not an address").await {
            Ok(_) => {
                println!("FAIL: an unparsable listen address was accepted");
                failures += 1;
            }
            Err(err) => println!("ok: unparsable listen address -> {err}"),
        }

        // Port 1 is privileged, so binding it as a normal user fails. If this probe is run as
        // root it will succeed, which is why the failure is reported rather than asserted.
        match hiqlite::probe::bind("127.0.0.1:1").await {
            Ok(_) => println!("skip: binding a privileged port succeeded (running as root?)"),
            Err(err) => println!("ok: unbindable listen address -> {err}"),
        }

        // A storage directory owned by this same process. `024`: a second node in one process
        // is the same hazard as a second process.
        let dir = std::env::temp_dir().join(format!("hiqlite-abort-probe-{}", std::process::id()));
        let dir = dir.to_string_lossy().into_owned();
        let _ = std::fs::remove_dir_all(&dir);
        match hiqlite::probe::take_storage_ownership(&dir) {
            Ok(guard) => {
                match hiqlite::probe::take_storage_ownership(&dir) {
                    Ok(_) => {
                        println!("FAIL: a second owner took storage that was already owned");
                        failures += 1;
                    }
                    Err(err) => println!("ok: contended storage ownership -> {err}"),
                }
                drop(guard);
            }
            Err(err) => {
                println!("FAIL: the first owner could not take the storage: {err}");
                failures += 1;
            }
        }
        let _ = std::fs::remove_dir_all(&dir);
    });

    if failures == 0 {
        println!("probe: all expected failures were returned as errors, none panicked");
        std::process::exit(0);
    }
    println!("probe: {failures} expected failure(s) were not reported as errors");
    std::process::exit(1);
}
