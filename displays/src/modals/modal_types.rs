use eframe::egui::{vec2, Align, Align2, Button, Color32, Context, Frame, Id, Key, LayerId, Layout, Margin, NumExt, Order, Painter, Pos2, Rect, RichText, Rounding, Shape, Stroke, Ui, Widget, Window};
use super::{ModalAction, ModalResponse, ModalState};

pub trait ModalTypes: Default{
    fn modal_state(&mut self) -> &mut ModalState;

    fn title(mut self, title: String) -> Self where Self: Sized {
        self.modal_state().title = Some(title);
        self
    }

    /// Set the minimum width of the modal window.
    fn min_width(mut self, min_width: f32) -> Self where Self: Sized {
        self.modal_state().min_width = Some(min_width);
        self
    }

    /// Set the minimum height of the modal window.
    fn min_height(mut self, min_height: f32) -> Self where Self: Sized {
        self.modal_state().min_height = Some(min_height);
        self
    }

    /// Set the default height of the modal window.
    fn default_height(mut self, default_height: f32) -> Self where Self: Sized {
        self.modal_state().default_height = Some(default_height);
        self
    }

    /// Configure the content area of the modal for full span highlighting.
    /// This includes:
    /// - setting the vertical spacing to 0.0
    /// - removing any padding at the bottom of the area
    /// In this mode, the user code is responsible for adding spacing between items.
    fn full_span_content(mut self, full_span_content: bool) -> Self where Self: Sized {
        self.modal_state().full_span_content = full_span_content;
        self
    }

    /// Show the modal window.
    /// Typically called by [`ModalHandler::ui`].
    fn ui<R>(&mut self, ctx: &Context, content_ui: impl FnOnce(&mut Ui, &mut bool, &mut ModalAction) -> R) -> ModalResponse<R> {
        // Implementation for showing the modal
        Self::dim_background(ctx);

        let mut open = ctx.input(|i| !i.key_pressed(Key::Escape));
        // let mut page_state = &;

        let screen_height = ctx.screen_rect().height();
        let _screen_width = ctx.screen_rect().width();
        let modal_vertical_margins = (75.0).at_most(screen_height * 0.1);

        let mut window = Window::new(&*self.modal_state().title.as_ref().unwrap())
            .frame(
                Frame::default()
                .inner_margin(Margin::symmetric(0.0, 0.0))
                .outer_margin(Margin::same(30.0))
                .stroke(Stroke::new(2.0, Color32::from_additive_luminance(150)))
                .fill(Color32::BLACK)
                .rounding(Rounding::same(15.0))
            )
            .pivot(Align2::CENTER_TOP)
            .fixed_pos(ctx.screen_rect().center_top() + vec2(0.0, modal_vertical_margins))
            .constrain_to(ctx.screen_rect())
            .max_height(600.0)
            .max_width(680.0)
            .default_width(680.0)
            .collapsible(false)
            .resizable(false)
            .title_bar(false);

        if let Some(min_width) = self.modal_state().min_width {
            window = window.min_width(min_width);
        }

        if let Some(min_height) = self.modal_state().min_height {
            window = window.min_height(min_height);
        }

        if let Some(default_height) = self.modal_state().default_height {
            window = window.default_height(default_height);
        }

        let response = window.show(ctx, |ui| {
            
            let item_spacing_y = ui.spacing().item_spacing.y;
            ui.spacing_mut().item_spacing.y = 0.0;

            Frame {
                inner_margin: Margin::same(0.0),
                ..Default::default()
            }
            .show(ui, |ui| {
                Self::title_bar(ui, &self.modal_state().title.as_ref().unwrap_or(&"Modal".to_string()), &mut open);
                ui.add_space(item_spacing_y);

                Frame {
                    inner_margin: Margin::same(0.0),
                    ..Default::default()
                }
                .show(ui, |ui| {
                    ui.spacing_mut().item_spacing.y = item_spacing_y;
                    content_ui(ui, &mut open, &mut self.modal_state().page_state)
                })
                .inner
            })
            .inner
        });

        let cursor_was_over_window = response
            .as_ref()
            .and_then(|response| {
                ctx.input(|i| i.pointer.interact_pos())
                    .map(|interact_pos| {
                        let pos_x = interact_pos.x;
                        let pos_y = interact_pos.y;
                        let final_pos = Pos2::new(pos_x - 10.0, pos_y - 10.0);
                        response.response.rect.contains(final_pos)
                    })
            })
            .unwrap_or(false);
        if !cursor_was_over_window && ctx.input(|i| i.pointer.any_pressed()) {
            open = false;
        }

        ModalResponse {
            inner: response.and_then(|response| response.inner),
            open,
            page_state: self.modal_state().page_state.clone()
        }
    }

    fn dim_background(ctx: &Context) {
        let painter = Painter::new(
            ctx.clone(),
            LayerId::new(Order::PanelResizeLine, Id::new("DimLayer")),
            Rect::EVERYTHING,
        );
        painter.add(Shape::rect_filled(
            ctx.screen_rect(),
            Rounding::ZERO,
            Color32::from_black_alpha(240),
        ));
    }

    fn title_bar(ui: &mut Ui, title: &str, open: &mut bool) {
        let t: RichText = RichText::new(title).heading().strong();
        Frame::default()
            .fill(Color32::from_rgb(20, 20, 25))
            .rounding(Rounding{nw: 15.0,ne: 15.0,sw: 0.0,se: 0.0})
            .inner_margin(Margin::same(0.0))
            .outer_margin(Margin::same(0.0))
            .show(ui, |ui| 
        {
            ui
            .with_layout(
                Layout::top_down(Align::Max), 
            |ui|{
                if Button::new(" X ").rounding(Rounding::same(10.0))
                .fill(Color32::BLACK)
                    .ui(ui)
                    .clicked(){
                        *open = false;
                    }
            });

            ui
            .with_layout(
                Layout::top_down(Align::Center), 
            |ui|ui.heading(t));
        });
        ui.separator();
    }
}