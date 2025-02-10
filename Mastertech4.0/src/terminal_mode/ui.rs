use std::sync::atomic::Ordering;

use anyhow::Result;
use crossterm::event::KeyEvent;
use yazi_config::keymap::Key;
use yazi_core::input::InputMode;
use yazi_macro::emit;
use yazi_shared::{Layer, event::{CmdCow, Event, NEED_RENDER}};

use crate::{Ctx, Executor, Router, Signals, Term, lives::Lives};

pub(crate) struct App {
	pub(crate) cx:      Ctx,
	pub(crate) term:    Option<Term>,
	pub(crate) signals: Signals,
}

impl App {
	pub(crate) async fn serve() -> Result<()> {
		let term = Term::start()?;
		let (mut rx, signals) = (Event::take(), Signals::start()?);

		Lives::register()?;
		let mut app = Self { cx: Ctx::make(), term: Some(term), signals };
		app.render();

		let mut times = 0;
		let mut events = Vec::with_capacity(200);
		while rx.recv_many(&mut events, 50).await > 0 {
			for event in events.drain(..) {
				times += 1;
				app.dispatch(event)?;
			}

			if !NEED_RENDER.swap(false, Ordering::Relaxed) {
				continue;
			}

			if times >= 50 {
				times = 0;
				app.render();
			} else if let Ok(event) = rx.try_recv() {
				events.push(event);
				emit!(Render);
			} else {
				times = 0;
				app.render();
			}
		}
		Ok(())
	}

	#[inline]
	fn dispatch(&mut self, event: Event) -> Result<()> {
		match event {
			Event::Call(cmd, layer) => self.dispatch_call(cmd, layer),
			Event::Seq(cmds, layer) => self.dispatch_seq(cmds, layer),
			Event::Render => self.dispatch_render(),
			Event::Key(key) => self.dispatch_key(key),
			Event::Mouse(mouse) => self.mouse(mouse),
			Event::Resize => self.resize(()),
			Event::Paste(str) => self.dispatch_paste(str),
			Event::Quit(opt) => self.quit(opt),
		}
		Ok(())
	}

	#[inline]
	fn dispatch_call(&mut self, cmd: CmdCow, layer: Layer) {
		Executor::new(self).execute(cmd, layer);
	}

	#[inline]
	fn dispatch_seq(&mut self, mut cmds: Vec<CmdCow>, layer: Layer) {
		if let Some(last) = cmds.pop() {
			Executor::new(self).execute(last, layer);
		}
		if !cmds.is_empty() {
			emit!(Seq(cmds, layer));
		}
	}

	#[inline]
	fn dispatch_render(&mut self) { NEED_RENDER.store(true, Ordering::Relaxed); }

	#[inline]
	fn dispatch_key(&mut self, key: KeyEvent) { Router::new(self).route(Key::from(key)); }

	#[inline]
	fn dispatch_paste(&mut self, str: String) {
		if self.cx.input.visible {
			let input = &mut self.cx.input;
			if input.mode() == InputMode::Insert {
				input.type_str(&str);
			} else if input.mode() == InputMode::Replace {
				input.replace_str(&str);
			}
		}
	}
} 


pub(crate) struct Tasks<'a> {
	cx: &'a Ctx,
}

impl<'a> Tasks<'a> {
	pub(crate) fn new(cx: &'a Ctx) -> Self { Self { cx } }

	pub(super) fn area(area: Rect) -> Rect {
		let chunk = layout::Layout::vertical([
			Constraint::Percentage((100 - TASKS_PERCENT) / 2),
			Constraint::Percentage(TASKS_PERCENT),
			Constraint::Percentage((100 - TASKS_PERCENT) / 2),
		])
		.split(area)[1];

		layout::Layout::horizontal([
			Constraint::Percentage((100 - TASKS_PERCENT) / 2),
			Constraint::Percentage(TASKS_PERCENT),
			Constraint::Percentage((100 - TASKS_PERCENT) / 2),
		])
		.split(chunk)[1]
	}
}

impl Widget for Tasks<'_> {
	fn render(self, area: Rect, buf: &mut Buffer) {
		let area = Self::area(area);

		yazi_plugin::elements::Clear::default().render(area, buf);
		let block = Block::bordered()
			.title(Line::styled("Tasks", THEME.tasks.title))
			.title_alignment(Alignment::Center)
			.padding(Padding::symmetric(1, 1))
			.border_type(BorderType::Rounded)
			.border_style(THEME.tasks.border);

		let inner = block.inner(area);
		block.render(area, buf);

		let tasks = &self.cx.tasks;
		let items = tasks.summaries.iter().take(inner.height as usize).enumerate().map(|(i, v)| {
			let mut item =
				Text::from_iter(textwrap::wrap(&v.name, inner.width as usize).into_iter().map(Line::from));
			if i == tasks.cursor {
				item = item.style(THEME.tasks.hovered);
			}
			item
		});

		List::new(items).render(inner, buf);
	}
}

use std::{io::{BufWriter, stderr}, sync::atomic::Ordering};

use crossterm::{execute, queue, terminal::{BeginSynchronizedUpdate, EndSynchronizedUpdate}};
use ratatui::{CompletedFrame, backend::{Backend, CrosstermBackend}, buffer::Buffer};
use scopeguard::defer;
use yazi_plugin::elements::COLLISION;
use yazi_shared::event::NEED_RENDER;

use crate::{app::App, lives::Lives, root::Root};

impl App {
	pub(crate) fn render(&mut self) {
		NEED_RENDER.store(false, Ordering::Relaxed);
		let Some(term) = &mut self.term else { return };

		queue!(stderr(), BeginSynchronizedUpdate).ok();
		defer! { execute!(stderr(), EndSynchronizedUpdate).ok(); }

		let collision = COLLISION.swap(false, Ordering::Relaxed);
		let frame = term
			.draw(|f| {
				_ = Lives::scope(&self.cx, || Ok(f.render_widget(Root::new(&self.cx), f.area())));

				if let Some(pos) = self.cx.cursor() {
					f.set_cursor_position(pos);
				}
			})
			.unwrap();

		if COLLISION.load(Ordering::Relaxed) {
			Self::patch(frame, self.cx.cursor());
		}
		if !self.cx.notify.messages.is_empty() {
			self.render_partially();
		}

		// Reload preview if collision is resolved
		if collision && !COLLISION.load(Ordering::Relaxed) {
			self.cx.manager.peek(true);
		}
	}

	pub(crate) fn render_partially(&mut self) {
		let Some(term) = &mut self.term else { return };
		if !term.can_partial() {
			return self.render();
		}

		let frame = term
			.draw_partial(|f| {
				_ = Lives::scope(&self.cx, || {
					f.render_widget(crate::tasks::Progress::new(&self.cx), f.area());
					f.render_widget(crate::notify::Notify::new(&self.cx), f.area());
					Ok(())
				});

				if let Some(pos) = self.cx.cursor() {
					f.set_cursor_position(pos);
				}
			})
			.unwrap();

		if COLLISION.load(Ordering::Relaxed) {
			Self::patch(frame, self.cx.cursor());
		}
	}

	#[inline]
	fn patch(frame: CompletedFrame, cursor: Option<(u16, u16)>) {
		let mut new = Buffer::empty(frame.area);
		for y in new.area.top()..new.area.bottom() {
			for x in new.area.left()..new.area.right() {
				let cell = &frame.buffer[(x, y)];
				if cell.skip {
					new[(x, y)] = cell.clone();
				}
				new[(x, y)].set_skip(!cell.skip);
			}
		}

		let patches = frame.buffer.diff(&new);
		let mut backend = CrosstermBackend::new(BufWriter::new(stderr().lock()));
		backend.draw(patches.into_iter()).ok();
		if let Some(pos) = cursor {
			backend.show_cursor().ok();
			backend.set_cursor_position(pos).ok();
		}
		backend.flush().ok();
	}
}

use mlua::{ObjectLike, Table};
use ratatui::{buffer::Buffer, layout::Rect, widgets::Widget};
use tracing::error;
use yazi_plugin::{LUA, elements::render_once};

use super::{completion, confirm, help, input, manager, pick, spot, tasks, which};
use crate::Ctx;

pub(super) struct Root<'a> {
	cx: &'a Ctx,
}

impl<'a> Root<'a> {
	pub(super) fn new(cx: &'a Ctx) -> Self { Self { cx } }

	pub(super) fn reflow(area: Rect) -> mlua::Result<Table> {
		let area = yazi_plugin::elements::Rect::from(area);
		let root = LUA.globals().raw_get::<Table>("Root")?.call_method::<Table>("new", area)?;
		root.call_method("reflow", ())
	}
}

impl Widget for Root<'_> {
	fn render(self, area: Rect, buf: &mut Buffer) {
		let mut f = || {
			let area = yazi_plugin::elements::Rect::from(area);
			let root = LUA.globals().raw_get::<Table>("Root")?.call_method::<Table>("new", area)?;

			render_once(root.call_method("redraw", ())?, buf, |p| self.cx.manager.area(p));
			Ok::<_, mlua::Error>(())
		};
		if let Err(e) = f() {
			error!("Failed to redraw the `Root` component:\n{e}");
		}

		manager::Preview::new(self.cx).render(area, buf);
		manager::Modal::new(self.cx).render(area, buf);

		if self.cx.tasks.visible {
			tasks::Tasks::new(self.cx).render(area, buf);
		}

		if self.cx.active().spot.visible() {
			spot::Spot::new(self.cx).render(area, buf);
		}

		if self.cx.pick.visible {
			pick::Pick::new(self.cx).render(area, buf);
		}

		if self.cx.input.visible {
			input::Input::new(self.cx).render(area, buf);
		}

		if self.cx.confirm.visible {
			confirm::Confirm::new(self.cx).render(area, buf);
		}

		if self.cx.help.visible {
			help::Help::new(self.cx).render(area, buf);
		}

		if self.cx.completion.visible {
			completion::Completion::new(self.cx).render(area, buf);
		}

		if self.cx.which.visible {
			which::Which::new(self.cx).render(area, buf);
		}
	}
}