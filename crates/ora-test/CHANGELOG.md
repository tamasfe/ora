# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## v0.2.0 (2023-09-15)

### New Features

 - <csr-id-933860bc82503d990938ad1925846eb0eecb0ee5/> handle concurrent workers
   - track workers per task to guarantee that at most one worker runs a task
   - renamed Worker -> Handler
   - renamed WorkerPool -> Worker

### Commit Statistics

<csr-read-only-do-not-edit/>

 - 1 commit contributed to the release.
 - 34 days passed between releases.
 - 1 commit was understood as [conventional](https://www.conventionalcommits.org).
 - 0 issues like '(#ID)' were seen in commit messages

### Commit Details

<csr-read-only-do-not-edit/>

<details><summary>view details</summary>

 * **Uncategorized**
    - Handle concurrent workers ([`933860b`](https://github.com/tamasfe/ora/commit/933860bc82503d990938ad1925846eb0eecb0ee5))
</details>

## v0.1.0 (2023-08-11)

<csr-id-d5cca440df67e94bb0cc18f8572518459d4264f1/>
<csr-id-7662ab153847c3818aee72e2ffcf752ef3797982/>

### Chore

 - <csr-id-d5cca440df67e94bb0cc18f8572518459d4264f1/> crate versions and changelog

### Chore

 - <csr-id-7662ab153847c3818aee72e2ffcf752ef3797982/> crate description

### Bug Fixes

 - <csr-id-8f03f918b44cfad310f0082e559fbc136d8f2170/> tokio features

### New Features

 - <csr-id-07c38305ea1c0ea48537aaac204698287bc44875/> initial implementation

### Commit Statistics

<csr-read-only-do-not-edit/>

 - 11 commits contributed to the release.
 - 4 commits were understood as [conventional](https://www.conventionalcommits.org).
 - 0 issues like '(#ID)' were seen in commit messages

### Commit Details

<csr-read-only-do-not-edit/>

<details><summary>view details</summary>

 * **Uncategorized**
    - Release ora-test v0.1.0, ora v0.1.0 ([`d02ee1b`](https://github.com/tamasfe/ora/commit/d02ee1b8ea8443b5e9b8f8874e512f073966f6be))
    - Crate description ([`7662ab1`](https://github.com/tamasfe/ora/commit/7662ab153847c3818aee72e2ffcf752ef3797982))
    - Release ora-store-sqlx v0.1.0, ora-test v0.1.0, ora v0.1.0 ([`709c80f`](https://github.com/tamasfe/ora/commit/709c80f3ab329c06af06b1efaa0ed39f59a3799a))
    - Release ora-store-memory v0.1.0, ora-store-sqlx v0.1.0, ora-test v0.1.0, ora v0.1.0 ([`9ac873b`](https://github.com/tamasfe/ora/commit/9ac873b7344a156234c49528d86b3c9ec0cb57b5))
    - Release ora-scheduler v0.1.0, ora-store-memory v0.1.0, ora-store-sqlx v0.1.0, ora-test v0.1.0, ora v0.1.0 ([`125e189`](https://github.com/tamasfe/ora/commit/125e1895e7c894c7c16f8eec01615fff19d7f421))
    - Release ora-util v0.1.0, ora-scheduler v0.1.0, ora-store-memory v0.1.0, ora-store-sqlx v0.1.0, ora-test v0.1.0, ora v0.1.0 ([`8fb9ee9`](https://github.com/tamasfe/ora/commit/8fb9ee956a23e1b243ea2bac14dc80cea7b2b5d9))
    - Release ora-timer v0.1.0, ora-util v0.1.0, ora-scheduler v0.1.0, ora-store-memory v0.1.0, ora-store-sqlx v0.1.0, ora-test v0.1.0, ora v0.1.0 ([`a2628e0`](https://github.com/tamasfe/ora/commit/a2628e02a6466893cd5e06b2973a46c301c7438b))
    - Tokio features ([`8f03f91`](https://github.com/tamasfe/ora/commit/8f03f918b44cfad310f0082e559fbc136d8f2170))
    - Release ora-common v0.1.0, ora-client v0.1.0, ora-worker v0.1.0, ora-api v0.1.0, ora-timer v0.1.0, ora-util v0.1.0, ora-scheduler v0.1.0, ora-store-memory v0.1.0, ora-store-sqlx v0.1.0, ora-test v0.1.0, ora v0.1.0 ([`cab6a7b`](https://github.com/tamasfe/ora/commit/cab6a7b16d23cb8a28d98e140d6fe5fdc4814c89))
    - Crate versions and changelog ([`d5cca44`](https://github.com/tamasfe/ora/commit/d5cca440df67e94bb0cc18f8572518459d4264f1))
    - Initial implementation ([`07c3830`](https://github.com/tamasfe/ora/commit/07c38305ea1c0ea48537aaac204698287bc44875))
</details>

