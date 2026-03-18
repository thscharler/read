#![cfg_attr(
    all(feature = "wgpu", not(feature = "term"), windows),
    windows_subsystem = "windows"
)]
#[cfg(feature = "term")]
pub(crate) use rat_salsa;
#[cfg(all(feature = "wgpu", not(feature = "term")))]
pub(crate) use rat_salsa_wgpu as rat_salsa;

use crate::rat_salsa::event::RenderedEvent;
#[cfg(feature = "term")]
use crate::rat_salsa::poll::PollCrossterm;
use crate::rat_salsa::poll::{PollRendered, PollTasks, PollTimers};
use crate::rat_salsa::timer::{TimeOut, TimerDef, TimerHandle};
use crate::rat_salsa::{Control, RunConfig, SalsaAppContext, SalsaContext, run_tui};
use anyhow::Error;
use log::debug;
#[cfg(all(feature = "wgpu", not(feature = "term")))]
use rat_salsa_wgpu::events::ConvertCrossterm;
use rat_salsa_wgpu::font::FontData;
#[cfg(all(feature = "wgpu", not(feature = "term")))]
use rat_salsa_wgpu::poll::PollBlink;
use rat_theme4::theme::SalsaTheme;
use rat_theme4::{WidgetStyle, create_salsa_theme};
use rat_widget::event::{Dialog, HandleEvent, Outcome, ReadOnly, Regular, ct_event, event_flow};
use rat_widget::focus::{FocusBuilder, FocusFlag, HasFocus, Navigation};
use rat_widget::list::{List, ListState};
use rat_widget::msgdialog::{MsgDialog, MsgDialogState};
use rat_widget::scrolled::{Scroll, ScrollbarPolicy};
use rat_widget::text::HasScreenCursor;
use rat_widget::text::clipboard::cli::setup_cli_clipboard;
use rat_widget::textarea::{TextArea, TextAreaState, TextWrap};
use ratatui::buffer::Buffer;
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::symbols;
use ratatui::widgets::{Block, Borders, ListItem, StatefulWidget};
use std::env::args;
use std::fs;
use std::path::PathBuf;
use std::time::Duration;

fn main() -> Result<(), Error> {
    setup_logging()?;
    setup_cli_clipboard();

    let mut config = Config::default();
    config.delay = 1500;
    let mut args = args();
    args.next();
    if let Some(dir) = args.next() {
        config.base = PathBuf::from(dir);
    } else {
        config.base = PathBuf::from(".");
    }

    let theme = rat_theme4::create_salsa_theme("EverForest Light");
    let mut global = Global::new(config, theme);
    let mut state = Scenery::default();
    state.show_files = true;

    #[cfg(all(feature = "wgpu", not(feature = "term")))]
    let run_cfg = {
        let mut r = RunConfig::new(ConvertCrossterm::new())?
            .window_title("Read")
            .poll(PollTasks::default())
            .poll(PollTimers::default())
            .poll(PollBlink::default())
            .poll(PollRendered);
        r
    };
    #[cfg(feature = "term")]
    let run_cfg = RunConfig::default()?
        .poll(PollCrossterm)
        .poll(PollTasks::default())
        .poll(PollTimers::default())
        .poll(PollRendered);

    run_tui(init, render, event, error, &mut global, &mut state, run_cfg)?;

    Ok(())
}

/// Globally accessible data/state.
pub struct Global {
    ctx: SalsaAppContext<RdEvent, Error>,

    pub cfg: Config,
    pub theme: SalsaTheme,
}

impl SalsaContext<RdEvent, Error> for Global {
    fn set_salsa_ctx(&mut self, app_ctx: SalsaAppContext<RdEvent, Error>) {
        self.ctx = app_ctx;
    }

    #[inline(always)]
    fn salsa_ctx(&self) -> &SalsaAppContext<RdEvent, Error> {
        &self.ctx
    }
}

impl Global {
    pub fn new(cfg: Config, theme: SalsaTheme) -> Self {
        Self {
            ctx: Default::default(),
            cfg,
            theme,
        }
    }
}

/// Configuration.
#[derive(Debug, Default)]
pub struct Config {
    delay: u64,
    base: PathBuf,
}

/// Application wide messages.
#[derive(Debug)]
pub enum RdEvent {
    Timer(TimeOut),
    Event(crossterm::event::Event),
    Rendered,
    Message(String),
}

impl From<RenderedEvent> for RdEvent {
    fn from(_: RenderedEvent) -> Self {
        Self::Rendered
    }
}

impl From<TimeOut> for RdEvent {
    fn from(value: TimeOut) -> Self {
        Self::Timer(value)
    }
}

impl From<crossterm::event::Event> for RdEvent {
    fn from(value: crossterm::event::Event) -> Self {
        Self::Event(value)
    }
}

#[derive(Debug, Default)]
pub struct Scenery {
    pub txt_files: Vec<(String, PathBuf)>,
    pub show_files: bool,

    pub timer: TimerHandle,

    pub files: ListState,
    pub text: TextAreaState,

    pub error_dlg: MsgDialogState,
}

impl HasFocus for Scenery {
    fn build(&self, builder: &mut FocusBuilder) {
        builder.widget_navigate(&self.text, Navigation::Regular);
        builder.widget(&self.files);
    }

    fn focus(&self) -> FocusFlag {
        unimplemented!("unused")
    }

    fn area(&self) -> Rect {
        unimplemented!("unused")
    }
}

pub fn render(
    area: Rect,
    buf: &mut Buffer,
    state: &mut Scenery,
    ctx: &mut Global,
) -> Result<(), Error> {
    if state.error_dlg.active() {
        MsgDialog::new()
            .styles(ctx.theme.style(WidgetStyle::MSG_DIALOG))
            .render(area, buf, &mut state.error_dlg);
    }

    let file_width = if state.show_files { 20 } else { 0 };
    let lr = Layout::horizontal([
        Constraint::Length(file_width), //
        Constraint::Fill(1),
    ])
    .split(area);

    List::new(state.txt_files.iter().map(|v| ListItem::new(v.0.as_str())))
        .scroll(Scroll::new().styles(ctx.theme.style(WidgetStyle::SCROLL)))
        .styles(ctx.theme.style(WidgetStyle::LIST))
        .render(lr[0], buf, &mut state.files);

    TextArea::new()
        .block(
            Block::new()
                .borders(Borders::LEFT)
                .border_set(symbols::border::EMPTY),
        )
        .vscroll(Scroll::new().policy(ScrollbarPolicy::Collapse))
        .text_wrap(TextWrap::Word(8))
        .styles(ctx.theme.style(WidgetStyle::TEXTVIEW))
        .render(lr[1], buf, &mut state.text);

    ctx.set_screen_cursor(state.text.screen_cursor());

    Ok(())
}

pub fn init(state: &mut Scenery, ctx: &mut Global) -> Result<(), Error> {
    ctx.set_focus(FocusBuilder::build_for(state));
    ctx.focus().focus(&state.files);

    load_files(state, ctx)?;
    if state.txt_files.len() > 0 {
        state.files.select(Some(0));
    }
    read_file(state)?;

    Ok(())
}

pub fn event(
    event: &RdEvent,
    state: &mut Scenery,
    ctx: &mut Global,
) -> Result<Control<RdEvent>, Error> {
    match event {
        RdEvent::Rendered => event_flow!({
            ctx.set_focus(FocusBuilder::rebuild_for(state, ctx.take_focus()));
            Control::Continue
        }),
        RdEvent::Message(s) => event_flow!({
            state.error_dlg.append(s.as_str());
            Control::Changed
        }),
        _ => {}
    }

    if let RdEvent::Event(event) = event {
        match &event {
            ct_event!(resized) => event_flow!(Control::Changed),
            ct_event!(focus_gained) => event_flow!(reload_files(state, ctx)?),
            ct_event!(key press CONTROL-'q') => event_flow!(Control::Quit),
            ct_event!(key press 'q') => event_flow!(Control::Quit),
            _ => {}
        };

        event_flow!({
            if state.error_dlg.active() {
                state.error_dlg.handle(event, Dialog).into()
            } else {
                Control::Continue
            }
        });

        ctx.handle_focus(event);

        match event {
            ct_event!(keycode press F(1)) => event_flow!(start_stop(state, ctx)?),
            ct_event!(keycode press F(3)) => event_flow!(flip_files(state, ctx)?),
            ct_event!(keycode press F(5)) => event_flow!(faster(state, ctx)?),
            ct_event!(keycode press F(6)) => event_flow!(slower(state, ctx)?),
            ct_event!(keycode press F(8)) => event_flow!(next_theme(ctx)?),
            ct_event!(keycode press SHIFT-F(8)) => event_flow!(prev_theme(ctx)?),
            #[cfg(all(feature = "wgpu", not(feature = "term")))]
            ct_event!(keycode press F(7)) => event_flow!(next_font(ctx)?),
            #[cfg(all(feature = "wgpu", not(feature = "term")))]
            ct_event!(keycode press SHIFT-F(7)) => event_flow!(prev_font(ctx)?),
            _ => {}
        }

        event_flow!(match state.files.handle(event, Regular) {
            Outcome::Changed => {
                read_file(state)?;
                Control::Changed
            }
            r => r.into(),
        });
        event_flow!(state.text.handle(event, ReadOnly));
    }

    if let RdEvent::Timer(timeout) = event {
        if state.timer == timeout.handle {
            event_flow!(auto_scroll(state, ctx)?)
        }
    }

    Ok(Control::Continue)
}

fn reload_files(state: &mut Scenery, ctx: &mut Global) -> Result<Control<RdEvent>, Error> {
    load_files(state, ctx)?;
    if state.files.selected_checked().is_none() {
        if state.txt_files.len() > 0 {
            state.files.select(Some(state.txt_files.len() - 1));
        }
        read_file(state)?;
        Ok(Control::Changed)
    } else {
        Ok(Control::Continue)
    }
}

fn load_files(state: &mut Scenery, ctx: &Global) -> Result<(), Error> {
    state.txt_files.clear();
    for f in fs::read_dir(&ctx.cfg.base)? {
        let f = f?;
        let m = f.metadata()?;
        if m.is_file() && f.file_name().to_string_lossy().ends_with(".txt") {
            let name = f
                .path()
                .file_stem()
                .unwrap_or_default()
                .to_string_lossy()
                .to_string();
            state.txt_files.push((name, f.path()));
        }
    }
    state.txt_files.sort();

    state.files.rows_changed(state.txt_files.len());

    Ok(())
}

fn read_file(state: &mut Scenery) -> Result<(), Error> {
    if let Some(idx) = state.files.selected_checked() {
        let f = &state.txt_files[idx].1;

        let txt = fs::read(f)?;
        let txt = String::from_utf8_lossy(&txt);
        let mut buf = String::new();
        for l in txt.lines() {
            if !l.is_empty() {
                buf.push_str(l.trim());
                buf.push(' ');
            } else {
                buf.push_str("\n\n");
            }
        }

        state.text.set_text(txt);
    } else {
        state.text.clear();
    }
    Ok(())
}

fn auto_scroll(state: &mut Scenery, _ctx: &mut Global) -> Result<Control<RdEvent>, Error> {
    let (_, mut row) = state
        .text
        .screen_cursor()
        .unwrap_or((state.text.area.x, state.text.area.y));
    if row + 1 < state.text.area.bottom() {
        row = state.text.area.bottom().saturating_sub(1);
    }
    state.text.set_screen_cursor((0, row as i16 + 1), false);
    Ok(Control::Changed)
}

fn flip_files(state: &mut Scenery, ctx: &mut Global) -> Result<Control<RdEvent>, Error> {
    state.show_files = !state.show_files;
    debug!("show {}", state.show_files);
    if state.show_files {
        ctx.focus().focus(&state.files);
    } else {
        ctx.focus().focus(&state.text);
    }
    Ok(Control::Changed)
}

fn slower(state: &mut Scenery, ctx: &mut Global) -> Result<Control<RdEvent>, Error> {
    if ctx.cfg.delay > 100 {
        ctx.cfg.delay -= 100;
    }
    state.timer = ctx.replace_timer(
        Some(state.timer),
        TimerDef::new()
            .timer(Duration::from_millis(ctx.cfg.delay))
            .repeat_forever(),
    );
    Ok(Control::Changed)
}

fn faster(state: &mut Scenery, ctx: &mut Global) -> Result<Control<RdEvent>, Error> {
    ctx.cfg.delay += 100;
    state.timer = ctx.replace_timer(
        Some(state.timer),
        TimerDef::new()
            .timer(Duration::from_millis(ctx.cfg.delay))
            .repeat_forever(),
    );
    Ok(Control::Changed)
}

fn start_stop(state: &mut Scenery, ctx: &mut Global) -> Result<Control<RdEvent>, Error> {
    if state.timer == TimerHandle::default() {
        state.timer = ctx.add_timer(
            TimerDef::new()
                .timer(Duration::from_millis(ctx.cfg.delay))
                .repeat_forever(),
        );
    } else {
        ctx.remove_timer(state.timer);
        state.timer = TimerHandle::default();
    }
    Ok(Control::Changed)
}

fn next_theme(ctx: &mut Global) -> Result<Control<RdEvent>, Error> {
    let themes = rat_theme4::salsa_themes();
    let idx = themes
        .iter()
        .position(|v| *v == &ctx.theme.name)
        .unwrap_or(0);
    if idx + 1 < themes.len() {
        ctx.theme = create_salsa_theme(themes[idx + 1]);
    } else {
        ctx.theme = create_salsa_theme(themes[0]);
    }
    Ok(Control::Changed)
}

fn prev_theme(ctx: &mut Global) -> Result<Control<RdEvent>, Error> {
    let themes = rat_theme4::salsa_themes();
    let idx = themes
        .iter()
        .position(|v| *v == &ctx.theme.name)
        .unwrap_or(0);
    if idx > 0 {
        ctx.theme = create_salsa_theme(themes[idx - 1]);
    } else {
        ctx.theme = create_salsa_theme(themes[themes.len() - 1]);
    }
    Ok(Control::Changed)
}

#[cfg(all(feature = "wgpu", not(feature = "term")))]
fn next_font(ctx: &mut Global) -> Result<Control<RdEvent>, Error> {
    let fonts = FontData.installed_fonts();
    let idx = fonts
        .iter()
        .position(|v| v == &ctx.font_family())
        .unwrap_or(0);
    if idx + 1 < fonts.len() {
        ctx.set_font_family(&fonts[idx + 1]);
    } else {
        ctx.set_font_family(&fonts[0]);
    }
    Ok(Control::Changed)
}

#[cfg(all(feature = "wgpu", not(feature = "term")))]
fn prev_font(ctx: &mut Global) -> Result<Control<RdEvent>, Error> {
    let fonts = FontData.installed_fonts();
    let idx = fonts
        .iter()
        .position(|v| v == &ctx.font_family())
        .unwrap_or(0);
    if idx > 1 {
        ctx.set_font_family(&fonts[idx - 1]);
    } else {
        ctx.set_font_family(&fonts[fonts.len() - 1]);
    }
    Ok(Control::Changed)
}

pub fn error(
    event: Error,
    state: &mut Scenery,
    _ctx: &mut Global,
) -> Result<Control<RdEvent>, Error> {
    state.error_dlg.append(format!("{:?}", &*event).as_str());
    Ok(Control::Changed)
}

fn setup_logging() -> Result<(), Error> {
    let log_path = PathBuf::from(".");
    let log_file = log_path.join("log.log");
    _ = fs::remove_file(&log_file);
    fern::Dispatch::new()
        .format(|out, message, _record| {
            out.finish(format_args!("{}", message)) //
        })
        .level(log::LevelFilter::Debug)
        .chain(fern::log_file(&log_file)?)
        .apply()?;
    Ok(())
}
