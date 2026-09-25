use crate::network::AppStateExt;
use crate::network::api::ApiStreamResponsePayload;
use crate::query::rows::{ColumnOwned, RowOwned, ValueOwned};
use crate::store::state_machine::sqlite::state_machine::{Query, QueryWrite, FORBIDDEN_NON_DET_FNS};
use crate::{Error, Params};
use tokio::sync::oneshot;
use tokio::task;
use tracing::debug;


/// The first SQL keyword token, lowercased, with leading whitespace and comments skipped.
///
/// Returns an empty string when there is no token. Character-boundary safe by construction: it
/// never slices by byte offset.
fn first_keyword(sql: &str) -> String {
    let mut rest = sql.trim_start();
    loop {
        if let Some(after) = rest.strip_prefix("--") {
            // a line comment runs to the end of the line
            rest = match after.find('\n') {
                Some(i) => after[i + 1..].trim_start(),
                None => "",
            };
            continue;
        }
        if let Some(after) = rest.strip_prefix("/*") {
            rest = match after.find("*/") {
                Some(i) => after[i + 2..].trim_start(),
                None => "",
            };
            continue;
        }
        break;
    }

    rest.chars()
        .take_while(|c| c.is_alphanumeric() || *c == '_')
        .flat_map(|c| c.to_lowercase())
        .collect()
}

pub(crate) async fn dashboard_query_dynamic(
    state: AppStateExt,
    sql: String,
) -> Result<Vec<RowOwned>, Error> {
    if sql.trim().is_empty() {
        return Err(Error::BadRequest("invalid query".into()));
    }

    if state.raft_db.log_statements {
        debug!("dashboard query:\n{}", sql)
    }

    // we need to check if we can do a local select query or if it is
    // modifying and needs to go through the raft
    //
    // F-086: `sql[..7]` is a byte slice, which panics when byte 7 is not a UTF-8 character
    // boundary, and the body it slices comes from `String::from_utf8_lossy` over the raw
    // request. F-088: a seven-byte prefix also misclassifies every read that does not begin
    // with one of the three keywords, so `WITH x AS (SELECT 1) SELECT ...`, `VALUES (1)` and
    // anything preceded by a comment took the **raft write** path: a read charged as a
    // replicated write, and refused by the non-determinism guard.
    //
    // Both go away by asking for the first keyword token instead of a fixed number of bytes.
    let is_select = matches!(
        first_keyword(&sql).as_str(),
        "select" | "explain" | "pragma" | "with" | "values"
    );

    if is_select {
        let conn = state.raft_db.read_pool.get().await?;

        task::spawn_blocking(move || {
            let mut stmt = conn.prepare(&sql)?;

            let columns = ColumnOwned::mapping_cols_from_stmt(stmt.columns())?;

            let mut rows = stmt.raw_query();
            let mut rows_owned = Vec::new();
            loop {
                match rows.next() {
                    Ok(Some(row)) => rows_owned.push(RowOwned::from_row_column(row, &columns)),
                    Ok(None) => break,
                    Err(err) => {
                        // never silently show a truncated result in the dashboard
                        return Err(Error::Sqlite(err.to_string().into()));
                    }
                }
            }

            Ok::<Vec<RowOwned>, Error>(rows_owned)
        })
        .await?
    } else {
        // The write path panics on non-deterministic functions. Dashboard queries are
        // manual, so catch them up front and return a readable error instead.
        if let Some(fn_name) = find_forbidden_non_det_fn(&sql) {
            return Err(Error::BadRequest(
                format!(
                    "`{fn_name}()` is non-deterministic and must never be used for writing \
                    queries in a Raft cluster"
                )
                .into(),
            ));
        }

        let sql = Query {
            sql: sql.into(),
            params: Params::new(),
        };

        // TODO check for `RETURNING` to execute `query` instead
        let rows_affected = match execute_dynamic(&state, sql.clone()).await {
            Ok(r) => r,
            Err(err) => {
                if let Some((id, node)) = err.is_forward_to_leader() {
                    state
                        .tx_client_stream
                        .send_async(crate::client::stream::ClientStreamReq::LeaderChange((
                            id,
                            node.clone(),
                        )))
                        .await
                        .map_err(|err| Error::Error(err.to_string().into()))?;
                    execute_dynamic(&state, sql.clone()).await?
                } else {
                    return Err(err);
                }
            }
        };

        let affected = if rows_affected > i64::MAX as usize {
            i64::MAX
        } else {
            rows_affected as i64
        };
        Ok(vec![RowOwned {
            columns: vec![ColumnOwned {
                name: "rows_affected".to_string(),
                value: ValueOwned::Integer(affected),
            }],
        }])
    }
}

#[inline]
async fn execute_dynamic(state: &AppStateExt, sql: Query) -> Result<usize, Error> {
    // `034` B-4: the dashboard's writes are held like every other client's during a restore.
    #[cfg(feature = "backup")]
    crate::app_state::ensure_not_restoring(&state.restore_hold)?;
    if is_this_local_leader(state).await? {
        debug!("Executing dynamic dashboard query as local leader");
        let res = state
            .raft_db
            .raft
            .client_write(QueryWrite::Execute(sql))
            .await?;
        let resp: crate::Response = res.data;
        match resp {
            crate::Response::Execute(res) => res.result,
            _ => unreachable!(),
        }
    } else {
        debug!("Executing dynamic dashboard query on remote leader");
        let (ack, rx) = oneshot::channel();
        state
            .tx_client_stream
            .send_async(crate::client::stream::ClientStreamReq::Execute(
                crate::client::stream::ClientExecutePayload {
                    request_id: state.new_request_id(),
                    sql,
                    ack,
                },
            ))
            .await
            .map_err(|err| Error::Error(err.to_string().into()))?;
        let res = rx
            .await
            .expect("To always receive an answer from Client Stream Manager")?;
        match res {
            ApiStreamResponsePayload::Execute(res) => res,
            _ => unreachable!(),
        }
    }
}

#[inline(always)]
pub(crate) async fn is_this_local_leader(state: &AppStateExt) -> Result<bool, Error> {
    match state.raft_db.raft.current_leader().await {
        None => Err(Error::LeaderChange(
            "Leader election has not finished yet".into(),
        )),
        Some(current) => {
            if state.id == current {
                Ok(true)
            } else {
                Ok(false)
            }
        }
    }
}

/// Finds a forbidden non-deterministic function in a manual dashboard query.
/// String literals and comments are skipped, so `'now()'` is not taken for a call.
fn find_forbidden_non_det_fn(sql: &str) -> Option<&'static str> {
    let lowered = sql.to_ascii_lowercase();
    let bytes = lowered.as_bytes();
    let mut i = 0;
    let mut in_string = false;
    while i < bytes.len() {
        if in_string {
            if bytes[i] == b'\'' {
                // SQL escapes a quote by doubling it: '' stays inside the string
                if i + 1 < bytes.len() && bytes[i + 1] == b'\'' {
                    i += 2;
                    continue;
                }
                in_string = false;
            }
            i += 1;
            continue;
        }

        match bytes[i] {
            b'\'' => {
                in_string = true;
                i += 1;
            }
            b'-' if bytes.get(i + 1) == Some(&b'-') => {
                // skip to end of line
                i += 2;
                while i < bytes.len() && bytes[i] != b'\n' {
                    i += 1;
                }
            }
            b'/' if bytes.get(i + 1) == Some(&b'*') => {
                // skip until */
                i += 2;
                while i + 1 < bytes.len() && !(bytes[i] == b'*' && bytes[i + 1] == b'/') {
                    i += 1;
                }
                i = (i + 2).min(bytes.len());
            }
            _ => {
                if let Some(name) = FORBIDDEN_NON_DET_FNS.iter().copied().find(|name| {
                    let needle = name.as_bytes();
                    bytes[i..].starts_with(needle)
                        && bytes[i..].get(needle.len()) == Some(&b'(')
                        && (i == 0
                            || {
                                let prev = bytes[i - 1];
                                !prev.is_ascii_alphanumeric() && prev != b'_'
                            })
                }) {
                    return Some(name);
                }
                i += 1;
            }
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    /// F-086 and F-088 are one line and two defects.
    ///
    /// F-086: `sql[..7]` is a **byte** slice over a body that came from
    /// `String::from_utf8_lossy`, so a query whose byte 7 is inside a multi-byte character
    /// panicked the handler. F-088: a seven-byte prefix also misclassifies every read that does
    /// not begin with one of three keywords, so a CTE, a bare `VALUES`, or anything preceded by
    /// a comment took the **raft write** path: a read charged as a replicated write.
    #[test]
    fn the_first_keyword_is_found_past_whitespace_and_comments() {
        // Reads the old prefix check got right.
        assert_eq!(first_keyword("SELECT 1"), "select");
        assert_eq!(first_keyword("explain query plan select 1"), "explain");
        assert_eq!(first_keyword("PRAGMA table_info(x)"), "pragma");

        // Reads it sent through the raft (F-088).
        assert_eq!(first_keyword("WITH x AS (SELECT 1) SELECT * FROM x"), "with");
        assert_eq!(first_keyword("VALUES (1)"), "values");
        assert_eq!(first_keyword("   \n\t SELECT 1"), "select");
        assert_eq!(first_keyword("-- a note\nSELECT 1"), "select");
        assert_eq!(first_keyword("/* a note */ SELECT 1"), "select");
        assert_eq!(first_keyword("/* one */ -- two\n SELECT 1"), "select");

        // Writes stay writes.
        assert_eq!(first_keyword("INSERT INTO t VALUES (1)"), "insert");
        assert_eq!(first_keyword("update t set a = 1"), "update");
        assert_eq!(first_keyword("DELETE FROM t"), "delete");

        // Multi-byte input is answered, not panicked (F-086). Byte 7 of each of these is
        // inside a character.
        assert_eq!(first_keyword("\u{20ac}\u{20ac}\u{20ac}"), "");
        assert_eq!(first_keyword("SELECT \u{20ac}"), "select");
        assert_eq!(first_keyword(""), "");
        assert_eq!(first_keyword("   "), "");
        assert_eq!(first_keyword("--"), "");
        assert_eq!(first_keyword("/* unterminated"), "");
    }

    #[test]
    fn forbidden_fn_scan_catches_only_real_calls() {
        // forbidden calls are detected ...
        assert_eq!(find_forbidden_non_det_fn("INSERT INTO t VALUES (now())"), Some("now"));
        assert_eq!(
            find_forbidden_non_det_fn("UPDATE t SET at = strftime('%s','now') WHERE id = 1"),
            Some("strftime")
        );
        assert_eq!(
            find_forbidden_non_det_fn("INSERT INTO t VALUES (datetime('now'))"),
            Some("datetime")
        );
        assert_eq!(find_forbidden_non_det_fn("INSERT INTO t VALUES (NOW())"), Some("now"));

        // ... while names that merely contain a forbidden fn do not match
        assert_eq!(find_forbidden_non_det_fn("SELECT * FROM my_now"), None);
        assert_eq!(
            find_forbidden_non_det_fn("INSERT INTO t VALUES ('a strftime b')"),
            None
        );

        // ... and forbidden names inside string literals or comments are not calls
        assert_eq!(find_forbidden_non_det_fn("INSERT INTO t VALUES ('now()')"), None);
        assert_eq!(find_forbidden_non_det_fn("INSERT INTO t VALUES ('a''now()''b')"), None);
        assert_eq!(
            find_forbidden_non_det_fn("-- now()\nINSERT INTO t VALUES (1)"),
            None
        );
        assert_eq!(
            find_forbidden_non_det_fn("/* now() */ INSERT INTO t VALUES (1)"),
            None
        );
    }
}
