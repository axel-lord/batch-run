use ::std::{
    io::{Write, pipe},
    num::NonZero,
    process::ExitCode,
};

use ::clap::Parser;
use ::derive_more::Display;
use ::iced::{
    Element, Length, Task, Theme, application,
    keyboard::{Key, Modifiers},
    widget::{
        self, Column, Row,
        text_editor::{self, Action, Binding, Edit},
    },
};
use ::iced_highlighter::Highlighter;
use ::serde::Serialize;
use ::strum::VariantArray;
use ::tokio::process::Command;

#[derive(Debug, Clone, Serialize)]
struct InputData<'i> {
    line: &'i str,
    idx: usize,
    len: usize,
    reversed: String,
}

#[derive(Debug)]
struct App {
    cli: Cli,
    content: text_editor::Content,
    settings: ::iced_highlighter::Settings,
    language: Language,
}

#[derive(Debug, Parser)]
struct Cli {
    /// Amount of spaces to insert on tab.
    #[arg(long, default_value_t = 4)]
    tabwidth: u8,

    /// Arguments to pass as stdin to batch script.
    args: Vec<String>,
}

#[derive(Debug, Clone)]
enum Msg {
    ContentAction(Action),
    Language(Language),
    InsertTab,
    Run,
}

#[derive(Debug, Clone, Copy, Display, PartialEq, Eq, VariantArray)]
enum Language {
    #[display("zsh")]
    Zsh,
    #[display("bash")]
    Bash,
    #[display("python")]
    Python,
    #[display("sh")]
    Sh,
}

impl App {
    pub fn update(&mut self, msg: Msg) -> Task<Msg> {
        match msg {
            Msg::ContentAction(action) => {
                self.content.perform(action);
                Task::none()
            }
            Msg::InsertTab => {
                if let Some(w) = NonZero::new(self.cli.tabwidth) {
                    for _ in 0..w.get() {
                        self.content.perform(Action::Edit(Edit::Insert(' ')));
                    }
                } else {
                    self.content.perform(Action::Edit(Edit::Insert('\t')));
                }
                Task::none()
            }
            Msg::Run => {
                let batch = self.content.text();
                let lang = self.language;
                let args = self.cli.args.clone();
                Task::future(async move {
                    let (r, mut w) = match pipe() {
                        Ok(pipe) => pipe,
                        Err(err) => {
                            eprintln!("could not create pipe\n{err}");
                            return;
                        }
                    };
                    let result = match lang {
                        Language::Zsh => Command::new("/usr/bin/zsh")
                            .args(["--emulate", "zsh", "-c"])
                            .arg(batch)
                            .arg("batch-script-zsh")
                            .stdin(r)
                            .spawn(),
                        Language::Bash => Command::new("/usr/bin/bash")
                            .arg("-c")
                            .arg(batch)
                            .arg("batch-script-bash")
                            .stdin(r)
                            .spawn(),
                        Language::Python => Command::new("/usr/bin/python")
                            .arg("-c")
                            .arg(batch)
                            .stdin(r)
                            .spawn(),
                        Language::Sh => Command::new("/usr/bin/sh")
                            .arg("-c")
                            .arg(batch)
                            .arg("batch-script-sh")
                            .stdin(r)
                            .spawn(),
                    };
                    let mut child = match result {
                        Ok(child) => child,
                        Err(err) => {
                            eprintln!("could not spawn child\n{err}");
                            return;
                        }
                    };

                    for (idx, arg) in args.into_iter().enumerate() {
                        let data = InputData {
                            line: &arg,
                            idx,
                            len: arg.len(),
                            reversed: arg.chars().rev().collect(),
                        };

                        let r = ::serde_json::to_writer(&mut w, &data);
                        if let Err(err) = r {
                            eprintln!("could not write data to pipe\n{err}");
                            return;
                        }
                        if let Err(err) = w.write_all(b"\n") {
                            eprintln!("could not terminate data written to pipe\n{err}");
                            return;
                        }
                    }

                    if let Err(err) = w.flush() {
                        eprintln!("could not flush pipe\n{err}");
                        return;
                    }
                    drop(w);

                    match child.wait().await {
                        Ok(status) => eprintln!("{status}"),
                        Err(err) => eprintln!("could not wait on child\n{err}"),
                    }
                })
                .then(|_| Task::none())
            }
            Msg::Language(language) => {
                self.language = language;
                self.settings.token = language.to_string();
                Task::none()
            }
        }
    }

    pub fn view(&self) -> Element<'_, Msg> {
        Column::new()
            .push(Row::new().push(widget::pick_list(
                Language::VARIANTS,
                Some(self.language),
                Msg::Language,
            )))
            .push(
                widget::text_editor(&self.content)
                    .on_action(Msg::ContentAction)
                    .height(Length::Fill)
                    .font(::iced::Font::MONOSPACE)
                    .highlight_with::<Highlighter>(self.settings.clone(), |h, _| h.to_format())
                    .key_binding(|keypress| {
                        if keypress.modifiers.is_empty()
                            && matches!(keypress.key, Key::Named(::iced::keyboard::key::Named::Tab))
                        {
                            Some(Binding::Custom(Msg::InsertTab))
                        } else if keypress.modifiers == Modifiers::CTRL
                            && keypress.key.as_ref() == Key::Character("r")
                        {
                            Some(Binding::Custom(Msg::Run))
                        } else {
                            Binding::from_key_press(keypress)
                        }
                    }),
            )
            .into()
    }
}

fn main() -> ExitCode {
    let cli = Cli::parse();
    match application("Batch Run", App::update, App::view)
        .theme(|_| Theme::SolarizedDark)
        .run_with(move || {
            (
                App {
                    cli,
                    settings: ::iced_highlighter::Settings {
                        theme: ::iced_highlighter::Theme::SolarizedDark,
                        token: "zsh".to_owned(),
                    },
                    language: Language::Zsh,
                    content: Default::default(),
                },
                Task::none(),
            )
        }) {
        Ok(_) => ExitCode::SUCCESS,
        Err(err) => {
            eprintln!("{err}");
            ExitCode::FAILURE
        }
    }
}
