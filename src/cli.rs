use std::{fmt::Debug, path::PathBuf};

use clap::{Parser, Subcommand, ValueEnum, builder::TypedValueParser};
use clio::ClioPath;

use crate::workshop::{AppId, Language, Tag};

static IGNORE_HELP: &'static str = r#"By default, files and directories matching ignore patterns from files like `.ignore` and `.gitignore` are excluded."#;

#[derive(Debug, Clone, Parser)]
#[command(author, version, arg_required_else_help = true, about = IGNORE_HELP)]
pub struct Cli {
    #[arg(short = 'q', long)]
    pub no_prompt: bool,
    #[command(subcommand)]
    pub command: Command,
}

#[derive(Debug, Clone, Subcommand)]
pub enum Command {
    Create(CreateCommand),
    Update(UpdateCommand),
    Init(InitCommand),
}

#[derive(Debug, Clone, clap::Args)]
pub struct WorkshopItemArgs {
    #[arg(long)]
    pub title: Option<String>,
    #[arg(long, conflicts_with = "description_file")]
    pub description: Option<String>,
    /// Automatically converted from Markdown to Steam markup if extension is .md or .markdown.
    #[arg(
        long = "description-file",
        value_name = "FILE",
        value_parser = clap::value_parser!(ClioPath)
            .exists()
            .is_file()
            .map(|it| it.to_path_buf()),
        conflicts_with = "description"
    )]
    pub description_file: Option<PathBuf>,
    #[arg(
        long = "content",
        value_name = "DIR",
        value_parser = clap::value_parser!(ClioPath)
        .exists()
        .is_dir()
        .map(|it| it.to_path_buf())
    )]
    pub content_path: Option<PathBuf>,
    #[arg(long)]
    pub visibility: Option<PublishedFileVisibility>,
    #[arg(short, long = "tag", value_parser = |s: &str| Tag::new(s.to_owned()))]
    pub tags: Vec<Tag>,
    /// Language code for title and description.
    #[arg(short = 'l', long, value_name = "LANG")]
    pub language: Option<Language>,
    /// Suggested formats include JPG, PNG and GIF (max 1 MiB).
    #[arg(
        long = "preview",
        value_name = "FILE",
        value_parser = clap::value_parser!(ClioPath)
        .exists()
        .is_file()
        .map(|it| it.to_path_buf())
    )]
    pub preview_path: Option<PathBuf>,
    #[arg(short = 'm', long, conflicts_with = "change_log_file")]
    pub change_log: Option<String>,
    /// Automatically converted from Markdown to Steam markup if extension is .md or .markdown.
    #[arg(
        long = "change-log-file",
        value_name = "FILE",
        value_parser = clap::value_parser!(ClioPath)
            .exists()
            .is_file()
            .map(|it| it.to_path_buf()),
        conflicts_with = "change_log"
    )]
    pub change_log_file: Option<PathBuf>,
    /// Treat description and changelog inputs as Markdown to be converted to Steam markup.
    #[arg(long)]
    pub markdown: bool,
    #[arg(short, long = "glob", value_name = "GLOB")]
    pub globs: Vec<String>,
    #[arg(
        long = "ignore-file",
        value_name = "FILE",
        value_parser = clap::value_parser!(ClioPath)
            .exists()
            .is_file()
            .map(|it| it.to_path_buf())
    )]
    pub ignore_files: Vec<PathBuf>,
}

/// Publish a new workshop item.
#[derive(Debug, Clone, Parser)]
#[command()]
pub struct CreateCommand {
    /// Steam AppId
    #[arg(long, value_parser = clap::value_parser!(u32).map(|it| AppId(it)))]
    // Getting to .map was painful, I was going around trying to impl TypedValueParser and whatnot
    pub app_id: Option<AppId>,
    #[command(flatten)]
    pub workshop_item: WorkshopItemArgs,
}

/// Update an existing workshop item.
#[derive(Debug, Clone, Parser)]
#[command()]
pub struct UpdateCommand {
    #[command(flatten)]
    pub workshop_item: WorkshopItemArgs,
    /// Skip updating the workshop item files; only use the content path to access the `workshop.toml` metadata file.
    #[arg(long = "no-content-update")]
    pub no_content_update: bool,
}

/// Create a `workshop.toml` project file without creating a new workshop item.
#[derive(Debug, Clone, Parser)]
#[command()]
pub struct InitCommand {
    /// Steam AppId
    #[arg(long, value_parser = clap::value_parser!(u32).map(|it| AppId(it)))]
    pub app_id: Option<AppId>,
    /// Steam Workshop Item ID
    #[arg(long)]
    pub item_id: Option<u64>,
    #[arg(
        long = "content",
        value_name = "DIR",
        value_parser = clap::value_parser!(ClioPath)
            .exists()
            .is_dir()
            .map(|it| it.to_path_buf())
    )]
    pub content_path: Option<PathBuf>,
    #[arg(short, long = "tag", value_parser = |s: &str| Tag::new(s.to_owned()))]
    pub tags: Vec<Tag>,
    #[arg(short, long)]
    pub force: bool,
}

#[derive(Debug, Clone, Copy, ValueEnum, Default, strum::Display)]
#[strum(serialize_all = "PascalCase")]
pub enum PublishedFileVisibility {
    FriendsOnly,
    #[default]
    Private,
    Public,
    Unlisted,
}

// pain
impl Into<PublishedFileVisibility> for steamworks::PublishedFileVisibility {
    fn into(self) -> PublishedFileVisibility {
        match self {
            Self::FriendsOnly => PublishedFileVisibility::FriendsOnly,
            Self::Private => PublishedFileVisibility::Private,
            Self::Public => PublishedFileVisibility::Public,
            Self::Unlisted => PublishedFileVisibility::Unlisted,
        }
    }
}

impl From<PublishedFileVisibility> for steamworks::PublishedFileVisibility {
    fn from(value: PublishedFileVisibility) -> Self {
        match value {
            PublishedFileVisibility::FriendsOnly => Self::FriendsOnly,
            PublishedFileVisibility::Private => Self::Private,
            PublishedFileVisibility::Public => Self::Public,
            PublishedFileVisibility::Unlisted => Self::Unlisted,
        }
    }
}
