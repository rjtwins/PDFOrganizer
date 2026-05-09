use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

#[derive(Default)]
pub(crate) struct DestinationManager {
    pub(crate) destinations: BTreeMap<String, String>,
    pub(crate) pending_destination: Option<PendingDestination>,
    pub(crate) active_move_pdf: Option<PathBuf>,
    pub(crate) error_message: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct PendingDestination {
    pub(crate) path: String,
    pub(crate) nickname: String,
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
        .insert(nickname, pending_destination.path);
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

pub(crate) fn remove_destination(manager: &mut DestinationManager, nickname: &str) -> bool {
    manager.destinations.remove(nickname).is_some()
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

    #[test]
    fn begin_destination_selection_creates_pending_destination() {
        let mut manager = DestinationManager::default();

        begin_destination_selection(&mut manager, "C:\\dest".to_string());

        assert_eq!(
            manager.pending_destination,
            Some(PendingDestination {
                path: "C:\\dest".to_string(),
                nickname: String::new(),
            })
        );
    }

    #[test]
    fn update_pending_destination_name_updates_pending_value() {
        let mut manager = DestinationManager::default();
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

    #[test]
    fn save_pending_destination_rejects_empty_name() {
        let mut manager = DestinationManager::default();
        begin_destination_selection(&mut manager, "C:\\dest".to_string());

        let result = save_pending_destination(&mut manager);

        assert_eq!(result, Err("Destination name is required.".to_string()));
        assert!(manager.pending_destination.is_some());
    }

    #[test]
    fn save_pending_destination_inserts_destination() {
        let mut manager = DestinationManager::default();
        begin_destination_selection(&mut manager, "C:\\dest".to_string());
        update_pending_destination_name(&mut manager, "backup".to_string());

        let result = save_pending_destination(&mut manager);

        assert_eq!(result, Ok(()));
        assert_eq!(
            manager.destinations.get("backup"),
            Some(&"C:\\dest".to_string())
        );
        assert!(manager.pending_destination.is_none());
    }

    #[test]
    fn save_pending_destination_rejects_duplicate_name() {
        let mut manager = DestinationManager::default();
        manager
            .destinations
            .insert("backup".to_string(), "C:\\existing".to_string());
        begin_destination_selection(&mut manager, "C:\\dest".to_string());
        update_pending_destination_name(&mut manager, "backup".to_string());

        let result = save_pending_destination(&mut manager);

        assert_eq!(result, Err("Destination name already exists.".to_string()));
        assert!(manager.pending_destination.is_some());
    }

    #[test]
    fn cancel_and_remove_destination_clear_state() {
        let mut manager = DestinationManager::default();
        begin_destination_selection(&mut manager, "C:\\dest".to_string());
        cancel_pending_destination(&mut manager);
        manager
            .destinations
            .insert("backup".to_string(), "C:\\dest".to_string());

        let removed = remove_destination(&mut manager, "backup");

        assert!(removed);
        assert!(manager.pending_destination.is_none());
        assert!(manager.destinations.is_empty());
    }

    #[test]
    fn begin_and_cancel_destination_move_update_active_pdf() {
        let mut manager = DestinationManager::default();
        let pdf_path = PathBuf::from("sample.pdf");

        begin_destination_move(&mut manager, pdf_path.clone());
        assert_eq!(manager.active_move_pdf, Some(pdf_path));

        cancel_destination_move(&mut manager);
        assert!(manager.active_move_pdf.is_none());
    }

    #[test]
    fn move_pdf_to_destination_moves_file() {
        let source_dir = unique_test_path("source");
        let destination_dir = unique_test_path("destination");
        fs::create_dir_all(&source_dir).expect("source dir should exist");
        fs::create_dir_all(&destination_dir).expect("destination dir should exist");

        let source_file = source_dir.join("sample.pdf");
        fs::write(&source_file, b"pdf-data").expect("source file should exist");

        let mut manager = DestinationManager::default();
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
}
