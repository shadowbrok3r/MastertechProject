use yeehaw::*;
use std::collections::HashMap;

struct MastertechTerminal {
    service_fields: HashMap<String, TextBox>,
    hardware_fields: HashMap<String, TextBox>,
}

impl MastertechTerminal {
    fn new(ctx: &Context) -> Self {
        let mut service_fields = HashMap::new();
        let mut hardware_fields = HashMap::new();

        let service_keys = vec![
            "Service Number", "Customer Name", "Phone number", "Assignee", "Tech",
            "Checkin Notes", "Recommendations"
        ];
        for key in &service_keys {
            let height = if key.eq(&"Checkin Notes") || key.eq(&"Recommendations") {
                DynVal::new_fixed(3)
            } else {
                DynVal::new_fixed(1)
            };
            let txt_box = TextBox::new(ctx, "").editable(ctx)
                .with_dyn_width(DynVal::new_fixed(30))
                .with_dyn_height(height)
                // .with_bg(Color::new(8, 8, 12))
                // .set_content_style(sty)
                .with_styles(
                    SelStyles {
                        selected_style: Style::standard().with_fg(Color::WHITE).with_bg(Color::new(12,12,12)),
                        ready_style: Style::standard().with_fg(Color::AQUA).with_bg(Color::PURPLE),
                        unselectable_style: Style::standard().with_fg(Color::YELLOW).with_bg(Color::GREEN),
                    }
                ).with_cursor_style(Style::standard().with_fg(Color::AQUA));
            let pane = &mut txt_box.pane.clone();
            pane.clone().with_bg(Color::BLUE_VIOLET)
            .with_fg(Color::GREEN);

            service_fields.insert(
                key.to_string(),
                txt_box
                        // .with_focus(Color::ORCHID))
            );
        }

        let hardware_keys = vec![
            "HostName", "CPU Name", "Total RAM", "GPU",
            "Drive letter/Type/AvailableSpace"
        ];
        for key in &hardware_keys {
            hardware_fields.insert(
                key.to_string(),
                TextBox::new(ctx, "").editable(ctx)
                    .with_dyn_width(DynVal::new_fixed(30))
                    .with_dyn_height(DynVal::new_fixed(1))
                    .with_style(
                        Style::default()
                            .with_bg(Color::MEDIUM_TURQUOISE)
                            .with_fg(Color::BLACK)
                            // .with_fg(Color::CYAN)
                    )
            );
        }

        Self {
            service_fields,
            hardware_fields,
        }
    }
}

pub async fn run_terminal_mode() -> Result<(), Error> {

    let (mut tui, ctx) = Tui::new()?;
    let app = MastertechTerminal::new(&ctx);
    
    let tabs = Tabs::new(&ctx);
    
    let service_form = ParentPane::new(&ctx, "service_form")
        .with_dyn_width(1.0)
        .with_dyn_height(1.0)
        .with_bg(Color::new(8,8,12));
    
    let service_fields = vec![
        "Service Number", "Customer Name", "Phone number", "Assignee", "Tech",
        "Checkin Notes", "Recommendations"
    ];
    
    for (i, field) in service_fields.iter().enumerate() {
        let label = Label::new(&ctx, field)
            .at(1, i as i32 * 4)
            .with_style(Style::default().with_fg(Color::DARK_CYAN).with_bg(Color::TRANSPARENT));
        let input = app.service_fields.get(&field.to_string()).unwrap().clone().at(30, i as i32 * 4);
        service_form.add_element(Box::new(label));
        service_form.add_element(Box::new(input));
    }
    
    let submit_button = Button::new(&ctx, "Submit")
        .with_fn(Box::new(move |_, _| {
            println!("Form submitted!");
            EventResponses::default()
        }))
        .at(20, (service_fields.len() as i32) * 4 + 2)
        .pane.with_style(Style::default().with_bg(Color::SPRING_GREEN).with_fg(Color::BLACK));
    service_form.add_element(Box::new(submit_button));
    
    let hardware_info = ParentPane::new(&ctx, "hardware_info")
        .with_dyn_width(1.0)
        .with_dyn_height(1.0)
        .with_bg(Color::MEDIUM_SLATE_BLUE);
    
    let hardware_fields = vec![
        "HostName", "CPU Name", "Total RAM", "GPU",
        "Drive letter/Type/AvailableSpace"
    ];
    
    for (i, field) in hardware_fields.iter().enumerate() {
        let label = Label::new(&ctx, field)
            .at(1, i as i32 * 2)
            .with_style(Style::default().with_fg(Color::LIGHT_CYAN));
        let input = app.hardware_fields.get(&field.to_string()).unwrap().clone().at(30, i as i32 * 2);
        hardware_info.add_element(Box::new(label));
        hardware_info.add_element(Box::new(input));
    }
    
    tabs.push(Box::new(service_form), "Service Details");
    tabs.push(Box::new(hardware_info), "Hardware Info");
    tabs.select(0);
    
    tui.run(Box::new(tabs)).await?;
    Ok(())
}
