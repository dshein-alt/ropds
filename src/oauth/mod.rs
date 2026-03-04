use serde_json::Value;
use std::str::FromStr;

/// Which OAuth provider was used.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProviderKind {
    Google,
    Yandex,
    Keycloak,
}

impl ProviderKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Google => "google",
            Self::Yandex => "yandex",
            Self::Keycloak => "keycloak",
        }
    }
}

impl FromStr for ProviderKind {
    type Err = ();

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "google" => Ok(Self::Google),
            "yandex" => Ok(Self::Yandex),
            "keycloak" => Ok(Self::Keycloak),
            _ => Err(()),
        }
    }
}

/// Normalized identity returned from any provider's userinfo endpoint.
pub struct UserInfo {
    pub provider: ProviderKind,
    pub provider_uid: String,
    pub email: Option<String>,
    pub display_name: Option<String>,
    /// Populated only for Keycloak (from realm_access.roles via client mapper).
    pub roles: Vec<String>,
}

/// Parse a provider's userinfo JSON response into a [`UserInfo`].
pub fn parse_userinfo(provider: ProviderKind, json: &Value) -> Result<UserInfo, String> {
    match provider {
        ProviderKind::Google => parse_google(json),
        ProviderKind::Yandex => parse_yandex(json),
        ProviderKind::Keycloak => parse_keycloak(json),
    }
}

fn parse_google(json: &Value) -> Result<UserInfo, String> {
    let uid = json["sub"]
        .as_str()
        .ok_or("Google userinfo missing 'sub'")?
        .to_string();
    Ok(UserInfo {
        provider: ProviderKind::Google,
        provider_uid: uid,
        email: json["email"].as_str().map(str::to_string),
        display_name: json["name"].as_str().map(str::to_string),
        roles: vec![],
    })
}

fn parse_yandex(json: &Value) -> Result<UserInfo, String> {
    let uid = json["id"]
        .as_str()
        .ok_or("Yandex userinfo missing 'id'")?
        .to_string();
    let email = json["default_email"]
        .as_str()
        .or_else(|| json["login"].as_str())
        .map(str::to_string);
    let name = json["real_name"]
        .as_str()
        .or_else(|| json["display_name"].as_str())
        .or_else(|| json["login"].as_str())
        .map(str::to_string);
    Ok(UserInfo {
        provider: ProviderKind::Yandex,
        provider_uid: uid,
        email,
        display_name: name,
        roles: vec![],
    })
}

fn parse_keycloak(json: &Value) -> Result<UserInfo, String> {
    let uid = json["sub"]
        .as_str()
        .ok_or("Keycloak userinfo missing 'sub'")?
        .to_string();
    let roles: Vec<String> = json["realm_access"]["roles"]
        .as_array()
        .map(|arr| {
            arr.iter()
                .filter_map(|v| v.as_str().map(str::to_string))
                .collect()
        })
        .unwrap_or_default();
    Ok(UserInfo {
        provider: ProviderKind::Keycloak,
        provider_uid: uid,
        email: json["email"].as_str().map(str::to_string),
        display_name: json["name"]
            .as_str()
            .or_else(|| json["preferred_username"].as_str())
            .map(str::to_string),
        roles,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_provider_kind_roundtrip() {
        assert_eq!(
            "google".parse::<ProviderKind>().ok(),
            Some(ProviderKind::Google)
        );
        assert_eq!(
            "yandex".parse::<ProviderKind>().ok(),
            Some(ProviderKind::Yandex)
        );
        assert_eq!(
            "keycloak".parse::<ProviderKind>().ok(),
            Some(ProviderKind::Keycloak)
        );
        assert_eq!("unknown".parse::<ProviderKind>().ok(), None);
    }

    #[test]
    fn test_google_parse_userinfo() {
        let json = serde_json::json!({"sub":"12345","email":"a@b.com","name":"Alice B"});
        let info = parse_userinfo(ProviderKind::Google, &json).unwrap();
        assert_eq!(info.provider_uid, "12345");
        assert_eq!(info.email.as_deref(), Some("a@b.com"));
        assert_eq!(info.display_name.as_deref(), Some("Alice B"));
    }

    #[test]
    fn test_yandex_parse_userinfo() {
        let json = serde_json::json!({
            "id": "yx-99", "default_email": "y@ya.ru", "real_name": "Yuri Y"
        });
        let info = parse_userinfo(ProviderKind::Yandex, &json).unwrap();
        assert_eq!(info.provider_uid, "yx-99");
        assert_eq!(info.email.as_deref(), Some("y@ya.ru"));
    }

    #[test]
    fn test_keycloak_parse_roles() {
        let json = serde_json::json!({
            "sub": "kc-1", "email": "k@corp.com", "name": "Karl",
            "realm_access": {"roles": ["ropds_can_upload", "offline_access"]}
        });
        let info = parse_userinfo(ProviderKind::Keycloak, &json).unwrap();
        assert!(info.roles.contains(&"ropds_can_upload".to_string()));
        assert_eq!(info.roles.len(), 2);
    }
}
