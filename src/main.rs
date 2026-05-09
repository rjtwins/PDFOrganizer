#![cfg_attr(target_os = "windows", windows_subsystem = "windows")]

use std::collections::HashMap;
use std::path::Path;
use std::path::PathBuf;
use std::time::Duration;

use destination::{
    DestinationManager, PendingDestination, begin_destination_move, begin_destination_selection,
    cancel_destination_move, cancel_pending_destination, move_pdf_to_destination,
    remove_destination, save_pending_destination, update_pending_destination_name,
};
use iced::widget::{Column, button, column, container, image, row, scrollable, text, text_input};
use iced::{Element, Fill, Length, Size, Subscription, Task, window};
use pdf_proc::{PdfRenderState, RenderedPdfPage, get_pdfs_in_folder, schedule_pdf_renders};

mod destination;
mod pdf_proc;

#[cfg(test)]
mod main_tests;

fn main() -> iced::Result {
    iced::application(State::default, update, view)
        .title("PDFOrganizer")
        .subscription(subscription)
        .window_size((900.0, 800.0))
        .resizable(true)
        .centered()
        .run()
}

const APP_PADDING: f32 = 12.0;
const PDF_SECTION_PADDING: f32 = 12.0;
const RESIZE_DEBOUNCE_DELAY: Duration = Duration::from_secs(1);

struct State {
    destination_manager: DestinationManager,
    destinations_collapsed: bool,
    folder_picker: FolderPicker,
    pdfs_in_folder: Vec<PathBuf>,
    pdf_renders: HashMap<PathBuf, PdfRenderState>,
    window_size: Size,
    rendered_pdf_width: u16,
    render_generation: u64,
    resize_debounce_generation: u64,
    pending_render_width: Option<u16>,
}

impl Default for State {
    fn default() -> Self {
        let window_size = Size::new(900.0, 800.0);

        Self {
            destination_manager: DestinationManager::default(),
            destinations_collapsed: false,
            folder_picker: FolderPicker::default(),
            pdfs_in_folder: Vec::new(),
            pdf_renders: HashMap::new(),
            window_size,
            rendered_pdf_width: calculate_pdf_render_width(window_size.width),
            render_generation: 0,
            resize_debounce_generation: 0,
            pending_render_width: None,
        }
    }
}

#[derive(Default)]
struct FolderPicker {
    selected_folder: Option<String>,
}

#[derive(Debug, Clone)]
pub(crate) enum Message {
    AddDestination,
    BeginMovePdf(PathBuf),
    CancelDestination,
    CancelMovePdf,
    MovePdfToDestination {
        pdf_path: PathBuf,
        destination_nickname: String,
    },
    PendingDestinationNameChanged(String),
    RemoveDestination(String),
    SaveDestination,
    ToggleDestinationsCollapsed,
    SelectFolder,
    WindowResized(Size),
    DebouncedResizeTriggered {
        generation: u64,
        render_width: u16,
    },
    PdfRendered {
        pdf_path: PathBuf,
        generation: u64,
        result: Result<Vec<RenderedPdfPage>, String>,
    },
}

fn subscription(_state: &State) -> Subscription<Message> {
    window::resize_events().map(|(_, size)| Message::WindowResized(size))
}

fn update(state: &mut State, message: Message) -> Task<Message> {
    match message {
        Message::AddDestination => {
            if let Some(path) = rfd::FileDialog::new().pick_folder() {
                begin_destination_selection(
                    &mut state.destination_manager,
                    path.display().to_string(),
                );
            }

            Task::none()
        }
        Message::BeginMovePdf(pdf_path) => {
            begin_destination_move(&mut state.destination_manager, pdf_path);
            Task::none()
        }
        Message::CancelDestination => {
            cancel_pending_destination(&mut state.destination_manager);
            Task::none()
        }
        Message::CancelMovePdf => {
            cancel_destination_move(&mut state.destination_manager);
            Task::none()
        }
        Message::MovePdfToDestination {
            pdf_path,
            destination_nickname,
        } => {
            match move_pdf_to_destination(
                &mut state.destination_manager,
                &pdf_path,
                &destination_nickname,
            ) {
                Ok(_) => {
                    remove_pdf_from_state(state, &pdf_path);
                    Task::none()
                }
                Err(error) => {
                    state.destination_manager.error_message = Some(error);
                    Task::none()
                }
            }
        }
        Message::PendingDestinationNameChanged(nickname) => {
            update_pending_destination_name(&mut state.destination_manager, nickname);
            Task::none()
        }
        Message::RemoveDestination(nickname) => {
            remove_destination(&mut state.destination_manager, &nickname);
            Task::none()
        }
        Message::SaveDestination => {
            if let Err(error) = save_pending_destination(&mut state.destination_manager) {
                state.destination_manager.error_message = Some(error);
            }

            Task::none()
        }
        Message::ToggleDestinationsCollapsed => {
            state.destinations_collapsed = !state.destinations_collapsed;
            Task::none()
        }
        Message::SelectFolder => {
            if let Some(path) = rfd::FileDialog::new().pick_folder() {
                load_selected_folder(state, path.display().to_string())
            } else {
                Task::none()
            }
        }
        Message::WindowResized(size) => {
            state.window_size = size;

            let render_width = calculate_pdf_render_width(size.width);

            if render_width != state.rendered_pdf_width
                || state
                    .pending_render_width
                    .is_some_and(|pending| pending != render_width)
            {
                state.resize_debounce_generation += 1;
                state.pending_render_width = Some(render_width);

                debounce_resize_task(state.resize_debounce_generation, render_width)
            } else {
                Task::none()
            }
        }
        Message::DebouncedResizeTriggered {
            generation,
            render_width,
        } => {
            if generation != state.resize_debounce_generation
                || state.pending_render_width != Some(render_width)
            {
                return Task::none();
            }

            state.pending_render_width = None;
            state.rendered_pdf_width = render_width;
            state.render_generation += 1;

            if state.pdfs_in_folder.is_empty() {
                Task::none()
            } else {
                schedule_pdf_renders(
                    &state.pdfs_in_folder,
                    &mut state.pdf_renders,
                    render_width,
                    state.render_generation,
                )
            }
        }
        Message::PdfRendered {
            pdf_path,
            generation,
            result,
        } => {
            if generation == state.render_generation {
                let render_state = match result {
                    Ok(images) => PdfRenderState::Loaded(images),
                    Err(error) => PdfRenderState::Failed(error),
                };

                state.pdf_renders.insert(pdf_path, render_state);
            }

            Task::none()
        }
    }
}

fn load_selected_folder(state: &mut State, selected_folder: String) -> Task<Message> {
    state.folder_picker.selected_folder = Some(selected_folder.clone());
    state.pdfs_in_folder = get_pdfs_in_folder(&selected_folder);
    state.rendered_pdf_width = calculate_pdf_render_width(state.window_size.width);
    state.render_generation += 1;

    schedule_pdf_renders(
        &state.pdfs_in_folder,
        &mut state.pdf_renders,
        state.rendered_pdf_width,
        state.render_generation,
    )
}

fn view(state: &State) -> Element<'_, Message> {
    let selected_folder = state
        .folder_picker
        .selected_folder
        .as_deref()
        .unwrap_or("No folder selected");

    let pdf_sections: Element<'_, Message> = if state.folder_picker.selected_folder.is_some() {
        scrollable(generate_pdf_sections(state)).height(Fill).into()
    } else {
        text("Select a folder to load PDF pages.").into()
    };

    container(
        column![
            generate_destinations_section(
                &state.destination_manager,
                state.destinations_collapsed,
            ),
            generate_select_folder_button(),
            text("Selected folder:"),
            generate_folder_path_display(selected_folder),
            pdf_sections,
        ]
        .spacing(12)
        .padding(12),
    )
    .width(Fill)
    .height(Fill)
    .align_top(Fill)
    .center_x(Fill)
    .into()
}

fn generate_destinations_section<'a>(
    destination_manager: &'a DestinationManager,
    is_collapsed: bool,
) -> Element<'a, Message> {
    let toggle_label = if is_collapsed {
        "Show Destinations"
    } else {
        "Hide Destinations"
    };

    let mut content = Column::new()
        .push(
            row![
                text("Destinations").size(24),
                button(toggle_label).on_press(Message::ToggleDestinationsCollapsed),
            ]
            .spacing(8),
        )
        .spacing(10)
        .width(Fill);

    if is_collapsed {
        return container(content)
            .padding(12)
            .width(Fill)
            .style(iced::widget::container::rounded_box)
            .into();
    }

    content = content.push(generate_add_destination_button());

    if let Some(pending_destination) = &destination_manager.pending_destination {
        content = content.push(generate_pending_destination_editor(pending_destination));
    }

    if let Some(error_message) = &destination_manager.error_message {
        content = content.push(text(error_message));
    }

    if destination_manager.destinations.is_empty() {
        content = content.push(text("No destinations added yet."));
    } else {
        content = content.push(generate_destination_list(destination_manager));
    }

    container(content)
        .padding(12)
        .width(Fill)
        .style(iced::widget::container::rounded_box)
        .into()
}

fn generate_add_destination_button<'a>() -> Element<'a, Message> {
    button("Add Destination")
        .on_press(Message::AddDestination)
        .into()
}

fn generate_pending_destination_editor<'a>(
    pending_destination: &'a PendingDestination,
) -> Element<'a, Message> {
    container(
        column![
            text("Selected destination folder:"),
            generate_folder_path_display(&pending_destination.path),
            text_input("Destination name", &pending_destination.nickname)
                .on_input(Message::PendingDestinationNameChanged),
            row![
                button("Save").on_press(Message::SaveDestination),
                button("Cancel").on_press(Message::CancelDestination),
            ]
            .spacing(8),
        ]
        .spacing(8)
        .width(Fill),
    )
    .padding(8)
    .width(Fill)
    .style(iced::widget::container::rounded_box)
    .into()
}

fn generate_destination_list<'a>(
    destination_manager: &'a DestinationManager,
) -> Element<'a, Message> {
    let destinations = destination_manager.destinations.iter().fold(
        Column::new().spacing(8).width(Fill),
        |column, (nickname, path)| {
            column.push(
                container(
                    row![
                        column![text(nickname), text(path)].spacing(4).width(Fill),
                        button("Remove").on_press(Message::RemoveDestination(nickname.clone(),)),
                    ]
                    .spacing(8)
                    .width(Fill),
                )
                .padding(8)
                .width(Fill)
                .style(iced::widget::container::rounded_box),
            )
        },
    );

    scrollable(destinations).height(Length::Fixed(180.0)).into()
}

fn generate_pdf_sections(state: &State) -> Column<'_, Message> {
    state
        .pdfs_in_folder
        .iter()
        .fold(Column::new().spacing(16).width(Fill), |column, pdf_path| {
            let section = match state.pdf_renders.get(pdf_path) {
                Some(PdfRenderState::Loading) => {
                    generate_loading_pdf_section(pdf_path, &state.destination_manager)
                }
                Some(PdfRenderState::Loaded(images)) => {
                    generate_pdf_section(pdf_path, images, &state.destination_manager)
                }
                Some(PdfRenderState::Failed(error)) => {
                    generate_failed_pdf_section(pdf_path, error, &state.destination_manager)
                }
                None => generate_loading_pdf_section(pdf_path, &state.destination_manager),
            };

            column.push(section)
        })
}

fn generate_pdf_section<'a>(
    pdf_path: &'a PathBuf,
    images: &'a [RenderedPdfPage],
    destination_manager: &'a DestinationManager,
) -> Element<'a, Message> {
    let file_name = pdf_display_name(pdf_path);
    let image_list = if images.is_empty() {
        Column::new()
            .push(text("No pages were rendered for this PDF."))
            .width(Fill)
    } else {
        images.iter().enumerate().fold(
            Column::new().spacing(12).width(Fill),
            |column, (_, page)| column.push(generate_pdf_page_image(page)),
        )
    };

    container(
        column![
            text(file_name).size(24),
            generate_pdf_destination_controls(pdf_path, destination_manager),
            scrollable(image_list).height(Length::Fixed(320.0)),
        ]
        .spacing(10)
        .width(Fill),
    )
    .padding(12)
    .width(Fill)
    .style(iced::widget::container::rounded_box)
    .into()
}

fn generate_loading_pdf_section<'a>(
    pdf_path: &'a PathBuf,
    destination_manager: &'a DestinationManager,
) -> Element<'a, Message> {
    container(
        column![
            text(pdf_display_name(pdf_path)).size(24),
            generate_pdf_destination_controls(pdf_path, destination_manager),
            text("Rendering preview..."),
        ]
        .spacing(10)
        .width(Fill),
    )
    .padding(12)
    .width(Fill)
    .style(iced::widget::container::rounded_box)
    .into()
}

fn generate_failed_pdf_section<'a>(
    pdf_path: &'a PathBuf,
    error: &'a str,
    destination_manager: &'a DestinationManager,
) -> Element<'a, Message> {
    container(
        column![
            text(pdf_display_name(pdf_path)).size(24),
            generate_pdf_destination_controls(pdf_path, destination_manager),
            text("Failed to render this PDF."),
            text(error),
        ]
        .spacing(10)
        .width(Fill),
    )
    .padding(12)
    .width(Fill)
    .style(iced::widget::container::rounded_box)
    .into()
}

fn generate_pdf_page_image<'a>(page: &'a RenderedPdfPage) -> Element<'a, Message> {
    container(
        column![
            text(format!("Page {}", page.page_number)),
            image(page.handle.clone()).width(Fill),
        ]
        .spacing(8)
        .width(Fill),
    )
    .width(Fill)
    .into()
}

fn generate_pdf_destination_controls<'a>(
    pdf_path: &'a PathBuf,
    destination_manager: &'a DestinationManager,
) -> Element<'a, Message> {
    let mut content = Column::new()
        .push(button("Move To Destination").on_press(Message::BeginMovePdf(pdf_path.clone())))
        .spacing(8)
        .width(Fill);

    if destination_manager.active_move_pdf.as_ref() == Some(pdf_path) {
        if destination_manager.destinations.is_empty() {
            content = content
                .push(text("Add at least one destination before moving this PDF."))
                .push(button("Cancel").on_press(Message::CancelMovePdf));
        } else {
            let choices = destination_manager.destinations.iter().fold(
                Column::new().spacing(6).width(Fill),
                |column, (nickname, path)| {
                    column.push(button(text(format!("{nickname} -> {path}"))).on_press(
                        Message::MovePdfToDestination {
                            pdf_path: pdf_path.clone(),
                            destination_nickname: nickname.clone(),
                        },
                    ))
                },
            );

            content = content
                .push(text("Choose destination:"))
                .push(choices)
                .push(button("Cancel").on_press(Message::CancelMovePdf));
        }
    }

    container(content)
        .padding(8)
        .width(Fill)
        .style(iced::widget::container::rounded_box)
        .into()
}

fn generate_select_folder_button<'a>() -> Element<'a, Message> {
    button("Select Folder")
        .on_press(Message::SelectFolder)
        .into()
}

fn generate_folder_path_display<'a>(path: &'a str) -> Element<'a, Message> {
    container(text(path).width(Fill))
        .padding(8)
        .width(Fill)
        .style(iced::widget::container::rounded_box)
        .into()
}

fn pdf_display_name(pdf_path: &Path) -> &str {
    pdf_path
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("Unnamed PDF")
}

fn calculate_pdf_render_width(window_width: f32) -> u16 {
    (window_width - (APP_PADDING * 2.0) - (PDF_SECTION_PADDING * 2.0))
        .max(1.0)
        .round() as u16
}

fn debounce_resize_task(generation: u64, render_width: u16) -> Task<Message> {
    Task::perform(
        async move {
            std::thread::sleep(RESIZE_DEBOUNCE_DELAY);
            render_width
        },
        move |render_width| Message::DebouncedResizeTriggered {
            generation,
            render_width,
        },
    )
}

fn remove_pdf_from_state(state: &mut State, pdf_path: &Path) {
    state.pdfs_in_folder.retain(|path| path != pdf_path);
    state.pdf_renders.remove(pdf_path);
}
