# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## 0.4.0 (2024-04-09)

<csr-id-4ec05d470e7d49102328c54ece287d10e6cb1400/>

### Chore

 - <csr-id-4ec05d470e7d49102328c54ece287d10e6cb1400/> lints and fmt

### New Features

 - <csr-id-e43b1a6d8c0eb6be0e1b15273bb129723c6c6b02/> do not panic on invalid JSON
 - <csr-id-2cc5958cb9bf2d7bcb9646effe2a2ef89a1520e3/> fail tasks of inactive workers

### Commit Statistics

<csr-read-only-do-not-edit/>

 - 5 commits contributed to the release over the course of 104 calendar days.
 - 131 days passed between releases.
 - 3 commits were understood as [conventional](https://www.conventionalcommits.org).
 - 0 issues like '(#ID)' were seen in commit messages

### Commit Details

<csr-read-only-do-not-edit/>

<details><summary>view details</summary>

 * **Uncategorized**
    - Release ora-timer v0.1.2, ora-util v0.1.1, ora-worker v0.4.1, ora-store-sqlx v0.4.0, ora-graphql v0.5.0, safety bump ora v0.5.0 ([`8ce026e`](https://github.com/tamasfe/ora/commit/8ce026ec3b761fd37d85a7bc373aa932d86771ea))
    - Do not panic on invalid JSON ([`e43b1a6`](https://github.com/tamasfe/ora/commit/e43b1a6d8c0eb6be0e1b15273bb129723c6c6b02))
    - Lints and fmt ([`4ec05d4`](https://github.com/tamasfe/ora/commit/4ec05d470e7d49102328c54ece287d10e6cb1400))
    - Fail tasks of inactive workers ([`2cc5958`](https://github.com/tamasfe/ora/commit/2cc5958cb9bf2d7bcb9646effe2a2ef89a1520e3))
    - Release ora-worker v0.4.1 ([`35a4c53`](https://github.com/tamasfe/ora/commit/35a4c53c854af0502b830c5cd9bb48a4f511ac8d))
</details>

## 0.3.2 (2023-11-29)

<csr-id-83cfe826e8b543bdeead54c0f9500706fbb72a7b/>
<csr-id-b39e3114c54d3ab9383d0401255c1f8fa8671d43/>

### Chore

 - <csr-id-83cfe826e8b543bdeead54c0f9500706fbb72a7b/> versions
 - <csr-id-b39e3114c54d3ab9383d0401255c1f8fa8671d43/> cleanup

### New Features

 - <csr-id-d6279daf534d793811232a2b8e765e8cd520bff2/> task api enhancements and worker registry

### Commit Statistics

<csr-read-only-do-not-edit/>

 - 5 commits contributed to the release over the course of 1 calendar day.
 - 54 days passed between releases.
 - 3 commits were understood as [conventional](https://www.conventionalcommits.org).
 - 0 issues like '(#ID)' were seen in commit messages

### Commit Details

<csr-read-only-do-not-edit/>

<details><summary>view details</summary>

 * **Uncategorized**
    - Release ora-macros v0.1.0, ora-timer v0.1.1, ora-scheduler v0.2.1, ora-store-memory v0.3.1, ora-store-sqlx v0.3.2, ora-test v0.3.1, ora v0.4.0 ([`6298b95`](https://github.com/tamasfe/ora/commit/6298b9553fae408543e9c028d32631bc5dc8641f))
    - Release ora-common v0.1.2, ora-worker v0.4.0, ora-api v0.3.2, ora-macros v0.1.0, ora-timer v0.1.1, ora-scheduler v0.2.1, ora-store-memory v0.3.1, ora-store-sqlx v0.3.2, ora-test v0.3.1, ora v0.4.0 ([`20021a7`](https://github.com/tamasfe/ora/commit/20021a756ac91b3b4503d8f449cb2f000a31e40e))
    - Versions ([`83cfe82`](https://github.com/tamasfe/ora/commit/83cfe826e8b543bdeead54c0f9500706fbb72a7b))
    - Task api enhancements and worker registry ([`d6279da`](https://github.com/tamasfe/ora/commit/d6279daf534d793811232a2b8e765e8cd520bff2))
    - Cleanup ([`b39e311`](https://github.com/tamasfe/ora/commit/b39e3114c54d3ab9383d0401255c1f8fa8671d43))
</details>

## 0.3.1 (2023-10-06)

### New Features

 - <csr-id-79308e2708ed34ac7f05eb62e43eca0197c25398/> better schedule cancellation
   Schedules and their tasks are cancelled in order and separate transactions
   to make sure the scheduler is aware of the cancelled
   schedule and doesn't spawn a new task.
   The separate transaction also helps with cancellation
   of new tasks that were spawned during the operation.

### Bug Fixes

 - <csr-id-ee05b5d999e5d0c047c62dfe4553ba6584153481/> fixed schedule cancellation

### Commit Statistics

<csr-read-only-do-not-edit/>

 - 3 commits contributed to the release.
 - 18 days passed between releases.
 - 2 commits were understood as [conventional](https://www.conventionalcommits.org).
 - 0 issues like '(#ID)' were seen in commit messages

### Commit Details

<csr-read-only-do-not-edit/>

<details><summary>view details</summary>

 * **Uncategorized**
    - Release ora-client v0.2.1, ora-api v0.3.1, ora-store-sqlx v0.3.1, ora v0.3.1 ([`65c0b4f`](https://github.com/tamasfe/ora/commit/65c0b4f771df465e27ae27555fd8aa7c0f4b5126))
    - Better schedule cancellation ([`79308e2`](https://github.com/tamasfe/ora/commit/79308e2708ed34ac7f05eb62e43eca0197c25398))
    - Fixed schedule cancellation ([`ee05b5d`](https://github.com/tamasfe/ora/commit/ee05b5d999e5d0c047c62dfe4553ba6584153481))
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
 - 2 days passed between releases.
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

## v0.2.1 (2023-09-16)

### Bug Fixes

 - <csr-id-a1d9cbd8bc638f2fcb00fb03d11a4a0f50a03f05/> schedule worker selector query

### Commit Statistics

<csr-read-only-do-not-edit/>

 - 2 commits contributed to the release.
 - 1 commit was understood as [conventional](https://www.conventionalcommits.org).
 - 0 issues like '(#ID)' were seen in commit messages

### Commit Details

<csr-read-only-do-not-edit/>

<details><summary>view details</summary>

 * **Uncategorized**
    - Release ora-common v0.1.1, ora-api v0.2.1, ora-store-sqlx v0.2.1 ([`5a2e17a`](https://github.com/tamasfe/ora/commit/5a2e17a80948cbebb219861a9a0faed84b50b4e3))
    - Schedule worker selector query ([`a1d9cbd`](https://github.com/tamasfe/ora/commit/a1d9cbd8bc638f2fcb00fb03d11a4a0f50a03f05))
</details>

## v0.2.0 (2023-09-15)

### New Features

 - <csr-id-933860bc82503d990938ad1925846eb0eecb0ee5/> handle concurrent workers
   - track workers per task to guarantee that at most one worker runs a task

### Bug Fixes

 - <csr-id-88b412116e59c8becf08414f2dd7f22e22fc6400/> better handling of timeouts
   - timeouts are applied even on scheduler restarts

### Commit Statistics

<csr-read-only-do-not-edit/>

 - 5 commits contributed to the release.
 - 34 days passed between releases.
 - 2 commits were understood as [conventional](https://www.conventionalcommits.org).
 - 0 issues like '(#ID)' were seen in commit messages

### Commit Details

<csr-read-only-do-not-edit/>

<details><summary>view details</summary>

 * **Uncategorized**
    - Release ora-store-sqlx v0.2.0, ora-test v0.2.0, ora v0.2.0, ora-graphql v0.2.0 ([`bc6f359`](https://github.com/tamasfe/ora/commit/bc6f359b246ce237690c05018afa07147731ee71))
    - Release ora-worker v0.2.1, ora-api v0.2.0, ora-store-memory v0.2.0, ora-store-sqlx v0.2.0, ora-test v0.2.0, ora v0.2.0, ora-graphql v0.2.0 ([`9c0812a`](https://github.com/tamasfe/ora/commit/9c0812a8005f496718406710c902c9de3346badc))
    - Release ora-scheduler v0.2.0, ora-client v0.2.0, ora-worker v0.2.0, ora-api v0.2.0, ora-store-memory v0.2.0, ora-store-sqlx v0.2.0, ora-test v0.2.0, ora v0.2.0, ora-graphql v0.2.0, safety bump 6 crates ([`3d59b5b`](https://github.com/tamasfe/ora/commit/3d59b5bcf244b6abbbda7e1feff30cb7931dc03f))
    - Better handling of timeouts ([`88b4121`](https://github.com/tamasfe/ora/commit/88b412116e59c8becf08414f2dd7f22e22fc6400))
    - Handle concurrent workers ([`933860b`](https://github.com/tamasfe/ora/commit/933860bc82503d990938ad1925846eb0eecb0ee5))
</details>

## v0.1.0 (2023-08-11)

<csr-id-987061ed68939e994d097fb6c353921cbc353416/>
<csr-id-d5cca440df67e94bb0cc18f8572518459d4264f1/>

### Chore

 - <csr-id-987061ed68939e994d097fb6c353921cbc353416/> crate descriptions

### Bug Fixes

 - <csr-id-8f03f918b44cfad310f0082e559fbc136d8f2170/> tokio features

### Chore

 - <csr-id-d5cca440df67e94bb0cc18f8572518459d4264f1/> crate versions and changelog

### New Features

 - <csr-id-07c38305ea1c0ea48537aaac204698287bc44875/> initial implementation

### Commit Statistics

<csr-read-only-do-not-edit/>

 - 10 commits contributed to the release.
 - 4 commits were understood as [conventional](https://www.conventionalcommits.org).
 - 0 issues like '(#ID)' were seen in commit messages

### Commit Details

<csr-read-only-do-not-edit/>

<details><summary>view details</summary>

 * **Uncategorized**
    - Release ora-store-sqlx v0.1.0, ora-test v0.1.0, ora v0.1.0 ([`709c80f`](https://github.com/tamasfe/ora/commit/709c80f3ab329c06af06b1efaa0ed39f59a3799a))
    - Release ora-store-memory v0.1.0, ora-store-sqlx v0.1.0, ora-test v0.1.0, ora v0.1.0 ([`9ac873b`](https://github.com/tamasfe/ora/commit/9ac873b7344a156234c49528d86b3c9ec0cb57b5))
    - Release ora-scheduler v0.1.0, ora-store-memory v0.1.0, ora-store-sqlx v0.1.0, ora-test v0.1.0, ora v0.1.0 ([`125e189`](https://github.com/tamasfe/ora/commit/125e1895e7c894c7c16f8eec01615fff19d7f421))
    - Release ora-util v0.1.0, ora-scheduler v0.1.0, ora-store-memory v0.1.0, ora-store-sqlx v0.1.0, ora-test v0.1.0, ora v0.1.0 ([`8fb9ee9`](https://github.com/tamasfe/ora/commit/8fb9ee956a23e1b243ea2bac14dc80cea7b2b5d9))
    - Release ora-timer v0.1.0, ora-util v0.1.0, ora-scheduler v0.1.0, ora-store-memory v0.1.0, ora-store-sqlx v0.1.0, ora-test v0.1.0, ora v0.1.0 ([`a2628e0`](https://github.com/tamasfe/ora/commit/a2628e02a6466893cd5e06b2973a46c301c7438b))
    - Tokio features ([`8f03f91`](https://github.com/tamasfe/ora/commit/8f03f918b44cfad310f0082e559fbc136d8f2170))
    - Release ora-common v0.1.0, ora-client v0.1.0, ora-worker v0.1.0, ora-api v0.1.0, ora-timer v0.1.0, ora-util v0.1.0, ora-scheduler v0.1.0, ora-store-memory v0.1.0, ora-store-sqlx v0.1.0, ora-test v0.1.0, ora v0.1.0 ([`cab6a7b`](https://github.com/tamasfe/ora/commit/cab6a7b16d23cb8a28d98e140d6fe5fdc4814c89))
    - Crate versions and changelog ([`d5cca44`](https://github.com/tamasfe/ora/commit/d5cca440df67e94bb0cc18f8572518459d4264f1))
    - Crate descriptions ([`987061e`](https://github.com/tamasfe/ora/commit/987061ed68939e994d097fb6c353921cbc353416))
    - Initial implementation ([`07c3830`](https://github.com/tamasfe/ora/commit/07c38305ea1c0ea48537aaac204698287bc44875))
</details>

