mod cli;
mod config;
mod defines;
mod ext;
mod workshop;

use std::{path::PathBuf, str::FromStr, sync::mpsc};

use clap::Parser;
use cli::{Cli, PublishedFileVisibility, WorkshopItemArgs};
use color_eyre::{
    eyre::{self, bail, ContextCompat},
    owo_colors::OwoColorize,
};
use config::{AppConfig, Config, ConfigWithPath, WorkshopItemConfig};
use defines::{APP_LOG_DIR, WORKSHOP_METADATA_FILENAME};
use ext::UpdateHandleBlockingExt;
use itertools::Itertools;
use tracing::{error, info};
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt, EnvFilter};
use tracing_utils::{format::SourceFormatter, writer::RotatingFileWriter};
use workshop::{check_tags_are_predefined, is_valid_preview_type, open_workshop_page, Tag};

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
        "workshop.log",
    )?);

    tracing_subscriber::registry()
        .with({
            tracing_subscriber::fmt::layer()
                .event_format(SourceFormatter)
                .with_writer(non_blocking)
        })
        .with(
            EnvFilter::builder()
                .from_env_lossy()
                .add_directive(concat!(env!("CARGO_CRATE_NAME"), "=debug").parse()?),
        )
        .init();

    run().inspect_err(|e| error!("{e}"))?;

    Ok(())
}

fn run() -> eyre::Result<()> {
    let cli = Cli::parse();
    let config = ConfigWithPath::<AppConfig>::load()?;

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

    fn inquire_preview_path() -> eyre::Result<Option<String>> {
        Ok(inquire::Text::new("Preview Image")
            .with_help_message("Suggested formats include JPG, PNG and GIF")
            .with_validator(|s: &str| {
                match PathBuf::from_str(s)
                    .map_err(eyre::Report::msg)
                    .and_then(|it| is_valid_preview_type(it))
                {
                    Ok(_) => Ok(inquire::validator::Validation::Valid),
                    Err(err) => Ok(inquire::validator::Validation::Invalid(err.into())),
                }
            })
            .prompt_skippable()?)
    }

    /// Note: Doesn't set `content_path`
    fn setup_update_handle(
        handle: steamworks::UpdateHandle<steamworks::ClientManager>,
        workshop_item: &WorkshopItemArgs,
    ) -> eyre::Result<steamworks::UpdateHandle<steamworks::ClientManager>> {
        let mut handle = handle
            .visibility(workshop_item.visibility.unwrap_or_default().into())
            .tags(workshop_item.tags.iter().collect_vec(), false);

        if let Some(title) = &workshop_item.title {
            handle = handle.title(title);
        }
        if let Some(description) = &workshop_item.description {
            handle = handle.description(description);
        }
        if let Some(preview_path) = &workshop_item.preview_path {
            is_valid_preview_type(&preview_path)?;
            handle = handle.preview_path(&preview_path.canonicalize()?);
        }

        Ok(handle)
    }

    let visibility_prompt = inquire::Select::new(
        "Visibility",
        [
            PublishedFileVisibility::FriendsOnly,
            PublishedFileVisibility::Private,
            PublishedFileVisibility::Public,
            PublishedFileVisibility::Unlisted,
        ]
        .to_vec(),
    );

    match cli.command {
        cli::Command::Create(mut command) => {
            let app_id =
                command
                    .app_id
                    .clone()
                    .map(|it| Ok(it))
                    .unwrap_or_else(|| -> eyre::Result<_> {
                        if cli.no_prompt {
                            bail!("AppId is required");
                        } else {
                            Ok(exit_on_none!(
                                inquire::CustomType::<u32>::new("AppId").prompt_skippable()?
                            )
                            .into())
                        }
                    })?;

            // Verify tags passed from cli
            let valid_tags = config.inner.valid_tags.get(&app_id);
            if let Some(valid_tags) = valid_tags {
                check_tags_are_predefined(&command.workshop_item.tags, &valid_tags)?;
            }

            let content_path = command
                .workshop_item
                .content_path
                .clone()
                .map(|it| Ok(it))
                .unwrap_or_else(|| {
                    if cli.no_prompt {
                        bail!("Path to Content Folder is required")
                    } else {
                        inquire_content_path()
                    }
                })?;

            if content_path.join(WORKSHOP_METADATA_FILENAME).is_file() {
                eprintln!(
                    "Metadata file `{}` already exists in {:?}. Aborting creation of a new item.",
                    WORKSHOP_METADATA_FILENAME, content_path
                );
                quit::with_code(exitcode::USAGE as u8);
            }

            // todo: validate title and description length

            if !cli.no_prompt {
                if command.workshop_item.title.is_none() {
                    command.workshop_item.title = inquire::Text::new("Title").prompt_skippable()?;
                }
                if command.workshop_item.description.is_none() {
                    command.workshop_item.description =
                        inquire::Editor::new("Description").prompt_skippable()?;
                }
                if command.workshop_item.tags.len() == 0 {
                    command.workshop_item.tags = if let Some(valid_tags) = valid_tags {
                        inquire::MultiSelect::new("Tags", valid_tags.clone())
                            .prompt_skippable()?
                            .unwrap_or_default()
                    } else {
                        inquire::Text::new("Tags")
                            .with_help_message("Values are comma-serparated")
                            .with_validator(|s: &str| {
                                match s
                                    .split(",")
                                    .map(|s| (s, Tag::new(s.to_owned())))
                                    .find(|(_, it)| it.is_err() || s.is_empty())
                                {
                                    Some((s, Err(err))) => {
                                        Ok(inquire::validator::Validation::Invalid(
                                            format!("`{s}` {err}").into(),
                                        ))
                                    }
                                    _ => Ok(inquire::validator::Validation::Valid),
                                }
                            })
                            .prompt_skippable()?
                            .map(|it| {
                                it.split(",")
                                    .map(|it| {
                                        Tag::new(it.to_owned())
                                            .expect("Already validated by prompt")
                                    })
                                    .collect_vec()
                            })
                            .unwrap_or_default()
                    };
                }
                if command.workshop_item.preview_path.is_none() {
                    command.workshop_item.preview_path = inquire_preview_path()?
                        .map(|s| PathBuf::from_str(&s).ok())
                        .flatten();
                }
                if command.workshop_item.visibility.is_none() {
                    command.workshop_item.visibility =
                        visibility_prompt.clone().prompt_skippable()?;
                }
                if command.workshop_item.change_log.is_none() {
                    command.workshop_item.change_log =
                        inquire::Editor::new("Changelog").prompt_skippable()?;
                }
            }

            eprintln!("{}", "[-] Creating workshop item...".cyan());

            let (client, single) = workshop::steamworks_client_init(app_id)?;
            let (file_id, _) = workshop::create_item_with_metadata_file(
                &client,
                &single,
                app_id,
                &content_path,
                &command.workshop_item.tags,
            )?;

            eprintln!(
                "{} {}{}",
                "[+] Created a new workshop item!".green(),
                "id=".italic(),
                file_id.0.italic()
            );
            eprintln!("{}", "[-] Preparing workshop content...".cyan());

            let prepared_content_dir = tempfile::TempDir::new()?;
            workshop::copy_filtered_content(
                &content_path,
                prepared_content_dir.path(),
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

            eprintln!(
                "{}",
                "[+] Made a staging copy of the workshop content folder.".green()
            );

            let handle = client
                .ugc()
                .start_item_update(app_id.into(), file_id)
                .content_path(prepared_content_dir.path());

            eprintln!("{}", "[-] Updating workshop item...".cyan());

            setup_update_handle(handle, &command.workshop_item)?.submit_blocking(
                &single,
                command
                    .workshop_item
                    .change_log
                    .as_ref()
                    .map(|it| it.as_str()),
            )?;

            eprintln!("{}", "[+] Workshop item updated!".green());

            info!(item_id = file_id.0, "Workshop item updated");

            if config.inner.open_item_page_on_complete {
                eprintln!("{}", "[+] Opening workshop page...".green());
                open_workshop_page(file_id.0)?;
            }
        }
        cli::Command::Update(mut command) => {
            let content_path = command
                .workshop_item
                .content_path
                .clone()
                .map(|it| Ok(it))
                .unwrap_or_else(|| {
                    if cli.no_prompt {
                        bail!("Path to Content Folder is required")
                    } else {
                        inquire_content_path()
                    }
                })?;

            if !content_path.join(WORKSHOP_METADATA_FILENAME).is_file() {
                eprintln!(
                    "Missing metadata file `{}` from {:?}",
                    WORKSHOP_METADATA_FILENAME, content_path
                );
                quit::with_code(exitcode::USAGE as u8);
            }

            // todo: item update status? EItemUpdateStatus

            let workshop_item_cfg =
                WorkshopItemConfig::try_load_path(content_path.join("workshop.toml"))?;

            // Using tags from metadata file only if no tag cli args are passed
            let update_tags = command.workshop_item.tags.len() != 0;
            if !update_tags {
                command
                    .workshop_item
                    .tags
                    .extend_from_slice(&workshop_item_cfg.tags);
            }

            let valid_tags = config
                .inner
                .valid_tags
                .get(&workshop_item_cfg.app_id.into());
            if let Some(valid_tags) = valid_tags {
                check_tags_are_predefined(&command.workshop_item.tags, &valid_tags)?;
            }

            let (client, single) = workshop::steamworks_client_init(workshop_item_cfg.app_id)?;

            let (tx, rx) = mpsc::channel();
            client
                .ugc()
                .query_item(workshop_item_cfg.item_id.into())?
                .include_long_desc(true)
                .fetch(move |result| {
                    _ = tx
                        .send(result.map(|it| it.iter().find_map(|it| it)).ok().flatten())
                        .inspect_err(|e| error!(%e));
                });

            let item_info = run_callbacks_blocking!(single, rx).with_context(|| {
                format!(
                    "Failed to receive query result for item id: {}",
                    workshop_item_cfg.item_id
                )
            })?;

            if !cli.no_prompt {
                if command.workshop_item.title.is_none() {
                    command.workshop_item.title = inquire::Text::new("Title")
                        .with_initial_value(&item_info.title)
                        .prompt_skippable()?;
                }
                if command.workshop_item.description.is_none() {
                    command.workshop_item.description = inquire::Editor::new("Description")
                        .with_predefined_text(&item_info.description)
                        .prompt_skippable()?;
                }
                if !command.no_content_update {
                    command.no_content_update =
                        inquire::Confirm::new("Skip updating item content files?")
                            .with_default(false)
                            .with_help_message("For when you'd like to only update preview, etc.")
                            .prompt_skippable()?
                            .unwrap_or_default();
                }
                if !command.no_content_update && command.workshop_item.change_log.is_none() {
                    command.workshop_item.change_log =
                        inquire::Editor::new("Changelog").prompt_skippable()?;
                }
            }

            command.workshop_item.title.get_or_insert(item_info.title);
            command
                .workshop_item
                .description
                .get_or_insert(item_info.description);
            command
                .workshop_item
                .visibility
                .get_or_insert(item_info.visibility.into());

            let mut handle = client.ugc().start_item_update(
                workshop_item_cfg.app_id.into(),
                workshop_item_cfg.item_id.into(),
            );

            eprintln!("{}", "[-] Preparing workshop content...".cyan());

            let prepared_content_dir;
            if !command.no_content_update {
                prepared_content_dir = tempfile::TempDir::new()?;
                workshop::copy_filtered_content(
                    &content_path,
                    prepared_content_dir.path(),
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
                handle = handle.content_path(prepared_content_dir.path()); // Symlinked files don't work unfortunately
                eprintln!(
                    "{}",
                    "[+] Made a staging copy of the workshop content folder.".green()
                );
            } else {
                eprintln!(
                    "{}",
                    "[+] Skipping content files due to user request.".green()
                );
            }

            eprintln!("{}", "[-] Updating workshop item...".cyan());

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

            eprintln!("{}", "[+] Workshop item updated!".green());

            info!(item_id = file_id.0, "Workshop item updated");

            if update_tags {
                if !cli.no_prompt && inquire::Confirm::new(
                    &format!("Do you want to overwrite tags in `{WORKSHOP_METADATA_FILENAME}` with the ones provided?"),
                )
                .with_default(false)
                .prompt_skippable()?
                .unwrap_or_default() {
                    WorkshopItemConfig {
                        tags: command.workshop_item.tags,
                        ..workshop_item_cfg
                    }
                    .store_path(content_path.join(WORKSHOP_METADATA_FILENAME))?;
                }
            }

            if config.inner.open_item_page_on_complete {
                eprintln!("{}", "[+] Opening workshop page...".green());
                open_workshop_page(file_id.0)?;
            }
        }
    }

    Ok(())
}
