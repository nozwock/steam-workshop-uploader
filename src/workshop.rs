use std::{borrow::Cow, fmt, path::Path};

use color_eyre::eyre::{self, bail, ContextCompat};
use fs_err::PathExt;
use itertools::Itertools;
use relative_path::PathExt as RelPathExt;
use serde::{Deserialize, Serialize};
use serde_with::{serde_as, DisplayFromStr};
use tracing::{debug, info, warn};

use crate::{
    config::{Config, WorkshopItemConfig},
    defines::WORKSHOP_METADATA_FILENAME,
    ext::{SteamworksClient, SteamworksSingleClient, UGCBlockingExt},
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

pub fn steamworks_client_init(
    app_id: impl Into<steamworks::AppId>,
) -> eyre::Result<(SteamworksClient, SteamworksSingleClient)> {
    Ok(steamworks::Client::init_app(app_id).map_err(|err| {
        eyre::eyre!(
            "{}",
            match err {
                // Display for this variant gives "Some Other Error" which is not helpful. Have to get the inner string like this
                steamworks::SteamAPIInitError::FailedGeneric(err) => err,
                err => format!("{err}"),
            }
        )
    })?)
}

/// Both `from` and `to` are paths to directory.
/// Make a copy of data in `from` in `to` while ignoring files matched in the glob.
pub fn copy_filtered_content<I, O>(
    from: I,
    to: O,
    globs: Option<&[impl AsRef<str>]>,
    ignore_files: Option<&[impl AsRef<Path>]>,
) -> eyre::Result<()>
where
    I: AsRef<Path>,
    O: AsRef<Path>,
{
    let mut overrides = ignore::overrides::OverrideBuilder::new(from.as_ref());
    overrides.add(&format!("!{}", WORKSHOP_METADATA_FILENAME))?;

    if let Some(globs) = globs {
        for glob in globs {
            overrides.add(glob.as_ref())?;
        }
    }

    let mut walk_builder = ignore::WalkBuilder::new(from.as_ref());
    walk_builder.overrides(overrides.build()?);

    if let Some(ignore_files) = ignore_files {
        for ignore_file in ignore_files {
            walk_builder.add_ignore(ignore_file.as_ref());
        }
    }

    for entry in walk_builder
        .build()
        .inspect(|it| {
            _ = it.as_ref().inspect_err(|err| warn!("{err}"));
        })
        .filter_map(|it| it.ok())
        .filter(|it| it.depth() != 0)
    {
        if let Some(file_type) = entry.file_type() {
            let relative_entry_path = entry.path().relative_to(&from.as_ref())?;
            let proxy_path = relative_entry_path.to_path(&to.as_ref());

            if file_type.is_dir() {
                fs_err::create_dir_all(proxy_path)?;
            } else if file_type.is_file() {
                debug!(file = %relative_entry_path, "Adding to item content");
                fs_err::copy(entry.path().fs_err_canonicalize()?, &proxy_path)?;
            }
        }
    }

    Ok(())
}

pub fn create_item_with_metadata_file(
    client: &SteamworksClient,
    single: &SteamworksSingleClient,
    app_id: impl Into<steamworks::AppId>,
    content_path: impl AsRef<Path>,
    tags: &[Tag],
) -> eyre::Result<(steamworks::PublishedFileId, bool)> {
    let app_id = app_id.into();
    let (file_id, agreement) =
        client
            .ugc()
            .create_item_blocking(single, app_id, steamworks::FileType::Community)?;

    info!(item_id = file_id.0, "Workshop item created");

    _ = WorkshopItemConfig {
        app_id: app_id.0,
        item_id: file_id.0,
        tags: tags.to_owned(),
    }
    .store_path(content_path.as_ref().join(WORKSHOP_METADATA_FILENAME))?;

    Ok((file_id, agreement))
}

pub fn open_workshop_page(item_id: u64) -> eyre::Result<()> {
    open::that(format!("steam://url/CommunityFilePage/{}", item_id))?;
    Ok(())
}
