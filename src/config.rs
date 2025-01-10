use std::{
    env,
    path::{Path, PathBuf},
};

use better_default::Default;

use color_eyre::eyre::{self, bail, ContextCompat};
use fs_err::PathExt;
use serde::{de::DeserializeOwned, Deserialize, Serialize};
use tracing::debug;

use crate::{defines::APP_CONFIG_PATH, workshop::Tag};

/// To be able to easily store config to the path, from which the config was initially read from.
#[derive(Debug, Clone, Default)]
pub struct ConfigWithPath<T: Config> {
    #[default(PathBuf::from("config.toml"))]
    config_path: PathBuf,
    pub inner: T,
}

#[allow(unused)]
impl<T: Config> ConfigWithPath<T> {
    pub fn default_with_path(path: impl AsRef<Path>) -> Self {
        Self {
            config_path: path.as_ref().to_path_buf(),
            inner: T::default(),
        }
    }
    #[tracing::instrument(skip(paths))]
    pub fn try_load_in_order(paths: &[PathBuf]) -> eyre::Result<Self> {
        for path in paths {
            if let Ok(cfg) = Self::try_load_path(path) {
                debug!(?path, "Loaded config");
                return Ok(cfg);
            }
        }

        bail!("Failed to find config at any set path");
    }
    /// Fails if config is bad, instead of returning its Default
    pub fn try_load_path(path: impl AsRef<Path>) -> eyre::Result<Self> {
        Ok(Self {
            config_path: path.as_ref().to_path_buf(),
            inner: T::try_load_path(path)?,
        })
    }
    pub fn load_path(path: impl AsRef<Path>) -> eyre::Result<Self> {
        Ok(Self {
            config_path: path.as_ref().to_path_buf(),
            inner: T::load_path(path)?,
        })
    }
    pub fn store(&self) -> eyre::Result<()> {
        self.inner.store_path(&self.config_path)
    }
}

impl ConfigWithPath<AppConfig> {
    pub fn load() -> eyre::Result<Self> {
        Self::try_load_in_order(&[
            // Local config
            env::current_exe()?
                .parent()
                .context("Failed to get path to executable's directory")?
                .join("config.toml"),
            // Config stored in standard config dir
            APP_CONFIG_PATH.to_path_buf(),
        ])
        .map_or_else(
            |_| {
                let cfg = Self::default_with_path(APP_CONFIG_PATH.as_path());
                cfg.store()?; // Write config to default path
                Ok(cfg)
            },
            |it| Ok(it),
        )
    }
}

#[derive(Debug, Default, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct AppConfig {
    #[default(true)]
    pub open_item_page_on_complete: bool,
}

impl Config for AppConfig {}
impl ConfigWithPathExt for AppConfig {
    fn config_path() -> impl AsRef<Path> {
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
pub trait ConfigWithPathExt: Config
where
    Self: Serialize + DeserializeOwned + Default,
{
    fn config_path() -> impl AsRef<Path>;

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
