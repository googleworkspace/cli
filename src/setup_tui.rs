// Copyright 2026 Google LLC
//
// Licensed under the Apache License, Version 2.0 (the "License");
// you may not use this file except in compliance with the License.
// You may obtain a copy of the License at
//
//     http://www.apache.org/licenses/LICENSE-2.0
//
// Unless required by applicable law or agreed to in writing, software
// distributed under the License is distributed on an "AS IS" BASIS,
// WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
// See the License for the specific language governing permissions and
// limitations under the License.

//! Interactive multi-select TUI for the setup flow.
//!
//! Provides a ratatui-based fullscreen multi-select picker
//! that the user can navigate with arrow keys, toggle with space,
//! select all with 'a', and confirm with Enter.

use crossterm::{
    event::{self, Event, KeyCode, KeyEventKind},
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
    ExecutableCommand,
};
use ratatui::{
    layout::{Constraint, Layout},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, List, ListItem, ListState, Paragraph},
    DefaultTerminal,
};
use std::io::stdout;

/// An item in the multi-select list.
#[derive(Clone)]
pub struct SelectItem {
    pub label: String,
    pub description: String,
    pub selected: bool,
    pub is_fixed: bool,
    pub is_template: bool,
    pub template_selects: Vec<String>,
}

/// Result of running the multi-select picker.
pub enum PickerResult {
    /// User confirmed selection.
    Confirmed(Vec<SelectItem>),
    /// User wanted to go back
    GoBack,
    /// User cancelled (q).
    Cancelled,
}

/// Result of running the input dialog.
pub enum InputResult {
    /// User confirmed input.
    Confirmed(String),
    /// User wanted to go back
    GoBack,
    /// User cancelled
    Cancelled,
}

/// Helper to wrap text to a particular max width.
pub fn wrap_text(text: &str, max_width: u16) -> Vec<String> {
    if max_width == 0 {
        return vec![text.to_string()];
    }
    let mut result = Vec::new();
    for paragraph in text.split('\n') {
        let mut current_line = String::new();
        for word in paragraph.split_whitespace() {
            if current_line.is_empty() {
                current_line.push_str(word);
            } else if current_line.chars().count() + 1 + word.chars().count() <= max_width as usize
            {
                current_line.push(' ');
                current_line.push_str(word);
            } else {
                result.push(current_line);
                current_line = word.to_string();
            }
        }
        if !current_line.is_empty() {
            result.push(current_line);
        } else if paragraph.is_empty() {
            result.push(String::new());
        }
    }
    result
}

/// State for the multi-select picker.
pub struct PickerState {
    pub items: Vec<SelectItem>,
    pub list_state: ListState,
    pub title: String,
    pub help_text: String,
    pub multiselect: bool,
}

/// State for the text input.
pub struct InputState {
    pub value: String,
    title: String,
}

impl InputState {
    pub fn new(title: &str, _help_text: &str, initial: Option<&str>) -> Self {
        Self {
            value: initial.unwrap_or("").to_string(),
            title: title.to_string(),
        }
    }

    pub fn handle_key(&mut self, code: KeyCode) -> Option<InputResult> {
        match code {
            KeyCode::Esc => Some(InputResult::Cancelled),
            KeyCode::Up | KeyCode::BackTab => Some(InputResult::GoBack),
            KeyCode::Enter => Some(InputResult::Confirmed(self.value.clone())),
            KeyCode::Backspace => {
                self.value.pop();
                None
            }
            KeyCode::Char(c) => {
                self.value.push(c);
                None
            }
            _ => None,
        }
    }
}

impl PickerState {
    pub fn new(title: &str, help_text: &str, items: Vec<SelectItem>, multiselect: bool) -> Self {
        let selected_idx = items.iter().position(|i| i.selected).unwrap_or(0);
        let mut list_state = ListState::default();
        list_state.select(Some(selected_idx));
        Self {
            items,
            list_state,
            title: title.to_string(),
            help_text: help_text.to_string(),
            multiselect,
        }
    }

    fn toggle_current(&mut self) {
        if let Some(i) = self.list_state.selected() {
            if !self.items[i].is_fixed {
                let current_label = self.items[i].label.clone();
                let current_selected = !self.items[i].selected;
                let is_template = self.items[i].is_template;
                let template_selects = self.items[i].template_selects.clone();

                self.items[i].selected = current_selected;

                if is_template {
                    // Turn off other templates
                    if current_selected {
                        for item in &mut self.items {
                            if item.is_template && item.label != current_label {
                                item.selected = false;
                            }
                        }
                        // Apply template selection to normal items
                        for item in &mut self.items {
                            if !item.is_template && !item.is_fixed {
                                item.selected = template_selects.contains(&item.label);
                            }
                        }
                    }
                } else {
                    // If a normal item is toggled, turn OFF all templates since the user is customizing
                    for item in &mut self.items {
                        if item.is_template {
                            item.selected = false;
                        }
                    }

                    // Handle readonly/superset interdependency
                    // Only deselect the counterpart when we are SELECTING an item
                    if current_selected {
                        let counterpart_to_deselect = if current_label.ends_with(".readonly") {
                            current_label.strip_suffix(".readonly").unwrap().to_string()
                        } else {
                            format!("{}.readonly", current_label)
                        };

                        self.items.iter_mut().for_each(|item| {
                            if item.label == counterpart_to_deselect && !item.is_fixed {
                                item.selected = false;
                            }
                        });
                    }
                }
            }
        }
    }

    fn toggle_all(&mut self) {
        let all_non_fixed_selected = self
            .items
            .iter()
            .filter(|i| !i.is_fixed)
            .all(|item| item.selected);
        for item in &mut self.items {
            if !item.is_fixed {
                item.selected = !all_non_fixed_selected;
            }
        }
    }

    fn next(&mut self) {
        let i = match self.list_state.selected() {
            Some(i) => (i + 1) % self.items.len(),
            None => 0,
        };
        self.list_state.select(Some(i));
    }

    fn previous(&mut self) {
        let i = match self.list_state.selected() {
            Some(i) => {
                if i == 0 {
                    self.items.len() - 1
                } else {
                    i - 1
                }
            }
            None => 0,
        };
        self.list_state.select(Some(i));
    }

    fn selected_count(&self) -> usize {
        self.items.iter().filter(|i| i.selected).count()
    }

    /// Handle a key press. Returns Some(result) if the picker should exit.
    pub fn handle_key(&mut self, code: KeyCode) -> Option<PickerResult> {
        match code {
            KeyCode::Char('q') | KeyCode::Esc => Some(PickerResult::Cancelled),
            KeyCode::Left | KeyCode::Char('h') | KeyCode::Backspace => Some(PickerResult::GoBack),
            KeyCode::Enter => {
                if !self.multiselect {
                    if let Some(idx) = self.list_state.selected() {
                        for (i, item) in self.items.iter_mut().enumerate() {
                            if !item.is_fixed {
                                item.selected = i == idx;
                            }
                        }
                    }
                }
                Some(PickerResult::Confirmed(self.items.clone()))
            }
            KeyCode::Char(' ') => {
                if self.multiselect {
                    self.toggle_current();
                } else if let Some(idx) = self.list_state.selected() {
                    for (i, item) in self.items.iter_mut().enumerate() {
                        if !item.is_fixed {
                            item.selected = i == idx;
                        }
                    }
                }
                None
            }
            KeyCode::Char('a') => {
                if self.multiselect {
                    self.toggle_all();
                }
                None
            }
            KeyCode::Up | KeyCode::Char('k') => {
                self.previous();
                if !self.multiselect {
                    if let Some(idx) = self.list_state.selected() {
                        for (i, item) in self.items.iter_mut().enumerate() {
                            if !item.is_fixed {
                                item.selected = i == idx;
                            }
                        }
                    }
                }
                None
            }
            KeyCode::Down | KeyCode::Char('j') => {
                self.next();
                if !self.multiselect {
                    if let Some(idx) = self.list_state.selected() {
                        for (i, item) in self.items.iter_mut().enumerate() {
                            if !item.is_fixed {
                                item.selected = i == idx;
                            }
                        }
                    }
                }
                None
            }
            _ => None,
        }
    }
}

/// Run an interactive multi-select picker.
///
/// Returns `PickerResult::Confirmed` with the final item states,
/// or `PickerResult::Cancelled` if the user pressed Esc/q.
pub fn run_picker(
    title: &str,
    help_text: &str,
    items: Vec<SelectItem>,
    multiselect: bool,
) -> std::io::Result<PickerResult> {
    // Enter TUI mode
    stdout().execute(EnterAlternateScreen)?;
    enable_raw_mode()?;

    let mut terminal = ratatui::init();
    let mut state = PickerState::new(title, help_text, items, multiselect);

    let result = run_picker_loop(&mut terminal, &mut state);

    // Restore terminal
    ratatui::restore();
    disable_raw_mode()?;
    stdout().execute(LeaveAlternateScreen)?;

    result
}

fn run_picker_loop(
    terminal: &mut DefaultTerminal,
    state: &mut PickerState,
) -> std::io::Result<PickerResult> {
    loop {
        terminal.draw(|frame| {
            let area = frame.area();

            // Layout: title (2) | list (stretch) | help bar (3)
            let chunks = Layout::vertical([
                Constraint::Length(3),
                Constraint::Min(5),
                Constraint::Length(3),
            ])
            .split(area);

            // Title
            let mut title_spans = vec![
                Span::styled(&state.title, Style::default().fg(Color::Cyan).bold()),
                Span::raw("  "),
            ];
            if state.multiselect {
                title_spans.push(Span::styled(
                    format!("{}/{} selected", state.selected_count(), state.items.len()),
                    Style::default().fg(Color::DarkGray),
                ));
            }
            let title = Paragraph::new(Line::from(title_spans))
                .block(Block::default().borders(Borders::BOTTOM));
            frame.render_widget(title, chunks[0]);

            // List items
            let items: Vec<ListItem> = state
                .items
                .iter()
                .map(|item| {
                    let checkbox = if state.multiselect {
                        if item.selected {
                            "[x] "
                        } else {
                            "[ ] "
                        }
                    } else if item.selected {
                        "◉ "
                    } else {
                        "○ "
                    };
                    let checkbox_style = if item.is_fixed {
                        Style::default().fg(Color::DarkGray)
                    } else if item.selected {
                        Style::default().fg(Color::Green)
                    } else {
                        Style::default().fg(Color::DarkGray)
                    };
                    let label_style = if item.is_fixed {
                        Style::default().fg(Color::DarkGray)
                    } else if item.selected {
                        Style::default().fg(Color::White)
                    } else {
                        Style::default().fg(Color::Gray)
                    };
                    let desc_style = Style::default().fg(Color::DarkGray);

                    ListItem::new(Line::from(vec![
                        Span::styled(checkbox, checkbox_style),
                        Span::styled(&item.label, label_style),
                        Span::raw("  "),
                        Span::styled(&item.description, desc_style),
                    ]))
                })
                .collect();

            let list = List::new(items)
                .highlight_style(
                    Style::default()
                        .bg(Color::DarkGray)
                        .add_modifier(Modifier::BOLD),
                )
                .highlight_symbol("▸ ");

            frame.render_stateful_widget(list, chunks[1], &mut state.list_state);

            // Help bar
            let mut help_spans = vec![
                Span::styled(" ↑↓ ", Style::default().fg(Color::Yellow)),
                Span::raw("Navigate  "),
            ];
            if state.multiselect {
                help_spans.push(Span::styled(" Space ", Style::default().fg(Color::Yellow)));
                help_spans.push(Span::raw("Toggle  "));
                help_spans.push(Span::styled(" a ", Style::default().fg(Color::Yellow)));
                help_spans.push(Span::raw("All  "));
            }
            help_spans.push(Span::styled(" Enter ", Style::default().fg(Color::Green)));
            help_spans.push(Span::raw("Confirm  "));
            help_spans.push(Span::styled(" Esc ", Style::default().fg(Color::Red)));
            help_spans.push(Span::raw("Cancel"));

            let help = Paragraph::new(Line::from(help_spans))
                .block(Block::default().borders(Borders::TOP));
            frame.render_widget(help, chunks[2]);

            // Additional help text at the bottom of the list area if provided
            if !state.help_text.is_empty() {
                // Rendered as part of the title block already
            }
        })?;

        // Handle input
        if let Event::Key(key) = event::read()? {
            if key.kind != KeyEventKind::Press {
                continue;
            }
            if let Some(result) = state.handle_key(key.code) {
                return Ok(result);
            }
        }
    }
}

// ── Setup Wizard (unified TUI session) ──────────────────────────

/// Status of a single setup step.
#[derive(Clone)]
pub enum StepStatus {
    Pending,
    InProgress(String),
    Done(String),
    Failed(String),
}

/// A step in the setup wizard.
#[derive(Clone)]
struct WizardStep {
    label: String,
    status: StepStatus,
}

/// Unified TUI session for the entire setup flow.
/// Renders step progress + inline picker in one ratatui session.
pub struct SetupWizard {
    steps: Vec<WizardStep>,
    terminal: DefaultTerminal,
    message: Option<String>,
}

impl SetupWizard {
    /// Enter ratatui and start the wizard with the given step labels.
    pub fn start(step_labels: &[&str]) -> std::io::Result<Self> {
        stdout().execute(EnterAlternateScreen)?;
        enable_raw_mode()?;
        let terminal = ratatui::init();
        let steps = step_labels
            .iter()
            .map(|label| WizardStep {
                label: label.to_string(),
                status: StepStatus::Pending,
            })
            .collect();
        let mut wizard = Self {
            steps,
            terminal,
            message: None,
        };
        wizard.draw_progress()?;
        Ok(wizard)
    }

    /// Update a step's status and redraw.
    pub fn update_step(&mut self, idx: usize, status: StepStatus) -> std::io::Result<()> {
        if idx < self.steps.len() {
            self.steps[idx].status = status;
        }
        self.message = None;
        self.draw_progress()
    }

    /// Show a message below the steps (e.g. "Loading projects...").
    pub fn show_message(&mut self, msg: &str) -> std::io::Result<()> {
        self.message = Some(msg.to_string());
        self.draw_progress()
    }

    /// Temporarily exit ratatui (e.g. for browser-based auth).
    pub fn suspend(&mut self) -> std::io::Result<()> {
        ratatui::restore();
        disable_raw_mode()?;
        stdout().execute(LeaveAlternateScreen)?;
        Ok(())
    }

    /// Re-enter ratatui after a suspend.
    pub fn resume(&mut self) -> std::io::Result<()> {
        stdout().execute(EnterAlternateScreen)?;
        enable_raw_mode()?;
        self.terminal = ratatui::init();
        self.draw_progress()
    }

    /// Show an inline picker and wait for user selection.
    pub fn show_picker(
        &mut self,
        title: &str,
        help_text: &str,
        items: Vec<SelectItem>,
        multiselect: bool,
    ) -> std::io::Result<PickerResult> {
        let mut picker = PickerState::new(title, help_text, items, multiselect);
        loop {
            let steps_snapshot = self.steps.clone();
            let msg = self.message.clone();
            self.terminal.draw(|frame| {
                let area = frame.area();
                let mut step_height = steps_snapshot.len() as u16 + 2;
                let msg_lines = if let Some(m) = &msg {
                    let wrapped = crate::setup_tui::wrap_text(m, area.width.saturating_sub(4));
                    step_height += wrapped.len() as u16 + 1;
                    wrapped
                } else {
                    vec![]
                };
                let chunks = Layout::vertical([
                    Constraint::Length(step_height),
                    Constraint::Min(5),
                    Constraint::Length(3),
                ])
                .split(area);

                Self::render_steps(frame, chunks[0], &steps_snapshot, &msg_lines);
                Self::render_picker(frame, chunks[1], &mut picker);

                let mut help_spans = vec![
                    Span::styled(" ↑↓ ", Style::default().fg(Color::Yellow)),
                    Span::raw("Navigate  "),
                ];
                if picker.multiselect {
                    help_spans.push(Span::styled(" Space ", Style::default().fg(Color::Yellow)));
                    help_spans.push(Span::raw("Toggle  "));
                    help_spans.push(Span::styled(" a ", Style::default().fg(Color::Yellow)));
                    help_spans.push(Span::raw("All  "));
                }
                help_spans.push(Span::styled(" Enter ", Style::default().fg(Color::Green)));
                help_spans.push(Span::raw("Confirm  "));
                help_spans.push(Span::styled(" Esc ", Style::default().fg(Color::Red)));
                help_spans.push(Span::raw("Cancel"));

                let help = Paragraph::new(Line::from(help_spans))
                    .block(Block::default().borders(Borders::TOP));
                frame.render_widget(help, chunks[2]);
            })?;

            if let Event::Key(key) = event::read()? {
                if key.kind != KeyEventKind::Press {
                    continue;
                }
                if let Some(result) = picker.handle_key(key.code) {
                    return Ok(result);
                }
            }
        }
    }

    /// Show an inline text input and wait for user submission.
    pub fn show_input(
        &mut self,
        title: &str,
        help_text: &str,
        initial: Option<&str>,
    ) -> std::io::Result<InputResult> {
        let mut input = InputState::new(title, help_text, initial);
        loop {
            let steps_snapshot = self.steps.clone();
            let msg = self.message.clone();
            self.terminal.draw(|frame| {
                let area = frame.area();
                let mut step_height = steps_snapshot.len() as u16 + 2;
                let msg_lines = if let Some(m) = &msg {
                    let wrapped = crate::setup_tui::wrap_text(m, area.width.saturating_sub(4));
                    step_height += wrapped.len() as u16 + 1;
                    wrapped
                } else {
                    vec![]
                };
                let chunks = Layout::vertical([
                    Constraint::Length(step_height),
                    Constraint::Min(5),
                    Constraint::Length(3),
                ])
                .split(area);

                Self::render_steps(frame, chunks[0], &steps_snapshot, &msg_lines);
                Self::render_input(frame, chunks[1], &mut input);

                let help = Paragraph::new(Line::from(vec![
                    Span::styled(" Type ", Style::default().fg(Color::Yellow)),
                    Span::raw("Input text  "),
                    Span::styled(" Enter ", Style::default().fg(Color::Green)),
                    Span::raw("Confirm  "),
                    Span::styled(" Esc ", Style::default().fg(Color::Red)),
                    Span::raw("Cancel"),
                ]))
                .block(Block::default().borders(Borders::TOP));
                frame.render_widget(help, chunks[2]);
            })?;

            if let Event::Key(key) = event::read()? {
                if key.kind != KeyEventKind::Press {
                    continue;
                }
                if let Some(result) = input.handle_key(key.code) {
                    return Ok(result);
                }
            }
        }
    }

    /// Exit ratatui cleanly.
    pub fn finish(self) -> std::io::Result<()> {
        ratatui::restore();
        disable_raw_mode()?;
        stdout().execute(LeaveAlternateScreen)?;
        Ok(())
    }

    fn draw_progress(&mut self) -> std::io::Result<()> {
        let steps_snapshot = self.steps.clone();
        let msg = self.message.clone();
        self.terminal.draw(|frame| {
            let area = frame.area();
            let mut step_height = steps_snapshot.len() as u16 + 2;
            let msg_lines = if let Some(m) = &msg {
                let wrapped = crate::setup_tui::wrap_text(m, area.width.saturating_sub(4));
                step_height += wrapped.len() as u16 + 1;
                wrapped
            } else {
                vec![]
            };
            let chunks =
                Layout::vertical([Constraint::Length(step_height), Constraint::Min(0)]).split(area);
            Self::render_steps(frame, chunks[0], &steps_snapshot, &msg_lines);
        })?;
        Ok(())
    }

    fn render_steps(
        frame: &mut ratatui::Frame,
        area: ratatui::layout::Rect,
        steps: &[WizardStep],
        msg_lines: &[String],
    ) {
        let mut items: Vec<ListItem> = steps
            .iter()
            .enumerate()
            .map(|(i, step)| {
                let num = format!("Step {}/{}:", i + 1, steps.len());
                match &step.status {
                    StepStatus::Pending => ListItem::new(Line::from(vec![
                        Span::styled("  ○ ", Style::default().fg(Color::DarkGray)),
                        Span::styled(num, Style::default().fg(Color::DarkGray)),
                        Span::raw(" "),
                        Span::styled(&step.label, Style::default().fg(Color::DarkGray)),
                    ])),
                    StepStatus::InProgress(detail) => {
                        let mut spans = vec![
                            Span::styled("  ▸ ", Style::default().fg(Color::Yellow).bold()),
                            Span::styled(num, Style::default().fg(Color::Yellow)),
                            Span::raw(" "),
                            Span::styled(&step.label, Style::default().fg(Color::White).bold()),
                        ];
                        if !detail.is_empty() {
                            spans.push(Span::styled(
                                format!(" — {detail}"),
                                Style::default().fg(Color::DarkGray),
                            ));
                        }
                        ListItem::new(Line::from(spans))
                    }
                    StepStatus::Done(detail) => {
                        let mut spans = vec![
                            Span::styled("  ✓ ", Style::default().fg(Color::Green)),
                            Span::styled(num, Style::default().fg(Color::Green)),
                            Span::raw(" "),
                            Span::styled(&step.label, Style::default().fg(Color::Green)),
                        ];
                        if !detail.is_empty() {
                            spans.push(Span::styled(
                                format!(" — {detail}"),
                                Style::default().fg(Color::DarkGray),
                            ));
                        }
                        ListItem::new(Line::from(spans))
                    }
                    StepStatus::Failed(detail) => ListItem::new(Line::from(vec![
                        Span::styled("  ✗ ", Style::default().fg(Color::Red)),
                        Span::styled(num, Style::default().fg(Color::Red)),
                        Span::raw(" "),
                        Span::styled(&step.label, Style::default().fg(Color::Red)),
                        Span::styled(format!(" — {detail}"), Style::default().fg(Color::Red)),
                    ])),
                }
            })
            .collect();

        if !msg_lines.is_empty() {
            items.push(ListItem::new(Line::from(vec![Span::raw("")])));
            for line in msg_lines {
                items.push(ListItem::new(Line::from(vec![Span::styled(
                    format!("  {line}"),
                    Style::default().fg(Color::Cyan),
                )])));
            }
        }

        let list = List::new(items).block(
            Block::default()
                .title(Span::styled(
                    " gws auth setup ",
                    Style::default().fg(Color::Cyan).bold(),
                ))
                .borders(Borders::ALL)
                .border_style(Style::default().fg(Color::DarkGray)),
        );
        frame.render_widget(list, area);
    }

    fn render_picker(
        frame: &mut ratatui::Frame,
        area: ratatui::layout::Rect,
        picker: &mut PickerState,
    ) {
        let items: Vec<ListItem> = picker
            .items
            .iter()
            .map(|item| {
                let checkbox = if item.selected { "◉ " } else { "○ " };
                let checkbox_style = if item.selected {
                    Style::default().fg(Color::Green)
                } else {
                    Style::default().fg(Color::DarkGray)
                };
                let label_style = if item.selected {
                    Style::default().fg(Color::White)
                } else {
                    Style::default().fg(Color::Gray)
                };
                ListItem::new(Line::from(vec![
                    Span::styled(checkbox, checkbox_style),
                    Span::styled(&item.label, label_style),
                    Span::raw("  "),
                    Span::styled(&item.description, Style::default().fg(Color::DarkGray)),
                ]))
            })
            .collect();

        let title_line = Line::from(vec![
            Span::styled(&picker.title, Style::default().fg(Color::Cyan).bold()),
            Span::raw("  "),
            Span::styled(
                format!(
                    "{}/{} selected",
                    picker.selected_count(),
                    picker.items.len()
                ),
                Style::default().fg(Color::DarkGray),
            ),
        ]);

        let list = List::new(items)
            .block(
                Block::default()
                    .title(title_line)
                    .borders(Borders::ALL)
                    .border_style(Style::default().fg(Color::DarkGray)),
            )
            .highlight_style(
                Style::default()
                    .bg(Color::DarkGray)
                    .add_modifier(Modifier::BOLD),
            )
            .highlight_symbol("▸ ");

        frame.render_stateful_widget(list, area, &mut picker.list_state);
    }

    fn render_input(
        frame: &mut ratatui::Frame,
        area: ratatui::layout::Rect,
        input: &mut InputState,
    ) {
        let title_line = Line::from(vec![Span::styled(
            &input.title,
            Style::default().fg(Color::Cyan).bold(),
        )]);

        let p = Paragraph::new(Line::from(vec![
            Span::raw("> "),
            Span::styled(&input.value, Style::default().fg(Color::White)),
            Span::styled(
                "█",
                Style::default()
                    .fg(Color::Gray)
                    .add_modifier(Modifier::RAPID_BLINK),
            ),
        ]))
        .block(
            Block::default()
                .title(title_line)
                .borders(Borders::ALL)
                .border_style(Style::default().fg(Color::DarkGray)),
        );

        frame.render_widget(p, area);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_items(labels: &[&str]) -> Vec<SelectItem> {
        labels
            .iter()
            .map(|&s| SelectItem {
                label: s.to_string(),
                description: format!("Desc {s}"),
                selected: false,
                is_fixed: false,
                is_template: false,
                template_selects: vec![],
            })
            .collect()
    }

    /// Helper: feed a sequence of key presses into a PickerState,
    /// returning the final result (Confirmed/Cancelled).
    fn run_keys(state: &mut PickerState, keys: &[KeyCode]) -> Option<PickerResult> {
        for key in keys {
            if let Some(result) = state.handle_key(*key) {
                return Some(result);
            }
        }
        None
    }

    // ── PickerState unit tests ──────────────────────────────────

    #[test]
    fn test_picker_state_toggle() {
        let mut items = make_items(&["A", "B"]);
        items[1].selected = true;
        let mut state = PickerState::new("Test", "", items, true);
        state.list_state.select(Some(0)); // cursor starts at preselected (1), move to 0 for test
        assert_eq!(state.selected_count(), 1);

        state.toggle_current(); // toggle A -> selected
        assert_eq!(state.selected_count(), 2);

        state.next();
        state.toggle_current(); // toggle B -> unselected
        assert_eq!(state.selected_count(), 1);
    }

    #[test]
    fn test_picker_state_toggle_all() {
        let items = make_items(&["A", "B"]);
        let mut state = PickerState::new("Test", "", items, true);

        state.toggle_all();
        assert!(state.items.iter().all(|i| i.selected));

        state.toggle_all();
        assert!(state.items.iter().all(|i| !i.selected));
    }

    #[test]
    fn test_picker_state_navigation() {
        let items = make_items(&["A", "B", "C"]);
        let mut state = PickerState::new("Test", "", items, true);

        assert_eq!(state.list_state.selected(), Some(0));
        state.next();
        assert_eq!(state.list_state.selected(), Some(1));
        state.next();
        assert_eq!(state.list_state.selected(), Some(2));
        state.next(); // wraps
        assert_eq!(state.list_state.selected(), Some(0));
        state.previous(); // wraps back
        assert_eq!(state.list_state.selected(), Some(2));
    }

    // ── handle_key / key-sequence tests ─────────────────────────

    #[test]
    fn test_enter_confirms_with_current_state() {
        let items = make_items(&["A", "B"]);
        let mut state = PickerState::new("Test", "", items, true);
        let result = state.handle_key(KeyCode::Enter);
        match result {
            Some(PickerResult::Confirmed(items)) => {
                assert_eq!(items.len(), 2);
                assert!(items.iter().all(|i| !i.selected));
            }
            _ => panic!("Expected Confirmed"),
        }
    }

    #[test]
    fn test_esc_cancels() {
        let items = make_items(&["A"]);
        let mut state = PickerState::new("Test", "", items, true);
        let result = state.handle_key(KeyCode::Esc);
        assert!(matches!(result, Some(PickerResult::Cancelled)));
    }

    #[test]
    fn test_q_cancels() {
        let items = make_items(&["A"]);
        let mut state = PickerState::new("Test", "", items, true);
        let result = state.handle_key(KeyCode::Char('q'));
        assert!(matches!(result, Some(PickerResult::Cancelled)));
    }

    #[test]
    fn test_space_toggle_then_enter() {
        let items = make_items(&["Gmail", "Drive", "Calendar"]);
        let mut state = PickerState::new("APIs", "", items, true);

        // Toggle first item, move down, toggle second, confirm
        let result = run_keys(
            &mut state,
            &[
                KeyCode::Char(' '), // select Gmail
                KeyCode::Down,      // move to Drive
                KeyCode::Char(' '), // select Drive
                KeyCode::Enter,     // confirm
            ],
        );
        match result {
            Some(PickerResult::Confirmed(items)) => {
                assert!(items[0].selected, "Gmail should be selected");
                assert!(items[1].selected, "Drive should be selected");
                assert!(!items[2].selected, "Calendar should not be selected");
            }
            _ => panic!("Expected Confirmed"),
        }
    }

    #[test]
    fn test_select_all_then_deselect_one() {
        let items = make_items(&["A", "B", "C"]);
        let mut state = PickerState::new("Test", "", items, true);

        let result = run_keys(
            &mut state,
            &[
                KeyCode::Char('a'), // select all
                KeyCode::Down,      // move to B
                KeyCode::Char(' '), // deselect B
                KeyCode::Enter,
            ],
        );
        match result {
            Some(PickerResult::Confirmed(items)) => {
                assert!(items[0].selected);
                assert!(!items[1].selected);
                assert!(items[2].selected);
            }
            _ => panic!("Expected Confirmed"),
        }
    }

    #[test]
    fn test_vim_navigation_j_k() {
        let items = make_items(&["A", "B", "C"]);
        let mut state = PickerState::new("Test", "", items, true);

        // j = down, k = up
        let result = run_keys(
            &mut state,
            &[
                KeyCode::Char('j'), // -> B
                KeyCode::Char('j'), // -> C
                KeyCode::Char(' '), // select C
                KeyCode::Char('k'), // -> B
                KeyCode::Char(' '), // select B
                KeyCode::Enter,
            ],
        );
        match result {
            Some(PickerResult::Confirmed(items)) => {
                assert!(!items[0].selected, "A not selected");
                assert!(items[1].selected, "B selected");
                assert!(items[2].selected, "C selected");
            }
            _ => panic!("Expected Confirmed"),
        }
    }

    #[test]
    fn test_wrap_around_navigation() {
        let items = make_items(&["A", "B"]);
        let mut state = PickerState::new("Test", "", items, true);

        // From A (0), go up -> wraps to B (1)
        let result = run_keys(
            &mut state,
            &[
                KeyCode::Up,        // wrap to B
                KeyCode::Char(' '), // select B
                KeyCode::Enter,
            ],
        );
        match result {
            Some(PickerResult::Confirmed(items)) => {
                assert!(!items[0].selected);
                assert!(items[1].selected);
            }
            _ => panic!("Expected Confirmed"),
        }
    }

    #[test]
    fn test_unknown_key_ignored() {
        let items = make_items(&["A"]);
        let mut state = PickerState::new("Test", "", items, true);
        // Random keys should not exit
        assert!(state.handle_key(KeyCode::Char('x')).is_none());
        assert!(state.handle_key(KeyCode::Char('z')).is_none());
        assert!(state.handle_key(KeyCode::Tab).is_none());
    }

    #[test]
    fn test_double_toggle_returns_to_original() {
        let items = make_items(&["A"]);
        let mut state = PickerState::new("Test", "", items, true);

        let result = run_keys(
            &mut state,
            &[
                KeyCode::Char(' '), // select A
                KeyCode::Char(' '), // deselect A
                KeyCode::Enter,
            ],
        );
        match result {
            Some(PickerResult::Confirmed(items)) => {
                assert!(!items[0].selected, "double toggle => back to unselected");
            }
            _ => panic!("Expected Confirmed"),
        }
    }

    #[test]
    fn test_toggle_all_twice_resets() {
        let items = make_items(&["A", "B", "C"]);
        let mut state = PickerState::new("Test", "", items, true);

        let result = run_keys(
            &mut state,
            &[
                KeyCode::Char('a'), // all on
                KeyCode::Char('a'), // all off
                KeyCode::Enter,
            ],
        );
        match result {
            Some(PickerResult::Confirmed(items)) => {
                assert!(items.iter().all(|i| !i.selected));
            }
            _ => panic!("Expected Confirmed"),
        }
    }

    #[test]
    fn test_preselected_items_preserved() {
        let mut items = make_items(&["A", "B", "C"]);
        items[1].selected = true; // B pre-selected
        let mut state = PickerState::new("Test", "", items, true);

        // Just confirm without changing anything
        let result = state.handle_key(KeyCode::Enter);
        match result {
            Some(PickerResult::Confirmed(items)) => {
                assert!(!items[0].selected);
                assert!(items[1].selected, "pre-selected B preserved");
                assert!(!items[2].selected);
            }
            _ => panic!("Expected Confirmed"),
        }
    }

    // ── InputState tests ───────────────────────────────────────

    #[test]
    fn test_input_state_new_empty() {
        let state = InputState::new("Title", "Help", None);
        assert_eq!(state.value, "");
    }

    #[test]
    fn test_input_state_new_with_initial() {
        let state = InputState::new("Title", "Help", Some("initial"));
        assert_eq!(state.value, "initial");
    }

    #[test]
    fn test_input_state_typing() {
        let mut state = InputState::new("Title", "Help", None);
        assert!(state.handle_key(KeyCode::Char('h')).is_none());
        assert!(state.handle_key(KeyCode::Char('i')).is_none());
        assert_eq!(state.value, "hi");
    }

    #[test]
    fn test_input_state_backspace() {
        let mut state = InputState::new("Title", "Help", Some("abc"));
        assert!(state.handle_key(KeyCode::Backspace).is_none());
        assert_eq!(state.value, "ab");
        assert!(state.handle_key(KeyCode::Backspace).is_none());
        assert_eq!(state.value, "a");
    }

    #[test]
    fn test_input_state_backspace_empty() {
        let mut state = InputState::new("Title", "Help", None);
        // Backspace on empty string should be a no-op
        assert!(state.handle_key(KeyCode::Backspace).is_none());
        assert_eq!(state.value, "");
    }

    #[test]
    fn test_input_state_enter_confirms() {
        let mut state = InputState::new("Title", "Help", Some("test_value"));
        let result = state.handle_key(KeyCode::Enter);
        match result {
            Some(InputResult::Confirmed(v)) => assert_eq!(v, "test_value"),
            _ => panic!("Expected Confirmed"),
        }
    }

    #[test]
    fn test_input_state_esc_cancels() {
        let mut state = InputState::new("Title", "Help", None);
        let result = state.handle_key(KeyCode::Esc);
        assert!(matches!(result, Some(InputResult::Cancelled)));
    }

    #[test]
    fn test_input_state_up_goes_back() {
        let mut state = InputState::new("Title", "Help", None);
        let result = state.handle_key(KeyCode::Up);
        assert!(matches!(result, Some(InputResult::GoBack)));
    }

    #[test]
    fn test_input_state_backtab_goes_back() {
        let mut state = InputState::new("Title", "Help", None);
        let result = state.handle_key(KeyCode::BackTab);
        assert!(matches!(result, Some(InputResult::GoBack)));
    }

    #[test]
    fn test_input_state_unknown_key_ignored() {
        let mut state = InputState::new("Title", "Help", None);
        assert!(state.handle_key(KeyCode::Down).is_none());
        assert!(state.handle_key(KeyCode::Tab).is_none());
        assert!(state.handle_key(KeyCode::Left).is_none());
    }

    #[test]
    fn test_input_state_type_backspace_confirm() {
        let mut state = InputState::new("Title", "Help", None);
        state.handle_key(KeyCode::Char('a'));
        state.handle_key(KeyCode::Char('b'));
        state.handle_key(KeyCode::Char('c'));
        state.handle_key(KeyCode::Backspace); // remove 'c'
        state.handle_key(KeyCode::Char('d'));
        let result = state.handle_key(KeyCode::Enter);
        match result {
            Some(InputResult::Confirmed(v)) => assert_eq!(v, "abd"),
            _ => panic!("Expected Confirmed"),
        }
    }

    // ── GoBack key tests ───────────────────────────────────────

    #[test]
    fn test_backspace_goes_back() {
        let items = make_items(&["A"]);
        let mut state = PickerState::new("Test", "", items, true);
        let result = state.handle_key(KeyCode::Backspace);
        assert!(matches!(result, Some(PickerResult::GoBack)));
    }

    #[test]
    fn test_left_goes_back() {
        let items = make_items(&["A"]);
        let mut state = PickerState::new("Test", "", items, true);
        let result = state.handle_key(KeyCode::Left);
        assert!(matches!(result, Some(PickerResult::GoBack)));
    }

    #[test]
    fn test_h_goes_back() {
        let items = make_items(&["A"]);
        let mut state = PickerState::new("Test", "", items, true);
        let result = state.handle_key(KeyCode::Char('h'));
        assert!(matches!(result, Some(PickerResult::GoBack)));
    }

    // ── selected_count tests ───────────────────────────────────

    #[test]
    fn test_selected_count_none() {
        let items = make_items(&["A", "B", "C"]);
        let state = PickerState::new("Test", "", items, true);
        assert_eq!(state.selected_count(), 0);
    }

    #[test]
    fn test_selected_count_some() {
        let mut items = make_items(&["A", "B", "C"]);
        items[0].selected = true;
        items[2].selected = true;
        let state = PickerState::new("Test", "", items, true);
        assert_eq!(state.selected_count(), 2);
    }

    #[test]
    fn test_selected_count_after_toggle() {
        let items = make_items(&["A", "B"]);
        let mut state = PickerState::new("Test", "", items, true);
        assert_eq!(state.selected_count(), 0);
        state.handle_key(KeyCode::Char(' ')); // toggle A
        assert_eq!(state.selected_count(), 1);
        state.handle_key(KeyCode::Down);
        state.handle_key(KeyCode::Char(' ')); // toggle B
        assert_eq!(state.selected_count(), 2);
    }

    // ── is_fixed item tests ────────────────────────────────────

    #[test]
    fn test_fixed_item_cannot_be_toggled() {
        let mut items = make_items(&["Fixed", "Normal"]);
        items[0].is_fixed = true;
        items[0].selected = true;
        let mut state = PickerState::new("Test", "", items, true);

        // Cursor on "Fixed", try to toggle
        state.handle_key(KeyCode::Char(' '));
        assert!(state.items[0].selected, "Fixed item should remain selected");
    }

    #[test]
    fn test_fixed_item_preserved_during_toggle_all() {
        let mut items = make_items(&["Fixed", "A", "B"]);
        items[0].is_fixed = true;
        items[0].selected = true;
        let mut state = PickerState::new("Test", "", items, true);

        // Toggle all on
        state.handle_key(KeyCode::Char('a'));
        assert!(state.items[0].selected, "Fixed remains selected");
        assert!(state.items[1].selected, "A selected");
        assert!(state.items[2].selected, "B selected");

        // Toggle all off
        state.handle_key(KeyCode::Char('a'));
        assert!(
            state.items[0].selected,
            "Fixed still selected even after toggle-all-off"
        );
        assert!(!state.items[1].selected, "A deselected");
        assert!(!state.items[2].selected, "B deselected");
    }

    // ── Single-select picker tests ─────────────────────────────

    #[test]
    fn test_single_select_enter_selects_highlighted() {
        let mut items = make_items(&["A", "B", "C"]);
        items[0].selected = true; // pre-select A
        let mut state = PickerState::new("Test", "", items, false);

        // Navigate to C (index 2) and press Enter
        let result = run_keys(
            &mut state,
            &[
                KeyCode::Down, // -> B
                KeyCode::Down, // -> C
                KeyCode::Enter,
            ],
        );
        match result {
            Some(PickerResult::Confirmed(items)) => {
                assert!(!items[0].selected, "A should be deselected");
                assert!(!items[1].selected, "B should be deselected");
                assert!(items[2].selected, "C should be selected");
            }
            _ => panic!("Expected Confirmed"),
        }
    }

    #[test]
    fn test_single_select_navigation_auto_selects() {
        let items = make_items(&["A", "B", "C"]);
        let mut state = PickerState::new("Test", "", items, false);

        // In single-select, navigating down should auto-select the new item
        state.handle_key(KeyCode::Down); // -> B
        assert!(state.items[1].selected, "B auto-selected on nav");
        assert!(!state.items[0].selected, "A deselected on nav");

        state.handle_key(KeyCode::Down); // -> C
        assert!(state.items[2].selected, "C auto-selected on nav");
        assert!(!state.items[1].selected, "B deselected on nav");
    }

    #[test]
    fn test_single_select_space_selects_current() {
        let items = make_items(&["A", "B", "C"]);
        let mut state = PickerState::new("Test", "", items, false);

        state.handle_key(KeyCode::Down); // -> B
        state.handle_key(KeyCode::Char(' ')); // space on B

        assert!(!state.items[0].selected);
        assert!(state.items[1].selected, "B selected via space");
        assert!(!state.items[2].selected);
    }

    #[test]
    fn test_single_select_a_does_not_toggle_all() {
        let items = make_items(&["A", "B"]);
        let mut state = PickerState::new("Test", "", items, false);

        // 'a' should be a no-op in single select mode
        state.handle_key(KeyCode::Char('a'));
        // Neither should be selected (they started unselected)
        assert!(!state.items[0].selected);
        assert!(!state.items[1].selected);
    }

    #[test]
    fn test_single_select_up_navigation_auto_selects() {
        let items = make_items(&["A", "B", "C"]);
        let mut state = PickerState::new("Test", "", items, false);

        // Go up from A wraps to C
        state.handle_key(KeyCode::Up);
        assert!(state.items[2].selected, "C auto-selected on up wrap");
        assert!(!state.items[0].selected, "A deselected");
    }

    #[test]
    fn test_single_select_k_navigation_auto_selects() {
        let items = make_items(&["A", "B", "C"]);
        let mut state = PickerState::new("Test", "", items, false);

        // Go up from A wraps to C using k
        state.handle_key(KeyCode::Char('k'));
        assert!(state.items[2].selected, "C auto-selected with k key");
    }

    #[test]
    fn test_single_select_j_navigation_auto_selects() {
        let items = make_items(&["A", "B", "C"]);
        let mut state = PickerState::new("Test", "", items, false);

        state.handle_key(KeyCode::Char('j'));
        assert!(state.items[1].selected, "B auto-selected with j key");
    }

    // ── Template toggling tests ────────────────────────────────

    fn make_template_items() -> Vec<SelectItem> {
        vec![
            SelectItem {
                label: "✨ Recommended".to_string(),
                description: "Template".to_string(),
                selected: false,
                is_fixed: false,
                is_template: true,
                template_selects: vec!["gmail".to_string(), "drive".to_string()],
            },
            SelectItem {
                label: "🔒 Read Only".to_string(),
                description: "Template".to_string(),
                selected: false,
                is_fixed: false,
                is_template: true,
                template_selects: vec!["gmail.readonly".to_string(), "drive.readonly".to_string()],
            },
            SelectItem {
                label: "gmail".to_string(),
                description: "Gmail access".to_string(),
                selected: false,
                is_fixed: false,
                is_template: false,
                template_selects: vec![],
            },
            SelectItem {
                label: "gmail.readonly".to_string(),
                description: "Gmail readonly".to_string(),
                selected: false,
                is_fixed: false,
                is_template: false,
                template_selects: vec![],
            },
            SelectItem {
                label: "drive".to_string(),
                description: "Drive access".to_string(),
                selected: false,
                is_fixed: false,
                is_template: false,
                template_selects: vec![],
            },
            SelectItem {
                label: "drive.readonly".to_string(),
                description: "Drive readonly".to_string(),
                selected: false,
                is_fixed: false,
                is_template: false,
                template_selects: vec![],
            },
        ]
    }

    #[test]
    fn test_template_select_applies_scopes() {
        let items = make_template_items();
        let mut state = PickerState::new("Scopes", "", items, true);

        // Toggle "Recommended" template (at index 0)
        state.handle_key(KeyCode::Char(' '));

        assert!(state.items[0].selected, "Recommended template selected");
        assert!(state.items[2].selected, "gmail selected by template");
        assert!(
            !state.items[3].selected,
            "gmail.readonly NOT in recommended"
        );
        assert!(state.items[4].selected, "drive selected by template");
        assert!(
            !state.items[5].selected,
            "drive.readonly NOT in recommended"
        );
    }

    #[test]
    fn test_template_deselects_other_templates() {
        let items = make_template_items();
        let mut state = PickerState::new("Scopes", "", items, true);

        // Select "Recommended"
        state.handle_key(KeyCode::Char(' '));
        assert!(state.items[0].selected, "Recommended selected");

        // Navigate to "Read Only" and select it
        state.handle_key(KeyCode::Down);
        state.handle_key(KeyCode::Char(' '));

        assert!(!state.items[0].selected, "Recommended deselected");
        assert!(state.items[1].selected, "Read Only selected");
        // Read Only template scopes applied
        assert!(!state.items[2].selected, "gmail NOT in readonly");
        assert!(state.items[3].selected, "gmail.readonly selected");
        assert!(!state.items[4].selected, "drive NOT in readonly");
        assert!(state.items[5].selected, "drive.readonly selected");
    }

    #[test]
    fn test_toggling_individual_deselects_templates() {
        let items = make_template_items();
        let mut state = PickerState::new("Scopes", "", items, true);

        // Select "Recommended"
        state.handle_key(KeyCode::Char(' '));
        assert!(state.items[0].selected);

        // Navigate to "gmail" (index 2) and toggle it off
        state.handle_key(KeyCode::Down); // -> Read Only
        state.handle_key(KeyCode::Down); // -> gmail
        state.handle_key(KeyCode::Char(' ')); // toggle gmail off

        assert!(!state.items[0].selected, "Recommended template deselected");
        assert!(!state.items[1].selected, "Read Only template deselected");
        assert!(!state.items[2].selected, "gmail toggled off");
    }

    #[test]
    fn test_deselect_template_does_not_change_individual_items() {
        let items = make_template_items();
        let mut state = PickerState::new("Scopes", "", items, true);

        // Select "Recommended" template
        state.handle_key(KeyCode::Char(' '));
        assert!(state.items[2].selected, "gmail selected");

        // Deselect "Recommended" template (toggle off)
        state.handle_key(KeyCode::Char(' '));
        assert!(!state.items[0].selected, "Recommended deselected");
        // Individual items should NOT change when deselecting a template
        // (only selecting a template applies its selections)
        assert!(
            state.items[2].selected,
            "gmail still selected after template deselect"
        );
    }

    // ── Readonly/superset scope interaction tests ──────────────

    #[test]
    fn test_selecting_scope_deselects_readonly_counterpart() {
        let items = make_template_items();
        let mut state = PickerState::new("Scopes", "", items, true);

        // Navigate to gmail.readonly (index 3) and select it
        state.handle_key(KeyCode::Down); // -> Read Only
        state.handle_key(KeyCode::Down); // -> gmail
        state.handle_key(KeyCode::Down); // -> gmail.readonly
        state.handle_key(KeyCode::Char(' ')); // select gmail.readonly

        assert!(state.items[3].selected, "gmail.readonly selected");

        // Now navigate to gmail (index 2) and select it
        state.handle_key(KeyCode::Up); // -> gmail
        state.handle_key(KeyCode::Char(' ')); // select gmail

        assert!(state.items[2].selected, "gmail selected");
        assert!(
            !state.items[3].selected,
            "gmail.readonly deselected (superset wins)"
        );
    }

    #[test]
    fn test_selecting_readonly_deselects_write_counterpart() {
        let items = make_template_items();
        let mut state = PickerState::new("Scopes", "", items, true);

        // Navigate to gmail (index 2) and select it
        state.handle_key(KeyCode::Down); // -> Read Only
        state.handle_key(KeyCode::Down); // -> gmail
        state.handle_key(KeyCode::Char(' ')); // select gmail

        assert!(state.items[2].selected, "gmail selected");

        // Now navigate to gmail.readonly (index 3) and select it
        state.handle_key(KeyCode::Down); // -> gmail.readonly
        state.handle_key(KeyCode::Char(' ')); // select gmail.readonly

        assert!(state.items[3].selected, "gmail.readonly selected");
        assert!(
            !state.items[2].selected,
            "gmail deselected (readonly overrides)"
        );
    }

    #[test]
    fn test_deselecting_scope_does_not_affect_counterpart() {
        let items = make_template_items();
        let mut state = PickerState::new("Scopes", "", items, true);

        // Select both gmail and drive
        state.handle_key(KeyCode::Down); // -> Read Only
        state.handle_key(KeyCode::Down); // -> gmail
        state.handle_key(KeyCode::Char(' ')); // select gmail
        state.handle_key(KeyCode::Down); // -> gmail.readonly
        state.handle_key(KeyCode::Down); // -> drive
        state.handle_key(KeyCode::Char(' ')); // select drive

        assert!(state.items[2].selected, "gmail selected");
        assert!(state.items[4].selected, "drive selected");

        // Now deselect gmail - should NOT affect gmail.readonly
        state.handle_key(KeyCode::Up); // -> gmail.readonly
        state.handle_key(KeyCode::Up); // -> gmail
        state.handle_key(KeyCode::Char(' ')); // deselect gmail

        assert!(!state.items[2].selected, "gmail deselected");
        assert!(
            !state.items[3].selected,
            "gmail.readonly was never selected"
        );
    }

    // ── wrap_text tests ────────────────────────────────────────

    #[test]
    fn test_wrap_text_no_wrapping_needed() {
        let result = wrap_text("short text", 80);
        assert_eq!(result, vec!["short text"]);
    }

    #[test]
    fn test_wrap_text_wraps_long_line() {
        let result = wrap_text("hello world foo bar", 11);
        assert_eq!(result, vec!["hello world", "foo bar"]);
    }

    #[test]
    fn test_wrap_text_preserves_newlines() {
        let result = wrap_text("line one\nline two", 80);
        assert_eq!(result, vec!["line one", "line two"]);
    }

    #[test]
    fn test_wrap_text_empty_lines() {
        let result = wrap_text("before\n\nafter", 80);
        assert_eq!(result, vec!["before", "", "after"]);
    }

    #[test]
    fn test_wrap_text_zero_width() {
        let result = wrap_text("any text", 0);
        assert_eq!(result, vec!["any text"]);
    }

    #[test]
    fn test_wrap_text_single_long_word() {
        let result = wrap_text("superlongword", 5);
        // A single word longer than max_width can't be split, so it stays as-is
        assert_eq!(result, vec!["superlongword"]);
    }

    #[test]
    fn test_wrap_text_multiple_paragraphs_with_wrapping() {
        let result = wrap_text("aaa bbb ccc\nddd eee", 7);
        assert_eq!(result, vec!["aaa bbb", "ccc", "ddd eee"]);
    }

    // ── PickerState::new initial selection tests ───────────────

    #[test]
    fn test_picker_starts_at_first_selected_item() {
        let mut items = make_items(&["A", "B", "C"]);
        items[2].selected = true;
        let state = PickerState::new("Test", "", items, true);
        assert_eq!(state.list_state.selected(), Some(2));
    }

    #[test]
    fn test_picker_starts_at_zero_when_none_selected() {
        let items = make_items(&["A", "B", "C"]);
        let state = PickerState::new("Test", "", items, true);
        assert_eq!(state.list_state.selected(), Some(0));
    }

    // ── Edge case tests ────────────────────────────────────────

    #[test]
    fn test_single_item_toggle() {
        let items = make_items(&["Only"]);
        let mut state = PickerState::new("Test", "", items, true);
        state.handle_key(KeyCode::Char(' ')); // toggle on
        assert!(state.items[0].selected);
        state.handle_key(KeyCode::Char(' ')); // toggle off
        assert!(!state.items[0].selected);
    }

    #[test]
    fn test_single_item_navigation_wraps() {
        let items = make_items(&["Only"]);
        let mut state = PickerState::new("Test", "", items, true);
        state.handle_key(KeyCode::Down); // wraps to 0
        assert_eq!(state.list_state.selected(), Some(0));
        state.handle_key(KeyCode::Up); // wraps to 0
        assert_eq!(state.list_state.selected(), Some(0));
    }

    #[test]
    fn test_fixed_item_in_single_select_preserved() {
        let mut items = make_items(&["Fixed", "A", "B"]);
        items[0].is_fixed = true;
        items[0].selected = true;
        let mut state = PickerState::new("Test", "", items, false);

        // Navigate to B and press Enter
        let result = run_keys(
            &mut state,
            &[
                KeyCode::Down, // -> A
                KeyCode::Down, // -> B
                KeyCode::Enter,
            ],
        );
        match result {
            Some(PickerResult::Confirmed(items)) => {
                assert!(items[0].selected, "Fixed item preserved");
                assert!(!items[1].selected, "A not selected");
                assert!(items[2].selected, "B selected");
            }
            _ => panic!("Expected Confirmed"),
        }
    }
}
