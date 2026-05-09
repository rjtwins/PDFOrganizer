use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

const APP_DATA_FOLDER_NAME: &str = "PDFOrganizer";
const DESTINATIONS_FILE_NAME: &str = "destinations.json";

pub(crate) struct DestinationManager {
    pub(crate) destinations: BTreeMap<String, String>,
    pub(crate) pending_destination: Option<PendingDestination>,
    pub(crate) active_move_pdf: Option<PathBuf>,
    pub(crate) error_message: Option<String>,
    storage_path: Option<PathBuf>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct PendingDestination {
    pub(crate) path: String,
    pub(crate) nickname: String,
}

#[derive(Debug, Default, Serialize, Deserialize)]
struct StoredDestinations {
    destinations: BTreeMap<String, String>,
}

impl Default for DestinationManager {
    fn default() -> Self {
        Self::load()
    }
}

impl DestinationManager {
    fn load() -> Self {
        match default_storage_path() {
            Ok(storage_path) => Self::load_from_storage_path(storage_path),
            Err(error) => Self::empty(
                None,
                Some(format!(
                    "Failed to determine destination storage path: {error}"
                )),
            ),
        }
    }

    fn load_from_storage_path(storage_path: PathBuf) -> Self {
        match load_destinations_from_path(&storage_path) {
            Ok(destinations) => Self {
                destinations,
                pending_destination: None,
                active_move_pdf: None,
                error_message: None,
                storage_path: Some(storage_path),
            },
            Err(error) => Self::empty(
                Some(storage_path),
                Some(format!("Failed to load destinations: {error}")),
            ),
        }
    }

    fn empty(storage_path: Option<PathBuf>, error_message: Option<String>) -> Self {
        Self {
            destinations: BTreeMap::new(),
            pending_destination: None,
            active_move_pdf: None,
            error_message,
            storage_path,
        }
    }
}

pub(crate) fn begin_destination_selection(manager: &mut DestinationManager, path: String) {
    manager.pending_destination = Some(PendingDestination {
        path,
        nickname: String::new(),
    });
    manager.active_move_pdf = None;
    manager.error_message = None;
}

pub(crate) fn update_pending_destination_name(manager: &mut DestinationManager, nickname: String) {
    if let Some(pending_destination) = &mut manager.pending_destination {
        pending_destination.nickname = nickname;
        manager.error_message = None;
    }
}

pub(crate) fn save_pending_destination(manager: &mut DestinationManager) -> Result<(), String> {
    let Some(pending_destination) = manager.pending_destination.take() else {
        return Err("No destination is waiting to be saved.".to_string());
    };

    let nickname = pending_destination.nickname.trim().to_string();

    if nickname.is_empty() {
        manager.pending_destination = Some(pending_destination);
        return Err("Destination name is required.".to_string());
    }

    if manager.destinations.contains_key(&nickname) {
        manager.pending_destination = Some(pending_destination);
        return Err("Destination name already exists.".to_string());
    }

    manager
        .destinations
        .insert(nickname.clone(), pending_destination.path.clone());

    if let Err(error) = persist_destinations(manager) {
        manager.destinations.remove(&nickname);
        manager.pending_destination = Some(pending_destination);
        return Err(error);
    }

    manager.error_message = None;

    Ok(())
}

pub(crate) fn cancel_pending_destination(manager: &mut DestinationManager) {
    manager.pending_destination = None;
    manager.error_message = None;
}

pub(crate) fn begin_destination_move(manager: &mut DestinationManager, pdf_path: PathBuf) {
    manager.active_move_pdf = Some(pdf_path);
    manager.error_message = None;
}

pub(crate) fn cancel_destination_move(manager: &mut DestinationManager) {
    manager.active_move_pdf = None;
    manager.error_message = None;
}

pub(crate) fn remove_destination(
    manager: &mut DestinationManager,
    nickname: &str,
) -> Result<bool, String> {
    let Some(removed_path) = manager.destinations.remove(nickname) else {
        return Ok(false);
    };

    if let Err(error) = persist_destinations(manager) {
        manager
            .destinations
            .insert(nickname.to_string(), removed_path);
        return Err(error);
    }

    manager.error_message = None;

    Ok(true)
}

pub(crate) fn move_pdf_to_destination(
    manager: &mut DestinationManager,
    pdf_path: &Path,
    destination_nickname: &str,
) -> Result<PathBuf, String> {
    let destination_folder = manager
        .destinations
        .get(destination_nickname)
        .cloned()
        .ok_or_else(|| "Selected destination does not exist.".to_string())?;

    let moved_path = move_file_to_folder(pdf_path, &destination_folder)?;
    manager.active_move_pdf = None;
    manager.error_message = None;

    Ok(moved_path)
}

fn move_file_to_folder(source_file: &Path, destination_folder: &str) -> Result<PathBuf, String> {
    let file_name = source_file
        .file_name()
        .ok_or_else(|| "File name is missing for the selected PDF.".to_string())?;
    let destination_path = Path::new(destination_folder).join(file_name);

    if destination_path.exists() {
        return Err("A file with that name already exists in the destination.".to_string());
    }

    match fs::rename(source_file, &destination_path) {
        Ok(()) => Ok(destination_path),
        Err(rename_error) => {
            fs::copy(source_file, &destination_path).map_err(|copy_error| {
                format!(
                    "Failed to move file. Rename error: {rename_error}. Copy error: {copy_error}"
                )
            })?;
            fs::remove_file(source_file).map_err(|remove_error| {
                format!("File was copied but the source could not be removed: {remove_error}")
            })?;

            Ok(destination_path)
        }
    }
}

fn default_storage_path() -> Result<PathBuf, String> {
    let local_data_dir =
        dirs::data_local_dir().ok_or_else(|| "local data directory is unavailable".to_string())?;

    Ok(local_data_dir
        .join(APP_DATA_FOLDER_NAME)
        .join(DESTINATIONS_FILE_NAME))
}

fn load_destinations_from_path(storage_path: &Path) -> Result<BTreeMap<String, String>, String> {
    if !storage_path.exists() {
        return Ok(BTreeMap::new());
    }

    let file_bytes = fs::read(storage_path).map_err(|error| {
        format!(
            "could not read destination file {}: {error}",
            storage_path.display()
        )
    })?;
    let stored_destinations =
        serde_json::from_slice::<StoredDestinations>(&file_bytes).map_err(|error| {
            format!(
                "could not parse destination file {} as JSON: {error}",
                storage_path.display()
            )
        })?;

    Ok(stored_destinations.destinations)
}

fn persist_destinations(manager: &DestinationManager) -> Result<(), String> {
    let Some(storage_path) = &manager.storage_path else {
        return Err(
            "Could not determine local data directory for destination storage.".to_string(),
        );
    };

    persist_destinations_to_path(storage_path, &manager.destinations)
}

fn persist_destinations_to_path(
    storage_path: &Path,
    destinations: &BTreeMap<String, String>,
) -> Result<(), String> {
    let parent = storage_path.parent().ok_or_else(|| {
        format!(
            "destination file path {} has no parent directory",
            storage_path.display()
        )
    })?;
    let file_contents = serde_json::to_vec_pretty(&StoredDestinations {
        destinations: destinations.clone(),
    })
    .map_err(|error| format!("could not serialize destinations: {error}"))?;

    fs::create_dir_all(parent).map_err(|error| {
        format!(
            "could not create destination directory {}: {error}",
            parent.display()
        )
    })?;
    fs::write(storage_path, file_contents).map_err(|error| {
        format!(
            "could not write destination file {}: {error}",
            storage_path.display()
        )
    })?;

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn unique_test_path(name: &str) -> PathBuf {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("system clock should be after unix epoch")
            .as_nanos();

        std::env::temp_dir().join(format!("PDFOrganizer_destination_{name}_{nanos}"))
    }

    fn test_storage_path(name: &str) -> PathBuf {
        unique_test_path(name).join("destinations.json")
    }

    fn test_manager(name: &str) -> DestinationManager {
        DestinationManager::load_from_storage_path(test_storage_path(name))
    }

    // Verifies `begin_destination_selection()` stores the chosen path as a pending destination.
    #[test]
    fn begin_destination_selection_creates_pending_destination() {
        let mut manager = test_manager("begin_destination_selection");

        begin_destination_selection(&mut manager, "C:\\dest".to_string());

        assert_eq!(
            manager.pending_destination,
            Some(PendingDestination {
                path: "C:\\dest".to_string(),
                nickname: String::new(),
            })
        );
    }

    // Verifies `update_pending_destination_name()` updates the nickname of the pending destination.
    #[test]
    fn update_pending_destination_name_updates_pending_value() {
        let mut manager = test_manager("update_pending_destination_name");
        begin_destination_selection(&mut manager, "C:\\dest".to_string());

        update_pending_destination_name(&mut manager, "backup".to_string());

        assert_eq!(
            manager
                .pending_destination
                .as_ref()
                .map(|pending| pending.nickname.as_str()),
            Some("backup")
        );
    }

    // Verifies `save_pending_destination()` rejects a destination when the nickname is empty.
    #[test]
    fn save_pending_destination_rejects_empty_name() {
        let mut manager = test_manager("save_pending_destination_rejects_empty_name");
        begin_destination_selection(&mut manager, "C:\\dest".to_string());

        let result = save_pending_destination(&mut manager);

        assert_eq!(result, Err("Destination name is required.".to_string()));
        assert!(manager.pending_destination.is_some());
    }

    // Verifies `save_pending_destination()` stores a valid destination and clears the pending state.
    #[test]
    fn save_pending_destination_inserts_destination() {
        let storage_path = test_storage_path("save_pending_destination_inserts_destination");
        let mut manager = DestinationManager::load_from_storage_path(storage_path.clone());
        begin_destination_selection(&mut manager, "C:\\dest".to_string());
        update_pending_destination_name(&mut manager, "backup".to_string());

        let result = save_pending_destination(&mut manager);

        assert_eq!(result, Ok(()));
        assert_eq!(
            manager.destinations.get("backup"),
            Some(&"C:\\dest".to_string())
        );
        assert!(manager.pending_destination.is_none());

        let reloaded = DestinationManager::load_from_storage_path(storage_path.clone());
        assert_eq!(
            reloaded.destinations.get("backup"),
            Some(&"C:\\dest".to_string())
        );

        let _ = fs::remove_file(&storage_path);
        let _ = fs::remove_dir_all(
            storage_path
                .parent()
                .expect("storage path should have a parent"),
        );
    }

    // Verifies `save_pending_destination()` rejects duplicate destination nicknames.
    #[test]
    fn save_pending_destination_rejects_duplicate_name() {
        let mut manager = test_manager("save_pending_destination_rejects_duplicate_name");
        manager
            .destinations
            .insert("backup".to_string(), "C:\\existing".to_string());
        begin_destination_selection(&mut manager, "C:\\dest".to_string());
        update_pending_destination_name(&mut manager, "backup".to_string());

        let result = save_pending_destination(&mut manager);

        assert_eq!(result, Err("Destination name already exists.".to_string()));
        assert!(manager.pending_destination.is_some());
    }

    // Verifies cancelling a pending destination and removing a saved destination clears manager state.
    #[test]
    fn cancel_and_remove_destination_clear_state() {
        let storage_path = test_storage_path("cancel_and_remove_destination_clear_state");
        let mut manager = DestinationManager::load_from_storage_path(storage_path.clone());
        begin_destination_selection(&mut manager, "C:\\dest".to_string());
        cancel_pending_destination(&mut manager);
        manager
            .destinations
            .insert("backup".to_string(), "C:\\dest".to_string());
        persist_destinations(&manager).expect("initial destinations should be written");

        let removed = remove_destination(&mut manager, "backup");

        assert_eq!(removed, Ok(true));
        assert!(manager.pending_destination.is_none());
        assert!(manager.destinations.is_empty());

        let reloaded = DestinationManager::load_from_storage_path(storage_path.clone());
        assert!(reloaded.destinations.is_empty());

        let _ = fs::remove_file(&storage_path);
        let _ = fs::remove_dir_all(
            storage_path
                .parent()
                .expect("storage path should have a parent"),
        );
    }

    // Verifies starting and cancelling a PDF move updates the active PDF being moved.
    #[test]
    fn begin_and_cancel_destination_move_update_active_pdf() {
        let mut manager = test_manager("begin_and_cancel_destination_move_update_active_pdf");
        let pdf_path = PathBuf::from("sample.pdf");

        begin_destination_move(&mut manager, pdf_path.clone());
        assert_eq!(manager.active_move_pdf, Some(pdf_path));

        cancel_destination_move(&mut manager);
        assert!(manager.active_move_pdf.is_none());
    }

    // Verifies `move_pdf_to_destination()` moves the PDF into the destination folder and clears move state.
    #[test]
    fn move_pdf_to_destination_moves_file() {
        let source_dir = unique_test_path("source");
        let destination_dir = unique_test_path("destination");
        fs::create_dir_all(&source_dir).expect("source dir should exist");
        fs::create_dir_all(&destination_dir).expect("destination dir should exist");

        let source_file = source_dir.join("sample.pdf");
        fs::write(&source_file, b"pdf-data").expect("source file should exist");

        let mut manager = test_manager("move_pdf_to_destination_moves_file");
        manager
            .destinations
            .insert("archive".to_string(), destination_dir.display().to_string());

        let moved_path = move_pdf_to_destination(&mut manager, &source_file, "archive")
            .expect("move should work");

        assert_eq!(moved_path, destination_dir.join("sample.pdf"));
        assert!(moved_path.exists());
        assert!(!source_file.exists());
        assert!(manager.active_move_pdf.is_none());

        let _ = fs::remove_dir_all(&source_dir);
        let _ = fs::remove_dir_all(&destination_dir);
    }

    // Verifies `DestinationManager::default()` loads an empty list when the storage file does not exist.
    #[test]
    fn load_from_storage_path_returns_empty_destinations_for_missing_file() {
        let storage_path = test_storage_path("load_from_storage_path_returns_empty");

        let manager = DestinationManager::load_from_storage_path(storage_path);

        assert!(manager.destinations.is_empty());
        assert!(manager.error_message.is_none());
    }

    // Verifies `load_from_storage_path()` reports JSON parsing errors without crashing.
    #[test]
    fn load_from_storage_path_sets_error_for_invalid_json() {
        let storage_path = test_storage_path("load_from_storage_path_sets_error_for_invalid_json");
        let parent = storage_path
            .parent()
            .expect("storage path should have a parent");
        fs::create_dir_all(parent).expect("storage directory should be created");
        fs::write(&storage_path, b"{ invalid json").expect("invalid json should be written");

        let manager = DestinationManager::load_from_storage_path(storage_path.clone());

        assert!(manager.destinations.is_empty());
        assert!(
            manager
                .error_message
                .as_deref()
                .is_some_and(|message| message.contains("Failed to load destinations"))
        );

        let _ = fs::remove_file(&storage_path);
        let _ = fs::remove_dir_all(parent);
    }
}
