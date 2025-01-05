use std::path::{Path, PathBuf};

use better_default::Default;

use color_eyre::eyre::{self, bail};
use fs_err::PathExt;
use serde::{de::DeserializeOwned, Deserialize, Serialize};

/// It should be stored in the workshop item content directory.
#[derive(Debug, Default, Clone, Serialize, Deserialize)]
pub struct WorkshopItemConfig {
    pub app_id: u32,
    pub item_id: u64,
}

impl Config for WorkshopItemConfig {}

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

pub trait ConfigExt: Config
where
    Self: Serialize + DeserializeOwned + Default,
{
    fn config_path() -> PathBuf;

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
