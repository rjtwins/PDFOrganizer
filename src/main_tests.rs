use super::*;
use crate::pdf_proc::{get_pdf_pages, render_pdf_task};
use std::fs;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

fn sample_page(page_number: usize) -> RenderedPdfPage {
    RenderedPdfPage {
        page_number,
        handle: image::Handle::from_rgba(1, 1, vec![255, 0, 0, 255]),
    }
}

fn unique_test_path(name: &str) -> PathBuf {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("system clock should be after unix epoch")
        .as_nanos();

    std::env::temp_dir().join(format!("PDFOrganizer_{name}_{nanos}"))
}

fn create_minimal_pdf(path: &Path) {
    let header = "%PDF-1.4\n";
    let objects = [
        "1 0 obj\n<< /Type /Catalog /Pages 2 0 R >>\nendobj\n",
        "2 0 obj\n<< /Type /Pages /Kids [3 0 R] /Count 1 >>\nendobj\n",
        "3 0 obj\n<< /Type /Page /Parent 2 0 R /MediaBox [0 0 200 200] /Contents 4 0 R /Resources << >> >>\nendobj\n",
        "4 0 obj\n<< /Length 0 >>\nstream\n\nendstream\nendobj\n",
    ];

    let mut pdf = String::from(header);
    let mut offsets = Vec::new();

    for object in objects {
        offsets.push(pdf.len());
        pdf.push_str(object);
    }

    let xref_offset = pdf.len();
    pdf.push_str("xref\n0 5\n0000000000 65535 f \n");

    for offset in offsets {
        pdf.push_str(&format!("{offset:010} 00000 n \n"));
    }

    pdf.push_str("trailer\n<< /Root 1 0 R /Size 5 >>\n");
    pdf.push_str(&format!("startxref\n{xref_offset}\n%%EOF\n"));

    fs::write(path, pdf).expect("test pdf should be written");
}

fn create_test_folder_with_pdfs() -> PathBuf {
    let folder = unique_test_path("pdf_folder");
    fs::create_dir_all(&folder).expect("test folder should be created");
    create_minimal_pdf(&folder.join("first.pdf"));
    create_minimal_pdf(&folder.join("second.pdf"));
    fs::write(folder.join("notes.txt"), b"ignore me").expect("text file should be created");
    folder
}

fn cleanup_path(path: &Path) {
    if path.is_dir() {
        let _ = fs::remove_dir_all(path);
    } else {
        let _ = fs::remove_file(path);
    }
}

// Verifies the `main()` entrypoint keeps the expected iced application signature.
#[test]
fn main_has_expected_signature() {
    let entrypoint: fn() -> iced::Result = main;
    let _ = entrypoint;
}

// Verifies `State::default()` uses the initial window size and derived render width.
#[test]
fn state_default_uses_initial_window_metrics() {
    let state = State::default();

    assert_eq!(state.window_size.width, 900.0);
    assert_eq!(state.window_size.height, 800.0);
    assert_eq!(
        state.rendered_pdf_width,
        calculate_pdf_render_width(state.window_size.width)
    );
    assert_eq!(state.render_generation, 0);
    assert_eq!(state.resize_debounce_generation, 0);
    assert_eq!(state.pending_render_width, None);
}

// Verifies `FolderPicker::default()` starts with no selected folder.
#[test]
fn folder_picker_default_has_no_selection() {
    let folder_picker = FolderPicker::default();

    assert!(folder_picker.selected_folder.is_none());
}

// Verifies `subscription()` can be created for the current application state.
#[test]
fn subscription_can_be_created() {
    let _subscription = subscription(&State::default());
}

// Verifies `load_selected_folder()` stores the chosen folder, filters PDFs, and marks them loading.
#[test]
fn load_selected_folder_tracks_folder_and_marks_pdfs_loading() {
    let folder = create_test_folder_with_pdfs();
    let folder_string = folder.display().to_string();
    let mut state = State::default();

    let task = load_selected_folder(&mut state, folder_string.clone());

    assert_eq!(
        state.folder_picker.selected_folder.as_deref(),
        Some(folder_string.as_str())
    );
    assert_eq!(state.pdfs_in_folder.len(), 2);
    assert_eq!(state.render_generation, 1);
    assert!(task.units() > 0);
    assert!(
        state
            .pdfs_in_folder
            .iter()
            .all(|path| matches!(state.pdf_renders.get(path), Some(PdfRenderState::Loading)))
    );

    cleanup_path(&folder);
}

// Verifies `update()` records the latest resize and starts a debounce task instead of rerendering immediately.
#[test]
fn update_window_resized_tracks_pending_render_width() {
    let mut state = State::default();
    let size = Size::new(1200.0, 700.0);

    let task = update(&mut state, Message::WindowResized(size));

    assert_eq!(state.window_size.width, 1200.0);
    assert_eq!(state.window_size.height, 700.0);
    assert_eq!(
        state.pending_render_width,
        Some(calculate_pdf_render_width(size.width))
    );
    assert_eq!(state.resize_debounce_generation, 1);
    assert!(task.units() > 0);
}

// Verifies `update()` skips debounce work when a resize does not change the target render width.
#[test]
fn update_window_resized_ignores_same_render_width() {
    let mut state = State::default();
    let current_size = state.window_size;

    let task = update(&mut state, Message::WindowResized(current_size));

    assert_eq!(task.units(), 0);
    assert!(state.pending_render_width.is_none());
}

// Verifies `update()` ignores stale debounce completions from older resize generations.
#[test]
fn update_ignores_stale_debounced_resize() {
    let mut state = State::default();
    state.resize_debounce_generation = 3;
    state.pending_render_width = Some(444);

    let task = update(
        &mut state,
        Message::DebouncedResizeTriggered {
            generation: 2,
            render_width: 444,
        },
    );

    assert_eq!(task.units(), 0);
    assert_eq!(state.pending_render_width, Some(444));
    assert_eq!(state.render_generation, 0);
}

// Verifies `update()` starts rerendering once the latest debounced resize fires.
#[test]
fn update_current_debounced_resize_schedules_renders() {
    let mut state = State::default();
    let pdf_path = PathBuf::from("example.pdf");
    state.pdfs_in_folder = vec![pdf_path.clone()];
    state.pending_render_width = Some(555);
    state.resize_debounce_generation = 1;

    let task = update(
        &mut state,
        Message::DebouncedResizeTriggered {
            generation: 1,
            render_width: 555,
        },
    );

    assert_eq!(state.pending_render_width, None);
    assert_eq!(state.rendered_pdf_width, 555);
    assert_eq!(state.render_generation, 1);
    assert!(task.units() > 0);
    assert!(matches!(
        state.pdf_renders.get(&pdf_path),
        Some(PdfRenderState::Loading)
    ));
}

// Verifies `update()` stores successfully rendered pages for the active generation.
#[test]
fn update_stores_successful_pdf_render_results() {
    let mut state = State::default();
    let pdf_path = PathBuf::from("success.pdf");
    state.render_generation = 7;

    let task = update(
        &mut state,
        Message::PdfRendered {
            pdf_path: pdf_path.clone(),
            generation: 7,
            result: Ok(vec![sample_page(1), sample_page(2)]),
        },
    );

    assert_eq!(task.units(), 0);
    match state.pdf_renders.get(&pdf_path) {
        Some(PdfRenderState::Loaded(pages)) => assert_eq!(pages.len(), 2),
        _ => panic!("expected loaded pages to be stored"),
    }
}

// Verifies `update()` stores render failures for the active generation.
#[test]
fn update_stores_failed_pdf_render_results() {
    let mut state = State::default();
    let pdf_path = PathBuf::from("failure.pdf");
    state.render_generation = 3;

    let _ = update(
        &mut state,
        Message::PdfRendered {
            pdf_path: pdf_path.clone(),
            generation: 3,
            result: Err("boom".to_string()),
        },
    );

    match state.pdf_renders.get(&pdf_path) {
        Some(PdfRenderState::Failed(error)) => assert_eq!(error, "boom"),
        _ => panic!("expected failed render state to be stored"),
    }
}

// Verifies `update()` ignores render results from an older render generation.
#[test]
fn update_ignores_stale_pdf_render_results() {
    let mut state = State::default();
    let pdf_path = PathBuf::from("stale.pdf");
    state.render_generation = 5;
    state
        .pdf_renders
        .insert(pdf_path.clone(), PdfRenderState::Loading);

    let _ = update(
        &mut state,
        Message::PdfRendered {
            pdf_path: pdf_path.clone(),
            generation: 4,
            result: Ok(vec![sample_page(1)]),
        },
    );

    assert!(matches!(
        state.pdf_renders.get(&pdf_path),
        Some(PdfRenderState::Loading)
    ));
}

// Verifies `view()` can build the placeholder UI before any folder has been chosen.
#[test]
fn view_builds_without_selected_folder() {
    let state = State::default();
    let _element = view(&state);
}

// Verifies `view()` can build the full UI after a folder and rendered pages exist.
#[test]
fn view_builds_with_rendered_folder_contents() {
    let mut state = State::default();
    let pdf_path = PathBuf::from("loaded.pdf");
    state.folder_picker.selected_folder = Some("C:\\temp".to_string());
    state.pdfs_in_folder.push(pdf_path.clone());
    state
        .pdf_renders
        .insert(pdf_path, PdfRenderState::Loaded(vec![sample_page(1)]));

    let _element = view(&state);
}

// Verifies `generate_pdf_sections()` can build sections for loading, loaded, and failed PDFs together.
#[test]
fn generate_pdf_sections_builds_for_all_render_states() {
    let mut state = State::default();
    let loading = PathBuf::from("loading.pdf");
    let loaded = PathBuf::from("loaded.pdf");
    let failed = PathBuf::from("failed.pdf");
    state.pdfs_in_folder = vec![loading.clone(), loaded.clone(), failed.clone()];
    state.pdf_renders.insert(loading, PdfRenderState::Loading);
    state
        .pdf_renders
        .insert(loaded, PdfRenderState::Loaded(vec![sample_page(1)]));
    state
        .pdf_renders
        .insert(failed, PdfRenderState::Failed("bad pdf".to_string()));

    let _column = generate_pdf_sections(&state);
}

// Verifies `generate_pdf_section()` can build a section when no pages were rendered.
#[test]
fn generate_pdf_section_builds_for_empty_page_list() {
    let destination_manager = DestinationManager::default();
    let pdf_path = PathBuf::from("empty.pdf");
    let _element = generate_pdf_section(&pdf_path, &[], &destination_manager);
}

// Verifies `generate_pdf_section()` can build a section for rendered pages.
#[test]
fn generate_pdf_section_builds_for_rendered_pages() {
    let pages = vec![sample_page(1), sample_page(2)];
    let destination_manager = DestinationManager::default();
    let pdf_path = PathBuf::from("pages.pdf");
    let _element = generate_pdf_section(&pdf_path, &pages, &destination_manager);
}

// Verifies `generate_loading_pdf_section()` builds the loading placeholder widget.
#[test]
fn generate_loading_pdf_section_builds() {
    let destination_manager = DestinationManager::default();
    let pdf_path = PathBuf::from("loading.pdf");
    let _element = generate_loading_pdf_section(&pdf_path, &destination_manager);
}

// Verifies `generate_failed_pdf_section()` builds the error placeholder widget.
#[test]
fn generate_failed_pdf_section_builds() {
    let destination_manager = DestinationManager::default();
    let pdf_path = PathBuf::from("failed.pdf");
    let _element = generate_failed_pdf_section(&pdf_path, "render error", &destination_manager);
}

// Verifies `generate_pdf_page_image()` builds an image widget from a stable page handle.
#[test]
fn generate_pdf_page_image_builds() {
    let page = sample_page(1);
    let _element = generate_pdf_page_image(&page);
}

// Verifies `generate_select_folder_button()` builds the folder selection button widget.
#[test]
fn generate_select_folder_button_builds() {
    let _element = generate_select_folder_button();
}

// Verifies `generate_folder_path_display()` builds the read-only path display widget.
#[test]
fn generate_folder_path_display_builds() {
    let _element = generate_folder_path_display("C:\\temp");
}

// Verifies `get_pdfs_in_folder()` only returns lowercase `.pdf` files from a directory.
#[test]
fn get_pdfs_in_folder_filters_non_pdf_files() {
    let folder = unique_test_path("filter_folder");
    fs::create_dir_all(&folder).expect("test folder should be created");
    create_minimal_pdf(&folder.join("match.pdf"));
    create_minimal_pdf(&folder.join("skip.PDF"));
    fs::write(folder.join("skip.txt"), b"ignore").expect("text file should be written");

    let pdfs = get_pdfs_in_folder(&folder.display().to_string());

    assert_eq!(pdfs.len(), 1);
    assert_eq!(
        pdfs[0].file_name().and_then(|name| name.to_str()),
        Some("match.pdf")
    );

    cleanup_path(&folder);
}

// Verifies `calculate_pdf_render_width()` subtracts the outer paddings from the available width.
#[test]
fn calculate_pdf_render_width_accounts_for_padding() {
    let width = calculate_pdf_render_width(900.0);

    assert_eq!(width, 852);
}

// Verifies `calculate_pdf_render_width()` clamps tiny widths to at least one pixel.
#[test]
fn calculate_pdf_render_width_clamps_to_one_pixel() {
    let width = calculate_pdf_render_width(1.0);

    assert_eq!(width, 1);
}

// Verifies `schedule_pdf_renders()` clears stale entries, marks every PDF as loading, and returns work.
#[test]
fn schedule_pdf_renders_marks_all_paths_loading() {
    let pdf_paths = vec![PathBuf::from("one.pdf"), PathBuf::from("two.pdf")];
    let mut pdf_renders = HashMap::new();
    pdf_renders.insert(
        PathBuf::from("old.pdf"),
        PdfRenderState::Failed("old".to_string()),
    );

    let task = schedule_pdf_renders(&pdf_paths, &mut pdf_renders, 300, 9);

    assert_eq!(pdf_renders.len(), 2);
    assert!(task.units() > 0);
    assert!(
        pdf_paths
            .iter()
            .all(|path| matches!(pdf_renders.get(path), Some(PdfRenderState::Loading)))
    );
}

// Verifies `schedule_pdf_renders()` returns no work for an empty PDF list.
#[test]
fn schedule_pdf_renders_returns_none_for_empty_input() {
    let mut pdf_renders = HashMap::new();

    let task = schedule_pdf_renders(&[], &mut pdf_renders, 300, 1);

    assert_eq!(task.units(), 0);
    assert!(pdf_renders.is_empty());
}

// Verifies `render_pdf_task()` produces a background task for a single PDF render request.
#[test]
fn render_pdf_task_produces_work() {
    let task = render_pdf_task(PathBuf::from("task.pdf"), 320, 4);

    assert!(task.units() > 0);
}

// Verifies `debounce_resize_task()` produces delayed work for the latest resize generation.
#[test]
fn debounce_resize_task_produces_work() {
    let task = debounce_resize_task(2, 480);

    assert!(task.units() > 0);
}

// Verifies `get_pdf_pages()` can render a minimal one-page PDF into stable page handles.
#[test]
fn get_pdf_pages_renders_minimal_pdf() {
    let pdf_path = unique_test_path("minimal.pdf");
    create_minimal_pdf(&pdf_path);

    let pages = get_pdf_pages(pdf_path.clone(), 200).expect("minimal pdf should render");

    assert_eq!(pages.len(), 1);
    assert_eq!(pages[0].page_number, 1);

    cleanup_path(&pdf_path);
}
