use std::{
    path::Path,
    time::{SystemTime, SystemTimeError, UNIX_EPOCH},
};

use color_eyre::{eyre, owo_colors::OwoColorize};
use tracing::info;

use crate::{Plugin, PluginContext, register_plugin};

const META_CPP_FILENAME: &str = "meta.cpp";

struct DayZPlugin;

impl Plugin for DayZPlugin {
    fn app_id(&self) -> u32 {
        221100
    }

    fn name(&self) -> &'static str {
        "DayZ"
    }

    fn pre_update(&self, ctx: &PluginContext, item_id: u64) -> eyre::Result<()> {
        eprintln!("{}", "[-] Updating meta.cpp...".cyan());
        write_dayz_meta_cpp(ctx.content_path, item_id)
    }
}

register_plugin!(DayZPlugin);

/// DayZ mods carry a `meta.cpp` in their root; the game uses `publishedid` in it to identify the workshop item a mod
/// came from, so it must match the actual item id.
///
/// Creates the file in `content_path` if missing, otherwise only replaces the `publishedid` and `timestamp` values
/// in-place, leaving other fields untouched.
pub fn write_dayz_meta_cpp(content_path: impl AsRef<Path>, item_id: u64) -> eyre::Result<()> {
    let content_path = content_path.as_ref();
    // `DateTime.ToBinary()` is what DayZ's own publishing tool seems to use.
    let timestamp = dotnet_to_binary_utc(SystemTime::now())?;

    let meta_cpp_path = content_path.join(META_CPP_FILENAME);
    let meta_cpp = if meta_cpp_path.exists() {
        let source = fs_err::read_to_string(&meta_cpp_path)?;
        let source = upsert_meta_cpp_field(&source, "publishedid", &item_id.to_string());
        upsert_meta_cpp_field(&source, "timestamp", &timestamp.to_string())
    } else {
        // Fallback to the content folder's name (minus the `@` prefix)
        let name = content_path
            .file_name()
            .and_then(|it| it.to_str())
            .map(|it| it.trim_start_matches('@'))
            .unwrap_or_default();
        format!(
            "protocol = 1;\npublishedid = {item_id};\nname = \"{name}\";\ntimestamp = {timestamp};\n"
        )
    };
    info!(path = ?meta_cpp_path, item_id, "Writing DayZ meta.cpp");
    fs_err::write(&meta_cpp_path, meta_cpp)?;

    Ok(())
}

fn upsert_meta_cpp_field(source: &str, key: &str, value: &str) -> String {
    let mut found = false;
    let mut lines: Vec<_> = source
        .lines()
        .map(|line| {
            let trimmed = line.trim_start();
            match trimmed.strip_prefix(key) {
                Some(rest) if rest.trim_start().starts_with('=') => {
                    found = true;
                    format!("{}{key} = {value};", &line[..line.len() - trimmed.len()])
                }
                _ => line.to_owned(),
            }
        })
        .collect();
    if !found {
        lines.push(format!("{key} = {value};"));
    }

    let mut out = lines.join("\n");
    out.push('\n');
    out
}

/// Encodes a `SystemTime` in .NET `DateTime.ToBinary()` format with `DateTimeKind.Utc`.
///
/// See [source](https://github.com/dotnet/runtime/blob/4271d88e0aebf3d04f188f1334c2220d80555ef6/src/libraries/System.Private.CoreLib/src/System/DateTime.cs#L1313-L1340).
fn dotnet_to_binary_utc(time: SystemTime) -> Result<u64, SystemTimeError> {
    const TICKS_PER_SECOND: u64 = 10_000_000;
    const DOTNET_KIND_UTC: u64 = 1 << 62;
    const SECONDS_FROM_YEAR_1_TO_UNIX_EPOCH: u64 = {
        const GREGORIAN_EPOCH_YEAR: u64 = 1;
        const UNIX_EPOCH_YEAR: u64 = 1970;

        const YEARS: u64 = UNIX_EPOCH_YEAR - GREGORIAN_EPOCH_YEAR;
        const LEAP_DAYS: u64 = YEARS / 4 - YEARS / 100 + YEARS / 400;
        const TOTAL_DAYS: u64 = YEARS * 365 + LEAP_DAYS;
        const SECONDS_PER_DAY: u64 = 24 * 60 * 60;

        TOTAL_DAYS * SECONDS_PER_DAY
    };

    let unix_secs = time.duration_since(UNIX_EPOCH)?.as_secs();
    let ticks = (unix_secs + SECONDS_FROM_YEAR_1_TO_UNIX_EPOCH) * TICKS_PER_SECOND;
    Ok(ticks | DOTNET_KIND_UTC)
}
