mod cli;
mod config;
mod defines;
mod ext;
mod workshop;

use std::{path::PathBuf, str::FromStr};

use clap::Parser;
use cli::{Cli, WorkshopItemArgs};
use color_eyre::eyre::{self};
use config::{Config, WorkshopItemConfig};
use defines::{APP_LOG_DIR, WORKSHOP_METADATA_FILENAME};
use ext::UpdateHandleBlockingExt;
use itertools::Itertools;
use tracing::info;
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt, EnvFilter};
use tracing_utils::{format::SourceFormatter, writer::RotatingFileWriter};

#[allow(unused)]
macro_rules! exit_on_err {
    ($res:expr) => {{
        match $res {
            Ok(value) => value,
            Err(_) => quit::with_code(1),
        }
    }};
}

#[allow(unused)]
macro_rules! exit_on_none {
    ($res:expr) => {{
        match $res {
            Some(value) => value,
            None => quit::with_code(1),
        }
    }};
}

#[quit::main]
fn main() -> eyre::Result<()> {
    let (non_blocking, _guard) = tracing_appender::non_blocking(RotatingFileWriter::new(
        3,
        APP_LOG_DIR.as_path(),
        "workshop-uploader.log",
    )?);

    tracing_subscriber::registry()
        .with({
            let layer = tracing_subscriber::fmt::layer()
                .event_format(SourceFormatter)
                .with_writer(non_blocking);
            #[cfg(not(debug_assertions))]
            {
                layer
            }
            #[cfg(debug_assertions)]
            {
                layer.with_writer(std::io::stderr)
            }
        })
        .with(
            EnvFilter::builder()
                .from_env_lossy()
                .add_directive(concat!(env!("CARGO_CRATE_NAME"), "=debug").parse()?),
        )
        .init();

    let cli = Cli::parse();

    fn inquire_content_path() -> eyre::Result<PathBuf> {
        Ok(PathBuf::from_str(&exit_on_none!(inquire::Text::new(
            "Content Path"
        )
        .with_validator(|s: &str| {
            match PathBuf::from_str(s) {
                Ok(path) if path.is_dir() => Ok(inquire::validator::Validation::Valid),
                Ok(_) => Ok(inquire::validator::Validation::Invalid(
                    "Is not a directory".into(),
                )),
                Err(err) => Ok(inquire::validator::Validation::Invalid(
                    err.to_string().into(),
                )),
            }
        })
        .prompt_skippable()?))?)
    }

    /// Note: Doesn't set `content_path`
    fn setup_update_handle(
        handle: steamworks::UpdateHandle<steamworks::ClientManager>,
        workshop_item: &WorkshopItemArgs,
    ) -> eyre::Result<steamworks::UpdateHandle<steamworks::ClientManager>> {
        info!(?workshop_item, "Setting up workshop item update");

        let mut handle = handle
            .visibility(workshop_item.visibility.into())
            // todo: validate tags
            // https://partner.steamgames.com/doc/api/ISteamUGC#SetItemTags
            .tags(workshop_item.tags.iter().collect_vec(), false);

        if let Some(title) = &workshop_item.title {
            handle = handle.title(title);
        }
        if let Some(description) = &workshop_item.description {
            handle = handle.description(description);
        }
        // todo: validate file format of the file
        if let Some(preview_path) = &workshop_item.preview {
            handle = handle.preview_path(&preview_path.canonicalize()?);
        }

        Ok(handle)
    }

    // todo: predefined tags for an appid

    match cli.command {
        cli::Command::Create(command) => {
            let app_id =
                command
                    .app_id
                    .clone()
                    .map(|it| Ok(it))
                    .unwrap_or_else(|| -> eyre::Result<_> {
                        Ok(exit_on_none!(
                            inquire::CustomType::<u32>::new("AppId").prompt_skippable()?
                        )
                        .into())
                    })?;
            let content_path = command
                .workshop_item
                .content_path
                .clone()
                .map(|it| Ok(it))
                .unwrap_or_else(|| inquire_content_path())?;

            if content_path.join(WORKSHOP_METADATA_FILENAME).is_file() {
                eprintln!(
                    "Metadata file `{}` already exists in {:?}. Aborting creation of a new item.",
                    WORKSHOP_METADATA_FILENAME, content_path
                );
                quit::with_code(exitcode::USAGE as u8);
            }

            let content_dir_proxy = tempfile::TempDir::new()?;
            workshop::copy_filtered_content(
                &content_path,
                content_dir_proxy.path(),
                Some(command.workshop_item.globs.as_slice()),
                Some(
                    command
                        .workshop_item
                        .ignore_files
                        .iter()
                        .collect_vec()
                        .as_slice(),
                ),
            )?;

            let (client, single) = workshop::steamworks_client_init(app_id)?;
            let (file_id, _) =
                workshop::create_item_with_metadata_file(&client, &single, content_path, app_id)?;

            let handle = client
                .ugc()
                .start_item_update(app_id, file_id)
                .content_path(content_dir_proxy.path());

            setup_update_handle(handle, &command.workshop_item)?.submit_blocking(
                &single,
                command
                    .workshop_item
                    .change_log
                    .as_ref()
                    .map(|it| it.as_str()),
            )?;

            info!(item_id = file_id.0, "Workshop item updated");
        }
        cli::Command::Update(command) => {
            let content_path = command
                .workshop_item
                .content_path
                .clone()
                .map(|it| Ok(it))
                .unwrap_or_else(|| inquire_content_path())?;

            if !content_path.join(WORKSHOP_METADATA_FILENAME).is_file() {
                eprintln!(
                    "Missing metadata file `{}` from {:?}",
                    WORKSHOP_METADATA_FILENAME, content_path
                );
                quit::with_code(exitcode::USAGE as u8);
            }

            let workshop_item =
                WorkshopItemConfig::try_load_path(content_path.join("workshop.toml"))?;

            let (client, single) = workshop::steamworks_client_init(workshop_item.app_id)?;

            let content_dir_proxy = tempfile::TempDir::new()?;
            workshop::copy_filtered_content(
                &content_path,
                content_dir_proxy.path(),
                Some(command.workshop_item.globs.as_slice()),
                Some(
                    command
                        .workshop_item
                        .ignore_files
                        .iter()
                        .collect_vec()
                        .as_slice(),
                ),
            )?;

            let handle = client
                .ugc()
                .start_item_update(workshop_item.app_id.into(), workshop_item.item_id.into())
                .content_path(content_dir_proxy.path()); // Symlinked files don't work unfortunately

            // todo: Change notes option

            let (file_id, _) = setup_update_handle(handle, &command.workshop_item)?
                .submit_blocking(
                    &single,
                    // This is such a horrible API, like `Option<&str>`? Seriously?
                    command
                        .workshop_item
                        .change_log
                        .as_ref()
                        .map(|it| it.as_str()),
                )?;

            info!(item_id = file_id.0, "Workshop item updated");
        }
    }

    Ok(())
}
