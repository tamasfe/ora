# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## v0.2.0 (2023-09-15)

### New Features

 - <csr-id-933860bc82503d990938ad1925846eb0eecb0ee5/> handle concurrent workers
   - track workers per task to guarantee that at most one worker runs a task

### Commit Statistics

<csr-read-only-do-not-edit/>

 - 3 commits contributed to the release.
 - 34 days passed between releases.
 - 1 commit was understood as [conventional](https://www.conventionalcommits.org).
 - 0 issues like '(#ID)' were seen in commit messages

### Commit Details

<csr-read-only-do-not-edit/>

<details><summary>view details</summary>

 * **Uncategorized**
    - Release ora-worker v0.2.1, ora-api v0.2.0, ora-store-memory v0.2.0, ora-store-sqlx v0.2.0, ora-test v0.2.0, ora v0.2.0, ora-graphql v0.2.0 ([`9c0812a`](https://github.com/tamasfe/ora/commit/9c0812a8005f496718406710c902c9de3346badc))
    - Release ora-scheduler v0.2.0, ora-client v0.2.0, ora-worker v0.2.0, ora-api v0.2.0, ora-store-memory v0.2.0, ora-store-sqlx v0.2.0, ora-test v0.2.0, ora v0.2.0, ora-graphql v0.2.0, safety bump 6 crates ([`3d59b5b`](https://github.com/tamasfe/ora/commit/3d59b5bcf244b6abbbda7e1feff30cb7931dc03f))
    - Handle concurrent workers ([`933860b`](https://github.com/tamasfe/ora/commit/933860bc82503d990938ad1925846eb0eecb0ee5))
</details>

<csr-unknown>
renamed Worker -> Handlerrenamed WorkerPool -> Worker<csr-unknown/>

## v0.1.0 (2023-08-11)

<csr-id-d5cca440df67e94bb0cc18f8572518459d4264f1/>
<csr-id-987061ed68939e994d097fb6c353921cbc353416/>
<csr-id-f40379b2fb458d0ef8d9ae5790c08564404f87fd/>

### Chore

 - <csr-id-d5cca440df67e94bb0cc18f8572518459d4264f1/> crate versions and changelog
 - <csr-id-987061ed68939e994d097fb6c353921cbc353416/> crate descriptions

### Chore

 - <csr-id-f40379b2fb458d0ef8d9ae5790c08564404f87fd/> changelog

### New Features

 - <csr-id-07c38305ea1c0ea48537aaac204698287bc44875/> initial implementation

### Commit Statistics

<csr-read-only-do-not-edit/>

 - 6 commits contributed to the release.
 - 4 commits were understood as [conventional](https://www.conventionalcommits.org).
 - 0 issues like '(#ID)' were seen in commit messages

### Commit Details

<csr-read-only-do-not-edit/>

<details><summary>view details</summary>

 * **Uncategorized**
    - Release ora-graphql v0.1.0 ([`0a5ecf9`](https://github.com/tamasfe/ora/commit/0a5ecf9815cd77d898f6568d43e5ebf989d24b9b))
    - Changelog ([`f40379b`](https://github.com/tamasfe/ora/commit/f40379b2fb458d0ef8d9ae5790c08564404f87fd))
    - Release ora-graphql v0.1.0 ([`0a1ed8c`](https://github.com/tamasfe/ora/commit/0a1ed8c1d8a1ebe0b360a87105ffe5185073d6c2))
    - Crate versions and changelog ([`d5cca44`](https://github.com/tamasfe/ora/commit/d5cca440df67e94bb0cc18f8572518459d4264f1))
    - Crate descriptions ([`987061e`](https://github.com/tamasfe/ora/commit/987061ed68939e994d097fb6c353921cbc353416))
    - Initial implementation ([`07c3830`](https://github.com/tamasfe/ora/commit/07c38305ea1c0ea48537aaac204698287bc44875))
</details>

