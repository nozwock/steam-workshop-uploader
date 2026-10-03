mod cli;
mod config;
mod defines;
mod ext;
mod workshop;

use std::{path::PathBuf, str::FromStr, sync::mpsc};

use clap::Parser;
use cli::{Cli, PublishedFileVisibility, WorkshopItemArgs};
use color_eyre::{
    eyre::{self, ContextCompat, bail},
    owo_colors::OwoColorize,
};
use config::{AppConfig, Config, ConfigWithPath, WorkshopItemConfig};
use defines::{APP_LOG_DIR, WORKSHOP_METADATA_FILENAME};
use ext::UpdateHandleBlockingExt;
use itertools::Itertools;
use tracing::{error, info};
use tracing_subscriber::{EnvFilter, layer::SubscriberExt, util::SubscriberInitExt};
use tracing_utils::{format::SourceFormatter, writer::RotatingFileWriter};
use workshop::{
    Tag, check_tags_are_predefined, is_valid_description, is_valid_preview_type, is_valid_title,
    open_workshop_page,
};

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
        Ok(PathBuf::from_str(&exit_on_none!(
            inquire::Text::new("Content Path")
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
                .prompt_skippable()?
        ))?)
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

    fn inquire_tags(valid_tags: Option<&[Tag]>) -> eyre::Result<Vec<Tag>> {
        if let Some(valid_tags) = valid_tags {
            Ok(inquire::MultiSelect::new("Tags", valid_tags.to_vec())
                .prompt_skippable()?
                .unwrap_or_default())
        } else {
            Ok(inquire::Text::new("Tags")
                .with_help_message("Values are comma-separated")
                .with_validator(|s: &str| {
                    if s.trim().is_empty() {
                        return Ok(inquire::validator::Validation::Valid);
                    }
                    match s
                        .split(",")
                        .map(|s| (s.trim(), Tag::new(s.trim().to_owned())))
                        .find(|(_, it)| it.is_err())
                    {
                        Some((s, Err(err))) => Ok(inquire::validator::Validation::Invalid(
                            format!("`{s}` {err}").into(),
                        )),
                        _ => Ok(inquire::validator::Validation::Valid),
                    }
                })
                .prompt_skippable()?
                .map(|it| {
                    it.split(",")
                        .map(|it| it.trim())
                        .filter(|it| !it.is_empty())
                        .map(|it| Tag::new(it.to_owned()).expect("Already validated by prompt"))
                        .collect_vec()
                })
                .unwrap_or_default())
        }
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
            is_valid_title(title)?;
            handle = handle.title(title);
        }
        if let Some(description) = &workshop_item.description {
            is_valid_description(description)?;
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
            let content_path = command
                .workshop_item
                .content_path
                .clone()
                .map(Ok)
                .unwrap_or_else(|| {
                    if cli.no_prompt {
                        bail!("Path to Content Folder is required")
                    } else {
                        inquire_content_path()
                    }
                })?;

            let existing_cfg = content_path
                .join(WORKSHOP_METADATA_FILENAME)
                .is_file()
                .then(|| {
                    WorkshopItemConfig::try_load_path(content_path.join(WORKSHOP_METADATA_FILENAME))
                        .ok()
                })
                .flatten();

            if let Some(existing_cfg) = &existing_cfg
                && let Some(item_id) = existing_cfg.item_id
            {
                eprintln!(
                    "Metadata file `{}` already exists in {:?} with item_id {}. Aborting creation of a new item.",
                    WORKSHOP_METADATA_FILENAME, content_path, item_id
                );
                quit::with_code(exitcode::USAGE as u8);
            }

            let app_id = command
                .app_id
                .or_else(|| existing_cfg.as_ref().map(|it| it.app_id.into()))
                .map(Ok)
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

            if command.workshop_item.tags.is_empty()
                && let Some(existing_cfg) = &existing_cfg
            {
                command
                    .workshop_item
                    .tags
                    .extend_from_slice(&existing_cfg.tags);
            }

            // Verify tags passed from cli
            let valid_tags = config.inner.valid_tags.get(&app_id);
            if let Some(valid_tags) = valid_tags {
                check_tags_are_predefined(&command.workshop_item.tags, valid_tags)?;
            }

            if let Some(title) = &command.workshop_item.title {
                is_valid_title(title)?;
            }
            if let Some(description) = &command.workshop_item.description {
                is_valid_description(description)?;
            }

            if !cli.no_prompt {
                if command.workshop_item.title.is_none() {
                    command.workshop_item.title = inquire::Text::new("Title")
                        .with_validator(|s: &str| match is_valid_title(s) {
                            Ok(_) => Ok(inquire::validator::Validation::Valid),
                            Err(err) => Ok(inquire::validator::Validation::Invalid(err.into())),
                        })
                        .prompt_skippable()?;
                }
                if command.workshop_item.description.is_none() {
                    command.workshop_item.description = inquire::Editor::new("Description")
                        .with_validator(|s: &str| match is_valid_description(s) {
                            Ok(_) => Ok(inquire::validator::Validation::Valid),
                            Err(err) => Ok(inquire::validator::Validation::Invalid(err.into())),
                        })
                        .prompt_skippable()?;
                }
                if command.workshop_item.tags.is_empty() {
                    let valid_tags = config.inner.valid_tags.get(&app_id).map(|v| v.as_slice());
                    command.workshop_item.tags = inquire_tags(valid_tags)?;
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
                .or_else(|| {
                    let cwd = std::env::current_dir().ok()?;
                    cwd.join(WORKSHOP_METADATA_FILENAME)
                        .is_file()
                        .then_some(cwd)
                })
                .map(Ok)
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
                WorkshopItemConfig::try_load_path(content_path.join(WORKSHOP_METADATA_FILENAME))?;

            let item_id = workshop_item_cfg.item_id.with_context(|| {
                format!(
                    "Missing `item_id` in `{}`. You must publish the item first using `workshop create`.",
                    WORKSHOP_METADATA_FILENAME
                )
            })?;

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

            if let Some(title) = &command.workshop_item.title {
                is_valid_title(title)?;
            }
            if let Some(description) = &command.workshop_item.description {
                is_valid_description(description)?;
            }

            let (client, single) = workshop::steamworks_client_init(workshop_item_cfg.app_id)?;

            let (tx, rx) = mpsc::channel();
            client
                .ugc()
                .query_item(item_id.into())?
                .include_long_desc(true)
                .fetch(move |result| {
                    _ = tx
                        .send(result.map(|it| it.iter().find_map(|it| it)).ok().flatten())
                        .inspect_err(|e| error!(%e));
                });

            let item_info = run_callbacks_blocking!(single, rx).with_context(|| {
                format!("Failed to receive query result for item id: {}", item_id)
            })?;

            if !cli.no_prompt {
                if command.workshop_item.title.is_none() {
                    command.workshop_item.title = inquire::Text::new("Title")
                        .with_initial_value(&item_info.title)
                        .with_validator(|s: &str| match is_valid_title(s) {
                            Ok(_) => Ok(inquire::validator::Validation::Valid),
                            Err(err) => Ok(inquire::validator::Validation::Invalid(err.into())),
                        })
                        .prompt_skippable()?;
                }
                if command.workshop_item.description.is_none() {
                    command.workshop_item.description = inquire::Editor::new("Description")
                        .with_predefined_text(&item_info.description)
                        .with_validator(|s: &str| match is_valid_description(s) {
                            Ok(_) => Ok(inquire::validator::Validation::Valid),
                            Err(err) => Ok(inquire::validator::Validation::Invalid(err.into())),
                        })
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

            let mut handle = client
                .ugc()
                .start_item_update(workshop_item_cfg.app_id.into(), item_id.into());

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
        cli::Command::Init(command) => {
            let content_path = command.content_path.map(Ok).unwrap_or_else(|| {
                if cli.no_prompt {
                    bail!("Path to Content Folder is required")
                } else {
                    inquire_content_path()
                }
            })?;

            let metadata_path = content_path.join(WORKSHOP_METADATA_FILENAME);
            if metadata_path.is_file() && !command.force {
                if cli.no_prompt {
                    bail!(
                        "Metadata file `{}` already exists in {:?}",
                        WORKSHOP_METADATA_FILENAME,
                        content_path
                    );
                } else {
                    let overwrite = inquire::Confirm::new(&format!(
                        "Metadata file `{}` already exists in {:?}. Overwrite?",
                        WORKSHOP_METADATA_FILENAME, content_path
                    ))
                    .with_default(false)
                    .prompt_skippable()?
                    .unwrap_or(false);

                    if !overwrite {
                        eprintln!("{}", "Aborted.".yellow());
                        return Ok(());
                    }
                }
            }

            let app_id = command
                .app_id
                .map(Ok)
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

            let item_id = if let Some(item_id) = command.item_id {
                Some(item_id)
            } else if cli.no_prompt {
                None
            } else {
                inquire::Text::new("ItemId")
                    .with_help_message(
                        "Leave empty if you have not published the item to Steam Workshop yet",
                    )
                    .with_validator(|s: &str| {
                        if s.trim().is_empty() {
                            Ok(inquire::validator::Validation::Valid)
                        } else {
                            match s.trim().parse::<u64>() {
                                Ok(_) => Ok(inquire::validator::Validation::Valid),
                                Err(err) => Ok(inquire::validator::Validation::Invalid(
                                    err.to_string().into(),
                                )),
                            }
                        }
                    })
                    .prompt_skippable()?
                    .and_then(|s| {
                        let s = s.trim();
                        if s.is_empty() {
                            None
                        } else {
                            s.parse::<u64>().ok()
                        }
                    })
            };

            let mut tags = command.tags;

            if tags.is_empty()
                && let Some(item_id) = item_id
                && let Ok((client, single)) = workshop::steamworks_client_init(app_id)
                && let Ok(Some(fetched_tags)) = workshop::fetch_item_tags(&client, &single, item_id)
                && !fetched_tags.is_empty()
            {
                eprintln!(
                    "{} {}",
                    "[+] Retrieved tags from Steam:".green(),
                    fetched_tags.iter().join(", ")
                );
                tags = fetched_tags;
            }

            if !cli.no_prompt && tags.is_empty() {
                let valid_tags = config.inner.valid_tags.get(&app_id).map(|v| v.as_slice());
                tags = inquire_tags(valid_tags)?;
            }

            if let Some(valid_tags) = config.inner.valid_tags.get(&app_id) {
                check_tags_are_predefined(&tags, valid_tags)?;
            }

            WorkshopItemConfig {
                app_id: app_id.0,
                item_id,
                tags,
            }
            .store_path(&metadata_path)?;

            eprintln!(
                "{} `{}`",
                "[+] Created workshop project file at".green(),
                metadata_path.display()
            );
        }
    }

    Ok(())
}
