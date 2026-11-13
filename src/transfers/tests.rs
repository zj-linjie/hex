use super::*;

struct Fixture {
    base: PathBuf,
    source: PathBuf,
    store: PathBuf,
}
impl Fixture {
    fn new(status: Status) -> Result<Self> {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let base = std::env::temp_dir().join(format!(
            "hex-transfers-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        let source = base.join("custom-source");
        fs::create_dir_all(&source)?;
        fs::write(source.join("body.txt"), b"hello")?;
        let manifest = Manifest {
            schema_version: 1,
            id: "42d75e12-88ed-41ee-94c8-40484b860045".into(),
            status,
            created_at: "2026-11-13T00:00:00Z".into(),
            title: "A transfer".into(),
            note: "A note".into(),
            attachments: vec![Attachment {
                name: "body.txt".into(),
                uti: "public.utf8-plain-text".into(),
                bytes: 5,
                kind: Kind::Text,
            }],
        };
        fs::write(source.join("manifest.json"), serde_json::to_vec(&manifest)?)?;
        Ok(Self {
            store: base.join("store"),
            base,
            source,
        })
    }
    fn change(&self, edit: impl FnOnce(&mut serde_json::Value)) -> Result<()> {
        let path = self.source.join("manifest.json");
        let mut value = serde_json::from_slice(&fs::read(&path)?)?;
        edit(&mut value);
        fs::write(path, serde_json::to_vec(&value)?)?;
        Ok(())
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.base);
    }
}

#[test]
fn held_is_not_copied() -> Result<()> {
    let f = Fixture::new(Status::Held)?;
    let reply = import_into(&f.source, &f.store)?;
    assert!(!reply.imported);
    assert_eq!(reply.reason.as_deref(), Some("held"));
    assert!(!f.store.exists());
    Ok(())
}
#[test]
fn queued_copies_once_and_does_not_write_a_receipt() -> Result<()> {
    let f = Fixture::new(Status::Queued)?;
    let first = import_into(&f.source, &f.store)?;
    assert!(first.imported);
    assert_eq!(
        fs::read(f.store.join(&first.id).join("body.txt"))?,
        b"hello"
    );
    assert!(!f.source.join("receipt.json").exists());
    assert!(!f.store.join(&first.id).join("receipt.json").exists());
    let second = import_into(&f.source, &f.store)?;
    assert_eq!(second.reason.as_deref(), Some("already"));
    assert!(!second.imported);
    assert_eq!(fs::read_dir(&f.store)?.count(), 1);
    Ok(())
}
#[test]
fn invalid_schema_missing_file_and_size_mismatch_copy_nothing() -> Result<()> {
    for failure in ["version", "missing-version", "file", "size", "traversal"] {
        let f = Fixture::new(Status::Queued)?;
        match failure {
            "version" => f.change(|m| m["schema_version"] = 2.into())?,
            "missing-version" => f.change(|m| {
                if let Some(o) = m.as_object_mut() {
                    o.remove("schema_version");
                }
            })?,
            "file" => fs::remove_file(f.source.join("body.txt"))?,
            "size" => fs::write(f.source.join("body.txt"), b"shortened")?,
            _ => f.change(|m| m["attachments"][0]["name"] = "../body.txt".into())?,
        }
        assert!(import_into(&f.source, &f.store).is_err(), "{failure}");
        assert!(!f.store.exists(), "{failure}");
    }
    Ok(())
}
#[test]
fn deleting_removes_only_the_mac_copy() -> Result<()> {
    let f = Fixture::new(Status::Queued)?;
    let reply = import_into(&f.source, &f.store)?;
    delete_in(&f.store, &reply.id)?;
    assert!(list_in(&f.store)?.is_empty());
    assert_eq!(fs::read(f.source.join("body.txt"))?, b"hello");
    assert!(f.source.join("manifest.json").exists());
    Ok(())
}
#[cfg(unix)]
#[test]
fn every_local_file_and_directory_is_owner_only() -> Result<()> {
    use std::os::unix::fs::PermissionsExt;
    let f = Fixture::new(Status::Queued)?;
    let reply = import_into(&f.source, &f.store)?;
    let directory = f.store.join(reply.id);
    for path in [&f.store, &directory] {
        assert_eq!(fs::metadata(path)?.permissions().mode() & 0o777, 0o700);
    }
    for path in [directory.join("body.txt"), directory.join("manifest.json")] {
        assert_eq!(fs::metadata(path)?.permissions().mode() & 0o777, 0o600);
    }
    Ok(())
}
#[cfg(unix)]
#[test]
fn symlink_attachments_are_refused() -> Result<()> {
    let f = Fixture::new(Status::Queued)?;
    fs::remove_file(f.source.join("body.txt"))?;
    std::os::unix::fs::symlink("manifest.json", f.source.join("body.txt"))?;
    assert!(import_into(&f.source, &f.store).is_err());
    assert!(!f.store.exists());
    Ok(())
}

#[test]
fn transfers_lab_contract() {
    let feature = crate::labs::Feature::Transfers;
    let mut settings = crate::app_settings::AppSettings::default();
    assert!(!feature.on(&settings));
    assert_eq!(feature.name(), "transfers");
    assert_eq!(feature.settings_key(), "labs.transfers");
    assert_eq!(feature.group(), crate::labs::Group::History);
    assert_eq!(feature.audience(), crate::labs::Audience::Everyone);
    assert_eq!(feature.on_this_os(), cfg!(target_os = "macos"));
    feature.set(&mut settings, true);
    assert!(feature.on(&settings));
}

#[test]
fn machine_import_reply_has_only_the_contract_fields() -> Result<()> {
    let id = "42d75e12-88ed-41ee-94c8-40484b860045";
    let reply = ImportReply {
        schema_version: 1,
        id: id.into(),
        imported: true,
        reason: None,
    };
    assert_eq!(
        serde_json::to_value(reply)?,
        serde_json::json!({"schema_version":1,"id":id,"imported":true})
    );
    Ok(())
}

#[test]
fn a_missing_second_attachment_never_copies_the_first() -> Result<()> {
    let f = Fixture::new(Status::Queued)?;
    f.change(|m| {
        if let Some(attachments) = m["attachments"].as_array_mut() {
            attachments.push(serde_json::json!({"name":"missing.pdf","uti":"com.adobe.pdf","bytes":12,"kind":"file"}));
        }
    })?;
    assert!(import_into(&f.source, &f.store).is_err());
    assert!(!f.store.exists());
    Ok(())
}

#[test]
fn reply_preserves_manifest_uuid_spelling_and_store_deduplicates_it() -> Result<()> {
    let f = Fixture::new(Status::Queued)?;
    let id = "42D75E12-88ED-41EE-94C8-40484B860045";
    f.change(|m| m["id"] = id.into())?;
    let first = import_into(&f.source, &f.store)?;
    assert_eq!(first.id, id);
    assert!(first.imported);
    f.change(|m| m["id"] = id.to_ascii_lowercase().into())?;
    assert_eq!(
        import_into(&f.source, &f.store)?.reason.as_deref(),
        Some("already")
    );
    assert_eq!(list_in(&f.store)?.len(), 1);
    Ok(())
}
