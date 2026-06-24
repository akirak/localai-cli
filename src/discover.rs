use serde::Deserialize;
use std::collections::HashMap;

#[derive(Deserialize, Debug)]
#[allow(dead_code)]
pub struct Doc {
    pub paths: HashMap<String, PathItem>,
    #[serde(default)]
    pub definitions: HashMap<String, serde_json::Value>,
}

#[derive(Deserialize, Debug, Default)]
#[allow(dead_code)]
pub struct PathItem {
    pub get: Option<Operation>,
    pub post: Option<Operation>,
    pub put: Option<Operation>,
    pub delete: Option<Operation>,
    pub patch: Option<Operation>,
    pub head: Option<Operation>,
}

impl PathItem {
    pub fn operations(&self) -> Vec<(&'static str, &Operation)> {
        let mut ops = Vec::new();
        if let Some(op) = &self.get {
            ops.push(("GET", op));
        }
        if let Some(op) = &self.post {
            ops.push(("POST", op));
        }
        if let Some(op) = &self.put {
            ops.push(("PUT", op));
        }
        if let Some(op) = &self.delete {
            ops.push(("DELETE", op));
        }
        if let Some(op) = &self.patch {
            ops.push(("PATCH", op));
        }
        if let Some(op) = &self.head {
            ops.push(("HEAD", op));
        }
        ops
    }
}

#[derive(Deserialize, Debug)]
#[allow(dead_code)]
pub struct Operation {
    pub summary: Option<String>,
    #[serde(default)]
    pub tags: Vec<String>,
    #[serde(default)]
    pub parameters: Vec<Parameter>,
    #[serde(default)]
    pub consumes: Vec<String>,
    #[serde(default)]
    pub produces: Vec<String>,
}

#[derive(Deserialize, Debug, Clone)]
#[allow(dead_code)]
pub struct Parameter {
    pub name: String,
    #[serde(rename = "in")]
    pub location: String,
    #[serde(default)]
    pub required: bool,
    #[serde(rename = "type")]
    pub param_type: Option<String>,
    pub description: Option<String>,
}

pub static DOC_JSON: &str = include_str!("../doc.json");

pub fn load_doc() -> anyhow::Result<Doc> {
    Ok(serde_json::from_str(DOC_JSON)?)
}

/// A flattened, ordered view of a single operation within an API path.
#[derive(Debug, Clone)]
pub struct Endpoint {
    pub method: String,
    pub path: String,
    pub summary: String,
    pub tags: Vec<String>,
    #[allow(dead_code)]
    pub parameters: Vec<Parameter>,
}

/// Build a flat, sorted list of all operations defined in the doc.
pub fn endpoints() -> anyhow::Result<Vec<Endpoint>> {
    let doc = load_doc()?;
    let mut out: Vec<Endpoint> = Vec::new();
    for (path, item) in &doc.paths {
        for (method, op) in item.operations() {
            out.push(Endpoint {
                method: method.to_string(),
                path: path.clone(),
                summary: op.summary.clone().unwrap_or_default(),
                tags: op.tags.clone(),
                parameters: op.parameters.clone(),
            });
        }
    }
    out.sort_by(|a, b| a.path.cmp(&b.path).then_with(|| a.method.cmp(&b.method)));
    Ok(out)
}

/// All tags found in the doc, sorted and deduplicated.
#[allow(dead_code)]
pub fn tags() -> anyhow::Result<Vec<String>> {
    let doc = load_doc()?;
    let mut tags: Vec<String> = doc
        .paths
        .values()
        .flat_map(|item| item.operations())
        .flat_map(|(_, op)| op.tags.iter().cloned())
        .collect();
    tags.sort();
    tags.dedup();
    Ok(tags)
}

pub fn list_endpoints(tag_filter: Option<&str>) -> anyhow::Result<()> {
    let doc = load_doc()?;
    let mut entries: Vec<(String, String, String)> = Vec::new();

    for (path, item) in &doc.paths {
        for (method, op) in item.operations() {
            if let Some(filter) = tag_filter
                && !op.tags.iter().any(|t| t.eq_ignore_ascii_case(filter))
            {
                continue;
            }
            let summary = op.summary.as_deref().unwrap_or("");
            let tags = op.tags.join(", ");
            entries.push((format!("{method} {path}"), summary.to_string(), tags));
        }
    }

    entries.sort_by(|a, b| a.0.cmp(&b.0));

    let max_len = entries.iter().map(|e| e.0.len()).max().unwrap_or(0);
    for (endpoint, summary, tags) in entries {
        println!("{endpoint:width$}  {summary}", width = max_len);
        if !tags.is_empty() {
            println!("{:width$}  [{tags}]", "", width = max_len);
        }
    }

    Ok(())
}

pub fn list_tags() -> anyhow::Result<()> {
    let doc = load_doc()?;
    let mut tags: Vec<String> = doc
        .paths
        .values()
        .flat_map(|item| item.operations())
        .flat_map(|(_, op)| op.tags.iter().cloned())
        .collect();
    tags.sort();
    tags.dedup();
    for tag in tags {
        println!("{tag}");
    }
    Ok(())
}
