//! Rendering + keyboard handling for `OrderQcTab`, split out to keep `mod.rs`
//! focused on state and async. Six sub-views switched by `[`/`]` (or 1–6).

use database::orders::checklist::ItemStatus;
use database::orders::{gate::GateOutcome, BackendKind};

use mtech_tui::events::action_handler::WidgetId;
use mtech_tui::styling::THEME;
use mtech_tui::widgets::{button::ButtonState, ButtonType, ShrinkArea, SHORTCUT_SET};
use ratatui::{
    crossterm::event::{KeyCode, KeyEvent},
    layout::{Constraint, Direction, Layout, Margin, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Cell, Paragraph, Row, Table, Widget, WidgetRef, Wrap},
    Frame,
};

use crate::provisioning::{self, Company};

use super::{
    AuthRole, OrderQcTab, View, COMMENT_ID, EXEC_EMAIL_ID, EXEC_PASS_ID, MKT_EMAIL_ID, MKT_PASS_ID,
    PROV_DMI_TOOL_ID, REPORT_NOTES_ID, SIGNOFF_EMAIL_ID, SIGNOFF_PASS_ID, TECH_EMAIL_ID,
    TECH_PASS_ID,
};

fn status_color(status: ItemStatus) -> Color {
    match status {
        ItemStatus::Pass => THEME.success,
        ItemStatus::Fail => THEME.error,
        ItemStatus::Na => THEME.text_muted,
        ItemStatus::Unset => THEME.warning,
    }
}

/// Fixed-width status cell; brackets the cell when it is the item's current status.
fn cell_label(cell: ItemStatus, current: ItemStatus) -> String {
    let (on, off) = match cell {
        ItemStatus::Pass => ("[P]", " P "),
        ItemStatus::Fail => ("[F]", " F "),
        ItemStatus::Na => ("[N/A]", " N/A "),
        ItemStatus::Unset => ("", ""),
    };
    if cell == current { on.to_string() } else { off.to_string() }
}

impl<'a> OrderQcTab<'a> {
    pub(super) fn render(&mut self, f: &mut Frame, area: Rect) {
        self.zones.begin();
        let block = Block::default()
            .borders(Borders::ALL)
            .border_set(SHORTCUT_SET)
            .border_style(THEME.border(false))
            .title_style(THEME.title())
            .title("Order QC");
        (&block).render(area, f.buffer_mut());
        let inner = block.inner(area);

        let rows = Layout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Length(1), Constraint::Fill(1)])
            .margin(1)
            .split(inner);

        self.draw_view_bar(f, rows[0]);

        match self.view {
            View::List => self.draw_list(f, rows[1]),
            View::Order => self.draw_order(f, rows[1]),
            View::SignOff => self.draw_signoff(f, rows[1]),
            View::Report => self.draw_report(f, rows[1]),
            View::Comments => self.draw_comments(f, rows[1]),
            View::Provision => self.draw_provision(f, rows[1]),
        }

        if self.prov_company_open {
            self.prov_company_menu.render(f, f.area());
            for (rect, idx) in self.prov_company_menu.item_rects() {
                self.zones.add(rect, format!("menu:{idx}"));
            }
        }
    }

    fn draw_view_bar(&self, f: &mut Frame, area: Rect) {
        let mut spans: Vec<Span> = Vec::new();
        let mut x = area.x;
        let hovered = self.zones.hovered();
        if self.session.is_none() {
            let label = " List ";
            let style = Style::default().fg(THEME.accent).add_modifier(Modifier::BOLD);
            spans.push(Span::styled(label, style));
            self.zones.add(Rect { x, y: area.y, width: label.len() as u16, height: 1 }, "view:list");
            spans.push(Span::styled(
                "  load an order to begin QC.   [/] switch views",
                Style::default().fg(THEME.text_muted),
            ));
        } else {
            let views = [
                (View::Order, "view:order"),
                (View::SignOff, "view:signoff"),
                (View::Report, "view:report"),
                (View::Comments, "view:comments"),
                (View::Provision, "view:provision"),
            ];
            for (v, zone) in views {
                let label = format!(" {} ", v.label());
                let style = if v == self.view {
                    Style::default().fg(THEME.accent).add_modifier(Modifier::BOLD)
                } else if hovered.as_deref() == Some(zone) {
                    Style::default().fg(THEME.text)
                } else {
                    Style::default().fg(THEME.text_muted)
                };
                let w = label.len() as u16;
                self.zones.add(Rect { x, y: area.y, width: w, height: 1 }, zone);
                x += w + 1;
                spans.push(Span::styled(label, style));
                spans.push(Span::raw("|"));
            }
            spans.push(Span::styled("  [/] or 1-6 to switch", Style::default().fg(THEME.text_muted)));
        }
        f.render_widget(Paragraph::new(Line::from(spans)), area);
    }

    // ---- view 1: list ----

    fn draw_list(&mut self, f: &mut Frame, area: Rect) {
        let rows = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Length(3), // lookup + load
                Constraint::Length(1), // signed-in tech
                Constraint::Length(2), // detected banner
                Constraint::Length(2), // recent header
                Constraint::Fill(1),   // recent table
                Constraint::Length(1), // error
            ])
            .split(area);

        let look = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([Constraint::Min(20), Constraint::Length(12)])
            .split(rows[0]);
        self.key_field.render_ref(look[0], f.buffer_mut());
        self.load_btn.render_ref(look[1], f.buffer_mut());

        let tech_line = match self.tech.as_ref() {
            Some(t) => Line::from(Span::styled(
                format!("Signed in: {}", t.name),
                Style::default().fg(THEME.success),
            )),
            None => Line::from(Span::styled(
                "Not signed in (sign in from the Sign-off view).",
                Style::default().fg(THEME.text_muted),
            )),
        };
        f.render_widget(Paragraph::new(tech_line), rows[1]);

        if self.resolve_busy {
            f.render_widget(
                Paragraph::new("Detecting order from machine serial…")
                    .style(Style::default().fg(THEME.text_muted)),
                rows[2],
            );
        } else if let Some(s) = self.resolved.as_ref() {
            let cust = if s.customer_name.is_empty() {
                String::new()
            } else {
                format!(" ({})", s.customer_name)
            };
            f.render_widget(
                Paragraph::new(format!("Detected on this machine: {}{} — Enter to load", s.reference, cust))
                    .wrap(Wrap { trim: true })
                    .style(Style::default().fg(THEME.success)),
                rows[2],
            );
        }

        let head = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([Constraint::Min(20), Constraint::Length(12)])
            .split(rows[3]);
        f.render_widget(
            Paragraph::new(Line::from(Span::styled(
                "Recent build-intake orders (Up/Down + Enter to load)",
                Style::default().fg(THEME.text).add_modifier(Modifier::BOLD),
            ))),
            head[0],
        );
        self.recent_refresh_btn.render_ref(head[1], f.buffer_mut());

        let block = Block::default()
            .borders(Borders::ALL)
            .border_style(THEME.border(false));
        let table_area = block.inner(rows[4]);
        f.render_widget(block, rows[4]);

        match self.recent.as_ref() {
            None => {
                let msg = if self.recent_busy { "Loading…" } else { "" };
                f.render_widget(
                    Paragraph::new(msg).style(Style::default().fg(THEME.text_muted)),
                    table_area,
                );
            }
            Some(Err(e)) => {
                f.render_widget(
                    Paragraph::new(e.as_str())
                        .wrap(Wrap { trim: true })
                        .style(Style::default().fg(THEME.error)),
                    table_area,
                );
            }
            Some(Ok(orders)) if orders.is_empty() => {
                f.render_widget(
                    Paragraph::new("No orders in Order Placed or Ready to Build right now.")
                        .style(Style::default().fg(THEME.text_muted)),
                    table_area,
                );
            }
            Some(Ok(orders)) => {
                let header = Row::new(vec!["Order", "Status", "Customer", "Build", "Serials", "Placed"])
                    .style(Style::default().fg(THEME.text_muted).add_modifier(Modifier::BOLD));
                let body: Vec<Row> = orders
                    .iter()
                    .enumerate()
                    .map(|(i, o)| {
                        let label = if o.reference.is_empty() { &o.id } else { &o.reference };
                        let row = Row::new(vec![
                            Cell::from(label.clone()),
                            Cell::from(o.status.name.clone()),
                            Cell::from(o.customer_name.clone()),
                            Cell::from(o.model.clone()),
                            Cell::from(format!("{}/{}", o.attached_serials, o.expected_serials)),
                            Cell::from(super::short_date(o.created_at.as_deref())),
                        ]);
                        if i == self.recent_sel {
                            row.style(Style::default().bg(THEME.surface).fg(THEME.accent))
                        } else {
                            row.style(Style::default().fg(THEME.text))
                        }
                    })
                    .collect();
                let n = orders.len();
                let table = Table::new(
                    body,
                    [
                        Constraint::Length(10),
                        Constraint::Length(16),
                        Constraint::Min(12),
                        Constraint::Length(16),
                        Constraint::Length(8),
                        Constraint::Length(11),
                    ],
                )
                .header(header);
                f.render_widget(table, table_area);
                for i in 0..n {
                    let y = table_area.y + 1 + i as u16;
                    if y >= table_area.bottom() {
                        break;
                    }
                    self.zones.add(
                        Rect { x: table_area.x, y, width: table_area.width, height: 1 },
                        format!("recent:{i}"),
                    );
                }
            }
        }

        if let Some(e) = self.error.as_ref() {
            f.render_widget(
                Paragraph::new(e.as_str())
                    .wrap(Wrap { trim: true })
                    .style(Style::default().fg(THEME.error)),
                rows[5],
            );
        }
    }

    // ---- view 2: order ----

    fn draw_order(&mut self, f: &mut Frame, area: Rect) {
        let rows = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Length(6), // header card
                Constraint::Length(4), // gate banner
                Constraint::Min(6),    // items
                Constraint::Fill(1),   // spec check
            ])
            .split(area);

        self.draw_header_card(f, rows[0]);
        self.draw_gate_banner(f, rows[1]);
        self.draw_items(f, rows[2]);
        self.draw_spec_check(f, rows[3]);
    }

    fn draw_header_card(&self, f: &mut Frame, area: Rect) {
        let Some(session) = self.session.as_ref() else { return };
        let order = &session.order;
        let photos = &session.photos;
        let block = Block::default()
            .borders(Borders::ALL)
            .border_style(THEME.border(false))
            .title("Order");
        let inner = block.inner(area);
        f.render_widget(block, area);

        let mut lines: Vec<Line> = Vec::new();
        let mut head: Vec<Span> = vec![Span::styled(
            format!("Order {}", order.id),
            Style::default().fg(THEME.text).add_modifier(Modifier::BOLD),
        )];
        if !order.reference.is_empty() && order.reference != order.id {
            head.push(Span::styled(format!("  ({})", order.reference), Style::default().fg(THEME.text_muted)));
        }
        head.push(Span::styled(format!("  {}", order.kind.as_str()), Style::default().fg(THEME.warning)));
        if let Some(b) = order.backend {
            head.push(Span::styled(format!("  {}", b.as_str()), Style::default().fg(THEME.text_muted)));
        }
        lines.push(Line::from(head));

        let mut meta: Vec<String> = Vec::new();
        if !order.customer_name.is_empty() {
            meta.push(order.customer_name.clone());
        }
        if !order.total_paid.is_empty() {
            meta.push(format!("Total ${}", order.total_paid));
        }
        if let Some(doc) = order.everest_doc.as_ref() {
            meta.push(format!("Everest {doc}"));
        }
        if let Some(serial) = order.build_serial.as_ref() {
            meta.push(serial.clone());
        }
        if let Some(parent) = order.parent_order_id.as_ref() {
            meta.push(format!("Parent {parent}"));
        }
        if !meta.is_empty() {
            lines.push(Line::from(Span::styled(meta.join("  ·  "), Style::default().fg(THEME.text))));
        }
        if let Some(config) = order.config.as_ref() {
            lines.push(Line::from(Span::styled(
                format!("Config: {}", config.name),
                Style::default().fg(THEME.text_muted),
            )));
        }
        if let Some(svc) = order.service_info.as_ref() {
            lines.push(Line::from(Span::styled(
                format!(
                    "Device: {} {} {} (SN {})",
                    svc.device_mfg, svc.device_name, svc.device_model, svc.device_serial
                ),
                Style::default().fg(THEME.text_muted),
            )));
        }
        let photo_line = if photos.present {
            Span::styled(format!("{} build photo(s)", photos.count), Style::default().fg(THEME.success))
        } else {
            Span::styled(
                "No build photo on order — upload before sign-off",
                Style::default().fg(THEME.error),
            )
        };
        lines.push(Line::from(photo_line));

        f.render_widget(Paragraph::new(lines).wrap(Wrap { trim: true }), inner);
    }

    fn draw_gate_banner(&mut self, f: &mut Frame, area: Rect) {
        let Some(session) = self.session.as_ref() else { return };
        let gate = &session.gate;
        let backend = session.order.backend;
        let (color, icon) = match gate.outcome {
            GateOutcome::GoodToMove { .. } => (THEME.success, "OK"),
            GateOutcome::RefuseToMove => (THEME.error, "X"),
            GateOutcome::Neutral => (THEME.warning, "!"),
        };
        let advance_target = gate.advance_target();

        let block = Block::default().borders(Borders::ALL).border_style(Style::default().fg(color));
        let inner = block.inner(area);
        f.render_widget(block, area);

        let rows = Layout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Length(1), Constraint::Length(1), Constraint::Fill(1)])
            .split(inner);

        f.render_widget(
            Paragraph::new(Line::from(Span::styled(
                format!("[{icon}] {} ({})", gate.status_name, gate.status_legacy_id),
                Style::default().fg(color).add_modifier(Modifier::BOLD),
            ))),
            rows[0],
        );
        f.render_widget(
            Paragraph::new(gate.message.clone())
                .wrap(Wrap { trim: true })
                .style(Style::default().fg(THEME.text)),
            rows[1],
        );

        if let Some(target) = advance_target {
            let cols = Layout::default()
                .direction(Direction::Horizontal)
                .constraints([Constraint::Length(14), Constraint::Min(0)])
                .split(rows[2]);
            let ps = backend == Some(BackendKind::Prestashop);
            self.advance_btn.set_disabled(self.advance_busy || !ps);
            self.advance_btn.render_ref(cols[0], f.buffer_mut());
            let note = if backend == Some(BackendKind::Shopify) {
                format!("Advance to {} ({target}) flows through the Worker (W7).", database::orders::gate::status_display(target, ""))
            } else {
                format!("Advance to {} ({target})", database::orders::gate::status_display(target, ""))
            };
            f.render_widget(
                Paragraph::new(note)
                    .wrap(Wrap { trim: true })
                    .style(Style::default().fg(THEME.text_muted)),
                cols[1].inner(Margin { horizontal: 1, vertical: 0 }),
            );
        }
        if let Some((ok, msg)) = self.advance_result.as_ref() {
            let c = if *ok { THEME.success } else { THEME.error };
            f.render_widget(
                Paragraph::new(msg.as_str()).wrap(Wrap { trim: true }).style(Style::default().fg(c)),
                rows[2],
            );
        }
    }

    fn draw_items(&self, f: &mut Frame, area: Rect) {
        let Some(session) = self.session.as_ref() else { return };
        let items = &session.order.items;
        let is_shopify = session.order.backend == Some(BackendKind::Shopify);

        let block = Block::default()
            .borders(Borders::ALL)
            .border_style(THEME.border(false))
            .title(if is_shopify { "Items & serials  (h: serial history)" } else { "Items & serials" });
        let inner = block.inner(area);
        f.render_widget(block, area);

        if items.is_empty() {
            f.render_widget(
                Paragraph::new("No line items on this order.").style(Style::default().fg(THEME.text_muted)),
                inner,
            );
            return;
        }

        let split = Layout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Fill(1), Constraint::Length(self.serial_history_height())])
            .split(inner);

        let header = Row::new(vec!["", "Item", "Ref", "Qty", "Serial"])
            .style(Style::default().fg(THEME.text_muted).add_modifier(Modifier::BOLD));
        let body: Vec<Row> = items
            .iter()
            .map(|item| {
                let (glyph, gcolor) = if item.serial_attached() {
                    ("OK", THEME.success)
                } else {
                    ("--", THEME.warning)
                };
                let serial = if item.serials.is_empty() {
                    "—".to_string()
                } else {
                    item.serials.join(", ")
                };
                Row::new(vec![
                    Cell::from(glyph).style(Style::default().fg(gcolor)),
                    Cell::from(item.name.clone()),
                    Cell::from(item.reference.clone()),
                    Cell::from(format!("{:.0}", item.quantity)),
                    Cell::from(serial),
                ])
            })
            .collect();
        let table = Table::new(
            body,
            [
                Constraint::Length(3),
                Constraint::Min(14),
                Constraint::Length(14),
                Constraint::Length(4),
                Constraint::Min(14),
            ],
        )
        .header(header)
        .style(Style::default().fg(THEME.text));
        f.render_widget(table, split[0]);
        if is_shopify {
            for i in 0..items.len() {
                let y = split[0].y + 1 + i as u16;
                if y >= split[0].bottom() {
                    break;
                }
                self.zones.add(
                    Rect { x: split[0].x, y, width: split[0].width, height: 1 },
                    format!("item:{i}"),
                );
            }
        }

        self.draw_serial_history(f, split[1]);
    }

    fn serial_history_height(&self) -> u16 {
        if self.serial_history.is_empty() && self.serial_busy.is_none() {
            0
        } else {
            (self.serial_history.len() as u16 + 2).min(8)
        }
    }

    fn draw_serial_history(&self, f: &mut Frame, area: Rect) {
        if area.height == 0 {
            return;
        }
        let mut lines: Vec<Line> = Vec::new();
        if let Some(busy) = self.serial_busy.as_ref() {
            lines.push(Line::from(Span::styled(
                format!("looking up {busy}…"),
                Style::default().fg(THEME.text_muted),
            )));
        }
        for (serial, result) in &self.serial_history {
            match result {
                Ok(h) => {
                    let mut parts = vec![Span::styled(
                        serial.clone(),
                        Style::default().fg(THEME.text).add_modifier(Modifier::BOLD),
                    )];
                    if !h.found {
                        parts.push(Span::styled(" not found", Style::default().fg(THEME.warning)));
                    }
                    if let Some(o) = h.current_order.as_ref() {
                        parts.push(Span::styled(format!(" installed on {o}"), Style::default().fg(THEME.text)));
                    }
                    if let Some(lot) = h.odoo_lot.as_ref() {
                        parts.push(Span::styled(format!(" Odoo: {lot}"), Style::default().fg(THEME.text_muted)));
                    }
                    if h.prestashop_allocations > 0 {
                        parts.push(Span::styled(
                            format!(" PS allocs: {}", h.prestashop_allocations),
                            Style::default().fg(THEME.text_muted),
                        ));
                    }
                    lines.push(Line::from(parts));
                    for flag in &h.flags {
                        lines.push(Line::from(Span::styled(format!("  ! {flag}"), Style::default().fg(THEME.error))));
                    }
                }
                Err(e) => lines.push(Line::from(Span::styled(
                    format!("{serial}: {e}"),
                    Style::default().fg(THEME.error),
                ))),
            }
        }
        let block = Block::default()
            .borders(Borders::TOP)
            .border_style(THEME.border(false))
            .title("Serial history");
        f.render_widget(Paragraph::new(lines).block(block).wrap(Wrap { trim: true }), area);
    }

    fn draw_spec_check(&mut self, f: &mut Frame, area: Rect) {
        let Some(session) = self.session.as_ref() else { return };
        let spec = &session.spec;

        let block = Block::default()
            .borders(Borders::ALL)
            .border_style(THEME.border(false))
            .title("Spec check");
        let inner = block.inner(area);
        f.render_widget(block, area);

        let rows = Layout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Length(3), Constraint::Fill(1)])
            .split(inner);

        let top = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([Constraint::Length(18), Constraint::Min(0)])
            .split(rows[0]);
        let snap_ready = self.ctx.lock().ok().and_then(|g| g.snapshot.clone())
            .map(|s| s.is_populated()).unwrap_or(false);
        self.spec_run_btn.set_disabled(!snap_ready || spec.is_empty());
        self.spec_run_btn.render_ref(top[0], f.buffer_mut());

        let summary = match self.spec_report.as_ref() {
            Some(r) if r.matched() => {
                Span::styled("Spec matches detected hardware", Style::default().fg(THEME.success))
            }
            Some(r) => Span::styled(
                format!("{} mismatch(es)", r.mismatch_count()),
                Style::default().fg(THEME.error),
            ),
            None if spec.is_empty() => Span::styled(
                "No hardware spec could be derived from this order.",
                Style::default().fg(THEME.text_muted),
            ),
            None => Span::styled(&spec.model, Style::default().fg(THEME.text)),
        };
        f.render_widget(
            Paragraph::new(Line::from(summary)).wrap(Wrap { trim: true }),
            top[1].inner(Margin { horizontal: 1, vertical: 1 }),
        );

        match self.spec_report.as_ref() {
            Some(report) => {
                let header = Row::new(vec!["Component", "Expected", "Detected", "Status"])
                    .style(Style::default().fg(THEME.text_muted).add_modifier(Modifier::BOLD));
                let body: Vec<Row> = report
                    .rows
                    .iter()
                    .map(|r| {
                        let color = match r.status {
                            crate::spec_check::CheckStatus::Match => THEME.success,
                            crate::spec_check::CheckStatus::Mismatch => THEME.error,
                            crate::spec_check::CheckStatus::NotDetected => THEME.warning,
                            crate::spec_check::CheckStatus::NotSpecified => THEME.text_muted,
                        };
                        let expected =
                            if r.expected.is_empty() { "(not on order)".to_string() } else { r.expected.clone() };
                        let detected =
                            if r.detected.is_empty() { "(not detected)".to_string() } else { r.detected.clone() };
                        Row::new(vec![
                            Cell::from(r.component.clone()),
                            Cell::from(expected),
                            Cell::from(detected),
                            Cell::from(r.status.label()).style(Style::default().fg(color)),
                        ])
                    })
                    .collect();
                let table = Table::new(
                    body,
                    [
                        Constraint::Length(12),
                        Constraint::Min(14),
                        Constraint::Min(14),
                        Constraint::Length(12),
                    ],
                )
                .header(header)
                .style(Style::default().fg(THEME.text));
                f.render_widget(table, rows[1]);
            }
            None if !spec.is_empty() => {
                let mut lines: Vec<Line> = Vec::new();
                for (label, value) in
                    [("CPU", spec.cpu.as_str()), ("GPU", spec.gpu.as_str()), ("RAM", spec.ram.as_str())]
                {
                    if !value.is_empty() {
                        lines.push(Line::from(vec![
                            Span::styled(format!("{label:<8}"), Style::default().fg(THEME.text_muted)),
                            Span::styled(value.to_string(), Style::default().fg(THEME.text)),
                        ]));
                    }
                }
                for drive in &spec.drives {
                    lines.push(Line::from(vec![
                        Span::styled(format!("{:<8}", drive.kind), Style::default().fg(THEME.text_muted)),
                        Span::styled(drive.name.clone(), Style::default().fg(THEME.text)),
                    ]));
                }
                f.render_widget(Paragraph::new(lines).wrap(Wrap { trim: true }), rows[1]);
            }
            None => {}
        }
    }

    // ---- view 3: sign-off ----

    fn draw_signoff(&mut self, f: &mut Frame, area: Rect) {
        let is_repair = self.is_repair();
        let is_shopify = self.is_shopify();
        let secret = if is_shopify { "PIN unused on Shopify (name-only)" } else { "password required" };

        let mut constraints = vec![
            Constraint::Length(1), // tech label
            Constraint::Length(3), // tech email
            Constraint::Length(3), // tech pass + signin
            Constraint::Length(1), // tech status
        ];
        if is_repair {
            constraints.extend([Constraint::Length(1), Constraint::Length(3), Constraint::Length(3), Constraint::Length(1)]);
        }
        constraints.push(Constraint::Length(1)); // influencer toggle
        if self.is_influencer {
            constraints.extend([
                Constraint::Length(3), Constraint::Length(3), Constraint::Length(1),
                Constraint::Length(3), Constraint::Length(3), Constraint::Length(1),
            ]);
        }
        constraints.push(Constraint::Fill(1));
        let rows = Layout::default().direction(Direction::Vertical).constraints(constraints).split(area);

        let mut i = 0;
        f.render_widget(
            Paragraph::new(Line::from(Span::styled(
                format!("QC technician ({secret})"),
                Style::default().fg(THEME.text).add_modifier(Modifier::BOLD),
            ))),
            rows[i],
        );
        i += 1;
        i = self.draw_auth_slot(
            f, &rows, i, AuthRole::Tech, self.tech.as_ref(), self.auth_busy, self.auth_error.as_deref(),
        );

        if is_repair {
            f.render_widget(
                Paragraph::new(Line::from(Span::styled(
                    "2nd sign-off (repair)",
                    Style::default().fg(THEME.warning).add_modifier(Modifier::BOLD),
                ))),
                rows[i],
            );
            i += 1;
            i = self.draw_auth_slot(
                f, &rows, i, AuthRole::Signoff, self.signoff.as_ref(), self.signoff_busy, self.signoff_error.as_deref(),
            );
        }

        let infl = if self.is_influencer { "[x]" } else { "[ ]" };
        f.render_widget(
            Paragraph::new(Line::from(Span::styled(
                format!("{infl} Influencer build (press 'i' to toggle)"),
                Style::default().fg(THEME.tertiary),
            ))),
            rows[i],
        );
        i += 1;

        if self.is_influencer {
            i = self.draw_auth_slot(
                f, &rows, i, AuthRole::Marketing, self.marketing.as_ref(), self.marketing_busy, self.marketing_error.as_deref(),
            );
            let _ = self.draw_auth_slot(
                f, &rows, i, AuthRole::Executive, self.executive.as_ref(), self.executive_busy, self.executive_error.as_deref(),
            );
        }
    }

    /// Renders an email field, a password+sign-in row, and an error line for an
    /// auth slot; returns the next row index.
    fn draw_auth_slot(
        &self,
        f: &mut Frame,
        rows: &[Rect],
        mut i: usize,
        role: AuthRole,
        identity: Option<&database::orders::TechIdentity>,
        busy: bool,
        error: Option<&str>,
    ) -> usize {
        let (email, pass, btn) = match role {
            AuthRole::Tech => (&self.tech_email, &self.tech_pass, &self.tech_signin_btn),
            AuthRole::Signoff => (&self.signoff_email, &self.signoff_pass, &self.signoff_signin_btn),
            AuthRole::Marketing => (&self.marketing_email, &self.marketing_pass, &self.mkt_signin_btn),
            AuthRole::Executive => (&self.executive_email, &self.executive_pass, &self.exec_signin_btn),
        };

        if let Some(t) = identity {
            f.render_widget(
                Paragraph::new(Line::from(Span::styled(
                    format!("  signed: {} ({})", t.name, t.id_employee),
                    Style::default().fg(THEME.success),
                ))),
                rows[i],
            );
            i += 1;
            // Skip the pass row + error row for a filled slot.
            i += 2;
            return i;
        }

        email.render_ref(rows[i], f.buffer_mut());
        i += 1;
        let cols = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([Constraint::Length(20), Constraint::Length(12), Constraint::Min(0)])
            .split(rows[i]);
        pass.render_ref(cols[0], f.buffer_mut());
        btn.set_disabled(busy);
        btn.render_ref(cols[1], f.buffer_mut());
        i += 1;
        if let Some(e) = error {
            f.render_widget(
                Paragraph::new(e).wrap(Wrap { trim: true }).style(Style::default().fg(THEME.error)),
                rows[i],
            );
        }
        i += 1;
        i
    }

    // ---- view 4: report ----

    fn draw_report(&mut self, f: &mut Frame, area: Rect) {
        let rows = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Length(1), // last run summary
                Constraint::Length(1), // prior signoff
                Constraint::Length(1), // air-cooled + progress
                Constraint::Fill(1),   // checklist
                Constraint::Length(3), // notes
                Constraint::Length(3), // submit
                Constraint::Length(2), // result
            ])
            .split(area);

        let run_line = {
            let guard = self.ctx.lock();
            match guard {
                Ok(g) => match g.last_verdict.as_ref() {
                    Some(v) => {
                        use database::schema::RunResult;
                        let (c, label) = match v.result {
                            RunResult::Pass => (THEME.success, "PASS"),
                            RunResult::Fail => (THEME.error, "FAIL"),
                            _ => (THEME.warning, "OTHER"),
                        };
                        Line::from(vec![
                            Span::styled("Last stress run: ", Style::default().fg(THEME.text)),
                            Span::styled(label, Style::default().fg(c).add_modifier(Modifier::BOLD)),
                            Span::styled(
                                format!(
                                    "  {:.0}s · WHEA {} · TDR {} · errs {}",
                                    v.duration_secs,
                                    v.summary.whea_delta_count,
                                    v.summary.tdr_count,
                                    v.summary.memory_errors + v.summary.disk_io_errors
                                ),
                                Style::default().fg(THEME.text_muted),
                            ),
                        ])
                    }
                    None => Line::from(Span::styled(
                        "No stress run this session — report will submit as not_run.",
                        Style::default().fg(THEME.warning),
                    )),
                },
                Err(_) => Line::from(""),
            }
        };
        f.render_widget(Paragraph::new(run_line), rows[0]);

        if let Some(summary) = self.prior_signoff.as_ref() {
            f.render_widget(
                Paragraph::new(format!("Already signed off — {summary} (press 'n' for new sign-off)"))
                    .wrap(Wrap { trim: true })
                    .style(Style::default().fg(THEME.success)),
                rows[1],
            );
        }

        let (resolved, total) = self.checklist.open_count();
        let air = if self.air_cooled { "[x]" } else { "[ ]" };
        let prog_color = if resolved == total { THEME.success } else { THEME.warning };
        let air_text = format!("{air} Air-cooled (a)  ");
        self.zones.add(
            Rect { x: rows[2].x, y: rows[2].y, width: air_text.len() as u16, height: 1 },
            "air",
        );
        f.render_widget(
            Paragraph::new(Line::from(vec![
                Span::styled(air_text, Style::default().fg(THEME.tertiary)),
                Span::styled(format!("{resolved}/{total} resolved"), Style::default().fg(prog_color)),
            ])),
            rows[2],
        );

        self.draw_checklist(f, rows[3]);

        self.report_notes.render_ref(rows[4], f.buffer_mut());

        let complete = self.checklist.is_complete();
        let can_submit = !self.submit_busy && self.tech.is_some() && complete;
        self.submit_btn.set_disabled(!can_submit);
        self.submit_btn.render_ref(rows[5].shrink(2, 0), f.buffer_mut());

        if let Some((ok, msg)) = self.submit_result.as_ref() {
            let c = if *ok { THEME.success } else { THEME.error };
            f.render_widget(
                Paragraph::new(msg.as_str()).wrap(Wrap { trim: true }).style(Style::default().fg(c)),
                rows[6],
            );
        } else if self.tech.is_none() {
            f.render_widget(
                Paragraph::new("Sign in to submit.").style(Style::default().fg(THEME.text_muted)),
                rows[6],
            );
        } else if !complete {
            f.render_widget(
                Paragraph::new("Finish every applicable item to submit.")
                    .style(Style::default().fg(THEME.text_muted)),
                rows[6],
            );
        }
    }

    fn draw_checklist(&self, f: &mut Frame, area: Rect) {
        let block = Block::default()
            .borders(Borders::ALL)
            .border_style(THEME.border(false))
            .title("Checklist  (Up/Down focus · click P/F/N · Enter cycle · l/p/n keys)");
        let inner = block.inner(area);
        f.render_widget(block, area);

        let active = self.checklist.first_incomplete();
        let flat = self.flat_items();

        // Build display lines, recording the line index of each item's status row.
        let mut lines: Vec<Line> = Vec::new();
        let mut item_rows: Vec<(usize, String, ItemStatus)> = Vec::new();
        for (sidx, sec) in self.checklist.sections.iter().enumerate() {
            let locked = active.map(|a| sidx > a).unwrap_or(false);
            let header = format!("§{} {}  ({})", sec.number, sec.title, sec.progress_text());
            if locked {
                lines.push(Line::from(Span::styled(format!("[locked] {header}"), Style::default().fg(THEME.text_muted))));
                continue;
            }
            lines.push(Line::from(Span::styled(header, Style::default().fg(THEME.accent).add_modifier(Modifier::BOLD))));
            if !sec.applicable {
                lines.push(Line::from(Span::styled("  N/A (air-cooled)", Style::default().fg(THEME.text_muted))));
                continue;
            }
            for item in &sec.items {
                let status = item.status();
                let focused = flat.get(self.focus).map(|k| k.as_str()) == Some(item.key.as_str());
                let marker = if focused { ">" } else { " " };
                let label_color = if self.blocked_keys.contains(&item.key) {
                    THEME.warning
                } else {
                    THEME.text
                };
                item_rows.push((lines.len(), item.key.clone(), status));
                let mut spans = vec![
                    Span::styled(format!("{marker} "), Style::default().fg(THEME.accent)),
                    Span::styled(cell_label(ItemStatus::Pass, status), Style::default().fg(status_color(ItemStatus::Pass))),
                    Span::styled(cell_label(ItemStatus::Fail, status), Style::default().fg(status_color(ItemStatus::Fail))),
                    Span::styled(cell_label(ItemStatus::Na, status), Style::default().fg(status_color(ItemStatus::Na))),
                    Span::styled(format!(" {}", item.text), Style::default().fg(label_color)),
                ];
                if item.auto_verified() {
                    spans.push(Span::styled(" auto✓", Style::default().fg(THEME.success)));
                }
                lines.push(Line::from(spans));
                if item.show_note() {
                    lines.push(Line::from(Span::styled(
                        format!("    note: {}", if item.note.is_empty() { "(required for Fail)" } else { &item.note }),
                        Style::default().fg(if item.note.is_empty() { THEME.warning } else { THEME.text_muted }),
                    )));
                }
                if item.captures_value && !item.value.is_empty() {
                    lines.push(Line::from(Span::styled(
                        format!("    value: {}", item.value),
                        Style::default().fg(THEME.text_muted),
                    )));
                }
                if focused && item.auto_verified() && !item.evidence.is_empty() {
                    lines.push(Line::from(Span::styled(
                        format!("    {}", item.evidence),
                        Style::default().fg(THEME.text_muted),
                    )));
                }
            }
        }
        // Scroll so the focused item stays visible.
        let scroll = self.checklist_scroll(&lines, inner.height);
        f.render_widget(Paragraph::new(lines).scroll((scroll, 0)), inner);

        // Status-cell zones on each item's first row. Cells follow the 2-col marker.
        for (line_idx, key, _) in &item_rows {
            let li = *line_idx as u16;
            if li < scroll {
                continue;
            }
            let y = inner.y + (li - scroll);
            if y >= inner.bottom() {
                continue;
            }
            let p_x = inner.x + 2;
            self.zones.add(Rect { x: p_x, y, width: 3, height: 1 }, format!("chk:{key}:pass"));
            self.zones.add(Rect { x: p_x + 3, y, width: 3, height: 1 }, format!("chk:{key}:fail"));
            self.zones.add(Rect { x: p_x + 6, y, width: 5, height: 1 }, format!("chk:{key}:na"));
        }
    }

    fn checklist_scroll(&self, lines: &[Line], height: u16) -> u16 {
        let total = lines.len() as u16;
        if total <= height {
            return 0;
        }
        total.saturating_sub(height)
    }

    // ---- view 5: comments ----

    fn draw_comments(&mut self, f: &mut Frame, area: Rect) {
        let rows = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Length(1), // header
                Constraint::Fill(1),   // bubbles
                Constraint::Length(3), // input
                Constraint::Length(2), // status
            ])
            .split(area);

        let count = self.session.as_ref().map(|s| s.comments.len()).unwrap_or(0);
        let head = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([Constraint::Min(10), Constraint::Length(12)])
            .split(rows[0]);
        f.render_widget(
            Paragraph::new(format!("{count} comment(s)")).style(Style::default().fg(THEME.text_muted)),
            head[0],
        );
        self.comment_refresh_btn.render_ref(head[1], f.buffer_mut());

        let own = self.tech.as_ref().map(|t| t.id_employee.clone());
        let mut lines: Vec<Line> = Vec::new();
        if let Some(session) = self.session.as_ref() {
            for c in &session.comments {
                let mine = own.is_some() && c.author_employee_id == own;
                let who_color = if mine { THEME.accent } else { THEME.tertiary };
                let mut hdr = vec![Span::styled(
                    format!("{}{}", if mine { "» " } else { "" }, c.author),
                    Style::default().fg(who_color).add_modifier(Modifier::BOLD),
                )];
                if c.private {
                    hdr.push(Span::styled(" [LOCK]", Style::default().fg(THEME.warning)));
                }
                if !c.created_at.is_empty() {
                    hdr.push(Span::styled(format!("  {}", c.created_at), Style::default().fg(THEME.text_muted)));
                }
                lines.push(Line::from(hdr));
                lines.push(Line::from(Span::styled(c.body.clone(), Style::default().fg(THEME.text))));
                lines.push(Line::from(""));
            }
            if session.comments.is_empty() {
                lines.push(Line::from(Span::styled("No comments on this order.", Style::default().fg(THEME.text_muted))));
            }
        }
        let block = Block::default().borders(Borders::ALL).border_style(THEME.border(false));
        let body_area = block.inner(rows[1]);
        f.render_widget(block, rows[1]);
        let scroll = self.checklist_scroll(&lines, body_area.height);
        f.render_widget(Paragraph::new(lines).wrap(Wrap { trim: true }).scroll((scroll, 0)), body_area);

        let input = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([Constraint::Min(10), Constraint::Length(10)])
            .split(rows[2]);
        self.comment_field.render_ref(input[0], f.buffer_mut());
        let can_send = !self.comment_busy && self.tech.is_some();
        self.comment_send_btn.set_disabled(!can_send);
        self.comment_send_btn.render_ref(input[1], f.buffer_mut());

        let status = if self.tech.is_none() {
            Some(("Sign in to post comments.".to_string(), THEME.text_muted))
        } else {
            self.comment_error.as_ref().map(|e| (e.clone(), THEME.error))
        };
        if let Some((msg, c)) = status {
            f.render_widget(
                Paragraph::new(msg).wrap(Wrap { trim: true }).style(Style::default().fg(c)),
                rows[3],
            );
        }
    }

    // ---- view 6: provision ----

    fn draw_provision(&mut self, f: &mut Frame, area: Rect) {
        let Some(order) = self.session.as_ref().map(|s| s.order.clone()) else { return };
        let company = self.prov_company.unwrap_or_else(|| Company::from_order(&order));
        let manifest = provisioning::load_manifest(company);

        let rows = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Length(1), // company line
                Constraint::Length(1), // hint
                Constraint::Fill(1),   // steps
                Constraint::Length(6), // log
            ])
            .split(area);

        let company_line = Line::from(vec![
            Span::styled("Company: ", Style::default().fg(THEME.text_muted)),
            Span::styled(company.label(), Style::default().fg(THEME.tertiary).add_modifier(Modifier::BOLD)),
            Span::styled(if self.prov_busy { "  (working…)" } else { "  (c / click: change)" }, Style::default().fg(THEME.text_muted)),
        ]);
        self.zones.add(rows[0], "prov:company");
        f.render_widget(Paragraph::new(company_line), rows[0]);
        f.render_widget(
            Paragraph::new("Step keys: 1 core-isolation · 2 timezone · 3 open-tools · 4 chipset · 5 display · 6 DMI (needs confirm)")
                .wrap(Wrap { trim: true })
                .style(Style::default().fg(THEME.text_muted)),
            rows[1],
        );

        let mut lines: Vec<Line> = Vec::new();
        let mut step_zones: Vec<(usize, &'static str)> = Vec::new();
        for step in &manifest.steps {
            let (label, kind) = match step.kind.as_str() {
                "core_isolation" => ("1) Enable core isolation", Some("core_isolation")),
                "timezone" => ("2) Set timezone (MST)", Some("timezone")),
                "open_tools" => ("3) Open system tools", Some("open_tools")),
                "chipset" => ("4) Install chipset driver", Some("chipset")),
                "display" => ("5) Install display driver", Some("display")),
                "dmi" => ("6) DMI / SMBIOS write", Some("dmi")),
                "branding" => ("Branding (.bat) — later phase", None),
                _ => continue,
            };
            if let Some(k) = kind {
                step_zones.push((lines.len(), k));
            }
            lines.push(Line::from(Span::styled(label, Style::default().fg(THEME.text))));
        }
        let mut dmi_confirm_idx: Option<usize> = None;
        if let Some(session) = self.session.as_ref() {
            let spec = &session.spec;
            if provisioning::dmi::is_threadripper(spec) {
                lines.push(Line::from(Span::styled("  DMI: Threadripper board — skipped.", Style::default().fg(THEME.warning))));
            } else if company.dmi_manufacturer().is_none() {
                lines.push(Line::from(Span::styled("  DMI: no manufacturer for this company — skipped.", Style::default().fg(THEME.text_muted))));
            } else {
                let confirm = if self.prov_dmi_confirm { "[x]" } else { "[ ]" };
                dmi_confirm_idx = Some(lines.len());
                lines.push(Line::from(Span::styled(
                    format!("  DMI confirm {confirm} (d / click to toggle; set tool path below)"),
                    Style::default().fg(THEME.text_muted),
                )));
            }
        }
        let steps_block = Block::default().borders(Borders::ALL).border_style(THEME.border(false)).title("Steps");
        let steps_inner = steps_block.inner(rows[2]);
        f.render_widget(steps_block, rows[2]);
        let step_rows = Layout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Fill(1), Constraint::Length(3)])
            .split(steps_inner);
        for (idx, kind) in step_zones {
            let y = step_rows[0].y + idx as u16;
            if y < step_rows[0].bottom() {
                self.zones.add(
                    Rect { x: step_rows[0].x, y, width: step_rows[0].width, height: 1 },
                    format!("prov:{kind}"),
                );
            }
        }
        if let Some(idx) = dmi_confirm_idx {
            let y = step_rows[0].y + idx as u16;
            if y < step_rows[0].bottom() {
                self.zones.add(
                    Rect { x: step_rows[0].x, y, width: step_rows[0].width, height: 1 },
                    "prov:dmi_confirm",
                );
            }
        }
        f.render_widget(Paragraph::new(lines).wrap(Wrap { trim: true }), step_rows[0]);
        self.prov_dmi_tool.render_ref(step_rows[1], f.buffer_mut());

        let mut log: Vec<Line> = Vec::new();
        for (s, ok, msg) in &self.prov_log {
            let c = if *ok { THEME.success } else { THEME.error };
            log.push(Line::from(Span::styled(format!("{s}: {msg}"), Style::default().fg(c))));
        }
        let log_block = Block::default().borders(Borders::ALL).border_style(THEME.border(false)).title("Log");
        f.render_widget(Paragraph::new(log).block(log_block).wrap(Wrap { trim: true }), rows[3]);
    }

    /// Flat list of checklist item keys in applicable, unlocked sections.
    fn flat_items(&self) -> Vec<String> {
        let active = self.checklist.first_incomplete();
        let mut out = Vec::new();
        for (sidx, sec) in self.checklist.sections.iter().enumerate() {
            if active.map(|a| sidx > a).unwrap_or(false) || !sec.applicable {
                continue;
            }
            for item in &sec.items {
                out.push(item.key.clone());
            }
        }
        out
    }

    // ---- keyboard ----

    pub(super) fn handle_key(&mut self, key: KeyEvent) -> bool {
        // Company dropdown captures keys while open.
        if self.prov_company_open {
            match key.code {
                KeyCode::Esc => self.prov_company_open = false,
                KeyCode::Down => self.prov_company_menu.select_next(),
                KeyCode::Up => self.prov_company_menu.select_prev(),
                KeyCode::Enter => {
                    if let Some(idx) = self.prov_company_menu.selected() {
                        if let Some(c) = Company::ALL.get(idx) {
                            self.prov_company = Some(*c);
                        }
                    }
                    self.prov_company_open = false;
                }
                _ => {}
            }
            return true;
        }

        // Active text field captures input until Esc/Enter releases it.
        if let Some(field_id) = self.active_field.clone() {
            match key.code {
                KeyCode::Esc => {
                    self.set_field_state(&field_id, ButtonState::Normal);
                    self.active_field = None;
                }
                KeyCode::Enter if field_id.0 != COMMENT_ID && field_id.0 != REPORT_NOTES_ID => {
                    self.set_field_state(&field_id, ButtonState::Normal);
                    self.active_field = None;
                    if field_id.0 == super::LOOKUP_ID {
                        self.start_load();
                    }
                }
                _ => self.route_key_to_field(&field_id, key),
            }
            return true;
        }

        match self.view {
            View::List => self.handle_list_key(key),
            View::Order => self.handle_order_key(key),
            View::SignOff => self.handle_signoff_key(key),
            View::Report => self.handle_report_key(key),
            View::Comments => self.handle_comments_key(key),
            View::Provision => self.handle_provision_key(key),
        }
        true
    }

    fn global_view_key(&mut self, key: KeyEvent) -> bool {
        match key.code {
            KeyCode::Char('[') => {
                self.switch_view(-1);
                true
            }
            KeyCode::Char(']') => {
                self.switch_view(1);
                true
            }
            KeyCode::Char('b') if self.session.is_some() => {
                self.back_to_list();
                true
            }
            KeyCode::Char(c @ '1'..='6') if self.session.is_some() => {
                let views = [View::Order, View::SignOff, View::Report, View::Comments, View::Provision];
                if let Some(idx) = (c as usize).checked_sub('1' as usize) {
                    if let Some(v) = views.get(idx) {
                        self.view = *v;
                        self.focus = 0;
                        self.active_field = None;
                        return true;
                    }
                }
                false
            }
            _ => false,
        }
    }

    fn handle_list_key(&mut self, key: KeyEvent) {
        match key.code {
            KeyCode::Up => self.recent_sel = self.recent_sel.saturating_sub(1),
            KeyCode::Down => {
                let n = self.recent_len();
                if n > 0 {
                    self.recent_sel = (self.recent_sel + 1).min(n - 1);
                }
            }
            KeyCode::Enter => {
                if let Some(input) = self.selected_recent_input() {
                    self.key_field.set_text(&input);
                    self.start_load();
                } else if !self.key_field.get_raw_text().trim().is_empty() {
                    self.start_load();
                } else if let Some(s) = self.resolved.as_ref() {
                    let input = s.lookup_input();
                    self.key_field.set_text(&input);
                    self.start_load();
                }
            }
            KeyCode::Char('e') => self.focus_field(super::LOOKUP_ID),
            _ => {}
        }
    }

    fn recent_len(&self) -> usize {
        match self.recent.as_ref() {
            Some(Ok(v)) => v.len(),
            _ => 0,
        }
    }

    fn selected_recent_input(&self) -> Option<String> {
        match self.recent.as_ref() {
            Some(Ok(v)) => v.get(self.recent_sel).map(|o| o.lookup_input()),
            _ => None,
        }
    }

    fn handle_order_key(&mut self, key: KeyEvent) {
        if self.global_view_key(key) {
            return;
        }
        match key.code {
            KeyCode::Char('h') if self.is_shopify() => {
                let serials: Vec<String> = self
                    .session
                    .as_ref()
                    .map(|s| s.order.items.iter().flat_map(|i| i.serials.clone()).collect())
                    .unwrap_or_default();
                for serial in serials {
                    self.start_serial_history(serial);
                }
            }
            KeyCode::Char('s') => self.run_spec_check_now(),
            KeyCode::Char('a') => {
                if let Some(target) = self.session.as_ref().and_then(|s| s.gate.advance_target()) {
                    let ps =
                        self.session.as_ref().and_then(|s| s.order.backend) == Some(BackendKind::Prestashop);
                    if ps && !self.advance_busy {
                        self.start_advance(target);
                    }
                }
            }
            _ => {}
        }
    }

    fn handle_signoff_key(&mut self, key: KeyEvent) {
        if self.global_view_key(key) {
            return;
        }
        match key.code {
            KeyCode::Char('i') => self.is_influencer = !self.is_influencer,
            KeyCode::Char('t') => self.focus_field(TECH_EMAIL_ID),
            KeyCode::Char('y') => self.focus_field(TECH_PASS_ID),
            KeyCode::Char('s') if self.is_repair() => self.focus_field(SIGNOFF_EMAIL_ID),
            KeyCode::Char('d') if self.is_repair() => self.focus_field(SIGNOFF_PASS_ID),
            KeyCode::Char('m') if self.is_influencer => self.focus_field(MKT_EMAIL_ID),
            KeyCode::Char(',') if self.is_influencer => self.focus_field(MKT_PASS_ID),
            KeyCode::Char('x') if self.is_influencer => self.focus_field(EXEC_EMAIL_ID),
            KeyCode::Char('.') if self.is_influencer => self.focus_field(EXEC_PASS_ID),
            KeyCode::Enter => self.start_auth(AuthRole::Tech),
            _ => {}
        }
    }

    fn handle_report_key(&mut self, key: KeyEvent) {
        if self.global_view_key(key) {
            return;
        }
        let flat = self.flat_items();
        match key.code {
            KeyCode::Up => self.focus = self.focus.saturating_sub(1),
            KeyCode::Down => {
                if !flat.is_empty() {
                    self.focus = (self.focus + 1).min(flat.len() - 1);
                }
            }
            KeyCode::Char('a') => self.toggle_air_cooled(),
            KeyCode::Char('n') if self.prior_signoff.is_some() => self.reset_signoff(),
            KeyCode::Char('l') | KeyCode::Char('P') => self.set_focused_status(flat, ItemStatus::Pass),
            KeyCode::Char('f') | KeyCode::Char('F') => self.set_focused_status(flat, ItemStatus::Fail),
            KeyCode::Char('/') | KeyCode::Char('N') => self.set_focused_status(flat, ItemStatus::Na),
            KeyCode::Enter => self.cycle_focused_status(flat),
            KeyCode::Char('o') => self.focus_field(REPORT_NOTES_ID),
            KeyCode::Char('S') => self.start_submit(),
            _ => {}
        }
    }

    fn set_focused_status(&mut self, flat: Vec<String>, status: ItemStatus) {
        if let Some(key) = flat.get(self.focus).cloned() {
            self.set_checklist_status(&key, status);
        }
    }

    fn cycle_focused_status(&mut self, flat: Vec<String>) {
        if let Some(key) = flat.get(self.focus).cloned() {
            let next = match self.checklist.item(&key).map(|i| i.status()) {
                Some(ItemStatus::Pass) => ItemStatus::Fail,
                Some(ItemStatus::Fail) => ItemStatus::Na,
                Some(ItemStatus::Na) => ItemStatus::Unset,
                _ => ItemStatus::Pass,
            };
            self.set_checklist_status(&key, next);
        }
    }

    fn reset_signoff(&mut self) {
        if let Some(s) = self.session.as_ref() {
            let machine = crate::reporting::machine_id();
            crate::checklist_store::clear(&s.order.id, &machine);
        }
        self.checklist = database::orders::ChecklistState::from_kind(self.checklist_kind);
        self.prior_signoff = None;
        self.verify_pending = true;
        self.submit_result = None;
    }

    fn handle_comments_key(&mut self, key: KeyEvent) {
        if self.global_view_key(key) {
            return;
        }
        match key.code {
            KeyCode::Char('r') => self.start_refresh_comments(),
            KeyCode::Char('e') => self.focus_field(COMMENT_ID),
            KeyCode::Enter => self.start_post_comment(),
            _ => {}
        }
    }

    fn handle_provision_key(&mut self, key: KeyEvent) {
        if self.global_view_key(key) {
            return;
        }
        match key.code {
            KeyCode::Char('c') => self.open_company_menu(),
            KeyCode::Char('d') => self.prov_dmi_confirm = !self.prov_dmi_confirm,
            KeyCode::Char('e') => self.focus_field(PROV_DMI_TOOL_ID),
            KeyCode::Char('1') => self.prov_step("core_isolation"),
            KeyCode::Char('2') => self.prov_step("timezone"),
            KeyCode::Char('3') => self.prov_step("open_tools"),
            KeyCode::Char('4') => self.prov_step("chipset"),
            KeyCode::Char('5') => self.prov_step("display"),
            KeyCode::Char('6') => self.prov_step("dmi"),
            _ => {}
        }
    }

    /// Run a provision manifest step by kind, mirroring the keyboard 1-6 keys.
    pub(super) fn prov_step(&mut self, kind: &str) {
        match kind {
            "company" => self.open_company_menu(),
            "dmi_confirm" => self.prov_dmi_confirm = !self.prov_dmi_confirm,
            "core_isolation" => {
                self.spawn_prov("Core isolation", provisioning::osconfig::enable_core_isolation)
            }
            "timezone" => self.spawn_prov("Timezone", provisioning::osconfig::set_timezone_mountain),
            "open_tools" => self.spawn_prov("Open tools", provisioning::osconfig::open_system_tools),
            "chipset" => {
                let path = crate::db::default_sqlite_path().to_string_lossy().to_string();
                self.spawn_prov("Chipset driver", move || provisioning::install_chipset(&path));
            }
            "display" => {
                let path = crate::db::default_sqlite_path().to_string_lossy().to_string();
                self.spawn_prov("Display driver", move || provisioning::install_display(&path));
            }
            "dmi" => self.run_dmi(),
            _ => {}
        }
    }

    fn run_dmi(&mut self) {
        let Some((order, spec)) = self.session.as_ref().map(|s| (s.order.clone(), s.spec.clone())) else {
            return;
        };
        let company = self.prov_company.unwrap_or_else(|| Company::from_order(&order));
        let manifest = provisioning::load_manifest(company);
        if provisioning::dmi::is_threadripper(&spec) || company.dmi_manufacturer().is_none() {
            return;
        }
        let tool = self.prov_dmi_tool.get_raw_text();
        if !self.prov_dmi_confirm || tool.trim().is_empty() || self.prov_busy {
            return;
        }
        let board = self.board_serial.clone().unwrap_or_default();
        let dctx = provisioning::dmi::DmiContext::build(&order, &spec, &manifest, &board, &board);
        let cmds = provisioning::dmi::ami_commands(&dctx);
        let tool = std::path::PathBuf::from(tool);
        self.spawn_prov("DMI write", move || provisioning::dmi::run(&tool, &cmds));
    }

    fn open_company_menu(&mut self) {
        let order = self.session.as_ref().map(|s| s.order.clone());
        let current = self
            .prov_company
            .unwrap_or_else(|| order.as_ref().map(Company::from_order).unwrap_or(Company::None));
        let items: Vec<mtech_tui::widgets::menu_item::MenuItem> = Company::ALL
            .iter()
            .map(|c| mtech_tui::widgets::menu_item::MenuItem::new(c.label()).active(*c == current))
            .collect();
        let anchor = Rect { x: 4, y: 4, width: 20, height: 1 };
        self.prov_company_menu.open_at(anchor, items, "Company");
        self.prov_company_open = true;
    }

    // ---- field focus + key routing ----

    fn focus_field(&mut self, id: &str) {
        let wid = WidgetId(id.to_string());
        self.set_field_state(&wid, ButtonState::Active);
        self.active_field = Some(wid);
    }

    fn field_ref(&self, id: &WidgetId) -> Option<&mtech_tui::widgets::input_field::InputField<'a>> {
        Some(match id.0.as_str() {
            super::LOOKUP_ID => &self.key_field,
            TECH_EMAIL_ID => &self.tech_email,
            TECH_PASS_ID => &self.tech_pass,
            SIGNOFF_EMAIL_ID => &self.signoff_email,
            SIGNOFF_PASS_ID => &self.signoff_pass,
            MKT_EMAIL_ID => &self.marketing_email,
            MKT_PASS_ID => &self.marketing_pass,
            EXEC_EMAIL_ID => &self.executive_email,
            EXEC_PASS_ID => &self.executive_pass,
            COMMENT_ID => &self.comment_field,
            REPORT_NOTES_ID => &self.report_notes,
            PROV_DMI_TOOL_ID => &self.prov_dmi_tool,
            _ => return None,
        })
    }

    fn set_field_state(&self, id: &WidgetId, state: ButtonState) {
        if let Some(field) = self.field_ref(id) {
            field.set_state(state);
        }
    }

    fn route_key_to_field(&self, id: &WidgetId, key: KeyEvent) {
        if let Some(field) = self.field_ref(id) {
            field.input.borrow_mut().input_without_shortcuts(key);
        }
    }
}
