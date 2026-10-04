mod plugins;

pub use inventory;

use color_eyre::eyre;
use std::path::Path;

pub struct PluginContext<'a> {
    pub app_id: u32,
    pub content_path: &'a Path,
}

impl<'a> PluginContext<'a> {
    pub fn new(app_id: u32, content_path: &'a Path) -> Self {
        Self {
            app_id,
            content_path,
        }
    }
}

pub trait Plugin: Send + Sync + 'static {
    /// Steam App ID this plugin handles.
    fn app_id(&self) -> u32;

    /// App name.
    fn name(&self) -> &'static str;

    /// Invoked before the item is registered on Steam.
    fn pre_create(&self, _ctx: &PluginContext) -> eyre::Result<()> {
        Ok(())
    }

    /// Invoked after Steam assigns a PublishedFileId, and before the content update process begins.
    fn post_create(&self, _ctx: &PluginContext, _item_id: u64) -> eyre::Result<()> {
        Ok(())
    }

    /// Invoked immediately before content is staged and updated.
    fn pre_update(&self, _ctx: &PluginContext, _item_id: u64) -> eyre::Result<()> {
        Ok(())
    }

    /// Invoked after the content update has successfully completed.
    fn post_update(&self, _ctx: &PluginContext, _item_id: u64) -> eyre::Result<()> {
        Ok(())
    }
}

inventory::collect!(&'static dyn Plugin);

#[macro_export]
macro_rules! register_plugin {
    ($plugin:expr) => {
        $crate::inventory::submit! {
            &$plugin as &'static dyn $crate::Plugin
        }
    };
}

/// Find a registered plugin that matches the given App ID.
pub fn find_plugin(app_id: u32) -> Option<&'static dyn Plugin> {
    inventory::iter::<&'static dyn Plugin>
        .into_iter()
        .copied()
        .find(|plugin| plugin.app_id() == app_id)
}
