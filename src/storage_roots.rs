use std::env;
use std::ffi::OsString;
use std::fmt::{self, Display};
use std::fs;
use std::path::{Path, PathBuf};

pub const STORAGE_DATA_ROOT_ENV: &str = "RECORDIT_CONTAINER_DATA_ROOT";
pub const APP_MANAGED_STORAGE_POLICY_ENV: &str = "RECORDIT_ENFORCE_APP_MANAGED_STORAGE_POLICY";
pub const TRANSCRIBE_APP_CONTAINER_ID: &str = "com.recordit.sequoiatranscribe";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CanonicalStorageRoots {
    pub data_root: PathBuf,
    pub sessions_root: PathBuf,
    pub models_root: PathBuf,
    pub logs_root: PathBuf,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ManagedStorageDomain {
    Sessions,
    Models,
    Logs,
}

impl ManagedStorageDomain {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Sessions => "sessions",
            Self::Models => "models",
            Self::Logs => "logs",
        }
    }

    fn allowed_root(self, roots: &CanonicalStorageRoots) -> &Path {
        match self {
            Self::Sessions => roots.sessions_root.as_path(),
            Self::Models => roots.models_root.as_path(),
            Self::Logs => roots.logs_root.as_path(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StorageRootError {
    MissingHomeEnv,
    InvalidDataRootOverride(String),
    NonAbsoluteDataRoot(PathBuf),
    CurrentDirectoryUnavailable(String),
    InvalidPolicyPath(PathBuf),
    CanonicalizationFailure {
        path: PathBuf,
        detail: String,
    },
    OutsideAllowedRoot {
        candidate: PathBuf,
        allowed_root: PathBuf,
        domain: ManagedStorageDomain,
    },
}

impl Display for StorageRootError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::MissingHomeEnv => write!(
                f,
                "unable to resolve canonical storage roots: HOME is not set and no override was provided"
            ),
            Self::InvalidDataRootOverride(raw) => write!(
                f,
                "invalid `{STORAGE_DATA_ROOT_ENV}` override `{raw}`: expected a non-empty absolute path"
            ),
            Self::NonAbsoluteDataRoot(path) => write!(
                f,
                "canonical storage data root must be absolute, got `{}`",
                path.display()
            ),
            Self::CurrentDirectoryUnavailable(detail) => {
                write!(f, "failed to resolve current working directory: {detail}")
            }
            Self::InvalidPolicyPath(path) => write!(
                f,
                "cannot normalize policy path `{}` because no existing ancestor could be resolved",
                path.display()
            ),
            Self::CanonicalizationFailure { path, detail } => {
                write!(f, "failed to canonicalize `{}`: {detail}", path.display())
            }
            Self::OutsideAllowedRoot {
                candidate,
                allowed_root,
                domain,
            } => write!(
                f,
                "app-managed write `{}` is outside canonical {} root `{}`",
                candidate.display(),
                domain.as_str(),
                allowed_root.display()
            ),
        }
    }
}

impl std::error::Error for StorageRootError {}

pub fn app_managed_storage_policy_enabled() -> bool {
    match env::var(APP_MANAGED_STORAGE_POLICY_ENV) {
        Ok(value) => matches!(
            value.trim().to_ascii_lowercase().as_str(),
            "1" | "true" | "yes" | "on"
        ),
        Err(_) => false,
    }
}

pub fn resolve_canonical_storage_roots() -> Result<CanonicalStorageRoots, StorageRootError> {
    let data_root = match env::var(STORAGE_DATA_ROOT_ENV) {
        Ok(raw) => {
            let trimmed = raw.trim();
            if trimmed.is_empty() {
                return Err(StorageRootError::InvalidDataRootOverride(raw));
            }
            PathBuf::from(trimmed)
        }
        Err(_) => {
            let home = env::var("HOME").map_err(|_| StorageRootError::MissingHomeEnv)?;
            PathBuf::from(home)
                .join("Library")
                .join("Containers")
                .join(TRANSCRIBE_APP_CONTAINER_ID)
                .join("Data")
        }
    };

    if !data_root.is_absolute() {
        return Err(StorageRootError::NonAbsoluteDataRoot(data_root));
    }

    let packaged_root = data_root.join("artifacts").join("packaged-beta");
    Ok(CanonicalStorageRoots {
        sessions_root: packaged_root.join("sessions"),
        models_root: packaged_root.join("models"),
        logs_root: packaged_root.join("logs"),
        data_root,
    })
}

pub fn validate_app_managed_write_path(
    candidate: &Path,
    domain: ManagedStorageDomain,
    roots: &CanonicalStorageRoots,
) -> Result<PathBuf, StorageRootError> {
    let normalized_candidate = normalize_for_policy(candidate)?;
    let normalized_root = normalize_for_policy(domain.allowed_root(roots))?;
    if normalized_candidate.starts_with(&normalized_root) {
        return Ok(normalized_candidate);
    }

    Err(StorageRootError::OutsideAllowedRoot {
        candidate: normalized_candidate,
        allowed_root: normalized_root,
        domain,
    })
}

fn normalize_for_policy(path: &Path) -> Result<PathBuf, StorageRootError> {
    let absolute = absolutize(path)?;
    if absolute.exists() {
        return fs::canonicalize(&absolute).map_err(|err| {
            StorageRootError::CanonicalizationFailure {
                path: absolute.clone(),
                detail: err.to_string(),
            }
        });
    }

    let mut unresolved_tail: Vec<OsString> = Vec::new();
    let mut cursor = absolute.clone();
    while !cursor.exists() {
        let segment = cursor
            .file_name()
            .ok_or_else(|| StorageRootError::InvalidPolicyPath(absolute.clone()))?;
        unresolved_tail.push(segment.to_os_string());
        cursor = cursor
            .parent()
            .ok_or_else(|| StorageRootError::InvalidPolicyPath(absolute.clone()))?
            .to_path_buf();
    }

    let mut normalized =
        fs::canonicalize(&cursor).map_err(|err| StorageRootError::CanonicalizationFailure {
            path: cursor.clone(),
            detail: err.to_string(),
        })?;
    while let Some(segment) = unresolved_tail.pop() {
        normalized.push(segment);
    }
    Ok(normalized)
}

fn absolutize(path: &Path) -> Result<PathBuf, StorageRootError> {
    if path.is_absolute() {
        return Ok(path.to_path_buf());
    }
    env::current_dir()
        .map(|cwd| cwd.join(path))
        .map_err(|err| StorageRootError::CurrentDirectoryUnavailable(err.to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{Mutex, OnceLock};
    use std::time::{SystemTime, UNIX_EPOCH};

    #[test]
    fn resolves_default_container_roots_from_home() {
        let _guard = env_lock().lock().unwrap();
        let original_override = env::var(STORAGE_DATA_ROOT_ENV).ok();
        unsafe {
            env::remove_var(STORAGE_DATA_ROOT_ENV);
        }

        let roots = resolve_canonical_storage_roots().expect("expected default root resolution");
        assert!(roots.data_root.is_absolute());
        let packaged_root = roots.data_root.join("artifacts").join("packaged-beta");
        assert_eq!(roots.sessions_root, packaged_root.join("sessions"));
        assert_eq!(roots.models_root, packaged_root.join("models"));
        assert_eq!(roots.logs_root, packaged_root.join("logs"));

        restore_optional_env(STORAGE_DATA_ROOT_ENV, original_override);
    }

    #[test]
    fn validates_candidate_within_domain_root() {
        let _guard = env_lock().lock().unwrap();
        let temp = unique_temp_root("storage-roots-allow");
        let roots = CanonicalStorageRoots {
            sessions_root: temp.join("sessions"),
            models_root: temp.join("models"),
            logs_root: temp.join("logs"),
            data_root: temp.clone(),
        };

        let candidate = roots.sessions_root.join("20260305").join("session.wav");
        let normalized =
            validate_app_managed_write_path(&candidate, ManagedStorageDomain::Sessions, &roots)
                .expect("path should be accepted");
        assert!(normalized.ends_with("session.wav"));
    }

    #[test]
    fn rejects_candidate_outside_domain_root() {
        let _guard = env_lock().lock().unwrap();
        let temp = unique_temp_root("storage-roots-deny");
        let roots = CanonicalStorageRoots {
            sessions_root: temp.join("sessions"),
            models_root: temp.join("models"),
            logs_root: temp.join("logs"),
            data_root: temp.clone(),
        };

        let candidate = temp.join("misc").join("session.wav");
        let err =
            validate_app_managed_write_path(&candidate, ManagedStorageDomain::Sessions, &roots)
                .expect_err("path outside sessions root should be rejected");
        let message = err.to_string();
        assert!(message.contains("outside canonical sessions root"));
    }

    #[test]
    fn normalizes_dotdot_and_rejects_escape_attempts() {
        let _guard = env_lock().lock().unwrap();
        let temp = unique_temp_root("storage-roots-dotdot");
        let roots = CanonicalStorageRoots {
            sessions_root: temp.join("sessions"),
            models_root: temp.join("models"),
            logs_root: temp.join("logs"),
            data_root: temp.clone(),
        };
        fs::create_dir_all(roots.sessions_root.join("safe")).unwrap();
        fs::create_dir_all(temp.join("outside")).unwrap();

        let escaping = roots
            .sessions_root
            .join("safe")
            .join("..")
            .join("..")
            .join("outside")
            .join("leak.txt");
        let err =
            validate_app_managed_write_path(&escaping, ManagedStorageDomain::Sessions, &roots)
                .expect_err("dotdot escape should be rejected");
        assert!(err.to_string().contains("outside canonical sessions root"));
    }

    fn unique_temp_root(prefix: &str) -> PathBuf {
        let stamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let root = env::temp_dir().join(format!("{prefix}-{stamp}"));
        fs::create_dir_all(&root).unwrap();
        root
    }

    fn restore_optional_env(name: &str, value: Option<String>) {
        match value {
            Some(value) => unsafe {
                env::set_var(name, value);
            },
            None => unsafe {
                env::remove_var(name);
            },
        }
    }

    fn env_lock() -> &'static Mutex<()> {
        static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
        LOCK.get_or_init(|| Mutex::new(()))
    }
}
