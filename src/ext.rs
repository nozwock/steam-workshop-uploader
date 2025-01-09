use std::sync::mpsc;

use color_eyre::eyre::{self, bail};
use tracing::error;

pub type SteamworksClient = steamworks::Client<steamworks::ClientManager>;
pub type SteamworksSingleClient = steamworks::SingleClient<steamworks::ClientManager>;

#[macro_export]
macro_rules! run_callbacks_blocking {
    ($single:ident, $rx:ident) => {{
        use ::std::sync::mpsc;
        let out;
        loop {
            match $rx.try_recv() {
                Err(mpsc::TryRecvError::Empty) => {
                    $single.run_callbacks();
                    ::std::thread::sleep(::std::time::Duration::from_millis(100));
                }
                Err(err @ mpsc::TryRecvError::Disconnected) => {
                    bail!(err);
                }
                Ok(result) => {
                    out = result;
                    break;
                }
            }
        }

        out
    }};
}

pub trait UGCBlockingExt {
    fn create_item_blocking(
        &self,
        single: &SteamworksSingleClient,
        app_id: steamworks::AppId,
        file_type: steamworks::FileType,
    ) -> eyre::Result<(steamworks::PublishedFileId, bool)>;
}

impl<Manager> UGCBlockingExt for steamworks::UGC<Manager> {
    fn create_item_blocking(
        &self,
        single: &SteamworksSingleClient,
        app_id: steamworks::AppId,
        file_type: steamworks::FileType,
    ) -> eyre::Result<(steamworks::PublishedFileId, bool)> {
        let (tx, rx) = mpsc::channel();

        self.create_item(app_id.into(), file_type, move |result| {
            _ = tx.send(result).inspect_err(|e| error!(%e));
        });

        // We love single.run_callbacks()!
        // Best API in the world
        Ok(run_callbacks_blocking!(single, rx)?)
    }
}

pub trait UpdateHandleBlockingExt {
    fn submit_blocking(
        self,
        single: &SteamworksSingleClient,
        change_note: Option<&str>,
    ) -> eyre::Result<(steamworks::PublishedFileId, bool)>;
}

impl<Manager> UpdateHandleBlockingExt for steamworks::UpdateHandle<Manager> {
    fn submit_blocking(
        self,
        single: &SteamworksSingleClient,
        change_note: Option<&str>,
    ) -> eyre::Result<(steamworks::PublishedFileId, bool)> {
        let (tx, rx) = mpsc::channel();

        self.submit(change_note, move |result| {
            _ = tx.send(result).inspect_err(|e| error!(%e));
        });

        Ok(run_callbacks_blocking!(single, rx)?)
    }
}
