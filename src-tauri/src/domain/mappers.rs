//! Field mappers between XML attributes and domain Task fields.
//!
//! Bidirectional mapping between XML attribute values (strings) and typed
//! domain fields. Unknown attributes and child elements are preserved.

use super::attachment;
use super::participant;
use super::progress_link;
use super::task::*;
use super::types::TaskId;
use super::xml_tree::{XmlElement, XmlNode};

const KNOWN_TASK_ATTRS: &[&str] = &[
    "ID", "TITLE", "REFID", "COMMENTSTYPE", "PRIORITY", "RISK", "PERCENTDONE",
    "STARTDATE", "STARTDATESTRING", "DUEDATE", "DUEDATESTRING",
    "CREATIONDATE", "CREATIONDATESTRING", "COMPLETIONDATE", "COMPLETIONDATESTRING",
    "LASTMOD", "LASTMODSTRING", "CREATEDBY", "LASTMODBY",
    "POS", "POSSTRING", "ALLOCATEDTO", "ALLOCATEDBY",
    "TIMEESTIMATE", "TIMEESTUNITS", "TIMESPENT", "TIMESPENTUNITS",
    "TEXTCOLOR", "TEXTWEBCOLOR", "PRIORITYCOLOR", "PRIORITYWEBCOLOR", "SUBTASKDONE",
];

/// Reads a Task from an XML TASK element.
pub fn read_task(elem: &XmlElement) -> Task {
    let id = TaskId::new(elem.get_attr("ID").unwrap_or("0"));
    let mut task = Task::new(id);

    task.title = elem.get_attr("TITLE").unwrap_or("").to_string();
    task.ref_id = elem.get_attr("REFID").unwrap_or("0").to_string();
    if let Some(ct) = elem.get_attr("COMMENTSTYPE") {
        task.comments_type = CommentType::from_attr_value(ct);
    }
    if let Some(p) = elem.get_attr("PRIORITY").and_then(|v| v.parse::<u8>().ok()) {
        task.priority = TaskPriority::new(p);
    }
    if let Some(r) = elem.get_attr("RISK").and_then(|v| v.parse::<u8>().ok()) {
        task.risk = r.min(10);
    }
    if let Some(pd) = elem.get_attr("PERCENTDONE").and_then(|v| v.parse::<u8>().ok()) {
        task.percent_done = pd.min(100);
    }
    task.start_date = elem.get_attr("STARTDATE").and_then(|v| v.parse::<f64>().ok());
    task.start_date_string = elem.get_attr("STARTDATESTRING").map(|s| s.to_string());
    task.due_date = elem.get_attr("DUEDATE").and_then(|v| v.parse::<f64>().ok());
    task.due_date_string = elem.get_attr("DUEDATESTRING").map(|s| s.to_string());
    task.creation_date = elem.get_attr("CREATIONDATE").and_then(|v| v.parse::<f64>().ok());
    task.creation_date_string = elem.get_attr("CREATIONDATESTRING").map(|s| s.to_string());
    task.completion_date = elem.get_attr("COMPLETIONDATE").and_then(|v| v.parse::<f64>().ok());
    task.completion_date_string = elem.get_attr("COMPLETIONDATESTRING").map(|s| s.to_string());
    task.last_mod = elem.get_attr("LASTMOD").and_then(|v| v.parse::<f64>().ok());
    task.last_mod_string = elem.get_attr("LASTMODSTRING").map(|s| s.to_string());
    task.created_by = elem.get_attr("CREATEDBY").map(|s| s.to_string());
    task.last_mod_by = elem.get_attr("LASTMODBY").map(|s| s.to_string());
    task.pos = elem.get_attr("POS").and_then(|v| v.parse::<u32>().ok()).unwrap_or(0);
    task.pos_string = elem.get_attr("POSSTRING").map(|s| s.to_string());
    if let Some(at) = elem.get_attr("ALLOCATEDTO") {
        task.allocated_to = parse_participants(at);
    }
    // M6: typed participant refs mirror the native ALLOCATEDTO mapping,
    // preserving source ordering exactly. Serialization stays driven by
    // `allocated_to` so XML compatibility is untouched.
    task.participants = participant::refs_from_names(&task.allocated_to);
    task.allocated_by = elem.get_attr("ALLOCATEDBY").map(|s| s.to_string());
    task.time_estimate = elem.get_attr("TIMEESTIMATE").and_then(|v| v.parse::<f64>().ok());
    task.time_est_units = elem.get_attr("TIMEESTUNITS").map(|s| s.to_string());
    task.time_spent = elem.get_attr("TIMESPENT").and_then(|v| v.parse::<f64>().ok());
    task.time_spent_units = elem.get_attr("TIMESPENTUNITS").map(|s| s.to_string());
    task.text_color = elem.get_attr("TEXTCOLOR").map(|s| s.to_string());
    task.text_web_color = elem.get_attr("TEXTWEBCOLOR").map(|s| s.to_string());
    task.priority_color = elem.get_attr("PRIORITYCOLOR").map(|s| s.to_string());
    task.priority_web_color = elem.get_attr("PRIORITYWEBCOLOR").map(|s| s.to_string());
    task.subtask_done = elem.get_attr("SUBTASKDONE").map(|s| s.to_string());

    for child in elem.child_elements() {
        match child.tag.as_str() {
            "TASK" => {
                if let Some(cid) = child.get_attr("ID") {
                    task.children.push(TaskId::new(cid));
                }
            }
            "FILEREFPATH" => task.file_links.push(TaskFileLink { path: child.text_content() }),
            "CATEGORY" => task.categories.push(TaskCategory { name: child.text_content() }),
            "DEPENDENCY" => task.dependencies.push(read_dependency(child)),
            "COMMENTS" => {
                task.comments = Some(TaskComment {
                    comment_type: task.comments_type.clone(),
                    content: child.text_content(),
                });
            }
            "METADATA" => {
                task.metadata.push(TaskMetadata {
                    attrs: child.attrs.iter().map(|a| (a.name.clone(), a.value.clone())).collect(),
                });
            }
            _ => task.unknown_children.push(elem_to_string(child)),
        }
    }

    // M6 extension attributes (Tier B fallback per delivery-plan risk #1:
    // custom XML attributes + unknown_attrs preservation). Consumed into
    // typed fields ONLY when they parse and re-validate cleanly; otherwise
    // they fall through to `unknown_attrs` and survive verbatim.
    let mut progress_links_consumed = false;
    if let Some(v) = elem.get_attr(progress_link::PROGRESS_LINKS_ATTR) {
        if let Some(links) = progress_link::links_from_attr_value(v) {
            task.progress_links = links;
            progress_links_consumed = true;
        }
    }
    let mut attachments_consumed = false;
    if let Some(v) = elem.get_attr(attachment::ATTACHMENTS_ATTR) {
        if let Some(atts) = attachment::attachments_from_attr_value(v) {
            task.attachments = atts;
            attachments_consumed = true;
        }
    }

    for attr in &elem.attrs {
        if KNOWN_TASK_ATTRS.contains(&attr.name.as_str()) {
            continue;
        }
        if attr.name == progress_link::PROGRESS_LINKS_ATTR && progress_links_consumed {
            continue;
        }
        if attr.name == attachment::ATTACHMENTS_ATTR && attachments_consumed {
            continue;
        }
        task.unknown_attrs.push((attr.name.clone(), attr.value.clone()));
    }
    task
}

/// Writes a Task back to an XML element.
pub fn write_task(task: &Task, elem: &mut XmlElement) {
    elem.set_attr("ID", task.id.as_str());
    elem.set_attr("TITLE", &task.title);
    elem.set_attr("REFID", &task.ref_id);
    elem.set_attr("COMMENTSTYPE", task.comments_type.as_attr_value());
    elem.set_attr("PRIORITY", task.priority.value().to_string());
    elem.set_attr("RISK", task.risk.to_string());
    elem.set_attr("PERCENTDONE", task.percent_done.to_string());
    opt_f64(elem, "STARTDATE", task.start_date);
    opt_str(elem, "STARTDATESTRING", &task.start_date_string);
    opt_f64(elem, "DUEDATE", task.due_date);
    opt_str(elem, "DUEDATESTRING", &task.due_date_string);
    opt_f64(elem, "CREATIONDATE", task.creation_date);
    opt_str(elem, "CREATIONDATESTRING", &task.creation_date_string);
    opt_f64(elem, "COMPLETIONDATE", task.completion_date);
    opt_str(elem, "COMPLETIONDATESTRING", &task.completion_date_string);
    opt_f64(elem, "LASTMOD", task.last_mod);
    opt_str(elem, "LASTMODSTRING", &task.last_mod_string);
    opt_str(elem, "CREATEDBY", &task.created_by);
    opt_str(elem, "LASTMODBY", &task.last_mod_by);
    elem.set_attr("POS", task.pos.to_string());
    opt_str(elem, "POSSTRING", &task.pos_string);
    if task.allocated_to.is_empty() {
        elem.remove_attr("ALLOCATEDTO");
    } else {
        elem.set_attr("ALLOCATEDTO", task.allocated_to.join("; "));
    }
    opt_str(elem, "ALLOCATEDBY", &task.allocated_by);
    opt_f64(elem, "TIMEESTIMATE", task.time_estimate);
    opt_str(elem, "TIMEESTUNITS", &task.time_est_units);
    opt_f64(elem, "TIMESPENT", task.time_spent);
    opt_str(elem, "TIMESPENTUNITS", &task.time_spent_units);
    opt_str(elem, "TEXTCOLOR", &task.text_color);
    opt_str(elem, "TEXTWEBCOLOR", &task.text_web_color);
    opt_str(elem, "PRIORITYCOLOR", &task.priority_color);
    opt_str(elem, "PRIORITYWEBCOLOR", &task.priority_web_color);
    opt_str(elem, "SUBTASKDONE", &task.subtask_done);
    // M6 extension attributes: written ONLY when the task carries M6 data,
    // so documents without progress links / attachment metadata serialize
    // exactly as before (no new attributes introduced on save). If the
    // attribute was NOT consumable (corrupt/foreign value) it lives on in
    // `unknown_attrs` and the element's raw attribute is left untouched.
    let pl_is_unknown = task
        .unknown_attrs
        .iter()
        .any(|(k, _)| k == progress_link::PROGRESS_LINKS_ATTR);
    if !pl_is_unknown {
        if task.progress_links.is_empty() {
            elem.remove_attr(progress_link::PROGRESS_LINKS_ATTR);
        } else {
            elem.set_attr(
                progress_link::PROGRESS_LINKS_ATTR,
                progress_link::links_to_attr_value(&task.progress_links),
            );
        }
    }
    let att_is_unknown = task
        .unknown_attrs
        .iter()
        .any(|(k, _)| k == attachment::ATTACHMENTS_ATTR);
    if !att_is_unknown {
        if task.attachments.is_empty() {
            elem.remove_attr(attachment::ATTACHMENTS_ATTR);
        } else {
            elem.set_attr(
                attachment::ATTACHMENTS_ATTR,
                attachment::attachments_to_attr_value(&task.attachments),
            );
        }
    }

    // Rebuild child elements
    let mut new_children = Vec::new();
    for child in &elem.children {
        match child {
            XmlNode::Element(e) => match e.tag.as_str() {
                "TASK" => new_children.push(child.clone()),
                "FILEREFPATH" | "CATEGORY" | "DEPENDENCY" | "COMMENTS" | "METADATA" => {}
                _ => new_children.push(child.clone()),
            },
            _ => new_children.push(child.clone()),
        }
    }
    for link in &task.file_links {
        let mut e = XmlElement::new("FILEREFPATH");
        e.children.push(XmlNode::Text(link.path.clone()));
        new_children.push(XmlNode::Element(e));
    }
    for cat in &task.categories {
        let mut e = XmlElement::new("CATEGORY");
        e.children.push(XmlNode::Text(cat.name.clone()));
        new_children.push(XmlNode::Element(e));
    }
    for dep in &task.dependencies {
        new_children.push(XmlNode::Element(write_dependency(dep)));
    }
    if let Some(ref c) = task.comments {
        let mut e = XmlElement::new("COMMENTS");
        e.children.push(XmlNode::Text(c.content.clone()));
        new_children.push(XmlNode::Element(e));
    }
    for meta in &task.metadata {
        let mut e = XmlElement::new("METADATA");
        for (k, v) in &meta.attrs { e.set_attr(k.clone(), v.clone()); }
        new_children.push(XmlNode::Element(e));
    }
    elem.children = new_children;
}

/// Reads DocumentMetadata from the root element.
pub fn read_document_metadata(
    root: &XmlElement,
    enc: super::encoding::XmlEncodingMeta,
) -> DocumentMetadata {
    let mut m = DocumentMetadata::new(enc);
    m.project_name = root.get_attr("PROJECTNAME").map(|s| s.to_string());
    m.filename = root.get_attr("FILENAME").map(|s| s.to_string());
    m.next_unique_id = root.get_attr("NEXTUNIQUEID").and_then(|v| v.parse::<u64>().ok()).unwrap_or(1);
    m.file_version = root.get_attr("FILEVERSION").map(|s| s.to_string());
    m.app_ver = root.get_attr("APPVER").map(|s| s.to_string());
    m.file_format = root.get_attr("FILEFORMAT").map(|s| s.to_string());
    m.earliest_due_date = root.get_attr("EARLIESTDUEDATE").and_then(|v| v.parse::<f64>().ok());
    m.last_mod = root.get_attr("LASTMOD").and_then(|v| v.parse::<f64>().ok());
    m.last_mod_string = root.get_attr("LASTMODSTRING").map(|s| s.to_string());
    const KNOWN: &[&str] = &["PROJECTNAME","EARLIESTDUEDATE","LASTMOD","LASTMODSTRING","FILENAME","NEXTUNIQUEID","FILEVERSION","APPVER","FILEFORMAT"];
    for attr in &root.attrs {
        if !KNOWN.contains(&attr.name.as_str()) {
            m.unknown_root_attrs.push((attr.name.clone(), attr.value.clone()));
        }
    }
    for child in root.children_by_tag("METADATA") {
        m.root_metadata.push(TaskMetadata {
            attrs: child.attrs.iter().map(|a| (a.name.clone(), a.value.clone())).collect(),
        });
    }
    m
}

fn read_dependency(e: &XmlElement) -> TaskDependency {
    let task_id = e.first_child_by_tag("TASKID").map(|c| c.text_content()).unwrap_or_default();
    let dependency_type = e.first_child_by_tag("DEPENDENCYTYPE").and_then(|c| c.text_content().parse().ok()).unwrap_or(0);
    // Lossless preservation (M6): capture the original element whenever it
    // carries extra data — ANY attribute (legacy plugin attrs or the M6
    // `MTDL_DOCUMENTID` external-reference marker), child elements other
    // than TASKID/DEPENDENCYTYPE, or comments/CDATA/PIs. Plain native
    // dependencies keep `raw_xml = None` and are rebuilt minimally, exactly
    // as before M6.
    let has_extra = !e.attrs.is_empty()
        || e.child_elements().any(|c| c.tag != "TASKID" && c.tag != "DEPENDENCYTYPE")
        || e.children.iter().any(|n| {
            matches!(
                n,
                XmlNode::Comment(_) | XmlNode::CData(_) | XmlNode::ProcessingInstruction(_)
            )
        });
    let raw_xml = if has_extra { Some(elem_to_string(e)) } else { None };
    TaskDependency { task_id, dependency_type, raw_xml }
}

fn write_dependency(d: &TaskDependency) -> XmlElement {
    if let Some(raw) = &d.raw_xml {
        // Rebuild from the preserved original element, refreshing the typed
        // fields so domain mutations are reflected while every unknown
        // attribute/child/comment survives unchanged.
        let wrapped = format!("<MTDLRAW>{}</MTDLRAW>", raw);
        if let Ok(doc) = super::xml_parser::parse_xml(wrapped.as_bytes()) {
            if let Some(mut e) = doc.root.first_child_by_tag("DEPENDENCY").cloned() {
                refresh_dependency_children(&mut e, d);
                return e;
            }
        }
        // Unparsable raw form (defensive): fall through to minimal native.
    }
    let mut e = XmlElement::new("DEPENDENCY");
    let mut t = XmlElement::new("TASKID");
    t.children.push(XmlNode::Text(d.task_id.clone()));
    e.children.push(XmlNode::Element(t));
    let mut dt = XmlElement::new("DEPENDENCYTYPE");
    dt.children.push(XmlNode::Text(d.dependency_type.to_string()));
    e.children.push(XmlNode::Element(dt));
    e
}

/// Updates the first TASKID / DEPENDENCYTYPE children of a preserved
/// DEPENDENCY element from the typed fields.
fn refresh_dependency_children(e: &mut XmlElement, d: &TaskDependency) {
    let mut seen_taskid = false;
    let mut seen_type = false;
    for child in &mut e.children {
        if let XmlNode::Element(c) = child {
            if c.tag == "TASKID" && !seen_taskid {
                c.set_text_content(d.task_id.clone());
                seen_taskid = true;
            } else if c.tag == "DEPENDENCYTYPE" && !seen_type {
                c.set_text_content(d.dependency_type.to_string());
                seen_type = true;
            }
        }
    }
}

fn parse_participants(v: &str) -> Vec<String> {
    v.split(';').map(|s| s.trim().to_string()).filter(|s| !s.is_empty()).collect()
}

fn elem_to_string(e: &XmlElement) -> String {
    let mut s = String::new();
    s.push('<');
    s.push_str(&e.tag);
    for a in &e.attrs { s.push_str(&format!(" {}=\"{}\"", a.name, super::xml_parser::escape_xml(&a.value))); }
    if e.children.is_empty() { s.push_str("/>"); return s; }
    s.push('>');
    for c in &e.children {
        match c {
            XmlNode::Element(ch) => s.push_str(&elem_to_string(ch)),
            XmlNode::Text(t) => s.push_str(&super::xml_parser::escape_xml(t)),
            XmlNode::Comment(c) => { s.push_str("<!--"); s.push_str(c); s.push_str("-->"); }
            XmlNode::CData(c) => { s.push_str("<![CDATA["); s.push_str(c); s.push_str("]]>"); }
            XmlNode::ProcessingInstruction(p) => { s.push_str("<?"); s.push_str(p); s.push_str("?>"); }
        }
    }
    s.push_str("</"); s.push_str(&e.tag); s.push('>');
    s
}

fn opt_f64(e: &mut XmlElement, name: &str, v: Option<f64>) {
    if let Some(v) = v { e.set_attr(name, format!("{:.8}", v)); }
}
fn opt_str(e: &mut XmlElement, name: &str, v: &Option<String>) {
    if let Some(ref v) = v { e.set_attr(name, v.as_str()); }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::xml_parser::parse_xml;

    fn task_elem(xml: &str) -> XmlElement {
        parse_xml(xml.as_bytes()).unwrap().root.first_child_by_tag("TASK").unwrap().clone()
    }

    #[test]
    fn read_basic() {
        let t = read_task(&task_elem(r#"<R><TASK ID="1" TITLE="Test" PRIORITY="8" RISK="3" PERCENTDONE="50" COMMENTSTYPE="PLAIN_TEXT" POS="0" REFID="0"></TASK></R>"#));
        assert_eq!(t.id.as_str(), "1");
        assert_eq!(t.title, "Test");
        assert_eq!(t.priority.value(), 8);
        assert_eq!(t.risk, 3);
        assert_eq!(t.percent_done, 50);
    }

    #[test]
    fn read_children() {
        let t = read_task(&task_elem(r#"<R><TASK ID="1"><FILEREFPATH>.\f.pdf</FILEREFPATH><CATEGORY>A</CATEGORY><CATEGORY>B</CATEGORY><COMMENTS>Hi</COMMENTS></TASK></R>"#));
        assert_eq!(t.file_links.len(), 1);
        assert_eq!(t.categories.len(), 2);
        assert_eq!(t.comments.as_ref().unwrap().content, "Hi");
    }

    #[test]
    fn read_unknown_attrs() {
        let t = read_task(&task_elem(r#"<R><TASK ID="1" CUSTOM="x"></TASK></R>"#));
        assert!(t.unknown_attrs.iter().any(|(k,v)| k=="CUSTOM" && v=="x"));
    }

    #[test]
    fn write_roundtrip() {
        let t = read_task(&task_elem(r#"<R><TASK ID="1" TITLE="X" PRIORITY="3" COMMENTSTYPE="PLAIN_TEXT" POS="0" REFID="0"><CATEGORY>Z</CATEGORY></TASK></R>"#));
        let mut e = XmlElement::new("TASK");
        write_task(&t, &mut e);
        assert_eq!(e.get_attr("TITLE"), Some("X"));
        assert_eq!(e.get_attr("PRIORITY"), Some("3"));
        let cats: Vec<_> = e.children_by_tag("CATEGORY").collect();
        assert_eq!(cats.len(), 1);
        assert_eq!(cats[0].text_content(), "Z");
    }

    #[test]
    fn participants_roundtrip() {
        let p = parse_participants("Alice; Bob; Charlie");
        assert_eq!(p, vec!["Alice", "Bob", "Charlie"]);
        assert_eq!(p.join("; "), "Alice; Bob; Charlie");
    }

    #[test]
    fn participants_mirror_allocated_to_in_order() {
        let t = read_task(&task_elem(
            r#"<R><TASK ID="1" ALLOCATEDTO="Zoe; Alice; Bob"></TASK></R>"#,
        ));
        assert_eq!(t.allocated_to, vec!["Zoe", "Alice", "Bob"]);
        let names: Vec<_> = t.participants.iter().map(|p| p.display_name.as_str()).collect();
        assert_eq!(names, vec!["Zoe", "Alice", "Bob"]);
        // Write path is driven by the native attribute (unchanged output).
        let mut e = XmlElement::new("TASK");
        write_task(&t, &mut e);
        assert_eq!(e.get_attr("ALLOCATEDTO"), Some("Zoe; Alice; Bob"));
    }

    #[test]
    fn progress_links_attr_roundtrip() {
        let link = super::super::progress_link::ProgressLink::new(
            "id-1",
            "PR",
            "https://github.com/o/r/pull/1",
        )
        .unwrap();
        let json = super::super::progress_link::links_to_attr_value(&[link.clone()]);
        let xml = format!(r#"<R><TASK ID="1" MTDL_PROGRESS_LINKS="{}"></TASK></R>"#, super::super::xml_parser::escape_xml(&json));
        let t = read_task(&task_elem(&xml));
        assert_eq!(t.progress_links, vec![link]);
        assert!(t.unknown_attrs.iter().all(|(k, _)| k != "MTDL_PROGRESS_LINKS"));
        // Write → re-read is stable.
        let mut e = XmlElement::new("TASK");
        write_task(&t, &mut e);
        let t2 = read_task(&e);
        assert_eq!(t2.progress_links, t.progress_links);
        // Empty links ⇒ attribute absent (byte-compatible for M6-free docs).
        let mut plain = read_task(&task_elem(r#"<R><TASK ID="2"></TASK></R>"#));
        plain.progress_links.clear();
        let mut e2 = XmlElement::new("TASK");
        write_task(&plain, &mut e2);
        assert_eq!(e2.get_attr("MTDL_PROGRESS_LINKS"), None);
    }

    #[test]
    fn corrupt_progress_links_attr_preserved_as_unknown() {
        let original = task_elem(r#"<R><TASK ID="1" MTDL_PROGRESS_LINKS="not-json"></TASK></R>"#);
        let t = read_task(&original);
        assert!(t.progress_links.is_empty());
        assert!(t
            .unknown_attrs
            .iter()
            .any(|(k, v)| k == "MTDL_PROGRESS_LINKS" && v == "not-json"));
        // Production rewrite pattern: write_task in place on the original
        // element — the corrupt attribute survives verbatim (unknown_attrs
        // are carried by the element itself).
        let mut e = original.clone();
        write_task(&t, &mut e);
        assert_eq!(e.get_attr("MTDL_PROGRESS_LINKS"), Some("not-json"));
        // A fresh-element write must not silently invent a valid attribute.
        let mut fresh = XmlElement::new("TASK");
        write_task(&t, &mut fresh);
        assert_eq!(fresh.get_attr("MTDL_PROGRESS_LINKS"), None);
    }

    #[test]
    fn attachments_attr_roundtrip() {
        let att = super::super::attachment::url_attachment("https://example.com/a.pdf", Some("A"))
            .unwrap();
        let json = super::super::attachment::attachments_to_attr_value(&[att.clone()]);
        let xml = format!(
            r#"<R><TASK ID="1" MTDL_ATTACHMENTS="{}"><FILEREFPATH>{}</FILEREFPATH></TASK></R>"#,
            super::super::xml_parser::escape_xml(&json),
            super::super::xml_parser::escape_xml(&att.path_or_url)
        );
        let t = read_task(&task_elem(&xml));
        assert_eq!(t.attachments, vec![att]);
        assert_eq!(t.file_links.len(), 1);
        let mut e = XmlElement::new("TASK");
        write_task(&t, &mut e);
        assert_eq!(read_task(&e).attachments, t.attachments);
    }

    #[test]
    fn dependency_extra_data_preserved_via_raw_xml() {
        let t = read_task(&task_elem(
            r#"<R><TASK ID="1"><DEPENDENCY MTDL_DOCUMENTID="doc-x" LEGACY="keep"><TASKID>9</TASKID><DEPENDENCYTYPE>1</DEPENDENCYTYPE><EXTRA>data</EXTRA></DEPENDENCY></TASK></R>"#,
        ));
        assert_eq!(t.dependencies.len(), 1);
        let dep = &t.dependencies[0];
        assert_eq!(dep.task_id, "9");
        assert_eq!(dep.dependency_type, 1);
        let raw = dep.raw_xml.clone().expect("extra data must be captured");
        assert!(raw.contains("MTDL_DOCUMENTID"));
        assert!(raw.contains("LEGACY"));
        assert!(raw.contains("<EXTRA>data</EXTRA>"));

        let mut e = XmlElement::new("TASK");
        write_task(&t, &mut e);
        let t2 = read_task(&e);
        let dep2 = &t2.dependencies[0];
        assert_eq!(dep2.task_id, "9");
        assert_eq!(dep2.dependency_type, 1);
        let raw2 = dep2.raw_xml.clone().unwrap();
        assert!(raw2.contains(r#"MTDL_DOCUMENTID="doc-x""#));
        assert!(raw2.contains(r#"LEGACY="keep""#));
        assert!(raw2.contains("<EXTRA>data</EXTRA>"));
    }

    #[test]
    fn plain_dependency_stays_minimal() {
        let t = read_task(&task_elem(
            r#"<R><TASK ID="1"><DEPENDENCY><TASKID>2</TASKID><DEPENDENCYTYPE>0</DEPENDENCYTYPE></DEPENDENCY></TASK></R>"#,
        ));
        assert!(t.dependencies[0].raw_xml.is_none());
        let mut e = XmlElement::new("TASK");
        write_task(&t, &mut e);
        let dep_elems: Vec<_> = e.children_by_tag("DEPENDENCY").collect();
        assert_eq!(dep_elems.len(), 1);
        assert!(dep_elems[0].attrs.is_empty());
        assert_eq!(dep_elems[0].children_by_tag("TASKID").count(), 1);
    }
}
