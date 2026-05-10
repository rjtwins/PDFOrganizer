use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Mutex, OnceLock};

use iced::Task;
use iced::widget::image;
use pdfium_render::prelude::*;

static PDFIUM_RENDER_LOCK: OnceLock<Mutex<()>> = OnceLock::new();

pub(crate) enum PdfRenderState {
    Loading,
    Loaded(Vec<RenderedPdfPage>),
    Failed(String),
}

#[derive(Debug, Clone)]
pub(crate) struct RenderedPdfPage {
    pub(crate) page_number: usize,
    pub(crate) handle: image::Handle,
}

pub(crate) fn get_pdfs_in_folder(folder_path: &str) -> Vec<PathBuf> {
    let mut pdf_file_paths = Vec::new();

    if let Ok(entries) = std::fs::read_dir(folder_path) {
        for entry in entries.flatten() {
            let path = entry.path();

            if path.extension().and_then(|s| s.to_str()) == Some("pdf") {
                pdf_file_paths.push(path);
            }
        }
    }

    pdf_file_paths
}

pub(crate) fn schedule_pdf_renders(
    pdf_paths: &[PathBuf],
    pdf_renders: &mut HashMap<PathBuf, PdfRenderState>,
    render_width: u16,
    generation: u64,
) -> Task<crate::Message> {
    pdf_renders.clear();

    if pdf_paths.is_empty() {
        return Task::none();
    }

    let tasks = pdf_paths.iter().cloned().map(|pdf_path| {
        pdf_renders.insert(pdf_path.clone(), PdfRenderState::Loading);
        render_pdf_task(pdf_path, render_width, generation)
    });

    Task::batch(tasks)
}

pub(crate) fn render_pdf_task(
    pdf_path: PathBuf,
    render_width: u16,
    generation: u64,
) -> Task<crate::Message> {
    let future_path = pdf_path.clone();

    Task::perform(
        async move { get_pdf_pages(future_path, render_width).map_err(|error| error.to_string()) },
        move |result| crate::Message::PdfRendered {
            pdf_path,
            generation,
            result,
        },
    )
}

pub(crate) fn get_pdf_pages(
    file_path: PathBuf,
    render_width: u16,
) -> Result<Vec<RenderedPdfPage>, PdfiumError> {
    let render_lock = PDFIUM_RENDER_LOCK.get_or_init(|| Mutex::new(()));
    let _render_guard = render_lock.lock().expect("pdfium render lock poisoned");

    let pdfium = load_pdfium_from_runtime_location()?;
    let document = pdfium.load_pdf_from_file(file_path.as_path(), None)?;
    let render_config = PdfRenderConfig::new()
        .set_target_width(render_width.into())
        .set_maximum_height(1080)
        .rotate_if_landscape(PdfPageRenderRotation::None, true);

    let mut pages = Vec::new();

    for (index, page) in document.pages().iter().enumerate() {
        let dynamic_image = page.render_with_config(&render_config)?.as_image()?;
        let rgba_image = dynamic_image.to_rgba8();
        let (width, height) = rgba_image.dimensions();

        pages.push(RenderedPdfPage {
            page_number: index + 1,
            handle: image::Handle::from_rgba(width, height, rgba_image.into_raw()),
        });
    }

    Ok(pages)
}

fn load_pdfium_from_runtime_location() -> Result<Pdfium, PdfiumError> {
    if let Ok(executable_path) = std::env::current_exe() {
        if let Some(executable_dir) = executable_path.parent() {
            let bundled_library = Pdfium::pdfium_platform_library_name_at_path(executable_dir);

            if bundled_library.exists() {
                return match Pdfium::bind_to_library(&bundled_library) {
                    Ok(bindings) => Ok(Pdfium::new(bindings)),
                    Err(PdfiumError::PdfiumLibraryBindingsAlreadyInitialized) => {
                        Ok(Pdfium::default())
                    }
                    Err(error) => Err(error),
                };
            }
        }
    }

    match Pdfium::bind_to_system_library() {
        Ok(bindings) => Ok(Pdfium::new(bindings)),
        Err(PdfiumError::PdfiumLibraryBindingsAlreadyInitialized) => Ok(Pdfium::default()),
        Err(error) => Err(error),
    }
}
