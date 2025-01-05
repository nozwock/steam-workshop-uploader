use std::path::{Path, PathBuf};

use better_default::Default;

use color_eyre::eyre::{self, Result};
use serde::{de::DeserializeOwned, Deserialize, Serialize};

/// It should be stored in the workshop item content directory.
#[derive(Debug, Default, Clone, Serialize, Deserialize)]
pub struct WorkshopItemConfig {
    appid: usize,
    itemid: usize,
}

impl Config for WorkshopItemConfig {}

pub trait Config
where
    Self: Serialize + DeserializeOwned + Default,
{
    fn load_path(path: impl AsRef<Path>) -> Result<Self> {
        confy::load_path::<Self>(path).map_err(eyre::Report::msg)
    }
    fn store_path(&self, path: impl AsRef<Path>) -> Result<()> {
        confy::store_path(path, &self).map_err(eyre::Report::msg)
    }
}

pub trait ConfigExt: Config
where
    Self: Serialize + DeserializeOwned + Default,
{
    fn config_path() -> PathBuf;

    fn load() -> Result<Self> {
        Self::load_path(Self::config_path())
    }
    fn store(&self) -> Result<()> {
        self.store_path(Self::config_path())
    }
}
