use std::{
    borrow::Cow,
    fmt,
    path::Path,
    sync::{Arc, Mutex, Weak, mpsc},
    time::Duration,
};

use color_eyre::eyre::{self, ContextCompat, WrapErr, bail};
use fs_err::PathExt;
use itertools::Itertools;
use relative_path::PathExt as RelPathExt;
use serde::{Deserialize, Serialize};
use serde_with::{DisplayFromStr, serde_as};
use tracing::{debug, error, info, warn};

use crate::{
    config::{Config, WorkshopItemConfig},
    defines::{LOCALE_ENV_VARS, WORKSHOP_METADATA_FILENAME},
};

#[serde_as]
#[derive(Debug, Copy, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct AppId(#[serde_as(as = "DisplayFromStr")] pub u32);
impl From<u32> for AppId {
    fn from(id: u32) -> Self {
        AppId(id)
    }
}

impl Into<steamworks::AppId> for AppId {
    fn into(self) -> steamworks::AppId {
        self.0.into()
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(transparent)]
pub struct Tag(Cow<'static, str>);

impl fmt::Display for Tag {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl Tag {
    pub fn new(s: impl Into<Cow<'static, str>>) -> eyre::Result<Self> {
        let s = s.into();
        Self::is_valid_tag(&s)?;

        Ok(Self(s))
    }
    /// https://partner.steamgames.com/doc/api/ISteamUGC#SetItemTags
    fn is_valid_tag(s: impl AsRef<str>) -> eyre::Result<()> {
        if s.as_ref().is_empty() {
            bail!("Empty tags are not allowed")
        }

        if !s.as_ref().len() < 256 {
            bail!("Tag can only have a max length of 255 characters")
        }

        if s.as_ref()
            .chars()
            .any(|c| !(c != ',' && (c.is_ascii_graphic() || c.is_ascii_whitespace())))
        {
            bail!("Tag contains invalid characters")
        };

        Ok(())
    }
    pub fn is_in_predefined_tags(&self, tags: &[Tag]) -> bool {
        tags.iter().any(|it| it.0 == self.0)
    }
}

pub fn check_tags_are_predefined(tags: &[Tag], predefined: &[Tag]) -> eyre::Result<()> {
    tags.iter()
        .map(|it| {
            it.is_in_predefined_tags(predefined)
                .then_some(())
                .with_context(|| {
                    eyre::eyre!(
                        "`{}` is not a predefined tag. Available tags are: {}",
                        it,
                        predefined.iter().join(", ")
                    )
                })
        })
        .find(|it| it.is_err())
        .unwrap_or(Ok(()))
}

impl AsRef<str> for Tag {
    fn as_ref(&self) -> &str {
        &self.0
    }
}

pub fn is_valid_preview_type(path: impl AsRef<Path>) -> eyre::Result<()> {
    match infer::get_from_path(path)?
        .context("Unknown file type")?
        .mime_type()
    {
        "image/jpeg" | "image/gif" | "image/png" => Ok(()),
        mime_type => {
            bail!(
                "Invalid preview filetype `{}`: Only png, jpeg, and gif are allowed",
                mime_type
            );
        }
    }
}

/// https://partner.steamgames.com/doc/api/ISteamUGC#SetItemTitle
pub fn is_valid_title(s: impl AsRef<str>) -> eyre::Result<()> {
    let s = s.as_ref();
    if s.is_empty() {
        bail!("Empty title is not allowed")
    }

    let max = steamworks_sys::k_cchPublishedDocumentTitleMax as usize;
    if s.len() > max {
        bail!("Title can only have a max length of {max} characters")
    }

    Ok(())
}

/// https://partner.steamgames.com/doc/api/ISteamUGC#SetItemDescription
pub fn is_valid_description(s: impl AsRef<str>) -> eyre::Result<()> {
    let s = s.as_ref();

    let max = steamworks_sys::k_cchPublishedDocumentDescriptionMax as usize;
    if s.len() > max {
        bail!("Description can only have a max length of {max} characters")
    }

    Ok(())
}

static ACTIVE_SESSION: Mutex<Option<Weak<SteamworksClientInner>>> = Mutex::new(None);

struct SteamworksClientInner {
    app_id: steamworks::AppId,
    client: steamworks::Client,
    shutdown_tx: Mutex<Option<mpsc::Sender<()>>>,
    thread_handle: Mutex<Option<std::thread::JoinHandle<()>>>,
}

impl Drop for SteamworksClientInner {
    fn drop(&mut self) {
        if let Ok(mut tx_guard) = self.shutdown_tx.lock()
            && let Some(tx) = tx_guard.take()
        {
            let _ = tx.send(());
        }

        if let Ok(mut handle_guard) = self.thread_handle.lock()
            && let Some(handle) = handle_guard.take()
            && let Err(e) = handle.join()
        {
            tracing::warn!("Steamworks callback thread panicked on join: {e:?}");
        }

        if let Ok(mut session_guard) = ACTIVE_SESSION.lock() {
            *session_guard = None;
        }
    }
}

#[derive(Clone)]
pub struct SteamworksClient {
    inner: Arc<SteamworksClientInner>,
}

impl SteamworksClient {
    const CALLBACK_INTERVAL: Duration = Duration::from_millis(50);

    /// Initializes a `SteamworksClient` singleton for the given `AppId` and starts a background callback thread.
    ///
    /// If an active client for the same `AppId` already exists, a copy of the existing client is returned directly
    /// without re-initializing Steamworks.
    ///
    /// ## SAFETY
    /// When initializing a fresh client, this function calls [`std::env::set_var`] and [`std::env::remove_var`] to
    /// preserve and restore process locale variables across `SteamAPI_Init` (which forces `LC_ALL=C`).
    ///
    /// Which means fresh initialization should only be called in a single-threaded context. So it's best to initialize
    /// the client before starting any other threads.
    pub fn init(app_id: impl Into<steamworks::AppId>) -> eyre::Result<Self> {
        let app_id = app_id.into();
        let mut session_guard = ACTIVE_SESSION
            .lock()
            .map_err(|e| eyre::eyre!("Failed to acquire steamworks session lock: {e}"))?;

        if let Some(weak) = &*session_guard
            && let Some(active) = weak.upgrade()
        {
            if active.app_id == app_id {
                return Ok(Self { inner: active });
            } else {
                bail!(
                    "Cannot initialize Steamworks for AppId {}: an active session for AppId {} already exists. \
                    Explicitly drop all the active clients before initializing a different AppId.",
                    app_id.0,
                    active.app_id.0
                );
            }
        } else {
            *session_guard = None;
        }

        // `SteamAPI_Init` modifies the process environment (notably forcing LC_ALL=C), which breaks UTF-8 handling in child
        // processes such as inquire::Editor.
        // Record all locale-related variables before initialization to restore them afterwards.
        let prev_locale_vars = LOCALE_ENV_VARS
            .iter()
            .map(|&var| (var, std::env::var_os(var)))
            .collect::<Vec<_>>();

        let res = steamworks::Client::init_app(app_id);

        unsafe {
            for (var, prev_val) in prev_locale_vars {
                match prev_val {
                    Some(val) => std::env::set_var(var, val),
                    None => std::env::remove_var(var),
                }
            }
        }

        let client = res.map_err(|err| {
            eyre::eyre!(
                "{}",
                match err {
                    // Display for this variant gives "Some Other Error" which is not helpful. Have to get the inner
                    // string like this
                    steamworks::SteamAPIInitError::FailedGeneric(err) => err,
                    err => format!("{err}"),
                }
            )
        })?;

        let (shutdown_tx, shutdown_rx) = mpsc::channel();
        let thread_client = client.clone();
        let thread_handle = std::thread::Builder::new()
            .name("steamworks-callbacks".to_string())
            .spawn(move || {
                loop {
                    thread_client.run_callbacks();
                    match shutdown_rx.recv_timeout(Self::CALLBACK_INTERVAL) {
                        Ok(_) | Err(mpsc::RecvTimeoutError::Disconnected) => break,
                        Err(mpsc::RecvTimeoutError::Timeout) => {}
                    }
                }
            })
            .context("Failed to spawn steamworks callback thread")?;

        let inner = Arc::new(SteamworksClientInner {
            app_id,
            client,
            shutdown_tx: Mutex::new(Some(shutdown_tx)),
            thread_handle: Mutex::new(Some(thread_handle)),
        });

        *session_guard = Some(Arc::downgrade(&inner));

        Ok(Self { inner })
    }
}

impl std::ops::Deref for SteamworksClient {
    type Target = steamworks::Client;

    fn deref(&self) -> &Self::Target {
        &self.inner.client
    }
}

impl fmt::Debug for SteamworksClient {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("SteamworksClient")
            .field("app_id", &self.inner.app_id)
            .finish_non_exhaustive()
    }
}

/// Attempts to create the directory in the parent of `content_path` so it resides on the same filesystem, enabling
/// hardlinks.
fn create_staging_dir(content_path: impl AsRef<Path>) -> eyre::Result<tempfile::TempDir> {
    let content_path = content_path
        .as_ref()
        .fs_err_canonicalize()
        .unwrap_or_else(|_| content_path.as_ref().to_path_buf());

    if let Some(parent) = content_path.parent()
        && parent.is_dir()
        && let Ok(dir) = tempfile::Builder::new()
            .prefix(".workshop_staging_")
            .tempdir_in(parent)
    {
        return Ok(dir);
    }

    Ok(tempfile::TempDir::new()?)
}

/// Attempts to hardlink files in `content_dir` to temporary staging directory while ignoring files matched in the glob.
/// Falls back to a standard copy if hardlinking fails (e.g. crossing filesystem boundaries).
pub fn stage_filtered_content(
    content_dir: impl AsRef<Path>,
    globs: Option<&[impl AsRef<str>]>,
    ignore_files: Option<&[impl AsRef<Path>]>,
) -> eyre::Result<tempfile::TempDir> {
    let mut overrides = ignore::overrides::OverrideBuilder::new(content_dir.as_ref());
    overrides.add(&format!("!{}", WORKSHOP_METADATA_FILENAME))?;

    if let Some(globs) = globs {
        for glob in globs {
            overrides.add(glob.as_ref())?;
        }
    }

    let mut walk_builder = ignore::WalkBuilder::new(content_dir.as_ref());
    walk_builder.overrides(overrides.build()?);

    if let Some(ignore_files) = ignore_files {
        for ignore_file in ignore_files {
            walk_builder.add_ignore(ignore_file.as_ref());
        }
    }

    let staged_dir = create_staging_dir(content_dir.as_ref())?;
    for entry in walk_builder
        .build()
        .inspect(|it| {
            _ = it.as_ref().inspect_err(|err| warn!("{err}"));
        })
        .filter_map(|it| it.ok())
        .filter(|it| it.depth() != 0)
    {
        if let Some(file_type) = entry.file_type() {
            let relative_entry_path = entry.path().relative_to(&content_dir.as_ref())?;
            let proxy_path = relative_entry_path.to_path(&staged_dir.as_ref());

            if file_type.is_dir() {
                fs_err::create_dir_all(proxy_path)?;
            } else if file_type.is_file() {
                debug!(file = %relative_entry_path, "Adding to item content");
                let source = entry.path().fs_err_canonicalize()?;
                // Symlinking doesn't seem to work with ISteamUGC SetItemContent
                if let Err(err) = fs_err::hard_link(&source, &proxy_path) {
                    debug!(%err, file = %relative_entry_path, "Hardlink failed, falling back to copy");
                    fs_err::copy(&source, &proxy_path)?;
                }
            }
        }
    }

    Ok(staged_dir)
}

pub fn create_item_with_metadata_file(
    client: &SteamworksClient,
    app_id: impl Into<steamworks::AppId>,
    content_path: impl AsRef<Path>,
    tags: &[Tag],
) -> eyre::Result<(steamworks::PublishedFileId, bool)> {
    let app_id = app_id.into();

    let (tx, rx) = mpsc::channel();
    client
        .ugc()
        .create_item(app_id, steamworks::FileType::Community, move |result| {
            _ = tx.send(result).inspect_err(|e| error!(%e));
        });

    let (file_id, agreement) = rx.recv()??;

    info!(item_id = file_id.0, "Workshop item created");

    _ = WorkshopItemConfig {
        app_id: app_id.0,
        item_id: Some(file_id.0),
        tags: tags.to_owned(),
    }
    .store_path(content_path.as_ref().join(WORKSHOP_METADATA_FILENAME))?;

    Ok((file_id, agreement))
}

pub fn fetch_item_tags(client: &SteamworksClient, item_id: u64) -> eyre::Result<Vec<Tag>> {
    let (tx, rx) = mpsc::channel();
    client
        .ugc()
        .query_item(item_id.into())?
        .fetch(move |result| {
            _ = tx
                .send(result.map(|it| it.iter().find_map(|it| it)).ok().flatten())
                .inspect_err(|e| error!(%e));
        });

    let item_info = rx
        .recv()?
        .with_context(|| format!("Failed to receive query result for item id: {item_id}"))?;

    let tags = item_info
        .tags
        .into_iter()
        .filter_map(|t| Tag::new(t).ok())
        .collect();

    Ok(tags)
}

pub fn open_workshop_page(item_id: u64) -> eyre::Result<()> {
    open::that(format!("steam://url/CommunityFilePage/{}", item_id))?;
    Ok(())
}
