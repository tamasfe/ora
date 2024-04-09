# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## 0.4.1 (2024-04-09)

<csr-id-4ec05d470e7d49102328c54ece287d10e6cb1400/>

### Chore

 - <csr-id-4ec05d470e7d49102328c54ece287d10e6cb1400/> lints and fmt

### New Features

 - <csr-id-8bc24b75c68dfb905d514973904175f0958d2aa9/> expose worker fields

### Commit Statistics

<csr-read-only-do-not-edit/>

 - 2 commits contributed to the release.
 - 104 days passed between releases.
 - 1 commit was understood as [conventional](https://www.conventionalcommits.org).
 - 0 issues like '(#ID)' were seen in commit messages

### Commit Details

<csr-read-only-do-not-edit/>

<details><summary>view details</summary>

 * **Uncategorized**
    - Release ora-timer v0.1.2, ora-util v0.1.1, ora-worker v0.4.1, ora-store-sqlx v0.4.0, ora-graphql v0.5.0, safety bump ora v0.5.0 ([`8ce026e`](https://github.com/tamasfe/ora/commit/8ce026ec3b761fd37d85a7bc373aa932d86771ea))
    - Lints and fmt ([`4ec05d4`](https://github.com/tamasfe/ora/commit/4ec05d470e7d49102328c54ece287d10e6cb1400))
</details>

## 0.4.0 (2023-11-29)

<csr-id-83cfe826e8b543bdeead54c0f9500706fbb72a7b/>
<csr-id-b39e3114c54d3ab9383d0401255c1f8fa8671d43/>

### Chore

 - <csr-id-83cfe826e8b543bdeead54c0f9500706fbb72a7b/> versions
 - <csr-id-b39e3114c54d3ab9383d0401255c1f8fa8671d43/> cleanup

### New Features

 - <csr-id-d6279daf534d793811232a2b8e765e8cd520bff2/> task api enhancements and worker registry

### Commit Statistics

<csr-read-only-do-not-edit/>

 - 4 commits contributed to the release over the course of 1 calendar day.
 - 72 days passed between releases.
 - 3 commits were understood as [conventional](https://www.conventionalcommits.org).
 - 0 issues like '(#ID)' were seen in commit messages

### Commit Details

<csr-read-only-do-not-edit/>

<details><summary>view details</summary>

 * **Uncategorized**
    - Release ora-common v0.1.2, ora-worker v0.4.0, ora-api v0.3.2, ora-macros v0.1.0, ora-timer v0.1.1, ora-scheduler v0.2.1, ora-store-memory v0.3.1, ora-store-sqlx v0.3.2, ora-test v0.3.1, ora v0.4.0 ([`20021a7`](https://github.com/tamasfe/ora/commit/20021a756ac91b3b4503d8f449cb2f000a31e40e))
    - Versions ([`83cfe82`](https://github.com/tamasfe/ora/commit/83cfe826e8b543bdeead54c0f9500706fbb72a7b))
    - Task api enhancements and worker registry ([`d6279da`](https://github.com/tamasfe/ora/commit/d6279daf534d793811232a2b8e765e8cd520bff2))
    - Cleanup ([`b39e311`](https://github.com/tamasfe/ora/commit/b39e3114c54d3ab9383d0401255c1f8fa8671d43))
</details>

## 0.3.0 (2023-09-18)

<csr-id-fabc6d25ea8ef8706e44e8794b80af3943518942/>

### Chore

 - <csr-id-fabc6d25ea8ef8706e44e8794b80af3943518942/> changelog

### New Features

 - <csr-id-f425761ec5f5cfa47490435edb39f4ceb1679972/> support graceful shutdown

### Commit Statistics

<csr-read-only-do-not-edit/>

 - 3 commits contributed to the release.
 - 3 days passed between releases.
 - 2 commits were understood as [conventional](https://www.conventionalcommits.org).
 - 0 issues like '(#ID)' were seen in commit messages

### Commit Details

<csr-read-only-do-not-edit/>

<details><summary>view details</summary>

 * **Uncategorized**
    - Release ora-worker v0.3.0, ora-api v0.3.0, ora-store-memory v0.3.0, ora-store-sqlx v0.3.0, ora-test v0.3.0, ora v0.3.0, ora-graphql v0.3.0, safety bump 5 crates ([`387ea7f`](https://github.com/tamasfe/ora/commit/387ea7fc0da2bdd9894415228f5e60e2f9716478))
    - Changelog ([`fabc6d2`](https://github.com/tamasfe/ora/commit/fabc6d25ea8ef8706e44e8794b80af3943518942))
    - Support graceful shutdown ([`f425761`](https://github.com/tamasfe/ora/commit/f425761ec5f5cfa47490435edb39f4ceb1679972))
</details>

## v0.2.1 (2023-09-15)

### New Features

 - <csr-id-933860bc82503d990938ad1925846eb0eecb0ee5/> handle concurrent workers
   - track workers per task to guarantee that at most one worker runs a task

### Bug Fixes

 - <csr-id-96e79cdc9a1e497b99c5b0cc4e1ef1bc6cb41efe/> add missing uuid feature

### Commit Statistics

<csr-read-only-do-not-edit/>

 - 4 commits contributed to the release.
 - 35 days passed between releases.
 - 2 commits were understood as [conventional](https://www.conventionalcommits.org).
 - 0 issues like '(#ID)' were seen in commit messages

### Commit Details

<csr-read-only-do-not-edit/>

<details><summary>view details</summary>

 * **Uncategorized**
    - Release ora-worker v0.2.1, ora-api v0.2.0, ora-store-memory v0.2.0, ora-store-sqlx v0.2.0, ora-test v0.2.0, ora v0.2.0, ora-graphql v0.2.0 ([`9c0812a`](https://github.com/tamasfe/ora/commit/9c0812a8005f496718406710c902c9de3346badc))
    - Add missing uuid feature ([`96e79cd`](https://github.com/tamasfe/ora/commit/96e79cdc9a1e497b99c5b0cc4e1ef1bc6cb41efe))
    - Release ora-scheduler v0.2.0, ora-client v0.2.0, ora-worker v0.2.0, ora-api v0.2.0, ora-store-memory v0.2.0, ora-store-sqlx v0.2.0, ora-test v0.2.0, ora v0.2.0, ora-graphql v0.2.0, safety bump 6 crates ([`3d59b5b`](https://github.com/tamasfe/ora/commit/3d59b5bcf244b6abbbda7e1feff30cb7931dc03f))
    - Handle concurrent workers ([`933860b`](https://github.com/tamasfe/ora/commit/933860bc82503d990938ad1925846eb0eecb0ee5))
</details>

## v0.2.0 (2023-09-15)

### New Features

 - <csr-id-933860bc82503d990938ad1925846eb0eecb0ee5/> handle concurrent workers
   - track workers per task to guarantee that at most one worker runs a task

<csr-unknown>
<csr-unknown>
<csr-unknown>
<csr-unknown>
<csr-unknown>
<csr-unknown>
renamed Worker -> Handlerrenamed WorkerPool -> Worker<csr-unknown/>
<csr-unknown/>
<csr-unknown/>
<csr-unknown/>
<csr-unknown/>
<csr-unknown/>

## v0.1.0 (2023-08-11)

<csr-id-987061ed68939e994d097fb6c353921cbc353416/>
<csr-id-d5cca440df67e94bb0cc18f8572518459d4264f1/>

### Chore

 - <csr-id-987061ed68939e994d097fb6c353921cbc353416/> crate descriptions

### Chore

 - <csr-id-d5cca440df67e94bb0cc18f8572518459d4264f1/> crate versions and changelog

### New Features

 - <csr-id-07c38305ea1c0ea48537aaac204698287bc44875/> initial implementation

### Commit Statistics

<csr-read-only-do-not-edit/>

 - 4 commits contributed to the release.
 - 3 commits were understood as [conventional](https://www.conventionalcommits.org).
 - 0 issues like '(#ID)' were seen in commit messages

### Commit Details

<csr-read-only-do-not-edit/>

<details><summary>view details</summary>

 * **Uncategorized**
    - Release ora-common v0.1.0, ora-client v0.1.0, ora-worker v0.1.0, ora-api v0.1.0, ora-timer v0.1.0, ora-util v0.1.0, ora-scheduler v0.1.0, ora-store-memory v0.1.0, ora-store-sqlx v0.1.0, ora-test v0.1.0, ora v0.1.0 ([`cab6a7b`](https://github.com/tamasfe/ora/commit/cab6a7b16d23cb8a28d98e140d6fe5fdc4814c89))
    - Crate versions and changelog ([`d5cca44`](https://github.com/tamasfe/ora/commit/d5cca440df67e94bb0cc18f8572518459d4264f1))
    - Crate descriptions ([`987061e`](https://github.com/tamasfe/ora/commit/987061ed68939e994d097fb6c353921cbc353416))
    - Initial implementation ([`07c3830`](https://github.com/tamasfe/ora/commit/07c38305ea1c0ea48537aaac204698287bc44875))
</details>

