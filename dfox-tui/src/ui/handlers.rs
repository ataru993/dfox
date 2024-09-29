use std::{
    io::{self, stdout},
    process,
};

use crossterm::{event::KeyCode, execute, terminal};
use dfox_lib::models::schema::TableSchema;
use ratatui::{
    prelude::CrosstermBackend,
    style::{Color, Style},
    widgets::{Block, Borders, List, ListItem},
    Terminal,
};

use super::{
    components::{FocusedWidget, InputField, ScreenState},
    DatabaseClientUI,
};

impl DatabaseClientUI {
    pub async fn handle_db_type_selection_input(&mut self, key: KeyCode) {
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

    pub async fn handle_input_event(&mut self, key: KeyCode) -> io::Result<()> {
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
                        let result = self.connect_to_default_db().await;
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

    pub async fn handle_database_selection_input(&mut self, key: KeyCode) -> io::Result<()> {
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
                    if let Err(err) = self.connect_to_selected_db(db_name).await {
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
        self.update_tables().await;
        Ok(())
    }

    pub async fn handle_table_view_input(
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
                            match self.describe_table(&selected_table).await {
                                Ok(table_schema) => {
                                    self.table_schemas
                                        .insert(selected_table.clone(), table_schema.clone());
                                    self.expanded_table = Some(self.selected_table);

                                    if let Err(err) =
                                        self.render_table_schema(terminal, &table_schema).await
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

    pub async fn render_table_schema(
        &mut self,
        terminal: &mut Terminal<CrosstermBackend<io::Stdout>>,
        table_schema: &TableSchema,
    ) -> io::Result<()> {
        terminal.draw(|f| {
            let size = f.area();

            // Создаем блок для таблицы
            let block = Block::default()
                .title(table_schema.table_name.clone())
                .borders(Borders::ALL);

            // Создаем список колонок
            let column_list: Vec<ListItem> = table_schema
                .columns
                .iter()
                .map(|col| {
                    let col_info = format!(
                        "{}: {} (Nullable: {}, Default: {:?})",
                        col.name, col.data_type, col.is_nullable, col.default
                    );
                    ListItem::new(col_info).style(Style::default().fg(Color::White))
                })
                .collect();

            let columns_widget = List::new(column_list).block(block);

            // Отрисовка виджета
            f.render_widget(columns_widget, size);
        })?;

        Ok(())
    }

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
