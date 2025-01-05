use std::sync::mpsc;

use color_eyre::eyre::{self, bail};

pub type SteamworksClient = steamworks::Client<steamworks::ClientManager>;
pub type SteamworksSingleClient = steamworks::SingleClient<steamworks::ClientManager>;

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
        let (create_tx, create_rx) = mpsc::channel();

        self.create_item(app_id.into(), file_type, move |result| {
            _ = create_tx.send(result);
        });

        // We love single.run_callbacks()!
        // Best API in the world
        loop {
            match create_rx.try_recv() {
                Err(mpsc::TryRecvError::Empty) => {
                    single.run_callbacks();
                    ::std::thread::sleep(::std::time::Duration::from_millis(100));
                }
                Err(err @ mpsc::TryRecvError::Disconnected) => {
                    bail!(err);
                }
                Ok(result) => {
                    return Ok(result?);
                }
            }
        }
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
        let (update_tx, update_rx) = mpsc::channel();

        self.submit(change_note, move |result| {
            _ = update_tx.send(result);
        });

        loop {
            match update_rx.try_recv() {
                Err(mpsc::TryRecvError::Empty) => {
                    single.run_callbacks();
                    ::std::thread::sleep(::std::time::Duration::from_millis(100));
                }
                Err(err @ mpsc::TryRecvError::Disconnected) => {
                    bail!(err);
                }
                Ok(result) => {
                    return Ok(result?);
                }
            }
        }
    }
}
