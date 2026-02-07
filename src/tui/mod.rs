pub mod event;
pub mod keymap;
pub mod state;
pub mod tree;
pub mod view;

use std::io::{self, Stdout};

use crossterm::cursor;
use crossterm::execute;
use crossterm::terminal::{
    EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode,
};
use ratatui::Terminal;
use ratatui::backend::CrosstermBackend;

use crate::error::AppError;
use crate::spec::index::EndpointSummary;
use crate::tui::event::FrameMetrics;
use crate::tui::state::{AppState, TuiContext};

#[derive(Debug, Clone, Copy)]
pub struct TuiOptions {
    pub use_alt_screen: bool,
}

pub fn run(
    context: TuiContext,
    endpoints: Vec<EndpointSummary>,
    options: TuiOptions,
) -> Result<(), AppError> {
    let mut terminal = init_terminal(options)?;
    let mut cleanup = TerminalCleanup::new(options);
    let run_result = run_event_loop(&mut terminal, context, endpoints);
    let restore_result = restore_terminal(&mut terminal, options);
    cleanup.disarm();

    match (run_result, restore_result) {
        (Err(run_error), _) => Err(run_error),
        (Ok(_), Err(restore_error)) => Err(restore_error),
        (Ok(_), Ok(_)) => Ok(()),
    }
}

fn init_terminal(options: TuiOptions) -> Result<Terminal<CrosstermBackend<Stdout>>, AppError> {
    enable_raw_mode().map_err(tui_error)?;

    let mut stdout = io::stdout();
    if options.use_alt_screen {
        execute!(stdout, EnterAlternateScreen, cursor::Hide).map_err(tui_error)?;
    }

    let backend = CrosstermBackend::new(stdout);
    Terminal::new(backend).map_err(tui_error)
}

fn run_event_loop(
    terminal: &mut Terminal<CrosstermBackend<Stdout>>,
    context: TuiContext,
    endpoints: Vec<EndpointSummary>,
) -> Result<(), AppError> {
    let mut state = AppState::new(context, endpoints);

    while !state.is_quit_requested() {
        let mut metrics = FrameMetrics::default();
        terminal
            .draw(|frame| {
                metrics = view::draw(frame, &mut state);
            })
            .map_err(tui_error)?;

        if state.is_quit_requested() {
            break;
        }

        event::process_next_event(&mut state, metrics)?;
    }

    Ok(())
}

fn restore_terminal(
    terminal: &mut Terminal<CrosstermBackend<Stdout>>,
    options: TuiOptions,
) -> Result<(), AppError> {
    disable_raw_mode().map_err(tui_error)?;
    if options.use_alt_screen {
        execute!(terminal.backend_mut(), LeaveAlternateScreen, cursor::Show).map_err(tui_error)?;
    } else {
        execute!(terminal.backend_mut(), cursor::Show).map_err(tui_error)?;
    }
    terminal.show_cursor().map_err(tui_error)?;
    Ok(())
}

fn tui_error(source: std::io::Error) -> AppError {
    AppError::TuiRuntime { source }
}

struct TerminalCleanup {
    options: TuiOptions,
    active: bool,
}

impl TerminalCleanup {
    fn new(options: TuiOptions) -> Self {
        Self {
            options,
            active: true,
        }
    }

    fn disarm(&mut self) {
        self.active = false;
    }
}

impl Drop for TerminalCleanup {
    fn drop(&mut self) {
        if !self.active {
            return;
        }

        let _ = disable_raw_mode();
        let mut stdout = io::stdout();
        if self.options.use_alt_screen {
            let _ = execute!(stdout, LeaveAlternateScreen, cursor::Show);
        } else {
            let _ = execute!(stdout, cursor::Show);
        }
    }
}
