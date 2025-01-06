use std::{fmt::Debug, path::PathBuf};

use clap::{builder::TypedValueParser, Parser, Subcommand, ValueEnum};
use clio::ClioPath;
use steamworks::AppId;

use crate::workshop::Tag;

static IGNORE_HELP: &'static str = r#"By default, files and directories matching ignore patterns from files like `.ignore` and `.gitignore` are excluded."#;

#[derive(Debug, Clone, Parser)]
#[command(author, version, arg_required_else_help = true, about = IGNORE_HELP)]
pub struct Cli {
    #[command(subcommand)]
    pub command: Command,
}

#[derive(Debug, Clone, Subcommand)]
pub enum Command {
    Create(CreateCommand),
    Update(UpdateCommand),
}

#[derive(Debug, Clone, clap::Args)]
pub struct WorkshopItemArgs {
    #[arg(long)]
    pub title: Option<String>,
    #[arg(long)]
    pub description: Option<String>,
    #[arg(
        long = "content",
        value_name = "DIR",
        value_parser = clap::value_parser!(ClioPath)
        .exists()
        .is_dir()
        .map(|it| it.to_path_buf())
    )]
    pub content_path: Option<PathBuf>,
    #[arg(long, required = false, default_value_t)]
    pub visibility: PublishedFileVisibility,
    #[arg(short, long = "tag", value_parser = |s: &str| Tag::new(s.to_owned()))]
    pub tags: Vec<Tag>,
    /// Suggested formats include JPG, PNG and GIF.
    /// Preview images are stored under the user's Cloud, so sufficient free space is required.
    #[arg(
        long = "preview",
        value_name = "FILE",
        value_parser = clap::value_parser!(ClioPath)
        .exists()
        .is_file()
        .map(|it| it.to_path_buf())
    )]
    pub preview_path: Option<PathBuf>,
    #[arg(short = 'm', long)]
    pub change_log: Option<String>,
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

#[derive(Debug, Clone, Copy, ValueEnum, Default, strum::Display)]
#[strum(serialize_all = "lowercase")]
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
