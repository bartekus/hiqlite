# Security policy

## Reporting a vulnerability

Report security issues **privately, through GitHub's private vulnerability
reporting** for this repository (`bartekus/hiqlite`): open the repository's
**Security** tab and choose **Report a vulnerability**. That is the only
reporting channel; there is no security email address.

Please do not open a public issue, pull request or discussion for a suspected
vulnerability.

Private vulnerability reporting is a repository setting the repository owner
enables. If the **Report a vulnerability** button is not shown, the setting is
not yet on; do not fall back to a public channel.

## What is in scope

This fork's code and what it publishes: the `hiqlite-patched`,
`hiqlite-wal-patched` and `hiqlite-derive-patched` crates, the `spec-spine`
branch they are released from, and this repository's workflows. A finding that
also affects the upstream project (`sebadob/hiqlite`) belongs to that project's
own policy as well; this repository does not report on its behalf.

## Supported versions

Only the latest `0.15.0-patched.*` pre-release and the current `spec-spine`
branch. Earlier `-patched` releases are not repaired in place.
