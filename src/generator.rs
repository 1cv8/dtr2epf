use crate::model::{Handler, HandlerKind, Integration, Project};
use crate::templates::Templates;
use std::collections::{HashMap, HashSet};
use std::time::{SystemTime, UNIX_EPOCH};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Severity {
    Error,
    Warning,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum GenerationVariant {
    Debug,
    SyntaxControl,
}

impl GenerationVariant {
    pub fn label(self) -> &'static str {
        match self {
            Self::Debug => "Для отладки",
            Self::SyntaxControl => "Для синтаксического контроля",
        }
    }
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
    variant: GenerationVariant,
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
    let (subscription_from_platform, subscription_to_platform) = match variant {
        GenerationVariant::Debug => (
            &templates.subscription_from_platform,
            &templates.subscription_to_platform,
        ),
        GenerationVariant::SyntaxControl => (
            &templates.syntax_control_subscription_from_platform,
            &templates.syntax_control_subscription_to_platform,
        ),
    };
    validate_template(
        subscription_from_platform,
        "шаблон входящего обработчика FromPlatform",
        &["{{NAME}}", "{{CODE}}"],
        &mut issues,
    );
    validate_template(
        subscription_to_platform,
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
            (HandlerKind::Subscription, Integration::FromPlatform) => subscription_from_platform,
            (HandlerKind::Subscription, _) => subscription_to_platform,
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
        ("{{FROM_PLATFORM}}", join_rendered_items(&from_platform)),
        ("{{TO_PLATFORM}}", join_rendered_items(&to_platform)),
        ("{{FUNCTIONS}}", join_rendered_items(&functions)),
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

fn join_rendered_items(items: &[String]) -> String {
    items.join("\n\n")
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
        (
            "{{SUBSCRIPTION_OBJECT}}",
            handler.subscription_object.clone(),
        ),
        (
            "{{SUBSCRIPTION_OBJECT_MANAGER}}",
            subscription_object_manager(&handler.subscription_object),
        ),
        (
            "{{SUBSCRIPTION_OBJECT_TYPE_REF}}",
            subscription_object_type_ref(&handler.subscription_object),
        ),
        (
            "{{SUBSCRIPTION_OBJECT_TYPE_OBJ}}",
            subscription_object_type_obj(&handler.subscription_object),
        ),
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

fn subscription_object_manager(subscription_object: &str) -> String {
    let parts: Vec<&str> = subscription_object.split('.').map(str::trim).collect();
    if parts.len() < 2 || parts[0].is_empty() || parts[1].is_empty() {
        return String::new();
    }

    let class = parts[0].to_uppercase();
    let object_name = parts[1];
    if class == "РЕГИСТРРАСЧЕТА" {
        return match parts.as_slice() {
            [_, object_name] => format!("РегистрыРасчета.{object_name}"),
            [_, object_name, child_class, child_name]
                if child_class.to_uppercase() == "ПЕРЕРАСЧЕТ" && !child_name.is_empty() =>
            {
                format!("РегистрыРасчета.{object_name}.Перерасчеты.{child_name}")
            }
            _ => String::new(),
        };
    }

    let manager = match class.as_str() {
        "ПЛАНОБМЕНА" => "ПланыОбмена",
        "СПРАВОЧНИК" => "Справочники",
        "ДОКУМЕНТ" => "Документы",
        "ЖУРНАЛДОКУМЕНТОВ" => "ЖурналыДокументов",
        "ПЕРЕЧИСЛЕНИЕ" => "Перечисления",
        "ОТЧЕТ" => "Отчеты",
        "ОБРАБОТКА" => "Обработки",
        "ПЛАНВИДОВХАРАКТЕРИСТИК" => "ПланыВидовХарактеристик",
        "ПЛАНСЧЕТОВ" => "ПланыСчетов",
        "ПЛАНВИДОВРАСЧЕТА" => "ПланыВидовРасчета",
        "РЕГИСТРСВЕДЕНИЙ" => "РегистрыСведений",
        "РЕГИСТРНАКОПЛЕНИЯ" => "РегистрыНакопления",
        "РЕГИСТРБУХГАЛТЕРИИ" => "РегистрыБухгалтерии",
        "БИЗНЕСПРОЦЕСС" => "БизнесПроцессы",
        "ЗАДАЧА" => "Задачи",
        "КОНСТАНТА" => "Константы",
        "ПОСЛЕДОВАТЕЛЬНОСТЬ" => "Последовательности",
        _ => return String::new(),
    };
    format!("{manager}.{object_name}")
}

const SUBSCRIPTION_OBJECT_TYPES: &[(&str, &str, &str)] = &[
    ("ПЛАНОБМЕНА", "ПланОбменаСсылка", "ПланОбменаОбъект"),
    ("СПРАВОЧНИК", "СправочникСсылка", "СправочникОбъект"),
    ("ДОКУМЕНТ", "ДокументСсылка", "ДокументОбъект"),
    ("ПЕРЕЧИСЛЕНИЕ", "ПеречислениеСсылка", "ПеречислениеСсылка"),
    ("ОТЧЕТ", "ОтчетОбъект", "ОтчетОбъект"),
    ("ОБРАБОТКА", "ОбработкаОбъект", "ОбработкаОбъект"),
    (
        "ПЛАНВИДОВХАРАКТЕРИСТИК",
        "ПланВидовХарактеристикСсылка",
        "ПланВидовХарактеристикОбъект",
    ),
    ("ПЛАНСЧЕТОВ", "ЛюбаяСсылка", "ЛюбаяСсылка"),
    ("ПЛАНВИДОВРАСЧЕТА", "ЛюбаяСсылка", "ЛюбаяСсылка"),
    (
        "РЕГИСТРСВЕДЕНИЙ",
        "РегистрСведенийНаборЗаписей",
        "РегистрСведенийНаборЗаписей",
    ),
    (
        "РЕГИСТРНАКОПЛЕНИЯ",
        "РегистрСведенийНаборЗаписей",
        "РегистрСведенийНаборЗаписей",
    ),
    (
        "РЕГИСТРБУХГАЛТЕРИИ",
        "РегистрСведенийНаборЗаписей",
        "РегистрСведенийНаборЗаписей",
    ),
    (
        "РЕГИСТРРАСЧЕТА",
        "РегистрСведенийНаборЗаписей",
        "РегистрСведенийНаборЗаписей",
    ),
    (
        "БИЗНЕСПРОЦЕСС",
        "БизнесПроцессСсылка",
        "БизнесПроцессОбъект",
    ),
    ("ЗАДАЧА", "ЗадачаСсылка", "ЗадачаОбъект"),
    ("КОНСТАНТА", "КонстантаМенеджер", "КонстантаМенеджер"),
    ("ПОСЛЕДОВАТЕЛЬНОСТЬ", "ЛюбаяСсылка", "ЛюбаяСсылка"),
];

fn subscription_object_type_ref(subscription_object: &str) -> String {
    subscription_object_type(subscription_object, SubscriptionObjectType::Reference)
}

fn subscription_object_type_obj(subscription_object: &str) -> String {
    subscription_object_type(subscription_object, SubscriptionObjectType::Object)
}

enum SubscriptionObjectType {
    Reference,
    Object,
}

fn subscription_object_type(
    subscription_object: &str,
    requested_type: SubscriptionObjectType,
) -> String {
    let Some((class, object_name)) = subscription_object.split_once('.') else {
        return String::new();
    };
    let class = class.trim().to_uppercase();
    let object_name = object_name.trim();
    if class.is_empty() || object_name.is_empty() {
        return String::new();
    }
    let Some((_, reference_type, object_type)) = SUBSCRIPTION_OBJECT_TYPES
        .iter()
        .find(|(known_class, _, _)| *known_class == class)
    else {
        return String::new();
    };
    let type_name = match requested_type {
        SubscriptionObjectType::Reference => reference_type,
        SubscriptionObjectType::Object => object_type,
    };
    format!("{type_name}.{object_name}")
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
    fn rendered_items_have_one_empty_line_between_them() {
        let items = vec!["FirstHandler".to_owned(), "SecondHandler".to_owned()];

        assert_eq!(join_rendered_items(&items), "FirstHandler\n\nSecondHandler");
    }

    #[test]
    fn converts_subscription_object_classes_to_manager_names() {
        let cases = [
            ("ПланОбмена.Test", "ПланыОбмена.Test"),
            ("Справочник.Test", "Справочники.Test"),
            ("Документ.Test", "Документы.Test"),
            ("ЖурналДокументов.Test", "ЖурналыДокументов.Test"),
            ("Перечисление.Test", "Перечисления.Test"),
            ("Отчет.Test", "Отчеты.Test"),
            ("Обработка.Test", "Обработки.Test"),
            (
                "ПланВидовХарактеристик.Test",
                "ПланыВидовХарактеристик.Test",
            ),
            ("ПланСчетов.Test", "ПланыСчетов.Test"),
            ("ПланВидовРасчета.Test", "ПланыВидовРасчета.Test"),
            ("РегистрСведений.Test", "РегистрыСведений.Test"),
            ("РегистрНакопления.Test", "РегистрыНакопления.Test"),
            ("РегистрБухгалтерии.Test", "РегистрыБухгалтерии.Test"),
            ("РегистрРасчета.Test", "РегистрыРасчета.Test"),
            ("БизнесПроцесс.Test", "БизнесПроцессы.Test"),
            ("Задача.Test", "Задачи.Test"),
            ("Константа.Test", "Константы.Test"),
            ("Последовательность.Test", "Последовательности.Test"),
        ];

        for (source, expected) in cases {
            assert_eq!(subscription_object_manager(source), expected, "{source}");
        }
        assert_eq!(
            subscription_object_manager("справочник.Автомобили"),
            "Справочники.Автомобили"
        );
    }

    #[test]
    fn converts_calculation_register_recalculation_manager() {
        assert_eq!(
            subscription_object_manager("РегистрРасчета.Payroll.Перерасчет.Adjustment"),
            "РегистрыРасчета.Payroll.Перерасчеты.Adjustment"
        );
        assert!(subscription_object_manager("РегистрРасчета.Payroll.Unknown.Item").is_empty());
        assert!(subscription_object_manager("Unknown.Test").is_empty());
        assert!(subscription_object_manager("Справочник").is_empty());
        assert!(subscription_object_manager("Справочник..Test").is_empty());
    }

    #[test]
    fn converts_subscription_object_classes_to_reference_and_object_types() {
        let cases = [
            ("ПланОбмена", "ПланОбменаСсылка", "ПланОбменаОбъект"),
            ("Справочник", "СправочникСсылка", "СправочникОбъект"),
            ("Документ", "ДокументСсылка", "ДокументОбъект"),
            ("Перечисление", "ПеречислениеСсылка", "ПеречислениеСсылка"),
            ("Отчет", "ОтчетОбъект", "ОтчетОбъект"),
            ("Обработка", "ОбработкаОбъект", "ОбработкаОбъект"),
            (
                "ПланВидовХарактеристик",
                "ПланВидовХарактеристикСсылка",
                "ПланВидовХарактеристикОбъект",
            ),
            ("ПланСчетов", "ЛюбаяСсылка", "ЛюбаяСсылка"),
            ("ПланВидовРасчета", "ЛюбаяСсылка", "ЛюбаяСсылка"),
            (
                "РегистрСведений",
                "РегистрСведенийНаборЗаписей",
                "РегистрСведенийНаборЗаписей",
            ),
            (
                "РегистрНакопления",
                "РегистрСведенийНаборЗаписей",
                "РегистрСведенийНаборЗаписей",
            ),
            (
                "РегистрБухгалтерии",
                "РегистрСведенийНаборЗаписей",
                "РегистрСведенийНаборЗаписей",
            ),
            (
                "РегистрРасчета",
                "РегистрСведенийНаборЗаписей",
                "РегистрСведенийНаборЗаписей",
            ),
            (
                "БизнесПроцесс",
                "БизнесПроцессСсылка",
                "БизнесПроцессОбъект",
            ),
            ("Задача", "ЗадачаСсылка", "ЗадачаОбъект"),
            ("Константа", "КонстантаМенеджер", "КонстантаМенеджер"),
            ("Последовательность", "ЛюбаяСсылка", "ЛюбаяСсылка"),
        ];

        for (source_class, expected_ref, expected_obj) in cases {
            let source = format!("{source_class}.Test");
            assert_eq!(
                subscription_object_type_ref(&source),
                format!("{expected_ref}.Test"),
                "{source}"
            );
            assert_eq!(
                subscription_object_type_obj(&source),
                format!("{expected_obj}.Test"),
                "{source}"
            );
        }
        assert_eq!(
            subscription_object_type_ref("справочник.Автомобили"),
            "СправочникСсылка.Автомобили"
        );
        assert_eq!(
            subscription_object_type_obj("РегистрРасчета.Payroll.Перерасчет.Adjustment"),
            "РегистрСведенийНаборЗаписей.Payroll.Перерасчет.Adjustment"
        );
        assert!(subscription_object_type_ref("Unknown.Test").is_empty());
        assert!(subscription_object_type_obj("Справочник").is_empty());
        assert!(subscription_object_type_ref("Справочник.").is_empty());
    }

    #[test]
    fn subscription_placeholders_are_filled_only_for_outgoing_handlers() {
        let project = load_fixture();
        let outgoing = project
            .handlers
            .iter()
            .find(|handler| {
                handler.integration == Integration::ToPlatform
                    && !handler.subscription_object.is_empty()
            })
            .expect("fixture has an outgoing handler with SubscriptionObject");
        let incoming = project
            .handlers
            .iter()
            .find(|handler| handler.integration == Integration::FromPlatform)
            .expect("fixture has an incoming handler");
        let template = "{{SUBSCRIPTION_OBJECT}}|{{SUBSCRIPTION_OBJECT_MANAGER}}|{{SUBSCRIPTION_OBJECT_TYPE_REF}}|{{SUBSCRIPTION_OBJECT_TYPE_OBJ}}";

        let outgoing_text = render_item(template, outgoing, &outgoing.name);
        let expected_manager = subscription_object_manager(&outgoing.subscription_object);
        let expected_type_ref = subscription_object_type_ref(&outgoing.subscription_object);
        let expected_type_obj = subscription_object_type_obj(&outgoing.subscription_object);
        assert_eq!(
            outgoing_text,
            format!(
                "{}|{expected_manager}|{expected_type_ref}|{expected_type_obj}",
                outgoing.subscription_object
            )
        );
        assert!(incoming.subscription_object.is_empty());
        assert_eq!(render_item(template, incoming, &incoming.name), "|||");
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
        let result = generate(
            &project,
            &selected,
            &Templates::default(),
            GenerationVariant::Debug,
        );
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

        let result = generate(&project, &selected, &templates, GenerationVariant::Debug);

        assert!(result.text.contains(&format!("// IN {incoming_name}")));
        assert!(result.text.contains(&format!("// OUT {outgoing_name}")));
    }

    #[test]
    fn generation_variant_selects_its_subscription_templates() {
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
        let selected = HashSet::from([incoming.id.clone(), outgoing.id.clone()]);
        project.handlers = vec![incoming, outgoing];
        let mut templates = Templates::default();
        templates.subscription_from_platform = "// DEBUG IN {{NAME}}\n{{CODE}}".into();
        templates.subscription_to_platform = "// DEBUG OUT {{NAME}}\n{{CODE}}".into();
        templates.syntax_control_subscription_from_platform =
            "// SYNTAX IN {{NAME}}\n{{CODE}}".into();
        templates.syntax_control_subscription_to_platform =
            "// SYNTAX OUT {{NAME}}\n{{CODE}}".into();

        let result = generate(
            &project,
            &selected,
            &templates,
            GenerationVariant::SyntaxControl,
        );

        assert!(result.text.contains("// SYNTAX IN"));
        assert!(result.text.contains("// SYNTAX OUT"));
        assert!(!result.text.contains("// DEBUG IN"));
        assert!(!result.text.contains("// DEBUG OUT"));
    }

    #[test]
    fn empty_selection_is_reported_as_error() {
        let project = load_fixture();
        let result = generate(
            &project,
            &HashSet::new(),
            &Templates::default(),
            GenerationVariant::Debug,
        );

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

        let result = generate(
            &project,
            &selected,
            &Templates::default(),
            GenerationVariant::Debug,
        );

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

        let result = generate(
            &project,
            &HashSet::from([handler_id]),
            &templates,
            GenerationVariant::Debug,
        );

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
        let result = generate(
            &project,
            &HashSet::new(),
            &Templates::default(),
            GenerationVariant::Debug,
        );
        assert!(result.text.contains("\r\n"));
        assert!(!result.text.replace("\r\n", "").contains('\n'));
    }
}
