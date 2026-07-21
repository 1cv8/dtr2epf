use serde::{Deserialize, Serialize};
use std::fs;
use std::path::Path;

#[derive(Clone, Serialize, Deserialize)]
pub struct Templates {
    pub module: String,
    pub subscription_from_platform: String,
    pub subscription_to_platform: String,
    pub function: String,
    pub region_from_platform: String,
    pub region_to_platform: String,
    pub region_functions: String,
}

#[derive(Deserialize)]
struct EmbeddedSettings {
    region_from_platform: String,
    region_to_platform: String,
    region_functions: String,
}

impl Default for Templates {
    fn default() -> Self {
        let settings: EmbeddedSettings =
            serde_json::from_str(include_str!("../templates/settings.json"))
                .expect("templates/settings.json must be valid");
        Self {
            module: include_str!("../templates/module.bsl.tpl").to_owned(),
            subscription_from_platform: include_str!(
                "../templates/subscription_from_platform.bsl.tpl"
            )
            .to_owned(),
            subscription_to_platform: include_str!("../templates/subscription_to_platform.bsl.tpl")
                .to_owned(),
            function: include_str!("../templates/function.bsl.tpl").to_owned(),
            region_from_platform: settings.region_from_platform,
            region_to_platform: settings.region_to_platform,
            region_functions: settings.region_functions,
        }
    }
}

impl Templates {
    pub fn save(&self, path: &Path) -> Result<(), String> {
        let text = serde_json::to_string_pretty(self).map_err(|e| e.to_string())?;
        fs::write(path, text).map_err(|e| e.to_string())
    }

    pub fn load(path: &Path) -> Result<Self, String> {
        let text = fs::read_to_string(path).map_err(|e| e.to_string())?;
        Self::from_json_text(&text)
    }

    fn from_json_text(text: &str) -> Result<Self, String> {
        let mut json: serde_json::Value = serde_json::from_str(&text).map_err(|e| e.to_string())?;
        if let Some(object) = json.as_object_mut() {
            if let Some(legacy) = object.remove("subscription") {
                object
                    .entry("subscription_from_platform")
                    .or_insert_with(|| legacy.clone());
                object.entry("subscription_to_platform").or_insert(legacy);
            }
        }
        serde_json::from_value(json).map_err(|e| e.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::Templates;

    #[test]
    fn migrates_legacy_subscription_template_to_both_directions() {
        let mut json = serde_json::to_value(Templates::default()).unwrap();
        let object = json.as_object_mut().unwrap();
        object.remove("subscription_from_platform");
        object.remove("subscription_to_platform");
        object.insert("subscription".into(), "LEGACY {{NAME}} {{CODE}}".into());

        let templates = Templates::from_json_text(&json.to_string()).unwrap();

        assert_eq!(
            templates.subscription_from_platform,
            "LEGACY {{NAME}} {{CODE}}"
        );
        assert_eq!(
            templates.subscription_to_platform,
            "LEGACY {{NAME}} {{CODE}}"
        );
    }
}
