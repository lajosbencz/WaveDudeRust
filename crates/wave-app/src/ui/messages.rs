//! The Messages pane: the log ring shown as a list; Info and up also reach the status bar.

use std::time::SystemTime;

use freya::prelude::*;
use tracing::Level;
use wave_widgets::{LogLevel, LogLine, MessageLabels, MessageList};

use super::app::Cx;
use super::shared::Topics;

fn level_of(level: Level) -> LogLevel {
	match level {
		Level::TRACE => LogLevel::Trace,
		Level::DEBUG => LogLevel::Debug,
		Level::INFO => LogLevel::Info,
		Level::WARN => LogLevel::Warn,
		_ => LogLevel::Error,
	}
}

fn time_text(t: SystemTime) -> String {
	let ms = t.duration_since(SystemTime::UNIX_EPOCH).map_or(0, |d| d.as_millis()) % 86_400_000;
	format!("{:02}:{:02}:{:02}.{:03}", ms / 3_600_000, ms / 60_000 % 60, ms / 1000 % 60, ms % 1000)
}

/// A ring line as the list's; a multi-line message keeps its first line, the whole text as the detail.
pub fn line_of(l: &wave_app_state::log::LogLine) -> LogLine {
	let first = l.message.lines().next().unwrap_or_default();
	let line = LogLine::new(time_text(l.time), level_of(l.level), l.target.clone(), first);
	if l.message.contains('\n') { line.detail(l.message.clone()) } else { line }
}

/// Moves new ring lines into the pane's log and the newest Info-or-up line into the status bar.
pub fn poll_log(cx: &Cx, seq: &mut Option<u64>) {
	let lines = cx.info.ring.since(*seq);
	let Some(last) = lines.last() else { return };
	*seq = Some(last.seq);
	if let Some(l) = lines.iter().rev().find(|l| l.reaches_status_bar()) {
		let mut status = cx.ui.status;
		let first = l.message.lines().next().unwrap_or_default().to_owned();
		let mut next = status.peek().clone();
		if next.apply(level_of(l.level), &first) {
			status.set(next);
		}
	}
	let mut log = cx.ui.messages;
	log.write().extend(lines.iter().map(line_of));
}

fn labels(cx: &Cx) -> MessageLabels {
	let s = cx.strings();
	let g = |k: &str| s.get(k).to_owned();
	MessageLabels {
		copy: g("MessagesCopy"),
		copy_all: g("MessagesCopyAll"),
		clear: g("MessagesClear"),
		search: g("MessagesSearchHint"),
		regex: g("MessagesRegex"),
		regex_tip: g("TipMessagesRegex"),
		auto_scroll: g("MessagesAutoScroll"),
		auto_scroll_tip: g("TipMessagesAutoScroll"),
		level_tip: g("TipMessagesLevel"),
		columns: ["MessagesColumnTime", "MessagesColumnLevel", "MessagesColumnLogger", "MessagesColumnMessage"].map(g),
	}
}

/// The Messages pane.
#[derive(PartialEq)]
pub struct MessagesPane;

impl Component for MessagesPane {
	fn render(&self) -> impl IntoElement {
		let cx = Cx::get();
		let words = {
			let cx = cx.clone();
			use_memo(move || {
				cx.shared.subscribe(Topics::SETTINGS);
				labels(&cx)
			})
		};
		MessageList::new(cx.ui.messages).labels(words.read().clone())
	}
}
