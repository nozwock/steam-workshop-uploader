use std::path::Path;

use better_default::Default;

use color_eyre::eyre::{self, bail};
use fs_err::PathExt;
use serde::{de::DeserializeOwned, Deserialize, Serialize};

use crate::{defines::APP_CONFIG_PATH, workshop::Tag};

#[derive(Debug, Default, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct AppConfig {
    #[default(true)]
    pub open_item_page_on_complete: bool,
}

impl Config for AppConfig {}
impl ConfigWithPath for AppConfig {
    fn config_path() -> impl AsRef<Path> + 'static {
        APP_CONFIG_PATH.as_path()
    }
}

/// It should be stored in the workshop item content directory.
#[derive(Debug, Default, Clone, Serialize, Deserialize)]
pub struct WorkshopItemConfig {
    pub app_id: u32,
    pub item_id: u64,
    /// Tags need to be stored in the metadata file, as Steam doesn’t retain them ifno tags are provided to Steamworks
    /// during an item update.
    pub tags: Vec<Tag>,
}

impl Config for WorkshopItemConfig {}

#[allow(unused)]
pub trait Config
where
    Self: Serialize + DeserializeOwned + Default,
{
    fn try_load_path(path: impl AsRef<Path>) -> eyre::Result<Self> {
        if !path.as_ref().fs_err_canonicalize()?.is_file() {
            bail!("{:?} is not a file.", path.as_ref());
        }
        Ok(confy::load_path::<Self>(path)?)
    }
    fn load_path(path: impl AsRef<Path>) -> eyre::Result<Self> {
        Ok(confy::load_path::<Self>(path)?)
    }
    fn store_path(&self, path: impl AsRef<Path>) -> eyre::Result<()> {
        Ok(confy::store_path(path, &self)?)
    }
}

#[allow(unused)]
pub trait ConfigWithPath: Config
where
    Self: Serialize + DeserializeOwned + Default,
{
    fn config_path() -> impl AsRef<Path> + 'static;

    fn try_load() -> eyre::Result<Self> {
        Self::try_load_path(Self::config_path())
    }
    fn load() -> eyre::Result<Self> {
        Self::load_path(Self::config_path())
    }
    fn store(&self) -> eyre::Result<()> {
        self.store_path(Self::config_path())
    }
}
