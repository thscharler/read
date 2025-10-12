use anyhow::Error;
use rat_salsa::event::RenderedEvent;
use rat_salsa::poll::{PollCrossterm, PollRendered, PollTasks, PollTimers};
use rat_salsa::timer::{TimeOut, TimerDef, TimerHandle};
use rat_salsa::{Control, RunConfig, SalsaAppContext, SalsaContext, run_tui};
use rat_theme3::{SalsaTheme, create_theme};
use rat_widget::event::{Dialog, HandleEvent, ReadOnly, ct_event, try_flow};
use rat_widget::focus::{FocusBuilder, FocusFlag, HasFocus};
use rat_widget::msgdialog::{MsgDialog, MsgDialogState};
use rat_widget::scrolled::{Scroll, ScrollbarPolicy};
use rat_widget::statusline::{StatusLine, StatusLineState};
use rat_widget::text::{HasScreenCursor, TextStyle};
use rat_widget::textarea::{TextArea, TextAreaState, TextWrap};
use ratatui::buffer::Buffer;
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::Style;
use ratatui::widgets::StatefulWidget;
use std::env::args;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::Duration;

fn main() -> Result<(), Error> {
    setup_logging()?;

    let mut args = args();
    args.next();
    let Some(f) = args.next() else {
        eprintln!("read file.txt");
        return Ok(());
    };

    let text = read_file(&PathBuf::from(&f))?;

    let config = Config::default();
    let theme = create_theme("Imperial Shell").expect("theme");
    let mut global = Global::new(config, theme);
    let mut state = Scenery::default();
    state.text.set_text(text);
    state.delay = 500;
    state.status.status(0, f);

    run_tui(
        init,
        render,
        event,
        error,
        &mut global,
        &mut state,
        RunConfig::default()?
            .poll(PollCrossterm)
            .poll(PollTimers::default())
            .poll(PollTasks::default())
            .poll(PollRendered),
    )?;

    Ok(())
}

/// Globally accessible data/state.
pub struct Global {
    ctx: SalsaAppContext<RdEvent, Error>,

    pub cfg: Config,
    pub theme: Box<dyn SalsaTheme>,
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
    pub fn new(cfg: Config, theme: Box<dyn SalsaTheme>) -> Self {
        Self {
            ctx: Default::default(),
            cfg,
            theme,
        }
    }
}

/// Configuration.
#[derive(Debug, Default)]
pub struct Config {}

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
    pub text: TextAreaState,
    pub status: StatusLineState,

    pub timer: TimerHandle,
    pub delay: u64,

    pub error_dlg: MsgDialogState,
}

impl HasFocus for Scenery {
    fn build(&self, builder: &mut FocusBuilder) {
        builder.widget(&self.text);
    }

    fn focus(&self) -> FocusFlag {
        unimplemented!("unused")
    }

    fn area(&self) -> Rect {
        unimplemented!("unused")
    }
}

fn read_file(file: &Path) -> Result<String, Error> {
    let txt = fs::read_to_string(file)?;
    let mut buf = String::new();

    for l in txt.lines() {
        if !l.is_empty() {
            buf.push_str(l.trim());
            buf.push(' ');
        } else {
            buf.push_str("\n\n");
        }
    }

    Ok(buf)
}

pub fn render(
    area: Rect,
    buf: &mut Buffer,
    state: &mut Scenery,
    ctx: &mut Global,
) -> Result<(), Error> {
    if state.error_dlg.active() {
        MsgDialog::new()
            .styles(ctx.theme.msg_dialog_style())
            .render(area, buf, &mut state.error_dlg);
    }

    let ll = Layout::vertical([Constraint::Fill(1), Constraint::Length(1)]).split(area);

    TextArea::new()
        .vscroll(Scroll::new().policy(ScrollbarPolicy::Collapse))
        .text_wrap(TextWrap::Word(8))
        .styles(TextStyle {
            style: Style::new().fg(ctx.theme.palette().text_light),
            select: Some(ctx.theme.text_select()),
            scroll: Some(ctx.theme.scroll_style()),
            border_style: Some(ctx.theme.container_border()),
            ..TextStyle::default()
        })
        .render(ll[0], buf, &mut state.text);

    state.status.status(
        1,
        format!("{}/{}", state.text.cursor().y, state.text.len_lines()),
    );
    if state.timer != TimerHandle::default() {
        state.status.status(2, format!("{}ms", state.delay));
    }

    StatusLine::new()
        .layout([
            Constraint::Fill(1),
            Constraint::Length(15),
            Constraint::Length(10),
            Constraint::Length(10),
        ])
        .styles_ext(ctx.theme.statusline_style_ext())
        .render(ll[1], buf, &mut state.status);

    ctx.set_screen_cursor(state.text.screen_cursor());

    Ok(())
}

pub fn init(state: &mut Scenery, ctx: &mut Global) -> Result<(), Error> {
    ctx.set_focus(FocusBuilder::build_for(state));
    ctx.focus().focus(&state.text);
    Ok(())
}

pub fn event(
    event: &RdEvent,
    state: &mut Scenery,
    ctx: &mut Global,
) -> Result<Control<RdEvent>, Error> {
    match event {
        RdEvent::Rendered => try_flow!({
            ctx.set_focus(FocusBuilder::rebuild_for(state, ctx.take_focus()));
            Control::Continue
        }),
        RdEvent::Message(s) => try_flow!({
            state.error_dlg.append(s.as_str());
            Control::Changed
        }),
        _ => {}
    }

    if let RdEvent::Event(event) = event {
        match &event {
            ct_event!(resized) => try_flow!(Control::Changed),
            ct_event!(key press CONTROL-'q') => try_flow!(Control::Quit),
            ct_event!(key press 'q') => try_flow!(Control::Quit),
            _ => {}
        };

        try_flow!({
            if state.error_dlg.active() {
                state.error_dlg.handle(event, Dialog).into()
            } else {
                Control::Continue
            }
        });

        ctx.handle_focus(event);
    }

    try_flow!(log text: text_event(event, state, ctx)?);

    Ok(Control::Continue)
}

fn text_event(
    event: &RdEvent,
    state: &mut Scenery,
    ctx: &mut Global,
) -> Result<Control<RdEvent>, Error> {
    if let RdEvent::Event(event) = event {
        try_flow!(state.text.handle(event, ReadOnly));

        match event {
            ct_event!(key press 'r') => try_flow!({
                state.delay += 100;

                ctx.remove_timer(state.timer);
                state.timer = ctx.add_timer(
                    TimerDef::new()
                        .timer(Duration::from_millis(state.delay))
                        .repeat_forever(),
                );

                Control::Changed
            }),
            ct_event!(key press 'e') => try_flow!({
                if state.delay > 100 {
                    state.delay -= 100;
                }

                ctx.remove_timer(state.timer);
                state.timer = ctx.add_timer(
                    TimerDef::new()
                        .timer(Duration::from_millis(state.delay))
                        .repeat_forever(),
                );

                Control::Changed
            }),
            ct_event!(key press 's') => try_flow!({
                ctx.remove_timer(state.timer);
                state.timer = TimerHandle::default();
                Control::Changed
            }),
            _ => {}
        }
    }

    if let RdEvent::Timer(timeout) = event {
        if state.timer == timeout.handle {
            try_flow!({
                let c = state
                    .text
                    .screen_cursor()
                    .unwrap_or((state.text.area.x, state.text.area.y));
                state
                    .text
                    .set_screen_cursor((c.0 as i16, c.1 as i16 + 1), false);
                Control::Changed
            })
        }
    }

    Ok(Control::Continue)
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
