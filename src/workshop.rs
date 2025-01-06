use std::{borrow::Cow, path::Path};

use color_eyre::eyre::{self, bail};
use fs_err::PathExt;
use relative_path::PathExt as RelPathExt;
use serde::{Deserialize, Serialize};
use tracing::{debug, info, warn};

use crate::{
    config::{Config, WorkshopItemConfig},
    defines::WORKSHOP_METADATA_FILENAME,
    ext::{SteamworksClient, SteamworksSingleClient, UGCBlockingExt},
};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(transparent)]
pub struct Tag(Cow<'static, str>);

impl Tag {
    pub fn new(s: impl Into<Cow<'static, str>>) -> eyre::Result<Self> {
        let s = s.into();
        Self::is_valid_tag(&s)?;

        Ok(Self(s))
    }
    /// https://partner.steamgames.com/doc/api/ISteamUGC#SetItemTags
    fn is_valid_tag(s: impl AsRef<str>) -> eyre::Result<()> {
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
}

impl AsRef<str> for Tag {
    fn as_ref(&self) -> &str {
        &self.0
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
    app_id: steamworks::AppId,
    content_path: impl AsRef<Path>,
    tags: &[Tag],
) -> eyre::Result<(steamworks::PublishedFileId, bool)> {
    let (file_id, agreement) = client.ugc().create_item_blocking(
        single,
        app_id.clone(),
        steamworks::FileType::Community,
    )?;

    info!(item_id = file_id.0, "Workshop item created");

    _ = WorkshopItemConfig {
        app_id: app_id.0,
        item_id: file_id.0,
        tags: tags.to_owned(),
    }
    .store_path(content_path.as_ref().join(WORKSHOP_METADATA_FILENAME))?;

    Ok((file_id, agreement))
}
