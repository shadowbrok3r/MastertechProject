use xml::writer::{EmitterConfig, EventWriter, XmlEvent as WriteXmlEvent};
use xml::reader::{EventReader, XmlEvent};
use xml::namespace::Namespace;
use std::borrow::Cow;

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

// pub fn remove_xml_tag(xml_str: &str, tag_to_remove: &str) -> anyhow::Result<String, anyhow::Error> {
//     let parser = EventReader::new(xml_str.as_bytes());
//     let mut writer_vec: Vec<u8> = Vec::new();
//     let mut writer = EventWriter::new_with_config(&mut writer_vec, EmitterConfig::new().perform_indent(true));

//     let mut depth = 0;
//     let mut skip_depth: Option<usize> = None;

//     for event in parser {
//         match event? {
//             XmlEvent::StartElement { name, .. } => {
//                 depth += 1;
//                 if skip_depth.is_none() && name.local_name == tag_to_remove {
//                     skip_depth = Some(depth);
//                     continue; // Skip writing this tag
//                 }

//                 if skip_depth.is_none() {
//                     writer.write(WriteXmlEvent::StartElement {
//                         name: name.borrow(),
//                         attributes: Cow::Owned(vec![]), // Remove attributes
//                         namespace: Cow::Owned(xml::namespace::Namespace::empty()),
//                     })?;
//                 }
//             }

//             XmlEvent::EndElement { name } => {
//                 if Some(depth) == skip_depth {
//                     skip_depth = None;
//                     depth -= 1;
//                     continue; // Don't write end tag
//                 }

//                 if skip_depth.is_none() {
//                     writer.write(WriteXmlEvent::EndElement {
//                         name: Some(name.borrow()),
//                     })?;
//                 }
//                 depth -= 1;
//             }

//             XmlEvent::Characters(_) | XmlEvent::CData(_) if skip_depth.is_some() => {
//                 // Skip tag content
//             }

//             XmlEvent::Characters(text) | XmlEvent::CData(text) => {
//                 writer.write(WriteXmlEvent::Characters(&text))?;
//             }

//             XmlEvent::Comment(comment) if skip_depth.is_none() => {
//                 writer.write(WriteXmlEvent::Comment(&comment))?;
//             }

//             XmlEvent::ProcessingInstruction { name, data } if skip_depth.is_none() => {
//                 writer.write(WriteXmlEvent::ProcessingInstruction {
//                     name: &name,
//                     data: data.as_deref(),
//                 })?;
//             }

//             XmlEvent::StartDocument { version, encoding, standalone } => {
//                 writer.write(WriteXmlEvent::StartDocument {
//                     version,
//                     encoding: Some(&encoding),
//                     standalone,
//                 })?;
//             }

//             XmlEvent::EndDocument => {}

//             _ => {}
//         }
//     }

//     Ok(String::from_utf8(writer_vec)?.replace("\n", ""))
// }