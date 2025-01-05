use std::{path::PathBuf, sync::LazyLock};

pub const APP_ID: &str = "io.github.nozwock.steam-workshop-uploader";

pub static APP_CONFIG_DIR: LazyLock<PathBuf> = LazyLock::new(|| {
    directories::BaseDirs::new()
        .map(|it| it.config_local_dir().join(APP_ID))
        .unwrap_or_default()
});

pub static APP_CACHE_DIR: LazyLock<PathBuf> = LazyLock::new(|| {
    directories::BaseDirs::new()
        .map(|it| it.cache_dir().join(APP_ID))
        .unwrap_or_default()
});

pub static APP_CONFIG_PATH: LazyLock<PathBuf> =
    LazyLock::new(|| APP_CONFIG_DIR.join("config.toml"));
pub static APP_LOG_DIR: LazyLock<PathBuf> = LazyLock::new(|| APP_CACHE_DIR.join("log"));

pub const WORKSHOP_METADATA_FILENAME: &str = "workshop.toml";
