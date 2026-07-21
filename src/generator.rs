use crate::model::{Handler, HandlerKind, Integration, Project};
use crate::templates::Templates;
use std::collections::{HashMap, HashSet};
use std::time::{SystemTime, UNIX_EPOCH};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Severity {
    Error,
    Warning,
}

#[derive(Clone, Debug)]
pub struct GenerationIssue {
    pub severity: Severity,
    pub message: String,
}

pub struct GenerationResult {
    pub text: String,
    pub issues: Vec<GenerationIssue>,
    pub selected_count: usize,
}

pub fn generate(
    project: &Project,
    selected: &HashSet<String>,
    templates: &Templates,
) -> GenerationResult {
    let mut issues = Vec::new();
    let mut handlers: Vec<&Handler> = project
        .handlers
        .iter()
        .filter(|h| selected.contains(&h.id))
        .collect();
    handlers.sort_by_key(|h| (h.kind, h.integration, h.name.to_lowercase()));
    if handlers.is_empty() {
        issues.push(error("Не выбран ни один обработчик или функция"));
    }
    validate_template(
        &templates.module,
        "шаблон модуля",
        &["{{FROM_PLATFORM}}", "{{TO_PLATFORM}}", "{{FUNCTIONS}}"],
        &mut issues,
    );
    validate_template(
        &templates.subscription_from_platform,
        "шаблон входящего обработчика FromPlatform",
        &["{{NAME}}", "{{CODE}}"],
        &mut issues,
    );
    validate_template(
        &templates.subscription_to_platform,
        "шаблон исходящего обработчика ToPlatform",
        &["{{NAME}}", "{{CODE}}"],
        &mut issues,
    );
    validate_template(
        &templates.function,
        "шаблон функции",
        &["{{NAME}}", "{{CODE}}"],
        &mut issues,
    );

    let mut names: HashMap<String, String> = HashMap::new();
    let mut from_platform = Vec::new();
    let mut to_platform = Vec::new();
    let mut functions = Vec::new();

    for handler in &handlers {
        let generated_name = identifier(&handler.name);
        if generated_name != handler.name {
            issues.push(warning(format!(
                "Имя «{}» преобразовано в допустимый идентификатор «{generated_name}»",
                handler.name
            )));
        }
        let key = generated_name.to_lowercase();
        if let Some(previous) = names.insert(key, handler.id.clone()) {
            issues.push(error(format!(
                "Дублируется имя метода «{generated_name}» (EntityId {previous} и {})",
                handler.id
            )));
        }
        if handler.code.trim().is_empty() {
            issues.push(error(format!(
                "Пустой или недоступный файл кода: {}",
                handler.code_path.display()
            )));
        }
        let item_template = match (handler.kind, handler.integration) {
            (HandlerKind::Function, _) => &templates.function,
            (HandlerKind::Subscription, Integration::FromPlatform) => {
                &templates.subscription_from_platform
            }
            (HandlerKind::Subscription, _) => &templates.subscription_to_platform,
        };
        let rendered = render_item(item_template, handler, &generated_name);
        if rendered.contains("{{") {
            issues.push(warning(format!(
                "В сгенерированном методе «{}» остались неизвестные плейсхолдеры",
                handler.name
            )));
        }
        match handler.kind {
            HandlerKind::Function => functions.push(rendered),
            HandlerKind::Subscription if handler.integration == Integration::FromPlatform => {
                from_platform.push(rendered)
            }
            HandlerKind::Subscription => to_platform.push(rendered),
        }
    }

    let generated_at = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| format!("Unix {}", d.as_secs()))
        .unwrap_or_else(|_| "время не определено".to_owned());
    let replacements = [
        ("{{GENERATED_AT}}", generated_at),
        ("{{SELECTED_COUNT}}", handlers.len().to_string()),
        (
            "{{REGION_FROM_PLATFORM}}",
            identifier(&templates.region_from_platform),
        ),
        (
            "{{REGION_TO_PLATFORM}}",
            identifier(&templates.region_to_platform),
        ),
        (
            "{{REGION_FUNCTIONS}}",
            identifier(&templates.region_functions),
        ),
        ("{{FROM_PLATFORM}}", from_platform.join("\n")),
        ("{{TO_PLATFORM}}", to_platform.join("\n")),
        ("{{FUNCTIONS}}", functions.join("\n")),
    ];
    let mut text = templates.module.clone();
    for (marker, value) in replacements {
        text = text.replace(marker, &value);
    }
    if text.contains("{{") {
        issues.push(warning(
            "В итоговом модуле остались неизвестные плейсхолдеры",
        ));
    }
    text = text
        .replace("\r\n", "\n")
        .replace('\r', "\n")
        .replace('\n', "\r\n");
    GenerationResult {
        text,
        issues,
        selected_count: handlers.len(),
    }
}

fn render_item(template: &str, handler: &Handler, name: &str) -> String {
    let parameters = handler
        .parameters
        .iter()
        .map(|p| identifier(p))
        .collect::<Vec<_>>()
        .join(", ");
    let pairs = [
        ("{{NAME}}", name.to_owned()),
        ("{{ORIGINAL_NAME}}", handler.name.clone()),
        ("{{TYPE}}", handler.kind.label().to_owned()),
        ("{{INTEGRATION}}", handler.integration.label().to_owned()),
        ("{{ENTITY_ID}}", handler.id.clone()),
        ("{{SOURCE_PATH}}", handler.code_path.display().to_string()),
        ("{{PARAMETERS}}", parameters),
        ("{{CODE}}", handler.code.trim().to_owned()),
    ];
    let mut result = template.to_owned();
    for (marker, value) in pairs {
        result = result.replace(marker, &value);
    }
    result.trim().to_owned()
}

fn validate_template(
    template: &str,
    name: &str,
    required: &[&str],
    issues: &mut Vec<GenerationIssue>,
) {
    for marker in required {
        if !template.contains(marker) {
            issues.push(error(format!(
                "{name}: отсутствует обязательный плейсхолдер {marker}"
            )));
        }
    }
}

pub fn identifier(value: &str) -> String {
    let mut result = String::new();
    for ch in value.trim().chars() {
        if ch == '_' || ch.is_alphanumeric() {
            result.push(ch);
        } else {
            result.push('_');
        }
    }
    if result.is_empty() {
        return "БезИмени".to_owned();
    }
    if result.chars().next().is_some_and(|ch| ch.is_numeric()) {
        result.insert(0, '_');
    }
    result
}

fn error(message: impl Into<String>) -> GenerationIssue {
    GenerationIssue {
        severity: Severity::Error,
        message: message.into(),
    }
}
fn warning(message: impl Into<String>) -> GenerationIssue {
    GenerationIssue {
        severity: Severity::Warning,
        message: message.into(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::load_fixture;

    #[test]
    fn identifier_replaces_invalid_characters() {
        assert_eq!(identifier(" 1 test-handler "), "_1_test_handler");
        assert_eq!(identifier("Обработчик_1С"), "Обработчик_1С");
    }

    #[test]
    fn generates_function_with_parameters_and_code() {
        let mut project = load_fixture();
        let handler = project.handlers.first_mut().expect("fixture has handlers");
        handler.kind = HandlerKind::Function;
        handler.integration = Integration::ToPlatform;
        handler.parameters = vec!["Parameter".into()];
        handler.code = "Result = Parameter;".into();
        let handler_id = handler.id.clone();
        let handler_name = handler.name.clone();
        project.handlers.truncate(1);
        let selected = HashSet::from([handler_id]);
        let result = generate(&project, &selected, &Templates::default());
        assert!(result.text.contains(&format!("{handler_name}(Parameter)")));
        assert!(result.text.contains("Result = Parameter;"));
        assert!(!result.issues.iter().any(|i| i.severity == Severity::Error));
    }

    #[test]
    fn uses_separate_templates_for_handler_directions() {
        let mut project = load_fixture();
        let incoming = project
            .handlers
            .iter()
            .find(|handler| handler.integration == Integration::FromPlatform)
            .expect("fixture has an incoming handler")
            .clone();
        let outgoing = project
            .handlers
            .iter()
            .find(|handler| handler.integration == Integration::ToPlatform)
            .expect("fixture has an outgoing handler")
            .clone();
        let incoming_name = incoming.name.clone();
        let outgoing_name = outgoing.name.clone();
        let selected = HashSet::from([incoming.id.clone(), outgoing.id.clone()]);
        project.handlers = vec![incoming, outgoing];
        let mut templates = Templates::default();
        templates.subscription_from_platform = "// IN {{NAME}}\n{{CODE}}".into();
        templates.subscription_to_platform = "// OUT {{NAME}}\n{{CODE}}".into();

        let result = generate(&project, &selected, &templates);

        assert!(result.text.contains(&format!("// IN {incoming_name}")));
        assert!(result.text.contains(&format!("// OUT {outgoing_name}")));
    }

    #[test]
    fn empty_selection_is_reported_as_error() {
        let project = load_fixture();
        let result = generate(&project, &HashSet::new(), &Templates::default());

        assert!(result.text.contains("#Область"));
        assert!(result.issues.iter().any(|issue| {
            issue.severity == Severity::Error && issue.message.contains("Не выбран")
        }));
    }

    #[test]
    fn duplicate_generated_method_names_are_reported() {
        let mut project = load_fixture();
        let mut first = project.handlers[0].clone();
        let mut second = project.handlers[1].clone();
        first.name = "Duplicate name".into();
        second.name = "Duplicate-name".into();
        let selected = HashSet::from([first.id.clone(), second.id.clone()]);
        project.handlers = vec![first, second];

        let result = generate(&project, &selected, &Templates::default());

        assert!(result.issues.iter().any(|issue| {
            issue.severity == Severity::Error && issue.message.contains("Дублируется имя метода")
        }));
    }

    #[test]
    fn missing_required_template_marker_is_reported() {
        let mut templates = Templates::default();
        templates.function = "Функция БезМаркеров()\nКонецФункции".into();
        let mut project = load_fixture();
        let handler = project.handlers.first_mut().expect("fixture has handlers");
        handler.kind = HandlerKind::Function;
        let handler_id = handler.id.clone();
        project.handlers.truncate(1);

        let result = generate(&project, &HashSet::from([handler_id]), &templates);

        assert!(result.issues.iter().any(|issue| {
            issue.severity == Severity::Error && issue.message.contains("{{NAME}}")
        }));
        assert!(result.issues.iter().any(|issue| {
            issue.severity == Severity::Error && issue.message.contains("{{CODE}}")
        }));
    }

    #[test]
    fn generated_module_uses_windows_line_endings() {
        let project = load_fixture();
        let result = generate(&project, &HashSet::new(), &Templates::default());
        assert!(result.text.contains("\r\n"));
        assert!(!result.text.replace("\r\n", "").contains('\n'));
    }
}
