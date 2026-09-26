use rust_embed::RustEmbed;
use serde::{Deserialize, Serialize};

pub struct Migrations;

impl Migrations {
    /// Build the migration set, or say what is wrong with it.
    ///
    /// Five rules, each with its own named error. They used to be five `expect`s and two
    /// `panic!`s inside a function returning `Vec<Migration>`, which the caller in
    /// `client/migrate.rs` then invoked from a function that returns `Result`: a bad migration
    /// set ended the process under an aborting profile instead of failing the deployment
    /// (F-066). Two of the messages were also wrong about which rule had been broken:
    ///
    /// - a name with no `_` reported the **gap** rule rather than the missing separator,
    ///   because the `split_once` `expect` fired first and the id parse's message is the one a
    ///   reader sees for `create_users.sql` (F-064);
    /// - a **duplicate** index reported "Migration index has a gap: 1 does not follow 1",
    ///   which is neither true nor actionable (F-065).
    pub fn try_build<T: RustEmbed>() -> Result<Vec<Migration>, crate::Error> {
        let invalid = |msg: String| crate::Error::Config(msg.into());

        let mut files = Vec::new();
        for name in T::iter() {
            let Some((id, _)) = name.split_once('_') else {
                return Err(invalid(format!(
                    "migration file `{name}` must be named `<integer>_<migration_name>.sql`; it \
                     has no `_` separating the index from the name"
                )));
            };
            let id = id.parse::<u32>().map_err(|err| {
                invalid(format!(
                    "migration file `{name}` must start with an integer index: `{id}` does not \
                     parse ({err})"
                ))
            })?;
            files.push((id, name));
        }

        if files.is_empty() {
            return Ok(Vec::default());
        }

        files.sort_by_key(|(id, _)| *id);
        if let Some((first_id, first_name)) = files.first()
            && *first_id != 1
        {
            return Err(invalid(format!(
                "migrations must start at index 1; the lowest is {first_id} (`{first_name}`)"
            )));
        }

        let mut res: Vec<Migration> = Vec::with_capacity(files.len());

        for (id, file_name) in files {
            let Some(data) = T::get(file_name.as_ref()) else {
                return Err(invalid(format!(
                    "migration file `{file_name}` disappeared between listing and reading"
                )));
            };
            let hash = hex::encode(data.metadata.sha256_hash());
            let content = data.data.to_vec();

            let Some(stripped) = file_name.strip_suffix(".sql") else {
                return Err(invalid(format!(
                    "migration file `{file_name}` must end with `.sql`"
                )));
            };
            let (_, name) = stripped
                .split_once('_')
                .expect("the separator was checked when the id was parsed");

            let migration = Migration {
                id,
                name: name.to_string(),
                hash,
                content,
            };

            if let Some(previous) = res.last() {
                if migration.id == previous.id {
                    return Err(invalid(format!(
                        "migration index {} is used twice: `{}` and `{file_name}`",
                        migration.id, previous.name
                    )));
                }
                if migration.id != previous.id + 1 {
                    return Err(invalid(format!(
                        "migration index has a gap: {} does not follow {}",
                        migration.id, previous.id
                    )));
                }
            }

            res.push(migration);
        }

        Ok(res)
    }

    /// [`Self::try_build`], panicking on an invalid set.
    ///
    /// Kept because it is the published signature and the `migrate!` macro expands to it. New
    /// code should call `try_build`, which is what the client's migration path now does.
    pub fn build<T: RustEmbed>() -> Vec<Migration> {
        match Self::try_build::<T>() {
            Ok(migrations) => migrations,
            Err(err) => panic!("{err}"),
        }
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

    /// Replaces `a_name_without_a_numeric_index_panics_with_the_other_rules_message`, which
    /// pinned F-064: the fixture is named for a missing leading index and the message that
    /// fired was the one about increasing integers with no gaps, because the id parse is what
    /// a reader sees for `create_users.sql`.
    ///
    /// The message now names the rule that was actually broken, and it is a returned error.
    #[test]
    fn a_name_without_a_numeric_index_names_that_rule() {
        let err = Migrations::try_build::<Bad1>()
            .expect_err("a migration with no numeric index is not a valid set");
        let text = err.to_string();
        assert!(
            text.contains("must start with an integer index") || text.contains("no `_` separating"),
            "the message must name the rule that was broken, got: {text}"
        );
        assert!(
            !text.contains("no gaps"),
            "and must not name the gap rule, got: {text}"
        );
    }

    #[test]
    fn an_index_set_that_does_not_start_at_one_is_an_error() {
        let err = Migrations::try_build::<Bad2>().expect_err("indices must start at 1");
        assert!(
            err.to_string().contains("must start at index 1"),
            "got: {err}"
        );
    }

    /// F-065: a duplicate index was reported as "Migration index has a gap: 1 does not follow
    /// 1", which is neither true nor actionable. The fixture is two files claiming index 1.
    #[test]
    fn a_duplicate_index_says_so() {
        #[derive(RustEmbed)]
        #[folder = "tests/cluster/migrations/duplicate"]
        struct Duplicate;

        let err =
            Migrations::try_build::<Duplicate>().expect_err("two files cannot both be migration 1");
        let text = err.to_string();
        assert!(
            text.contains("used twice"),
            "the message must name the duplicate, got: {text}"
        );
        assert!(
            !text.contains("gap"),
            "and must not call it a gap, got: {text}"
        );
    }

    /// F-066: `build` panicked inside a function whose caller returns `Result`. `try_build` is
    /// what the client now calls, and `build` is kept as the published panicking wrapper.
    #[test]
    fn the_panicking_wrapper_still_panics_for_source_compatibility() {
        assert!(Migrations::try_build::<Good>().is_ok());
        let res = std::panic::catch_unwind(|| Migrations::build::<Bad2>());
        assert!(res.is_err(), "`build` keeps its published behavior");
    }
}
