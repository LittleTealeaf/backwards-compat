# Changelog

## [4.0.2](https://github.com/LittleTealeaf/backwards-compat/compare/v4.0.1...v4.0.2) (2026-11-27)


### Bug Fixes

* resolve broken intra-doc links and build docs in CI ([#76](https://github.com/LittleTealeaf/backwards-compat/issues/76)) ([153f909](https://github.com/LittleTealeaf/backwards-compat/commit/153f909b0be34ba5c31dc81b60fd1b86877aa784))

## [4.0.1](https://github.com/LittleTealeaf/backwards-compat/compare/v4.0.0...v4.0.1) (2026-11-27)


### Performance Improvements

* optimize codegen size, LLVM IR, DAG resolution, and modularize internals ([#74](https://github.com/LittleTealeaf/backwards-compat/issues/74)) ([7a79459](https://github.com/LittleTealeaf/backwards-compat/commit/7a7945900440e393127f0eb82212cd3afe66d783))

## [4.0.0](https://github.com/LittleTealeaf/backwards-compat/compare/v3.1.0...v4.0.0) (2026-11-26)


### ⚠ BREAKING CHANGES

* support full serialization formats, external tagging, and homogeneous version tags ([#59](https://github.com/LittleTealeaf/backwards-compat/issues/59))

### Features

* support full serialization formats, external tagging, and homogeneous version tags ([#59](https://github.com/LittleTealeaf/backwards-compat/issues/59)) ([5eb7fa8](https://github.com/LittleTealeaf/backwards-compat/commit/5eb7fa82caa70f57058b4f1192e1e2e713c49e91))

## [3.1.0](https://github.com/LittleTealeaf/backwards-compat/compare/v3.0.1...v3.1.0) (2026-11-26)


### Features

* add no_std, perf optimizations, runtime version serialization, schemars, and benchmarks ([#57](https://github.com/LittleTealeaf/backwards-compat/issues/57)) ([8601d6d](https://github.com/LittleTealeaf/backwards-compat/commit/8601d6d4555afeb0b927bd12f122751720f2075d))

## [3.0.1](https://github.com/LittleTealeaf/backwards-compat/compare/v3.0.0...v3.0.1) (2026-11-25)


### Bug Fixes

* Fix supporting RON struct syntax ([#55](https://github.com/LittleTealeaf/backwards-compat/issues/55)) ([dc9739e](https://github.com/LittleTealeaf/backwards-compat/commit/dc9739ee1c90374489f690af3661a1ba44021cc1))

## [3.0.0](https://github.com/LittleTealeaf/backwards-compat/compare/v2.2.0...v3.0.0) (2026-11-25)


### ⚠ BREAKING CHANGES

* field-level versioning, streaming deserialization, and unified via macro ([#53](https://github.com/LittleTealeaf/backwards-compat/issues/53))

### Features

* field-level versioning, streaming deserialization, and unified via macro ([#53](https://github.com/LittleTealeaf/backwards-compat/issues/53)) ([e046c0b](https://github.com/LittleTealeaf/backwards-compat/commit/e046c0b85c149be01aa984f857b33b9c38f7e3e1))

## [2.2.0](https://github.com/LittleTealeaf/backwards-compat/compare/v2.1.0...v2.2.0) (2026-11-23)


### Features

* support type targets in =&gt; transitions and bidirectional duplicate-type inheritance ([#52](https://github.com/LittleTealeaf/backwards-compat/issues/52)) ([34f9cd8](https://github.com/LittleTealeaf/backwards-compat/commit/34f9cd866dbb86b2302925c2b24f25162b7247d3))


### Bug Fixes

* respan generated shadow and trait items to call_site ([#50](https://github.com/LittleTealeaf/backwards-compat/issues/50)) ([cb930e4](https://github.com/LittleTealeaf/backwards-compat/commit/cb930e405124cda09b9465c1b7ec6d335fc86d67))

## [2.1.0](https://github.com/LittleTealeaf/backwards-compat/compare/v2.0.1...v2.1.0) (2026-11-23)


### Features

* add #[backwards_compat_via] proxy redirection macro ([#48](https://github.com/LittleTealeaf/backwards-compat/issues/48)) ([aa6bb02](https://github.com/LittleTealeaf/backwards-compat/commit/aa6bb021e60e4052b28e2768fd9ae69cf5e33869))

## [2.0.1](https://github.com/LittleTealeaf/backwards-compat/compare/v2.0.0...v2.0.1) (2026-11-22)


### Bug Fixes

* mark single-field tuple shadow wire structs transparent ([#46](https://github.com/LittleTealeaf/backwards-compat/issues/46)) ([f662283](https://github.com/LittleTealeaf/backwards-compat/commit/f662283d12ff6b3bf6d16a80e6e2cf4232543665))

## [2.0.0](https://github.com/LittleTealeaf/backwards-compat/compare/v1.3.0...v2.0.0) (2026-11-22)


### ⚠ BREAKING CHANGES

* Version 2.0.0 ([#43](https://github.com/LittleTealeaf/backwards-compat/issues/43))

### Features

* Version 2.0.0 ([#43](https://github.com/LittleTealeaf/backwards-compat/issues/43)) ([bf97240](https://github.com/LittleTealeaf/backwards-compat/commit/bf9724065dd95d12f1f0c8b646ea96bafb1656cf))

## [1.3.0](https://github.com/LittleTealeaf/backwards-compat/compare/v1.2.0...v1.3.0) (2026-11-13)


### Features

* support pre-versioned compatability ([#40](https://github.com/LittleTealeaf/backwards-compat/issues/40)) ([c101087](https://github.com/LittleTealeaf/backwards-compat/commit/c1010877fd04c5d56ebceb3b52c6810cc414ba8f))

## [1.2.0](https://github.com/LittleTealeaf/backwards-compat/compare/v1.1.0...v1.2.0) (2026-10-04)


### Features

* support multiple schema versions sharing the same type ([#27](https://github.com/LittleTealeaf/backwards-compat/issues/27)) ([742f0a7](https://github.com/LittleTealeaf/backwards-compat/commit/742f0a70ed906923d1834cfee8f6f989675fa6bc))

## [1.1.0](https://github.com/LittleTealeaf/backwards-compat/compare/v1.0.0...v1.1.0) (2026-10-03)


### Features

* zero-copy serialization, generics/lifetimes, adjacent tagging, and expanded docs ([#24](https://github.com/LittleTealeaf/backwards-compat/issues/24)) ([f3313b6](https://github.com/LittleTealeaf/backwards-compat/commit/f3313b62b9e671588cadad150ae956ca00d9550d))

## [1.0.0](https://github.com/LittleTealeaf/backwards-compat/compare/v0.6.0...v1.0.0) (2026-10-03)


### ⚠ BREAKING CHANGES

* remove backwards_compat_decl and enhance attribute ergonomics ([#22](https://github.com/LittleTealeaf/backwards-compat/issues/22))

### Features

* remove backwards_compat_decl and enhance attribute ergonomics ([#22](https://github.com/LittleTealeaf/backwards-compat/issues/22)) ([ce79225](https://github.com/LittleTealeaf/backwards-compat/commit/ce79225e645ad4089f8c0ede7c747904c684d012))

## [0.6.0](https://github.com/LittleTealeaf/backwards-compat/compare/v0.5.0...v0.6.0) (2026-10-03)


### Features

* implement #[backwards_compat] attribute macro with implicit wire structs ([#19](https://github.com/LittleTealeaf/backwards-compat/issues/19)) ([96739e6](https://github.com/LittleTealeaf/backwards-compat/commit/96739e62e6050c9b18ab7ee6fa9dc0ffe70544b7))

## [0.5.0](https://github.com/LittleTealeaf/backwards-compat/compare/v0.4.0...v0.5.0) (2026-10-02)


### Features

* Remove blanket serialize requirements ([#17](https://github.com/LittleTealeaf/backwards-compat/issues/17)) ([0cfd722](https://github.com/LittleTealeaf/backwards-compat/commit/0cfd722fbd12607ba707c32bb0d91dd57fc718c4))

## [0.4.0](https://github.com/LittleTealeaf/backwards-compat/compare/v0.3.0...v0.4.0) (2026-09-27)


### Features

* automatically generate From and TryFrom implementations for historical versions ([#14](https://github.com/LittleTealeaf/backwards-compat/issues/14)) ([9f15e1c](https://github.com/LittleTealeaf/backwards-compat/commit/9f15e1cc83841dcb928fe32956cc8a7637fc93fa))


## [0.3.0](https://github.com/LittleTealeaf/backwards-compat/compare/v0.2.0...v0.3.0) (2026-09-07)

### Documentation

* Add comprehensive README with usage patterns and architectural overview
* Set up GitHub Pages documentation deployment and add docs links to crates

### Build & CI

* Gate release workflow on successful CI completion
* Configure unified single release tagging (`vX.Y.Z`)

## [0.2.0](https://github.com/LittleTealeaf/backwards-compat/releases/tag/v0.2.0) (2026-09-07)


### Features

* Implemnet Version 1 ([0a2dfc3](https://github.com/LittleTealeaf/backwards-compat/commit/0a2dfc3a498ddb27f0e8fae82b7308c7e3fb709d))
