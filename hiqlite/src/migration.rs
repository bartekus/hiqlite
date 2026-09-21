use rust_embed::RustEmbed;
use serde::{Deserialize, Serialize};
use std::borrow::Cow;

pub struct Migrations;

impl Migrations {
    pub fn build<T: RustEmbed>() -> Vec<Migration> {
        let mut files = T::iter()
            .map(|name| {
                let (id, _) = name
                    .split_once('_')
                    .expect("Migration file names must start with `<integer>_<migration_name>");
                let id = id.parse::<u32>().expect(
                    "Migration scripts must start with an increasing integer with \
                    no gaps and starting at index 1",
                );
                (id, name)
            })
            .collect::<Vec<(u32, Cow<'static, str>)>>();

        if files.is_empty() {
            return Vec::default();
        }

        files.sort_by(|(a, _), (b, _)| a.partial_cmp(b).unwrap());
        if let Some((first_id, _)) = files.first()
            && *first_id != 1
        {
            panic!("Migrations must start at index 1");
        }

        let mut res: Vec<Migration> = Vec::with_capacity(files.len());

        for (id, file_name) in files {
            let data = T::get(file_name.as_ref()).unwrap();
            let hash = hex::encode(data.metadata.sha256_hash());
            let content = data.data.to_vec();

            let stripped = file_name
                .strip_suffix(".sql")
                .expect("Migration scripts must always end with .sql");
            let (_, name) = stripped.split_once('_').unwrap();

            let migration = Migration {
                id,
                name: name.to_string(),
                hash,
                content,
            };

            let len = res.len();
            if len > 0 && migration.id != (res[len - 1].id + 1) {
                panic!(
                    "Migration index has a gap: {} does not follow {}",
                    migration.id,
                    res[len - 1].id
                );
            }

            res.push(migration);
        }

        res
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Migration {
    pub id: u32,
    pub name: String,
    /// sha256 hash as hex
    pub hash: String,
    pub content: Vec<u8>,
}

/// Applied migrations to the database.
///
/// Can be retrieved with `.query_map("SELECT * FROM _migrations", params!())`
#[derive(Debug, Clone)]
pub struct AppliedMigration {
    pub id: u32,
    pub name: String,
    pub ts: i64,
    /// sha256 hash as hex
    pub hash: String,
}

impl From<&mut crate::Row<'_>> for AppliedMigration {
    fn from(row: &mut crate::Row<'_>) -> Self {
        Self {
            id: row.get::<i64>("id") as u32,
            name: row.get("name"),
            ts: row.get("ts"),
            hash: row.get("hash"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rust_embed::Embed;

    #[derive(Embed)]
    #[folder = "tests/cluster/migrations/good"]
    struct Good;

    /// Names are valid; the failure this fixture exists for is a SQL syntax
    /// error, which happens when the migration is applied and not when it is
    /// built.
    #[derive(Embed)]
    #[folder = "tests/cluster/migrations/bad_3"]
    struct Bad3;

    /// The repository's fixture for "no leading integer index".
    #[derive(Embed)]
    #[folder = "tests/cluster/migrations/bad_1"]
    struct Bad1;

    /// The repository's fixture for "index does not start at 1".
    #[derive(Embed)]
    #[folder = "tests/cluster/migrations/bad_2"]
    struct Bad2;

    #[test]
    fn a_valid_set_is_ordered_by_index_and_hashed_by_content() {
        let migrations = Migrations::build::<Good>();
        assert_eq!(migrations.len(), 3);

        assert_eq!(migrations[0].id, 1);
        assert_eq!(migrations[0].name, "init");
        // the same hash the cluster suite asserts against the `_migrations`
        // table, pinned here at the layer that computes it
        assert_eq!(
            migrations[0].hash,
            "46a52cfa9b2532439423fe769a3a75aa17e8690ee98b2c1b7c5c21560702e2aa"
        );

        assert_eq!(migrations[1].id, 2);
        assert_eq!(migrations[1].name, "another_migration");
        assert_eq!(
            migrations[1].hash,
            "c61c731c49a33a44ad56112365423f8d654e7ddbe9320f2492746aa61f54a733"
        );

        assert_eq!(migrations[2].id, 3);
        assert_eq!(migrations[2].name, "types_conversion");

        // the name is everything after the first underscore, suffix stripped
        assert!(!migrations[2].name.ends_with(".sql"));
        assert!(!migrations[0].content.is_empty());
    }

    /// `build` validates names and indices only. A migration whose SQL is
    /// invalid is built without complaint and fails later, when applied.
    #[test]
    fn a_syntactically_invalid_migration_still_builds() {
        let migrations = Migrations::build::<Bad3>();
        assert_eq!(migrations.len(), 1);
        assert_eq!(migrations[0].id, 1);
        assert_eq!(migrations[0].name, "filename_ok");
    }

    /// F-064: the fixture is named for a missing leading index, and the panic
    /// that fires is the one about increasing integers with no gaps. The
    /// `split_once` message about `<integer>_<migration_name>` is reachable
    /// only for a file name with no underscore at all, which no fixture has.
    #[test]
    #[should_panic(expected = "Migration scripts must start with an increasing integer")]
    fn a_name_without_a_numeric_index_panics_with_the_other_rules_message() {
        let _ = Migrations::build::<Bad1>();
    }

    #[test]
    #[should_panic(expected = "Migrations must start at index 1")]
    fn an_index_set_that_does_not_start_at_one_panics() {
        let _ = Migrations::build::<Bad2>();
    }
}
