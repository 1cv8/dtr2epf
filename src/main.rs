mod app;
mod generator;
mod model;
mod templates;
#[cfg(test)]
mod test_support;

use app::DtrApp;
use egui::vec2;
use egui_software_backend::SoftwareBackendAppConfiguration;
use model::Project;
use std::path::Path;

fn main() {
    let args: Vec<String> = std::env::args().collect();
    if args.get(1).map(String::as_str) == Some("--check") {
        let Some(path) = args.get(2).map(String::as_str) else {
            eprintln!("usage: dtr2epf --check <Datareon src path>");
            std::process::exit(2);
        };
        match Project::load(Path::new(path)) {
            Ok(project) => {
                println!(
                    "adapters={} handlers={} folders={} warnings={}",
                    project.adapters.len(),
                    project.handlers.len(),
                    project.folders.len(),
                    project.warnings.len()
                );
                for warning in project.warnings.iter().take(20) {
                    eprintln!("warning: {warning}");
                }
                std::process::exit(if project.handlers.is_empty() { 2 } else { 0 });
            }
            Err(error) => {
                eprintln!("error: {error}");
                std::process::exit(1);
            }
        }
    }

    let initial_source = command_line_source(&args);

    let settings = SoftwareBackendAppConfiguration::new()
        .inner_size(Some(vec2(1400.0, 850.0)))
        .min_inner_size(Some(vec2(1000.0, 650.0)))
        .title(Some("Datareon → внешняя обработка 1С".to_owned()));

    if let Err(error) = egui_software_backend::run_app_with_software_backend(settings, move |ctx| {
        DtrApp::new(ctx, initial_source.clone())
    }) {
        let message = format!("Не удалось запустить графический интерфейс:\n{error}");
        eprintln!("{message}");
        rfd::MessageDialog::new()
            .set_title("Ошибка запуска dtr2epf")
            .set_description(&message)
            .set_level(rfd::MessageLevel::Error)
            .show();
    }
}

fn command_line_source(args: &[String]) -> Option<std::path::PathBuf> {
    args.windows(2)
        .find(|pair| pair[0] == "--source" || pair[0] == "-s")
        .map(|pair| std::path::PathBuf::from(&pair[1]))
        .or_else(|| {
            args.iter()
                .find_map(|arg| arg.strip_prefix("--source=").map(std::path::PathBuf::from))
        })
}

#[cfg(test)]
mod tests {
    use super::command_line_source;
    use crate::test_support::fixture_path;

    #[test]
    fn parses_source_argument() {
        let fixture = fixture_path();
        let fixture_text = fixture.to_string_lossy().into_owned();
        let args = vec!["dtr2epf".into(), "--source".into(), fixture_text.clone()];
        assert_eq!(command_line_source(&args), Some(fixture.clone()));

        let args = vec!["dtr2epf".into(), format!("--source={fixture_text}")];
        assert_eq!(command_line_source(&args), Some(fixture));
    }

    #[test]
    fn no_source_argument_means_empty_startup() {
        assert_eq!(command_line_source(&["dtr2epf".into()]), None);
    }
}
