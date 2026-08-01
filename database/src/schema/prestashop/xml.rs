use xml::writer::{EmitterConfig, EventWriter, XmlEvent as WriteXmlEvent};
use xml::reader::{EventReader, XmlEvent};
use xml::namespace::Namespace;
use std::borrow::Cow;

/// Returns the first `<error>` code/message from a Prestashop webservice response body.
pub fn first_prestashop_error(xml_str: &str) -> Option<String> {
    let parser = EventReader::new(xml_str.as_bytes());
    let mut in_errors = false;
    let mut field: Option<String> = None;
    let mut code: Option<String> = None;
    let mut message: Option<String> = None;

    for event in parser {
        let Ok(event) = event else { return None };
        match event {
            XmlEvent::StartElement { name, .. } => match name.local_name.as_str() {
                "errors" => in_errors = true,
                "code" | "message" if in_errors => field = Some(name.local_name),
                _ => {}
            },
            XmlEvent::EndElement { name } => {
                if name.local_name == "errors" {
                    break;
                }
                field = None;
            }
            XmlEvent::CData(text) | XmlEvent::Characters(text) => match field.as_deref() {
                Some("code") => code = Some(text.trim().to_string()),
                Some("message") => message = Some(text.trim().to_string()),
                _ => {}
            },
            _ => {}
        }
    }

    match (code, message) {
        (Some(c), Some(m)) => Some(format!("[{c}] {m}")),
        (None, Some(m)) => Some(m),
        (Some(c), None) => Some(format!("error code {c}")),
        (None, None) => in_errors.then(|| "unspecified Prestashop error".to_string()),
    }
}

/// Text content of the first element with this name, empty when self-closing.
pub fn element_text(xml_str: &str, key: &str) -> Option<String> {
    let parser = EventReader::new(xml_str.as_bytes());
    let mut depth = 0usize;
    let mut target_depth = None;
    let mut text = String::new();

    for event in parser {
        let Ok(event) = event else { return None };
        match event {
            XmlEvent::StartElement { name, .. } => {
                depth += 1;
                if target_depth.is_none() && name.local_name == key {
                    target_depth = Some(depth);
                }
            }
            XmlEvent::EndElement { .. } => {
                if Some(depth) == target_depth {
                    return Some(text);
                }
                depth -= 1;
            }
            XmlEvent::CData(chunk) | XmlEvent::Characters(chunk) if Some(depth) == target_depth => {
                text.push_str(&chunk);
            }
            _ => {}
        }
    }

    None
}

/// True when the document contains an element with this name at any depth.
pub fn has_element(xml_str: &str, key: &str) -> bool {
    EventReader::new(xml_str.as_bytes()).into_iter().any(|event| {
        matches!(event, Ok(XmlEvent::StartElement { ref name, .. }) if name.local_name == key)
    })
}

pub fn modify_xml(xml_str: &str, key: &str, new_value: &str) -> anyhow::Result<String, anyhow::Error> {
    let parser = EventReader::new(xml_str.as_bytes());
    let mut writer_vec: Vec<u8> = Vec::new();
    let mut writer = EventWriter::new_with_config(&mut writer_vec, EmitterConfig::new().perform_indent(true));

    let mut depth = 0;
    let mut target_depth = None;

    for event in parser {
        match event? {
            XmlEvent::StartElement { name, .. } => {
                depth += 1;
                writer.write(WriteXmlEvent::StartElement {
                    name: name.borrow(),
                    attributes: Cow::Owned(vec![]), // ✂️ Strip all attributes
                    namespace: Cow::Owned(Namespace::empty()),
                })?;

                if name.local_name == key {
                    target_depth = Some(depth);
                }
            }

            XmlEvent::EndElement { name } => {
                if Some(depth) == target_depth {
                    writer.write(WriteXmlEvent::Characters(new_value))?; // 🧽 Insert plain text
                    target_depth = None;
                }
                writer.write(WriteXmlEvent::EndElement {
                    name: Some(name.borrow()),
                })?;
                depth -= 1;
            }

            XmlEvent::CData(_) | XmlEvent::Characters(_) if Some(depth) == target_depth => {
                // 👻 Skip original inner content of the target tag
            }

            XmlEvent::CData(text) | XmlEvent::Characters(text) => {
                // 🧼 Write text as plain characters (no CDATA)
                writer.write(WriteXmlEvent::Characters(&text))?;
            }

            XmlEvent::Comment(comment) => {
                writer.write(WriteXmlEvent::Comment(&comment))?;
            }

            XmlEvent::ProcessingInstruction { name, data } => {
                writer.write(WriteXmlEvent::ProcessingInstruction {
                    name: &name,
                    data: data.as_deref(),
                })?;
            }

            XmlEvent::StartDocument { version, encoding, standalone } => {
                writer.write(WriteXmlEvent::StartDocument {
                    version,
                    encoding: Some(&encoding),
                    standalone,
                })?;
            }

            XmlEvent::EndDocument => {
                // ignored by writer
            }

            _ => {}
        }
    }

    Ok(String::from_utf8(writer_vec)?.replace("\n", ""))
}

#[cfg(test)]
mod tests {
    use super::*;

    const ERROR_BODY: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<prestashop><errors><error><code>65</code><message>The field id_employee_sales_rep is required</message></error></errors></prestashop>"#;

    const ORDER_BODY: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<prestashop><order><id>2151916</id><current_state>30</current_state><id_employee_sales_rep>1382</id_employee_sales_rep><shipping_number/></order></prestashop>"#;

    #[test]
    fn extracts_code_and_message() {
        assert_eq!(
            first_prestashop_error(ERROR_BODY).as_deref(),
            Some("[65] The field id_employee_sales_rep is required")
        );
    }

    #[test]
    fn order_body_is_not_an_error() {
        assert_eq!(first_prestashop_error(ORDER_BODY), None);
    }

    #[test]
    fn unparseable_body_is_not_an_error() {
        assert_eq!(first_prestashop_error("<!DOCTYPE html><html><br></html>"), None);
    }

    /// A note whose CDATA happens to contain the literal text `<errors>` is data, not an
    /// error element — which a substring scan would get wrong.
    #[test]
    fn cdata_mentioning_errors_is_not_an_error() {
        let body = r#"<prestashop><order><id>1</id><check_in_notes><![CDATA[see <errors> in the log]]></check_in_notes></order></prestashop>"#;
        assert_eq!(first_prestashop_error(body), None);
    }

    #[test]
    fn element_text_reads_values_used_for_write_verification() {
        assert_eq!(element_text(ORDER_BODY, "current_state").as_deref(), Some("30"));
        assert_eq!(element_text(ORDER_BODY, "id_employee_sales_rep").as_deref(), Some("1382"));
        // Self-closing tags read as empty, not absent.
        assert_eq!(element_text(ORDER_BODY, "shipping_number").as_deref(), Some(""));
        assert_eq!(element_text(ORDER_BODY, "id_employee_split_rep"), None);
    }

    #[test]
    fn element_text_reads_cdata() {
        let body = r#"<prestashop><order><check_in_notes><![CDATA[secure boot]]></check_in_notes></order></prestashop>"#;
        assert_eq!(element_text(body, "check_in_notes").as_deref(), Some("secure boot"));
    }

    /// The order-level field must win over a same-named element nested deeper.
    #[test]
    fn element_text_takes_the_first_occurrence() {
        let body = r#"<prestashop><order><id>2111019</id><associations><order_rows><order_row><id>99</id></order_row></order_rows></associations></order></prestashop>"#;
        assert_eq!(element_text(body, "id").as_deref(), Some("2111019"));
    }

    #[test]
    fn has_element_detects_present_absent_and_self_closing() {
        assert!(has_element(ORDER_BODY, "id_employee_sales_rep"));
        assert!(has_element(ORDER_BODY, "shipping_number"));
        assert!(!has_element(ORDER_BODY, "id_employee_split_rep"));
    }

    /// The write path relies on this: an absent key leaves the document unchanged, which is
    /// why order_write checks has_element before modifying.
    #[test]
    fn modify_xml_is_a_no_op_for_an_absent_key() {
        let out = modify_xml(ORDER_BODY, "id_employee_split_rep", "35").unwrap();
        assert!(!out.contains("id_employee_split_rep"));
    }

    #[test]
    fn modify_xml_replaces_the_target_value() {
        let out = modify_xml(ORDER_BODY, "id_employee_sales_rep", "1347").unwrap();
        assert!(out.contains("<id_employee_sales_rep>1347</id_employee_sales_rep>"));
        assert!(out.contains("<current_state>30</current_state>"));
    }
}

pub fn remove_xml_tag(xml_str: &str, key: &str) -> anyhow::Result<String, anyhow::Error> {
    let parser = EventReader::new(xml_str.as_bytes());
    let mut writer_vec: Vec<u8> = Vec::new();
    let mut writer = EventWriter::new_with_config(&mut writer_vec, EmitterConfig::new().perform_indent(true));
    let mut depth: usize = 0;
    let mut in_skip = false;
    let mut skip_depth: usize = 0;

    for event in parser {
        match &event? {
            XmlEvent::StartElement { name, attributes, namespace } => {
                depth += 1;
                if in_skip {
                    // skip
                } else {
                    if name.local_name == key {
                        in_skip = true;
                        skip_depth = depth;
                        // do not write
                    } else {
                        let filtered_attrs: Vec<_> = attributes
                            .iter()
                            .filter(|a| !(a.name.prefix.as_ref().map_or(false, |p| p == "xlink") && a.name.local_name == "href"))
                            .map(|a| a.borrow())
                            .collect();
                        writer.write(WriteXmlEvent::StartElement {
                            name: name.borrow(),
                            attributes: Cow::Owned(filtered_attrs),
                            namespace: Cow::Borrowed(namespace),
                        })?;
                    }
                }
            }
            XmlEvent::EndElement { name } => {
                if in_skip {
                    depth -= 1;
                    if depth < skip_depth {
                        in_skip = false;
                    }
                } else {
                    writer.write(WriteXmlEvent::EndElement { name: Some(name.borrow()) })?;
                    depth -= 1;
                }
            }
            XmlEvent::Characters(s) => {
                if !in_skip {
                    writer.write(WriteXmlEvent::Characters(s))?;
                }
            }
            XmlEvent::CData(s) => {
                if !in_skip {
                    writer.write(WriteXmlEvent::Characters(s))?;
                }
            }
            other => {
                if !in_skip {
                    if let Some(we) = other.as_writer_event() {
                        writer.write(we)?;
                    }
                }
            }
        }
    }

    Ok(String::from_utf8(writer_vec)?.replace("\n", ""))
}