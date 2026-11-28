//! Bootstrap an exclusively owned profile before the Darwin helper serves requests.
use dzc_core::{
    api::HistoryService,
    crypto::{install_key_provider, EncryptionError, KeyProvider},
};
use dzc_platform::{
    darwin_keychain::DarwinKeyProvider,
    ipc::{
        canonical_private_directory, lock_private_profile, read_private, write_private, Endpoint,
        Listener,
    },
};
use std::{
    fs::{self, File},
    io,
    os::unix::fs::{MetadataExt, OpenOptionsExt},
    path::{Path, PathBuf},
};
use uuid::Uuid;

const IDENTITY: &str = "profile-id";
const DATABASE: &str = "history.sqlite3";
const SQLITE_FILES: &[&str] = &[
    DATABASE,
    "history.sqlite3-wal",
    "history.sqlite3-shm",
    "history.sqlite3-journal",
];

pub struct DarwinProfile {
    pub directory: PathBuf,
    pub endpoint: Endpoint,
    pub listener: Listener,
    pub history: HistoryService,
    pub id: Uuid,
    _ownership: File,
}

impl DarwinProfile {
    /// Call from the helper's runtime. The exclusive endpoint is bound before
    /// opening the database, and accepts only the helper's actual launcher.
    /// The helper retains the profile owner's handle until request draining ends.
    pub fn open(
        directory: &Path,
        key_provider: impl FnOnce(Uuid) -> Result<DarwinKeyProvider, EncryptionError>,
    ) -> io::Result<Self> {
        let directory = canonical_private_directory(directory)?;
        let ownership = lock_private_profile(&directory)?;
        let owner = ownership.metadata()?.uid();
        let endpoint = Endpoint::for_current_session(&directory)?;
        let listener = endpoint.bind_for_parent()?;
        let database = directory.join(DATABASE);
        if database.to_str().is_none() {
            return Err(io::Error::from(io::ErrorKind::InvalidInput));
        }
        let has_database = is_private_file(&database, owner)?;
        for sidecar in &SQLITE_FILES[1..] {
            if is_private_file(&directory.join(sidecar), owner)? && !has_database {
                return Err(invalid_profile());
            }
        }

        let identity = directory.join(IDENTITY);
        let (id, proof) = match read_private(&identity) {
            Ok(bytes) => {
                if !is_private_file(&identity, owner)? {
                    return Err(invalid_profile());
                }
                parse_identity(&bytes)?
            }
            Err(error) if error.kind() == io::ErrorKind::NotFound => {
                if !is_empty_profile(&directory, owner, false)? {
                    return Err(invalid_profile());
                }
                let id = Uuid::new_v4();
                write_identity(&directory, id, None)?;
                (id, None)
            }
            Err(error) => return Err(error),
        };

        let provider = key_provider(id).map_err(io::Error::other)?;
        if proof.is_some() || has_database {
            provider.verify_existing().map_err(io::Error::other)?;
            if !has_database {
                return Err(invalid_profile());
            }
        } else {
            if !is_empty_profile(&directory, owner, true)? {
                return Err(invalid_profile());
            }
            // A retry after interruption uses the same account. Provisioning
            // leaves any key already added for that account intact.
            provider.provision_new_profile().map_err(io::Error::other)?;
            File::options()
                .write(true)
                .create_new(true)
                .mode(0o600)
                .open(&database)?
                .sync_all()?;
        }

        if let Some(proof) = &proof {
            let expected = database_proof(&database)?;
            if provider.open_text(proof).as_deref() != Some(expected.as_str()) {
                return Err(io::Error::other(EncryptionError::KeyUnavailable));
            }
        } else {
            // Commit the initial schema under Core's standalone policy before
            // claiming a durable database identity, then use desktop WAL policy.
            drop(HistoryService::open(&database).map_err(io::Error::other)?);
            let proof = provider
                .seal_text(&database_proof(&database)?)
                .map_err(io::Error::other)?;
            write_identity(&directory, id, Some(&proof))?;
        }
        install_key_provider(Box::leak(Box::new(provider))).map_err(io::Error::other)?;
        let history = HistoryService::open_desktop(&database).map_err(io::Error::other)?;
        Ok(Self {
            directory,
            endpoint,
            listener,
            history,
            id,
            _ownership: ownership,
        })
    }
}

fn is_private_file(path: &Path, owner: u32) -> io::Result<bool> {
    match fs::symlink_metadata(path) {
        Ok(metadata)
            if metadata.is_file()
                && metadata.uid() == owner
                && metadata.nlink() == 1
                && metadata.mode() & 0o077 == 0 =>
        {
            Ok(true)
        }
        Ok(_) => Err(invalid_profile()),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(false),
        Err(error) => Err(error),
    }
}

fn is_empty_profile(directory: &Path, owner: u32, identity_present: bool) -> io::Result<bool> {
    for item in fs::read_dir(directory)? {
        let item = item?;
        let name = item.file_name();
        if name == "profile.lock" || identity_present && name == IDENTITY {
            continue;
        }
        let temporary_identity = name
            .to_str()
            .and_then(|name| name.strip_prefix("profile-id.")?.strip_suffix(".new"))
            .is_some_and(|value| Uuid::try_parse(value).is_ok());
        if !temporary_identity || !is_private_file(&item.path(), owner)? {
            return Ok(false);
        }
    }
    Ok(true)
}

fn parse_identity(bytes: &[u8]) -> io::Result<(Uuid, Option<String>)> {
    let text = std::str::from_utf8(bytes).map_err(|_| invalid_profile())?;
    let mut fields = text.split(' ');
    let (Some("v1"), Some(phase), Some(account)) = (fields.next(), fields.next(), fields.next())
    else {
        return Err(invalid_profile());
    };
    let Some(id) = Uuid::try_parse(account)
        .ok()
        .filter(|value| !value.is_nil())
    else {
        return Err(invalid_profile());
    };
    let proof = match (phase, fields.next(), fields.next()) {
        ("creating", None, None) => None,
        ("ready", Some(proof), None) if !proof.is_empty() => Some(proof.to_owned()),
        _ => return Err(invalid_profile()),
    };
    Ok((id, proof))
}

fn write_identity(directory: &Path, id: Uuid, proof: Option<&str>) -> io::Result<()> {
    let identity = match proof {
        Some(proof) => format!("v1 ready {id} {proof}"),
        None => format!("v1 creating {id}"),
    };
    write_private(&directory.join(IDENTITY), identity.as_bytes())?;
    File::open(directory)?.sync_all()
}

fn database_proof(database: &Path) -> io::Result<String> {
    let id = HistoryService::read_profile_database_id(database).map_err(io::Error::other)?;
    Ok(format!("dezirclip-history:v1:{id}"))
}

fn invalid_profile() -> io::Error {
    io::Error::new(
        io::ErrorKind::InvalidData,
        "invalid or incomplete Darwin profile",
    )
}
