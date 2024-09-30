use std::{
    io::{self, stdout},
    process,
};

use crossterm::{event::KeyCode, execute, terminal};
use ratatui::{prelude::CrosstermBackend, Terminal};

use crate::db::PostgresUI;

use super::{
    components::{FocusedWidget, InputField, ScreenState},
    DatabaseClientUI, UIHandler, UIRenderer,
};

impl UIHandler for DatabaseClientUI {
    async fn handle_db_type_selection_input(&mut self, key: KeyCode) {
        match key {
            KeyCode::Up => {
                if self.selected_db_type > 0 {
                    self.selected_db_type -= 1;
                }
            }
            KeyCode::Down => {
                if self.selected_db_type < 2 {
                    self.selected_db_type += 1;
                }
            }
            KeyCode::Enter => self.current_screen = ScreenState::ConnectionInput,
            KeyCode::Char('q') => {
                terminal::disable_raw_mode().unwrap();
                execute!(stdout(), terminal::LeaveAlternateScreen).unwrap();
                process::exit(0);
            }
            _ => {}
        }
    }

    async fn handle_input_event(&mut self, key: KeyCode) -> io::Result<()> {
        match key {
            KeyCode::Esc => {
                self.current_screen = ScreenState::DbTypeSelection;
            }
            _ => match self.connection_input.current_field {
                InputField::Username => match key {
                    KeyCode::Char(c) => self.connection_input.username.push(c),
                    KeyCode::Backspace => {
                        self.connection_input.username.pop();
                    }
                    KeyCode::Enter => {
                        self.connection_input.current_field = InputField::Password;
                    }
                    _ => {}
                },
                InputField::Password => match key {
                    KeyCode::Char(c) => self.connection_input.password.push(c),
                    KeyCode::Backspace => {
                        self.connection_input.password.pop();
                    }
                    KeyCode::Enter => {
                        self.connection_input.current_field = InputField::Hostname;
                    }
                    _ => {}
                },
                InputField::Hostname => match key {
                    KeyCode::Char(c) => self.connection_input.hostname.push(c),
                    KeyCode::Backspace => {
                        self.connection_input.hostname.pop();
                    }
                    KeyCode::Enter => {
                        let result = PostgresUI::connect_to_default_db(self).await;
                        if result.is_ok() {
                            self.current_screen = ScreenState::DatabaseSelection;
                        }
                    }
                    _ => {}
                },
            },
        }
        Ok(())
    }

    async fn handle_database_selection_input(&mut self, key: KeyCode) -> io::Result<()> {
        match key {
            KeyCode::Up => {
                if self.selected_db_type > 0 {
                    self.selected_db_type -= 1;
                }
            }
            KeyCode::Down => {
                if !self.databases.is_empty() && self.selected_db_type < self.databases.len() - 1 {
                    self.selected_db_type += 1;
                }
            }
            KeyCode::Enter => {
                let cloned = self.databases.clone();
                if let Some(db_name) = cloned.get(self.selected_db_type) {
                    if let Err(err) = PostgresUI::connect_to_selected_db(self, db_name).await {
                        eprintln!("Error connecting to database: {}", err);
                    } else {
                        self.current_screen = ScreenState::TableView;
                    }
                }
            }
            KeyCode::Char('q') => {
                terminal::disable_raw_mode().unwrap();
                execute!(stdout(), terminal::LeaveAlternateScreen).unwrap();
                process::exit(0);
            }
            _ => {}
        }
        PostgresUI::update_tables(self).await;
        Ok(())
    }

    async fn handle_table_view_input(
        &mut self,
        key: KeyCode,
        terminal: &mut Terminal<CrosstermBackend<io::Stdout>>,
    ) {
        match key {
            KeyCode::Tab => self.cycle_focus(),
            KeyCode::Up => {
                if let FocusedWidget::TablesList = self.current_focus {
                    self.move_selection_up();
                }
            }
            KeyCode::Down => {
                if let FocusedWidget::TablesList = self.current_focus {
                    self.move_selection_down();
                }
            }
            KeyCode::Enter => {
                if let FocusedWidget::TablesList = self.current_focus {
                    if self.tables.is_empty() {
                        println!("No tables available.");
                        return;
                    }

                    if self.selected_table < self.tables.len() {
                        let selected_table = self.tables[self.selected_table].clone();

                        if Some(self.selected_table) == self.expanded_table {
                            self.expanded_table = None;
                        } else {
                            match PostgresUI::describe_table(self, &selected_table).await {
                                Ok(table_schema) => {
                                    self.table_schemas
                                        .insert(selected_table.clone(), table_schema.clone());
                                    self.expanded_table = Some(self.selected_table);

                                    if let Err(err) = UIRenderer::render_table_schema(
                                        self,
                                        terminal,
                                        &table_schema,
                                    )
                                    .await
                                    {
                                        eprintln!("Error rendering table schema: {}", err);
                                    }
                                }
                                Err(err) => {
                                    eprintln!("Error describing table: {}", err);
                                }
                            }
                        }
                    } else {
                        eprintln!("Selected table index out of bounds.");
                    }
                }
            }
            _ => {}
        }
    }

    async fn handle_sql_editor_input(
        &mut self,
        key: KeyCode,
        terminal: &mut Terminal<CrosstermBackend<io::Stdout>>,
    ) {
        match key {
            KeyCode::Tab => self.cycle_focus(),
            KeyCode::Enter if self.sql_editor_content.is_empty() => {
                return;
            }
            KeyCode::Enter => {
                let sql_content = self.sql_editor_content.clone();

                if let Ok(result) = PostgresUI::execute_sql_query(self, &sql_content).await {
                    self.sql_query_result = result;
                } else {
                    eprintln!("Error executing query");
                }

                self.sql_editor_content.clear();
            }
            KeyCode::Char(c) => {
                self.sql_editor_content.push(c);
            }
            KeyCode::Backspace => {
                self.sql_editor_content.pop();
            }
            _ => {}
        }

        if let Err(err) = UIRenderer::render_table_view_screen(self, terminal).await {
            eprintln!("Error rendering UI: {}", err);
        }
    }
}

impl DatabaseClientUI {
    pub fn cycle_focus(&mut self) {
        self.current_focus = match self.current_focus {
            FocusedWidget::TablesList => FocusedWidget::SqlEditor,
            FocusedWidget::SqlEditor => FocusedWidget::QueryResult,
            FocusedWidget::QueryResult => FocusedWidget::TablesList,
        };
    }

    pub fn move_selection_up(&mut self) {
        if self.selected_table > 0 {
            self.selected_table -= 1;
        }
    }

    pub fn move_selection_down(&mut self) {
        if self.selected_table < self.databases.len().saturating_sub(1) {
            self.selected_table += 1;
        }
    }
}