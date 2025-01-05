use std::fmt::Debug;

use clap::{builder::TypedValueParser, Parser, Subcommand, ValueEnum};
use clio::ClioPath;
use steamworks::AppId;

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
    #[arg(long = "content", value_name = "DIR", value_parser = clap::value_parser!(ClioPath).exists().is_dir())]
    pub content_path: Option<ClioPath>,
    #[arg(long)]
    pub visibility: Option<PublishedFileVisibility>,
    #[arg(short, long = "tag")]
    pub tags: Vec<String>,
    #[arg(long, value_name = "FILE", value_parser = clap::value_parser!(ClioPath).exists().is_file())]
    pub preview: Option<ClioPath>,
    #[arg(short, long = "glob", value_name = "GLOB")]
    pub globs: Vec<String>,
    #[arg(long = "ignore-file", value_name = "FILE", value_parser = clap::value_parser!(ClioPath).exists().is_file())]
    pub ignore_files: Vec<ClioPath>,
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
}

#[derive(Debug, Clone, Copy, ValueEnum)]
pub enum PublishedFileVisibility {
    FriendsOnly,
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
