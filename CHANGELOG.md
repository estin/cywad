# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Changed
- split to crates, refactor
- refactor: ws client actor logic 

### Fixed
- mark result as failed on general engine error to prevent hangs
- scheduler logic
- update result item datetime on start processing 
- ws request timeout
- don't hang on worth devtools config response 

### Added
- frontend on yew introduced

## [0.1.2] - 2020-08-16

### Fixed
- webkit tests

### Changed
- webkit: update gtk to 0.9.1
- use serde with derive feature instead of serde_derive, fmt&clippy 
- prevent panic if step index out of bounds
- don't send Network.clearBrowserCache and Network.clearBrowserCookies

## [0.1.1] - 2020-01-14

### Fixed
- update `scheduled` attribute on any item state

### Changed
- Edition 2018: wip
- up to date actix/tokio/futures/bytes and others
- http server starts default workers count
- clippy and rustfmt fixes

## [0.1.0] - 2020-01-02

### Added

- First public release
