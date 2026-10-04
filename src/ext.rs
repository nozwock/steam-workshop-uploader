use std::sync::mpsc;

use color_eyre::eyre::{self, bail};
use tracing::error;

pub type SteamworksClient = steamworks::Client;

#[macro_export]
macro_rules! run_callbacks_blocking {
    ($client:ident, $rx:ident) => {{
        use ::std::sync::mpsc;
        let out;
        loop {
            match $rx.try_recv() {
                Err(mpsc::TryRecvError::Empty) => {
                    $client.run_callbacks();
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
        client: &SteamworksClient,
        app_id: steamworks::AppId,
        file_type: steamworks::FileType,
    ) -> eyre::Result<(steamworks::PublishedFileId, bool)>;
}

impl UGCBlockingExt for steamworks::UGC {
    fn create_item_blocking(
        &self,
        client: &SteamworksClient,
        app_id: steamworks::AppId,
        file_type: steamworks::FileType,
    ) -> eyre::Result<(steamworks::PublishedFileId, bool)> {
        let (tx, rx) = mpsc::channel();

        self.create_item(app_id.into(), file_type, move |result| {
            _ = tx.send(result).inspect_err(|e| error!(%e));
        });

        // We love single.run_callbacks()!
        // Best API in the world
        Ok(run_callbacks_blocking!(client, rx)?)
    }
}

pub trait UpdateHandleBlockingExt {
    fn submit_blocking(
        self,
        client: &SteamworksClient,
        change_note: Option<&str>,
    ) -> eyre::Result<(steamworks::PublishedFileId, bool)>;
}

impl UpdateHandleBlockingExt for steamworks::UpdateHandle {
    fn submit_blocking(
        self,
        client: &SteamworksClient,
        change_note: Option<&str>,
    ) -> eyre::Result<(steamworks::PublishedFileId, bool)> {
        let (tx, rx) = mpsc::channel();

        self.submit(change_note, move |result| {
            _ = tx.send(result).inspect_err(|e| error!(%e));
        });

        Ok(run_callbacks_blocking!(client, rx)?)
    }
}
