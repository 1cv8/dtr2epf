use serde_json::Value;
use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};
use walkdir::WalkDir;

#[derive(Clone, Debug)]
pub struct Adapter {
    pub id: String,
    pub name: String,
    pub source_path: PathBuf,
    pub handler_ids: Vec<String>,
}

#[derive(Clone, Debug)]
pub struct Handler {
    pub id: String,
    pub folder_id: String,
    pub name: String,
    pub kind: HandlerKind,
    pub integration: Integration,
    pub subscription_object: String,
    pub parameters: Vec<String>,
    pub code: String,
    pub code_path: PathBuf,
    pub source_order: usize,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd)]
pub enum HandlerKind {
    Subscription,
    Function,
}

impl HandlerKind {
    pub fn label(self) -> &'static str {
        match self {
            Self::Subscription => "Обработчик 1С",
            Self::Function => "Функция 1С",
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd)]
pub enum Integration {
    FromPlatform,
    ToPlatform,
    Unknown,
}

impl Integration {
    pub fn label(self) -> &'static str {
        match self {
            Self::FromPlatform => "В 1С (FromPlatform)",
            Self::ToPlatform => "Из 1С (ToPlatform)",
            Self::Unknown => "Направление не задано",
        }
    }
}

#[derive(Clone, Debug)]
pub struct Folder {
    pub id: String,
    pub parent_id: String,
    pub name: String,
    pub source_order: usize,
}

#[derive(Default, Debug)]
pub struct Project {
    pub root: PathBuf,
    pub adapters: Vec<Adapter>,
    pub handlers: Vec<Handler>,
    pub folders: Vec<Folder>,
    pub warnings: Vec<String>,
}

#[derive(Clone, Debug)]
pub enum TreeRowKind {
    Folder(String),
    Handler(String),
}

#[derive(Clone, Debug)]
pub struct TreeRow {
    pub kind: TreeRowKind,
    pub depth: usize,
    pub name: String,
    pub detail: String,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum TreeSort {
    Source,
    Name,
    Type,
}

impl Project {
    pub fn load(root: &Path) -> Result<Self, String> {
        let adapters_dir = root.join("Adapters");
        let handlers_dir = root.join("Metadata").join("Handlers");
        if !adapters_dir.is_dir() {
            return Err(format!("Не найден каталог {}", adapters_dir.display()));
        }
        if !handlers_dir.is_dir() {
            return Err(format!("Не найден каталог {}", handlers_dir.display()));
        }

        let mut result = Self {
            root: root.to_path_buf(),
            ..Self::default()
        };
        result.load_handlers(&handlers_dir);
        result.load_adapters(&adapters_dir);

        result
            .adapters
            .sort_by(|a, b| natural_key(&a.name).cmp(&natural_key(&b.name)));
        Ok(result)
    }

    fn load_handlers(&mut self, dir: &Path) {
        let mut order = 0usize;
        for entry in WalkDir::new(dir)
            .follow_links(false)
            .into_iter()
            .filter_map(Result::ok)
        {
            let path = entry.path();
            if !entry.file_type().is_file()
                || path.extension().and_then(|v| v.to_str()) != Some("json")
            {
                continue;
            }
            order += 1;
            let Some(json) = read_json(path, &mut self.warnings) else {
                continue;
            };

            if json.get("ChildrenType").and_then(Value::as_str) == Some("Handlers") {
                let id = string_field(&json, "EntityId");
                if !id.is_empty() {
                    self.folders.push(Folder {
                        id,
                        parent_id: string_field(&json, "FolderId"),
                        name: string_field(&json, "Name"),
                        source_order: order,
                    });
                }
                continue;
            }

            let kind = match json.get("Type").and_then(Value::as_str) {
                Some("Subscription1C") => HandlerKind::Subscription,
                Some("Function1C") => HandlerKind::Function,
                _ => continue,
            };
            let integration = match json.get("Integration").and_then(Value::as_str) {
                Some("FromPlatform") => Integration::FromPlatform,
                Some("ToPlatform") => Integration::ToPlatform,
                _ => Integration::Unknown,
            };
            let subscription_object =
                if kind == HandlerKind::Subscription && integration == Integration::ToPlatform {
                    string_field(&json, "SubscriptionObject")
                } else {
                    String::new()
                };
            let id = string_field(&json, "EntityId");
            let name = string_field(&json, "Name");
            let code_name = string_field(&json, "Code");
            if id.is_empty() || name.is_empty() {
                self.warnings
                    .push(format!("Пропущен неполный обработчик: {}", path.display()));
                continue;
            }
            let code_path = if code_name.is_empty() {
                let expected_name = format!(
                    "{} [Code].ext",
                    path.file_stem()
                        .and_then(|value| value.to_str())
                        .unwrap_or(&name)
                );
                self.warnings.push(format!(
                    "У обработчика «{name}» не задано поле Code; ожидаемый файл: {expected_name}"
                ));
                path.parent().unwrap_or(dir).join(expected_name)
            } else {
                path.parent().unwrap_or(dir).join(code_name)
            };
            let code = match fs::read_to_string(&code_path) {
                Ok(value) => normalize_newlines(&value),
                Err(error) => {
                    self.warnings
                        .push(format!("Не прочитан код {}: {error}", code_path.display()));
                    String::new()
                }
            };
            let parameters = json
                .get("Parameters")
                .and_then(Value::as_array)
                .into_iter()
                .flatten()
                .filter_map(|item| item.get("Name").and_then(Value::as_str))
                .filter(|name| !name.trim().is_empty())
                .map(str::to_owned)
                .collect();
            self.handlers.push(Handler {
                id,
                folder_id: string_field(&json, "FolderId"),
                name,
                kind,
                integration,
                subscription_object,
                parameters,
                code,
                code_path,
                source_order: order,
            });
        }
    }

    fn load_adapters(&mut self, dir: &Path) {
        for entry in WalkDir::new(dir)
            .max_depth(3)
            .follow_links(false)
            .into_iter()
            .filter_map(Result::ok)
        {
            let path = entry.path();
            if !entry.file_type().is_file()
                || path.extension().and_then(|v| v.to_str()) != Some("json")
            {
                continue;
            }
            let Some(json) = read_json(path, &mut self.warnings) else {
                continue;
            };
            let Some(config) = json.get("Config") else {
                continue;
            };
            if config.get("ConnectorType").and_then(Value::as_str) != Some("_1C") {
                continue;
            }
            let handler_ids = config
                .get("HandlersList")
                .and_then(Value::as_array)
                .into_iter()
                .flatten()
                .filter_map(|item| item.get("HandlerId").and_then(Value::as_str))
                .map(str::to_owned)
                .collect();
            self.adapters.push(Adapter {
                id: string_field(&json, "EntityId"),
                name: string_field(&json, "Name"),
                source_path: path.to_path_buf(),
                handler_ids,
            });
        }
    }

    pub fn handler(&self, id: &str) -> Option<&Handler> {
        self.handlers.iter().find(|item| item.id == id)
    }

    pub fn folder_descendants(&self, folder_id: &str) -> Vec<String> {
        let mut folder_ids = HashSet::from([folder_id.to_owned()]);
        loop {
            let before = folder_ids.len();
            for folder in &self.folders {
                if folder_ids.contains(&folder.parent_id) {
                    folder_ids.insert(folder.id.clone());
                }
            }
            if folder_ids.len() == before {
                break;
            }
        }
        self.handlers
            .iter()
            .filter(|handler| folder_ids.contains(&handler.folder_id))
            .map(|handler| handler.id.clone())
            .collect()
    }

    pub fn tree_rows(
        &self,
        expanded: &HashSet<String>,
        sort: TreeSort,
        filter: &str,
    ) -> Vec<TreeRow> {
        let folder_by_id: HashMap<&str, &Folder> =
            self.folders.iter().map(|f| (f.id.as_str(), f)).collect();
        let known_folders: HashSet<&str> = folder_by_id.keys().copied().collect();
        let mut child_folders: HashMap<&str, Vec<&Folder>> = HashMap::new();
        for folder in &self.folders {
            child_folders
                .entry(folder.parent_id.as_str())
                .or_default()
                .push(folder);
        }
        let mut child_handlers: HashMap<&str, Vec<&Handler>> = HashMap::new();
        for handler in &self.handlers {
            child_handlers
                .entry(handler.folder_id.as_str())
                .or_default()
                .push(handler);
        }
        for children in child_folders.values_mut() {
            sort_folders(children, sort);
        }
        for children in child_handlers.values_mut() {
            sort_handlers(children, sort);
        }

        let filter = filter.trim().to_lowercase();
        if !filter.is_empty() {
            let mut rows = Vec::new();
            let mut matches: Vec<&Handler> = self
                .handlers
                .iter()
                .filter(|h| {
                    h.name.to_lowercase().contains(&filter)
                        || h.kind.label().to_lowercase().contains(&filter)
                        || h.integration.label().to_lowercase().contains(&filter)
                })
                .collect();
            sort_handlers(&mut matches, sort);
            for handler in matches {
                let folder_path = self.folder_path(&handler.folder_id);
                rows.push(TreeRow {
                    kind: TreeRowKind::Handler(handler.id.clone()),
                    depth: 0,
                    name: if folder_path.is_empty() {
                        handler.name.clone()
                    } else {
                        format!("{folder_path} / {}", handler.name)
                    },
                    detail: format!("{} · {}", handler.kind.label(), handler.integration.label()),
                });
            }
            return rows;
        }

        let mut roots: Vec<&Folder> = self
            .folders
            .iter()
            .filter(|folder| !known_folders.contains(folder.parent_id.as_str()))
            .collect();
        sort_folders(&mut roots, sort);
        let mut rows = Vec::new();
        for folder in roots {
            append_folder_rows(
                folder,
                0,
                expanded,
                &child_folders,
                &child_handlers,
                &mut rows,
            );
        }
        let mut orphan_handlers: Vec<&Handler> = self
            .handlers
            .iter()
            .filter(|handler| !known_folders.contains(handler.folder_id.as_str()))
            .collect();
        sort_handlers(&mut orphan_handlers, sort);
        for handler in orphan_handlers {
            rows.push(handler_row(handler, 0));
        }
        rows
    }

    fn folder_path(&self, folder_id: &str) -> String {
        let map: HashMap<&str, &Folder> = self.folders.iter().map(|f| (f.id.as_str(), f)).collect();
        let mut current = folder_id;
        let mut result = Vec::new();
        let mut visited = HashSet::new();
        while let Some(folder) = map.get(current) {
            if !visited.insert(current) {
                break;
            }
            result.push(folder.name.as_str());
            current = folder.parent_id.as_str();
        }
        result.reverse();
        result.join(" / ")
    }
}

fn append_folder_rows(
    folder: &Folder,
    depth: usize,
    expanded: &HashSet<String>,
    child_folders: &HashMap<&str, Vec<&Folder>>,
    child_handlers: &HashMap<&str, Vec<&Handler>>,
    rows: &mut Vec<TreeRow>,
) {
    rows.push(TreeRow {
        kind: TreeRowKind::Folder(folder.id.clone()),
        depth,
        name: folder.name.clone(),
        detail: "Группа".to_owned(),
    });
    if !expanded.contains(&folder.id) {
        return;
    }
    if let Some(children) = child_folders.get(folder.id.as_str()) {
        for child in children {
            append_folder_rows(
                child,
                depth + 1,
                expanded,
                child_folders,
                child_handlers,
                rows,
            );
        }
    }
    if let Some(children) = child_handlers.get(folder.id.as_str()) {
        for handler in children {
            rows.push(handler_row(handler, depth + 1));
        }
    }
}

fn handler_row(handler: &Handler, depth: usize) -> TreeRow {
    TreeRow {
        kind: TreeRowKind::Handler(handler.id.clone()),
        depth,
        name: handler.name.clone(),
        detail: format!("{} · {}", handler.kind.label(), handler.integration.label()),
    }
}

fn sort_folders(items: &mut Vec<&Folder>, sort: TreeSort) {
    items.sort_by(|a, b| match sort {
        TreeSort::Source => a.source_order.cmp(&b.source_order),
        TreeSort::Name | TreeSort::Type => natural_key(&a.name).cmp(&natural_key(&b.name)),
    });
}

fn sort_handlers(items: &mut Vec<&Handler>, sort: TreeSort) {
    items.sort_by(|a, b| match sort {
        TreeSort::Source => a.source_order.cmp(&b.source_order),
        TreeSort::Name => natural_key(&a.name).cmp(&natural_key(&b.name)),
        TreeSort::Type => (a.kind, a.integration, natural_key(&a.name)).cmp(&(
            b.kind,
            b.integration,
            natural_key(&b.name),
        )),
    });
}

fn natural_key(value: &str) -> String {
    value.to_lowercase()
}

fn string_field(json: &Value, name: &str) -> String {
    json.get(name)
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_owned()
}

fn read_json(path: &Path, warnings: &mut Vec<String>) -> Option<Value> {
    let text = match fs::read_to_string(path) {
        Ok(value) => value,
        Err(error) => {
            warnings.push(format!("Не прочитан JSON {}: {error}", path.display()));
            return None;
        }
    };
    match serde_json::from_str(&text) {
        Ok(value) => Some(value),
        Err(error) => {
            warnings.push(format!("Некорректный JSON {}: {error}", path.display()));
            None
        }
    }
}

fn normalize_newlines(value: &str) -> String {
    value.replace("\r\n", "\n").replace('\r', "\n")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::{copy_fixture, fixture_path, load_fixture};
    use std::fs;

    #[test]
    fn loads_datareon_project_and_filters_types() {
        let project = load_fixture();

        assert_eq!(project.adapters.len(), 5);
        assert_eq!(project.handlers.len(), 106);
        assert_eq!(project.folders.len(), 3);
        assert!(project.warnings.is_empty());
        assert!(project.handlers.iter().all(|handler| {
            handler.kind == HandlerKind::Subscription
                && !handler.code.is_empty()
                && handler.code_path.is_file()
        }));
    }

    #[test]
    fn folder_selection_includes_nested_handlers() {
        let project = load_fixture();
        let root = project
            .folders
            .iter()
            .find(|folder| folder.name.eq_ignore_ascii_case("mdm"))
            .expect("fixture must contain the Mdm handler folder");
        let descendants: HashSet<String> =
            project.folder_descendants(&root.id).into_iter().collect();

        assert_eq!(descendants.len(), project.handlers.len());
        assert!(
            project
                .handlers
                .iter()
                .all(|handler| descendants.contains(&handler.id))
        );
    }

    #[test]
    fn tree_filter_preserves_handler_path_and_type() {
        let project = load_fixture();
        let handler = project.handlers.first().expect("fixture has handlers");
        let rows = project.tree_rows(&HashSet::new(), TreeSort::Name, &handler.name);

        assert_eq!(rows.len(), 1);
        assert!(rows[0].name.ends_with(&handler.name));
        assert!(rows[0].detail.contains(handler.kind.label()));
    }

    #[test]
    fn handler_without_code_is_visible_with_warning() {
        let temp = copy_fixture();
        let project = Project::load(temp.path()).unwrap();
        let handler = project.handlers.first().expect("fixture has handlers");
        let handler_id = handler.id.clone();
        let json_path = handler
            .code_path
            .parent()
            .unwrap()
            .join(format!("[Handler] {}.json", handler.name));
        let text = fs::read_to_string(&json_path).unwrap();
        let mut json: Value = serde_json::from_str(&text).unwrap();
        json.as_object_mut().unwrap().remove("Code");
        fs::write(&json_path, serde_json::to_vec_pretty(&json).unwrap()).unwrap();
        fs::remove_file(&handler.code_path).unwrap();

        let project = Project::load(temp.path()).unwrap();

        assert!(project.handler(&handler_id).is_some());
        assert!(project.handler(&handler_id).unwrap().code.is_empty());
        assert!(
            project
                .warnings
                .iter()
                .any(|warning| warning.contains("не задано поле Code"))
        );
    }

    #[test]
    fn rejects_directory_without_required_structure() {
        let error = Project::load(&fixture_path().join("Metadata")).unwrap_err();
        assert!(error.contains("Adapters"));
    }
}
