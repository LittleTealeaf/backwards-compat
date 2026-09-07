use backwards_compat::backwards_compat;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct UserV1 {
    pub name: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct UserV2 {
    pub name: String,
    pub email: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct UserV3 {
    pub name: String,
    pub email: String,
    pub is_admin: bool,
}

impl From<UserV1> for UserV2 {
    fn from(v1: UserV1) -> Self {
        Self {
            name: v1.name,
            email: "unknown@example.com".to_string(),
        }
    }
}

impl From<UserV2> for UserV3 {
    fn from(v2: UserV2) -> Self {
        Self {
            name: v2.name,
            email: v2.email,
            is_admin: false,
        }
    }
}

backwards_compat! {
    #[tag = "schema_version"]
    pub enum UserVersion {
        v1 = UserV1,
        v2 = UserV2,
        v3 = UserV3,
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(from = "UserVersion", into = "UserVersion")]
pub struct User {
    pub name: String,
    pub email: String,
    pub is_admin: bool,
}

impl From<UserVersion> for User {
    fn from(v: UserVersion) -> Self {
        let v3: UserV3 = v.into();
        Self {
            name: v3.name,
            email: v3.email,
            is_admin: v3.is_admin,
        }
    }
}

impl From<User> for UserVersion {
    fn from(u: User) -> Self {
        Self::v3(UserV3 {
            name: u.name,
            email: u.email,
            is_admin: u.is_admin,
        })
    }
}

#[test]
fn test_json_upgrade_from_v1() {
    let raw = r#"{"schema_version": "1", "name": "Alice"}"#;
    let user: User = serde_json::from_str(raw).unwrap();
    assert_eq!(
        user,
        User {
            name: "Alice".to_string(),
            email: "unknown@example.com".to_string(),
            is_admin: false,
        }
    );
}

#[test]
fn test_json_upgrade_from_v2() {
    let raw = r#"{"schema_version": "2", "name": "Bob", "email": "bob@example.com"}"#;
    let user: User = serde_json::from_str(raw).unwrap();
    assert_eq!(
        user,
        User {
            name: "Bob".to_string(),
            email: "bob@example.com".to_string(),
            is_admin: false,
        }
    );
}

#[test]
fn test_json_upgrade_from_v3() {
    let raw = r#"{"schema_version": "3", "name": "Charlie", "email": "charlie@example.com", "is_admin": true}"#;
    let user: User = serde_json::from_str(raw).unwrap();
    assert_eq!(
        user,
        User {
            name: "Charlie".to_string(),
            email: "charlie@example.com".to_string(),
            is_admin: true,
        }
    );
}

#[test]
fn test_serialization_uses_latest_version() {
    let user = User {
        name: "Dana".to_string(),
        email: "dana@example.com".to_string(),
        is_admin: true,
    };
    let json = serde_json::to_string(&user).unwrap();
    assert!(json.contains(r#""schema_version":"3""#));

    let roundtripped: User = serde_json::from_str(&json).unwrap();
    assert_eq!(user, roundtripped);
}

#[test]
fn test_ron_support() {
    let user = User {
        name: "Eve".to_string(),
        email: "eve@example.com".to_string(),
        is_admin: false,
    };
    let ron_str = ron::to_string(&user).unwrap();
    assert!(ron_str.contains("schema_version") && ron_str.contains(r#""3""#));

    let roundtripped: User = ron::from_str(&ron_str).unwrap();
    assert_eq!(user, roundtripped);
}

#[test]
fn test_toml_support() {
    let user = User {
        name: "Frank".to_string(),
        email: "frank@example.com".to_string(),
        is_admin: true,
    };
    let toml_str = toml::to_string(&user).unwrap();
    assert!(toml_str.contains(r#"schema_version = "3""#));

    let roundtripped: User = toml::from_str(&toml_str).unwrap();
    assert_eq!(user, roundtripped);
}
