# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## v0.2.1 (2023-11-29)

<csr-id-b39e3114c54d3ab9383d0401255c1f8fa8671d43/>

### Chore

 - <csr-id-b39e3114c54d3ab9383d0401255c1f8fa8671d43/> cleanup

### New Features

 - <csr-id-f4abb814a193a13981e424963cbb62caa80b8707/> default timeout

### Commit Statistics

<csr-read-only-do-not-edit/>

 - 3 commits contributed to the release over the course of 1 calendar day.
 - 75 days passed between releases.
 - 2 commits were understood as [conventional](https://www.conventionalcommits.org).
 - 0 issues like '(#ID)' were seen in commit messages

### Commit Details

<csr-read-only-do-not-edit/>

<details><summary>view details</summary>

 * **Uncategorized**
    - Release ora-common v0.1.2, ora-worker v0.4.0, ora-api v0.3.2, ora-macros v0.1.0, ora-timer v0.1.1, ora-scheduler v0.2.1, ora-store-memory v0.3.1, ora-store-sqlx v0.3.2, ora-test v0.3.1, ora v0.4.0 ([`20021a7`](https://github.com/tamasfe/ora/commit/20021a756ac91b3b4503d8f449cb2f000a31e40e))
    - Default timeout ([`f4abb81`](https://github.com/tamasfe/ora/commit/f4abb814a193a13981e424963cbb62caa80b8707))
    - Cleanup ([`b39e311`](https://github.com/tamasfe/ora/commit/b39e3114c54d3ab9383d0401255c1f8fa8671d43))
</details>

## v0.2.0 (2023-09-15)

### Bug Fixes

 - <csr-id-88b412116e59c8becf08414f2dd7f22e22fc6400/> better handling of timeouts
   - timeouts are applied even on scheduler restarts

### Commit Statistics

<csr-read-only-do-not-edit/>

 - 2 commits contributed to the release.
 - 34 days passed between releases.
 - 1 commit was understood as [conventional](https://www.conventionalcommits.org).
 - 0 issues like '(#ID)' were seen in commit messages

### Commit Details

<csr-read-only-do-not-edit/>

<details><summary>view details</summary>

 * **Uncategorized**
    - Release ora-scheduler v0.2.0, ora-client v0.2.0, ora-worker v0.2.0, ora-api v0.2.0, ora-store-memory v0.2.0, ora-store-sqlx v0.2.0, ora-test v0.2.0, ora v0.2.0, ora-graphql v0.2.0, safety bump 6 crates ([`3d59b5b`](https://github.com/tamasfe/ora/commit/3d59b5bcf244b6abbbda7e1feff30cb7931dc03f))
    - Better handling of timeouts ([`88b4121`](https://github.com/tamasfe/ora/commit/88b412116e59c8becf08414f2dd7f22e22fc6400))
</details>

<csr-unknown>
timeout delays are properly calculated for past deadlines<csr-unknown/>

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

 - 8 commits contributed to the release.
 - 4 commits were understood as [conventional](https://www.conventionalcommits.org).
 - 0 issues like '(#ID)' were seen in commit messages

### Commit Details

<csr-read-only-do-not-edit/>

<details><summary>view details</summary>

 * **Uncategorized**
    - Release ora-scheduler v0.1.0, ora-store-memory v0.1.0, ora-store-sqlx v0.1.0, ora-test v0.1.0, ora v0.1.0 ([`125e189`](https://github.com/tamasfe/ora/commit/125e1895e7c894c7c16f8eec01615fff19d7f421))
    - Release ora-util v0.1.0, ora-scheduler v0.1.0, ora-store-memory v0.1.0, ora-store-sqlx v0.1.0, ora-test v0.1.0, ora v0.1.0 ([`8fb9ee9`](https://github.com/tamasfe/ora/commit/8fb9ee956a23e1b243ea2bac14dc80cea7b2b5d9))
    - Release ora-timer v0.1.0, ora-util v0.1.0, ora-scheduler v0.1.0, ora-store-memory v0.1.0, ora-store-sqlx v0.1.0, ora-test v0.1.0, ora v0.1.0 ([`a2628e0`](https://github.com/tamasfe/ora/commit/a2628e02a6466893cd5e06b2973a46c301c7438b))
    - Tokio features ([`8f03f91`](https://github.com/tamasfe/ora/commit/8f03f918b44cfad310f0082e559fbc136d8f2170))
    - Release ora-common v0.1.0, ora-client v0.1.0, ora-worker v0.1.0, ora-api v0.1.0, ora-timer v0.1.0, ora-util v0.1.0, ora-scheduler v0.1.0, ora-store-memory v0.1.0, ora-store-sqlx v0.1.0, ora-test v0.1.0, ora v0.1.0 ([`cab6a7b`](https://github.com/tamasfe/ora/commit/cab6a7b16d23cb8a28d98e140d6fe5fdc4814c89))
    - Crate versions and changelog ([`d5cca44`](https://github.com/tamasfe/ora/commit/d5cca440df67e94bb0cc18f8572518459d4264f1))
    - Crate descriptions ([`987061e`](https://github.com/tamasfe/ora/commit/987061ed68939e994d097fb6c353921cbc353416))
    - Initial implementation ([`07c3830`](https://github.com/tamasfe/ora/commit/07c38305ea1c0ea48537aaac204698287bc44875))
</details>

