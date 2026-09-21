---
id: "010-node-lifecycle-and-split-brain"
title: "Adopt the node lifecycle and the split-brain observation path"
status: draft
kind: "adoption"
created: "2026-09-21"
owner: "hiqlite maintainers"
risk: high
implementation: complete
depends_on:
  - "000-hiqlite-ownership-bootstrap"
  - "003-client-consistency-and-retry-outcomes"
  - "009-configuration-contract"
origin:
  retroactive: true
  paths:
    - "hiqlite/src/start.rs"
    - "hiqlite/src/init.rs"
    - "hiqlite/src/app_state.rs"
    - "hiqlite/src/split_brain_check.rs"
establishes:
  - "hiqlite/src/start.rs"
  - "hiqlite/src/init.rs"
  - "hiqlite/src/app_state.rs"
  - "hiqlite/src/split_brain_check.rs"
extends:
  - spec: "000-hiqlite-ownership-bootstrap"
    unit: { kind: file, path: "spec-spine.toml" }
    nature: additive
  - spec: "009-configuration-contract"
    unit: { kind: file, path: "hiqlite.env" }
    nature: additive
  - spec: "005-adoption-assessment-and-plan"
    unit: { kind: file, path: "standards/spec/findings-register.md" }
    nature: additive
  - spec: "005-adoption-assessment-and-plan"
    unit: { kind: file, path: "standards/spec/adoption-plan.md" }
    nature: additive
summary: >
  Adopts what a hiqlite node does between process start and process exit: the
  startup order, how a node resolves its own identity and listen addresses, how
  a fresh node 1 initializes, how every other node joins as learner and then
  voter, what the reset escape hatch destroys, which background tasks are
  spawned and owned by nobody, and what the split-brain checker observes. Six
  defects are recorded, four of them new and two executed. Documents
  HQL_SPLIT_BRAIN_INTERVAL, closing F-011. Repairs no runtime behavior.
---

# 010: Adopt the node lifecycle and the split-brain observation path

## 1. Purpose

`009` adopted what a node is configured with. Nothing yet adopts what it then
does with that configuration. `start_node_inner` is the single entry point for
every embedded node and every `hiqlite-server` process, and between it and
`init.rs` sit the decisions that determine whether a cluster forms, re-forms, or
quietly fails to: which node initializes the Raft, which nodes join it, what a
restart after volume loss does to remote membership, and what happens when any
of that goes wrong.

**It is an adoption.** Every behavior below is described as found. Six defects
are recorded under `known-defects` and left unfixed, per constitution VI. The
only non-test change to a shipped file is one comment block added to
`hiqlite.env` (section 5, F-011). No runtime behavior changes.

## 2. Territory

**Establishes**, none of which any spec claimed before:

- `hiqlite/src/start.rs`, the node entry point and the listener wiring;
- `hiqlite/src/init.rs`, pristine initialization, the join sequence, the leave
  sequence, and the reset escape hatch;
- `hiqlite/src/app_state.rs`, the shared node state the whole process reads;
- `hiqlite/src/split_brain_check.rs`, the membership divergence observer.

**Extends**, without re-establishing:

- `000` on `spec-spine.toml`, for the freshness declarations of section 8;
- `009` on `hiqlite.env`, to document `HQL_SPLIT_BRAIN_INTERVAL`, which `009`
  section 7 deferred to this spec by name;
- `005` on the findings register and the adoption plan, to record this spec's
  defects and to move the `W-02` row, following `008`'s precedent.

**Depends on** `003` for the shutdown sequence. `hiqlite/src/client/` is `003`'s
unit and this spec does not touch it. What this spec owns is the node-side half:
the `is_shutting_down` flag and the `tx_shutdown` watch channel created here and
consumed there. B-7 states that boundary precisely because a defect sits exactly
on it.

## 3. Behavior

### B-1. Startup is a fixed order, and most of it is not recoverable

`start_node_inner` (`start.rs:24-318`) runs, in order: configuration validation
(`:28`), the process-wide rustls provider install (`:30-35`), TLS material
resolution (`:37-43`), encryption key init and dashboard init (`:45-49`), backup
restore (`:51-52`), Raft config validation (`:54`), the reset check (`:56`),
storage start for each enabled Raft group (`:57-62`), address derivation
(`:64-78`), `AppState` construction (`:83-111`), the split-brain spawn
(`:113-118`), the shutdown channel (`:125`), the two HTTP servers (`:127-246`),
the join tasks (`:248-289`), client construction (`:291-306`), and the backup
cron (`:309-315`).

Three of those steps end the process rather than return an error:
`raft_config.validate().unwrap()` at `:54`, the node lookup `expect` at `:65-68`,
and `check_execute_reset`'s parse `expect` (`init.rs:29-33`). A fourth class is
deferred into spawned tasks and discussed in B-7.

The join step is the only one that blocks: `member_db.await??` and
`member_cache.await??` (`:286-289`) mean `start_node_inner` does not return until
this node is a replicated member of every enabled Raft group, or `learner_only`
short-circuits it (B-5). The HTTP servers are already listening by then, which is
required: peers reach this node's `/cluster/*` endpoints during the join.

### B-2. A node's listen address is its own host plus its peers' idea of its port

`build_listen_addr` (`start.rs:321-330`) takes the host from
`listen_addr_api` / `listen_addr_raft` and the port from the **advertised**
address in `nodes` (`addr_api` / `addr_raft`), splitting at the first `:`. With
no port on the advertised address it falls back to `443` when that endpoint has
TLS material and `80` otherwise. This is the only place in the lifecycle where
TLS changes an address.

The split is `str::split_once(':')`, which is correct for `host:port` and wrong
for a bracketed IPv6 literal. KD-1 records that; the first two cases are
asserted by `listen_port_comes_from_the_advertised_address` and
`missing_advertised_port_falls_back_to_the_scheme_default`.

### B-3. Node identity is resolved two different ways

`start.rs:65-68` resolves "this node" **positionally**, as
`nodes[node_id - 1]`, and `expect`s the index. `init.rs:138-148`
(`get_this_node`) resolves it **by id**, filtering `nodes` for `id == node_id`,
and `expect`s a match. `NodeConfig::is_valid` (`config.rs:429-431`, `009`'s
extended unit) bounds `node_id` against `nodes.len()` and never inspects the ids.

The two agree only when the ids are exactly `1..=n` in ascending order. `Node`'s
doc comment (`lib.rs:131-133`) requires an `id == 1` to exist and says nothing
about the rest. KD-2 records the divergence; both halves are executed.

### B-4. A pristine node 1 initializes itself, and checks first that it should

`init_pristine_node_1_db` and `init_pristine_node_1_cache`
(`init.rs:73-136`) do nothing unless `node_id == 1`. Node 1 then asks two
questions in order.

`is_initialized_timeout_*` (`:785-858`) returns `true` immediately if the Raft
reports initialized **and** this node appears in its own membership. Otherwise it
sleeps five heartbeat intervals and re-asks, so that a node whose volume was lost
does not race a still-running cluster. Initialized with an empty membership logs
`log_no_membership_error` (`:890-907`), which names
`HQL_DANGER_RAFT_STATE_RESET` as the operator recovery, and returns `false`.

`should_node_1_skip_init` (`:151-224`) then polls every other node's
`/cluster/membership/{raft_type}` until either some remote answers with a
non-empty membership, in which case node 1 skips its own init, or `nodes.len()/2`
remotes have answered with an error, in which case it proceeds. Unreachable
remotes are retried forever with a one-second pause: a connection error does not
count toward the error quorum, only an HTTP error response does. A remote that
answers successfully with an **empty** membership panics on purpose
(`:199-204`), described there as impossible.

The cache group skips the persistence check when `cache_storage_disk` is false
(`:119`), because an in-memory group has no state to have preserved.

### B-5. Every other node joins as learner, then as voter, and waits for both to replicate

`become_cluster_member` (`init.rs:236-414`) is the join sequence.

If the Raft is already initialized it sets the running flags (B-6) and, for a
multi-node cluster, waits up to five seconds for a leader that is **not** this
node, then returns (`:244-273`). The comment records why: after a fast restart
this node can still be the recorded leader.

Otherwise the node first checks whether a remote cluster still lists it as a
member (`is_remote_cluster_member`, `:541-641`) and, if so, leaves that cluster
before proceeding (`leave_remote_cluster`, `:645-780`), with the `expect` at
`:288` making a failed leave fatal. That is deliberate: the case it handles is a
node whose volume was lost while the cluster kept its membership entry.

It then posts `add_learner` to peers in order until one succeeds (`try_become`,
`:417-536`), waits until it can see itself in the replicated membership
(`:334-355`), and, unless `learner_only` is set (`:357-364`), posts
`become_member` and waits again until it appears among the replicated
`voter_ids` (`:388-411`). Both waits are unbounded one-second loops with no
timeout and no attempt limit.

`try_become` is also an unbounded loop. A remote error that decodes as
"forward to leader" naming **this** node is either a fast-restart race, handled
by returning `SkipBecome::Yes`, or, if this node's Raft is not initialized,
a deliberate panic (`:494-506`) whose message explains the in-memory-cache
restart timing that causes it. Every other error and every connection error
sleeps 500ms and tries the next peer, forever.

### B-6. `AppState` is the process-wide node state, and two flags gate readiness

`AppState` (`app_state.rs:52-78`) is constructed once in `start.rs:83-111`, wrapped
in an `Arc`, and shared by the HTTP handlers, the client, the split-brain checker
and the join tasks. It carries no lock over itself; the mutable parts are the
atomics `is_shutting_down` and `client_request_id`, the per-group
`is_raft_stopped` and `is_startup_finished` flags on `StateRaftDB` /
`StateRaftCache`, and `raft_lock`.

`set_raft_running` (`init.rs:860-888`) is the only writer of the startup pair: it
clears `is_raft_stopped` and sets `is_startup_finished` for one group. It is
called on both join routes, and `RaftType::Unknown` reaches `unreachable!()`
there. Every atomic in the lifecycle path uses `Ordering::Relaxed`.

`is_shutting_down` is read by `api::ready` (`network/api.rs:99`), which is what
lets an orchestrator drain a node before `003`'s shutdown sequence begins.

### B-7. Background tasks are spawned and owned by nobody

Six tasks are spawned during startup and their handles are dropped: the two HTTP
servers (`start.rs:141`, `:151`, `:227`, `:237`), the split-brain checker and its
watchdog (`split_brain_check.rs:13`, `:16`), and the backup cron. Only the two
join tasks are awaited, and only until the join completes.

Inside those dropped-handle tasks, failure is expressed as `expect` and `unwrap`:
the socket address parse (`start.rs:142`, `:228`), the TCP bind (`:152-154`,
`:238-240`), and `serve(...).unwrap()` in all four branches. **A listener that
cannot bind therefore does not fail startup.** The panic is confined to its task
under an unwinding profile, `start_node_inner` returns `Ok`, and the node reports
itself started with no listener on that address. Under this repository's
`panic = "abort"` release profile (`Cargo.toml:15`) the same failure ends the
process instead. KD-4 records this; F-025 records the same split for the cache
handlers.

**The shutdown boundary.** `start.rs:125` creates the `tx_shutdown` watch channel.
Exactly two receivers ever exist: the one `shutdown_signal` future built at `:138`
and `rx_shutdown` itself at `:242`. Each is consumed only on the **plaintext**
branch of its server; on the TLS branch the `axum_server` task is spawned with no
shutdown future at all, which the two `TODO` comments at `:143-144` and `:229-230`
acknowledge. `003`'s `shutdown_execute` ends by sending on that channel
(`client/mgmt.rs:374-376`) and `expect`s the send. KD-3 records what follows.

### B-8. The split-brain checker observes, and reports only to the log

`spawn` (`split_brain_check.rs:12-22`) starts the checker and a watchdog. The
checker reads `HQL_SPLIT_BRAIN_INTERVAL`, defaulting to 60 seconds, and
`expect`s the parse (F-009). Then, forever: for each enabled Raft group, read the
local leader and membership, and for every node that is not the expected leader,
fetch `/cluster/metrics/{group}` and compare.

Two things are reported, both as log records and nothing else. A node from the
configured `nodes` list that is missing from a remote's membership produces a
`warn!` whose text states that this is a split brain **if** the missing node is
up, and is ignorable if it is starting or offline (`:103-111`); the check cannot
distinguish those cases, so the decision is left to the reader. A remote
membership that differs from the local one produces an `error!` (`:156-159`).

**Nothing acts on either.** No metric, no health-check effect, no
`is_shutting_down`, no error returned anywhere. The checker is an observability
path, and F-014 records what happens when the observability path itself fails.

The watchdog is a second task that sleeps 600 seconds and asserts the checker's
handle is not finished, under a comment calling it "just a safety net until
everything runs super smooth and stable". F-014 records, in full, why it achieves
nothing under either panic strategy, and section 6 states the decision that keeps
it as found.

## 4. Evidence and its limits

Six characterization tests were added, three in `start.rs` and three in
`init.rs`. Each asserts current behavior and would fail if that behavior changed.
None is a regression test for a repair, because nothing is repaired here.

| test | what it establishes |
|---|---|
| `start::tests::listen_port_comes_from_the_advertised_address` | the port comes from `nodes[..].addr_*`, the host from `listen_addr_*` (B-2) |
| `start::tests::missing_advertised_port_falls_back_to_the_scheme_default` | 443 with TLS, 80 without, and that this is TLS's only effect on the address (B-2) |
| `start::tests::ipv6_advertised_address_produces_an_unparsable_listen_address` | the exact malformed string, and that `SocketAddr::from_str` rejects it (KD-1) |
| `init::tests::node_identity_is_resolved_by_id_here_and_by_position_in_start` | with ids `2,3,4` the two routes name different nodes (KD-2) |
| `init::tests::is_valid_accepts_a_nodes_list_whose_ids_are_not_positions` | that configuration is accepted as valid (KD-2) |
| `init::tests::get_this_node_panics_when_the_id_is_absent` | the join path panics instead of returning a configuration error (KD-2) |

**What the tests do not establish.**

- **Anything that requires a running node or a cluster.** B-1's ordering, B-4's
  two-question init, B-5's whole join sequence, B-6's flag transitions, B-7's
  listener and shutdown behavior and B-8's comparison loop are **read from source
  and not executed**. No node was started, no Raft was initialized, no HTTP
  request was made, and no process was shut down by these tests.
- **KD-3 and KD-4 are source-established.** Both are statements about which
  values are moved into which spawned task, and reproducing either needs a node
  with real TLS material and a real shutdown. Section 9's acceptance block pins
  their source shape by assertion instead, so the description cannot silently
  stop matching the code; that is not the same as executing the failure.
- **Nothing about the abort profile.** Every claim about `panic = "abort"` is
  read from `Cargo.toml:15`. The tests run under the default unwinding profile,
  which is also why `get_this_node_panics_when_the_id_is_absent` can be written
  at all: under abort there would be no test process left to observe the panic.
  This asymmetry is the point of the distinction and is stated again in KD-4.
- **Nothing about a downstream consumer's profile.** hiqlite is primarily an
  embeddable library, so `Cargo.toml:15` governs this repository's binaries and
  not the applications that link it. F-014 already records this; every
  abort-versus-unwind statement in section 5 inherits the same limit.

## 5. Documenting `HQL_SPLIT_BRAIN_INTERVAL`, and why only in `hiqlite.env`

F-011 records that `HQL_SPLIT_BRAIN_INTERVAL` appears in neither reference file,
so an operator cannot discover a variable that, per F-009, ends the process when
set wrongly. `009` section 7 left it to "the lifecycle adoption that owns the
checker", which is this spec.

It is documented in `hiqlite.env` only. `split_brain_check.rs:25` reads it with
`env::var` directly; `config_toml.rs` has no corresponding key and never reads
it, so a TOML entry would document a setting that does not exist. Adding it to
`hiqlite.toml` would create a defect rather than close one.

This closes F-011, which is an `evidence` finding about a documentation gap, and
it is the only change to a shipped non-test file in this spec. It is recorded as
a repair rather than performed silently. The variable's parse behavior is
unchanged, and F-009 stays open.

## 6. Known defects

Recorded as found, none repaired here. Each is also filed in
`standards/spec/findings-register.md` so later work can cite a stable id.

**KD-1. A bracketed IPv6 advertised address yields an unparsable listen address**
(F-037). `build_listen_addr` (`start.rs:321-330`) splits the advertised address at
the first `:`. For `[fd00::1]:8100` that is the colon **inside** the brackets, so
the "port" is `:1]:8100` and the result is `::::1]:8100`. `SocketAddr::from_str`
rejects it at `start.rs:142` / `:228`, inside a task whose handle was dropped, so
under unwinding the node starts with no listener on that address and reports
success. `NodeConfig::is_valid` validates no address at all. Observed by
execution, both the malformed string and its rejection.

**KD-2. `node_id` means a position in one file and an id in another** (F-038).
`start.rs:65-68` indexes `nodes[node_id - 1]` to pick the addresses this node
binds; `init.rs:138-148` searches `nodes` for `id == node_id` to pick the identity
this node registers with the cluster. `is_valid` (`config.rs:429-431`) bounds
`node_id` by `nodes.len()` only. With ids `2,3,4` and `node_id = 3`, the node
binds node 4's ports and joins as node 3; with an id missing entirely, the join
path panics. Observed by execution in all three parts.

**KD-3. A fully TLS-configured node cannot be shut down without a panic**
(F-039). The `tx_shutdown` watch channel has exactly two receivers
(`start.rs:138`, `:242`), and each is consumed only on the plaintext branch of
its server. With `tls_raft` **and** `tls_api` both set, neither branch runs,
both receivers drop when `start_node_inner` returns, and `003`'s
`shutdown_execute` then panics at `client/mgmt.rs:374-376`, where the send is
`expect`ed with "The global Hiqlite shutdown handler to always listen". With
exactly one of the two set, one receiver survives and the send succeeds, but
that receiver belongs to the plaintext server: **no TLS listener has ever been
given a shutdown future**, so a TLS-configured endpoint keeps accepting until
the process exits. The two `TODO` comments at `:143-144` and `:229-230`
acknowledge the missing graceful shutdown and not the panic. Source-established;
section 4 states why, and the acceptance block pins the shape.

**KD-4. A listener that cannot bind does not fail startup** (F-040). The socket
address parse, the TCP bind and `serve` are all `expect`ed or `unwrap`ped inside
tasks whose `JoinHandle`s are dropped (`start.rs:141-160`, `:227-246`). Under an
unwinding profile the panic stays in the task, `start_node_inner` returns `Ok`,
and the node is reported started with an endpoint that does not exist; under this
repository's `panic = "abort"` release profile the same failure ends the process.
Neither outcome is a startup error, and which one occurs is a property of the
**consumer's** build profile for an embedded node. Source-established. This is
the same class as F-025 and the same profile split F-009 and F-014 record.

**Retained without change.** F-009 (`HQL_SPLIT_BRAIN_INTERVAL` parse `expect`,
`split_brain_check.rs:25-29`) and F-014 (the watchdog achieves nothing under
either panic strategy) both sit inside units this spec now establishes. They are
described in B-8 and left exactly as the register states them. F-025's cache
handler `expect`s sit in `006`'s unit and are cited, not adopted.

## 7. Resolved decisions

**D-1 (2026-09-21, the watchdog is described, not changed).** OD-3 in the
adoption plan records the owner's direction: preserve behavior, investigate
first. B-8 and F-014 together are that investigation written down. Removing the
watchdog, making it report, or making it terminate deliberately are three
different policies with three different consequences, and choosing among them is
not an adoption act. The row stays open under W-22.

**D-2 (2026-09-21, KD-3 and KD-4 are recorded, not repaired).** Both have
obvious-looking one-line fixes: subscribe a receiver unconditionally, or retain
the `JoinHandle`s. Both fixes change what a node does when a listener fails,
which is precisely the policy W-22 exists to decide, and KD-3's repair also
changes behavior inside `003`'s unit. Repairing either here would make this spec
a repair wearing an adoption's frontmatter.

**D-3 (2026-09-21, the shutdown sequence stays `003`'s).** This spec claims the
four lifecycle files and describes the node-side half of shutdown in B-7. It
takes no edge on `hiqlite/src/client/`. KD-3's failure is observed in `003`'s
file and caused in this spec's file, so it is recorded here, where the cause is,
and cited from the register where `003` can reach it.

**D-4 (2026-09-21, no test starts a node).** A test that established B-4 or B-5
would need a multi-node cluster with real timing, which is what
`hiqlite/tests/cluster/` is for and what F-017 and W-15 record as unclaimed and
partly non-completing. Adding to that surface is W-15's work, not this spec's.
Section 4 states the resulting limit rather than hiding it behind six tests that
do not reach the claims.

## 8. The inventory declarations this spec requires

None of the four established files is in any content hash at the pinned revision:
`[index] extra_hashed_inputs` covers `hiqlite/src/config.rs`,
`hiqlite/src/config_toml.rs`, the query, client, network and sqlite state-machine
trees, but not `hiqlite/src/*.rs` at the crate root. A claim on them resolves,
but `spec-spine lint --fail-on-warn` exits `1` with `L-008` warnings, because a
file in no content hash can change without staling any shard, and `just
spine-check` runs that lint.

`spec-spine.toml` therefore gains four `extra_hashed_inputs` entries:
`hiqlite/src/start.rs`, `hiqlite/src/init.rs`, `hiqlite/src/app_state.rs`,
`hiqlite/src/split_brain_check.rs`. No `[coverage] governed_scope` entry is
needed: all four are `.rs` files inside the `hiqlite` cargo package, which the
package walk already counts in the denominator.

This is `000` section 12.1's pattern and `009` section 6's precedent. **It is a
freshness declaration, not an enforcement setting**: `coupling.require_ownership`,
`coupling.bypass_prefixes` and `index coverage --fail-on-untraced` are untouched.

## 9. Out of scope

- **Every repair.** KD-1 through KD-4 are recorded and left, and F-009 and F-014
  are retained as the register states them.
- **The startup-error and background-task policy.** W-22 carries it, and KD-3,
  KD-4, F-009, F-014 and F-025 are five instances of the same open question.
- **`hiqlite/src/tls.rs`.** The TLS material this lifecycle consumes is W-03's,
  adopted separately. B-7 describes what `start.rs` does with a
  `ServerTlsConfig`, not how that config is built or trusted.
- **`hiqlite/src/client/`.** `003`'s unit, including the shutdown sequence whose
  final send KD-3 is about.
- **`hiqlite/src/network/`, `hiqlite/src/store/`, `hiqlite/src/helpers.rs` and
  `hiqlite/src/http_client.rs`.** The join sequence calls into all four; none is
  claimed here.
- **Ratification of anything, and any enforcement change.**

## Verification

Run with `just spine-verify 010`.

```verify:cli
test -f hiqlite/src/start.rs
test -f hiqlite/src/init.rs
test -f hiqlite/src/app_state.rs
test -f hiqlite/src/split_brain_check.rs
sh -c 'spec-spine index owner hiqlite/src/start.rs | grep -q 010-node-lifecycle-and-split-brain'
sh -c 'spec-spine index owner hiqlite/src/init.rs | grep -q 010-node-lifecycle-and-split-brain'
sh -c 'spec-spine index owner hiqlite/src/app_state.rs | grep -q 010-node-lifecycle-and-split-brain'
sh -c 'spec-spine index owner hiqlite/src/split_brain_check.rs | grep -q 010-node-lifecycle-and-split-brain'
sh -c 'spec-spine registry relationships 010-node-lifecycle-and-split-brain | grep -q 009-configuration-contract'
cargo test -p hiqlite --lib start::tests::listen_port_comes_from_the_advertised_address -- --exact
cargo test -p hiqlite --lib start::tests::missing_advertised_port_falls_back_to_the_scheme_default -- --exact
cargo test -p hiqlite --lib start::tests::ipv6_advertised_address_produces_an_unparsable_listen_address -- --exact
cargo test -p hiqlite --lib init::tests::node_identity_is_resolved_by_id_here_and_by_position_in_start -- --exact
cargo test -p hiqlite --lib init::tests::is_valid_accepts_a_nodes_list_whose_ids_are_not_positions -- --exact
cargo test -p hiqlite --lib init::tests::get_this_node_panics_when_the_id_is_absent -- --exact
grep -q '.get(node_config.node_id as usize - 1)' hiqlite/src/start.rs
grep -q 'expect("this node to always exist in all nodes")' hiqlite/src/init.rs
grep -q 'let (tx_shutdown, rx_shutdown) = tokio::sync::watch::channel(false);' hiqlite/src/start.rs
grep -q 'let shutdown = shutdown_signal(rx_shutdown.clone());' hiqlite/src/start.rs
grep -q 'with_graceful_shutdown(shutdown_signal(rx_shutdown))' hiqlite/src/start.rs
sh -c 'test "$(grep -c "with_graceful_shutdown" hiqlite/src/start.rs)" -eq 2'
sh -c 'test "$(grep -c "axum_server::bind_rustls" hiqlite/src/start.rs)" -eq 2'
grep -q 'expect("The global Hiqlite shutdown handler to always listen")' hiqlite/src/client/mgmt.rs
grep -q 'assert!(!handle.is_finished())' hiqlite/src/split_brain_check.rs
grep -q 'expect("Cannot parse HQL_SPLIT_BRAIN_INTERVAL as u64")' hiqlite/src/split_brain_check.rs
grep -q 'HQL_SPLIT_BRAIN_INTERVAL' hiqlite.env
sh -c '! grep -q "split_brain" hiqlite.toml'
grep -q 'panic = "abort"' Cargo.toml
grep -q 'hiqlite/src/start.rs' spec-spine.toml
grep -q 'hiqlite/src/init.rs' spec-spine.toml
grep -q 'hiqlite/src/app_state.rs' spec-spine.toml
grep -q 'hiqlite/src/split_brain_check.rs' spec-spine.toml
spec-spine check --fail-on-unresolved --fail-on-warn
spec-spine lint --fail-on-warn
spec-spine index coverage
sh -c '! grep -rl "$(printf "\342\200\224")" specs/010-node-lifecycle-and-split-brain'
```
