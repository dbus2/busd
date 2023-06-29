use anyhow::{anyhow, Error, Result};
use rand::Rng;
#[cfg(unix)]
use std::{fs::Permissions, os::unix::prelude::PermissionsExt};
use std::{
    io,
    str::FromStr,
    time::{Duration, SystemTime, UNIX_EPOCH},
};
#[cfg(unix)]
use tokio::fs::set_permissions;
use tokio::{
    fs::{create_dir_all, metadata, remove_file, rename, File, OpenOptions},
    io::{AsyncBufReadExt, AsyncWriteExt, BufReader},
    sync::oneshot::{self, Receiver},
    task::JoinHandle,
    time::sleep,
};
use tracing::{debug, info, instrument, trace, warn};
use xdg_home::home_dir;

/// Run the cookie sync task.
///
/// Returns a handle to the task and a receiver that will be signaled when initial sync completes.
#[instrument]
pub(super) fn run_sync() -> (JoinHandle<Error>, Receiver<()>) {
    let (tx, rx) = oneshot::channel();
    (
        tokio::spawn(async move {
            // Initial sync.
            if let Err(e) = sync().await {
                return e;
            }
            if tx.send(()).is_err() {
                return anyhow!("Failed to send cookie sync completion signal.");
            }

            loop {
                // No need to sync unitl another 3 minutes.
                sleep(Duration::from_secs(3 * 60)).await;

                if let Err(e) = sync().await {
                    break e;
                }
            }
        }),
        rx,
    )
}

async fn sync() -> Result<()> {
    let cookie_dir_path = home_dir().unwrap().join(".dbus-keyrings");

    // Ensure the cookie directory exists and has the correct permissions.
    match metadata(&cookie_dir_path).await {
        #[cfg(unix)]
        Ok(metadata) => {
            let mode = metadata.permissions().mode();
            if mode & 0o077 != 0 {
                anyhow::bail!(
                    "Invalid permissions on the cookie directory `{}`.",
                    cookie_dir_path.display(),
                );
            }
        }
        #[cfg(not(unix))]
        Ok(_) => (),
        Err(e) if e.kind() == io::ErrorKind::NotFound => {
            create_dir_all(&cookie_dir_path).await?;
            #[cfg(unix)]
            set_permissions(&cookie_dir_path, Permissions::from_mode(0o700)).await?;
        }
        Err(e) => Err(e)?,
    }

    let cookie_path = cookie_dir_path.join(COOKIE_CONTEXT);
    let lock_file_path = cookie_path.with_extension("lock");
    trace!("Opening lock file `{}`..", lock_file_path.display());
    let mut open_options = OpenOptions::new();
    #[allow(unused_mut)]
    let mut open_options = open_options.write(true).create_new(true);
    #[cfg(unix)]
    {
        open_options = open_options.mode(0o600);
    }
    let mut attempts = 0;
    let lock_file = loop {
        attempts += 1;

        match open_options.open(&lock_file_path).await {
            Ok(f) => break f,
            Err(e) if e.kind() == io::ErrorKind::AlreadyExists => {
                if attempts > 3 {
                    debug!(
                        "Cookies file {} still locked. Attempting to force lock..",
                        cookie_path.display()
                    );
                    // Try to delete the file (likely broker died while editting the cookies file).
                    remove_file(&lock_file_path).await?;
                } else {
                    if attempts == 0 {
                        debug!(
                            "Cookies file {} locked. Waiting for it be unlocked..",
                            cookie_path.display()
                        );
                    }
                    sleep(Duration::from_secs(5)).await;
                }
            }
            Err(e) => Err(e)?,
        }
    };

    trace!("Reading cookies file `{}`..", cookie_path.display());
    let (mut cookies, mut changed, new_cookie_needed) = match open_options
        // Reset options only needed for the lock file.
        .write(false)
        .create_new(false)
        // Set options for cookies file.
        .read(true)
        .open(&cookie_path)
        .await
    {
        Ok(cookies_file) => load_cookies(cookies_file).await?,
        Err(e) if e.kind() == io::ErrorKind::NotFound => (vec![], true, true),
        Err(e) => Err(e)?,
    };

    if new_cookie_needed {
        trace!("Creating a new cookie (🍪)..");
        // No cookies left, let's add one then.
        let mut rng = rand::thread_rng();
        let mut cookie_bytes = [0u8; 32];
        rng.fill(&mut cookie_bytes);
        let cookie = Cookie {
            id: rng.gen(),
            created: SystemTime::now().duration_since(UNIX_EPOCH)?.as_secs(),
            cookie: hex::encode(cookie_bytes),
        };
        trace!("Created cookie with ID `{}`.", cookie.id);
        cookies.insert(0, cookie);
        changed = true;
    }

    if !changed {
        trace!("No changes to write back to the cookies file.");
        trace!("Removing lock file `{}`..", lock_file_path.display());
        drop(lock_file);
        remove_file(&lock_file_path).await?;
        trace!("Removed lock file `{}`..", lock_file_path.display());

        return Ok(());
    }

    // Write back the cookies file but it has to be done in at atomic way so first write to a temp
    // file and then just rename that to cookies file.
    let temp_file_path = cookie_path.with_extension(".temp");
    trace!(
        "Writing temporary cookies file `{}`..",
        temp_file_path.display()
    );
    let mut temp_file = open_options
        // Reset options only needed for reading the cookies file.
        .read(false)
        // Set options for temporary cookies file.
        .write(true)
        .create(true)
        .open(&temp_file_path)
        .await?;
    for cookie in cookies {
        temp_file.write_all(cookie.to_string().as_bytes()).await?;
        temp_file.write_all(b"\n").await?;
    }
    trace!(
        "Closing temporary cookies file `{}`..",
        temp_file_path.display()
    );
    drop(temp_file);
    trace!(
        "Renaming temporary cookies file `{}` to actual file `{}`..",
        temp_file_path.display(),
        cookie_path.display(),
    );
    rename(temp_file_path, cookie_path).await?;

    trace!("Closing cookies lock file `{}`..", lock_file_path.display());
    drop(lock_file);
    trace!("Removing lock file `{}`..", lock_file_path.display());
    remove_file(lock_file_path).await?;
    trace!("Cookies setup");

    Ok(())
}

#[derive(Debug)]
struct Cookie {
    id: usize,
    created: u64,
    cookie: String,
}

impl FromStr for Cookie {
    type Err = anyhow::Error;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let mut split = s.split_whitespace();
        let id = split.next().ok_or_else(|| anyhow!("Missing ID"))?.parse()?;
        let created = split.next().ok_or_else(|| anyhow!("Missing ID"))?.parse()?;
        let cookie = split
            .next()
            .ok_or_else(|| anyhow!("Missing ID"))?
            .to_string();

        Ok(Self {
            id,
            created,
            cookie,
        })
    }
}

impl ToString for Cookie {
    fn to_string(&self) -> String {
        format!("{} {} {}", self.id, self.created, self.cookie)
    }
}

// Just use the default cookie context.
const COOKIE_CONTEXT: &str = "org_freedesktop_general";

/// Loads the cookies from the given file.
///
/// Returns a tuple of the cookies and a boolean indicating if any cookies were filtered out.
#[instrument]
async fn load_cookies(cookies_file: File) -> Result<(Vec<Cookie>, bool, bool)> {
    trace!("Loading cookies..");
    let mut cookies = vec![];
    let mut lines = BufReader::new(cookies_file).lines();
    let mut filtered = false;
    let mut n_about_to_expire = 0;
    while let Some(line) = lines.next_line().await? {
        let now = SystemTime::now().duration_since(UNIX_EPOCH)?.as_secs();
        let cookie = match Cookie::from_str(&line) {
            Err(e) => {
                warn!("Failed to parse cookie: {e}");
                filtered = true;

                continue;
            }
            // Spec recommends deleting cookies that are either:
            //
            // * 7 mins in the past, or
            // * 5 mins into the future.
            Ok(cookie) if cookie.created < now - 7 * 60 => {
                info!("Deleting cookie with ID {} as it is too old.", cookie.id);
                filtered = true;

                continue;
            }
            Ok(cookie) if cookie.created > now + 5 * 60 => {
                info!("Deleting cookie with ID {} as it is too new.", cookie.id);
                filtered = true;

                continue;
            }
            Ok(cookie) => {
                // The cookie is about to expire.
                if cookie.created < now - 4 * 60 {
                    n_about_to_expire += 1;
                }
                cookie
            }
        };
        cookies.push(cookie);
    }
    trace!("Loaded {} cookies.", cookies.len());
    let new_cookie_needed = if n_about_to_expire == cookies.len() {
        trace!("All cookies are about to expire.");
        true
    } else if cookies.is_empty() {
        trace!("Out of cookies. ∅🍪");
        true
    } else {
        false
    };

    Ok((cookies, filtered, new_cookie_needed))
}
