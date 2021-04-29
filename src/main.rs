use anyhow::Result;
use iced::{
    button, scrollable, Application, Button, Column, Command, Container, Length, Row, Scrollable,
    Subscription, Text,
};
use importer::Progress;
use std::path::PathBuf;

mod importer;
mod importer_funcs;

#[derive(Debug, Default, Clone)]
pub struct Summary {
    scores_found: u32,
    scores_imported: u32,
    fail_messages: Vec<String>,
}

#[derive(Debug, Default)]
struct State {
    ksm_path: Option<PathBuf>,
    db_path: Option<PathBuf>,
    progress: Option<importer::Progress>,
    summary: Option<Summary>,
    ksm_button: button::State,
    db_button: button::State,
    import_button: button::State,
    back_button: button::State,
    error_scroll: scrollable::State,
}

#[derive(Debug, Clone)]
enum Message {
    KsmButton,
    DbButton,
    BackButton,
    Start,
    Progress(importer::Progress),
}

fn main() -> Result<()> {
    let settings = iced::Settings {
        window: iced::window::Settings {
            size: (800, 600),
            resizable: true,
            decorations: true,
            min_size: Some((400, 300)),
            max_size: None,
            transparent: false,
            always_on_top: false,
            icon: None,
        },
        antialiasing: true,
        ..Default::default()
    };
    Ok(State::run(settings)?)
}

enum Stage {
    Paths,
    Importing,
    Finished,
}

impl Application for State {
    type Executor = iced::executor::Default;

    type Message = Message;

    type Flags = ();

    fn new(_flags: Self::Flags) -> (Self, iced::Command<Self::Message>) {
        (Self::default(), Command::none())
    }

    fn title(&self) -> String {
        "KSM To USC Score Import Tool".to_string()
    }

    fn subscription(&self) -> Subscription<Self::Message> {
        match (&self.ksm_path, &self.db_path, &self.progress) {
            (Some(ksm_path), Some(db_path), Some(_progress)) => {
                match importer::import(ksm_path, db_path) {
                    Ok(s) => s.map(|p| Message::Progress(p)),
                    Err(e) => {
                        rfd::MessageDialog::new()
                            .set_title("Failed to start import")
                            .set_description(&format!("{:?}", e))
                            .set_level(rfd::MessageLevel::Error)
                            .set_buttons(rfd::MessageButtons::Ok)
                            .show();
                        Subscription::none()
                    }
                }
            }
            _ => Subscription::none(),
        }
    }

    fn update(
        &mut self,
        message: Self::Message,
        _clipboard: &mut iced::Clipboard,
    ) -> iced::Command<Self::Message> {
        match message {
            Message::Progress(p) => match p {
                Progress::Finished(s) => {
                    self.progress = None;
                    self.summary = Some(s)
                }
                _ => self.progress = Some(p),
            },
            Message::KsmButton => self.ksm_path = rfd::FileDialog::new().pick_folder(),
            Message::DbButton => {
                self.db_path = rfd::FileDialog::new()
                    .add_filter("Database", &["db"])
                    .pick_file()
            }
            Message::Start => match (&self.db_path, &self.ksm_path) {
                (Some(db), Some(ksm)) => match importer::validate_paths(ksm, db) {
                    Ok(_) => self.progress = Some(importer::Progress::Started),
                    Err(e) => {
                        rfd::MessageDialog::new()
                            .set_title("Failed to start import")
                            .set_description(&format!("{:?}", e))
                            .set_level(rfd::MessageLevel::Error)
                            .set_buttons(rfd::MessageButtons::Ok)
                            .show();
                    }
                },

                _ => {}
            },
            Message::BackButton => self.progress = None,
        };

        Command::none()
    }

    fn view(&mut self) -> iced::Element<'_, Self::Message> {
        let stage = match (
            self.ksm_path.is_some(),
            self.db_path.is_some(),
            self.progress.is_some(),
            self.summary.is_some(),
        ) {
            (false, false, _, _) | (true, false, _, _) | (false, true, _, _) => Stage::Paths,
            (true, true, false, false) => Stage::Paths,
            (_, _, _, true) => Stage::Finished,
            (_, _, true, false) => Stage::Importing,
        };

        let content = match stage {
            Stage::Paths => Column::new()
                .align_items(iced::Align::Center)
                .spacing(20)
                .push(
                    Row::new()
                        .align_items(iced::Align::Center)
                        .spacing(10)
                        .push(
                            Text::new(
                                self.ksm_path
                                    .clone()
                                    .unwrap_or_default()
                                    .to_str()
                                    .unwrap_or_default(),
                            )
                            .width(Length::FillPortion(3))
                            .horizontal_alignment(iced::HorizontalAlignment::Right)
                            .vertical_alignment(iced::VerticalAlignment::Center),
                        )
                        .push(
                            Button::new(
                                &mut self.ksm_button,
                                Text::new("KSM Path")
                                    .horizontal_alignment(iced::HorizontalAlignment::Center),
                            )
                            .on_press(Message::KsmButton)
                            .width(Length::FillPortion(1)),
                        ),
                )
                .push(
                    Row::new()
                        .align_items(iced::Align::Center)
                        .spacing(10)
                        .push(
                            Text::new(
                                self.db_path
                                    .clone()
                                    .unwrap_or_default()
                                    .to_str()
                                    .unwrap_or_default(),
                            )
                            .width(Length::FillPortion(3))
                            .horizontal_alignment(iced::HorizontalAlignment::Right)
                            .vertical_alignment(iced::VerticalAlignment::Center),
                        )
                        .push(
                            Button::new(
                                &mut self.db_button,
                                Text::new("USC maps.db Path")
                                    .horizontal_alignment(iced::HorizontalAlignment::Center),
                            )
                            .on_press(Message::DbButton)
                            .width(Length::FillPortion(1)),
                        ),
                )
                .push(
                    Button::new(
                        &mut self.import_button,
                        Text::new("Import").horizontal_alignment(iced::HorizontalAlignment::Center),
                    )
                    .on_press(Message::Start),
                ),

            Stage::Importing => Column::new()
                .push(Text::new("Importing"))
                .push(match self.progress.as_ref().unwrap() {
                    importer::Progress::Advanced(p, _score_file) => {
                        Column::new().push(iced::ProgressBar::new(0.0_f32..=1.0_f32, *p))
                    }

                    importer::Progress::Started => Column::new().push(Text::new("Starting")),
                    importer::Progress::Finished(_) => Column::new().push(Text::new("Finished")),
                    importer::Progress::Errored(e) => {
                        Column::new().push(Text::new(&format!("Error: {}", e)))
                    }
                })
                .push(match self.progress.as_ref().unwrap() {
                    importer::Progress::Errored(_) => Row::new().push(
                        Button::new(&mut self.back_button, Text::new("Back"))
                            .on_press(Message::BackButton),
                    ),
                    _ => Row::new(),
                }),
            Stage::Finished => {
                let summary = self.summary.as_ref().unwrap();
                let error_view = summary
                    .fail_messages
                    .iter()
                    .fold(Scrollable::new(&mut self.error_scroll), |v, e| {
                        v.push(Text::new(e))
                    });
                Column::new()
                    .spacing(5)
                    .push(Text::new("Finished"))
                    .push(Text::new(&format!(
                        "Scores Imported: {}",
                        summary.scores_imported
                    )))
                    .push(Text::new(&format!(
                        "Failed Imports: {}",
                        summary.fail_messages.len()
                    )))
                    .push(iced::Space::with_height(Length::Units(10)))
                    .push(Text::new("Errors:"))
                    .push(error_view)
            }
        };

        Container::new(content)
            .width(Length::Fill)
            .height(Length::Fill)
            .padding(15)
            .center_x()
            .center_y()
            .into()
    }
}
