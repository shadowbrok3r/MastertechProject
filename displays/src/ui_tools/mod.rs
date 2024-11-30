use eframe::egui::{text::LayoutJob, Color32, FontId, TextFormat};

pub mod autocomplete;
pub mod carl_dark;
pub mod mention_handler;
pub mod toasts;
pub mod tokyo_dark;
pub mod theme_config;

/// Function to color text between two delimiters
pub fn color_between_delimiters(
    layout_job: &mut LayoutJob,
    text: &str,
    delimiters: (&str, &str),
    color: Color32,
) -> String {
    let mut remaining_text = String::from(text);

    while let Some(start_idx) = remaining_text.find(delimiters.0) {
        let after_start = start_idx + delimiters.0.len();
        if let Some(end_idx) = remaining_text[after_start..].find(delimiters.1) {
            let end_idx = after_start + end_idx;

            // Append the text before the first delimiter
            layout_job.append(&remaining_text[..start_idx], 0.0, TextFormat::default());

            // Append the first delimiter itself
            layout_job.append(delimiters.0, 0.0, TextFormat::default());

            // Append the text between the delimiters with the given color
            layout_job.append(
                &remaining_text[after_start..end_idx],
                0.0,
                TextFormat::simple(FontId::default(), color),
            );

            // Append the second delimiter
            layout_job.append(delimiters.1, 0.0, TextFormat::default());

            // Update remaining_text to the part after the second delimiter
            remaining_text = remaining_text[end_idx + delimiters.1.len()..].to_string();
        } else {
            break;
        }
    }

    remaining_text
}

pub fn highlight_text(
    text: &str,
    pattern: &str,
    delimiters: (&str, &str),
    pattern_color: Color32,
    delimiter_color: Color32,
    base_format: &TextFormat,
) -> LayoutJob {
    let mut job = LayoutJob::default();
    let mut i = 0;
    let text_len = text.len();

    while i < text_len {
        let remaining_text = &text[i..];

        // Check if remaining_text is empty before proceeding
        if remaining_text.is_empty() {
            break;
        }

        // Check if the text at position i matches the start delimiter
        if remaining_text.starts_with(delimiters.0) {
            // Append the start delimiter with base format
            let delimiter_len = delimiters.0.len();
            job.append(&text[i..i + delimiter_len], 0.0, base_format.clone());
            i += delimiter_len;

            // Ensure we don't go out of bounds
            if i >= text_len {
                break;
            }

            // Find the end delimiter
            let end_delimiter_pos = text[i..].find(delimiters.1);

            let end_idx = match end_delimiter_pos {
                Some(pos) => pos,
                None => text_len - i, // No end delimiter found, take the rest of the text
            };

            // Append the text between delimiters with delimiter color
            let mut format = base_format.clone();
            format.color = delimiter_color;

            // Ensure we don't go out of bounds
            if i + end_idx <= text_len {
                job.append(&text[i..i + end_idx], 0.0, format);
                i += end_idx;
            } else {
                // Not enough bytes left, break the loop
                break;
            }

            // Check if end delimiter is present
            if i + delimiters.1.len() <= text_len && text[i..].starts_with(delimiters.1) {
                // Append the end delimiter with base format
                job.append(&text[i..i + delimiters.1.len()], 0.0, base_format.clone());
                i += delimiters.1.len();
            }
        }
        // Check if the text at position i matches the pattern
        else if remaining_text.starts_with(pattern) {
            // Append the pattern with the pattern color
            let pattern_len = pattern.len();
            if i + pattern_len <= text_len {
                let mut format = base_format.clone();
                format.color = pattern_color;
                job.append(&text[i..i + pattern_len], 0.0, format);
                i += pattern_len;
            } else {
                // Not enough bytes left, break the loop
                break;
            }
        } else {
            // Append the current character with base format
            if let Some(c) = remaining_text.chars().next() {
                let c_len = c.len_utf8();
                if i + c_len <= text_len {
                    job.append(&text[i..i + c_len], 0.0, base_format.clone());
                    i += c_len;
                } else {
                    // Not enough bytes left, break the loop
                    break;
                }
            } else {
                // No more characters, break the loop
                break;
            }
        }
    }

    job
}

/// Function to color text that matches a specific substring
pub fn color_matching_text(
    layout_job: &mut LayoutJob,
    text: &str,
    pattern: &str,
    color: Color32,
) -> String {
    let mut remaining_text = String::from(text);

    while let Some(start_idx) = remaining_text.find(pattern) {
        let end_idx = start_idx + pattern.len();

        // Append the text before the pattern
        layout_job.append(&remaining_text[..start_idx], 0.0, TextFormat::default());

        // Append the pattern with the given color
        layout_job.append(pattern, 0.0, TextFormat::simple(FontId::default(), color));

        // Update remaining_text to the part after the pattern
        remaining_text = remaining_text[end_idx..].to_string();
    }

    remaining_text
}
