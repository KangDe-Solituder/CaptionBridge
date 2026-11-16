use crate::models::{AppSettings, TranslationMode, TranslationProfiles};

pub fn normalize(settings: &mut AppSettings) {
    let first = settings.llm_profiles.first().map(|p| p.id.clone());
    let legacy = settings
        .llm
        .profile_id
        .clone()
        .filter(|id| settings.llm_profiles.iter().any(|p| &p.id == id))
        .or(first.clone());
    let routes = settings
        .translation_profiles
        .get_or_insert_with(|| TranslationProfiles {
            selection: legacy.clone(),
            live_caption: legacy.clone(),
            screenshot: legacy,
        });
    for id in [
        &mut routes.selection,
        &mut routes.live_caption,
        &mut routes.screenshot,
    ] {
        if !id
            .as_ref()
            .is_some_and(|id| settings.llm_profiles.iter().any(|p| &p.id == id))
        {
            *id = first.clone();
        }
    }
}

pub fn resolve(settings: &AppSettings, mode: &TranslationMode) -> Result<AppSettings, String> {
    let mut snapshot = settings.clone();
    // Legacy snapshots may predate per-feature routes; never resolve a stale explicit ID silently.
    if snapshot.translation_profiles.is_none() {
        normalize(&mut snapshot);
    }
    let routes = snapshot.translation_profiles.as_ref().unwrap();
    let id = match mode {
        TranslationMode::Selection => &routes.selection,
        TranslationMode::LiveCaption => &routes.live_caption,
        TranslationMode::Screenshot => &routes.screenshot,
    };
    let profile = snapshot
        .llm_profiles
        .iter()
        .find(|p| Some(&p.id) == id.as_ref())
        .ok_or("此功能的翻译配置不可用，请在设置 → 功能与入口中选择翻译模型")?;
    snapshot.llm = profile.config.clone();
    snapshot.llm.profile_id = Some(profile.id.clone());
    Ok(snapshot)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::{LlmProfile, LlmProviderSettings};
    fn settings() -> AppSettings {
        let mut s = AppSettings::default();
        for id in ["first", "second", "unused"] {
            s.llm_profiles.push(LlmProfile {
                id: id.into(),
                name: id.into(),
                api_key_configured: false,
                config: LlmProviderSettings {
                    profile_id: Some(id.into()),
                    model: id.into(),
                    ..Default::default()
                },
            });
        }
        s.llm = s.llm_profiles[1].config.clone();
        s
    }
    #[test]
    fn migration_preserves_active_and_routes_resolve_independently() {
        let mut value = serde_json::to_value(settings()).unwrap();
        value
            .as_object_mut()
            .unwrap()
            .remove("translation_profiles");
        let mut s: AppSettings = serde_json::from_value(value).unwrap();
        normalize(&mut s);
        assert_eq!(
            resolve(&s, &TranslationMode::Selection).unwrap().llm.model,
            "second"
        );
        s.translation_profiles.as_mut().unwrap().screenshot = Some("first".into());
        assert_eq!(
            resolve(&s, &TranslationMode::Screenshot).unwrap().llm.model,
            "first"
        );
        assert_eq!(
            resolve(&s, &TranslationMode::LiveCaption)
                .unwrap()
                .llm
                .model,
            "second"
        );
        let restored: AppSettings =
            serde_json::from_str(&serde_json::to_string(&s).unwrap()).unwrap();
        assert_eq!(restored.translation_profiles, s.translation_profiles);
    }
    #[test]
    fn deletion_only_repairs_affected_routes_and_empty_stops_translation() {
        let mut s = settings();
        normalize(&mut s);
        let before = s.translation_profiles.clone();
        s.llm_profiles.retain(|p| p.id != "unused");
        normalize(&mut s);
        assert_eq!(s.translation_profiles, before);
        s.llm_profiles.retain(|p| p.id != "second");
        assert!(resolve(&s, &TranslationMode::LiveCaption).is_err());
        normalize(&mut s);
        assert_eq!(
            resolve(&s, &TranslationMode::LiveCaption)
                .unwrap()
                .llm
                .model,
            "first"
        );
        s.llm_profiles.clear();
        normalize(&mut s);
        assert!(resolve(&s, &TranslationMode::Selection).is_err());
        assert!(s.translation_profiles.unwrap().selection.is_none());
    }
}
